//! Persistent device monitor: holds a live TMP session with a dense ~250 ms
//! `ConnectionHeartbeat` so the unit PUSHES its state changes (footswitch taps,
//! scene recalls, preset changes done ON THE HARDWARE) UNSOLICITED, and mirrors
//! those pushes to the frontend as Tauri events. This is the backend half of the
//! R3 "live-sync" Scenes feature — the app moves on its own when the unit moves.
//!
//! Why a dedicated Session loop (not the `watcher.rs` IOHIDManager): the watcher
//! is a NON-seizing matcher (attach/detach only, zero I/O). The monitor must SEIZE
//! the device to receive pushes — exactly like every command. So it goes through
//! `Session`/`Hid` (inheriting the proven open-retry + handshake), NOT raw IOKit.
//!
//! ## Seize ownership — app-level monitor owns idle device I/O
//!
//! The TMP is single-connection exclusive-HID: there is exactly ONE seize owner at
//! any instant. At app startup `connect_device` releases any old UI session, enables
//! `MONITOR_ENABLED`, and waits for this monitor's startup snapshot. While connected,
//! the monitor is the idle owner; commands borrow the device through the pause/ack
//! protocol below.
//!
//! `MONITOR_ENABLED` still gates the monitor:
//!   * `connect_device` sets it and clears `AppState.session`, making the monitor the
//!     sole startup/list/live owner.
//!   * `stop_live_sync` is retained for diagnostics/settings paths; it clears the flag,
//!     the monitor drops its `Session`, and the persistent UI session is re-established.
//!   * The monitor opens HID ONLY when `MONITOR_ENABLED` is set AND
//!     `AppState.session == None` — it never retry-spams against a held UI seize.
//!
//! ## Seize-sharing with commands (while live-sync is enabled)
//!
//! While enabled the monitor holds the seize; every command opens its OWN fresh
//! `Session`. Two open seizes → `0xe00002c5`. They coexist via a PAUSE-THEN-ACK
//! handshake gated by the SAME `DEVICE_OP_LOCK` the commands serialize on (see
//! `lib::lock_device_op`):
//!   1. A command acquires `DEVICE_OP_LOCK`, sets `MONITOR_PAUSE_REQ`, and waits
//!      (bounded) for `MONITOR_PAUSED_ACK`.
//!   2. The monitor, polling the flag each pump iteration, drops its `Session`
//!      (freeing the seize), sets the ack, emits a neutral `sync`, and parks until
//!      the request clears.
//!   3. The command's guard Drop clears `MONITOR_PAUSE_REQ`; the monitor reconnects
//!      and re-reads the (possibly command-changed) state, then emits `sync: false`.
//!
//! The monitor never acquires a lock, so the command's bounded *sleep* on the ack
//! is not a lock cycle → no deadlock. `hid.rs`'s open-retry absorbs the residual
//! seize-recycle race on reconnect.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex, OnceLock};

use crossbeam_channel::{Receiver, Sender};
use serde::Serialize;

use crate::session::{self, ActiveGraph, PresetEntry, Session};
use crate::{footswitch, MONITOR_ENABLED, MONITOR_PAUSED_ACK, MONITOR_PAUSE_REQ};

// ─── Startup snapshot ───────────────────────────────────────────────────────
//
// App-level `connect_device` no longer opens its own HID session. It enables this
// monitor and waits for the monitor's first successful handshake snapshot: firmware,
// My Presets list, and the active signal graph when available. That keeps list +
// graph + firmware sourced from one startup session.

#[derive(Debug, Clone)]
pub(crate) struct StartupSnapshot {
    pub firmware: Option<String>,
    pub presets: Vec<PresetEntry>,
    pub graph: Option<ActiveGraph>,
}

#[derive(Debug)]
struct AppliedStartupLive {
    scene_list: Option<SceneListPayload>,
    live_scene: Option<LiveScene>,
    signal_chain: Option<ActiveGraph>,
}

static STARTUP_SNAPSHOT: OnceLock<Mutex<Option<StartupSnapshot>>> = OnceLock::new();
static LAST_CONNECT_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn snapshot_slot() -> &'static Mutex<Option<StartupSnapshot>> {
    STARTUP_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

fn error_slot() -> &'static Mutex<Option<String>> {
    LAST_CONNECT_ERROR.get_or_init(|| Mutex::new(None))
}

/// e2e seam: install a fixture startup snapshot so `connect_device` / `list_presets`
/// serve it WITHOUT a live monitor thread. The offline UI server pre-fills this and
/// marks the monitor enabled; the device command lanes then fall back to the classic
/// `Session::connect()` path, which the `session::e2e_transport` factory routes to a
/// `SimDevice`. Sidesteps making the (Wry-typed) monitor/watcher generic over the
/// MockRuntime. Compiled only under `--features e2e`.
#[cfg(feature = "e2e")]
pub fn e2e_install_snapshot(
    firmware: Option<String>,
    presets: Vec<PresetEntry>,
    graph: Option<ActiveGraph>,
) {
    *crate::lock_ok(snapshot_slot()) = Some(StartupSnapshot {
        firmware,
        presets,
        graph,
    });
}

pub(crate) fn startup_snapshot() -> Option<StartupSnapshot> {
    crate::lock_ok(snapshot_slot())
        .clone()
}

/// Just the cached snapshot's graph — clones only the (small) graph, not the whole
/// snapshot (the 504-entry preset list). For the cheap `current_graph` re-seed read.
pub(crate) fn startup_graph() -> Option<ActiveGraph> {
    crate::lock_ok(snapshot_slot())
        .as_ref()
        .and_then(|s| s.graph.clone())
}

pub(crate) fn last_connect_error() -> Option<String> {
    crate::lock_ok(error_slot())
        .clone()
}

pub(crate) fn reset_startup_state() {
    *crate::lock_ok(snapshot_slot()) = None;
    *crate::lock_ok(error_slot()) = None;
}

pub(crate) fn clear_startup_snapshot() {
    *crate::lock_ok(snapshot_slot()) = None;
}

fn store_startup_snapshot(snapshot: StartupSnapshot) {
    *crate::lock_ok(snapshot_slot()) = Some(snapshot);
    *crate::lock_ok(error_slot()) = None;
}

/// Keep the cached startup snapshot's graph aligned with the device's CURRENT
/// preset: called from `decode_and_emit` on every fresh field-3 graph, so an
/// already-running `connect_device` (webview reload) returns the current graph,
/// not the connect-time one. No-op until the handshake stores the first snapshot.
pub(crate) fn refresh_snapshot_graph(graph: ActiveGraph) {
    if let Some(snapshot) = crate::lock_ok(snapshot_slot()).as_mut() {
        snapshot.graph = Some(graph);
    }
}

fn store_connect_error(error: String) {
    *crate::lock_ok(snapshot_slot()) = None;
    *crate::lock_ok(error_slot()) = Some(error);
}

// ─── Live command lane ─────────────────────────────────────────────────────────
//
// While live-sync is active the monitor holds the ONLY open session — and a tiny
// device send (LoadPreset / LoadScene) doesn't need a connection of its own: Pro
// Control fires these on its one persistent session in ~100 ms. So commands can
// hand such an op to the monitor, which executes it on its live session between
// pumps (the `probe --scenes-load` precedent: `send_and_collect`, NOT
// `session::load_preset`, so the resulting pushes stay in the accumulator and the
// normal pump-loop decode turns them into `tmp://live-*` events). This skips the
// whole pause → release → fresh-handshake → reconnect bookend (~2 s → ~0.2 s).
//
// Safety: the lane is OPPORTUNISTIC. The sender checks `MONITOR_SESSION_LIVE`
// first; every monitor state that is NOT actively pumping a healthy session
// drains the queue with `NotLive`, and the caller then falls back to the classic
// `with_released_seize` path (which serializes on `DEVICE_OP_LOCK` as before).
// A live op never touches `DEVICE_OP_LOCK` — it doesn't release or acquire any
// seize, and a paused monitor (a command holds the gate) replies `NotLive`, so
// command serialization is preserved.

/// A device op the monitor can execute on its live session. Deliberately limited
/// to the proven-on-a-live-heartbeat-session sends (the scene-scan precedent);
/// anything needing its own connection semantics stays on the classic path.
pub(crate) enum LiveOp {
    /// `loadPreset` — 0-based My-Presets list index (the +1 wire translation
    /// happens here, mirroring `session::load_preset`).
    LoadPreset(u32),
    /// `loadScene` — 0-based wire `scenes[]` slot ([`session::BASE_SCENE_SLOT`]
    /// recalls base). Only valid for the ACTIVE preset (the proto has no preset
    /// addressing of its own).
    LoadScene(u32),
}

/// Reply from the monitor for a [`LiveCmd`]. `NotLive` = not executed (caller
/// must fall back to the classic path); `Done` = executed on the live session.
pub(crate) enum LiveReply {
    Done(Result<(), String>),
    NotLive,
}

pub(crate) struct LiveCmd {
    op: LiveOp,
    reply: Sender<LiveReply>,
}

pub(crate) struct MetadataReadCmd {
    list_index: u32,
    reply: Sender<MetadataReadReply>,
}

pub(crate) enum MetadataReadReply {
    Done(Result<Option<Vec<u8>>, String>),
    NotLive,
}

/// Sender half of the live command lane, installed once by [`spawn`]. Unset on
/// non-macOS (no monitor thread) — `try_live_op` then returns `None`.
static LIVE_CMD_TX: OnceLock<Sender<LiveCmd>> = OnceLock::new();
static METADATA_READ_TX: OnceLock<Sender<MetadataReadCmd>> = OnceLock::new();

/// True exactly while [`pump_loop`] is pumping a healthy session — the sender-side
/// gate for the live lane. Cleared (via RAII guard) on every pump-loop exit:
/// pause, disable, or device error.
static MONITOR_SESSION_LIVE: AtomicBool = AtomicBool::new(false);

/// Pump window (ms) after a live-lane send — just enough to flush the send and
/// catch the first echo; the surrounding pump loop keeps collecting the rest of
/// the push burst as usual.
const LIVE_OP_PUMP_MS: u64 = 60;

/// Try to execute `op` on the monitor's live session. Returns `None` when the
/// lane isn't available (live-sync off, monitor paused/reconnecting, non-macOS)
/// — the caller falls back to the classic `with_released_seize` path. Blocks up
/// to a few seconds (the monitor consumes the queue every pump, ≤ ~0.5 s).
pub(crate) fn try_live_op(op: LiveOp) -> Option<Result<(), String>> {
    if !MONITOR_ENABLED.load(SeqCst) || !MONITOR_SESSION_LIVE.load(SeqCst) {
        return None;
    }
    let tx = LIVE_CMD_TX.get()?;
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    tx.send(LiveCmd {
        op,
        reply: reply_tx,
    })
    .ok()?;
    match reply_rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(LiveReply::Done(r)) => Some(r),
        // Raced a pause/disable between the gate check and consumption — fall back.
        Ok(LiveReply::NotLive) => None,
        // The monitor consumes or drains the queue every pump iteration, so a
        // timeout means the monitor thread is wedged — surface it rather than
        // double-executing via a fallback.
        Err(_) => Some(Err("live command lane timed out".into())),
    }
}

/// Opportunistically read one preset's plaintext field-8 JSON on the monitor's
/// open session. Returns `None` when the monitor is not actively pumping; callers
/// must fall back to the classic pause + fresh-session path.
pub(crate) fn try_metadata_read(list_index: u32) -> Option<Result<Option<Vec<u8>>, String>> {
    if !MONITOR_ENABLED.load(SeqCst) || !MONITOR_SESSION_LIVE.load(SeqCst) {
        return None;
    }
    let tx = METADATA_READ_TX.get()?;
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    tx.send(MetadataReadCmd {
        list_index,
        reply: reply_tx,
    })
    .ok()?;
    match reply_rx.recv_timeout(std::time::Duration::from_secs(4)) {
        Ok(MetadataReadReply::Done(r)) => Some(r),
        Ok(MetadataReadReply::NotLive) => None,
        Err(_) => Some(Err("metadata read lane timed out".into())),
    }
}

/// Reply `NotLive` to every queued live command — called from every monitor state
/// that can't execute on a healthy session, so senders never hang and fall back
/// to the classic path promptly.
#[cfg(target_os = "macos")]
fn drain_not_live(rx: &Receiver<LiveCmd>) {
    while let Ok(cmd) = rx.try_recv() {
        let _ = cmd.reply.send(LiveReply::NotLive);
    }
}

#[cfg(target_os = "macos")]
fn drain_metadata_not_live(rx: &Receiver<MetadataReadCmd>) {
    while let Ok(cmd) = rx.try_recv() {
        let _ = cmd.reply.send(MetadataReadReply::NotLive);
    }
}

/// Execute one live op on the monitor's session — `send_and_collect` (accumulator
/// preserved) so the triggered pushes (`PresetLoaded`, field-3, `SceneLoaded`) are
/// decoded by the normal pump-loop pass right after.
#[cfg(target_os = "macos")]
fn exec_live(session: &mut Session, op: &LiveOp) -> Result<(), String> {
    match op {
        // Same wire form as `session::load_preset` / the scene-scan precedent:
        // device slot = list index + 1, tabEnum 1 = My Presets.
        LiveOp::LoadPreset(idx) => session.send_and_collect(
            &crate::proto::load_preset(*idx as u64 + 1, 1),
            LIVE_OP_PUMP_MS,
        ),
        // Explicit slot emit even for 0 (the device ignores an empty LoadScene{}).
        LiveOp::LoadScene(slot) => {
            session.send_and_collect(&crate::proto::load_scene(*slot as u64), LIVE_OP_PUMP_MS)
        }
    }
}

#[cfg(target_os = "macos")]
fn exec_metadata_read(
    session: &mut Session,
    cmd: &MetadataReadCmd,
) -> Result<Option<Vec<u8>>, String> {
    // Live-controller session: skip the `connection_request` re-arm (the session
    // is already armed by its dense heartbeat — re-arming draws a `connectionError`)
    // and keep that heartbeat alive through the harvest.
    session.read_slot_preset_json_live(cmd.list_index + 1)
}

/// RAII guard for [`MONITOR_SESSION_LIVE`]: constructed when [`pump_loop`] starts
/// pumping a healthy session, cleared on every exit path (incl. panics).
#[cfg(target_os = "macos")]
struct LiveFlagGuard;

#[cfg(target_os = "macos")]
impl LiveFlagGuard {
    fn arm() -> Self {
        MONITOR_SESSION_LIVE.store(true, SeqCst);
        LiveFlagGuard
    }
}

#[cfg(target_os = "macos")]
impl Drop for LiveFlagGuard {
    fn drop(&mut self) {
        MONITOR_SESSION_LIVE.store(false, SeqCst);
    }
}

/// Event names the frontend listens for (`@tauri-apps/api/event`), mirroring
/// `watcher.rs`'s `EVT_ATTACHED`/`EVT_DETACHED`.
pub const EVT_LIVE_PRESET: &str = "tmp://live-preset";
pub const EVT_LIVE_SCENE: &str = "tmp://live-scene";
pub const EVT_SCENE_LIST: &str = "tmp://scene-list";
pub const EVT_SIGNAL_CHAIN: &str = "tmp://signal-chain";
pub const EVT_SYNC: &str = "tmp://sync";

/// `tmp://live-preset` — the active preset's identity, coalesced from
/// `PresetLoaded(11)` (list index) + `CurrentPresetInfoChanged(22)` (name/dirty/fav).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LivePreset {
    /// 0-based My-Presets list index (`PresetLoaded.presetSlot − 1`); `None` when
    /// the active preset isn't a My-Presets slot (factory / song context) or before
    /// the first `PresetLoaded` push.
    list_index: Option<u32>,
    name: String,
    is_dirty: bool,
    is_favorite: bool,
}

/// `tmp://live-scene` — the unit's current scene. Emitted twice per change, both
/// honest: the `SceneLoaded(102)` echo (fast path — its embedded `sceneJson.sceneName`
/// is authoritative for the NAME, but its `sceneSlot` is NOT a reliable `scenes[]`
/// index for FS scenes: HW-observed `{sceneSlot:0, "Dist"}` while Dist sat at
/// `scenes[5]`), then the field-3 `lastLoadedScene` (authoritative for the INDEX —
/// same document as the scene names, so the spaces agree by construction; last-writer
/// wins in the UI).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveScene {
    /// `"base"` when the scene is the base scene (the constant wire slot
    /// [`session::BASE_SCENE_SLOT`]), else the numeric FS scene index (0-based into
    /// `scenes[]`). Serializes untagged: a string OR a number — the UI's sole
    /// base-vs-FS discriminator and row key.
    key: SceneKey,
    /// Scene display name — from `SceneLoaded.sceneJson` (fast path) or
    /// `scenes[lastLoadedScene]` (field-3 path); `None` if absent / truncated (the
    /// row renders regardless).
    name: Option<String>,
}

/// Base-vs-FS-scene discriminator. Serializes to `"base"` or a bare number so the
/// frontend's `key: "base" | number` union matches by construction.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum SceneKey {
    /// `"base"` — the wire's constant base slot (8), even for 0-FS-scene presets.
    Base(BaseTag),
    /// A 0-based FS scene index into `scenes[]`.
    Index(u32),
}

/// Unit type that always serializes to the string `"base"` (a serde untagged enum
/// can't carry a literal, so a one-variant enum gives the constant).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
enum BaseTag {
    Base,
}

/// `tmp://scene-list` — the active preset's live scene rows. CANONICAL source: the
/// field-3 `currentPresetDataChanged` push (arrives on EVERY device change AND in the
/// connect handshake), whose preset JSON carries `scenes[].sceneName` in slot order +
/// the `ftsw` map from the same document. `sceneListResponse(125)` is only a fast-paint
/// top-up — the unit pushes it solely on an actual preset SWITCH while we're listening
/// (NOT on scene taps, NOT when the preset was already active — HW-observed),
/// so a UI fed by 125 alone stays empty on a mid-preset connect. `fs` is the real
/// assigned switch; `null` when the scene has no active footswitch (em-dash).
#[derive(Debug, Clone, Serialize)]
struct SceneListPayload {
    scenes: Vec<SceneListRow>,
}

#[derive(Debug, Clone, Serialize)]
struct SceneListRow {
    name: String,
    /// The footswitch (1-based, human "FS1..FS8") this scene is recalled by, or
    /// `null` when no active switch is assigned. Derived from the live `ftsw` map
    /// (`footswitch::scene_fs_map`) — `index + 1`.
    fs: Option<u32>,
}

/// `tmp://sync` — a device-push / (re)connect is in flight; the UI shows the neutral
/// catching-up state until the first real state lands.
#[derive(Debug, Clone, Serialize)]
struct SyncPayload {
    syncing: bool,
}

/// Per-monitor coalescing cache. `PresetLoaded(11)` and `CurrentPresetInfoChanged(22)`
/// arrive independently; the handoff renders them as ONE row, so we hold both halves
/// and emit a merged `LivePreset` on each half's arrival. The scene caches are owned
/// by the field-3 decode (wholesale-replaced on every push that carries them — the
/// dense-heartbeat field-3 is effectively always complete, so the cache is memoryless
/// and self-healing); `sceneListResponse(125)` only tops up the names.
#[derive(Default)]
struct LiveCache {
    list_index: Option<u32>,
    name: String,
    is_dirty: bool,
    is_favorite: bool,
    /// The active preset's scene names in `scenes[]` slot order (canonical: the
    /// field-3 preset JSON; top-up: `sceneListResponse`). Row index = wire sceneSlot.
    scene_names: Vec<String>,
    /// `sceneSlot(0-based `scenes[]` index) → footswitch index(0-based)`, parsed from
    /// the field-3 `ftsw` (same document as `scene_names`, so the spaces agree).
    /// `None` until a field-3 carrying `ftsw` is seen.
    ftsw_map: Option<std::collections::HashMap<u32, u32>>,
    /// The last `lastLoadedScene` emitted from a field-3 (de-dupes the live-scene
    /// re-emit across the many field-3 pushes that don't change the scene). Cleared
    /// on `PresetLoaded` (it's per-preset state).
    last_scene: Option<u32>,
    /// When a scenes-bearing field-3 last refreshed `scene_names` — the staleness
    /// test for the `PresetLoaded` guard below. `None` until the first one.
    scene_doc_at: Option<std::time::Instant>,
}

/// How recent a scenes-bearing field-3 must be, at `PresetLoaded` time, for the
/// scene caches to be trusted as belonging to the NEW preset. In the hardware's
/// preset-switch burst the field-3 precedes `PresetLoaded` by well under a second;
/// anything older means the caches describe the PREVIOUS preset (e.g. the monitor
/// reconnected after a command-driven switch and the handshake field-3 was lean) —
/// keep them and the UI would show (and CLICK-ROUTE BY) the old preset's rows.
#[cfg(target_os = "macos")]
const SCENE_DOC_FRESH_MS: u128 = 1000;

/// Spawn the monitor thread. Lives for the whole process; never joined (like the
/// `watcher`). Mirrors `watcher::spawn`'s `(app, session)` signature. The `session`
/// arc is the shared `AppState.session` — the monitor READS it (`is_none()`) to prove
/// it can own the seize, and never holds it across an open (it owns its OWN `Session`).
/// Idle until app-level `connect_device` sets `MONITOR_ENABLED`.
#[cfg(target_os = "macos")]
pub fn spawn(app: tauri::AppHandle, session: Arc<Mutex<Option<Session>>>) {
    // Install the live command lane before the thread starts so `try_live_op`
    // can never observe a live flag without a sender.
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<LiveCmd>();
    let (metadata_tx, metadata_rx) = crossbeam_channel::unbounded::<MetadataReadCmd>();
    let _ = LIVE_CMD_TX.set(cmd_tx);
    let _ = METADATA_READ_TX.set(metadata_tx);
    std::thread::Builder::new()
        .name("tmp-device-monitor".into())
        .spawn(move || monitor_loop(app, session, cmd_rx, metadata_rx))
        .expect("spawn tmp-device-monitor");
}

#[cfg(not(target_os = "macos"))]
pub fn spawn(_app: tauri::AppHandle, _session: Arc<Mutex<Option<Session>>>) {}

/// How long to pump each inner iteration (ms). Short enough that the pause flag is
/// checked ~8×/sec and a ~250 ms heartbeat lands close to Pro Control's 4/sec.
#[cfg(target_os = "macos")]
const PUMP_MS: u64 = 120;
/// Heartbeat cadence (ms). The proven keepalive that keeps pushes flowing without
/// the session lapsing into `connectionError` (a 10 s cadence let it lapse).
#[cfg(target_os = "macos")]
const HEARTBEAT_MS: u64 = 250;
/// Backoff between reconnect attempts when no device is present / the UI session
/// holds the seize (ms).
#[cfg(target_os = "macos")]
const RECONNECT_BACKOFF_MS: u64 = 300;
/// Idle-poll cadence while live-sync is disabled (the default) — cheap flag check,
/// no device I/O (ms).
#[cfg(target_os = "macos")]
const DISABLED_POLL_MS: u64 = 200;

/// Bounded warmup after the first handshake if its push bodies did not include a
/// usable graph. Short enough to keep startup bounded; the final fallback is the
/// non-destructive field-8 active-slot read.
#[cfg(target_os = "macos")]
const STARTUP_GRAPH_WARMUP_STEPS: u32 = 8;
#[cfg(target_os = "macos")]
const STARTUP_GRAPH_WARMUP_MS: u64 = 120;

/// Re-snapshot retries when a connect lands with `graph=none` (every in-session
/// fallback exhausted — a congested handshake can miss the field-3 push AND the
/// PresetLoaded body the field-8 fallback needs). An IDLE device never pushes
/// field-3 on its own, so without a retry the hero stays "No active preset"
/// until the user touches the amp. A fresh handshake after a short backoff is
/// the proven graph source; bounded so a pathological unit can't reconnect-loop.
#[cfg(target_os = "macos")]
const GRAPH_RETRY_MAX: u32 = 2;
#[cfg(target_os = "macos")]
const GRAPH_RETRY_BACKOFF_MS: u64 = 3000;

#[cfg(target_os = "macos")]
enum PumpExit {
    Disabled,
    Paused,
    Error,
}

#[cfg(target_os = "macos")]
fn monitor_loop(
    app: tauri::AppHandle,
    session_arc: Arc<Mutex<Option<Session>>>,
    live_rx: Receiver<LiveCmd>,
    metadata_rx: Receiver<MetadataReadCmd>,
) {
    let cache = Arc::new(Mutex::new(LiveCache::default()));
    let mut last_block = "";
    let mut log_block = |b: &'static str| {
        if b != last_block {
            log::info!("monitor: blocked on '{b}'");
            last_block = b;
        }
    };
    // Budget for graph=none re-snapshot retries; refilled on a graph-bearing
    // snapshot and on every deliberate op cycle (pause/resume reconnect).
    let mut graph_retries: u32 = 0;
    loop {
        // Contain a panic to ONE iteration: the monitor must never die while
        // MONITOR_ENABLED (a dead loop would wedge every device op on the pause-ack wait
        // forever). lock_ok already removes the poisoned-lock panic vector; this catches
        // anything else (e.g. a decode/emit bug on a malformed device message).
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_monitor_iteration(
                &app,
                &session_arc,
                &cache,
                &live_rx,
                &metadata_rx,
                &mut graph_retries,
                &mut log_block,
            );
        }));
        if outcome.is_err() {
            log::error!("monitor: loop iteration panicked — recovered; monitor stays alive");
            graph_retries = 0;
            sleep(RECONNECT_BACKOFF_MS);
        }
    }
}

/// ONE iteration of the monitor loop, factored out so [`monitor_loop`] can run it under
/// `catch_unwind`. A `return` here ends the iteration; the outer `loop` continues (so the
/// bodies below use `return` where the inline loop used `continue`).
fn run_monitor_iteration(
    app: &tauri::AppHandle,
    session_arc: &Arc<Mutex<Option<Session>>>,
    cache: &Arc<Mutex<LiveCache>>,
    live_rx: &Receiver<LiveCmd>,
    metadata_rx: &Receiver<MetadataReadCmd>,
    graph_retries: &mut u32,
    log_block: &mut impl FnMut(&'static str),
) {
    // Disabled (default): live-sync not started. Idle-poll; do nothing.
    if !MONITOR_ENABLED.load(SeqCst) {
        log_block("disabled");
        drain_not_live(live_rx);
        drain_metadata_not_live(metadata_rx);
        sleep(DISABLED_POLL_MS);
        return;
    }
    // Paused: a command holds the device. Make sure we hold no Session, ack, and
    // park until the request clears. (Re-checked at the top of every reconnect.)
    if MONITOR_PAUSE_REQ.load(SeqCst) {
        log_block("paused");
        park_while_paused(app, live_rx, metadata_rx);
        return;
    }
    // Opportunistic ownership: only open HID when no persistent UI session is
    // holding the seize. `connect_device` releases that session before enabling us,
    // but a diagnostic stop/reconnect or command bookend can momentarily re-take it.
    if crate::lock_ok(session_arc).is_some() {
        log_block("ui-session-held");
        drain_not_live(live_rx);
        drain_metadata_not_live(metadata_rx);
        sleep(RECONNECT_BACKOFF_MS);
        return;
    }
    log_block("connecting");
    // Connect with the firmware request riding the same startup handshake that
    // carries the preset list and usually the active field-3 graph.
    emit_sync(app, true);
    let mut session = match Session::connect_with_firmware() {
        Ok(s) => s,
        Err(e) => {
            // No device / lost the open race to a command window. Back off and
            // retry; on detach this is the steady state until replug. (The
            // watcher remains the authoritative attach/detach signal for the UI.)
            log::warn!("monitor: connect failed ({e}) — backing off");
            store_connect_error(e);
            drain_not_live(live_rx);
            drain_metadata_not_live(metadata_rx);
            sleep(RECONNECT_BACKOFF_MS);
            return;
        }
    };
    // The lean handshake's own streams already carry the current
    // PresetLoaded/InfoChanged + field-3 graph — decode + emit as the first
    // state, then clear `sync`.
    let mut seen = 0usize;
    {
        let mut c = crate::lock_ok(cache);
        let bodies = session.push_bodies();
        log::info!(
            "monitor: connected, decoding {} handshake bodies",
            bodies.len()
        );
        for body in &bodies {
            decode_and_emit(app, body, &mut c);
            seen += 1;
        }
    }
    let snapshot = assemble_startup_snapshot(&mut session);
    match snapshot {
        Ok((snapshot, startup_live, reset_seen)) => {
            let graph_missing = snapshot.graph.is_none();
            log::info!(
                "monitor: startup snapshot firmware={:?} presets={} graph={}",
                snapshot.firmware,
                snapshot.presets.len(),
                if graph_missing { "none" } else { "ok" }
            );
            store_startup_snapshot(snapshot);
            if reset_seen {
                seen = 0;
            }
            if graph_missing && *graph_retries < GRAPH_RETRY_MAX {
                // Every in-session fallback failed (congested handshake) and an
                // idle device will never push field-3 on its own — drop the
                // session, let the congestion clear, and re-handshake. The
                // stored snapshot keeps serving the list meanwhile; a graph-ok
                // retry replaces it and its handshake decode emits the graph.
                *graph_retries += 1;
                log::warn!(
                    "monitor: snapshot has no graph — re-snapshot retry \
                     {graph_retries}/{GRAPH_RETRY_MAX} in {GRAPH_RETRY_BACKOFF_MS} ms"
                );
                drop(session);
                emit_sync(app, false);
                drain_not_live(live_rx);
                drain_metadata_not_live(metadata_rx);
                sleep(GRAPH_RETRY_BACKOFF_MS);
                return;
            }
            if !graph_missing {
                *graph_retries = 0;
            }
            if let Some(live) = startup_live {
                let mut c = crate::lock_ok(cache);
                emit_startup_live(app, &mut c, live);
            }
        }
        Err(e) => {
            log::warn!("monitor: startup snapshot failed ({e})");
            store_connect_error(e);
        }
    }
    emit_sync(app, false);

    match pump_loop(app, session, cache, seen, live_rx, metadata_rx) {
        PumpExit::Disabled => clear_startup_snapshot(),
        PumpExit::Paused => {
            // A deliberate op cycle reconnects next — fresh retry budget.
            *graph_retries = 0;
        }
        PumpExit::Error => clear_startup_snapshot(),
    }
    // pump_loop returns when the device errors, live-sync stops, or a pause is
    // requested; the outer loop then re-checks pause → reconnect → re-emit fresh state.
}

/// Drop any held Session, ack the pause, show the neutral state, and park until the
/// command's guard clears `MONITOR_PAUSE_REQ`. (No Session is held here — the caller
/// already dropped it — but we still ack so the command isn't stranded.) Keeps the
/// live command lane drained so a raced sender falls back instead of hanging.
#[cfg(target_os = "macos")]
fn park_while_paused(
    app: &tauri::AppHandle,
    live_rx: &Receiver<LiveCmd>,
    metadata_rx: &Receiver<MetadataReadCmd>,
) {
    emit_sync(app, true);
    MONITOR_PAUSED_ACK.store(true, SeqCst);
    while MONITOR_PAUSE_REQ.load(SeqCst) {
        drain_not_live(live_rx);
        drain_metadata_not_live(metadata_rx);
        sleep(PAUSE_WAIT_STEP);
    }
    MONITOR_PAUSED_ACK.store(false, SeqCst);
}

/// Step granularity (ms) for the pause-park spin — matches the command-side wait so
/// the resume lands promptly.
#[cfg(target_os = "macos")]
const PAUSE_WAIT_STEP: u64 = 25;

#[cfg(target_os = "macos")]
fn assemble_startup_snapshot(
    session: &mut Session,
) -> Result<(StartupSnapshot, Option<session::CurrentPresetLive>, bool), String> {
    let firmware = session.firmware_version();
    // Strict (completeness-validated) list: the tolerant harvest silently accepted
    // a tail-truncated 371-of-504 list when this reconnect ran right after a heavy
    // sweep congested the device.
    let presets = session.list_my_presets_strict()?;
    let (live, reset_seen) = startup_live(session);
    let graph = live.as_ref().and_then(|l| l.graph.clone());
    Ok((
        StartupSnapshot {
            firmware,
            presets,
            graph,
        },
        live,
        reset_seen,
    ))
}

#[cfg(target_os = "macos")]
fn startup_live(session: &mut Session) -> (Option<session::CurrentPresetLive>, bool) {
    if let Some(live) = live_from_pushes(session) {
        return (Some(live), false);
    }
    for _ in 0..STARTUP_GRAPH_WARMUP_STEPS {
        if session.heartbeat().is_err() || session.pump_more(STARTUP_GRAPH_WARMUP_MS).is_err() {
            return (None, false);
        }
        if let Some(live) = live_from_pushes(session) {
            return (Some(live), false);
        }
    }
    let Some(active_slot) = session.loaded_slot() else {
        return (None, false);
    };
    // Live-controller session: skip the re-arm (already armed by the handshake's
    // heartbeat cadence) so the fallback doesn't draw a `connectionError`, and keep
    // the heartbeat alive through the read.
    match session.read_slot_preset_json_live(active_slot + 1) {
        Ok(Some(json)) => {
            let Some(mut live) = session::decode_plain_preset_live(&json) else {
                return (None, true);
            };
            let Some(graph) = live.graph.as_mut() else {
                return (None, true);
            };
            if graph.slot.is_none() {
                graph.slot = Some(active_slot);
            }
            (Some(live), true)
        }
        Ok(None) => (None, true),
        Err(e) => {
            log::warn!("monitor: startup field-8 graph fallback failed ({e})");
            (None, true)
        }
    }
}

#[cfg(target_os = "macos")]
fn live_from_pushes(session: &Session) -> Option<session::CurrentPresetLive> {
    let active_slot = session.loaded_slot();
    for body in session.push_bodies().iter().rev() {
        let Some(live) = session::decode_current_preset_live(body) else {
            continue;
        };
        let mut live = live;
        let Some(graph) = live.graph.as_mut() else {
            continue;
        };
        if graph.slot.is_none() {
            graph.slot = active_slot;
        }
        return Some(live);
    }
    None
}

/// Inner pump loop: drain + decode + emit pushes, firing the heartbeat every
/// `HEARTBEAT_MS`. Mirrors `Session::listen_dump` but emits Tauri events instead of
/// printing and yields the device promptly on a pause request. Returns on any HID
/// error (device gone / seize lost) or when a pause is requested.
#[cfg(target_os = "macos")]
fn pump_loop(
    app: &tauri::AppHandle,
    mut session: Session,
    cache: &Arc<Mutex<LiveCache>>,
    mut seen: usize,
    live_rx: &Receiver<LiveCmd>,
    metadata_rx: &Receiver<MetadataReadCmd>,
) -> PumpExit {
    let _live = LiveFlagGuard::arm(); // sender-side gate for the live command lane
    let mut last_hb = std::time::Instant::now();
    loop {
        // Live-sync stopped: drop our Session (free the seize) so `stop_live_sync` can
        // re-establish the persistent UI session. Return to the (disabled) outer loop.
        if !MONITOR_ENABLED.load(SeqCst) {
            drop(session);
            return PumpExit::Disabled;
        }
        // A command wants the device: drop our Session (free the seize), ack, and
        // return to the outer loop which parks until the command's guard clears.
        if MONITOR_PAUSE_REQ.load(SeqCst) {
            drop(session);
            // Clear the live gate BEFORE parking so senders racing the park see it.
            drop(_live);
            park_while_paused(app, live_rx, metadata_rx);
            return PumpExit::Paused;
        }
        // Execute queued live-lane ops on the healthy session (between pumps — the
        // pushes they trigger land in the same accumulator and are decoded below).
        while let Ok(cmd) = live_rx.try_recv() {
            let r = exec_live(&mut session, &cmd.op);
            let failed = r.is_err();
            let _ = cmd.reply.send(LiveReply::Done(r));
            if failed {
                return PumpExit::Error; // device error — reconnect from the top (guard clears the gate)
            }
        }
        while let Ok(cmd) = metadata_rx.try_recv() {
            let r = exec_metadata_read(&mut session, &cmd);
            let failed = r.is_err();
            let _ = cmd.reply.send(MetadataReadReply::Done(r));
            // `read_slot_preset_json` clears the accumulator before harvesting field-9.
            // Restart the seen cursor so later monitor pushes are decoded normally.
            seen = 0;
            if failed {
                return PumpExit::Error;
            }
        }
        if session.pump_more(PUMP_MS).is_err() {
            return PumpExit::Error; // device gone / seize lost — reconnect from the top
        }
        let bodies = session.push_bodies();
        // Decode every NEWLY-completed stream except the newest (it may still be
        // growing — same "print all but the last" rule as `listen_dump`).
        if bodies.len() > seen + 1 {
            let mut c = crate::lock_ok(cache);
            while seen + 1 < bodies.len() {
                decode_and_emit(app, &bodies[seen], &mut c);
                seen += 1;
            }
        }
        if last_hb.elapsed().as_millis() as u64 >= HEARTBEAT_MS {
            if session.heartbeat().is_err() {
                return PumpExit::Error;
            }
            last_hb = std::time::Instant::now();
        }
    }
}

/// Decode one inbound stream body and emit the matching Tauri event(s). Reuses the
/// shared `session::decode_*` push decoders (no second parser). Updates the coalescing
/// cache for `live-preset` and the base-slot classifier.
#[cfg(target_os = "macos")]
/// Emit `tmp://scene-list` from the cached scene names, tagging each row with its
/// real footswitch (`ftsw_map`, displayed 1-based) or `null` (em-dash). Called when
/// the scene-list push arrives AND re-called when a later field-3 `ftsw` fills the
/// map (so the tags appear regardless of push order).
fn emit_scene_list(app: &tauri::AppHandle, cache: &LiveCache) {
    use tauri::Emitter;
    let _ = app.emit(EVT_SCENE_LIST, scene_list_payload(cache));
}

fn scene_list_payload(cache: &LiveCache) -> SceneListPayload {
    let scenes = cache
        .scene_names
        .iter()
        .enumerate()
        .map(|(i, name)| SceneListRow {
            name: name.clone(),
            fs: cache
                .ftsw_map
                .as_ref()
                .and_then(|m| m.get(&(i as u32)))
                .map(|sw| sw + 1),
        })
        .collect();
    SceneListPayload { scenes }
}

fn live_scene_from_slot(scene_slot: u32, scene_names: &[String]) -> LiveScene {
    let (key, name) = if scene_slot >= session::BASE_SCENE_SLOT {
        (SceneKey::Base(BaseTag::Base), None)
    } else {
        (
            SceneKey::Index(scene_slot),
            scene_names.get(scene_slot as usize).cloned(),
        )
    };
    LiveScene { key, name }
}

fn apply_startup_live(
    cache: &mut LiveCache,
    live: session::CurrentPresetLive,
) -> AppliedStartupLive {
    let mut rows_changed = false;
    if let Some(names) = live.scene_names {
        if cache.scene_names != names {
            log::info!("monitor: startup document scene order = {names:?}");
        }
        cache.scene_names = names;
        cache.scene_doc_at = Some(std::time::Instant::now());
        rows_changed = true;
    }
    if let Some(ftsw) = &live.ftsw {
        let map = footswitch::scene_fs_map(ftsw);
        rows_changed |= cache.ftsw_map.as_ref() != Some(&map);
        cache.ftsw_map = Some(map);
    }
    let scene_list = rows_changed.then(|| scene_list_payload(cache));
    let live_scene = live.last_loaded_scene.map(|lls| {
        cache.last_scene = Some(lls);
        let scene = live_scene_from_slot(lls, &cache.scene_names);
        log::info!(
            "monitor: startup lastLoadedScene={lls} name={:?}",
            scene.name
        );
        scene
    });

    AppliedStartupLive {
        scene_list,
        live_scene,
        signal_chain: live.graph,
    }
}

fn emit_startup_live(
    app: &tauri::AppHandle,
    cache: &mut LiveCache,
    live: session::CurrentPresetLive,
) {
    use tauri::Emitter;
    let applied = apply_startup_live(cache, live);
    if let Some(payload) = applied.scene_list {
        let _ = app.emit(EVT_SCENE_LIST, payload);
    }
    if let Some(scene) = applied.live_scene {
        let _ = app.emit(EVT_LIVE_SCENE, scene);
    }
    if let Some(graph) = applied.signal_chain {
        refresh_snapshot_graph(graph.clone());
        let _ = app.emit(EVT_SIGNAL_CHAIN, graph);
    }
}

fn decode_and_emit(app: &tauri::AppHandle, body: &[u8], cache: &mut LiveCache) {
    use tauri::Emitter;

    // PresetLoaded(11) → live-preset (list index half).
    if let Some(pl) = session::decode_preset_loaded(body) {
        // tabEnum 1 = My Presets; presetSlot is 1-based → 0-based list index.
        // Other tabs (factory / song context) ⇒ no My-Presets list index.
        let new_index = if pl.tab_enum == 1 {
            pl.preset_slot.checked_sub(1).map(|s| s as u32)
        } else {
            None
        };
        // `lastLoadedScene` is per-preset state → forget it so the new preset's first
        // field-3 re-emits the live scene.
        cache.last_scene = None;
        // Scene-cache freshness guard: in the hardware switch burst the new preset's
        // field-3 precedes this PresetLoaded (so the caches are already the NEW
        // preset's — keep them). But when the active preset CHANGED and no
        // scenes-bearing field-3 landed recently (a monitor reconnect after a
        // command-driven switch whose handshake field-3 was lean), the caches still
        // describe the PREVIOUS preset — showing them would also CLICK-ROUTE by the
        // wrong rows. Clear + emit empty (honest blank until real data arrives).
        let changed = cache.list_index != new_index;
        let doc_stale = cache
            .scene_doc_at
            .is_none_or(|at| at.elapsed().as_millis() > SCENE_DOC_FRESH_MS);
        if changed && doc_stale && !cache.scene_names.is_empty() {
            log::info!("monitor: preset changed with stale scene doc → clearing scene rows");
            cache.scene_names.clear();
            cache.ftsw_map = None;
            emit_scene_list(app, cache);
        }
        cache.list_index = new_index;
        let _ = app.emit(EVT_LIVE_PRESET, cache.live_preset());
        return;
    }
    // CurrentPresetInfoChanged(22) → live-preset (name/dirty/fav half).
    if let Some((name, is_dirty, is_favorite)) = session::decode_info_changed(body) {
        cache.name = name;
        cache.is_dirty = is_dirty;
        cache.is_favorite = is_favorite;
        let _ = app.emit(EVT_LIVE_PRESET, cache.live_preset());
        return;
    }
    // sceneListResponse(125) → scene-list top-up. Only pushed on an actual preset
    // SWITCH while listening; the canonical row source is the field-3 branch below.
    // FS tags come from the cached `ftsw_map` (same-document field-3); if the 125
    // arrives first, tags are null until the field-3 re-emit ~200 ms later.
    if let Some(names) = session::decode_scene_list(body) {
        log::info!(
            "monitor: scene-list top-up ({} names) = {names:?}",
            names.len()
        );
        cache.scene_names = names;
        emit_scene_list(app, cache);
        return;
    }
    // SceneLoaded(102) → live-scene fast path. The embedded `sceneJson.sceneName` is
    // the authoritative NAME; the echo's `sceneSlot` is only trusted for the base
    // test (>= BASE_SCENE_SLOT) — for FS scenes it does NOT reliably index `scenes[]`
    // (HW-observed `{sceneSlot:0, "Dist"}` with Dist at `scenes[5]`). The field-3
    // that follows re-emits with the authoritative index (`lastLoadedScene`).
    if let Some((scene_slot, name)) = session::decode_scene_loaded(body) {
        log::info!("monitor: SceneLoaded sceneSlot={scene_slot} name={name:?}");
        let key = if scene_slot >= session::BASE_SCENE_SLOT {
            SceneKey::Base(BaseTag::Base)
        } else {
            SceneKey::Index(scene_slot)
        };
        let _ = app.emit(EVT_LIVE_SCENE, LiveScene { key, name });
        return;
    }
    // currentPresetDataChanged(3) — the CANONICAL scene source: one preset-JSON
    // document carrying `scenes[].sceneName` (rows), `lastLoadedScene` (live scene)
    // and `ftsw` (FS tags), plus the signal-chain graph. Each part wholesale-replaces
    // its cache when present (a healthy dense-heartbeat field-3 carries them all;
    // `scenes` truncates only inside the final scene's `uuid`).
    if let Some(live) = session::decode_current_preset_live(body) {
        let mut rows_changed = false;
        if let Some(names) = live.scene_names {
            if cache.scene_names != names {
                rows_changed = true;
                log::info!("monitor: field-3 document scene order = {names:?}");
            }
            cache.scene_names = names;
            cache.scene_doc_at = Some(std::time::Instant::now());
        }
        if let Some(ftsw) = &live.ftsw {
            let map = footswitch::scene_fs_map(ftsw);
            rows_changed |= cache.ftsw_map.as_ref() != Some(&map);
            cache.ftsw_map = Some(map);
        }
        if rows_changed {
            emit_scene_list(app, cache);
        }
        // The authoritative live scene (same document as the rows, so the index
        // space agrees by construction). De-duped against the last emit.
        if let Some(lls) = live.last_loaded_scene {
            if cache.last_scene != Some(lls) {
                cache.last_scene = Some(lls);
                let scene = live_scene_from_slot(lls, &cache.scene_names);
                log::info!(
                    "monitor: field-3 lastLoadedScene={lls} name={:?}",
                    scene.name
                );
                let _ = app.emit(EVT_LIVE_SCENE, scene);
            }
        }
        // The graph came from the SAME decode (no second parse of the ~17 KB body).
        log::debug!(
            "monitor: field-3 → rows_changed={rows_changed} graph={}",
            live.graph.is_some()
        );
        if let Some(graph) = live.graph {
            refresh_snapshot_graph(graph.clone());
            let _ = app.emit(EVT_SIGNAL_CHAIN, graph);
        }
        return;
    }
    // ConnectionMessage error (4[3]) should be ZERO at 250 ms. If one appears the
    // session is lapsing — log once at warn, never spam the UI.
    if let Some(code) = decode_connection_error(body) {
        log::warn!("monitor: connectionError (session lapsing?) hex={code}");
        return;
    }
    // An unhandled body — debug-log its presetMessage(2) inner field numbers (this
    // is how SetFootswitchAssignment(54)/AllBlockPresetsResponse(136) were identified).
    let top = crate::proto::parse(body);
    if let Some(pm) = crate::proto::first_bytes(&top, 2) {
        let inner: Vec<u32> = crate::proto::parse(pm).iter().map(|(f, _)| *f).collect();
        log::debug!(
            "monitor: unhandled presetMessage body, inner fields={inner:?} ({}B)",
            body.len()
        );
    } else {
        let fields: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
        log::debug!(
            "monitor: unhandled body, top fields={fields:?} ({}B)",
            body.len()
        );
    }
}

/// True-ish: extract a hex of a `ConnectionMessage.connectionError` (TMS field 4,
/// inner field 3) body for the lapse warning, else None.
#[cfg(target_os = "macos")]
fn decode_connection_error(body: &[u8]) -> Option<String> {
    let top = crate::proto::parse(body);
    let cm = crate::proto::first_bytes(&top, 4)?;
    let inner = crate::proto::parse(cm);
    // connectionError is inner field 3.
    if inner.iter().any(|(f, _)| *f == 3) {
        Some(body.iter().take(16).map(|b| format!("{b:02x}")).collect())
    } else {
        None
    }
}

impl LiveCache {
    fn live_preset(&self) -> LivePreset {
        LivePreset {
            list_index: self.list_index,
            name: self.name.clone(),
            is_dirty: self.is_dirty,
            is_favorite: self.is_favorite,
        }
    }
}

#[cfg(target_os = "macos")]
fn emit_sync(app: &tauri::AppHandle, syncing: bool) {
    use tauri::Emitter;
    let _ = app.emit(EVT_SYNC, SyncPayload { syncing });
}

#[cfg(target_os = "macos")]
fn sleep(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

#[cfg(test)]
mod tests {
    use super::*;

    // The base-slot classifier: sceneSlot >= BASE_SCENE_SLOT (the wire constant 8)
    // ⇒ base. Pure logic, mirroring `decode_and_emit`'s test (no device, no AppHandle).
    fn classify(scene_slot: u32) -> SceneKey {
        if scene_slot >= session::BASE_SCENE_SLOT {
            SceneKey::Base(BaseTag::Base)
        } else {
            SceneKey::Index(scene_slot)
        }
    }

    fn key_str(k: &SceneKey) -> String {
        serde_json::to_string(k).unwrap()
    }

    fn graph(name: &str) -> ActiveGraph {
        ActiveGraph {
            name: Some(name.to_string()),
            slot: None,
            template: None,
            split_mix: None,
            nodes: Vec::new(),
            input_type: Some("guitar".to_string()),
            output_type: Some("out".to_string()),
            inputs: None,
            outputs: None,
            lanes: None,
            stages: Vec::new(),
        }
    }

    #[test]
    fn base_slot_classifier_uses_the_wire_constant() {
        // Base is the CONSTANT wire slot 8 — even for a 0-FS-scene preset (HW: Cello,
        // 0 scenes, base SceneLoaded sceneSlot=8) and for a full 8-FS-scene preset
        // (Guitar, lastLoadedScene=8 on base). NOT scene_count+1 (refuted theory —
        // the old `slot > count` classifier missed base at slot 8 with count 8).
        assert_eq!(key_str(&classify(8)), "\"base\"");
        assert_eq!(key_str(&classify(7)), "7");
        assert_eq!(key_str(&classify(0)), "0");
    }

    #[test]
    fn scene_key_serializes_untagged() {
        // "base" is a bare string; an index is a bare number — matching the
        // frontend's `key: "base" | number` union.
        assert_eq!(
            serde_json::to_string(&SceneKey::Base(BaseTag::Base)).unwrap(),
            "\"base\""
        );
        assert_eq!(serde_json::to_string(&SceneKey::Index(3)).unwrap(), "3");
    }

    #[test]
    fn live_preset_coalesces_both_halves() {
        // Only the (22) half arrived: list_index stays None, name/dirty/fav set.
        let mut c = LiveCache {
            name: "Lead".into(),
            is_dirty: true,
            ..Default::default()
        };
        let lp = c.live_preset();
        assert_eq!(lp.list_index, None);
        assert_eq!(lp.name, "Lead");
        assert!(lp.is_dirty);
        assert!(!lp.is_favorite);
        // Then the (11) half arrives (presetSlot 12 → list index 11).
        c.list_index = Some(11);
        let lp = c.live_preset();
        assert_eq!(lp.list_index, Some(11));
        assert_eq!(lp.name, "Lead"); // name half preserved
    }

    #[test]
    fn live_preset_payload_serializes_camel_for_the_frontend() {
        // The contract is camelCase keys (listIndex/isDirty/isFavorite).
        let lp = LivePreset {
            list_index: Some(3),
            name: "X".into(),
            is_dirty: false,
            is_favorite: true,
        };
        let j = serde_json::to_string(&lp).unwrap();
        assert!(j.contains("\"listIndex\":3"), "{j}");
        assert!(j.contains("\"isDirty\":false"), "{j}");
        assert!(j.contains("\"isFavorite\":true"), "{j}");
    }

    #[test]
    fn scene_list_row_serializes_null_fs_as_em_dash_seam() {
        let p = SceneListPayload {
            scenes: vec![SceneListRow {
                name: "Clean".into(),
                fs: None,
            }],
        };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"name\":\"Clean\""), "{j}");
        assert!(j.contains("\"fs\":null"), "{j}");
    }

    #[test]
    fn live_scene_serializes_camel_and_untagged_key() {
        let s = LiveScene {
            key: SceneKey::Base(BaseTag::Base),
            name: Some("Base".into()),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"key\":\"base\""), "{j}");
        assert!(j.contains("\"name\":\"Base\""), "{j}");
    }

    #[test]
    fn startup_live_applies_scene_rows_and_signal_chain() {
        let mut c = LiveCache::default();
        let applied = apply_startup_live(
            &mut c,
            session::CurrentPresetLive {
                scene_names: Some(vec!["Clean".into(), "Lead".into()]),
                last_loaded_scene: None,
                ftsw: None,
                graph: Some(graph("Guitar")),
            },
        );

        let rows = applied.scene_list.expect("scene rows emitted");
        assert_eq!(rows.scenes.len(), 2);
        assert_eq!(rows.scenes[0].name, "Clean");
        assert_eq!(rows.scenes[1].name, "Lead");
        assert_eq!(
            applied
                .signal_chain
                .expect("signal-chain emitted")
                .name
                .as_deref(),
            Some("Guitar")
        );
    }

    #[test]
    fn startup_live_scene_base_uses_wire_constant() {
        let mut c = LiveCache::default();
        let applied = apply_startup_live(
            &mut c,
            session::CurrentPresetLive {
                scene_names: Some(vec!["Clean".into()]),
                last_loaded_scene: Some(session::BASE_SCENE_SLOT),
                ftsw: None,
                graph: None,
            },
        );

        let scene = applied.live_scene.expect("live scene emitted");
        assert_eq!(key_str(&scene.key), "\"base\"");
        assert_eq!(scene.name, None);
    }

    #[test]
    fn startup_live_scene_index_uses_scene_name() {
        let mut c = LiveCache::default();
        let applied = apply_startup_live(
            &mut c,
            session::CurrentPresetLive {
                scene_names: Some(vec!["Clean".into(), "Lead".into()]),
                last_loaded_scene: Some(0),
                ftsw: None,
                graph: None,
            },
        );

        let scene = applied.live_scene.expect("live scene emitted");
        assert_eq!(key_str(&scene.key), "0");
        assert_eq!(scene.name.as_deref(), Some("Clean"));
    }

    #[test]
    fn startup_live_empty_scene_names_clear_stale_rows() {
        let mut c = LiveCache {
            scene_names: vec!["Stale".into()],
            ..Default::default()
        };
        let applied = apply_startup_live(
            &mut c,
            session::CurrentPresetLive {
                scene_names: Some(vec![]),
                last_loaded_scene: None,
                ftsw: None,
                graph: None,
            },
        );

        assert!(c.scene_names.is_empty());
        assert!(applied
            .scene_list
            .expect("empty scene-list emitted")
            .scenes
            .is_empty());
    }

    #[test]
    fn refresh_snapshot_graph_updates_only_a_stored_snapshot() {
        // Self-contained set/clear: the snapshot statics are process-global and
        // this is the only test that touches them.
        clear_startup_snapshot();
        // No snapshot stored → no-op (must not fabricate one).
        refresh_snapshot_graph(graph("Lead"));
        assert!(startup_snapshot().is_none());

        store_startup_snapshot(StartupSnapshot {
            firmware: Some("1.8.45".into()),
            presets: vec![],
            graph: Some(graph("Old")),
        });
        refresh_snapshot_graph(graph("Current"));
        let snap = startup_snapshot().expect("snapshot kept");
        assert_eq!(snap.graph.unwrap().name.as_deref(), Some("Current"));
        assert_eq!(snap.firmware.as_deref(), Some("1.8.45")); // other fields intact
        clear_startup_snapshot();
    }
}
