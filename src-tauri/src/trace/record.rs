//! The recorder: one bounded queue and one writer thread per process, and the
//! [`RecordingTransport`] that feeds it every HID call.
//!
//! The caller's side does no disk I/O, hashing, hex or metering. It clones the bytes it
//! has to (an OUT body, the returned reports, a capture), stamps a monotonic time and
//! `try_send`s one message. A full queue never blocks the caller: the message is
//! counted and the writer later emits a `drop` event, which marks the trace as not a
//! clean conformance source. The device drops a client after 750 ms without host
//! traffic, so the recorder must never add, delay, drop or reorder a write; it only
//! observes them.
//!
//! The writer assigns `seq` (gap-free per process), keeps the nominal clock `t_nom_ms`
//! (pump windows, nominal settles and nominal capture lengths), decodes kinds, writes
//! the lines and flushes at least every 250 ms. A capture's metering, hashing and WAV
//! run on a second thread, so a capture line is written when that work finishes (its
//! `t_us` and `t_nom_ms` are still the capture's own) and nothing else waits behind it.
//! A transport drop (`close`) waits briefly for the writer to catch up on the cheap
//! lines, so a short-lived probe that exits right after its last session still leaves a
//! complete trace, and a reopen mid-session is never held up by capture work.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, never, select, Receiver, Sender, TrySendError};

use super::capture::{self, CaptureJob, CaptureStats};
use super::format::{self, Line};
use crate::hid::HidTransport;

/// Queue depth for wire and small events. A full handshake is a few hundred messages.
const QUEUE_CAP: usize = 16_384;
/// Captures waiting for the writer at once (each holds ~5–6 MB of samples).
const MAX_CAPTURES_IN_FLIGHT: usize = 3;
/// The writer flushes at least this often.
const FLUSH_EVERY: Duration = Duration::from_millis(250);
/// How long a transport drop waits for the writer to catch up. The writer never does
/// capture work on its own thread, so catching up takes well under a millisecond; the
/// bound only caps a stalled disk.
const CLOSE_FLUSH_WAIT: Duration = Duration::from_millis(50);

/// Where a transport came from: the real device, or the e2e factory's in-memory fake.
/// Set from the branch that ran, never inferred from the environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Device,
    #[cfg(any(test, feature = "e2e"))]
    Sim,
}

impl Backend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Backend::Device => "device",
            #[cfg(any(test, feature = "e2e"))]
            Backend::Sim => "sim",
        }
    }
}

/// What the recorder writes and where. Built from the environment by
/// [`Config::from_vars`]; `None` when `TMP_TRACE_DIR` is unset.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    /// The trace directory. Captures' WAVs and field-8 files go to `wav/` and `field8/`
    /// next to it (the run directory).
    pub dir: PathBuf,
    pub process: String,
    pub run_id: Option<String>,
    pub git_sha: Option<String>,
    pub tree_key: Option<String>,
    pub unit_config: Option<String>,
    /// `TMP_TRACE_WAV=pair`: write each capture's processed pair as a WAV.
    pub wav_pair: bool,
    /// Where a device session's firmware version is written.
    pub fw_file: Option<PathBuf>,
}

impl Config {
    /// Read the recorder's variables through `get` (the environment in production).
    pub(crate) fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Config> {
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        let dir = PathBuf::from(non_empty("TMP_TRACE_DIR")?);
        let process = non_empty("TMP_TRACE_PROCESS").unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "process".into())
        });
        Some(Config {
            dir,
            process,
            run_id: non_empty("TMP_TRACE_RUN_ID"),
            git_sha: non_empty("TMP_TRACE_GIT_SHA"),
            tree_key: non_empty("TMP_TRACE_TREE_KEY"),
            unit_config: non_empty("TMP_E2E_UNIT_CONFIG"),
            wav_pair: non_empty("TMP_TRACE_WAV").as_deref() == Some("pair"),
            fw_file: non_empty("TMP_TRACE_FW_FILE").map(PathBuf::from),
        })
    }

    fn run_dir(&self) -> PathBuf {
        self.dir
            .parent()
            .map_or_else(|| self.dir.clone(), Path::to_path_buf)
    }

    fn trace_file(&self) -> PathBuf {
        self.dir
            .join(format!("{}-{}.tct.jsonl", self.process, std::process::id()))
    }
}

/// A capture on its way to the metering thread, with what the writer knew at hand-over.
type CaptureHandoff = (Box<CaptureJob>, CaptureMeta);

/// Messages from callers to the writer.
pub(crate) enum Msg {
    Open {
        sid: u64,
        backend: Backend,
        ok: bool,
        err: Option<String>,
        ms: u64,
        t: Instant,
    },
    Out {
        sid: u64,
        call_id: u64,
        call: &'static str,
        pump_ms: Option<u64>,
        body: Option<Vec<u8>>,
        t: Instant,
    },
    In {
        sid: u64,
        call_id: u64,
        t: Instant,
        t_end: Instant,
        reports: Vec<Vec<u8>>,
        err: Option<String>,
    },
    Close {
        sid: u64,
        t: Instant,
    },
    Settle {
        nominal_ms: u64,
        applied_ms: u64,
        t: Instant,
    },
    Capture(Box<CaptureJob>, u64),
    Cmd {
        name: String,
        args: Vec<u8>,
        ok: bool,
        ms: u64,
        t: Instant,
    },
    State {
        slot: u32,
        body: Vec<u8>,
        pristine: Option<String>,
        t: Instant,
    },
    Flush(Sender<()>),
}

/// Messages the queue shed, by class, until the writer reports them.
#[derive(Default)]
struct Drops {
    wire: AtomicU64,
    capture: AtomicU64,
    other: AtomicU64,
}

/// A process's recorder handle. Cheap to share; every method is non-blocking except
/// [`Recorder::flush`].
pub(crate) struct Recorder {
    tx: Sender<Msg>,
    next_sid: AtomicU64,
    next_call: AtomicU64,
    next_cap: AtomicU64,
    captures_in_flight: Arc<AtomicUsize>,
    drops: Arc<Drops>,
}

impl Recorder {
    /// Open the trace file and start the writer thread.
    pub(crate) fn start(cfg: Config) -> Result<Arc<Recorder>, String> {
        let (rec, writer) = Self::detached(cfg, QUEUE_CAP)?;
        writer.spawn()?;
        Ok(rec)
    }

    /// A recorder whose writer has not started: the caller spawns it (tests fill the
    /// queue first).
    pub(crate) fn detached(cfg: Config, cap: usize) -> Result<(Arc<Recorder>, Writer), String> {
        std::fs::create_dir_all(&cfg.dir)
            .map_err(|e| format!("trace dir {}: {e}", cfg.dir.display()))?;
        let path = cfg.trace_file();
        let file = File::create(&path).map_err(|e| format!("trace {}: {e}", path.display()))?;
        let (tx, rx) = bounded(cap);
        let t0 = Instant::now();
        let drops = Arc::new(Drops::default());
        let captures_in_flight = Arc::new(AtomicUsize::new(0));
        let (cap_tx, cap_rx) = bounded(MAX_CAPTURES_IN_FLIGHT);
        let (done_tx, done_rx) = bounded(MAX_CAPTURES_IN_FLIGHT);
        let rec = Arc::new(Recorder {
            tx,
            next_sid: AtomicU64::new(1),
            next_call: AtomicU64::new(1),
            next_cap: AtomicU64::new(1),
            captures_in_flight: captures_in_flight.clone(),
            drops: drops.clone(),
        });
        let mut writer = Writer {
            rx,
            out: BufWriter::new(file),
            cfg,
            t0,
            seq: 0,
            t_nom_ms: 0,
            current_sid: None,
            backends: HashMap::new(),
            call_seq: HashMap::new(),
            reamp: None,
            reamp_on: crate::proto::set_reamp_mode(true),
            reamp_off: crate::proto::set_reamp_mode(false),
            captures_in_flight,
            drops,
            cap_tx: Some(cap_tx),
            done_rx,
            worker: Some((cap_rx, done_tx)),
        };
        writer.header();
        Ok((rec, writer))
    }

    fn push(&self, msg: Msg, class: &AtomicU64) {
        match self.tx.try_send(msg) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                class.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn call_id(&self) -> u64 {
        self.next_call.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn settle(&self, nominal_ms: u64, applied_ms: u64) {
        self.push(
            Msg::Settle {
                nominal_ms,
                applied_ms,
                t: Instant::now(),
            },
            &self.drops.other,
        );
    }

    /// Hand a finished capture to the writer, or count it dropped when too many are
    /// already waiting.
    pub(crate) fn capture(&self, job: CaptureJob) {
        if self.captures_in_flight.fetch_add(1, Ordering::AcqRel) >= MAX_CAPTURES_IN_FLIGHT {
            self.captures_in_flight.fetch_sub(1, Ordering::AcqRel);
            self.drops.capture.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let n = self.next_cap.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = self.tx.try_send(Msg::Capture(Box::new(job), n)) {
            self.captures_in_flight.fetch_sub(1, Ordering::AcqRel);
            if matches!(e, TrySendError::Full(_)) {
                self.drops.capture.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn cmd(&self, name: &str, args: &[u8], ok: bool, ms: u64) {
        self.push(
            Msg::Cmd {
                name: name.to_string(),
                args: args.to_vec(),
                ok,
                ms,
                t: Instant::now(),
            },
            &self.drops.other,
        );
    }

    pub(crate) fn field8(&self, slot: u32, body: &[u8], pristine: Option<String>) {
        self.push(
            Msg::State {
                slot,
                body: body.to_vec(),
                pristine,
                t: Instant::now(),
            },
            &self.drops.other,
        );
    }

    /// Wait (up to `wait`) until the writer has written every line queued before this
    /// call. A capture still being metered is not waited for (see [`Recorder::drain`]).
    pub(crate) fn flush(&self, wait: Duration) {
        let (tx, rx) = bounded(1);
        if self.tx.send_timeout(Msg::Flush(tx), wait).is_ok() {
            let _ = rx.recv_timeout(wait);
        }
    }

    /// Wait (up to `wait`) for every handed-over capture to be written, then flush.
    #[cfg(test)]
    pub(crate) fn drain(&self, wait: Duration) {
        let deadline = Instant::now() + wait;
        while self.captures_in_flight.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.flush(deadline.saturating_duration_since(Instant::now()));
    }

    /// Open a transport through `f` and wrap it. The `open` event records the attempt,
    /// a failed seize included.
    pub(crate) fn open(
        self: &Arc<Self>,
        backend: Backend,
        f: impl FnOnce() -> Result<Box<dyn HidTransport>, String>,
    ) -> Result<Box<dyn HidTransport>, String> {
        let t = Instant::now();
        let r = f();
        let sid = self.next_sid.fetch_add(1, Ordering::Relaxed);
        self.push(
            Msg::Open {
                sid,
                backend,
                ok: r.is_ok(),
                err: r.as_ref().err().cloned(),
                ms: t.elapsed().as_millis() as u64,
                t,
            },
            &self.drops.wire,
        );
        let inner = r?;
        Ok(Box::new(RecordingTransport {
            inner,
            rec: self.clone(),
            sid,
        }))
    }
}

/// Records every call on the transport it wraps: an `out` just before the inner call,
/// an `in` with the returned reports just after, and `close` when dropped. It sits
/// below `TrackedHid`, so the auto-reopens are recorded too. The replies it returns are
/// the inner transport's, untouched.
pub(crate) struct RecordingTransport {
    inner: Box<dyn HidTransport>,
    rec: Arc<Recorder>,
    sid: u64,
}

impl RecordingTransport {
    fn call(
        &self,
        call: &'static str,
        pump_ms: Option<u64>,
        body: Option<&[u8]>,
        f: impl FnOnce(&dyn HidTransport) -> Result<Vec<Vec<u8>>, String>,
    ) -> Result<Vec<Vec<u8>>, String> {
        let call_id = self.rec.call_id();
        let t = Instant::now();
        self.rec.push(
            Msg::Out {
                sid: self.sid,
                call_id,
                call,
                pump_ms,
                body: body.map(<[u8]>::to_vec),
                t,
            },
            &self.rec.drops.wire,
        );
        let r = f(self.inner.as_ref());
        let (reports, err) = match &r {
            Ok(reports) => (reports.clone(), None),
            Err(e) => (Vec::new(), Some(e.clone())),
        };
        self.rec.push(
            Msg::In {
                sid: self.sid,
                call_id,
                t,
                t_end: Instant::now(),
                reports,
                err,
            },
            &self.rec.drops.wire,
        );
        r
    }
}

impl HidTransport for RecordingTransport {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.call("send", None, Some(body), |t| {
            t.send(body).map(|()| Vec::new())
        })
        .map(|_| ())
    }
    fn transact(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.call("transact", Some(pump_ms), Some(body), |t| {
            t.transact(body, pump_ms)
        })
    }
    fn transact_chunked(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.call("transact_chunked", Some(pump_ms), Some(body), |t| {
            t.transact_chunked(body, pump_ms)
        })
    }
    fn pump(&self, pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.call("pump", Some(pump_ms), None, |t| t.pump(pump_ms))
    }
    fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.call("transact_eager", Some(max_ms), Some(body), |t| {
            t.transact_eager(body, max_ms)
        })
    }
    fn template_mark(&self) -> std::sync::Arc<crate::hid::TemplateMark> {
        self.inner.template_mark()
    }
}

impl Drop for RecordingTransport {
    fn drop(&mut self) {
        self.rec.push(
            Msg::Close {
                sid: self.sid,
                t: Instant::now(),
            },
            &self.rec.drops.wire,
        );
        self.rec.flush(CLOSE_FLUSH_WAIT);
    }
}

/// The last re-amp toggle seen on the wire.
struct ReampSeen {
    on: bool,
    seq: u64,
    call_id: u64,
    echo: bool,
}

/// The writer: owns the trace file.
pub(crate) struct Writer {
    rx: Receiver<Msg>,
    out: BufWriter<File>,
    cfg: Config,
    t0: Instant,
    seq: u64,
    t_nom_ms: u64,
    current_sid: Option<u64>,
    backends: HashMap<u64, Backend>,
    call_seq: HashMap<u64, u64>,
    reamp: Option<ReampSeen>,
    reamp_on: Vec<u8>,
    reamp_off: Vec<u8>,
    captures_in_flight: Arc<AtomicUsize>,
    drops: Arc<Drops>,
    /// Captures handed to the metering thread, with what the writer knew at hand-over.
    cap_tx: Option<Sender<CaptureHandoff>>,
    /// Finished captures back from the metering thread.
    done_rx: Receiver<CaptureDone>,
    /// The metering thread's ends, until [`Writer::spawn`] starts it.
    worker: Option<(Receiver<CaptureHandoff>, Sender<CaptureDone>)>,
}

/// What the writer knew about a capture when it was handed over: its place in the
/// event order, kept so the line reads as if written then.
pub(crate) struct CaptureMeta {
    cap_id: String,
    sid: Option<u64>,
    t_nom_ms: u64,
    engage: String,
    ctx: String,
    /// Where to write the pair WAV (the run directory), when asked to.
    wav_dir: Option<PathBuf>,
}

/// A capture after the metering thread's work.
pub(crate) struct CaptureDone {
    job: Box<CaptureJob>,
    meta: CaptureMeta,
    stim_sha: String,
    stats: Option<CaptureStats>,
    wav: Option<String>,
}

/// The metering thread's work for one capture: the stimulus hash, the stats and the
/// optional pair WAV. Pure apart from the WAV file.
fn measure_capture(job: Box<CaptureJob>, meta: CaptureMeta) -> CaptureDone {
    let stim_bytes: Vec<u8> = job.stimulus.iter().flat_map(|s| s.to_le_bytes()).collect();
    let stim_sha = sha256_hex(&stim_bytes);
    let mut wav = None;
    let stats = job.result.as_ref().ok().map(|cap| {
        if let Some(run_dir) = &meta.wav_dir {
            let rel = format!("wav/{}.wav", meta.cap_id);
            let path = run_dir.join(&rel);
            match std::fs::create_dir_all(run_dir.join("wav"))
                .map_err(|e| e.to_string())
                .and_then(|()| capture::write_pair_wav(&path.to_string_lossy(), cap))
            {
                Ok(()) => wav = Some(rel),
                Err(e) => log::warn!("trace: capture WAV {}: {e}", path.display()),
            }
        }
        capture::stats(cap)
    });
    CaptureDone {
        job,
        meta,
        stim_sha,
        stats,
        wav,
    }
}

impl Writer {
    pub(crate) fn spawn(mut self) -> Result<(), String> {
        if let Some((cap_rx, done_tx)) = self.worker.take() {
            std::thread::Builder::new()
                .name("tct-capture".into())
                .spawn(move || {
                    for (job, meta) in cap_rx {
                        if done_tx.send(measure_capture(job, meta)).is_err() {
                            return;
                        }
                    }
                })
                .map_err(|e| format!("trace capture thread: {e}"))?;
        }
        std::thread::Builder::new()
            .name("tct-writer".into())
            .spawn(move || self.run())
            .map(|_| ())
            .map_err(|e| format!("trace writer: {e}"))
    }

    fn run(&mut self) {
        loop {
            let done_rx = self.done_rx.clone();
            select! {
                recv(self.rx) -> msg => match msg {
                    Ok(msg) => {
                        self.handle(msg);
                        if self.rx.is_empty() {
                            self.report_drops();
                            let _ = self.out.flush();
                        }
                    }
                    Err(_) => {
                        // Every recorder handle is gone: finish the captures in hand.
                        self.cap_tx = None;
                        while let Ok(done) = done_rx.recv() {
                            self.capture_done(done);
                        }
                        self.report_drops();
                        let _ = self.out.flush();
                        return;
                    }
                },
                recv(done_rx) -> done => match done {
                    Ok(done) => {
                        self.capture_done(done);
                        let _ = self.out.flush();
                    }
                    // The metering thread is gone; nothing more comes back.
                    Err(_) => self.done_rx = never(),
                },
                default(FLUSH_EVERY) => {
                    self.report_drops();
                    let _ = self.out.flush();
                }
            }
        }
    }

    fn us(&self, t: Instant) -> u64 {
        t.saturating_duration_since(self.t0).as_micros() as u64
    }

    fn line(&mut self, sid: Option<u64>, t: Instant, ev: &str) -> Line {
        self.line_at(sid, t, self.t_nom_ms, ev)
    }

    fn line_at(&mut self, sid: Option<u64>, t: Instant, t_nom_ms: u64, ev: &str) -> Line {
        let seq = self.seq;
        self.seq += 1;
        Line::event(seq, sid, self.us(t), t_nom_ms, ev)
    }

    fn write(&mut self, line: Line) {
        if let Err(e) = self.out.write_all(line.finish().as_bytes()) {
            log::warn!("trace write failed: {e}");
        }
    }

    fn header(&mut self) {
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let cfg = self.cfg.clone();
        let l = self
            .line(None, self.t0, "header")
            .u64("tct", u64::from(format::TCT_VERSION))
            .str(
                "recorder",
                concat!("tmp-companion ", env!("CARGO_PKG_VERSION")),
            )
            .str("process", &cfg.process)
            .u64("pid", u64::from(std::process::id()))
            .str("os", std::env::consts::OS)
            .str("wall_anchor_utc", &format::utc_millis(wall))
            .opt_str("run_id", cfg.run_id.as_deref())
            .opt_str("git_sha", cfg.git_sha.as_deref())
            .opt_str("tree_key", cfg.tree_key.as_deref())
            .str(
                "fixture_stamp",
                crate::probe_api::seed_scenario::FIXTURE_SOURCE_STAMP,
            )
            .opt_str("unit_config", cfg.unit_config.as_deref());
        self.write(l);
        let _ = self.out.flush();
    }

    fn report_drops(&mut self) {
        let classes = [
            ("wire", self.drops.wire.swap(0, Ordering::Relaxed)),
            ("capture", self.drops.capture.swap(0, Ordering::Relaxed)),
            ("other", self.drops.other.swap(0, Ordering::Relaxed)),
        ];
        for (what, count) in classes {
            if count > 0 {
                let l = self
                    .line(self.current_sid, Instant::now(), "drop")
                    .str("what", what)
                    .u64("count", count);
                self.write(l);
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Open {
                sid,
                backend,
                ok,
                err,
                ms,
                t,
            } => {
                if ok {
                    self.backends.insert(sid, backend);
                    self.current_sid = Some(sid);
                }
                let l = self
                    .line(Some(sid), t, "open")
                    .str("backend", backend.as_str())
                    .bool("ok", ok)
                    .opt_str("err", err.as_deref())
                    .u64("ms", ms);
                self.write(l);
            }
            Msg::Out {
                sid,
                call_id,
                call,
                pump_ms,
                body,
                t,
            } => {
                let seq = self.seq;
                self.call_seq.insert(call_id, seq);
                if let Some(b) = body.as_deref() {
                    if b == self.reamp_on.as_slice() || b == self.reamp_off.as_slice() {
                        self.reamp = Some(ReampSeen {
                            on: b == self.reamp_on.as_slice(),
                            seq,
                            call_id,
                            echo: false,
                        });
                    }
                }
                let l = self
                    .line(Some(sid), t, "out")
                    .str("call", call)
                    .opt_u64("pump_ms", pump_ms)
                    .opt_str("body", body.as_deref().map(format::hex).as_deref())
                    .opt_str("k", body.as_deref().map(format::body_kind).as_deref());
                self.write(l);
                self.t_nom_ms += pump_ms.unwrap_or(0);
            }
            Msg::In {
                sid,
                call_id,
                t,
                t_end,
                reports,
                err,
            } => self.inbound(sid, call_id, t, t_end, &reports, err.as_deref()),
            Msg::Close { sid, t } => {
                if self.current_sid == Some(sid) {
                    self.current_sid = None;
                }
                let l = self.line(Some(sid), t, "close");
                self.write(l);
            }
            Msg::Settle {
                nominal_ms,
                applied_ms,
                t,
            } => {
                let l = self
                    .line(self.current_sid, t, "settle")
                    .u64("nominal_ms", nominal_ms)
                    .u64("applied_ms", applied_ms);
                self.write(l);
                self.t_nom_ms += nominal_ms;
            }
            Msg::Capture(job, n) => self.capture_received(job, n),
            Msg::Cmd {
                name,
                args,
                ok,
                ms,
                t,
            } => {
                let sha = sha256_hex(&args);
                let l = self
                    .line(self.current_sid, t, "cmd")
                    .str("name", &name)
                    .str("args_sha256", &sha)
                    .bool("ok", ok)
                    .u64("ms", ms);
                self.write(l);
            }
            Msg::State {
                slot,
                body,
                pristine,
                t,
            } => self.state(slot, &body, pristine.as_deref(), t),
            Msg::Flush(ack) => {
                self.report_drops();
                let _ = self.out.flush();
                let _ = ack.send(());
            }
        }
    }

    fn inbound(
        &mut self,
        sid: u64,
        call_id: u64,
        t: Instant,
        t_end: Instant,
        reports: &[Vec<u8>],
        err: Option<&str>,
    ) {
        let of = self.call_seq.remove(&call_id);
        let kinds: Vec<String> = reports.iter().map(|r| format::report_kind(r)).collect();
        if let Some(seen) = self.reamp.as_mut() {
            if seen.call_id == call_id && kinds.iter().any(|k| k == "3.30") {
                seen.echo = true;
            }
        }
        let hexes: Vec<String> = reports.iter().map(|r| format::hex(r)).collect();
        let t_end_us = self.us(t_end);
        let l = self
            .line(Some(sid), t, "in")
            .opt_u64("of", of)
            .u64("t_end_us", t_end_us)
            .raw("reports", &format::str_array(&hexes))
            .raw("k", &format::str_array(&kinds))
            .opt_str("err", err);
        self.write(l);
        // A firmware reply is a single frame (settings 3, field 2).
        let fw = reports
            .iter()
            .filter(|r| r.len() > 1 && r[1] == 0x35)
            .find_map(|r| crate::session::extract_fw_version(format::report_body(r)));
        if let Some(fw) = fw {
            let l = self.line(Some(sid), t_end, "fw").str("fw", &fw);
            self.write(l);
            if self.backends.get(&sid) == Some(&Backend::Device) {
                if let Some(path) = &self.cfg.fw_file {
                    if let Err(e) = std::fs::write(path, format!("{fw}\n")) {
                        log::warn!("trace: writing {}: {e}", path.display());
                    }
                }
            }
        }
    }

    /// Snapshot what the line needs from the event order (session, nominal clock,
    /// re-amp state) and hand the heavy work to the metering thread.
    fn capture_received(&mut self, job: Box<CaptureJob>, n: u64) {
        let cap_id = format!("{}-{}-c{n}", self.cfg.process, std::process::id());
        let engage = self.reamp.as_ref().map_or_else(
            || "null".to_string(),
            |r| format!("{{\"on\":{},\"seq\":{},\"echo\":{}}}", r.on, r.seq, r.echo),
        );
        let ctx = job.ctx.as_ref().map_or_else(
            || "null".to_string(),
            |c| {
                Line::object()
                    .str("label", &c.label)
                    .opt_u64("slot", c.slot.map(u64::from))
                    .opt_u64("scene_slot", c.scene_slot.map(u64::from))
                    .opt_u64("switch", c.switch.map(u64::from))
                    .close()
            },
        );
        let meta = CaptureMeta {
            cap_id,
            sid: self.current_sid,
            t_nom_ms: self.t_nom_ms,
            engage,
            ctx,
            wav_dir: self.cfg.wav_pair.then(|| self.cfg.run_dir()),
        };
        let stim_frames = job.stimulus.len() as u64;
        self.t_nom_ms += stim_frames * 1000 / u64::from(job.rate.max(1)) + job.tail_ms;
        let unsent = match &self.cap_tx {
            Some(tx) => tx.send((job, meta)).err().map(|e| e.into_inner()),
            None => Some((job, meta)),
        };
        // No metering thread (a writer that was never spawned): do the work here.
        if let Some((job, meta)) = unsent {
            self.capture_done(measure_capture(job, meta));
        }
    }

    fn capture_done(&mut self, done: CaptureDone) {
        let CaptureDone {
            job,
            meta,
            stim_sha,
            stats,
            wav,
        } = done;
        let mut l = self
            .line_at(meta.sid, job.t_start, meta.t_nom_ms, "capture")
            .str("cap_id", &meta.cap_id)
            .str("backend", job.backend)
            .u64("t_start_us", self.us(job.t_start))
            .u64("t_end_us", self.us(job.t_end))
            .str("stim_sha256", &stim_sha)
            .u64("stim_frames", job.stimulus.len() as u64)
            .u64("rate", u64::from(job.rate))
            .u64("tail_ms", job.tail_ms);
        let err = match (&job.result, stats) {
            (Ok(_), Some(st)) => {
                l = l
                    .u64("channels", st.channels as u64)
                    .u64("frames", st.frames as u64)
                    .raw(
                        "lufs_i",
                        &st.lufs_i.map_or_else(|| "null".into(), format::num_json),
                    )
                    .raw(
                        "true_peak_dbtp",
                        &st.true_peak_dbtp
                            .map_or_else(|| "null".into(), format::num_json),
                    )
                    .raw(
                        "spread_lu",
                        &st.spread_lu.map_or_else(|| "null".into(), format::num_json),
                    )
                    .raw("peak", &format::f64_array(&st.peak))
                    .raw("rms", &format::f64_array(&st.rms));
                st.lufs_err.map(|e| format!("lufs: {e}"))
            }
            (result, _) => {
                l = l
                    .raw("channels", "null")
                    .raw("frames", "null")
                    .raw("lufs_i", "null")
                    .raw("true_peak_dbtp", "null")
                    .raw("spread_lu", "null")
                    .raw("peak", "[]")
                    .raw("rms", "[]");
                Some(result.as_ref().err().cloned().unwrap_or_default())
            }
        };
        l = l
            .raw("engage", &meta.engage)
            .raw("ctx", &meta.ctx)
            .opt_str("wav", wav.as_deref())
            .opt_str("err", err.as_deref());
        self.write(l);
        self.captures_in_flight.fetch_sub(1, Ordering::AcqRel);
    }

    fn state(&mut self, slot: u32, body: &[u8], pristine: Option<&str>, t: Instant) {
        let canon = super::state::canonical_json(body);
        let mut file = None;
        if let Some(c) = &canon {
            let rel = format!("field8/{slot}.json");
            let dir = self.cfg.run_dir().join("field8");
            let path = self.cfg.run_dir().join(&rel);
            match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, c)) {
                Ok(()) => file = Some(rel),
                Err(e) => log::warn!("trace: field-8 file {}: {e}", path.display()),
            }
        }
        let sha = canon.as_deref().map(|c| sha256_hex(c.as_bytes()));
        let l = self
            .line(self.current_sid, t, "state")
            .str("kind", "field8")
            .u64("slot", u64::from(slot))
            .u64("bytes", body.len() as u64)
            .bool("complete", canon.is_some())
            .opt_str("canon_sha256", sha.as_deref())
            .opt_str("pristine", pristine)
            .opt_str("file", file.as_deref());
        self.write(l);
    }
}

/// Lowercase hex SHA-256.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    format::hex(&sha2::Sha256::digest(bytes))
}
