//! `RemoteHid`: the [`HidTransport`] over a socket to a remote device, for the
//! `remote` e2e tier (`--features e2e`, `TMP_E2E_REMOTE=<addr>`). It speaks the unit's
//! HID protocol over a socket, with no USB in between; this transport is the app's side of
//! that link and knows nothing about what serves the other end.
//!
//! WIRE. A byte stream (TCP, or a Unix stream socket) of fixed 64-byte records, the same
//! bytes the hidraw transport moves:
//! - OUT: `0x00` (the report ID of the TMP's unnumbered reports) then the 63-byte frame of
//!   `proto::make_chunked_envelopes`, exactly what `hid.rs`'s Linux `write_frame` writes.
//!   The far end strips that byte, as the host's HID stack does, so the device side reads
//!   the 63-byte frame the unit receives.
//! - IN: the unit's 64-byte input report, led by its own `0x00`, pushed RAW, as the
//!   macOS and Linux transports push it (see the Linux `imp` note on inbound framing).
//!
//! Fixed records are the whole framing: a stream read may stop mid-record at a pump's
//! deadline, so the tail is kept for the next pump rather than dropped.
//!
//! SEIZE. One handle per remote address at a time IN THIS PROCESS, refused with
//! [`DeviceError::SeizeHeld`] like a second seize of the unit, and every handle is counted
//! in `LIVE_HANDLES`, so [`super::wait_released`] waits for a closing remote handle as it does
//! for a real one. Nothing refuses across processes: the far end serves the newest
//! connection, so a drop-then-reopen never races the old link's close.
//! A refused connect is [`DeviceError::NotFound`]: no remote device is listening. Behind a port
//! forwarder an absent remote can accept and then close instead; that surfaces as a closed
//! link on the first exchange, so the handshake fails with [`DeviceError::Other`].
//!
//! PUMPS burn their budget like `CFRunLoopRunInMode`, and the eager pump reuses
//! [`fold_frame_open`], so the session layer sees the same timing shape as on hardware.

use std::collections::HashSet;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::{fold_frame_open, HidTransport, EAGER_QUIET_MS, EAGER_SLICE_MS, LIVE_HANDLES};
use crate::error::DeviceError;
use crate::proto;

/// How long one connect attempt to a TCP address may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long one send may block on a peer that has stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// One record on the remote link, both directions.
pub(crate) const RECORD_LEN: usize = 64;

/// Where the remote device listens: `tcp:<host>:<port>` or `unix:<path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAddr {
    Tcp(String),
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

impl RemoteAddr {
    /// Parse a `TMP_E2E_REMOTE` value. The scheme is required, so a bare port or path is an
    /// error rather than a guess.
    pub fn parse(s: &str) -> Result<RemoteAddr, String> {
        let s = s.trim();
        if let Some(hp) = s.strip_prefix("tcp:") {
            let ok = hp
                .rsplit_once(':')
                .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok_and(|p| p > 0));
            return if ok {
                Ok(RemoteAddr::Tcp(hp.to_string()))
            } else {
                Err(format!("remote address {s:?}: expected tcp:<host>:<port>"))
            };
        }
        if let Some(path) = s.strip_prefix("unix:") {
            #[cfg(unix)]
            return if path.is_empty() {
                Err(format!("remote address {s:?}: expected unix:<path>"))
            } else {
                Ok(RemoteAddr::Unix(path.into()))
            };
            #[cfg(not(unix))]
            return Err(format!(
                "remote address {s:?}: Unix sockets need a Unix host ({path})"
            ));
        }
        Err(format!(
            "remote address {s:?}: expected tcp:<host>:<port> or unix:<path>"
        ))
    }

    fn key(&self) -> String {
        self.to_string()
    }
}

impl std::fmt::Display for RemoteAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteAddr::Tcp(hp) => write!(f, "tcp:{hp}"),
            #[cfg(unix)]
            RemoteAddr::Unix(p) => write!(f, "unix:{}", p.display()),
        }
    }
}

/// The remote addresses with a live handle in this process (see SEIZE above).
static HELD: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn held() -> std::sync::MutexGuard<'static, Option<HashSet<String>>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A connected byte stream to a remote address; `remote_audio` opens its job links with it too.
pub(crate) enum Stream {
    Tcp(std::net::TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl Stream {
    pub(crate) fn connect(addr: &RemoteAddr) -> std::io::Result<Stream> {
        match addr {
            RemoteAddr::Tcp(hp) => {
                // Bounded: an address that drops the connect fails in CONNECT_TIMEOUT per
                // resolved address, not after the OS's own minutes-long timeout.
                let mut last = None;
                for sa in std::net::ToSocketAddrs::to_socket_addrs(hp.as_str())? {
                    match std::net::TcpStream::connect_timeout(&sa, CONNECT_TIMEOUT) {
                        Ok(s) => {
                            // Records are small and latency-bound, like interrupt reports.
                            let _ = s.set_nodelay(true);
                            return Ok(Stream::Tcp(s));
                        }
                        Err(e) => last = Some(e),
                    }
                }
                Err(last.unwrap_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no address resolved")
                }))
            }
            #[cfg(unix)]
            RemoteAddr::Unix(p) => std::os::unix::net::UnixStream::connect(p).map(Stream::Unix),
        }
    }
    pub(crate) fn shutdown(&self) {
        match self {
            Stream::Tcp(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            #[cfg(unix)]
            Stream::Unix(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
    pub(crate) fn set_read_timeout(&self, d: Duration) -> std::io::Result<()> {
        // A zero timeout is an error on both socket types: clamp to the smallest wait.
        let d = Some(d.max(Duration::from_millis(1)));
        match self {
            Stream::Tcp(s) => s.set_read_timeout(d),
            #[cfg(unix)]
            Stream::Unix(s) => s.set_read_timeout(d),
        }
    }
    pub(crate) fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => (&*s).read(buf),
            #[cfg(unix)]
            Stream::Unix(s) => (&*s).read(buf),
        }
    }
    pub(crate) fn set_write_timeout(&self, d: Duration) -> std::io::Result<()> {
        let d = Some(d.max(Duration::from_millis(1)));
        match self {
            Stream::Tcp(s) => s.set_write_timeout(d),
            #[cfg(unix)]
            Stream::Unix(s) => s.set_write_timeout(d),
        }
    }
    pub(crate) fn write(&self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => (&*s).write(buf),
            #[cfg(unix)]
            Stream::Unix(s) => (&*s).write(buf),
        }
    }
    pub(crate) fn write_all(&self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Tcp(s) => (&*s).write_all(buf),
            #[cfg(unix)]
            Stream::Unix(s) => (&*s).write_all(buf),
        }
    }
}

/// An open link to one remote device. Dropping it closes the link and releases the seize.
pub struct RemoteHid {
    stream: Stream,
    key: String,
    /// Bytes of a record the last pump's deadline cut short.
    partial: std::cell::RefCell<Vec<u8>>,
    /// The remote device closed the link: later sends fail, pumps return nothing.
    closed: std::cell::Cell<bool>,
}

impl RemoteHid {
    /// Connect to the remote device at `addr`, taking its seize.
    pub fn open(addr: &RemoteAddr) -> Result<RemoteHid, DeviceError> {
        let key = addr.key();
        if !held().get_or_insert_with(HashSet::new).insert(key.clone()) {
            return Err(DeviceError::SeizeHeld(format!(
                "the remote device at {key} is already held by another session in this process"
            )));
        }
        let release = |e: DeviceError| {
            if let Some(set) = held().as_mut() {
                set.remove(&key);
            }
            e
        };
        let stream = Stream::connect(addr).map_err(|e| {
            release(DeviceError::NotFound(format!(
                "no remote device listening at {key} ({e})"
            )))
        })?;
        // Bounded writes: a peer that stops reading fails the send (and closes the link)
        // instead of holding the device-op lock forever.
        let _ = stream.set_write_timeout(WRITE_TIMEOUT);
        LIVE_HANDLES.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteHid {
            stream,
            key,
            partial: std::cell::RefCell::new(Vec::new()),
            closed: std::cell::Cell::new(false),
        })
    }

    fn send_body(&self, body: &[u8]) -> Result<(), String> {
        if self.closed.get() {
            return Err(format!(
                "remote write: the remote device at {} closed the link",
                self.key
            ));
        }
        let frames = proto::make_chunked_envelopes(body);
        let mut out = Vec::with_capacity(frames.len() * RECORD_LEN);
        for f in &frames {
            out.push(0x00);
            out.extend_from_slice(f);
        }
        self.stream.write_all(&out).map_err(|e| {
            self.closed.set(true);
            format!("remote write: {e}")
        })
    }

    /// Collect records for up to `ms`, then return; keeps a cut-short record for later.
    fn pump_into(&self, ms: u64, out: &mut Vec<Vec<u8>>) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        let mut partial = self.partial.borrow_mut();
        let mut buf = [0u8; RECORD_LEN * 16];
        while !self.closed.get() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || self.stream.set_read_timeout(left).is_err() {
                return;
            }
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    // The remote device went away: the counterpart of hidraw's POLLHUP.
                    self.closed.set(true);
                    return;
                }
                Ok(n) => {
                    partial.extend_from_slice(&buf[..n]);
                    let whole = partial.len() / RECORD_LEN * RECORD_LEN;
                    out.extend(partial[..whole].chunks(RECORD_LEN).map(<[u8]>::to_vec));
                    partial.drain(..whole);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return,
                Err(_) => {
                    self.closed.set(true);
                    return;
                }
            }
        }
    }

    fn pump_for(&self, ms: u64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        self.pump_into(ms, &mut out);
        out
    }

    /// The eager pump: the same policy as the hardware transports, `max_ms` the hard cap.
    fn pump_eager(&self, max_ms: u64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let (mut elapsed, mut quiet, mut scanned, mut open) = (0u64, 0u64, 0usize, false);
        while elapsed < max_ms {
            let slice = EAGER_SLICE_MS.min(max_ms - elapsed);
            self.pump_into(slice, &mut out);
            elapsed += slice;
            if out.len() > scanned {
                open = fold_frame_open(&out[scanned..], open);
                scanned = out.len();
                quiet = 0;
            } else {
                quiet += slice;
            }
            if scanned > 0 && !open && quiet >= EAGER_QUIET_MS {
                break;
            }
        }
        out
    }
}

impl Drop for RemoteHid {
    fn drop(&mut self) {
        // Close the link BEFORE releasing the seize, as the hardware transports close the
        // device before their handle count drops: the next open must find the old link gone.
        self.stream.shutdown();
        if let Some(set) = held().as_mut() {
            set.remove(&self.key);
        }
        LIVE_HANDLES.fetch_sub(1, Ordering::SeqCst);
    }
}

impl HidTransport for RemoteHid {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.send_body(body)
    }

    fn transact(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.send_body(body)?;
        Ok(self.pump_for(pump_ms))
    }

    // Chunking is inside `send_body`, as on the hardware transports.
    fn transact_chunked(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.transact(body, pump_ms)
    }

    fn pump(&self, pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.pump_for(pump_ms))
    }

    fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.send_body(body)?;
        Ok(self.pump_eager(max_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A remote stand-in on an ephemeral port: hands the accepted stream to `peer`.
    fn listen(peer: impl FnOnce(std::net::TcpStream) + Send + 'static) -> RemoteAddr {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = RemoteAddr::Tcp(l.local_addr().expect("addr").to_string());
        std::thread::spawn(move || {
            if let Ok((s, _)) = l.accept() {
                peer(s);
            }
        });
        addr
    }

    fn report(head: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_LEN];
        r[..head.len()].copy_from_slice(head);
        r
    }

    #[test]
    fn addresses_need_a_scheme() {
        assert_eq!(
            RemoteAddr::parse("tcp:127.0.0.1:9600"),
            Ok(RemoteAddr::Tcp("127.0.0.1:9600".into()))
        );
        #[cfg(unix)]
        assert_eq!(
            RemoteAddr::parse(" unix:/tmp/remote.sock "),
            Ok(RemoteAddr::Unix("/tmp/remote.sock".into()))
        );
        for bad in [
            "9600",
            "/tmp/remote.sock",
            "tcp:9600",
            "tcp:host:0",
            "tcp::9600",
            "unix:",
        ] {
            assert!(RemoteAddr::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    /// OUT is the hidraw write shape: report ID `0x00` + the 63-byte frame, one 64-byte
    /// record per chunk, so the far end can strip the ID like the host stack does.
    #[test]
    fn a_send_writes_one_hidraw_shaped_record_per_chunk() {
        let (tx, rx) = std::sync::mpsc::channel();
        let addr = listen(move |mut s| {
            let mut got = Vec::new();
            let _ = s.read_to_end(&mut got);
            let _ = tx.send(got);
        });
        let body: Vec<u8> = (0..130u8).collect(); // three chunks: 60 + 60 + 10
        {
            let remote = RemoteHid::open(&addr).expect("open");
            remote.send(&body).expect("send");
        }
        let got = rx.recv().expect("peer bytes");
        let want: Vec<u8> = proto::make_chunked_envelopes(&body)
            .iter()
            .flat_map(|f| std::iter::once(0x00).chain(f.iter().copied()))
            .collect();
        assert_eq!(got.len(), 3 * RECORD_LEN);
        assert_eq!(got, want);
    }

    /// IN records reach the caller RAW, and a record a pump deadline cuts in two is
    /// completed by the next pump, never dropped or misaligned.
    #[test]
    fn a_split_record_is_kept_for_the_next_pump() {
        let a = report(&[0x00, 0x35, 0x00, 0x04, 0x22, 0x04, 0x12, 0x02]);
        let b = report(&[0x00, 0x35, 0x00, 0x02, 0x08, 0x01]);
        let (a2, b2) = (a.clone(), b.clone());
        // Each side waits on the other, so the cut lands between the pumps on any host.
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let (wrote_tx, wrote_rx) = std::sync::mpsc::channel::<()>();
        let addr = listen(move |mut s| {
            let _ = s.set_nodelay(true);
            s.write_all(&a2).expect("a");
            s.write_all(&b2[..20]).expect("b head");
            let _ = wrote_tx.send(());
            let _ = go_rx.recv();
            s.write_all(&b2[20..]).expect("b tail");
            let _ = wrote_tx.send(());
            std::thread::sleep(Duration::from_millis(500));
        });
        let remote = RemoteHid::open(&addr).expect("open");
        wrote_rx.recv().expect("peer wrote a and the head of b");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut first = Vec::new();
        while remote.partial.borrow().len() < 20 && Instant::now() < deadline {
            first.extend(remote.pump(50).expect("pump"));
        }
        assert_eq!(first, vec![a]);
        assert_eq!(
            remote.partial.borrow().len(),
            20,
            "the head of b is carried"
        );
        go_tx.send(()).expect("go");
        wrote_rx.recv().expect("peer wrote the tail");
        let mut second = Vec::new();
        while second.is_empty() && Instant::now() < deadline {
            second.extend(remote.pump(50).expect("pump"));
        }
        assert_eq!(second, vec![b]);
    }

    /// The eager pump returns once the framing closed and the line went quiet, well
    /// inside its cap, exactly as on hardware.
    #[test]
    fn the_eager_pump_stops_once_the_reply_is_complete() {
        let reply = report(&[0x00, 0x35, 0x00, 0x02, 0x08, 0x01]);
        let r2 = reply.clone();
        let addr = listen(move |mut s| {
            let mut req = [0u8; RECORD_LEN];
            if s.read_exact(&mut req).is_ok() {
                s.write_all(&r2).expect("reply");
            }
            std::thread::sleep(Duration::from_secs(3));
        });
        let remote = RemoteHid::open(&addr).expect("open");
        let t = Instant::now();
        let got = remote
            .transact_eager(&proto::connection_request(), 2000)
            .expect("transact");
        assert_eq!(got, vec![reply]);
        assert!(
            t.elapsed() < Duration::from_millis(1000),
            "{:?}",
            t.elapsed()
        );
    }

    /// One seize per remote: a second open is refused while the first is live, and allowed
    /// once it drops. No listener is a NotFound, and it leaves nothing held.
    #[test]
    fn a_remote_is_seized_once_and_a_missing_remote_is_not_found() {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = RemoteAddr::Tcp(l.local_addr().expect("addr").to_string());
        std::thread::spawn(move || for _ in l.incoming() {});
        let first = RemoteHid::open(&addr).expect("first open");
        assert!(matches!(
            RemoteHid::open(&addr),
            Err(DeviceError::SeizeHeld(_))
        ));
        drop(first);
        drop(RemoteHid::open(&addr).expect("reopen after drop"));

        let gone = TcpListener::bind("127.0.0.1:0").expect("bind");
        let dead = RemoteAddr::Tcp(gone.local_addr().expect("addr").to_string());
        drop(gone);
        assert!(matches!(
            RemoteHid::open(&dead),
            Err(DeviceError::NotFound(_))
        ));
        assert!(!held().as_ref().is_some_and(|s| s.contains(&dead.key())));
    }

    /// A remote device that goes away ends the pump at once and fails the next send, like a unit
    /// pulled mid-session.
    #[test]
    fn a_closed_remote_ends_the_pump_and_fails_the_next_send() {
        let addr = listen(drop);
        let remote = RemoteHid::open(&addr).expect("open");
        let t = Instant::now();
        assert!(remote.pump(2000).expect("pump").is_empty());
        assert!(t.elapsed() < Duration::from_millis(1000));
        assert!(remote.send(&proto::connection_request()).is_err());
    }

    /// A remote stand-in that answers like the unit's side of the link: it strips the report
    /// ID, reassembles the frames into a body, lets a SimDevice answer it, and writes each
    /// reply as a 64-byte input report, zero-padded as the unit pads them.
    fn sim_remote(sim: crate::sim_device::SimDevice) -> RemoteAddr {
        listen(move |mut s| {
            let mut body = Vec::new();
            let mut rec = [0u8; RECORD_LEN];
            while s.read_exact(&mut rec).is_ok() {
                let (magic, len) = (rec[1], usize::from(u16::from_be_bytes([rec[2], rec[3]])));
                body.extend_from_slice(&rec[4..4 + len.min(60)]);
                if magic != 0x35 {
                    continue;
                }
                let mut replies = sim.transact(&std::mem::take(&mut body), 0).expect("sim");
                replies.extend(sim.pump(0).expect("sim pushes"));
                for mut r in replies {
                    r.resize(RECORD_LEN, 0);
                    if s.write_all(&r).is_err() {
                        return;
                    }
                }
            }
        })
    }

    struct RemoteTestRig(RemoteAddr);
    impl crate::rig::Rig for RemoteTestRig {
        fn open_transport(&self) -> Result<Box<dyn HidTransport>, DeviceError> {
            Ok(Box::new(RemoteHid::open(&self.0)?))
        }
        fn capture(
            &self,
            _: &[f32],
            _: u32,
            _: u64,
        ) -> Result<crate::audio::Capture, crate::error::LevelError> {
            Err(crate::error::LevelError::NoSignal)
        }
        fn settle_ms(&self, _: u64) -> u64 {
            0
        }
        fn aborted(&self) -> bool {
            false
        }
    }

    /// The session layer runs unchanged over the remote link: the full first-connect
    /// handshake and a store read come back through `RemoteHid`'s records. (The fake serves
    /// no preset list or firmware string; the offline tier pins both from fixtures.)
    #[test]
    fn a_session_handshakes_over_the_remote_link() {
        let sim = crate::sim_device::SimDevice::new();
        let addr = sim_remote(sim);
        let rig = std::rc::Rc::new(RemoteTestRig(addr));
        let state = crate::rig::scoped(rig, || {
            let mut s = crate::session::Session::connect().expect("connect");
            s.read_reamp_state().expect("re-amp read")
        });
        assert!(
            state.settings.is_some(),
            "the store read crossed the link: {state:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_unix_socket_remote_carries_the_same_records() {
        let path = std::env::temp_dir().join(format!("tmp-remote-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let l = std::os::unix::net::UnixListener::bind(&path).expect("bind");
        let reply = report(&[0x00, 0x35, 0x00, 0x02, 0x08, 0x01]);
        let r2 = reply.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut req = [0u8; RECORD_LEN];
                if s.read_exact(&mut req).is_ok() && req[0] == 0x00 && req[1] == 0x35 {
                    s.write_all(&r2).expect("reply");
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        let remote = RemoteHid::open(&RemoteAddr::Unix(path.clone())).expect("open");
        let mut got = remote
            .transact(&proto::connection_request(), 50)
            .expect("transact");
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.is_empty() && Instant::now() < deadline {
            got.extend(remote.pump(50).expect("pump"));
        }
        drop(remote);
        let _ = std::fs::remove_file(&path);
        assert_eq!(got, vec![reply]);
    }
}
