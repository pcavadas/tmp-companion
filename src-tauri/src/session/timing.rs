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

/// The `connectionRequest` a write after a possible lapse sends first (`TrackedHid`).
pub(super) const REOPEN_MS: u64 = 100;

/// The in-burst `currentFwRequest` window: its reply is one tiny frame.
pub(super) const FW_READ_MS: u64 = 200;
/// A discovery connect's wait for the handshake drain's multi-packet preset JSON (~2 s).
pub(super) const HANDSHAKE_PRESET_JSON_MS: u64 = 1800;
/// A slot read injected inside the handshake burst (`extra_burst`).
pub(super) const EXTRA_BURST_MS: u64 = 2000;
/// The slot-read burst experiment (`connect_slotread`): connect, each burst request, the
/// in-burst read, and the closing batch-3 drain.
pub(super) const SLOTREAD_CONNECT_MS: u64 = 200;
pub(super) const SLOTREAD_BURST_STEP_MS: u64 = 20;
pub(super) const SLOTREAD_READ_MS: u64 = 1500;
pub(super) const SLOTREAD_DRAIN_MS: u64 = 300;
/// The backup request's own window, before its wall-clock stream loop.
pub(super) const BACKUP_REQUEST_MS: u64 = 120;

/// How long [`Session::read_reamp_state`] collects, kept alive, for both stores' replies.
/// A read opened right after a session that requested a preset dump gets that dump's
/// continuation frames first, and the Mixer reply arrived ~2.4 s late behind them (fw
/// 1.8.58, device-confirmed), so the window covers that with room to spare.
pub(super) const REAMP_STATE_WAIT_MS: u64 = 5_000;

// ─── Command windows ─────────────────────────────────────────────────────────
// One const per purpose. Each is the pump window (or round cap) its call site passed
// as a literal before the split; the wire golden pins the values.

/// One beat of the live heartbeat cadence: `heartbeat` then this pump (live edits,
/// structural confirms, footswitch edits, node-JSON reads, the rich load).
pub(super) const LIVE_BEAT_MS: u64 = 200;
/// `loadPreset` and `loadScene` echo window (also the rich load's `LoadPreset`).
pub(super) const LOAD_ECHO_MS: u64 = 300;
/// `setPresetLevel` echo window (`PresetLevelChanged`).
pub(super) const SET_LEVEL_ECHO_MS: u64 = 250;
/// `changeParameter` (float and bool) window.
pub(super) const CHANGE_PARAMETER_MS: u64 = 250;
/// Save, clear, rename and move of a user preset.
pub(super) const PRESET_WRITE_ECHO_MS: u64 = 300;
/// Re-amp setter echo window.
pub(super) const REAMP_ECHO_MS: u64 = 400;
/// The held-edit re-arm: its `connectionRequest`, then its `currentPresetInfoRequest`.
pub(super) const REARM_CONNECT_MS: u64 = 80;
pub(super) const REARM_INFO_REQUEST_MS: u64 = 120;
/// One poll beat while waiting for the active preset's name.
pub(super) const ACTIVE_PRESET_BEAT_MS: u64 = 150;
/// The live working-copy read: its request, then each kept-alive poll.
pub(super) const LIVE_VALUE_REQUEST_MS: u64 = 300;
pub(super) const LIVE_VALUE_POLL_MS: u64 = 200;
/// A per-node scene edit (`setNodeSceneEdit`).
pub(super) const SCENE_EDIT_ECHO_MS: u64 = 300;
/// Replace, remove, insert, node-JSON and string-parameter sends.
pub(super) const STRUCTURAL_EDIT_ECHO_MS: u64 = 200;
/// The settle after an IR file write, before its read-back.
pub(super) const IR_SETTLE_MS: u64 = 150;
/// Footswitch assignment set and clear.
pub(super) const FTSW_EDIT_ECHO_MS: u64 = 250;
/// Song-slot setters (assign, move, swap, clear, tap tempo, BPM active).
pub(super) const SONG_SETTER_ECHO_MS: u64 = 300;
/// Song and setlist list setters (load song, add, rename, remove, notes, setlist songs).
pub(super) const SONG_LIST_SETTER_ECHO_MS: u64 = 400;
/// A song/setlist harvest: its request, then each kept-alive poll.
pub(super) const HARVEST_REQUEST_MS: u64 = 250;
pub(super) const HARVEST_POLL_MS: u64 = 250;
/// My Presets list request window.
pub(super) const LIST_REQUEST_MS: u64 = 200;
/// A silent list-collection window once the list is proven (no keepalive needed).
pub(super) const LIST_QUIET_WINDOW_MS: u64 = 700;
/// `drain_until_quiet` before a list re-read: window and round cap.
pub(super) const DRAIN_QUIET_WINDOW_MS: u64 = 250;
pub(super) const DRAIN_MAX_WINDOWS: u32 = 20;
/// Round cap of the Factory list's growth-stability harvest.
pub(super) const FACTORY_LIST_MAX_WINDOWS: u32 = 20;
/// A slot read: the lapsed-session re-arm, the request, and its growth-stability harvest.
pub(super) const SLOT_READ_REARM_MS: u64 = 20;
pub(super) const SLOT_READ_REQUEST_MS: u64 = 200;
pub(super) const SLOT_READ_MAX_WINDOWS: u32 = 24;
pub(super) const SLOT_READ_WINDOW_MS: u64 = 150;
/// Round cap of the field-3 slot harvest.
#[cfg(test)] // the in-burst slot-read spike; only the wire golden runs it
pub(super) const SLOT_HARVEST_MAX_WINDOWS: u32 = 28;
/// One beat of the rich-harvest warm-up.
pub(super) const RICH_WARMUP_BEAT_MS: u64 = 120;
/// One settle step of `capture_full_preset_json`.
pub(super) const CAPTURE_SETTLE_STEP_MS: u64 = 250;
