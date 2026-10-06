//! Test-only fixtures and synthetic-signal helpers shared across module test
//! suites (`audio`'s onset tests, `leveller`'s Doctor onset-gate tests, the
//! preset-list readers' scripted HID transport) —
//! ONE home so a shared fixture/generator can't drift between suites.
//! `#[cfg(test)]`-gated at the `mod test_support;` declaration in `lib.rs`;
//! nothing here is compiled into a release binary.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
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
    let mut resp = Vec::new();
    crate::proto::field_varint(&mut resp, 1, 1); // listEnum = My Presets
    for i in 0..total {
        let name = if i % 3 == 0 {
            format!("Preset {i}")
        } else {
            "Empty".to_string()
        };
        let rec = crate::proto::len_delimited(1, name.as_bytes());
        resp.extend(crate::proto::len_delimited(2, &rec));
    }
    // presetMessage(2) → presetListResponse(5)
    let body = crate::proto::len_delimited(2, &crate::proto::len_delimited(5, &resp));
    crate::sim_device::frame_multi(&body)
}

type Batch = Vec<Vec<u8>>;

/// A scripted transport for preset-list reads. Every `preset_list_request` pops the
/// next scripted reply — report batches, the first delivered with the send and each
/// later one by one `pump` (an empty batch = a stalled window). Every sent body is
/// recorded. [`Self::with_inactivity_timeout`] models the device dropping the HID client
/// (see `Session::list_my_presets`): once the host has written nothing for longer than
/// the timeout, every undelivered batch is lost.
#[derive(Clone, Default)]
pub(crate) struct ListTransport {
    replies: Arc<Mutex<VecDeque<Vec<Batch>>>>,
    pending: Arc<Mutex<VecDeque<Batch>>>,
    pub sent: Arc<Mutex<Vec<Vec<u8>>>>,
    timeout_ms: Option<u64>,
    idle_ms: Arc<AtomicU64>,
}

impl ListTransport {
    /// Queue `batches` as if already in flight (the handshake's own list reply).
    pub fn with_pending(self, batches: Vec<Batch>) -> Self {
        self.pending.lock().unwrap().extend(batches);
        self
    }
    /// Script the reply to the next `preset_list_request(1, 1)`.
    pub fn with_reply(self, batches: Vec<Batch>) -> Self {
        self.replies.lock().unwrap().push_back(batches);
        self
    }
    pub fn with_inactivity_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
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
    pub fn count_sent(&self, body: &[u8]) -> usize {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|b| *b == body)
            .count()
    }
    pub fn list_requests(&self) -> usize {
        self.count_sent(&crate::proto::preset_list_request(1, 1))
    }
    pub fn heartbeats(&self) -> usize {
        self.count_sent(&crate::proto::heartbeat())
    }
    fn wrote(&self, body: &[u8]) {
        self.sent.lock().unwrap().push(body.to_vec());
        self.idle_ms.store(0, Ordering::SeqCst);
    }
    /// Advance the host-silence clock by `ms`; past the timeout the queue is dropped.
    fn idle(&self, ms: u64) -> bool {
        let idle = self.idle_ms.fetch_add(ms, Ordering::SeqCst) + ms;
        let dropped = self.timeout_ms.is_some_and(|t| idle > t);
        if dropped {
            self.pending.lock().unwrap().clear();
        }
        dropped
    }
}

impl crate::hid::HidTransport for ListTransport {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.wrote(body);
        Ok(())
    }
    fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.wrote(body);
        let mut batches: VecDeque<Batch> = if body == crate::proto::preset_list_request(1, 1) {
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default()
                .into()
        } else {
            VecDeque::new()
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
pub(crate) fn session_over(t: &ListTransport, raw: Vec<Vec<u8>>) -> crate::session::Session {
    let mut s = crate::session::Session::from_transport(Box::new(t.clone()));
    s.raw = raw;
    s
}
