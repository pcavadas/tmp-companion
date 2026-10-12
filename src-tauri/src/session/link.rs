//! The session's link to the device: the transport seam (`open_transport`, the e2e
//! factory), `TrackedHid` (host-silence clock and auto-reopen), and the send/pump/collect
//! primitives every command uses, kept-alive forms included. Transport set: only this
//! file and its siblings in the set touch `self.hid`, the clock or a literal window.

use super::*;

/// Offline UI e2e seam (compiled only under `--features e2e`): a process-global factory
/// that yields the [`HidTransport`] every device open uses, so a Playwright run can drive
/// the REAL backend against an in-memory `SimDevice` instead of USB. Both device-open
/// lanes — the command lane (`connect_inner`) and the monitor lane (`connect_with_firmware`
/// → `connect_inner`) — route through [`open_transport`], so ONE installed factory covers
/// every session. Pass a closure that clones one shared `SimDevice` and every session
/// shares the same fake (no divergent state, no second-open seize race). Production never
/// compiles this and always opens the real seized [`Hid`].
#[cfg(feature = "e2e")]
pub mod e2e_transport {
    use crate::hid::HidTransport;
    use std::sync::{Mutex, OnceLock};

    pub(super) type Factory = Box<dyn Fn() -> Box<dyn HidTransport> + Send + Sync>;
    pub(super) static FACTORY: OnceLock<Mutex<Option<Factory>>> = OnceLock::new();

    pub(super) fn cell() -> &'static Mutex<Option<Factory>> {
        FACTORY.get_or_init(|| Mutex::new(None))
    }

    /// Install the transport factory (offline mode). Subsequent device opens all go
    /// through `f`.
    pub fn set_factory(f: Factory) {
        *cell().lock().expect("e2e transport factory poisoned") = Some(f);
    }

    /// Yield a transport from the factory, or `None` if none is installed (online mode →
    /// the caller opens the real `Hid`).
    pub fn take() -> Option<Box<dyn HidTransport>> {
        cell()
            .lock()
            .expect("e2e transport factory poisoned")
            .as_ref()
            .map(|f| f())
    }
}

/// Open the device transport: the e2e fake when a factory is installed (`--features e2e`,
/// offline UI tests), else the real seized [`Hid`]. Single chokepoint for both the command
/// lane and the monitor lane, and so the one place the trace recorder (`crate::trace`)
/// hooks in, below `TrackedHid`; each branch names its own backend.
pub(super) fn open_transport() -> Result<Box<dyn HidTransport>, String> {
    #[cfg(feature = "e2e")]
    if let Some(t) = e2e_transport::take() {
        return crate::trace::open(crate::trace::Backend::Sim, || Ok(t));
    }
    crate::trace::open(crate::trace::Backend::Device, || Ok(Box::new(Hid::open()?)))
}

/// The transport plus how long the HOST has been silent (see [`HID_INACTIVITY_DROP_MS`]):
/// every write resets it, every pump extends it. `nominal_ms` (summed pump windows) makes
/// it testable on fakes whose pumps return instantly; on hardware wall-clock is the
/// truth (it also counts settles between pumps), so [`Self::silent_ms`] takes the max.
///
/// It also REOPENS: a write after a silence the device may have dropped the client in is
/// preceded by a `connectionRequest` (a lapsed client silently ignores everything else,
/// setters included). `auto_reopen` off is for experiments that measure lapsed behavior.
pub(super) struct TrackedHid {
    pub(super) inner: Box<dyn HidTransport>,
    pub(super) last_write: std::cell::Cell<std::time::Instant>,
    pub(super) nominal_ms: std::cell::Cell<u64>,
    pub(super) auto_reopen: bool,
    /// Reports the reopen's own pump window collected, handed out with the next batch.
    pub(super) reopened: std::cell::RefCell<Vec<Vec<u8>>>,
    /// The device's empty-template mark, set when any inbound report carries
    /// `presetError` 6: the unit swapped the working copy for its EMPTY template under the
    /// preset's name (a rejected body at load, or `loadScene(N≠8)` on a scene-less preset;
    /// static RE + device-confirmed). Device-scoped, so it outlives this session; cleared
    /// by a preset load that lands without the error. While set,
    /// [`Session::save_current_preset`] refuses.
    pub(super) template_mark: std::sync::Arc<crate::hid::TemplateMark>,
}

impl TrackedHid {
    pub(super) fn new(inner: Box<dyn HidTransport>) -> Self {
        Self {
            template_mark: inner.template_mark(),
            inner,
            last_write: std::cell::Cell::new(std::time::Instant::now()),
            nominal_ms: std::cell::Cell::new(0),
            auto_reopen: true,
            reopened: std::cell::RefCell::new(Vec::new()),
        }
    }
    /// Note a template substitution in any reassembled body of `reports`.
    pub(super) fn scan(&self, reports: &[Vec<u8>]) {
        if proto::reassemble_streams(reports)
            .iter()
            .any(|s| is_template_substitution(&s.body))
        {
            self.note_substituted();
        }
    }
    pub(super) fn note_substituted(&self) {
        if self.template_mark.note() {
            log::warn!("presetError 6: the unit replaced the working copy with an empty preset");
        }
    }
    /// About to write `body`: reopen a client the device may have dropped, then reset
    /// the silence clock.
    pub(super) fn wrote(&self, body: &[u8]) -> Result<(), String> {
        let gap_ms = self.last_write.get().elapsed().as_millis() as u64;
        let reopening = self.auto_reopen && self.maybe_lapsed();
        // Headroom accounting for the online e2e lane (`scripts/e2e.sh` counts these lines):
        // every reopen, and every OUT gap that ate into the last 250 ms before the drop. Counted,
        // never failed on: idle gaps the reopen covers are normal (one unloaded run: 141 of
        // 224 were reopens).
        if reopening || gap_ms > HEADROOM_WARN_MS {
            let at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            log::warn!(
                "hid-headroom: OUT gap {gap_ms} ms (silent {} ms) before a write{} at={at_ms}",
                self.silent_ms(),
                if reopening { ", auto-reopen" } else { "" }
            );
        }
        if reopening {
            let reopen = proto::connection_request();
            if body != reopen {
                let reports = self.inner.transact(&reopen, REOPEN_MS)?;
                self.reopened.borrow_mut().extend(reports);
            }
        }
        self.last_write.set(std::time::Instant::now());
        self.nominal_ms.set(0);
        Ok(())
    }
    /// One write + its pump window.
    pub(super) fn timed(
        &self,
        body: &[u8],
        ms: u64,
        f: impl FnOnce(&dyn HidTransport) -> Result<Vec<Vec<u8>>, String>,
    ) -> Result<Vec<Vec<u8>>, String> {
        self.wrote(body)?;
        let r = f(self.inner.as_ref());
        self.waited(ms);
        r.map(|reports| self.with_reopened(reports))
    }
    /// `reports`, preceded by any the reopen collected (oldest first).
    pub(super) fn with_reopened(&self, reports: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        let mut out = self.reopened.take();
        if out.is_empty() {
            out = reports;
        } else {
            out.extend(reports);
        }
        self.scan(&out);
        out
    }
    pub(super) fn waited(&self, ms: u64) {
        self.nominal_ms.set(self.nominal_ms.get() + ms);
    }
    pub(super) fn silent_ms(&self) -> u64 {
        let wall = self.last_write.get().elapsed().as_millis() as u64;
        wall.max(self.nominal_ms.get())
    }
    /// Whether the device may have dropped the client: silence at or past
    /// [`CLEARLY_LIVE_MS`].
    pub(super) fn maybe_lapsed(&self) -> bool {
        self.silent_ms() >= CLEARLY_LIVE_MS
    }
}

impl HidTransport for TrackedHid {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        self.wrote(body)?;
        self.inner.send(body)
    }
    fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.timed(body, ms, |t| t.transact(body, ms))
    }
    fn transact_chunked(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.timed(body, ms, |t| t.transact_chunked(body, ms))
    }
    fn transact_eager(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.timed(body, ms, |t| t.transact_eager(body, ms))
    }
    fn pump(&self, ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let r = self.inner.pump(ms);
        self.waited(ms);
        r.map(|reports| self.with_reopened(reports))
    }
    fn template_mark(&self) -> std::sync::Arc<crate::hid::TemplateMark> {
        self.template_mark.clone()
    }
}

impl Session {
    /// One write and its pump window through `TrackedHid`, the reports handed back to
    /// the caller (not collected into `raw`). Command modules use this, never `self.hid`.
    pub(super) fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.hid.transact(body, ms)
    }

    /// [`Self::transact`] that returns early once the reply framing is complete and the
    /// line is quiet (`ms` stays the cap): only for sends whose reports nobody parses.
    pub(super) fn transact_eager(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
        self.hid.transact_eager(body, ms)
    }

    /// Whether the unit answered `presetError` 6 since the last preset load that landed,
    /// on this session or an earlier one: the working copy is its EMPTY template under the
    /// preset's name, and a save would persist it.
    pub fn template_substituted(&self) -> bool {
        self.hid.template_mark.is_set()
    }

    /// Run a preset load: the mark clears only when `load` succeeds and its own reply
    /// carried no `presetError` 6 (a real reload restores the stored body).
    pub(super) fn landing_load<T>(
        &mut self,
        load: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let epoch = self.hid.template_mark.epoch();
        let out = load(self)?;
        self.hid.template_mark.clear_unless_noted_since(epoch);
        Ok(out)
    }

    /// Whether the device may have dropped this client (see `TrackedHid::maybe_lapsed`).
    pub(super) fn maybe_lapsed(&self) -> bool {
        self.hid.maybe_lapsed()
    }

    /// Pump until the inbound stream goes quiet (`max_windows` windows of
    /// `window_ms` with no new reports, or stop growing). Used by the passive
    /// scene scan to drain the handshake flood before the first re-armed
    /// field-8 read. NOTE: a batch-bearing `preset_list_request` is NOT
    /// answered on a minimal burst (HW-observed — the device only answers it
    /// inside the recognized full sequence), so the scan can't avoid the full
    /// handshake; it drains it instead.
    pub fn drain_until_quiet(&mut self, window_ms: u64, max_windows: u32) -> Result<(), String> {
        let mut last = self.raw.len();
        for _ in 0..max_windows {
            self.pump_silent(window_ms)?;
            if self.raw.len() == last {
                return Ok(());
            }
            last = self.raw.len();
        }
        Ok(())
    }

    pub fn heartbeat(&mut self) -> Result<(), String> {
        self.hid.send(&proto::heartbeat())
    }

    /// Send a request and accumulate the raw reports received during `ms`.
    pub(crate) fn send_and_collect(&mut self, body: &[u8], ms: u64) -> Result<(), String> {
        debug_assert!(
            !self.handshake_done || proto::answered_at_once(body),
            "after the handshake a request must carry batchStatus 0, 3 or none \
             (1/2 only queue for a later session, ≥4 is dropped)"
        );
        let reports = self.hid.transact(body, ms)?;
        self.raw.extend(reports);
        Ok(())
    }

    /// Pump (no send) and accumulate the raw reports received during `ms`.
    pub(crate) fn pump_silent(&mut self, ms: u64) -> Result<(), String> {
        let reports = self.hid.pump(ms)?;
        self.raw.extend(reports);
        Ok(())
    }

    /// Drop the accumulated inbound buffer (so a following `push_bodies` reflects
    /// only what arrives next). Public for diagnostics that isolate one exchange.
    pub fn clear_raw(&mut self) {
        self.raw.clear();
    }

    /// Reassemble everything accumulated so far into message streams.
    pub(super) fn streams(&self) -> Vec<proto::Stream> {
        proto::reassemble_streams(&self.raw)
    }

    /// Reassemble using the 0x35-is-final rule (for complete list responses).
    pub(super) fn streams_final(&self) -> Vec<proto::Stream> {
        proto::reassemble_streams_final(&self.raw)
    }

    /// The reassembled inbound message bodies accumulated so far, using the
    /// 0x35-is-final rule (same as [`Self::streams_final`], the `listen_dump`
    /// reassembly). Public so the device monitor can drain pushes off the open
    /// connection without `raw`/`streams_final` being module-private to it. Re-parses
    /// all of `raw` on every call: a long-lived reader uses [`Self::drain_closed_pushes`].
    pub fn push_bodies(&self) -> Vec<Vec<u8>> {
        let bodies: Vec<Vec<u8>> = self.streams_final().into_iter().map(|s| s.body).collect();
        if bodies.iter().any(|b| is_template_substitution(b)) {
            self.hid.note_substituted();
        }
        bodies
    }

    /// Move every accumulated report into `pushes` and return the bodies of the streams that
    /// CLOSED, in arrival order — the incremental form of [`Self::push_bodies`] for the
    /// live-sync monitor (#262). `raw` is left EMPTY, so a session that pumps for hours holds
    /// one open stream, not its whole push history, and nothing is parsed twice. A closed
    /// newest stream is returned at once; only a still-open one waits in `pushes`.
    pub fn drain_closed_pushes(&mut self, pushes: &mut proto::FinalReassembler) -> Vec<Vec<u8>> {
        let mut closed = Vec::new();
        for report in self.raw.drain(..) {
            pushes.push(&report, &mut closed);
        }
        let bodies: Vec<Vec<u8>> = closed.into_iter().map(|s| s.body).collect();
        if bodies.iter().any(|b| is_template_substitution(b)) {
            self.hid.note_substituted();
        }
        bodies
    }

    /// One kept-alive [`KEEPALIVE_WINDOW_MS`] window.
    pub(super) fn keepalive_pump(&mut self) -> Result<(), String> {
        self.pump_collect_alive(KEEPALIVE_WINDOW_MS)
    }

    /// Pump `ms` in slices of at most [`KEEPALIVE_WINDOW_MS`], heartbeating before any
    /// slice that would otherwise carry the host silence to [`CLEARLY_LIVE_MS`] — a reply
    /// streaming through the wait is never cut by the device's inactivity drop.
    pub(crate) fn pump_collect_alive(&mut self, ms: u64) -> Result<(), String> {
        let mut left = ms;
        while left > 0 {
            let w = left.min(KEEPALIVE_WINDOW_MS);
            if self.hid.silent_ms() + w >= CLEARLY_LIVE_MS {
                self.heartbeat()?;
            }
            self.pump_silent(w)?;
            left -= w;
        }
        Ok(())
    }

    /// Pump kept-alive `window`-ms slices (at most `max`) until `len` reads the same
    /// non-zero value for two slices running — a multi-frame reply that stopped growing.
    pub(crate) fn pump_until_stable(
        &mut self,
        max: u32,
        window: u64,
        len: impl Fn(&Self) -> usize,
    ) -> Result<(), String> {
        let (mut last, mut stable) = (0usize, 0u32);
        for _ in 0..max {
            self.pump_collect_alive(window)?;
            let cur = len(self);
            if cur > 0 && cur == last {
                stable += 1;
                if stable >= 2 {
                    break;
                }
            } else {
                stable = 0;
            }
            last = cur;
        }
        Ok(())
    }

    /// [`Self::send_and_collect`] whose window past [`KEEPALIVE_WINDOW_MS`] is kept alive.
    pub(crate) fn send_and_collect_alive(&mut self, body: &[u8], ms: u64) -> Result<(), String> {
        let first = ms.min(KEEPALIVE_WINDOW_MS);
        self.send_and_collect(body, first)?;
        self.pump_collect_alive(ms - first)
    }

    /// Pump silently until the device has dropped the client — the LAPSED session the
    /// quiet-session re-arm recipes expect.
    pub(crate) fn lapse(&mut self) -> Result<(), String> {
        let target = HID_INACTIVITY_DROP_MS + 100;
        let silent = self.hid.silent_ms();
        if silent < target {
            self.pump_silent(target - silent)?;
        }
        Ok(())
    }

    /// Whether a write after a silence first reopens a client the device may have
    /// dropped (on by default — see [`TrackedHid`]). Off only for experiments that
    /// measure lapsed behavior itself.
    pub(crate) fn set_auto_reopen(&mut self, on: bool) {
        self.hid.auto_reopen = on;
    }

    /// Send a body via the chunked (`0x33/0x34/0x35`) framing and collect the reply —
    /// the single-report `transact` panics on bodies > 60 B, which a `replaceNode`
    /// with long ids (e.g. an `…CabIR` node) overflows.
    pub(crate) fn send_chunked_collect(&mut self, body: &[u8], ms: u64) -> Result<(), String> {
        let reports = self.hid.transact_chunked(body, ms)?;
        self.raw.extend(reports);
        Ok(())
    }
}
