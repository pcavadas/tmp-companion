//! Test-only fixtures and synthetic-signal helpers shared across module test
//! suites (`audio`'s onset tests, `leveller`'s Doctor onset-gate tests, the
//! preset-list readers' scripted HID transport) —
//! ONE home so a shared fixture/generator can't drift between suites.
//! `#[cfg(test)]`-gated at the `mod test_support;` declaration in `lib.rs`;
//! nothing here is compiled into a release binary.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A pluck train with a distinctive envelope (like the shipped stimuli) — a
/// synthetic stand-in for the real guitar-humbucker Doctor stimulus, which
/// isn't a bundled test asset.
pub(crate) fn plucky(secs: f32) -> Vec<f32> {
    use std::f32::consts::PI;
    const SR: u32 = 48_000;
    let n = (secs * SR as f32) as usize;
    let note = SR as usize / 2; // 500 ms notes
    (0..n)
        .map(|i| {
            let t = (i % note) as f32 / SR as f32;
            let env = (-t / 0.12).exp();
            env * (2.0 * PI * 220.0 * i as f32 / SR as f32).sin() * 0.5
        })
        .collect()
}

/// A tiny deterministic LCG (no new dependency) — just needs to be
/// unpredictable enough that per-hop noise can't accidentally correlate with
/// a stimulus envelope.
pub(crate) struct Lcg(pub(crate) u64);
impl Lcg {
    pub(crate) fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f64 / (1u64 << 24) as f64 - 1.0) as f32
    }
}

/// 2 ms hop at 48 kHz — matches the `fs13_wash_envelope_2ms` fixture's
/// envelope resolution.
const HOP: usize = 96;

/// Reconstruct a capture whose 2 ms-hop RMS matches `envelope` exactly: each
/// hop is deterministic LCG noise normalised to unit RMS, then scaled by that
/// hop's envelope value — so `doctor::tail_energy_ratio` (which only ever
/// reads RMS over sample ranges) reproduces the real capture's numbers.
pub(crate) fn reconstruct_capture(envelope: &[f64]) -> Vec<f32> {
    let mut lcg = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::with_capacity(envelope.len() * HOP);
    for &e in envelope {
        let mut hop: Vec<f32> = (0..HOP).map(|_| lcg.next_f32()).collect();
        let rms = (hop
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / HOP as f64)
            .sqrt();
        let scale = if rms > 0.0 { e / rms } else { 0.0 };
        for v in &mut hop {
            *v = (f64::from(*v) * scale) as f32;
        }
        out.extend(hop);
    }
    out
}

/// 2 ms RMS envelope of a REAL fs13 (`ACD_TMLargePlate`, 65%-wet) Doctor
/// capture — see `fixtures/fs13_wash_envelope_2ms.txt`'s header for
/// provenance and the pinned ground-truth `tail_energy_ratio` table.
const FS13_ENVELOPE: &str = include_str!("fixtures/fs13_wash_envelope_2ms.txt");

pub(crate) fn fs13_envelope() -> Vec<f64> {
    FS13_ENVELOPE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.parse::<f64>().expect("fixture line parses as f64"))
        .collect()
}

pub(crate) fn fs13_capture() -> Vec<f32> {
    reconstruct_capture(&fs13_envelope())
}

/// The device's inbound framing of one My-Presets `presetListResponse` carrying `total`
/// records (`0x33` start · `0x34` continue · `0x35` final). A slice `[..k]` of it is a
/// tail-truncated read as the HW one arrives (no terminal frame).
pub(crate) fn my_presets_list_frames(total: usize) -> Vec<Vec<u8>> {
    preset_list_frames(1, total)
}

/// [`my_presets_list_frames`] for any preset list (`list_enum` 1 / 3 / 4).
pub(crate) fn preset_list_frames(list_enum: u64, total: usize) -> Vec<Vec<u8>> {
    let mut resp = Vec::new();
    crate::proto::field_varint(&mut resp, 1, list_enum);
    for i in 0..total {
        let name = if i % 3 == 0 {
            format!("Preset {i}")
        } else {
            "Empty".to_string()
        };
        let rec = crate::proto::len_delimited(1, name.as_bytes());
        resp.extend(crate::proto::len_delimited(2, &rec));
    }
    // presetMessage → presetListResponse(5)
    crate::sim_device::frame_multi(&crate::sim_device::preset_message(5, &resp))
}

/// The device's inbound framing of a `presetDataChanged`(9) reply — the field-8 slot
/// read — carrying a `len`-byte presetJson. Slice it into batches to stream it.
pub(crate) fn preset_data_frames(len: usize) -> Vec<Vec<u8>> {
    crate::sim_device::frame_multi(&crate::sim_device::preset_data_changed(1, &vec![b'x'; len]))
}

/// The `importPresetResponse`(118) echo the device sends after an import landed at
/// `(list_enum, slot)`.
pub(crate) fn import_echo_frames(list_enum: u64, slot: u64) -> Vec<Vec<u8>> {
    let mut echo = Vec::new();
    crate::proto::field_varint(&mut echo, 2, list_enum);
    crate::proto::field_varint(&mut echo, 3, slot);
    crate::sim_device::frame_multi(&crate::sim_device::preset_message(118, &echo))
}

/// `frames` as reply batches of `per` frames — one batch per pump window.
pub(crate) fn streamed(frames: &[Vec<u8>], per: usize) -> Vec<Batch> {
    frames.chunks(per).map(<[Vec<u8>]>::to_vec).collect()
}

type Batch = Vec<Vec<u8>>;
/// Scripted replies: `(request body, reply batches)`, consumed in order per request.
type Script = VecDeque<(Vec<u8>, Vec<Batch>)>;

/// A scripted device for reply-wait tests. Each scripted request body pops its next
/// reply — report batches, the first delivered with the send and each later one by one
/// `pump` (an empty batch = a stalled window). Every sent body is recorded.
///
/// [`Self::device`] models fw 1.8.58's client drop (see
/// `Session::list_my_presets`): once the host has written nothing for longer than the
/// timeout, every undelivered batch is lost and the client is LAPSED — it answers and
/// applies nothing (a heartbeat does not revive it) until a `connectionRequest` reopens
/// it. A `connectionRequest` on an OPEN client is counted as a live re-arm (the device
/// answers it with a `connectionError`).
#[derive(Clone, Default)]
pub(crate) struct ScriptedTransport {
    replies: Arc<Mutex<Script>>,
    pending: Arc<Mutex<VecDeque<Batch>>>,
    pub sent: Arc<Mutex<Vec<Vec<u8>>>>,
    timeout_ms: Option<u64>,
    idle_ms: Arc<AtomicU64>,
    lapsed: Arc<AtomicBool>,
    dropped_writes: Arc<AtomicU64>,
    live_rearms: Arc<AtomicU64>,
}

impl ScriptedTransport {
    /// A device with fw 1.8.58's inactivity drop.
    pub fn device() -> Self {
        Self {
            timeout_ms: Some(crate::session::HID_INACTIVITY_DROP_MS),
            ..Self::default()
        }
    }
    /// Queue `batches` as if already in flight (e.g. the handshake's own list reply).
    pub fn with_pending(self, batches: Vec<Batch>) -> Self {
        self.pending.lock().unwrap().extend(batches);
        self
    }
    /// Script the reply to the next My Presets re-read (`preset_list_request` at batch 3).
    pub fn with_reply(self, batches: Vec<Batch>) -> Self {
        self.with_reply_to(&Self::list_reread(), batches)
    }
    /// Script the reply to the next send of exactly `request`.
    pub fn with_reply_to(self, request: &[u8], batches: Vec<Batch>) -> Self {
        self.replies
            .lock()
            .unwrap()
            .push_back((request.to_vec(), batches));
        self
    }
    /// Start the host-silence clock at `ms` — e.g. the handshake's final pump window.
    pub fn with_idle(self, ms: u64) -> Self {
        self.idle_ms.store(ms, Ordering::SeqCst);
        self
    }
    /// Host silence since the last write, in nominal pump milliseconds.
    pub fn idle_ms(&self) -> u64 {
        self.idle_ms.load(Ordering::SeqCst)
    }
    /// Writes (other than heartbeats) a lapsed client ignored.
    pub fn dropped_writes(&self) -> u64 {
        self.dropped_writes.load(Ordering::SeqCst)
    }
    /// `connectionRequest`s sent to a client that was still open.
    pub fn live_rearms(&self) -> u64 {
        self.live_rearms.load(Ordering::SeqCst)
    }
    pub fn count_sent(&self, body: &[u8]) -> usize {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|b| *b == body)
            .count()
    }
    pub fn list_requests(&self) -> usize {
        self.count_sent(&Self::list_reread())
    }
    fn list_reread() -> Vec<u8> {
        crate::proto::preset_list_request(1, crate::proto::BATCH_DRAIN)
    }
    pub fn heartbeats(&self) -> usize {
        self.count_sent(&crate::proto::heartbeat())
    }
    fn past_timeout(&self, idle: u64) -> bool {
        self.timeout_ms.is_some_and(|t| idle > t)
    }
    /// Record a write; `true` when the client is lapsed and ignores it.
    fn wrote(&self, body: &[u8]) -> bool {
        self.sent.lock().unwrap().push(body.to_vec());
        let idle = self.idle_ms.swap(0, Ordering::SeqCst);
        let lapsed = self.lapsed.load(Ordering::SeqCst) || self.past_timeout(idle);
        let reopen = body == crate::proto::connection_request();
        if lapsed && !reopen {
            self.lapsed.store(true, Ordering::SeqCst);
            if body != crate::proto::heartbeat() {
                self.dropped_writes.fetch_add(1, Ordering::SeqCst);
            }
            return true;
        }
        if reopen && !lapsed {
            self.live_rearms.fetch_add(1, Ordering::SeqCst);
        }
        self.lapsed.store(false, Ordering::SeqCst);
        false
    }
    /// Advance the host-silence clock by `ms`; past the timeout the client lapses and
    /// its queue is dropped.
    fn idle(&self, ms: u64) -> bool {
        let idle = self.idle_ms.fetch_add(ms, Ordering::SeqCst) + ms;
        let dropped = self.past_timeout(idle);
        if dropped {
            self.lapsed.store(true, Ordering::SeqCst);
            self.pending.lock().unwrap().clear();
        }
        dropped
    }
}

impl crate::hid::HidTransport for ScriptedTransport {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.wrote(body);
        Ok(())
    }
    fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        if self.wrote(body) {
            self.idle(ms);
            return Ok(Vec::new());
        }
        let mut batches: VecDeque<Batch> = {
            let mut replies = self.replies.lock().unwrap();
            match replies.iter().position(|(req, _)| req == body) {
                Some(i) => replies.remove(i).map(|(_, b)| b).unwrap_or_default().into(),
                None => VecDeque::new(),
            }
        };
        let first = batches.pop_front().unwrap_or_default();
        self.pending.lock().unwrap().extend(batches);
        Ok(if self.idle(ms) { Vec::new() } else { first })
    }
    fn transact_chunked(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.transact(body, ms)
    }
    fn transact_eager(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.transact(body, ms)
    }
    fn pump(&self, ms: u64) -> Result<Vec<Vec<u8>>, String> {
        if self.idle(ms) {
            return Ok(Vec::new());
        }
        Ok(self.pending.lock().unwrap().pop_front().unwrap_or_default())
    }
}

/// A [`crate::session::Session`] over `t` whose accumulator already holds `raw` (the
/// handshake's reports).
pub(crate) fn session_over(t: &ScriptedTransport, raw: Vec<Vec<u8>>) -> crate::session::Session {
    let mut s = crate::session::Session::from_transport(Box::new(t.clone()));
    s.raw = raw;
    s
}
