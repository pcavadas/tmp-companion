//! Every session wait, in one place: the device's inactivity drop, the keepalive
//! window, the handshake windows and one named const per command's pump window.
//! Transport set: a change here is a timing change and runs the full online suite.

/// fw 1.8.58 drops the HID client after this long with no INBOUND frame (static RE,
/// `ClientManager` timeout `0x0065e490`): the reply in flight and the whole send queue
/// are discarded, and the LAPSED client answers and applies nothing — a heartbeat does
/// not revive it — until a `connectionRequest` reopens it. A heartbeat on a live client
/// refreshes the timer without drawing a reply frame. Waits for a reply therefore keep
/// the client alive ([`Session::pump_collect_alive`]); writes after a silence reopen it
/// first (`TrackedHid`).
pub(crate) const HID_INACTIVITY_DROP_MS: u64 = 750;

/// Host-silence cap of a keepalive window: well inside [`HID_INACTIVITY_DROP_MS`].
pub(crate) const KEEPALIVE_WINDOW_MS: u64 = 350;

/// How long an import waits for its `importPresetResponse` echo.
pub(super) const IMPORT_ECHO_WAIT_MS: u64 = 1500;

/// Host silence under which the client is certainly still open. Between this and the
/// drop the device's timer and ours may disagree, so callers treat it as lapsed: a
/// re-arm on a still-live client costs one `connectionError`, a skipped one costs the read.
pub(super) const CLEARLY_LIVE_MS: u64 = HID_INACTIVITY_DROP_MS - 150;

/// An OUT gap above this leaves under 250 ms of headroom to [`HID_INACTIVITY_DROP_MS`];
/// `TrackedHid` logs each one (`hid-headroom:`) so a load-induced lapse is a named event.
pub(super) const HEADROOM_WARN_MS: u64 = 500;

/// The handshake's base pump windows `[connect, per-request, final]` (full or lean).
pub(super) fn handshake_base_windows(lean: bool) -> [u64; 3] {
    if lean {
        [50, 5, 75]
    } else {
        [200, 20, 300]
    }
}

/// One handshake pump window under the `TMP_HANDSHAKE_SCALE` bisect knob, capped below
/// the inactivity drop: every window is followed by the next request, so a window IS
/// the burst's longest host silence, and one past the drop discards the burst mid-flood.
pub(super) fn handshake_window(ms: u64, scale: Option<f64>) -> u64 {
    scale
        .map_or(ms, |s| ((ms as f64) * s).round() as u64)
        .min(CLEARLY_LIVE_MS)
}

/// How long [`Session::read_reamp_state`] collects, kept alive, for both stores' replies.
/// A read opened right after a session that requested a preset dump gets that dump's
/// continuation frames first, and the Mixer reply arrived ~2.4 s late behind them (fw
/// 1.8.58, device-confirmed), so the window covers that with room to spare.
pub(super) const REAMP_STATE_WAIT_MS: u64 = 5_000;
