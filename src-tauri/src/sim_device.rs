//! In-memory fake [`HidTransport`] for end-to-end tests of the held-session edit and
//! leveling orchestration WITHOUT real hardware.
//!
//! It parses the `FenderMessageTMS` requests a [`Session`] issues and answers with
//! correctly-framed device replies (`PresetLoaded`, `nodeReplaced`/`nodeInserted`/
//! `nodeRemoved`, `presetError`, `presetLevelChanged`), while recording the wire ops in
//! order so a test can assert the exact sequence AND the save-only-on-confirm safety
//! gate. It is faithful to the load-bearing protocol facts the real device enforces:
//!
//! - structural edits confirm via `nodeReplaced`(40) / `nodeRemoved`(36) /
//!   `nodeInserted`(33); a save must follow ONLY a confirmed edit,
//! - a REJECTED edit replies `presetError`(53) and must NEVER be followed by a save
//!   (an unconfirmed save corrupted a real preset — `confirm_structural_edit`),
//! - the COLD first structural edit after a fresh load can be silently DROPPED, which
//!   the held-session path retry-hardens (`apply_copy_op`'s `retry_drop`).
//!
//! Configure [`SimDevice::with_drop_first`] / [`SimDevice::with_reject_at`] to drive
//! those two adversarial cases. Replies are produced SYNCHRONOUSLY from the send call
//! (the reports land in `Session::raw`); the subsequent heartbeat `pump`s return empty,
//! exactly as the confirm loop expects.
//!
//! **Lazy-commit saved doc (e2e only, HW-confirmed 1.8.45):** the real device's
//! `saveCurrentPreset` commits 45-100 s LATE — a same-slot `loadPreset` inside that
//! window materializes the PRE-save preset, while a field-8 saved-preset READ is
//! read-your-writes (immediately fresh). This corrupted a footswitch leveling run
//! offline-invisibly until now: a save's `(pending_doc, commit_deadline)` is tracked
//! per slot ([`SimState::saved_levels`] via `TMP_SIM_COMMIT_LATENCY_MS` / the
//! `/sim/commit-latency` bridge route, default 0 ms) — even 0 ms changes LOAD's
//! semantics from "preserve whatever `presetLevel` was last set" to "materialize this
//! slot's own committed doc", which is what makes the stale-load class reproducible
//! offline at all. The doc is `presetLevel` PLUS a footswitch bake's own baked param PLUS a
//! scene deferred save's own overlay param PLUS the `ftsw` array (`SavedDoc`) — narrower
//! than a full merged presetJson still (the CAPTURE MODEL doesn't reseed a scene overlay's
//! write on load, only its TEXT round-trips; `SavedDoc`'s doc comment has the residual
//! deviation). See `saved_levels`' field doc for the exact read/load asymmetry.
//!
//! **Footswitch assignment writes (HW semantics, no dedicated echo):**
//! `setFootswitchAssignment`(54) and `clearFootswitchAssignment`(55) edit the WORKING-COPY
//! `ftsw` array — live but unsaved until a `saveCurrentPreset`, exactly as on the unit. The
//! schema has no confirm echo for either (unlike `nodeReplaced`), so the fake answers them
//! with NOTHING and the caller confirms the only way hardware allows: a
//! `currentPresetDataRequest`(2) re-prompt, whose `currentPresetDataChanged`(3) push renders
//! the edited `ftsw` (`Session::live_ftsw`; the read-back branch of
//! `leveller::write_fs_values_on_session`'s confirm gate). A set REPLACES the function at
//! `functionIndex` or APPENDS when the index is at/past the switch's function count; a clear
//! SPLICES, shifting the switch's remaining functions down. The `swap` flag is decoded and
//! recorded on the [`SimEvent`] but models no behavioural difference — its device semantics
//! are empirically unresolved (`proto::set_footswitch_assignment`) and production always
//! sends `false`.
//!
//! **Scene recall semantics (HW-verified fw 1.8.45, this week):** a `loadScene` recall
//! merges the scene's JSON overlay onto base PER PARAM, not per node — a FULL overlay
//! masks base for every param it lists, but a bypass-only overlay (`{bypass: …}`) masks
//! ONLY `bypass`; every other param renders base, with NO retention from whatever scene
//! was recalled before ([`SimState::rendered_param`] / [`SimState::rendered_bypass`],
//! resolved fresh on every call, never cached). `scenes[].ftswStates` is a DERIVED CACHE
//! the real device ignores on recall — the sim never reads that key either, deriving a
//! footswitch's active state from the materialized block-bypass state alone. A
//! `changeParameter` write accepts RAW values outside `[0,1]` (dB-calibrated params like
//! `ACD_Boost.gain`) with no clamp. And a scene-context write with no preceding
//! `setNodeSceneEdit(enable=true)` lands on BASE only when the node's overlay in that scene
//! is NOT Full-shaped (BypassOnly, empty, or absent — Scene Edit disabled/never
//! materialized); against a Full-shaped overlay it lands ON the overlay EVEN FOR a param
//! the overlay doesn't yet carry, extending it per-param with siblings and base untouched
//! (HW-verified fw 1.8.45, crafted Full-partial overlay — see `overlay_is_full_shaped`'s
//! doc). The Scene-Edit flag state decides the landing, never per-param containment
//! (`F_CHANGE_PARAMETER`'s `landing_scene`). See `scene_context_tests` for the pinning
//! tests, one per fact.
//!
//! [`Session`]: crate::session::Session
//! [`HidTransport`]: crate::hid::HidTransport

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::hid::HidTransport;
use crate::proto;

/// The `i64` scene key used in [`SimState::param_writes`]: `-1` = the base state (no
/// active scene), otherwise the 0-based `scenes[]` wire index. A `changeParameter` with
/// per-block Scene Edit writes the CURRENT scene's overlay, so knob writes are scoped by
/// scene — that is why the offline capture model's `outputLevel` term reads back only the
/// override written for the scene being measured (a scene with no override measures its
/// stored knob → a 0 LU shift, the locked convention).
pub(crate) const SCENE_BASE: i64 = -1;

/// FenderMessageTMS field carrying a `PresetMessage`.
const TMS_PRESET: u32 = 2;
/// FenderMessageTMS fields carrying a `SongMessage` / `SetlistMessage` (CRUD).
const TMS_SONG: u32 = 11;
const TMS_SETLIST: u32 = 12;
// Song/Setlist inner field numbers (shared shape): listRequest=2, addX=4, removeX=6,
// renameX=7; the list RESPONSE is field 3 with records (field 2) carrying name=field 1.
const F_LIST_REQUEST: u32 = 2;
const F_LIST_ADD: u32 = 4;
const F_LIST_REMOVE: u32 = 6;
const F_LIST_RENAME: u32 = 7;
const F_LIST_RESPONSE: u32 = 3;
// A setlist's song MEMBERSHIP: request = setlistMessage(12).setlistSongListRequest(12),
// response = setlistSongListResponse(13). The sim models no membership, but must answer
// (an empty, complete response) so selecting a setlist doesn't hang on the read.
const F_SETLIST_SONGS_REQUEST: u32 = 12;
const F_SETLIST_SONGS_RESPONSE: u32 = 13;
// PresetMessage inner field numbers (mirror `proto`'s encoders + `session.rs`).
const F_LOAD_PRESET: u32 = 10;
const F_REPLACE_NODE: u32 = 39;
const F_REPLACE_WITH_BLOCK: u32 = 100;
const F_INSERT_NODE: u32 = 34;
const F_REMOVE_NODE: u32 = 35;
const F_NODE_JSON_REQUEST: u32 = 119;
const F_RENAME: u32 = 13;
const F_SAVE: u32 = 14;
const F_SET_PRESET_LEVEL: u32 = 76;
const F_CHANGE_PARAMETER: u32 = 12;
const F_LOAD_SCENE: u32 = 101;
const F_SET_NODE_SCENE_EDIT: u32 = 107;
/// `setFootswitchAssignment`(54) / `clearFootswitchAssignment`(55) — the `ftsw` working-copy
/// setters (`proto::set_footswitch_assignment`). NO dedicated confirm echo on the real device
/// (`Session::set_footswitch_assignment`'s doc), so neither replies here either: the caller
/// confirms through the field-2 re-prompt above.
const F_SET_FOOTSWITCH_ASSIGNMENT: u32 = 54;
const F_CLEAR_FOOTSWITCH_ASSIGNMENT: u32 = 55;
/// `currentPresetDataRequest`(2) — the WORKING-COPY re-prompt (`proto::current_preset_data_request`).
/// The device answers with a fresh `currentPresetDataChanged`(3) push; that is the ONLY way a
/// caller reads back an edit with no dedicated echo (`Session::live_ftsw`).
const F_CURRENT_PRESET_DATA_REQUEST: u32 = 2;
/// `presetDataRequest`(8) — the slot-addressed saved-preset ("field-8") read.
const F_PRESET_DATA_REQUEST: u32 = 8;
/// `presetDataChanged`(9) — its reply, carrying PLAINTEXT `presetJson`(3).
const F_PRESET_DATA_CHANGED: u32 = 9;
/// FenderMessageTMS field carrying a `SettingsMessage` (the re-amp toggle lives here).
const TMS_SETTINGS: u32 = 3;
/// `SettingsMessage.reampModeActive` (30) → `{ value(1) }`.
const F_REAMP_SETTING: u32 = 30;
// Device confirmation / echo field numbers.
const F_PRESET_LOADED: u32 = 11;
const F_NODE_INSERTED: u32 = 33;
const F_NODE_REMOVED: u32 = 36;
const F_NODE_REPLACED: u32 = 40;
const F_PRESET_ERROR: u32 = 53;
const F_PRESET_LEVEL_CHANGED: u32 = 77;

/// One device-visible action the fake observed, in order.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "e2e", derive(serde::Serialize))]
pub enum SimEvent {
    /// `loadPreset` — the **0-based** list index (the fake echoes `PresetLoaded`).
    Loaded(u32),
    /// `replaceNode`(39) → a stock model.
    Replace {
        group: String,
        node_id: String,
        fender_id: String,
    },
    /// `replaceNodeWithBlock`(100) → a saved block at a library index.
    ReplaceWithBlock {
        group: String,
        node_id: String,
        fender_id: String,
        index: u64,
    },
    /// `insertNode`(34) — field-2 = the FenderId to insert BEFORE; `before = None`
    /// appends at the group end.
    Insert {
        group: String,
        before: Option<String>,
        fender_id: String,
    },
    /// `removeNode`(35).
    Remove { group: String, node_id: String },
    /// `renameCurrentPreset`(13).
    Renamed(String),
    /// `saveCurrentPreset`(14) — the **0-based** list index.
    Saved(u32),
    /// `setPresetLevel`(76) — the linear amplitude that was sent.
    PresetLevel(f32),
    /// `loadScene`(101) — the 0-based `scenes[]` wire index.
    LoadScene(u32),
    /// `changeParameter`(12) float write. `scene` is the [`SCENE_BASE`] sentinel or the
    /// 0-based `scenes[]` index it landed under (the currently active scene at send time).
    ChangeParameter {
        scene: i64,
        group: String,
        node: String,
        param: String,
        value: f32,
    },
    /// `changeParameter` on `bypass` via the BOOL path (`ChangeParameter.boolVal`, field 7).
    /// Separate from [`SimEvent::ChangeParameter`] (a float `dspUnitParameters` write) because
    /// the WIRE MESSAGE differs — and because the leveler's isolation bypasses must be
    /// order-checkable against the scene recalls that revert them.
    Bypass { node: String, on: bool },
    /// `setNodeSceneEdit`(107).
    SceneEdit {
        group: String,
        node: String,
        enable: bool,
    },
    /// `setFootswitchAssignment`(54) — a working-copy `ftsw[addr]` function write. `index` is
    /// the REQUESTED 0-based function slot (an `index` past the switch's current function count
    /// APPENDS, so the landed slot can be lower — see [`SimState::ftsw_set`]). `function_json`
    /// is the raw wire string, so a test can parse the exact `valueA`/`valueB` that went out.
    /// `swap` is the wire flag verbatim; the fake models no behavioural difference for it (the
    /// semantics are HW-unverified — `proto::set_footswitch_assignment`).
    SetFootswitchAssignment {
        addr: u32,
        index: u32,
        function_json: String,
        swap: bool,
    },
    /// `clearFootswitchAssignment`(55) — remove function `index` from `ftsw[addr]`.
    ClearFootswitchAssignment { addr: u32, index: u32 },
    /// `SettingsMessage(3) → reampModeActive(30)` — the re-amp toggle (`true` = engage).
    /// Recorded because the ENGAGE is the only ordering landmark a capture has: everything a
    /// measurement sets up (the scene recall, the isolation bypasses, the pinned handle) is
    /// only in that capture if it went out BEFORE the engage — re-amp latches preset state at
    /// engage (`danger.md`), so "the write happened" and "the write was heard" are different
    /// facts and only the order tells them apart.
    ReAmp(bool),
    /// `Session::heartbeat` — a pure fire-and-forget keep-alive send with no reply, the ONLY
    /// caller of `HidTransport::send` (as opposed to `transact`/`transact_chunked`). Recorded
    /// so a naked-gap-breaker choreography (`capture_on_session`, `measure_scene_asis`,
    /// `arm_pair_measurement`'s zero-write base arm) can assert that a heartbeat actually
    /// landed between the last write and the engage — the structural fact `danger.md`'s
    /// naked-gap rule rests on.
    Heartbeat,
}

/// What a `saveCurrentPreset` pushes afterwards (see `SimState::post_save_push`). Only the
/// `#[cfg(test)]` knobs construct it, so the non-test build sees no constructor.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum PostSavePush {
    /// The slot's served document, unmutated by the edits — the load-time graph.
    LoadTimeEcho,
    /// A specific document (raw preset JSON).
    Doc(String),
}

struct SimState {
    events: Vec<SimEvent>,
    /// Count of structural edits (`replace`/`insert`/`remove`) seen — drives the
    /// drop-first / reject-at adversarial injections.
    structural_seen: u32,
    /// When set, the FIRST structural edit is silently DROPPED (no confirm, no error) —
    /// reproduces the cold-first-edit drop the held-session path retries past.
    drop_first: bool,
    /// When `Some(n)`, the Nth structural edit (1-based) is REJECTED with `presetError`.
    reject_at: Option<u32>,
    /// When set, a `saveCurrentPreset` queues a `currentPresetDataChanged`(3) push for the
    /// next `pump` — the document a held session scrapes off its buffer as the Copy
    /// post-save read-back.
    post_save_push: Option<PostSavePush>,
    /// Device-initiated pushes waiting for the next `pump` (replies to a send are delivered
    /// synchronously from the send instead).
    pending_pushes: Vec<Vec<u8>>,
    /// Song / Setlist names (slot = index + 1), mutated by the CRUD setters so a
    /// read-back-after-write reflects the change — the Songs tab's contract.
    songs: Vec<String>,
    setlists: Vec<String>,
    /// The preset JSON `currentPresetDataChanged`(3) echoes right after a `loadPreset` —
    /// the pre-edit roster the `blockcaps` guard reads before its first structural
    /// edit. Defaults to a plausible two-node `G1` graph (both ids uncapped by any of
    /// the 5 firmware block-count caps) so the guard's mandatory roster read succeeds
    /// without every test having to configure one; [`SimDevice::with_preset_json`]
    /// overrides it for a test that needs a specific pre-edit roster (e.g. to exercise
    /// an over-cap refusal).
    preset_json: String,
    /// Lazy cache of a parsed [`SimState::scene_render_json_source`], shared by
    /// `overlay_is_full_shaped` (every scene-context `changeParameter` on the live dispatch
    /// path) and the two `#[cfg(test)]` renderers — each used to clone the source string and
    /// re-parse it on every call. INVALIDATED (set back to `None`) at every WIRE-DRIVEN point
    /// the SOURCE STRING can change: `F_LOAD_PRESET` (a slot change picks a different e2e
    /// scenario doc) and [`SimDevice::with_preset_json`] (the plain-build override). A save
    /// does NOT invalidate it — `record_save`/`with_patched_doc` patch the ECHOED text
    /// (`current_preset_data_changed`), never `scene_render_json_source`'s own raw scenario
    /// read. `current_scene` does NOT invalidate it either — the cache is the un-recalled
    /// document; scene selection is applied fresh on every read via
    /// [`crate::probe_api::scene_jobs::scene_overlay`]. A test that pokes `current_slot`
    /// directly on a live `SimState` (bypassing `F_LOAD_PRESET`) — e.g. `physics_tests` —
    /// must clear this itself if it exercises the cache; today none of those tests do
    /// (they only reach `model_lufs`, which never reads this field).
    parsed_preset_cache: Option<serde_json::Value>,

    // ── DSP state for the offline physics-faithful capture model (`e2e_capture`) ──
    // Pure state writes updated by the wire setters below; the setters echo nothing
    // (they match the real device, which acks these fire-and-forget). Read only by the
    // offline `--features e2e` capture model, so they carry no reply framing.
    /// The 0-based list index of the last `loadPreset` (the sidecar / stored-knob key).
    current_slot: u32,
    /// The active scene: `None` = base, else the 0-based `scenes[]` wire index from the
    /// last `loadScene`. Restored on `loadPreset` from [`saved_scene`](SimState::saved_scene)
    /// (a fresh load activates the preset's saved `lastLoadedScene`, not base — HW-confirmed).
    current_scene: Option<u32>,
    /// Per-slot `lastLoadedScene` (0-based `scenes[]` index, `None` = base) as of the last
    /// `saveCurrentPreset` — what a subsequent `loadPreset` restores into `current_scene`.
    /// [`SimDevice::with_saved_scene`] seeds it directly for a test that doesn't want to
    /// drive a save first.
    saved_scene: HashMap<u32, Option<u32>>,
    /// Gate seam: while scene `.0` is active, field-3 pushes are cut at the first byte of `.1`.
    /// Field-8 stays whole — it is what `scene_jobs::backfill_scene_docs_from_saved` repairs from.
    #[cfg(all(test, feature = "e2e"))]
    truncated_scene_push: Option<(u32, String)>,
    /// The last `setPresetLevel` — the linear global multiplier the model shifts by
    /// `20·log10`. PLAIN (non-e2e) BUILD: a real `loadPreset` restores the slot's SAVED
    /// presetLevel, but this sim tracks no per-slot saved value here, so it just
    /// PRESERVES the last-set value across a load — a `ref_level = None` capture (the
    /// Doctor A/B) after leveling a DIFFERENT slot would read a leaked multiplier.
    /// E2E BUILD: superseded — `loadPreset` sets this from [`SimState::saved_levels`]'s
    /// per-slot lazy-commit store instead (module header), so a load DOES restore the
    /// right slot's value, faithfully INCLUDING the stale-load corruption window while a
    /// save is still pending.
    preset_level: f32,
    /// Scene-scoped knob writes: `(scene, group, node, param) → value`. The model reads
    /// the `outputLevel` entry for the scene under measurement (see [`SCENE_BASE`]).
    /// Cleared on `loadPreset` (a fresh load discards the edit buffer).
    param_writes: HashMap<(i64, String, String, String), f32>,
    /// Forced block bypasses (`node → bypassed`), from `changeParameter` boolVal on
    /// `bypass`. Drives the off-branch verdict: a switch that bypasses the sidecar's
    /// `routedNode` mutes the routed sound → silence. Cleared on `loadPreset`.
    bypass_writes: HashMap<String, bool>,
    /// `(scene, group, node)` triples that have had `setNodeSceneEdit(enable=true)` sent
    /// THIS session, since the last `loadPreset` — the gate `F_CHANGE_PARAMETER` checks
    /// before routing a scene-context write onto BASE instead of the scene (HW fw
    /// 1.8.45, module header): a write against a node with NO recorded enable, whose
    /// scene overlay is not Full-shaped, lands on base rather than extending the overlay.
    /// Scene-keyed only (base writes never consult this set). Cleared on `loadPreset`,
    /// same as `param_writes`/`bypass_writes`.
    ///
    /// KNOWN OFFLINE DIVERGENCE: on hardware a Scene-Edit enable does NOT survive a
    /// reconnect (session-scoped, like the re-amp toggle and the loaded scene itself),
    /// but this set lives on `SimState`, which this fake's `Arc<Mutex<_>>` persists
    /// ACROSS `Session` instances — the sim has no transport-level "new connection"
    /// signal distinct from a wire message, so there is no cheap seam to clear it on
    /// reconnect without one. An offline spec must not rely on a write landing on the
    /// overlay ACROSS a fresh connection without re-sending the enable — only
    /// `loadPreset` is a trustworthy reset point here.
    scene_edit_enabled: std::collections::HashSet<(u32, String, String)>,
    /// The WORKING-COPY `ftsw` array once a `setFootswitchAssignment`(54) /
    /// `clearFootswitchAssignment`(55) has edited it this load; `None` = untouched, so the
    /// rendered doc keeps whatever `ftsw` its source text (or the saved doc) carries. Held as
    /// the whole array rather than per-`(addr, index)` edits because a CLEAR splices and
    /// SHIFTS the switch's remaining functions down — index-keyed edits could not compose with
    /// that. Materialized lazily by [`SimState::base_ftsw`] and cleared on `loadPreset`
    /// alongside `param_writes`/`bypass_writes` (a fresh load discards the edit buffer), so a
    /// field-3 render after a load shows the SAVED `ftsw`, never a stale unsaved one.
    ftsw_working: Option<serde_json::Value>,
    /// Whether re-amp is engaged (the `SettingsMessage` toggle). Latched at capture; a
    /// capture with re-amp OFF returns silence (the real device routes no USB return).
    reamp_on: bool,
    /// Capture-fault injection (`POST /sim/fault`): when armed for a slot, that slot's
    /// NEXT capture returns silence once (the leveller's no-signal path), then disarms.
    /// e2e-only — its only reader is the offline capture model.
    #[cfg(feature = "e2e")]
    fail_capture_slot: Option<u32>,
    /// Per-slot lazy-commit `presetLevel` store (the same-slot stale-load corruption
    /// mechanism — module header). Absent entry = never touched (read OR saved) this
    /// run. Seeded lazily from the slot's own scenario JSON (or 1.0 for a non-scenario
    /// slot) on first touch by EITHER `record_save` or the field-3/field-8 echo readers
    /// (`load_echo_json`/`saved_slot_json_body`) — NOT by `F_LOAD_PRESET` itself, which
    /// only ever READS this map (see [`SimState::ever_saved`] for why). e2e-only: a
    /// plain build has no per-slot scenario doc to seed from.
    #[cfg(feature = "e2e")]
    saved_levels: HashMap<u32, PendingLevel>,
    /// Slots `record_save` has actually committed a pending doc for THIS run — the ONLY
    /// reliable "has this slot been saved" signal (`saved_levels.contains_key` is NOT
    /// one: the read-only echo paths lazily insert into that map too). `F_LOAD_PRESET`
    /// gates its `preset_level`/baked-param restore on this set, not on `saved_levels`,
    /// so a slot that's merely been LOOKED AT (echoed) but never saved keeps the old
    /// "preserve the last-set value" behavior — restoring unconditionally on every load
    /// (this fix's first cut) broke scene/base-only offline specs on OTHER scenario
    /// slots that never save a presetLevel at all, caught only by the full offline
    /// Playwright e2e suite, not `cargo test --lib` alone.
    #[cfg(feature = "e2e")]
    ever_saved: std::collections::HashSet<u32>,
    /// Test/spec override for [`SimState::commit_latency`] — bypasses
    /// `TMP_SIM_COMMIT_LATENCY_MS` so parallel unit tests never race each other over a
    /// shared env var, and so a single Playwright spec can arm latency on the ONE
    /// already-running offline server process (`POST /sim/commit-latency`) without an
    /// env var set before that process started ever reaching it.
    #[cfg(feature = "e2e")]
    commit_latency_override: Option<std::time::Duration>,
}

/// Lazy-commit state for ONE slot's SAVED doc (module header): `presetLevel` plus the
/// baked (base-scene) block params a footswitch bake has written onto it — the two
/// fields a footswitch-leveling save can actually change offline. `committed` is what
/// LOAD (and the field-3 graph echo) sees until `pending`'s deadline passes; a field-8
/// READ always sees `pending` immediately when one exists (read-your-writes — mirrors
/// the real device's field-8/load asymmetry). Lives on [`SimState`] — per SimDevice
/// INSTANCE, never the process-global scenario `OnceLock`s: a save mutates one slot's
/// own copy, never the shared immutable fixture text.
///
/// REMAINING DEVIATION from the plan's full "merged presetJson" (module header,
/// `record_save`'s doc): `scene_params` (the fold below) makes the SAVED DOCUMENT TEXT
/// correctly carry a scene overlay's write (`witness_value_in_doc`'s scene-indexed witness
/// and `persisted_value`'s scene-overlay read both consult it — Fix 2, closing the gap this
/// note used to record as "no offline spec reads one back through a save"). What is STILL
/// NOT modeled: `F_LOAD_PRESET` reseeds only the `SCENE_BASE` entries of `param_writes` on a
/// fresh load (that handler's own comment), never `scene_params` — so after a save→load
/// round trip the rendered TEXT holds the leveled scene value while the CAPTURE MODEL
/// (`model_lufs`, which reads `param_writes`, not the doc text) still renders that scene's
/// PRE-level loudness. A future spec that re-measures a scene AFTER its own save must reseed
/// `param_writes` from `scene_params` on load first, or its capture will silently disagree
/// with the saved document it just read (post-review amendment 4).
///
/// `ftsw` ASSIGN edits DO round-trip now (they did not before field 54 was modeled), because
/// the production Assign flow has three readers that a non-persisting `ftsw` makes lie:
/// `leveller::verify_fs_persisted_writes` re-reads FIELD-8 and looks the solved value up as
/// `ftsw`'s `valueA` (`ftsw_value_a`) — without persistence every offline Assign row reports
/// `persist_mismatch: true`; `leveller::witness_value_in_doc` resolves a `SaveWitness::Param`
/// against `ftsw`'s `valueA` too (for an Assign the block's own `dspUnitParameters` value
/// exists but can NEVER match), so an unpersisted assign starves `ensure_fresh_load` into its
/// time-gated fallback; and the post-load field-3 echo would show the pre-run assignment.
#[cfg(feature = "e2e")]
#[derive(Clone, Default)]
struct SavedDoc {
    preset_level: f32,
    /// Baked `(group, node, param) → value` overlay — a footswitch bake's own knob,
    /// SCENE_BASE-scoped only (see the deviation note above).
    params: HashMap<(String, String, String), f32>,
    /// Scene-scoped `(scene, group, node, param) → value` overlay — a scene deferred save's
    /// own knob (see the deviation note above for what this does and does not fix).
    scene_params: HashMap<(u32, String, String, String), f32>,
    /// The whole saved `ftsw` array once a footswitch set/clear has been saved for this slot;
    /// `None` = never edited, so the slot's own fixture text still owns it. Stored whole for
    /// the same reason [`SimState::ftsw_working`] is (a clear SHIFTS indices).
    ftsw: Option<serde_json::Value>,
}

#[cfg(feature = "e2e")]
#[derive(Clone, Default)]
struct PendingLevel {
    committed: SavedDoc,
    pending: Option<(SavedDoc, std::time::Instant)>,
}

impl Default for SimState {
    fn default() -> Self {
        SimState {
            events: Vec::new(),
            structural_seen: 0,
            drop_first: false,
            reject_at: None,
            post_save_push: None,
            pending_pushes: Vec::new(),
            songs: vec!["Opening Set".into(), "Encore".into()],
            setlists: vec!["Saturday Night".into()],
            // A recognized amp (with an `outputLevel` control) + one effect, under a known
            // routing template — so offline scene-leveling's `list_level_blocks` discovery
            // finds a levelable amp candidate. The physics model reads the WRITTEN outputLevel
            // node-agnostically (there is one amp per sound in the fixtures), so scene rows
            // converge against this default amp regardless of the scenario's real trunk node
            // (the deferred graph-echo fidelity fix — see the FIDELITY CEILING note).
            preset_json: r#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{"G1":[
                {"FenderId":"ACD_Twin57","nodeId":"n1","dspUnitParameters":{"bypass":false,"outputLevel":0.5}},
                {"FenderId":"ACD_ChorusCE2","nodeId":"n2","dspUnitParameters":{"bypass":false}}
            ]}}}"#
                .to_string(),
            parsed_preset_cache: None,
            current_slot: 0,
            current_scene: None,
            saved_scene: HashMap::new(),
            #[cfg(all(test, feature = "e2e"))]
            truncated_scene_push: None,
            preset_level: 1.0,
            param_writes: HashMap::new(),
            bypass_writes: HashMap::new(),
            scene_edit_enabled: std::collections::HashSet::new(),
            ftsw_working: None,
            reamp_on: false,
            #[cfg(feature = "e2e")]
            fail_capture_slot: None,
            #[cfg(feature = "e2e")]
            saved_levels: HashMap::new(),
            #[cfg(feature = "e2e")]
            ever_saved: std::collections::HashSet::new(),
            #[cfg(feature = "e2e")]
            commit_latency_override: None,
        }
    }
}

impl SimState {
    /// The active scene as the [`param_writes`](SimState::param_writes) `i64` key.
    fn scene_key(&self) -> i64 {
        self.current_scene.map_or(SCENE_BASE, i64::from)
    }

    /// The JSON a scene-recall render resolves against: the slot's real scenario fixture
    /// when one exists (e2e only — same precedence as [`load_echo_json`]), else
    /// `preset_json` (so [`SimDevice::with_preset_json`] drives a plain, non-e2e test
    /// too). Called only by [`SimState::parsed_preset`], which caches the parse — see that
    /// method and [`SimState::parsed_preset_cache`] for the invalidation contract.
    fn scene_render_json_source(&self) -> String {
        #[cfg(feature = "e2e")]
        {
            if let Some(j) = scenario_json_for(self.current_slot) {
                return j.to_string();
            }
        }
        self.preset_json.clone()
    }

    /// Parse [`SimState::scene_render_json_source`] once and cache it — shared by
    /// `overlay_is_full_shaped` (the live `F_CHANGE_PARAMETER` dispatch path) and the two
    /// `#[cfg(test)]` renderers below, which each used to clone the source string and
    /// re-parse it on every call. See [`SimState::parsed_preset_cache`] for the
    /// invalidation contract; a stale cache here would silently falsify offline gates, so
    /// every call site that can change the source string clears it first.
    fn parsed_preset(&mut self) -> Option<&serde_json::Value> {
        if self.parsed_preset_cache.is_none() {
            let json = self.scene_render_json_source();
            self.parsed_preset_cache = serde_json::from_str(&json).ok();
        }
        self.parsed_preset_cache.as_ref()
    }

    /// `scene`'s own overlay value for `(node, param)`, from the cached parsed doc — Full
    /// and BypassOnly overlays both carry the raw per-scene body (Absent/Unknown carry
    /// none). A BypassOnly overlay only ever carries the bypass-family keys
    /// (`scene_jobs::BYPASS_ONLY_KEYS`), so a non-bypass `param` against one naturally
    /// misses here and the caller falls through to base — the SAME classifier
    /// [`SimState::overlay_is_full_shaped`] uses, so the two can't read a shape
    /// differently. Shared by `rendered_param` (any param) and `rendered_bypass` (asked
    /// for `"bypass"`).
    #[cfg(any(test, feature = "e2e"))]
    fn scene_overlay_value(
        &mut self,
        scene: u32,
        node: &str,
        param: &str,
    ) -> Option<serde_json::Value> {
        use crate::probe_api::scene_jobs::SceneOverlay;
        let doc = self.parsed_preset()?;
        match crate::probe_api::scene_jobs::scene_overlay(doc, scene, node) {
            SceneOverlay::Full(params) | SceneOverlay::BypassOnly(params) => {
                params.get(param).cloned()
            }
            SceneOverlay::Absent | SceneOverlay::Unknown => None,
        }
    }

    /// The BASE graph node's own `(group, node, param)` value, from the cached parsed doc —
    /// the un-overlaid fallback both renderers fall through to. Shared walk, replacing what
    /// used to be a hand-rolled `guitarNodes` traversal duplicated in three places.
    #[cfg(any(test, feature = "e2e"))]
    fn base_graph_value(
        &mut self,
        group: &str,
        node: &str,
        param: &str,
    ) -> Option<serde_json::Value> {
        let doc = self.parsed_preset()?;
        doc.get("audioGraph")?
            .get("guitarNodes")?
            .get(group)?
            .as_array()?
            .iter()
            .find(|n| n.get("nodeId").and_then(|x| x.as_str()) == Some(node))?
            .get("dspUnitParameters")?
            .get(param)
            .cloned()
    }

    /// The EFFECTIVE value of `(group, node, param)` a `loadScene` recall renders RIGHT
    /// NOW (HW fw 1.8.45): an explicit `changeParameter` write for the active scene wins;
    /// else the active scene's own JSON overlay carries the param (a FULL overlay masks
    /// base for every param it lists, a bypass-only overlay masks nothing else); else an
    /// explicit BASE write; else base's own static value. Resolved fresh every call from
    /// the cached parsed doc, so recalling a DIFFERENT scene never retains a value the new
    /// scene's own overlay doesn't carry
    /// (pin: `scene_recall_renders_the_overlay_per_param_with_no_retention_from_the_prior_scene`).
    /// Test-only: its sole caller is [`SimDevice::rendered_param`] (`#[cfg(test)]`).
    #[cfg(any(test, feature = "e2e"))]
    fn rendered_param(&mut self, group: &str, node: &str, param: &str) -> Option<f64> {
        let scene_key = self.scene_key();
        // 1. An explicit write for the address space under measurement (the active
        // scene, or base) always wins.
        if let Some(v) = self.param_writes.get(&(
            scene_key,
            group.to_string(),
            node.to_string(),
            param.to_string(),
        )) {
            return Some(f64::from(*v));
        }
        // 2. In a real scene, that scene's own JSON overlay (a FULL overlay masks base
        // for every param it lists; a bypass-only overlay masks nothing else — fact 1).
        if let Some(scene) = self.current_scene {
            if let Some(pv) = self
                .scene_overlay_value(scene, node, param)
                .and_then(|v| v.as_f64())
            {
                return Some(pv);
            }
        }
        // 3. Falls through to BASE: an explicit base write (e.g. fact 4's landing site)
        // wins over the static fixture's own base value — a scene inheriting base must
        // see what base was just WRITTEN to, not the pristine fixture body.
        if scene_key != SCENE_BASE {
            if let Some(v) = self.param_writes.get(&(
                SCENE_BASE,
                group.to_string(),
                node.to_string(),
                param.to_string(),
            )) {
                return Some(f64::from(*v));
            }
        }
        self.base_graph_value(group, node, param)
            .and_then(|v| v.as_f64())
    }

    /// The EFFECTIVE `bypass` a `loadScene` recall renders RIGHT NOW for `(group, node)` —
    /// the bypass analogue of [`SimState::rendered_param`], and the mechanism behind fact
    /// 2 (HW fw 1.8.45): it derives from an explicit `changeParameter` bool write, else
    /// the active scene's own overlay, else base — and NEVER consults a fixture's
    /// `ftswStates` array (a derived cache the real device also ignores on recall), by
    /// construction: nothing in this method's lookup chain ever reads that key. Two callers:
    /// [`SimDevice::rendered_bypass`] (`#[cfg(test)]`, the scene-recall pins) and — under
    /// `e2e` — [`model_lufs`]'s leveled-param activation, which needs it to tell "the run
    /// forced this block OFF" from "the run never touched it and the preset has it ON" (see
    /// that predicate's doc).
    #[cfg(any(test, feature = "e2e"))]
    fn rendered_bypass(&mut self, group: &str, node: &str) -> Option<bool> {
        if let Some(b) = self.bypass_writes.get(node) {
            return Some(*b);
        }
        if let Some(scene) = self.current_scene {
            if let Some(bv) = self
                .scene_overlay_value(scene, node, "bypass")
                .and_then(|v| v.as_bool())
            {
                return Some(bv);
            }
        }
        self.base_graph_value(group, node, "bypass")
            .and_then(|v| v.as_bool())
    }

    /// The value `(group, node, param)` is AUTHORED at for the sound under measurement —
    /// [`SimState::rendered_param`] with the `param_writes` steps removed: the active
    /// scene's own overlay, else base's static value. This is the value the sidecar's C was
    /// calibrated against, so it is the anchor a WRITE's loudness delta is measured from
    /// ([`wet_mix_gain_db`]); reading `rendered_param` instead would return the write itself
    /// and the delta would collapse to zero.
    #[cfg(feature = "e2e")]
    fn authored_param(&mut self, group: &str, node: &str, param: &str) -> Option<f64> {
        if let Some(scene) = self.current_scene {
            if let Some(v) = self
                .scene_overlay_value(scene, node, param)
                .and_then(|v| v.as_f64())
            {
                return Some(v);
            }
        }
        self.base_graph_value(group, node, param)
            .and_then(|v| v.as_f64())
    }

    /// The `ftsw` array a working-copy edit starts from: the slot's own SAVED array when a
    /// save has already changed it this run (e2e's lazy-commit doc), else the pristine source
    /// document's own `ftsw`, else an empty array (the plain build's default two-node graph
    /// carries no `ftsw` key — an edit against it materializes one, exactly as a real preset
    /// with an empty switch would render).
    fn base_ftsw(&mut self) -> serde_json::Value {
        #[cfg(feature = "e2e")]
        {
            let slot0 = self.current_slot;
            if let Some(f) = self.committed_doc(slot0).ftsw.clone() {
                return f;
            }
        }
        self.parsed_preset()
            .and_then(|d| d.get("ftsw").cloned())
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()))
    }

    /// Apply `setFootswitchAssignment`(54) to the working copy: put `func` at
    /// `ftsw[addr][index]`. An `index` inside the switch's existing function list REPLACES that
    /// function; an `index` at or past its length APPENDS (the only two shapes
    /// `commands::level_footswitch::resolve_footswitch_job` ever asks for — an existing match's
    /// index, or `sw.len()`). Padding the gap with nulls instead would render an `ftsw` that
    /// `leveller::param_fn_present` mis-reads, so the fake never does it. Missing switches
    /// ahead of `addr` are materialized as empty function lists.
    fn ftsw_set(&mut self, addr: u32, index: u32, func: serde_json::Value) {
        self.with_working_ftsw(addr, |switches| {
            if (index as usize) < switches.len() {
                switches[index as usize] = func;
            } else {
                switches.push(func);
            }
        });
    }

    /// Apply `clearFootswitchAssignment`(55): remove function `index` from `ftsw[addr]`,
    /// SPLICING the switch's remaining functions down a slot (the array shape the preset JSON
    /// carries has no holes). An out-of-range `index` is a no-op — the device has nothing to
    /// remove either. The shift is the unverified half of this op: production only ever clears
    /// a switch's LAST-resolved `param` function (`FsWrite::Bake`'s `clear_stale`), and both
    /// confirm paths (`footswitch::existing_param_fn_index` returning `None`) read the same
    /// whether or not siblings shifted.
    fn ftsw_clear(&mut self, addr: u32, index: u32) {
        self.with_working_ftsw(addr, |switches| {
            if (index as usize) < switches.len() {
                switches.remove(index as usize);
            }
        });
    }

    /// Take-or-base the working `ftsw` array, run `f` against switch `addr`'s function list,
    /// and write the array back to `ftsw_working` — the byte-identical take/write-back
    /// prelude and epilogue [`SimState::ftsw_set`]/[`SimState::ftsw_clear`] used to
    /// duplicate.
    fn with_working_ftsw(&mut self, addr: u32, f: impl FnOnce(&mut Vec<serde_json::Value>)) {
        let mut arr = match self.ftsw_working.take() {
            Some(a) => a,
            None => self.base_ftsw(),
        };
        f(ftsw_switch_list(&mut arr, addr));
        self.ftsw_working = Some(arr);
    }

    /// True when scene `scene`'s overlay for `node` is FULL-shaped — routed straight
    /// through the production classifier ([`crate::probe_api::scene_jobs::scene_overlay`])
    /// off the cached parsed doc, so this sim's landing rule tracks the real classifier BY
    /// CONSTRUCTION (micNodes overlays, FenderId-keyed overlay entries, and `Unknown`
    /// handling all included — the old hand-rolled walk here covered only
    /// `guitarNodes`/node-id keying and folded `Unknown` into "not full", which happened to
    /// give the same conservative answer for that one shape but was a second, divergeable
    /// copy of the real rule). `scene_overlay` resolves the node's group itself off the
    /// roster, so no `group` argument is needed here.
    ///
    /// The landing gate below (fact 4) needs exactly this — Full-shaped or not — and NOT
    /// "does the overlay carry THIS param": HW-verified fw 1.8.45 (crafted Full-partial
    /// overlay: a TubeScreamer scene-0 overlay carrying `blend`/`overdrive`/`tone` but not
    /// `level`, base `level` 0.65) proved an enable-less `changeParameter(level, …)` lands
    /// IN the overlay — EXTENDING it per-param, siblings unchanged, base untouched — not on
    /// base. The Scene-Edit flag state alone decides the landing.
    ///
    /// False when the node has NO overlay at all in this scene (Absent), the overlay is
    /// Unknown (base slot, or an unparseable cached doc), or BypassOnly — all land on BASE
    /// below, per the HW-proven leak-to-base for "no per-scene knobs materialized here".
    fn overlay_is_full_shaped(&mut self, scene: u32, node: &str) -> bool {
        let Some(doc) = self.parsed_preset() else {
            return false;
        };
        matches!(
            crate::probe_api::scene_jobs::scene_overlay(doc, scene, node),
            crate::probe_api::scene_jobs::SceneOverlay::Full(_)
        )
    }
}

#[cfg(feature = "e2e")]
impl SimState {
    /// `slot0`'s lazy-commit entry, seeding it from the slot's own scenario JSON (or 1.0
    /// for a non-scenario slot) the first time this slot is touched THIS run. The baked
    /// param overlay always starts EMPTY — a never-yet-saved-this-run slot's own static
    /// scenario body is the un-overlaid truth (its dspUnitParameters ARE the base values;
    /// [`SimState::param_writes`]' seed-on-load reads them straight off the field-3/field-8
    /// body, not through this overlay — see the LOAD handler).
    fn pending_level_entry(&mut self, slot0: u32) -> &mut PendingLevel {
        self.saved_levels
            .entry(slot0)
            .or_insert_with(|| PendingLevel {
                committed: SavedDoc {
                    preset_level: scenario_preset_level(slot0).unwrap_or(1.0),
                    params: HashMap::new(),
                    scene_params: HashMap::new(),
                    // `None` = the slot's own fixture `ftsw` is still the saved truth.
                    ftsw: None,
                },
                pending: None,
            })
    }

    /// `slot0`'s lazy-commit entry with any DUE pending save promoted to committed first —
    /// every doc accessor goes through this, so a read or load after the commit window
    /// always answers with the settled doc, never a phantom still-pending one.
    fn promoted_entry(&mut self, slot0: u32) -> &mut PendingLevel {
        let now = std::time::Instant::now();
        let entry = self.pending_level_entry(slot0);
        if matches!(&entry.pending, Some((_, deadline)) if now >= *deadline) {
            if let Some((doc, _)) = entry.pending.take() {
                entry.committed = doc;
            }
        }
        entry
    }

    /// The doc a LOAD is entitled to see right now for `slot0`: committed only — a
    /// still-pending save is invisible to a load until its deadline passes (the stale-load
    /// corruption window this model exists to reproduce).
    fn committed_doc(&mut self, slot0: u32) -> &SavedDoc {
        &self.promoted_entry(slot0).committed
    }

    /// The doc a field-8 READ sees right now for `slot0`: the pending doc when one exists
    /// (read-your-writes), else the committed doc.
    fn readable_doc(&mut self, slot0: u32) -> &SavedDoc {
        let entry = self.promoted_entry(slot0);
        entry
            .pending
            .as_ref()
            .map_or(&entry.committed, |(doc, _)| doc)
    }

    /// Record a save: the CURRENT working `preset_level`, plus the CURRENTLY-COMMITTED
    /// baked params merged with this session's own SCENE_BASE `param_writes` (a footswitch
    /// bake) AND its scene-scoped `param_writes` (a scene deferred save — `scene_params`,
    /// Fix 2), becomes `slot0`'s pending doc, landing after [`SimState::commit_latency`].
    /// Even a 0 ms latency changes LOAD's semantics (module header) — a genuinely non-zero
    /// latency additionally REPRODUCES the same-slot stale-load incident (a load before the
    /// deadline still sees the OLD committed doc). Merging onto the CURRENT committed params
    /// (not the session's writes alone) mirrors the real device: a save persists this
    /// session's edits ON TOP of whatever was already saved, not a wholesale replacement —
    /// a base-only save must not erase an earlier footswitch save's baked knob, and vice
    /// versa (two separate save call sites in the real leveling flow, base then FS batch).
    fn record_save(&mut self, slot0: u32) {
        self.ever_saved.insert(slot0);
        let level = self.preset_level;
        let mut params = self.pending_level_entry(slot0).committed.params.clone();
        let mut scene_params = self
            .pending_level_entry(slot0)
            .committed
            .scene_params
            .clone();
        for ((scene, group, node, param), v) in &self.param_writes {
            if *scene == SCENE_BASE {
                params.insert((group.clone(), node.clone(), param.clone()), *v);
            } else {
                // Registration filter (post-review amendment 5): fold every non-base scene
                // key, cast the wire `i64` scene index down to the witness/overlay's `u32`.
                scene_params.insert(
                    (*scene as u32, group.clone(), node.clone(), param.clone()),
                    *v,
                );
            }
        }
        // `ftsw` persists WHOLESALE: the working copy was itself materialized from the saved
        // array (`base_ftsw`), so it already carries every earlier save's functions — there is
        // nothing to merge, and merging per-index could not express a clear's shift anyway. No
        // working-copy edit this session leaves the saved array exactly as it was.
        let ftsw = self
            .ftsw_working
            .clone()
            .or_else(|| self.pending_level_entry(slot0).committed.ftsw.clone());
        let deadline = std::time::Instant::now() + self.commit_latency();
        self.pending_level_entry(slot0).pending = Some((
            SavedDoc {
                preset_level: level,
                params,
                scene_params,
                ftsw,
            },
            deadline,
        ));
    }

    /// How long a save stays pending before a LOAD may see it: the spec/test override
    /// first (`SimDevice::with_commit_latency` / `POST /sim/commit-latency`), else
    /// `TMP_SIM_COMMIT_LATENCY_MS`, else 0.
    fn commit_latency(&self) -> std::time::Duration {
        self.commit_latency_override.unwrap_or_else(|| {
            std::env::var("TMP_SIM_COMMIT_LATENCY_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .map(std::time::Duration::from_millis)
                .unwrap_or(std::time::Duration::ZERO)
        })
    }
}

/// The slot's own scenario-fixture `audioGraph.presetLevel` — the seed value a
/// never-yet-saved-this-run slot's lazy-commit store starts at. `None` for a
/// non-scenario slot (the caller falls back to 1.0, matching the shared default body's
/// own implicit unity gain).
#[cfg(feature = "e2e")]
fn scenario_preset_level(slot0: u32) -> Option<f32> {
    scenario_json_for(slot0)
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .and_then(|v| v["audioGraph"]["presetLevel"].as_f64())
        .map(|v| v as f32)
}

/// Patch `audioGraph.presetLevel` PLUS a footswitch bake's own baked `(group, node,
/// param)` overlay PLUS every scene deferred save's own `(scene, group, node, param)`
/// overlay (Fix 2, [`patch_scene_overlays`]) into a preset-JSON BODY TEXT — the fields the
/// lazy-commit model synthesizes into the field-3/field-8 JSON string itself (the rest of
/// that TEXT — `ftsw`, every OTHER scene field — stays exactly the committed scenario body;
/// no offline caller reads those back through a save round trip, so deep-merging them is out
/// of scope here). Patching the baked params into this TEXT (not just
/// `SimState::param_writes`, which is what `model_lufs` actually reads — and, for the scene
/// overlay, does NOT reseed on load; see `SavedDoc`'s deviation note) matters for readers of
/// the text itself: `leveller::witness_value_in_doc` compares a `SaveWitness::Param` against
/// the FIELD-3 echo (both the base-baked and the scene-indexed shape), and
/// `leveller::persisted_value`'s post-save field-8 read expects the just-written value to
/// show up in the SAVED document, not the pristine fixture body — without this, both would
/// see the pre-write value forever and report a false persist-mismatch / a permanently-stale
/// witness compare. Falls back to the original bytes on a parse failure, which never happens
/// for a committed fixture but a caller must still get SOMETHING. Patches
/// `audioGraph.guitarNodes` groups ONLY for the base overlay: a `micNodes` baked param would
/// silently patch nothing — no current fixture routes one; extend the group lookup here
/// FIRST if a mic-path fixture ever bakes a param, or its persist verify reads the pristine
/// value and reports a false mismatch with no diagnostic.
#[cfg(feature = "e2e")]
fn with_patched_doc(json: &str, doc: &SavedDoc) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    if let Some(ag) = v.get_mut("audioGraph") {
        ag["presetLevel"] = serde_json::json!(doc.preset_level);
        if let Some(groups) = ag.get_mut("guitarNodes").and_then(|g| g.as_object_mut()) {
            for ((group, node, param), value) in &doc.params {
                let Some(nodes) = groups.get_mut(group).and_then(|g| g.as_array_mut()) else {
                    continue;
                };
                for n in nodes {
                    if n.get("nodeId").and_then(|v| v.as_str()) == Some(node.as_str()) {
                        if let Some(dsp) = n.get_mut("dspUnitParameters") {
                            dsp[param] = serde_json::json!(value);
                        }
                    }
                }
            }
        }
    }
    patch_scene_overlays(&mut v, &doc.scene_params);
    serde_json::to_string(&v).unwrap_or_else(|_| json.to_string())
}

/// Patch `scene_params` entries (`record_save`'s fold) into `scenes[scene].{guitarNodes,
/// micNodes}.<group>` of the already-parsed doc `v` — [`with_patched_doc`]'s scene-overlay
/// sibling. Resolves the node's own `(node_id, fender_id)` off ONE `audiograph::roster(v)` walk
/// hoisted before the loop (the per-entry `roster_entry` call this used to make was a fresh
/// whole-graph rebuild EVERY entry — the hot-path cost this hoist resolves), then locates and
/// writes the overlay entry through [`crate::probe_api::scene_jobs::scene_overlay_entry_mut`] —
/// the write-side counterpart of `scene_jobs::scene_overlay_for`'s read, so the two can never key
/// a node differently. A node no longer in the base graph, or a scene/group the doc doesn't
/// carry, is silently skipped — nothing to patch onto.
#[cfg(feature = "e2e")]
fn patch_scene_overlays(
    v: &mut serde_json::Value,
    scene_params: &HashMap<(u32, String, String, String), f32>,
) {
    let roster = crate::audiograph::roster(v);
    for ((scene, group, node, param), value) in scene_params {
        let Some((_, node_id, fender_id)) = roster
            .iter()
            .find(|(_, nid, fid)| nid == node || fid == node)
        else {
            continue;
        };
        let Some(entry) = crate::probe_api::scene_jobs::scene_overlay_entry_mut(
            v,
            *scene,
            (group, node_id, fender_id),
        ) else {
            continue;
        };
        // Coerce a non-object `dspUnitParameters` (or none yet) to `{}` ONCE, then write —
        // `scene_overlay_entry_mut` already seeds a FRESH entry with an object, so this only
        // ever fires for a pre-existing entry whose shape is off.
        if !entry
            .get("dspUnitParameters")
            .is_some_and(|d| d.is_object())
        {
            entry["dspUnitParameters"] = serde_json::json!({});
        }
        entry["dspUnitParameters"][param] = serde_json::json!(value);
    }
}

/// `ftsw[addr]`'s function list, growing the switch array (with empty lists) and coercing a
/// non-array `ftsw`/switch entry as needed — so an edit against a document with no `ftsw` key,
/// or one whose switch `addr` was never authored, still lands somewhere readable.
fn ftsw_switch_list(ftsw: &mut serde_json::Value, addr: u32) -> &mut Vec<serde_json::Value> {
    if !ftsw.is_array() {
        *ftsw = serde_json::Value::Array(Vec::new());
    }
    let switches = ftsw.as_array_mut().expect("coerced to an array above");
    while switches.len() <= addr as usize {
        switches.push(serde_json::Value::Array(Vec::new()));
    }
    let sw = &mut switches[addr as usize];
    if !sw.is_array() {
        *sw = serde_json::Value::Array(Vec::new());
    }
    sw.as_array_mut().expect("coerced to an array above")
}

/// Overlay a whole `ftsw` array onto a preset-JSON BODY TEXT. `None` (no edit, no saved
/// override) returns the text VERBATIM — deliberately, so the common path never pays a
/// re-serialize that would reorder the document's keys (`serde_json::Value` is a `BTreeMap`
/// here). Falls back to the original bytes on a parse failure, like [`with_patched_doc`].
fn with_ftsw(json: &str, ftsw: Option<&serde_json::Value>) -> String {
    let Some(ftsw) = ftsw else {
        return json.to_string();
    };
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    match v.as_object_mut() {
        Some(o) => {
            o.insert("ftsw".to_string(), ftsw.clone());
        }
        None => return json.to_string(),
    }
    serde_json::to_string(&v).unwrap_or_else(|_| json.to_string())
}

/// An in-memory fake device. Clone shares the same recording (an `Arc<Mutex<…>>`), so a
/// test keeps a handle to read [`SimDevice::events`] after moving a clone into
/// [`crate::session::Session::from_transport`].
#[derive(Clone, Default)]
pub struct SimDevice {
    state: Arc<Mutex<SimState>>,
}

impl SimDevice {
    pub fn new() -> SimDevice {
        SimDevice::default()
    }

    /// Silently DROP the first structural edit (forces the held-session retry path).
    #[cfg(test)]
    pub fn with_drop_first(self) -> SimDevice {
        self.state.lock().expect("sim lock").drop_first = true;
        self
    }

    /// REJECT the `n`th structural edit (1-based) with `presetError` (never save after).
    #[cfg(test)]
    pub fn with_reject_at(self, n: u32) -> SimDevice {
        self.state.lock().expect("sim lock").reject_at = Some(n);
        self
    }

    /// After every `saveCurrentPreset`, push the slot's served (pre-edit) document on the
    /// next `pump` — the stale post-save read-back the Copy cache patch must not trust.
    #[cfg(test)]
    pub fn with_stale_push_after_save(self) -> SimDevice {
        self.state.lock().expect("sim lock").post_save_push = Some(PostSavePush::LoadTimeEcho);
        self
    }

    /// Like [`with_stale_push_after_save`], but the post-save push carries `json` — lets a
    /// test hand the Copy read-back a document that DOES show the acked edit.
    #[cfg(test)]
    pub fn with_post_save_push(self, json: &str) -> SimDevice {
        self.state.lock().expect("sim lock").post_save_push =
            Some(PostSavePush::Doc(json.to_string()));
        self
    }

    /// Seed slot `slot0`'s (0-based) saved `lastLoadedScene` — what a subsequent
    /// `loadPreset` for that slot restores `current_scene` to. Lets a test drive the
    /// "loading a preset activates its saved scene, not base" behavior without first
    /// driving a `loadScene` + `saveCurrentPreset` round-trip.
    #[cfg(test)]
    pub fn with_saved_scene(self, slot0: u32, scene: Option<u32>) -> SimDevice {
        self.state
            .lock()
            .expect("sim lock")
            .saved_scene
            .insert(slot0, scene);
        self
    }

    /// Override the preset JSON a `loadPreset` echoes as `currentPresetDataChanged`(3) —
    /// the pre-edit roster the `blockcaps` guard reads. Lets a test configure a specific
    /// `audioGraph` (e.g. one already at a block-count cap) to exercise the guard's
    /// refusal end-to-end.
    #[cfg(test)]
    pub fn with_preset_json(self, json: &str) -> SimDevice {
        {
            let mut st = self.state.lock().expect("sim lock");
            st.preset_json = json.to_string();
            st.parsed_preset_cache = None; // the source string just changed — drop the stale parse
        }
        self
    }

    /// Test-only: fix the lazy-commit latency (bypasses `TMP_SIM_COMMIT_LATENCY_MS` so
    /// parallel unit tests never race each other over a shared env var). The Playwright
    /// spec instead uses the `POST /sim/commit-latency` bridge route → [`set_commit_latency`]
    /// (this SimDevice is already installed and running by the time the spec starts).
    #[cfg(all(test, feature = "e2e"))]
    pub fn with_commit_latency(self, ms: u64) -> SimDevice {
        self.state.lock().expect("sim lock").commit_latency_override =
            Some(std::time::Duration::from_millis(ms));
        self
    }

    /// `cut_before` = the first amp node id drops every amp, and — `BTreeMap` keys, `guitarNodes`
    /// before `template` — that scene's routing template with them.
    #[cfg(all(test, feature = "e2e"))]
    pub fn with_truncated_scene_push(self, scene: u32, cut_before: &str) -> SimDevice {
        self.state.lock().expect("sim lock").truncated_scene_push =
            Some((scene, cut_before.to_string()));
        self
    }

    /// The CURRENT `preset_level` — test-only introspection for the lazy-commit specs
    /// (mirrors [`SimDevice::bypass_write`] / [`SimDevice::param_write`]).
    #[cfg(all(test, feature = "e2e"))]
    pub fn preset_level(&self) -> f32 {
        self.state.lock().expect("sim lock").preset_level
    }

    /// Seed the song / setlist names (slot = index + 1) the live read-back returns —
    /// used by the offline marketing-screenshot showcase to display curated, non-personal
    /// songs instead of the generic defaults. Read-back-after-write CRUD still mutates them.
    #[cfg(feature = "e2e")]
    pub fn with_songs(self, songs: Vec<String>, setlists: Vec<String>) -> SimDevice {
        {
            let mut st = self.state.lock().expect("sim lock");
            st.songs = songs;
            st.setlists = setlists;
        }
        self
    }

    /// The ordered list of device actions observed so far.
    #[cfg(any(test, feature = "e2e"))]
    pub fn events(&self) -> Vec<SimEvent> {
        self.state.lock().expect("sim lock").events.clone()
    }

    /// The current song names (read-back-after-write CRUD mutates them).
    #[cfg(all(test, feature = "e2e"))]
    pub fn song_names(&self) -> Vec<String> {
        self.state.lock().expect("sim lock").songs.clone()
    }

    /// The value last written to `node`'s `bypass` via `changeParameter`'s BOOL path
    /// (`ChangeParameter.boolVal`), or `None` if never written. Node-keyed only (matches
    /// `bypass_writes`) — the sim's bypass model isn't scene-scoped.
    #[cfg(test)]
    pub fn bypass_write(&self, node: &str) -> Option<bool> {
        self.state
            .lock()
            .expect("sim lock")
            .bypass_writes
            .get(node)
            .copied()
    }

    /// The value `(scene, group, node, param)` currently holds — the post-reseed STATE, not
    /// the event log. Distinct on purpose: enabling Scene Edit rewrites `param_writes`
    /// entries no `changeParameter` ever touched (the reseed), so only this can tell whether
    /// a scene param SURVIVED. `scene` is the `param_writes` key ([`SCENE_BASE`] for base).
    #[cfg(test)]
    pub fn param_write(&self, scene: i64, group: &str, node: &str, param: &str) -> Option<f32> {
        self.state
            .lock()
            .expect("sim lock")
            .param_writes
            .get(&(
                scene,
                group.to_string(),
                node.to_string(),
                param.to_string(),
            ))
            .copied()
    }

    /// The EFFECTIVE value of `(group, node, param)` a `loadScene` recall renders RIGHT
    /// NOW for the CURRENTLY active scene — [`SimState::rendered_param`]. Test-only.
    #[cfg(test)]
    pub fn rendered_param(&self, group: &str, node: &str, param: &str) -> Option<f64> {
        self.state
            .lock()
            .expect("sim lock")
            .rendered_param(group, node, param)
    }

    /// The EFFECTIVE `bypass` a `loadScene` recall renders RIGHT NOW for `(group, node)` —
    /// [`SimState::rendered_bypass`]. Test-only.
    #[cfg(test)]
    pub fn rendered_bypass(&self, group: &str, node: &str) -> Option<bool> {
        self.state
            .lock()
            .expect("sim lock")
            .rendered_bypass(group, node)
    }

    /// Parse one request body and produce the device's framed reply reports.
    fn handle(&self, body: &[u8]) -> Vec<Vec<u8>> {
        let top = proto::parse(body);
        let Some(pm) = proto::first_bytes(&top, TMS_PRESET) else {
            // Song (11) / Setlist (12) CRUD; else heartbeat / connection / settings (ignored).
            if let Some(sm) = proto::first_bytes(&top, TMS_SONG) {
                return self.handle_list_msg(sm, true);
            }
            if let Some(slm) = proto::first_bytes(&top, TMS_SETLIST) {
                return self.handle_list_msg(slm, false);
            }
            // SettingsMessage(3) → reampModeActive(30) → { value(1) } — the re-amp toggle
            // (ON=`1a05f201020801`, OFF=`1a03f20100`). Latched for the capture model; the
            // real device acks nothing, so no reply.
            if let Some(sm) = proto::first_bytes(&top, TMS_SETTINGS) {
                if let Some(re) = proto::first_bytes(&proto::parse(sm), F_REAMP_SETTING) {
                    let on = proto::first_varint(&proto::parse(re), 1).unwrap_or(0) != 0;
                    let mut st = self.state.lock().expect("sim lock");
                    st.reamp_on = on;
                    st.events.push(SimEvent::ReAmp(on));
                }
            }
            return Vec::new();
        };
        let f = proto::parse(pm);
        let mut st = self.state.lock().expect("sim lock");

        if let Some(lp) = proto::first_bytes(&f, F_LOAD_PRESET) {
            let dev_slot = proto::first_varint(&proto::parse(lp), 6).unwrap_or(0);
            let slot0 = dev_slot.saturating_sub(1) as u32;
            st.events.push(SimEvent::Loaded(slot0));
            // A load activates the preset's saved `lastLoadedScene` (HW-confirmed — a bare
            // load does NOT reset to base) and discards the edit buffer (the scene-scoped
            // knob writes + forced bypasses) — reseeded from the slot's own COMMITTED doc
            // just below (e2e only; see that block's doc for why a plain build can't).
            st.current_slot = slot0;
            // A different slot resolves to a different `scene_render_json_source` (e2e:
            // its own scenario doc) — drop the stale cached parse.
            st.parsed_preset_cache = None;
            st.current_scene = match st.saved_scene.get(&slot0).copied() {
                Some(scene) => scene,
                // No recorded save this run → the FIXTURE's own `lastLoadedScene`, so a
                // scenario preset saved in a scene (404) activates it offline exactly as the
                // unit does, with no per-test setup.
                None => fixture_last_loaded_scene(slot0),
            };
            st.param_writes.clear();
            st.bypass_writes.clear();
            st.scene_edit_enabled.clear();
            st.ftsw_working = None;
            // Lazy-commit doc (e2e only — module header): a load restores THIS slot's own
            // committed `presetLevel` AND baked param overlay, faithfully INCLUDING the
            // stale-load corruption window while an earlier save is still pending — but
            // ONLY once this run has actually SAVED slot0 at least once (`ever_saved`). A
            // slot nobody has saved this run keeps the OLD "preserve the last-set value"
            // behavior unperturbed: restoring unconditionally on EVERY load (this run's
            // first cut) changed the ambient `preset_level` for scene/base-only offline
            // fixtures that never save a presetLevel at all, silently shifting their
            // calibrated measured LUFS and breaking specs unrelated to the stale-load
            // incident (caught only by the full offline Playwright e2e suite, not
            // `cargo test --lib` alone). A plain (non-e2e) build has no per-slot
            // scenario doc to consult, so `preset_level` keeps its old behavior
            // unconditionally, and
            // `param_writes` simply starts empty (today's behavior).
            #[cfg(feature = "e2e")]
            if st.ever_saved.contains(&slot0) {
                st.preset_level = st.committed_doc(slot0).preset_level;
                // Reseed the SCENE_BASE param_writes a footswitch bake previously wrote and
                // saved — without this, a fresh load (the `measure_sound_asis_strict`/
                // `e2e_measure_sound` re-measure seam, or a later leveling batch on the same
                // slot) reads NO write for the leveled param and `model_lufs` treats the
                // switch as "engaged with nothing written" → silence, even though the real
                // device would still be sounding the SAVED knob value.
                for ((group, node, param), v) in st.committed_doc(slot0).params.clone() {
                    st.param_writes.insert((SCENE_BASE, group, node, param), v);
                }
            }
            // Echo `currentPresetDataChanged`(3) right after the load — the real device's
            // post-load push the `blockcaps` guard reads as the pre-edit roster
            // (`Session::current_preset_value`). May exceed one HID frame, so chunk it.
            let json = load_echo_json(&mut st, slot0);
            let mut reports = vec![frame(&preset_loaded(dev_slot))];
            reports.extend(frame_multi(&current_preset_data_changed(&json)));
            return reports;
        }
        if proto::first_bytes(&f, F_CURRENT_PRESET_DATA_REQUEST).is_some() {
            // `currentPresetDataRequest`(2) → a fresh `currentPresetDataChanged`(3) push of the
            // WORKING COPY (unsaved `ftsw` edits included — `load_echo_json`). This is the
            // device's re-prompt, and the ONLY confirm channel the no-dedicated-echo setters
            // have: `Session::live_ftsw` sends exactly this and reads `ftsw` off the reply.
            // Also fires inside the handshake burst, which is faithful — the real unit pushes
            // its current preset there too.
            let current_slot = st.current_slot;
            let json = load_echo_json(&mut st, current_slot);
            return frame_multi(&current_preset_data_changed(&json));
        }
        if let Some(dr) = proto::first_bytes(&f, F_PRESET_DATA_REQUEST) {
            // presetDataRequest{ listEnum(1), presetSlot(2) } → presetDataChanged(9) with the
            // slot's SAVED document. THE offline field-8 read: `read_saved_preset` feeds
            // `set_knobs`' overlay classification + the footswitch bake gate, and without a
            // reply every scene/footswitch write is refused ("no saved-preset read").
            let dev_slot = proto::first_varint(&proto::parse(dr), 2).unwrap_or(0);
            let slot0 = dev_slot.saturating_sub(1) as u32;
            return match saved_slot_json_body(&mut st, slot0) {
                // Plaintext (NOT lz4): every read path does `from_utf8_lossy` on field 9.
                Some(j) => frame_multi(&preset_data_changed(dev_slot, j.as_bytes())),
                None => Vec::new(), // non-scenario slot: the read times out, as offline today
            };
        }
        if let Some(rn) = proto::first_bytes(&f, F_REPLACE_NODE) {
            let (group, node_id, fender_id) = three_strings(rn);
            st.events.push(SimEvent::Replace {
                group,
                node_id,
                fender_id,
            });
            return structural_reply(&mut st, F_NODE_REPLACED);
        }
        if let Some(rb) = proto::first_bytes(&f, F_REPLACE_WITH_BLOCK) {
            let (group, node_id, fender_id) = three_strings(rb);
            let index = proto::first_varint(&proto::parse(rb), 4).unwrap_or(0);
            st.events.push(SimEvent::ReplaceWithBlock {
                group,
                node_id,
                fender_id,
                index,
            });
            return structural_reply(&mut st, F_NODE_REPLACED);
        }
        if let Some(ins) = proto::first_bytes(&f, F_INSERT_NODE) {
            let inner = proto::parse(ins);
            let group = str_field(&inner, 1);
            // field-2 = the FenderId to insert BEFORE (None → append).
            let before =
                proto::first_bytes(&inner, 2).map(|b| String::from_utf8_lossy(b).into_owned());
            let fender_id = str_field(&inner, 3);
            st.events.push(SimEvent::Insert {
                group,
                before,
                fender_id,
            });
            return structural_reply(&mut st, F_NODE_INSERTED);
        }
        if let Some(rm) = proto::first_bytes(&f, F_REMOVE_NODE) {
            let inner = proto::parse(rm);
            st.events.push(SimEvent::Remove {
                group: str_field(&inner, 1),
                node_id: str_field(&inner, 2),
            });
            return structural_reply(&mut st, F_NODE_REMOVED);
        }
        if let Some(rename) = proto::first_bytes(&f, F_RENAME) {
            st.events
                .push(SimEvent::Renamed(str_field(&proto::parse(rename), 1)));
            return Vec::new();
        }
        if let Some(save) = proto::first_bytes(&f, F_SAVE) {
            let dev_slot = proto::first_varint(&proto::parse(save), 1).unwrap_or(0);
            let slot0 = dev_slot.saturating_sub(1) as u32;
            let scene = st.current_scene;
            st.saved_scene.insert(slot0, scene);
            st.events.push(SimEvent::Saved(slot0));
            if let Some(push) = st.post_save_push.clone() {
                let json = match push {
                    // This fake never mutates its served document on a structural edit,
                    // so the echo is exactly the LOAD-TIME (pre-edit) graph the real unit
                    // handed the Copy post-save read-back (HW 2026-09-02, fw 1.8.45).
                    PostSavePush::LoadTimeEcho => load_echo_json(&mut st, slot0),
                    PostSavePush::Doc(doc) => doc.into_bytes(),
                };
                let push = frame_multi(&current_preset_data_changed(&json));
                st.pending_pushes.extend(push);
            }
            // Lazy-commit: this becomes slot0's PENDING doc (presetLevel + baked params),
            // landing after `commit_latency()` (module header) — a same-slot load before
            // that deadline must still see the OLD committed doc.
            #[cfg(feature = "e2e")]
            st.record_save(slot0);
            return Vec::new();
        }
        if let Some(spl) = proto::first_bytes(&f, F_SET_PRESET_LEVEL) {
            let level = proto::parse(spl)
                .iter()
                .find(|(n, _)| *n == 1)
                .and_then(|(_, v)| v.as_f32())
                .unwrap_or(0.0);
            st.events.push(SimEvent::PresetLevel(level));
            st.preset_level = level;
            return vec![frame(&preset_level_changed(level))];
        }
        if let Some(ls) = proto::first_bytes(&f, F_LOAD_SCENE) {
            // LoadScene{ sceneSlot(1) } — the 0-based scenes[] wire index, OR the
            // `BASE_SCENE_SLOT` (8) sentinel that recalls base (not a real `scenes[]`
            // entry — HW-confirmed; `session::BASE_SCENE_SLOT`).
            let wire_slot = proto::first_varint(&proto::parse(ls), 1).unwrap_or(0) as u32;
            st.current_scene = if wire_slot == crate::session::BASE_SCENE_SLOT {
                None
            } else {
                Some(wire_slot)
            };
            st.events.push(SimEvent::LoadScene(wire_slot));
            // A recall runs the device's own level-apply — base included — silently
            // reverting an unsaved working-copy `presetLevel` to the currently-SAVED
            // value (HW: `probe --levelpreset 400 -24 save` solved 0.3096 and the saved
            // doc still read the prior 0.32; `leveller::recall_reassert_save`'s doc has
            // the full evidence). Mirrors `load_preset`'s own committed-level restore
            // exactly — same e2e-only gate, same `ever_saved` condition — rather than
            // inventing a parallel rule.
            let current_slot = st.current_slot;
            #[cfg(feature = "e2e")]
            if st.ever_saved.contains(&current_slot) {
                st.preset_level = st.committed_doc(current_slot).preset_level;
            }
            // Push `currentPresetDataChanged`(3) so the un-engaged scene-leveling PRE-PASS can
            // classify each scene's routing (the real device pushes the scene graph on a scene
            // CHANGE). The sim carries one graph, and the physics model reads the written
            // `outputLevel` node-agnostically, so re-serving the same graph per scene is enough
            // for the amp pick to resolve — without it, the pre-pass harvests nothing and every
            // scene fails to classify ("read failed").
            let json = load_echo_json(&mut st, current_slot);
            return frame_multi(&current_preset_data_changed(&json));
        }
        if let Some(cp) = proto::first_bytes(&f, F_CHANGE_PARAMETER) {
            // changeParameter{ group(1), node(2), param(3), floatVal(5) | boolVal(7) }.
            let inner = proto::parse(cp);
            let (group, node, param) = (
                str_field(&inner, 1),
                str_field(&inner, 2),
                str_field(&inner, 3),
            );
            let scene = st.scene_key();
            let float_val = inner
                .iter()
                .find(|(n, _)| *n == 5)
                .and_then(|(_, val)| val.as_f32());
            if let Some(v) = float_val {
                st.events.push(SimEvent::ChangeParameter {
                    // The event records the ACTIVE-SCENE CONTEXT the write was sent
                    // under, not where it landed (`scene_context_tests` asserts on this
                    // meaning) — the landing key below can differ per fact 4.
                    scene,
                    group: group.clone(),
                    node: node.clone(),
                    param: param.clone(),
                    value: v,
                });
                // Fact 4 (HW fw 1.8.45, revised): a scene-context write with NO preceding
                // `setNodeSceneEdit(enable=true)` for this (scene, group, node) lands on
                // BASE only when that node's overlay in this scene is NOT Full-shaped
                // (BypassOnly, empty, or altogether absent — Scene Edit disabled/never
                // materialized). Against a FULL-shaped overlay the write lands ON the
                // overlay EVEN FOR a param it doesn't yet carry, extending it per-param —
                // the Scene-Edit flag state decides the landing, not per-param containment
                // (see `overlay_is_full_shaped`'s doc for the crafted-fixture HW evidence).
                // Copied out of `st` up front: `overlay_is_full_shaped` below now needs
                // `&mut st` (the cached-parse lookup), which the match scrutinee borrowing
                // `st.current_scene` directly would conflict with.
                let current_scene = st.current_scene;
                let landing_scene = match current_scene {
                    Some(sc)
                        if !st
                            .scene_edit_enabled
                            .contains(&(sc, group.clone(), node.clone()))
                            && !st.overlay_is_full_shaped(sc, &node) =>
                    {
                        SCENE_BASE
                    }
                    _ => scene,
                };
                st.param_writes
                    .insert((landing_scene, group, node, param), v);
            } else if param == "bypass" {
                let on = proto::first_varint(&inner, 7).unwrap_or(0) != 0;
                st.events.push(SimEvent::Bypass {
                    node: node.clone(),
                    on,
                });
                st.bypass_writes.insert(node, on);
            }
            return Vec::new();
        }
        if let Some(se) = proto::first_bytes(&f, F_SET_NODE_SCENE_EDIT) {
            // SetNodeSceneEdit{ nodeId(1), groupId(2), sceneEditEnable(3) }.
            let inner = proto::parse(se);
            let node = str_field(&inner, 1);
            let group = str_field(&inner, 2);
            let enable = proto::first_varint(&inner, 3).unwrap_or(0) != 0;
            st.events.push(SimEvent::SceneEdit {
                group: group.clone(),
                node: node.clone(),
                enable,
            });
            // Fact 4's gate (F_CHANGE_PARAMETER, above): track which (scene, group, node)
            // triples have had Scene Edit enabled THIS session, since the last load.
            if let Some(sc) = st.current_scene {
                if enable {
                    st.scene_edit_enabled
                        .insert((sc, group.clone(), node.clone()));
                } else {
                    st.scene_edit_enabled
                        .remove(&(sc, group.clone(), node.clone()));
                }
            }
            // HW-confirmed (B): enabling Scene Edit on a node RESEEDS that node's scene
            // overlay from base — any prior override for OTHER params on this node in the
            // active scene is lost, replaced by whatever the node currently holds at base.
            // The offline model can only see explicit writes, so it reseeds from base's
            // recorded `param_writes` (the [`SCENE_BASE`] entries) rather than the true
            // stored/firmware value — a param never explicitly written at base has nothing
            // to reseed from and is simply dropped, a known offline-fidelity gap.
            let scene_key = st.scene_key();
            if enable && scene_key != SCENE_BASE {
                let mut reseed = Vec::new();
                st.param_writes.retain(|(s, g, n, p), v| {
                    if *s == SCENE_BASE && *g == group && *n == node {
                        reseed.push(((scene_key, g.clone(), n.clone(), p.clone()), *v));
                        true
                    } else {
                        !(*s == scene_key && *g == group && *n == node)
                    }
                });
                st.param_writes.extend(reseed);
            }
            return Vec::new();
        }
        if let Some(sfa) = proto::first_bytes(&f, F_SET_FOOTSWITCH_ASSIGNMENT) {
            // SetFootswitchAssignment{ footswitchAddress(1), functionIndex(2), functionJson(3),
            // swap(4) } — arrives through `Session::send_chunked_collect` (a `param`
            // functionJson overflows one 60 B report), but the fake's `transact_chunked` is
            // handed the WHOLE assembled body, so there is nothing to reassemble here.
            //
            // Applies to the WORKING COPY (`ftsw_working`), NOT the saved doc: on hardware the
            // edit is live-but-unsaved until a `saveCurrentPreset`, and it is the field-3
            // re-prompt above — never a dedicated echo — that confirms it, which is exactly the
            // read-back branch `leveller::write_fs_values_on_session` falls to. Hence NO reply
            // (`Session::set_footswitch_assignment`'s "no dedicated confirm echo").
            let inner = proto::parse(sfa);
            let addr = proto::first_varint(&inner, 1).unwrap_or(0) as u32;
            let index = proto::first_varint(&inner, 2).unwrap_or(0) as u32;
            let function_json = str_field(&inner, 3);
            // The wire flag verbatim. Its device semantics are empirically TBD
            // (`proto::set_footswitch_assignment`; `probe --ftsw-validate` is the resolver) and
            // production always sends `false`, so the fake records it and models no behavioural
            // difference rather than inventing one.
            let swap = proto::first_varint(&inner, 4).unwrap_or(0) != 0;
            st.events.push(SimEvent::SetFootswitchAssignment {
                addr,
                index,
                function_json: function_json.clone(),
                swap,
            });
            if let Ok(func) = serde_json::from_str::<serde_json::Value>(&function_json) {
                st.ftsw_set(addr, index, func);
            }
            return Vec::new();
        }
        if let Some(cfa) = proto::first_bytes(&f, F_CLEAR_FOOTSWITCH_ASSIGNMENT) {
            // ClearFootswitchAssignment{ footswitchAddress(1), functionIndex(2) } — the same
            // working-copy + no-echo model as field 54 above.
            let inner = proto::parse(cfa);
            let addr = proto::first_varint(&inner, 1).unwrap_or(0) as u32;
            let index = proto::first_varint(&inner, 2).unwrap_or(0) as u32;
            st.events
                .push(SimEvent::ClearFootswitchAssignment { addr, index });
            st.ftsw_clear(addr, index);
            return Vec::new();
        }
        if proto::first_bytes(&f, F_NODE_JSON_REQUEST).is_some() {
            // The edit-context preamble: the device replies `nodeJsonResponse`(120), but
            // `replace_node`/`remove_node` ignore that reply — an empty ack suffices.
            return Vec::new();
        }
        Vec::new()
    }

    /// Handle a `SongMessage`(11) / `SetlistMessage`(12): a list request replies with the
    /// current list (single frame); add/remove/rename mutate the in-memory state so the
    /// app's read-back-after-write sees the change. `notes`/`bpm`/membership setters are
    /// accepted and ignored (they don't affect the name-list the CRUD spec asserts).
    fn handle_list_msg(&self, inner_bytes: &[u8], is_song: bool) -> Vec<Vec<u8>> {
        let f = proto::parse(inner_bytes);
        let tms = if is_song { TMS_SONG } else { TMS_SETLIST };
        let mut st = self.state.lock().expect("sim lock");
        let list = if is_song {
            &mut st.songs
        } else {
            &mut st.setlists
        };
        if !is_song && proto::first_bytes(&f, F_SETLIST_SONGS_REQUEST).is_some() {
            // Empty but COMPLETE membership response so the read resolves (no modeled songs).
            let resp = proto::len_delimited(
                TMS_SETLIST,
                &proto::len_delimited(F_SETLIST_SONGS_RESPONSE, &[]),
            );
            return vec![frame(&resp)];
        }
        if proto::first_bytes(&f, F_LIST_REQUEST).is_some() {
            return frame_multi(&list_response(tms, list));
        }
        if let Some(add) = proto::first_bytes(&f, F_LIST_ADD) {
            list.push(str_field(&proto::parse(add), 1));
            return Vec::new();
        }
        if let Some(rm) = proto::first_bytes(&f, F_LIST_REMOVE) {
            let slot = proto::first_varint(&proto::parse(rm), 1).unwrap_or(0) as usize;
            if slot >= 1 && slot <= list.len() {
                list.remove(slot - 1);
            }
            return Vec::new();
        }
        if let Some(rn) = proto::first_bytes(&f, F_LIST_RENAME) {
            let inner = proto::parse(rn);
            let slot = proto::first_varint(&inner, 1).unwrap_or(0) as usize;
            let name = str_field(&inner, 2);
            if slot >= 1 && slot <= list.len() {
                list[slot - 1] = name;
            }
            return Vec::new();
        }
        Vec::new()
    }
}

/// Offline-e2e handle to the CURRENTLY-installed fake so the bridge's `/sim/events`
/// endpoint can read its event log (the SimDevice is otherwise reachable only from inside
/// the transport-factory closure). Set by `e2e_install_offline_fake` — re-set on every
/// `/sim/reset`, which installs a fresh device with an empty log. Online never installs the
/// fake, so this stays `None` and `live_events()` returns `[]`.
#[cfg(feature = "e2e")]
static LIVE: Mutex<Option<SimDevice>> = Mutex::new(None);

/// Record the installed fake as the live handle for `/sim/events`.
#[cfg(feature = "e2e")]
pub fn set_live(dev: &SimDevice) {
    *LIVE.lock().expect("sim live lock") = Some(dev.clone());
}

/// The current fake's ordered event log (empty when no fake is installed).
#[cfg(feature = "e2e")]
pub fn live_events() -> Vec<SimEvent> {
    LIVE.lock()
        .expect("sim live lock")
        .as_ref()
        .map(SimDevice::events)
        .unwrap_or_default()
}

// ─── offline physics-faithful capture model (`--features e2e`) ──────────────────────
//
// The single injection point is `audio::reamp_capture`'s offline branch. Instead of
// returning the stimulus verbatim (which made loudness bugs invisible offline), it drives
// the real loudness law the leveller assumes — `captured_LUFS = 20·log10(presetLevel) + C`
// — plus a scene-relative `outputLevel` term, so the offline e2e suite becomes a genuine
// oracle. The model is DETERMINISTIC (no noise): a scaled copy of the fixed stimulus whose
// measured LUFS lands exactly at the modeled value. Scaling changes only LEVEL, never the
// spectrum, so the Doctor's spectral diagnosis is unaffected.
//
// GRAPH ECHO (the PR3 faithful fix — `load_echo_json` + `scenario_json_for`): `loadPreset` and
// `loadScene` echo each SCENARIO slot's REAL presetJson as `currentPresetDataChanged`(3), so
// offline scene-leveling's prepass classifies each scene against the SAME amp node the
// backup-derived candidates name (`written == stored` → faithful convergence AND a correct amp
// pick for the parallel/split templates). Without it the prepass doc (a constant default graph)
// didn't contain the backup candidate's amp, so `build_scene_jobs` failed to match and every
// scene read-failed. Any non-scenario slot (and all non-e2e builds) still uses the default graph.
//
// FIDELITY DECISIONS (PR3, updated by the stale-load-fix PR's `leveled_params` addition):
//  • The "clamped-at-max" outcome is authored on Base (presetLevel → C clamp at LEVEL_MAX,
//    fully faithful); a scene-outputLevel CLAMP verdict is still NOT faithfully authorable
//    (the closed-loop verify converges regardless of the sidecar clamp) → base-path only.
//  • `SlotLoudness::offbranch_switch_node` models a block-acting footswitch whose toggled
//    block sits on a MUTED parallel branch (engaging it routes to dead air) — the model reads
//    `bypass_writes` directly, no field-8 needed, so this ONE off-branch shape has always been
//    offline-faithful (the field-8 read `saved_slot_json_body` performs is for the LEVELED-PARAM
//    knob curve below, a separate mechanism).
//  • STALE (superseded): earlier revisions of this comment called offline footswitch leveling
//    "unmodeled (out of PR3 scope)" and listed it under "online-only classes" below.
//    `SlotLoudness::leveled_params` (a drive pedal's own knob → `saturated_pedal_lufs`, module
//    header) now models it — see `e2e/specs/level-fs-preset24.spec.ts` for full offline
//    footswitch-leveling coverage, including a save→load round trip of the baked value
//    (`SimState::param_writes` reseeded from the lazy-commit `SavedDoc` on load).
//  • The ASSIGN-path footswitch WIRE flow is modeled too (module header): `handle` applies
//    `setFootswitchAssignment`(54)/`clearFootswitchAssignment`(55) to the working-copy `ftsw`,
//    the field-2 re-prompt renders it back for the confirm gate, and a save persists it into
//    `SavedDoc::ftsw` so the field-8 persist verify and the `ensure_fresh_load` witness both
//    resolve. The CAPTURE side is modeled too, as of the wet-floor fix: `model_lufs`'s
//    leveled-param predicate also fires on "no bypass write for the node AND the recall
//    renders it ENGAGED", which is exactly an Assign's isolation shape
//    (`siblings_off_excluding` forces only the OTHER switches' blocks off). A swept param
//    still only moves the modeled loudness when the slot declares it in `leveledParams`, so
//    an assign on an UNDECLARED param reads a flat response (a headroom clamp) rather than
//    the curve the real preset has.
//  • Two leveled-param curves, deliberately asymmetric (`LeveledCurve`): the drive pedal's
//    `saturatedPedal` REPLACES the flat law (an absolute post-DSP loudness), while `wetMix`
//    is a DELTA added to it and is zero unless the run actually WROTE the param — so
//    declaring a wet-mix param cannot perturb any capture that does not sweep it.
// What offline-green STILL does NOT prove: a scene-outputLevel CLAMP verdict, the loudness
// RESPONSE of any param outside `leveledParams`, and parallel-lane summation (joint-k) —
// still online-only classes. The wet-mix curve is NOT HW-measured (see `wet_mix_gain_db`):
// an offline wet solve proves the OUTCOME PLUMBING, never the real preset's numbers.

/// Arm the currently-installed fake so `slot`'s NEXT capture returns silence once (the
/// `POST /sim/fault` bridge endpoint). No-op when no fake is installed (online).
#[cfg(feature = "e2e")]
pub fn arm_capture_fault(slot: u32) {
    if let Some(dev) = LIVE.lock().expect("sim live lock").as_ref() {
        dev.state.lock().expect("sim lock").fail_capture_slot = Some(slot);
    }
}

/// Arm the currently-installed fake's lazy-commit latency (the `POST /sim/commit-latency`
/// bridge endpoint) — the spec-side way to reproduce the same-slot stale-load incident
/// against the ONE already-running offline server process, where a per-test env var
/// can't reach a process that started before the test did. No-op when no fake is
/// installed (online). `/sim/reset` installs a fresh fake with latency back at 0, so this
/// never leaks past the test that armed it.
#[cfg(feature = "e2e")]
pub fn set_commit_latency(ms: u64) {
    if let Some(dev) = LIVE.lock().expect("sim live lock").as_ref() {
        dev.state.lock().expect("sim lock").commit_latency_override =
            Some(std::time::Duration::from_millis(ms));
    }
}

/// Offline re-amp capture: read the installed fake's DSP state, compute the modeled
/// loudness, and return a stimulus scaled to hit it. Falls back to the stimulus
/// passthrough (the pre-physics behavior) when no fake is installed — a direct Rust
/// leveller test that did not call [`set_live`], or the showcase tour (which injects
/// Doctor profiles rather than measuring). The runtime online guard in
/// `audio::reamp_capture` means this is never reached online.
#[cfg(feature = "e2e")]
pub fn e2e_capture(stimulus: &[f32], rate: u32) -> crate::audio::Capture {
    match LIVE.lock().expect("sim live lock").as_ref() {
        Some(dev) => dev.e2e_capture(stimulus, rate),
        None => {
            log::debug!("e2e_capture: no live SimDevice — stimulus passthrough");
            passthrough(stimulus, rate)
        }
    }
}

#[cfg(feature = "e2e")]
impl SimDevice {
    /// Compute this fake's modeled capture (see the module comment). Silence (→ the
    /// leveller's no-signal path) for a capture-fault, re-amp OFF, or an off-branch sound.
    fn e2e_capture(&self, stimulus: &[f32], rate: u32) -> crate::audio::Capture {
        let mut st = self.state.lock().expect("sim lock");
        if st.fail_capture_slot == Some(st.current_slot) {
            st.fail_capture_slot = None; // one-shot
            return silent_capture(stimulus.len(), rate);
        }
        // Re-amp must be engaged for the device to route a USB return.
        if !st.reamp_on {
            return silent_capture(stimulus.len(), rate);
        }
        match model_lufs(&mut st, sidecar(), stored_levels()) {
            Some(l_model) => scale_stimulus(stimulus, rate, l_model),
            None => silent_capture(stimulus.len(), rate), // off-branch
        }
    }
}

#[cfg(feature = "e2e")]
impl SimState {
    /// The `outputLevel` the leveller wrote for the scene under measurement (node-agnostic:
    /// there is one amp per sound in the fixtures, so at most one such write). `None` when
    /// nothing was written → the model reads the stored knob (a 0 LU shift — the locked
    /// relative-`outputLevel` convention).
    fn scene_output_level(&self) -> Option<f32> {
        let scene = self.scene_key();
        self.param_writes
            .iter()
            .find(|((s, _, _, p), _)| *s == scene && p == "outputLevel")
            .map(|(_, v)| *v)
    }
}

/// The modeled captured LUFS: `C[slot, scene] + 20·log10(presetLevel) +
/// 20·log10(outputLevel_written / outputLevel_stored)`. `None` = off-branch (silence): an
/// engaged switch has bypassed the sidecar's routed amp node. Pure — the caller supplies
/// the sidecar C table and the presetJson-derived stored knob map.
#[cfg(feature = "e2e")]
fn model_lufs(
    st: &mut SimState,
    sidecar: &Sidecar,
    stored: &std::collections::HashMap<u32, StoredLevels>,
) -> Option<f64> {
    let entry = sidecar.slots.get(&st.current_slot.to_string());
    // Off-branch: the routed amp forced bypassed → muted sound → silence.
    if let Some(node) = entry.and_then(|e| e.routed_node.as_ref()) {
        if st.bypass_writes.get(node).copied().unwrap_or(false) {
            return None;
        }
    }
    // Off-branch FOOTSWITCH (a switch whose toggled block sits on a MUTED parallel
    // branch): silence when that block is ENGAGED (bypass=false). Distinct from
    // `routed_node` (silences when BYPASSED): the block is force-ON only while THAT
    // footswitch is under its own isolated measurement — Base + other-switch isolation
    // force every on-off block OFF (bypass=true), so those sounds stay measurable. This
    // makes exactly one footswitch off-branch in a whole-preset run without silencing Base
    // — the current model can't express that via `routed_node` (Base bypasses everything).
    if let Some(node) = entry.and_then(|e| e.offbranch_switch_node.as_ref()) {
        if st.bypass_writes.get(node).copied() == Some(false) {
            return None;
        }
    }
    // Leveled-param response: the declared block's OWN knob drives loudness, but only while
    // that block is ENGAGED for the sound under measurement. Two shapes count as engaged,
    // and both are needed because the two footswitch WRITE PLANS isolate differently:
    //  * `bypass_writes[node] == Some(false)` — the run force-ENGAGED it. That is a BAKE
    //    row's own isolated measurement (`FsLevelPlan::Bake`, 405's off-in-base pedals).
    //  * NO bypass write for the node AND the recall renders it engaged
    //    ([`SimState::rendered_bypass`]) — the run never touched it and the preset has it
    //    ON. That is an ASSIGN row (`FsLevelPlan::Assign`, every levelable switch on 400):
    //    `siblings_off_excluding` forces only the OTHER switches' blocks off, so the
    //    LEVELED block gets no bypass write at all. Without this arm the declared param had
    //    no authority over the capture on an Assign row and the solve read a flat response.
    // Base and every OTHER switch's isolated measurement force this block OFF
    // (`bypass_writes[node] == Some(true)`), so they fall through unperturbed.
    let leveled: Option<LeveledParam> = entry.and_then(|e| {
        e.leveled_params
            .iter()
            .find(|lp| match st.bypass_writes.get(&lp.node).copied() {
                Some(bypassed) => !bypassed,
                None => st.rendered_bypass(&lp.group, &lp.node) == Some(false),
            })
            .cloned()
    });
    let preset_term = 20.0 * f64::from(st.preset_level.max(1e-6)).log10();
    // The two curves' contributions are computed at ONE site (`leveled_contribution`) so a
    // future third `LeveledCurve` variant fails to compile there instead of silently reading
    // flat. What they DO with that contribution stays deliberately ASYMMETRIC — see
    // `Contribution` for why — and is resolved exhaustively right here too.
    let contribution = match leveled.as_ref() {
        Some(lp) => Some(leveled_contribution(st, lp)?),
        None => None,
    };
    let wet_term = match contribution {
        Some(Contribution::Absolute(c)) => return Some(c + preset_term),
        Some(Contribution::Delta(d)) => d,
        None => 0.0,
    };
    // Scene overlay C falls back to base; an unlisted preset falls back to a flat default.
    let c = entry.map_or(sidecar.default, |e| e.c_for(st.current_scene));
    let stored_ol = stored
        .get(&st.current_slot)
        .and_then(|s| s.output_level(st.current_scene))
        .unwrap_or(1.0);
    let written_ol = st.scene_output_level().unwrap_or(stored_ol);
    let ol_term = if stored_ol > 0.0 && written_ol > 0.0 {
        20.0 * (f64::from(written_ol) / f64::from(stored_ol)).log10()
    } else {
        0.0
    };
    Some(c + preset_term + ol_term + wet_term)
}

/// One [`LeveledParam`]'s contribution to the modeled loudness — the two curves are
/// deliberately ASYMMETRIC and must not be "unified":
///  * `Absolute` (`SaturatedPedal`) REPLACES the flat `C` law — it is an absolute post-DSP
///    loudness (a drive pedal into a saturated amp swamps the chain, so the sidecar's C no
///    longer describes the sound at all).
///  * `Delta` (`WetMix`) is added ON TOP of the flat law — a reverb's mix knob perturbs the
///    same calibrated sound the sidecar C already measures.
#[cfg(feature = "e2e")]
enum Contribution {
    Absolute(f64),
    Delta(f64),
}

/// Evaluate `lp`'s [`LeveledCurve`] into its [`Contribution`] — the ONE site both curves are
/// computed from (this used to be two complementary filters at two call sites in
/// `model_lufs`, one filtering `== SaturatedPedal` and early-returning, the other filtering
/// `!= WetMix` to 0.0; an exhaustive `match lp.curve` here means a future third variant fails
/// to compile instead of silently reading flat). `None` propagates `model_lufs`'s off-branch
/// silence for an engaged-but-unrendered `SaturatedPedal` block, not a curve failure.
#[cfg(feature = "e2e")]
fn leveled_contribution(st: &mut SimState, lp: &LeveledParam) -> Option<Contribution> {
    // The current param value for the sound under measurement — shared by both curves.
    let key = (
        st.scene_key(),
        lp.group.clone(),
        lp.node.clone(),
        lp.param.clone(),
    );
    let written = st.param_writes.get(&key).copied();
    match lp.curve {
        LeveledCurve::SaturatedPedal => {
            // The leveler's own sweep write when there is one; otherwise the block's
            // AUTHORED value, rendered through the same chain a `loadScene` recall uses
            // (`rendered_param`). Without that fallback a capture of this block ENGAGED but
            // NEVER WRITTEN — exactly a VERIFY row's engaged capture, which writes no param
            // by definition — returned `None` and the model reported it as off-branch
            // SILENCE, making the whole verify-vs-level distinction unobservable offline.
            let v = match written {
                Some(v) => v,
                None => st.rendered_param(&lp.group, &lp.node, &lp.param)? as f32,
            };
            Some(Contribution::Absolute(saturated_pedal_lufs(v)?))
        }
        LeveledCurve::WetMix => {
            // `0.0` whenever the run has written NO value for the declared param — the whole
            // safety argument for the widened activation predicate above. The sidecar's C is
            // calibrated on the sound AS AUTHORED, so an unswept capture must read exactly the
            // C it always did; making the term structurally zero (rather than arithmetically
            // zero via an anchor that happens to match) is what keeps a base run, a scene run,
            // a Doctor capture and a VERIFY row on slot 400 byte-identical to before this
            // branch existed — including scene 1 "Lead", whose FULL overlay authors a
            // different `mix` (0.55) than base.
            let Some(written) = written else {
                return Some(Contribution::Delta(0.0));
            };
            let delta = match st.authored_param(&lp.group, &lp.node, &lp.param) {
                Some(authored) => wet_mix_gain_db(f64::from(written), authored),
                None => 0.0,
            };
            Some(Contribution::Delta(delta))
        }
    }
}

/// The stereo meter's fixed gain over mono for a DUAL-MONO signal (identical
/// content on both channels): `10*log10(2)`, algebraically exact under BS.1770
/// regardless of the content (duplicating a channel doubles every per-block mean
/// square uniformly, so the relative gate's block selection — and hence which
/// blocks integrate — cannot change; see `lufs.rs::dual_mono_stereo_reads_3_01_over_mono`).
/// Ground-truthed against an external BS.1770 stereo meter (dual-mono +3.01 LU
/// over mono on the same clip, matching ffmpeg's independent `ebur128` to 0.02 LU).
#[cfg(feature = "e2e")]
const DUAL_MONO_GAIN_LU: f64 = 3.0103;

/// Duplicate a mono buffer onto both channels — the offline model of the TMP's
/// mirrored USB-Out 1/2. Dual-mono is the only stereo shape the offline sim ever
/// needs to emit: the fixture C-table models a single amp signal, never
/// independent L/R.
#[cfg(feature = "e2e")]
fn to_dual_mono(mono: &[f32], rate: u32) -> crate::audio::Capture {
    let mut interleaved = Vec::with_capacity(mono.len() * 2);
    for &s in mono {
        interleaved.push(s);
        interleaved.push(s);
    }
    crate::audio::Capture {
        interleaved,
        channels: 2,
        sample_rate: rate,
    }
}

/// Scale `stimulus` so its measured integrated LUFS, AS THE SIM WILL EMIT IT
/// (dual-mono, per `to_dual_mono`), lands exactly at `l_model`
/// (`s = 10^((l_model − DUAL_MONO_GAIN_LU − l_stim)/20)`). `l_stim` is measured
/// MONO (`measure_mono` — the input/stimulus-side convention never changes, T2),
/// so without the `DUAL_MONO_GAIN_LU` term the stereo meter would add its own
/// +3.01 ON TOP of a buffer already scaled to hit `l_model`, landing every
/// offline capture +3 too hot (plan finding F1 — the double-count trap this
/// constant closes; see `scale_stimulus_lands_on_model` for the contract proof
/// through the PRODUCTION measure path, not a hand-rolled stereo re-check).
/// Deterministic. A silent stimulus (non-finite `l_stim`) can't be modeled →
/// returned verbatim (still dual-mono).
#[cfg(feature = "e2e")]
fn scale_stimulus(stimulus: &[f32], rate: u32, l_model: f64) -> crate::audio::Capture {
    // ponytail: re-measure L_stim per capture — ebur128 over the fixed ~3 s WAV is sub-ms
    // and replaces a multi-second real capture, so caching it (keyed by stimulus identity,
    // which differs across tests) isn't worth the invalidation risk.
    let l_stim = crate::lufs::measure_mono(stimulus, rate)
        .map(|m| m.integrated_lufs)
        .unwrap_or(f64::NEG_INFINITY);
    if !l_stim.is_finite() {
        return passthrough(stimulus, rate);
    }
    let s = 10f64.powf((l_model - DUAL_MONO_GAIN_LU - l_stim) / 20.0) as f32;
    let mono: Vec<f32> = stimulus.iter().map(|x| x * s).collect();
    to_dual_mono(&mono, rate)
}

/// The stimulus, dual-mono — the pre-physics fallback (no live SimDevice /
/// unmodeled `l_stim`). Mirrors the real TMP's mirrored USB-Out (D1).
#[cfg(feature = "e2e")]
fn passthrough(stimulus: &[f32], rate: u32) -> crate::audio::Capture {
    to_dual_mono(stimulus, rate)
}

/// `n` samples of silence, dual-mono — `processed_loudness` reports "no signal
/// captured" on it, exactly as a real silent USB return does, driving the
/// leveller's no-signal / off-branch verdict.
#[cfg(feature = "e2e")]
fn silent_capture(n: usize, rate: u32) -> crate::audio::Capture {
    to_dual_mono(&vec![0.0; n.max(1)], rate)
}

/// Per-slot C values + the routed amp node, hand-authored in
/// `e2e/fixtures/scenario-loudness.json` (C + flags ONLY — the stored knob values are
/// derived from the presetJson in `scenario-presets.json`, never duplicated here).
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlotLoudness {
    /// The base sound's max-reachable loudness at `presetLevel = 1.0`, `outputLevel` stored.
    base: f64,
    /// Per-scene C (0-based `scenes[]` index); a missing entry inherits `base`.
    #[serde(default)]
    scenes: Vec<f64>,
    /// The routed sound's amp — an engaged switch bypassing it mutes the sound (off-branch
    /// → silence). Matched against `bypass_writes`, which is keyed by the WIRE `nodeId`
    /// (`changeParameter` field 2), so this must be that amp's **nodeId** — in the e2e
    /// fixtures `nodeId == FenderId`, so the FenderId here is also the nodeId. Optional
    /// (only presets with an off-branch case set it).
    #[serde(default)]
    routed_node: Option<String>,
    /// The **nodeId** of a block-acting footswitch's toggled block that sits on a MUTED
    /// parallel branch: engaging it (bypass=false — which happens only while that switch is
    /// under its own isolated measurement) routes to a dead branch → silence → the
    /// leveller's off-branch verdict. See the `model_lufs` off-branch checks. Optional.
    #[serde(default)]
    offbranch_switch_node: Option<String>,
    /// Blocks/params whose OWN knob drives the modeled loudness instead of (or on top of)
    /// the flat `C + preset_term + ol_term` law — see [`LeveledCurve`] for the two shapes.
    /// Activates only while that block is ENGAGED for the sound under measurement (the
    /// two-armed predicate in [`model_lufs`]); every other slot's empty default vector
    /// leaves the branch dead, so it can never perturb an existing scenario's numbers.
    #[serde(default)]
    leveled_params: Vec<LeveledParam>,
}

/// One block/param whose knob drives the modeled loudness — see
/// [`SlotLoudness::leveled_params`].
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize, Clone)]
struct LeveledParam {
    group: String,
    node: String,
    param: String,
    #[serde(default)]
    curve: LeveledCurve,
}

/// Which response a [`LeveledParam`]'s knob follows. The default keeps every pre-existing
/// sidecar entry (405's four drive pedals, written before this field existed) on the curve
/// it was authored against.
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase")]
enum LeveledCurve {
    /// [`saturated_pedal_lufs`] — an ABSOLUTE post-DSP loudness that REPLACES the flat law.
    #[default]
    SaturatedPedal,
    /// [`wet_mix_gain_db`] — a DELTA in LU added to the flat law.
    WetMix,
}

/// A wet/dry `mix` knob's loudness DELTA, in LU, moving from `authored` to `v` — the offline
/// response for slot 400's spring-reverb mix (`ACD_TMSpring63.mix`, the wet-floor fixture).
///
/// Shape: `20·log10(1 + m)`, i.e. the wet return summing with a unity dry path, its amplitude
/// scaled linearly by the mix control. Three properties earn it:
///  * **Monotonic and smooth** over the whole `[0, 1]` range, so the bounded secant in
///    `leveller::solve_param_secant` brackets and converges instead of chasing a dip — a
///    crossfade law (`(1−m)² + (k·m)²`) is NOT monotonic and would fight the solver.
///  * **Modest authority**: +6.02 dB from fully dry to full wet, which is what a reverb mix
///    actually buys you. Around the authored 0.42 that is −2.18 LU down at the wet floor
///    (0.105) and +2.98 LU up at full wet.
///  * **Zero at the anchor** by construction, so an UNSWEPT capture cannot move (see
///    [`leveled_contribution`], which never even calls this without a write).
///
/// Not a hardware-measured curve — no wet-mix sweep has been captured off the unit. It is a
/// plausible, solver-friendly stand-in whose only job is to give the offline Assign-path
/// solve real authority, so the WET-FLOOR outcome is reachable offline.
#[cfg(feature = "e2e")]
pub(crate) fn wet_mix_gain_db(v: f64, authored: f64) -> f64 {
    let g = |m: f64| 20.0 * (1.0 + m.clamp(0.0, 1.0)).log10();
    g(v) - g(authored)
}

/// Preset-024-class ("TR+BD2+BMP") saturated-amp response for a drive pedal's OWN
/// level/volume knob: silent (no signal) at or below the noise floor, a steep ~42 LU
/// cliff, then a heavily compressed plateau — see notes/leveling.md §"Saturated-amp
/// response shape (preset 024…)". Pure and EXACT (no HW noise, no stimulus-scaling
/// slack) so the FS solver's tightened `FS_TOL_LU` = 0.1 acceptance is meaningful
/// offline. `None` below the floor is genuine silence (the leveller's no-signal path),
/// matching the HW note that the bracket-expansion probe down there reads NO_SIGNAL, not
/// merely "very quiet".
#[cfg(feature = "e2e")]
pub(crate) fn saturated_pedal_lufs(v: f32) -> Option<f64> {
    let v = f64::from(v);
    if v <= 0.10 {
        return None;
    }
    if v <= 0.20 {
        // The cliff: 42 LU over this 0.10 span. Floor anchor +3 (PR2 D1: the
        // coherent +3 world) from the mono-era -62.0.
        return Some(-59.0 + 420.0 * (v - 0.10));
    }
    if v <= 0.30 {
        // Shoulder — the cliff's top IS the plateau's own floor (continuous, no
        // dip). Anchor +3 (PR2 D1) from the mono-era -20.0.
        return Some(-17.0);
    }
    // The compressed plateau: +3 LU across the remaining 0.70 of knob travel
    // (a RELATIVE span, untouched by the D1 absolute shift above).
    let t = ((v - 0.30) / 0.70).min(1.0);
    Some(-17.0 + 3.0 * t)
}

#[cfg(feature = "e2e")]
impl SlotLoudness {
    /// C for the sound under measurement: a scene's overlay C, falling back to `base`.
    fn c_for(&self, scene: Option<u32>) -> f64 {
        scene
            .and_then(|i| self.scenes.get(i as usize).copied())
            .unwrap_or(self.base)
    }
}

#[cfg(feature = "e2e")]
#[derive(serde::Deserialize)]
struct Sidecar {
    slots: std::collections::HashMap<String, SlotLoudness>,
    /// C for a preset the sidecar doesn't list (keeps an unlisted slot from panicking).
    default: f64,
}

/// The amp's stored `outputLevel` per (slot, scene), derived at load from the presetJson.
#[cfg(feature = "e2e")]
struct StoredLevels {
    /// The base amp `outputLevel` — the FIRST guitarNodes node carrying one, scanning
    /// `G1..G7` in order (see [`stored_from_preset`]: the fixtures keep every amp in one
    /// sound at the same value, so "first" is unambiguous).
    base: Option<f32>,
    /// Per-scene overlay `outputLevel` (0-based); `None` = the scene inherits `base`.
    scenes: Vec<Option<f32>>,
}

#[cfg(feature = "e2e")]
impl StoredLevels {
    fn output_level(&self, scene: Option<u32>) -> Option<f32> {
        match scene {
            Some(i) => self.scenes.get(i as usize).copied().flatten().or(self.base),
            None => self.base,
        }
    }
}

/// The sidecar C table, loaded once. `TMP_E2E_LOUDNESS_SIDECAR` overrides the path (tests).
#[cfg(feature = "e2e")]
fn sidecar() -> &'static Sidecar {
    static SIDECAR: std::sync::OnceLock<Sidecar> = std::sync::OnceLock::new();
    SIDECAR.get_or_init(|| {
        let path = std::env::var("TMP_E2E_LOUDNESS_SIDECAR").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/scenario-loudness.json"
            )
            .to_string()
        });
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                log::warn!("e2e_capture: no loudness sidecar at {path} — flat default C");
                Sidecar {
                    slots: std::collections::HashMap::new(),
                    default: -18.0,
                }
            })
    })
}

/// The presetJson-derived stored `outputLevel` map, loaded once from
/// `scenario-presets.json` (`TMP_E2E_SCENARIO_PRESETS` overrides the path for tests).
#[cfg(feature = "e2e")]
fn stored_levels() -> &'static std::collections::HashMap<u32, StoredLevels> {
    static STORED: std::sync::OnceLock<std::collections::HashMap<u32, StoredLevels>> =
        std::sync::OnceLock::new();
    STORED.get_or_init(|| load_stored_levels().unwrap_or_default())
}

#[cfg(feature = "e2e")]
fn load_stored_levels() -> Option<std::collections::HashMap<u32, StoredLevels>> {
    // Reuse the seed module's ONE reader of `scenario-presets.json` (+ its
    // `TMP_E2E_SCENARIO_PRESETS` override) so the physics model and the seeder can't drift
    // on which fixture they read; only the per-preset knob extraction is ours.
    let mut out = std::collections::HashMap::new();
    for p in crate::probe_api::seed_scenario::scenario_spec().ok()? {
        let pj: serde_json::Value = serde_json::from_str(&p.preset_json).ok()?;
        out.insert(p.list_index, stored_from_preset(&pj));
    }
    Some(out)
}

/// Extract the amp's base + per-scene stored `outputLevel` from one preset's decoded JSON.
/// The base graph (`guitarNodes.<group>`) is an ARRAY of node objects; a scene overlay
/// (`scenes[i].guitarNodes.<group>`) is a MAP of `nodeId → { dspUnitParameters }`.
///
/// It scans EVERY guitar group in `G1..G7` order, not just `G1`. A trunk-amp preset is
/// unaffected (its amp is the first `outputLevel` anywhere), but a `gtrParallel*` fixture
/// puts its amps in the split LANES, and a `G1`-only probe silently fell back to `1.0` for
/// those — which desyncs written-over-stored and, worse, hid a scene deliberately saved
/// with the amp output at ZERO (the routing-clamp case: with `stored_ol == 0` the model's
/// `ol_term` collapses to 0, so the knob provably has no authority over the capture).
/// `serde_json::Map` is a BTreeMap here, so the group order is the sorted key order and the
/// pick is deterministic; the fixtures additionally keep every amp in one sound at the SAME
/// `outputLevel` (pinned by `fixture_gates`), so WHICH amp is picked cannot matter.
#[cfg(feature = "e2e")]
fn stored_from_preset(pj: &serde_json::Value) -> StoredLevels {
    let base = pj
        .get("audioGraph")
        .and_then(|a| a.get("guitarNodes"))
        .and_then(|g| g.as_object())
        .and_then(|groups| {
            groups
                .values()
                .filter_map(|arr| arr.as_array())
                .find_map(|arr| arr.iter().find_map(node_output_level))
        });
    let scenes = pj
        .get("scenes")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .map(|sc| {
                    sc.get("guitarNodes")
                        .and_then(|g| g.as_object())
                        .and_then(|groups| {
                            groups
                                .values()
                                .filter_map(|nodes| nodes.as_object())
                                .find_map(|m| m.values().find_map(node_output_level))
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    StoredLevels { base, scenes }
}

/// `dspUnitParameters.outputLevel` of a node object, if present.
#[cfg(feature = "e2e")]
fn node_output_level(node: &serde_json::Value) -> Option<f32> {
    node.get("dspUnitParameters")
        .and_then(|d| d.get("outputLevel"))
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32)
}

/// Keyed on the ACTIVE scene, not the recall that selected it: the `currentPresetDataRequest`
/// re-push mid-prepass must be cut too, or it heals the scene the seam exists to break.
#[cfg(all(test, feature = "e2e"))]
fn truncate_scene_push(st: &SimState, json: Vec<u8>) -> Vec<u8> {
    let Some((scene, marker)) = st.truncated_scene_push.as_ref() else {
        return json;
    };
    if st.current_scene != Some(*scene) {
        return json;
    }
    match json
        .windows(marker.len())
        .position(|w| w == marker.as_bytes())
    {
        Some(at) => json[..at].to_vec(),
        None => json,
    }
}

#[cfg(not(all(test, feature = "e2e")))]
fn truncate_scene_push(_st: &SimState, json: Vec<u8>) -> Vec<u8> {
    json
}

/// The field-3 graph a `loadPreset`/`loadScene` echoes for slot `slot0`. For a SCENARIO slot
/// (e2e) it echoes that slot's REAL presetJson, `presetLevel` PLUS any baked footswitch
/// param patched to the slot's COMMITTED lazy-commit doc (module header — a load sees
/// committed-only, never a still-pending save), so offline scene-leveling's prepass
/// classifies against the same amp node the backup-derived candidates name (written==stored
/// → faithful convergence + a correct amp pick for the parallel/split templates), AND a
/// LATER `ensure_fresh_load` barrier witnessing a `SaveWitness::Param` (a footswitch bake)
/// can actually match against this echo; any other slot (and all non-e2e builds) uses the
/// shared default two-node graph (`with_preset_json` overrides it).
fn load_echo_json(st: &mut SimState, slot0: u32) -> Vec<u8> {
    #[cfg(feature = "e2e")]
    if let Some(j) = scenario_json_for(slot0) {
        let doc = st.committed_doc(slot0).clone();
        let patched = with_patched_doc(j, &doc);
        // `ftsw`: the UNSAVED working copy when this session has edited it, else the slot's
        // saved array — the field-3 push is the LIVE document, which is what makes it the
        // confirm channel for the no-echo footswitch setters.
        let ftsw = st.ftsw_working.as_ref().or(doc.ftsw.as_ref());
        return truncate_scene_push(st, with_ftsw(&patched, ftsw).into_bytes());
    }
    let _ = slot0;
    truncate_scene_push(
        st,
        with_ftsw(&st.preset_json, st.ftsw_working.as_ref()).into_bytes(),
    )
}

/// The field-8 (`presetDataChanged`) read body for `slot0` — the slot's static scenario
/// JSON with `presetLevel` PLUS any baked footswitch param patched to what a READ is
/// entitled to see right now (the pending doc if one exists — read-your-writes — else
/// committed; module header). `None` for a non-scenario slot / a non-e2e build, exactly as
/// [`saved_slot_json`] alone.
fn saved_slot_json_body(
    #[cfg_attr(not(feature = "e2e"), allow(unused_variables))] st: &mut SimState,
    slot0: u32,
) -> Option<String> {
    #[cfg(feature = "e2e")]
    {
        let doc = st.readable_doc(slot0).clone();
        // NO working copy here: field-8 is the SAVED document, so an unsaved `ftsw` edit must
        // be invisible to it (that asymmetry against the field-3 render above is the point —
        // `verify_fs_persisted_writes` reads this to decide whether the assign PERSISTED).
        saved_slot_json(slot0).map(|j| with_ftsw(&with_patched_doc(j, &doc), doc.ftsw.as_ref()))
    }
    #[cfg(not(feature = "e2e"))]
    {
        let _ = st;
        saved_slot_json(slot0).map(str::to_string)
    }
}

/// The committed scenario preset JSON for a 0-based list index, cached (the seed module owns
/// the one reader of `scenario-presets.json`, so the echoed graph can't drift from the seed /
/// backup fixture). `None` for a non-scenario slot → the caller uses the default graph.
#[cfg(feature = "e2e")]
fn scenario_json_for(slot0: u32) -> Option<&'static str> {
    static MAP: std::sync::OnceLock<HashMap<u32, String>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        crate::probe_api::seed_scenario::scenario_spec()
            .map(|v| {
                v.into_iter()
                    .map(|p| (p.list_index, p.preset_json))
                    .collect()
            })
            .unwrap_or_default()
    })
    .get(&slot0)
    .map(String::as_str)
}

/// Build a `songListResponse`(11→3) / `setlistListResponse`(12→3): records (field 2) each
/// carrying `name` (field 1). Small lists fit one inbound frame.
fn list_response(tms: u32, names: &[String]) -> Vec<u8> {
    const F_RECORD: u32 = 2; // repeated record field inside the list response
    let mut records = Vec::new();
    for name in names {
        let rec = proto::len_delimited(1, name.as_bytes());
        records.extend_from_slice(&proto::len_delimited(F_RECORD, &rec));
    }
    proto::len_delimited(tms, &proto::len_delimited(F_LIST_RESPONSE, &records))
}

/// Produce the framed confirm/reject reply for a structural edit, honoring the
/// drop-first / reject-at injections.
fn structural_reply(st: &mut SimState, confirm_field: u32) -> Vec<Vec<u8>> {
    st.structural_seen += 1;
    let n = st.structural_seen;
    if st.drop_first && n == 1 {
        return Vec::new(); // silent drop — no confirm, no error
    }
    if st.reject_at == Some(n) {
        return vec![frame(&preset_message(F_PRESET_ERROR, &[]))];
    }
    vec![frame(&preset_message(confirm_field, &[]))]
}

impl HidTransport for SimDevice {
    fn send(&self, _body: &[u8]) -> Result<(), String> {
        self.state
            .lock()
            .expect("sim lock")
            .events
            .push(SimEvent::Heartbeat);
        Ok(()) // fire-and-forget (heartbeat) — no reply
    }
    fn transact(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.handle(body))
    }
    fn transact_chunked(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.handle(body))
    }
    fn pump(&self, _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        // Replies are delivered synchronously from the send; only a queued device push
        // (`with_stale_push_after_save`) arrives here.
        Ok(std::mem::take(
            &mut self.state.lock().expect("sim lock").pending_pushes,
        ))
    }
    fn transact_eager(&self, body: &[u8], _max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.handle(body))
    }
}

// ─── reply encoding ──────────────────────────────────────────────────────────

/// Frame a message body into a single inbound report `[0x00, 0x35, 0x00, len, body…]`
/// (`MAGIC_OUT` single/final frame). Most fake replies are tiny (< 60 B), so one frame
/// suffices; `Session::push_bodies` keeps a standalone `0x35` as its own message. A body
/// that overflows one frame must use [`frame_multi`].
fn frame(body: &[u8]) -> Vec<u8> {
    debug_assert!(body.len() <= 60, "fake reply exceeds one HID frame");
    let mut report = vec![0x00, 0x35, 0x00, body.len() as u8];
    report.extend_from_slice(body);
    report
}

/// Frame a body across one OR more inbound reports using the device's `0x33` start /
/// `0x34` continue / `0x35` final chunking (≤60 B each), so `streams_final` reassembles
/// it byte-identically. A short body collapses to a single `0x35` frame (= [`frame`]).
/// Needed for the showcase song/setlist lists, which exceed one frame.
fn frame_multi(body: &[u8]) -> Vec<Vec<u8>> {
    const MAX: usize = 60;
    if body.len() <= MAX {
        return vec![frame(body)];
    }
    let chunks: Vec<&[u8]> = body.chunks(MAX).collect();
    let last = chunks.len() - 1;
    chunks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let magic = if i == 0 {
                0x33
            } else if i == last {
                0x35
            } else {
                0x34
            };
            let mut report = vec![0x00, magic, 0x00, c.len() as u8];
            report.extend_from_slice(c);
            report
        })
        .collect()
}

/// `FenderMessageTMS{ preset(2): PresetMessage{ field: payload } }`. Built with the
/// crate's golden-tested protobuf encoders so the fake's wire bytes can't drift from
/// the real codec.
fn preset_message(field: u32, payload: &[u8]) -> Vec<u8> {
    proto::len_delimited(TMS_PRESET, &proto::len_delimited(field, payload))
}

/// `PresetLoaded{ tabEnum(1)=1, presetSlot(6)=dev_slot }` (1-based device slot).
fn preset_loaded(dev_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    proto::field_varint(&mut inner, 1, 1);
    proto::field_varint(&mut inner, 6, dev_slot);
    preset_message(F_PRESET_LOADED, &inner)
}

/// `currentPresetDataChanged`(3) — `presetJson`(1) = LZ4("stored"/uncompressed-block)
/// of the preset JSON. Mirrors the wire shape `session.rs`'s own fixtures build
/// (`decode_current_preset_data_yields_active_graph_with_known_template`) and what
/// `Session::current_preset_value` (`best_json_payload`'s `(3, 1)` carrier) reads back.
const F_CURRENT_PRESET_DATA_CHANGED: u32 = 3;

fn current_preset_data_changed(json: &[u8]) -> Vec<u8> {
    let lz4 = proto::lz4_block_compress_stored(json);
    let inner = proto::len_delimited(1, &lz4);
    preset_message(F_CURRENT_PRESET_DATA_CHANGED, &inner)
}

/// `presetDataChanged`(9) — `{ listEnum(1)=1, presetSlot(2)=dev_slot, presetJson(3) }`.
/// `presetJson` is PLAINTEXT here (unlike field 3's lz4 block): the read path hands the
/// field-9 bytes straight to `from_utf8_lossy` (`session::read_slot_preset_json`).
fn preset_data_changed(dev_slot: u64, json: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    proto::field_varint(&mut inner, 1, 1);
    proto::field_varint(&mut inner, 2, dev_slot);
    inner.extend_from_slice(&proto::len_delimited(3, json));
    preset_message(F_PRESET_DATA_CHANGED, &inner)
}

/// The SAVED document for a 0-based list index — the committed scenario presetJson, which
/// is exactly what the unit stores for these slots (the seed imports it verbatim). `None`
/// for a non-scenario slot / a non-e2e build.
fn saved_slot_json(slot0: u32) -> Option<&'static str> {
    #[cfg(feature = "e2e")]
    {
        scenario_json_for(slot0)
    }
    #[cfg(not(feature = "e2e"))]
    {
        let _ = slot0;
        None
    }
}

/// The scenario fixture's own `lastLoadedScene` for a slot (0-based `scenes[]` index;
/// `None` = base, including the `BASE_SCENE_SLOT` sentinel). Only preset 404 carries the
/// key, so every other slot keeps its base default.
fn fixture_last_loaded_scene(slot0: u32) -> Option<u32> {
    let scene = saved_slot_json(slot0)
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .and_then(|v| v.get("lastLoadedScene").and_then(serde_json::Value::as_u64))?;
    (scene != u64::from(crate::session::BASE_SCENE_SLOT)).then_some(scene as u32)
}

/// `PresetLevelChanged{ presetLevel(1)=level }` (fixed32 float echo).
fn preset_level_changed(level: f32) -> Vec<u8> {
    let mut inner = Vec::new();
    proto::field_f32(&mut inner, 1, level);
    preset_message(F_PRESET_LEVEL_CHANGED, &inner)
}

/// The string value of len-delimited `field` in a parsed message (empty if absent).
fn str_field(fields: &[(u32, proto::Val)], field: u32) -> String {
    proto::first_bytes(fields, field)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

/// Extract the `(group=1, nodeId=2, fenderId=3)` string triple from an op's inner bytes.
fn three_strings(inner_bytes: &[u8]) -> (String, String, String) {
    let inner = proto::parse(inner_bytes);
    (
        str_field(&inner, 1),
        str_field(&inner, 2),
        str_field(&inner, 3),
    )
}

// ─── sim-physics unit tests (verify every capture-model law can fail) ─────────────────
#[cfg(all(test, feature = "e2e"))]
mod physics_tests {
    use super::*;

    fn one_slot(slot: u32, base: f64, scenes: Vec<f64>, routed: Option<&str>) -> Sidecar {
        let mut slots = std::collections::HashMap::new();
        slots.insert(
            slot.to_string(),
            SlotLoudness {
                base,
                scenes,
                routed_node: routed.map(str::to_string),
                offbranch_switch_node: None,
                leveled_params: Vec::new(),
            },
        );
        Sidecar {
            slots,
            default: -18.0,
        }
    }

    fn tone(rate: u32) -> Vec<f32> {
        (0..rate as usize)
            .map(|i| 0.2 * (std::f32::consts::TAU * 220.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    // The core C-model term: presetLevel is a linear multiplier → halving it drops the
    // modeled loudness by ~6.02 LU (20*log10(0.5)).
    #[test]
    fn preset_level_halving_drops_about_6_lu() {
        let sc = one_slot(401, -16.0, vec![], None);
        let stored = std::collections::HashMap::new();
        let loud = model_lufs(
            &mut SimState {
                current_slot: 401,
                preset_level: 0.5,
                ..Default::default()
            },
            &sc,
            &stored,
        )
        .unwrap();
        let quiet = model_lufs(
            &mut SimState {
                current_slot: 401,
                preset_level: 0.25,
                ..Default::default()
            },
            &sc,
            &stored,
        )
        .unwrap();
        assert!(
            (loud - quiet - 6.0206).abs() < 0.2,
            "halving presetLevel should drop ~6 LU, got {}",
            loud - quiet
        );
    }

    // The LOCKED convention: the outputLevel term is RELATIVE to the scene's stored knob.
    // Rewriting the stored value is a 0 LU shift; doubling it is +6 LU.
    #[test]
    fn output_level_is_relative_to_stored_knob() {
        let sc = one_slot(400, -18.0, vec![-19.0], None);
        let mut stored = std::collections::HashMap::new();
        stored.insert(
            400u32,
            StoredLevels {
                base: Some(0.5),
                scenes: vec![Some(0.4)], // scene 0 stored outputLevel
            },
        );
        let with_write = |written: Option<f32>| {
            let mut st = SimState {
                current_slot: 400,
                current_scene: Some(0),
                preset_level: 1.0,
                ..Default::default()
            };
            if let Some(w) = written {
                st.param_writes
                    .insert((0, "G1".into(), "amp".into(), "outputLevel".into()), w);
            }
            model_lufs(&mut st, &sc, &stored).unwrap()
        };
        let as_stored = with_write(None);
        assert!(
            (as_stored - with_write(Some(0.4))).abs() < 0.05,
            "rewriting the stored knob (0.4) must be a 0 LU shift"
        );
        assert!(
            (with_write(Some(0.8)) - as_stored - 6.0206).abs() < 0.2,
            "writing 2x the stored knob must be +6 LU"
        );
    }

    // An engaged switch bypassing the routed amp mutes the sound → silence (None).
    #[test]
    fn offbranch_routed_node_bypassed_is_silence() {
        let sc = one_slot(400, -18.0, vec![], Some("ACD_Amp"));
        let stored = std::collections::HashMap::new();
        let mut st = SimState {
            current_slot: 400,
            ..Default::default()
        };
        st.bypass_writes.insert("ACD_Amp".into(), true);
        assert!(
            model_lufs(&mut st, &sc, &stored).is_none(),
            "bypassing the routed amp must silence the capture"
        );
        st.bypass_writes.insert("ACD_Amp".into(), false);
        assert!(
            model_lufs(&mut st, &sc, &stored).is_some(),
            "un-bypassed, the sound is measurable"
        );
    }

    // An off-branch FOOTSWITCH block is silent ONLY when engaged (bypass=false, its own
    // isolated measurement); forced OFF (bypass=true, as a Base/sibling isolation) it does
    // NOT silence the sound — so Base stays measurable while that one switch is off-branch.
    #[test]
    fn offbranch_switch_node_silences_only_when_engaged() {
        let mut sc = one_slot(403, -18.0, vec![], None);
        sc.slots.get_mut("403").expect("slot").offbranch_switch_node =
            Some("ACD_TubeScreamer".into());
        let stored = std::collections::HashMap::new();
        let with_bypass = |byp: Option<bool>| {
            let mut st = SimState {
                current_slot: 403,
                ..Default::default()
            };
            if let Some(b) = byp {
                st.bypass_writes.insert("ACD_TubeScreamer".into(), b);
            }
            model_lufs(&mut st, &sc, &stored)
        };
        assert!(
            with_bypass(Some(false)).is_none(),
            "engaged (bypass=false) → the off-branch switch measures silence"
        );
        assert!(
            with_bypass(Some(true)).is_some(),
            "forced OFF (bypass=true) → NOT silenced (Base/sibling isolation stays measurable)"
        );
        assert!(
            with_bypass(None).is_some(),
            "no write for that node → measurable"
        );
    }

    // The scaling actually lands the measured LUFS on the modeled value.
    #[test]
    fn scale_stimulus_lands_on_model() {
        // T1/F1: run the scaled capture through the PRODUCTION measure path
        // (`leveller::processed_lufs`, the 2-ch BS.1770 hub every leveling
        // measurement uses) rather than a hand-rolled per-channel re-check — a bug
        // in `Capture::processed_stereo`'s stereo pairing wouldn't show up in a
        // `measure_mono(&cap.channel(0), ..)` re-check, only in the real path.
        let rate = 48_000u32;
        let cap = scale_stimulus(&tone(rate), rate, -20.0);
        assert_eq!(cap.channels, 2, "the sim emits dual-mono (D1)");
        let measured = crate::leveller::processed_lufs(Ok(cap)).unwrap();
        assert!(
            (measured + 20.0).abs() < 0.05,
            "scaled to hit -20 LUFS through the production measure, got {measured}"
        );
    }

    // T1/F1, the brief's exact contract test: a capture built for l_model = -24.0
    // must measure -24.0 through the production path, tight tolerance — this is
    // what would have shipped +3.0 (or +6.0, stacked with the C-table shift) off
    // had `scale_stimulus` not compensated for the dual-mono duplication it now
    // performs.
    #[test]
    fn scale_stimulus_contract_survives_dual_mono_emission() {
        let rate = 48_000u32;
        let cap = scale_stimulus(&tone(rate), rate, -24.0);
        let measured = crate::leveller::processed_lufs(Ok(cap)).unwrap();
        assert!(
            (measured - (-24.0)).abs() < 0.05,
            "measured == l_model contract broke: got {measured}"
        );
    }

    // The capture-fault field silences ONE capture for the armed slot, then recovers.
    #[test]
    fn capture_fault_silences_once_then_recovers() {
        let dev = SimDevice::new();
        {
            let mut st = dev.state.lock().expect("sim lock");
            st.current_slot = 401; // real sidecar: base -16
            st.reamp_on = true;
            st.preset_level = 1.0;
            st.fail_capture_slot = Some(401);
        }
        let rate = 48_000u32;
        let stim = tone(rate);
        let first = dev.e2e_capture(&stim, rate);
        assert!(
            !crate::lufs::measure_mono(&first.channel(0), rate)
                .unwrap()
                .integrated_lufs
                .is_finite(),
            "the armed fault silences the next capture"
        );
        let second = dev.e2e_capture(&stim, rate);
        assert!(
            crate::lufs::measure_mono(&second.channel(0), rate)
                .unwrap()
                .integrated_lufs
                .is_finite(),
            "the capture after the one-shot fault is healthy"
        );
    }

    // ── saturated-pedal response shape (notes/leveling.md §preset-024) ──────────────

    #[test]
    fn saturated_pedal_response_is_silent_at_and_below_the_noise_floor() {
        assert!(saturated_pedal_lufs(0.0).is_none());
        assert!(saturated_pedal_lufs(0.05).is_none());
        // (not asserting the exact 0.10 boundary itself — f32→f64 widening puts it a
        // hair either side of the literal, which the fixture never writes anyway)
        assert!(saturated_pedal_lufs(0.1001).is_some());
    }

    #[test]
    fn saturated_pedal_response_cliff_is_about_42_lu_over_0_10_to_0_20() {
        let lo = saturated_pedal_lufs(0.10 + 1e-4).expect("just past the floor");
        let hi = saturated_pedal_lufs(0.20).expect("cliff top");
        assert!(
            (hi - lo - 42.0).abs() < 0.1,
            "cliff span must be ~42 LU, got {}",
            hi - lo
        );
        // Our two FS fixture targets (-23/-21, PR2 D1: +3 from the mono-era -26/-24)
        // must land ON this cliff, not the flat plateau — the whole reason the
        // fixture is solvable at a tight ±0.1 LU offline.
        assert!(
            (lo..=hi).contains(&-23.0) && (lo..=hi).contains(&-21.0),
            "targets -23/-21 must fall inside the cliff [{lo}, {hi}]"
        );
    }

    #[test]
    fn saturated_pedal_response_plateau_is_far_shallower_than_the_cliff() {
        let cliff = saturated_pedal_lufs(0.20).unwrap() - saturated_pedal_lufs(0.1001).unwrap();
        let plateau = saturated_pedal_lufs(1.0).unwrap() - saturated_pedal_lufs(0.30).unwrap();
        assert!((plateau - 3.0).abs() < 0.05, "plateau span: {plateau}");
        assert!(
            plateau < cliff / 10.0,
            "the plateau ({plateau} LU) must be far shallower than the cliff ({cliff} LU)"
        );
    }

    #[test]
    fn model_lufs_leveled_param_activates_only_while_its_own_block_is_engaged() {
        let mut slots = std::collections::HashMap::new();
        slots.insert(
            "999".to_string(),
            SlotLoudness {
                base: -10.0,
                scenes: vec![],
                routed_node: None,
                offbranch_switch_node: None,
                leveled_params: vec![LeveledParam {
                    group: "G1".into(),
                    node: "pedal".into(),
                    param: "level".into(),
                    curve: LeveledCurve::SaturatedPedal,
                }],
            },
        );
        let sc = Sidecar {
            slots,
            default: -18.0,
        };
        let stored = std::collections::HashMap::new();
        let mut st = SimState {
            current_slot: 999,
            preset_level: 1.0,
            ..Default::default()
        };
        // Not engaged: no bypass write AND the default graph has no such node, so the
        // recall renders nothing — the ordinary flat C, unperturbed by the pedal curve.
        // (A node the recall renders ENGAGED does activate the branch with no bypass write;
        // that arm is pinned by `model_lufs_leveled_param_activates_on_a_base_engaged_block`.)
        assert!(
            (model_lufs(&mut st, &sc, &stored).unwrap() - (-10.0)).abs() < 1e-6,
            "unengaged must fall through to the flat C"
        );
        // Engaged (bypass=false), nothing written AND no authored value in the graph → no
        // signal, never a silent fallback to the flat C (which would mask a real bug).
        st.bypass_writes.insert("pedal".into(), false);
        assert!(
            model_lufs(&mut st, &sc, &stored).is_none(),
            "engaged with neither a write nor an authored value must not read as the flat C"
        );
        // Engaged with the block's AUTHORED value and still no write — a VERIFY row's
        // engaged capture, which writes no param by definition. It must render the pedal
        // curve at the authored 0.5, NOT off-branch silence (which is what made the whole
        // verify-vs-level distinction unobservable offline).
        st.preset_json = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "pedal", "dspUnitParameters": { "level": 0.5 } }
            ] } }
        })
        .to_string();
        st.parsed_preset_cache = None;
        let authored = model_lufs(&mut st, &sc, &stored).expect("authored value renders");
        assert!(
            (authored - saturated_pedal_lufs(0.5).unwrap()).abs() < 1e-6,
            "an unwritten engaged block must render its authored 0.5, got {authored}"
        );
        // Engaged with a mid-cliff value → the curve's own arithmetic.
        st.param_writes.insert(
            (SCENE_BASE, "G1".into(), "pedal".into(), "level".into()),
            0.1857,
        );
        let lufs = model_lufs(&mut st, &sc, &stored).unwrap();
        // PR2 re-baseline: +3 from the mono-era -26 (the cliff's floor anchor moved
        // -62 → -59, same -420*(v-0.10) slope).
        assert!(
            (lufs - (-23.0)).abs() < 0.2,
            "v=0.1857 should land near -23 LUFS, got {lufs}"
        );
        // Forced OFF again (a sibling's own isolated measurement) — back to the flat C.
        st.bypass_writes.insert("pedal".into(), true);
        assert!((model_lufs(&mut st, &sc, &stored).unwrap() - (-10.0)).abs() < 1e-6);
    }

    /// The ASSIGN arm of the activation predicate plus the `wetMix` curve's two guarantees,
    /// on one sidecar: a block ENGAGED IN BASE with NO bypass write (an Assign row's
    /// isolation forces only siblings off) activates the branch, an UNSWEPT capture still
    /// reads the flat C exactly, and a SWEPT one moves by the curve's delta from the
    /// AUTHORED value. The middle assertion is the whole blast-radius argument for widening
    /// the predicate — every base / scene / Doctor / verify capture on such a block is
    /// unswept by definition.
    #[test]
    fn model_lufs_leveled_param_activates_on_a_base_engaged_block() {
        let mut slots = std::collections::HashMap::new();
        slots.insert(
            "999".to_string(),
            SlotLoudness {
                base: -15.0,
                scenes: vec![],
                routed_node: None,
                offbranch_switch_node: None,
                leveled_params: vec![LeveledParam {
                    group: "G1".into(),
                    node: "verb".into(),
                    param: "mix".into(),
                    curve: LeveledCurve::WetMix,
                }],
            },
        );
        let sc = Sidecar {
            slots,
            default: -18.0,
        };
        let stored = std::collections::HashMap::new();
        let mut st = SimState {
            current_slot: 999,
            preset_level: 1.0,
            preset_json: serde_json::json!({
                "audioGraph": { "guitarNodes": { "G1": [
                    { "nodeId": "verb", "dspUnitParameters": { "bypass": false, "mix": 0.42 } }
                ] } }
            })
            .to_string(),
            ..Default::default()
        };
        st.parsed_preset_cache = None;
        // Engaged in base, no bypass write, NOTHING swept → the flat C, to the bit.
        assert!(
            (model_lufs(&mut st, &sc, &stored).unwrap() - (-15.0)).abs() < 1e-9,
            "an unswept wet-mix declaration must not move the capture"
        );
        // Swept DOWN to the wet floor (0.25 x the authored 0.42) → quieter by the curve.
        st.param_writes.insert(
            (SCENE_BASE, "G1".into(), "verb".into(), "mix".into()),
            0.105,
        );
        let floored = model_lufs(&mut st, &sc, &stored).unwrap();
        // `param_writes` is f32, so widen through the SAME rounding the model sees.
        let expect = -15.0 + wet_mix_gain_db(f64::from(0.105_f32), 0.42);
        assert!(
            (floored - expect).abs() < 1e-9,
            "the wet floor must read the curve's own delta ({expect}), got {floored}"
        );
        assert!(floored < -15.0, "less wet must be QUIETER: {floored}");
        // Swept UP → louder, and monotone across the whole range.
        st.param_writes
            .insert((SCENE_BASE, "G1".into(), "verb".into(), "mix".into()), 1.0);
        let full = model_lufs(&mut st, &sc, &stored).unwrap();
        assert!(full > -15.0, "full wet must be LOUDER: {full}");
        // Forced OFF by a sibling's isolation → the branch is dead again even with a write.
        st.bypass_writes.insert("verb".into(), true);
        assert!((model_lufs(&mut st, &sc, &stored).unwrap() - (-15.0)).abs() < 1e-9);
    }

    /// The wet-mix curve's contract: zero at its anchor, monotonically increasing, and a
    /// modest +6.02 dB from fully dry to full wet (see [`wet_mix_gain_db`]'s doc for why the
    /// shape has to be monotone — the bounded secant brackets on it).
    #[test]
    fn wet_mix_curve_is_monotone_zero_at_the_anchor_and_modest() {
        assert!(wet_mix_gain_db(0.42, 0.42).abs() < 1e-12);
        let mut prev = f64::NEG_INFINITY;
        for i in 0..=100 {
            let v = f64::from(i) / 100.0;
            let g = wet_mix_gain_db(v, 0.42);
            assert!(g > prev, "not monotone at {v}: {g} <= {prev}");
            prev = g;
        }
        let span = wet_mix_gain_db(1.0, 0.0);
        assert!(
            (span - 6.0206).abs() < 0.01,
            "dry -> full wet must be +6.02 dB, got {span}"
        );
        // The floor sits ~2.2 LU under the authored mix — enough authority for a solve,
        // far too little to reach an absurd target (which is what the wet floor reports).
        let to_floor = wet_mix_gain_db(0.105, 0.42);
        assert!(
            (to_floor - (-2.177)).abs() < 0.01,
            "authored -> wet floor must be about -2.18 LU, got {to_floor}"
        );
    }

    // ── lazy-commit `presetLevel` (the same-slot stale-load incident, at the sim layer) ──

    /// The field-8 read body's `audioGraph.presetLevel`, via a real `Session` (proves the
    /// wire round-trip, not just the internal store).
    fn read_saved_preset_level(s: &mut crate::session::Session, dev_slot: u32) -> f64 {
        let bytes = s
            .read_slot_preset_json(dev_slot)
            .expect("field-8 read")
            .expect("a scenario slot answers field-8");
        let json: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&bytes)).expect("valid json");
        json["audioGraph"]["presetLevel"]
            .as_f64()
            .expect("presetLevel present")
    }

    #[test]
    fn save_then_immediate_field8_read_shows_the_pending_value() {
        // A long latency proves the read does NOT depend on elapsed time at all.
        let sim = SimDevice::new().with_commit_latency(60_000);
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.set_preset_level(0.81).unwrap();
        s.save_current_preset(401).unwrap();
        let level = read_saved_preset_level(&mut s, 402); // dev slot = list index + 1
        assert!(
            (level - 0.81).abs() < 1e-3,
            "field-8 must read-your-writes immediately, got {level}"
        );
    }

    #[test]
    fn load_before_the_deadline_materializes_the_committed_old_value() {
        let sim = SimDevice::new().with_commit_latency(60_000);
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.set_preset_level(0.81).unwrap();
        s.save_current_preset(401).unwrap();
        // A same-slot reload WELL inside the (60 s) commit window must NOT see 0.81 — the
        // committed doc is still the fixture's own baked-in 0.32 (401's static presetLevel).
        s.load_preset(401).unwrap();
        assert!(
            (sim.preset_level() - 0.32).abs() < 1e-3,
            "a load inside the commit window must materialize the PRE-save value, got {}",
            sim.preset_level()
        );
    }

    #[test]
    fn load_after_the_deadline_materializes_the_new_value() {
        let sim = SimDevice::new().with_commit_latency(50);
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.set_preset_level(0.81).unwrap();
        s.save_current_preset(401).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(90)); // past the 50 ms deadline
        s.load_preset(401).unwrap();
        assert!(
            (sim.preset_level() - 0.81).abs() < 1e-3,
            "a load after the commit window must materialize the NEW value, got {}",
            sim.preset_level()
        );
    }

    // ── merged-doc extension: a footswitch bake's own knob round-trips a save→load ──

    /// A footswitch bake writes a `changeParameter` float onto the block's own knob at
    /// SCENE_BASE, saves, then a LATER fresh load must still see it — the reseed this
    /// fix adds to the `F_LOAD_PRESET` handler (`committed_params`). Without it, a
    /// re-measure seam (`e2e_measure_sound`) or a second leveling batch on the same
    /// slot would read NO write for the leveled param and silently mis-model it as
    /// off-branch silence.
    #[test]
    fn a_baked_param_survives_a_save_then_fresh_load() {
        let sim = SimDevice::new(); // default 0 ms latency
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.change_parameter("G1", "pedal", "level", 0.1857).unwrap();
        s.save_current_preset(401).unwrap();
        s.load_preset(401).unwrap(); // fresh load, no new write this session
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "pedal", "level"),
            Some(0.1857),
            "a baked param must survive a save→fresh-load round trip"
        );
    }

    /// The merge is ADDITIVE across separate save sessions (the real leveling flow's
    /// shape: a base save, then a LATER footswitch batch save) — a later save must not
    /// erase an earlier save's already-baked param just because this session's own
    /// `param_writes` doesn't happen to touch it.
    #[test]
    fn a_later_unrelated_save_does_not_erase_an_earlier_baked_param() {
        let sim = SimDevice::new();
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.change_parameter("G1", "pedal", "level", 0.1857).unwrap();
        s.save_current_preset(401).unwrap();
        // A second, unrelated session: fresh load (reseeds the baked param), a plain
        // presetLevel change, save — never touches "pedal"/"level" itself.
        s.load_preset(401).unwrap();
        s.set_preset_level(0.9).unwrap();
        s.save_current_preset(401).unwrap();
        s.load_preset(401).unwrap();
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "pedal", "level"),
            Some(0.1857),
            "an unrelated later save must not drop an earlier baked param"
        );
    }

    /// A same-slot load INSIDE the commit window must see the OLD baked overlay, not a
    /// still-pending one — the param-overlay analogue of
    /// `load_before_the_deadline_materializes_the_committed_old_value`.
    #[test]
    fn load_before_the_deadline_materializes_the_old_baked_param() {
        let sim = SimDevice::new().with_commit_latency(60_000);
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.change_parameter("G1", "pedal", "level", 0.1857).unwrap();
        s.save_current_preset(401).unwrap();
        s.load_preset(401).unwrap(); // well inside the 60 s window
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "pedal", "level"),
            None,
            "a load inside the commit window must not see the still-pending baked param"
        );
    }

    /// RED-PIN: pins the incident mechanism itself (notes/leveling.md's corruption
    /// class) at the sim layer. With a non-zero commit latency, a load right after a
    /// save — no barrier, no wait, exactly the un-fixed shape — must NOT see the
    /// just-saved value. HW: base saved 0.4377, the footswitch batch's load 2 s later
    /// still read the pre-save ≈0.798.
    #[test]
    fn red_pin_load_right_after_save_consumes_the_stale_preset_level() {
        let sim = SimDevice::new().with_commit_latency(2_000);
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.set_preset_level(0.9).unwrap();
        s.save_current_preset(401).unwrap();
        s.load_preset(401).unwrap(); // NO wait — the un-fixed shape
        assert_ne!(
            sim.preset_level(),
            0.9,
            "a same-slot load with no fresh-load barrier must consume the STALE \
             (pre-save) value, not the just-saved one — the whole incident this fix exists for"
        );
        assert!(
            (sim.preset_level() - 0.32).abs() < 1e-3,
            "the stale value must be the fixture's own pre-save presetLevel, got {}",
            sim.preset_level()
        );
    }

    /// A `loadScene` recall — base sentinel included — runs the device's own
    /// level-apply exactly like `load_preset` does, silently reverting an unsaved
    /// working-copy `presetLevel` to the currently-COMMITTED value (danger.md's
    /// `loadScene` recall entry; HW: `probe --levelpreset 400 -24 save` solved 0.3096
    /// and the saved doc still read the prior 0.32; `leveller::recall_reassert_save`'s
    /// doc comment has the full evidence). FAILS before the fix: the old `F_LOAD_SCENE`
    /// handler never touched `preset_level` at all, so a live-set value would have
    /// survived the recall unperturbed.
    #[test]
    fn scene_recall_reverts_an_unsaved_preset_level_to_the_committed_value() {
        let sim = SimDevice::new(); // default 0 ms commit latency
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(401).unwrap();
        s.set_preset_level(0.81).unwrap();
        s.save_current_preset(401).unwrap(); // commits 0.81, marks 401 `ever_saved`

        // A real scene recall.
        s.set_preset_level(0.55).unwrap(); // unsaved working-copy write, e.g. a solved level
        s.load_scene(0).unwrap();
        assert!(
            (sim.preset_level() - 0.81).abs() < 1e-3,
            "a scene recall must revert an unsaved presetLevel to the committed value, got {}",
            sim.preset_level()
        );

        // The base sentinel recall (`BASE_SCENE_SLOT`) is the SAME mechanism, not a
        // special case — danger.md is explicit that base is included.
        s.set_preset_level(0.42).unwrap(); // another unsaved working-copy write
        s.load_scene(crate::session::BASE_SCENE_SLOT).unwrap();
        assert!(
            (sim.preset_level() - 0.81).abs() < 1e-3,
            "the base recall must revert an unsaved presetLevel exactly like a real \
             scene recall, got {}",
            sim.preset_level()
        );
    }
}

// ─── scene-context model tests (the bug this PR fixes was invisible without these) ──
#[cfg(test)]
mod scene_context_tests {
    use super::*;
    use crate::session::{Session, BASE_SCENE_SLOT};

    // (A) A bare write after loading a preset lands in that preset's SAVED scene, not base.
    #[test]
    fn a_load_activates_the_saved_scene_not_base() {
        let sim = SimDevice::new().with_saved_scene(2, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(2).unwrap();
        s.change_parameter("G1", "n1", "level", 0.42).unwrap();
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, SimEvent::ChangeParameter { scene: 3, .. })),
            "a bare write after loading a preset saved on scene 3 must land in scene 3: {ev:?}"
        );
    }

    // A preset with no saved scene (never re-saved after switching) loads into base.
    #[test]
    fn loading_a_preset_with_no_saved_scene_activates_base() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(5).unwrap();
        s.change_parameter("G1", "n1", "level", 0.42).unwrap();
        let ev = sim.events();
        assert!(
            ev.iter().any(|e| matches!(
                e,
                SimEvent::ChangeParameter {
                    scene: SCENE_BASE,
                    ..
                }
            )),
            "no saved scene → base: {ev:?}"
        );
    }

    // A save records the currently active scene as the slot's `lastLoadedScene` for the
    // NEXT load — including a save while base (BASE_SCENE_SLOT) is active.
    #[test]
    fn saving_records_the_active_scene_for_the_next_load() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(1).unwrap();
        s.load_scene(4).unwrap();
        s.save_current_preset(1).unwrap();
        s.load_preset(9).unwrap(); // a different slot in between
        s.load_preset(1).unwrap(); // reload slot 1 → restores scene 4
        s.change_parameter("G1", "n1", "level", 0.1).unwrap();
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, SimEvent::ChangeParameter { scene: 4, .. })),
            "reloading a slot saved on scene 4 must reactivate scene 4: {ev:?}"
        );
    }

    // `loadScene(BASE_SCENE_SLOT)` is the wire sentinel for base, not a real scenes[]
    // entry — it must resolve to the SCENE_BASE key, not scene index 8.
    #[test]
    fn load_scene_base_sentinel_resolves_to_scene_base() {
        let sim = SimDevice::new().with_saved_scene(0, Some(3));
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(0).unwrap();
        s.load_scene(BASE_SCENE_SLOT).unwrap();
        s.change_parameter("G1", "n1", "level", 0.9).unwrap();
        let ev = sim.events();
        assert!(
            ev.iter().any(|e| matches!(
                e,
                SimEvent::ChangeParameter {
                    scene: SCENE_BASE,
                    ..
                }
            )),
            "an explicit base recall (wire slot {BASE_SCENE_SLOT}) must write scene_base: {ev:?}"
        );
    }

    // (B) Enabling Scene Edit on a node reseeds that node's scene overlay from base: an
    // existing scene override for another param on the SAME node is dropped, and a base
    // override for that param is copied into the scene's overlay.
    #[test]
    fn enabling_scene_edit_reseeds_the_node_from_base() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(0).unwrap();
        // A prior scene-3 overlay write on "drive" (as if scene-edited earlier).
        s.load_scene(3).unwrap();
        s.set_node_scene_edit("G1", "amp", true).unwrap();
        s.change_parameter("G1", "amp", "drive", 0.2).unwrap();
        // A base override for "drive" (an explicit base recall + write).
        s.load_scene(BASE_SCENE_SLOT).unwrap();
        s.change_parameter("G1", "amp", "drive", 0.7).unwrap();
        // Re-enter the scene and re-enable Scene Edit on the SAME node (e.g. leveling a
        // different param on it) — must reseed "drive" from the base override (0.7), not
        // leave the earlier scene-local write (0.2).
        s.load_scene(3).unwrap();
        s.set_node_scene_edit("G1", "amp", true).unwrap();
        // The reseed isn't itself a ChangeParameter event; assert on the resulting state.
        let st = sim.state.lock().expect("sim lock");
        let reseeded = st
            .param_writes
            .get(&(3, "G1".to_string(), "amp".to_string(), "drive".to_string()))
            .copied();
        assert_eq!(
            reseeded,
            Some(0.7),
            "enabling Scene Edit again must reseed drive from base (0.7), not keep 0.2"
        );
    }

    // ── HW fw 1.8.45 findings (this week) ──────────────────────────────────────────

    const PER_PARAM_OVERLAY_FIXTURE: &str = r#"{"audioGraph":{"guitarNodes":{"G1":[
        {"FenderId":"X","nodeId":"amp","dspUnitParameters":{"bypass":false,"gain":2.5}}
    ]}},
    "scenes":[
        {"guitarNodes":{"G1":{"amp":{"dspUnitParameters":{"bypass":false,"gain":5.0}}}}},
        {"guitarNodes":{"G1":{"amp":{"dspUnitParameters":{"bypass":false}}}}}
    ]}"#;

    // (1) A FULL overlay (scene 0) masks base for every param it lists; a bypass-only
    // overlay (scene 1) masks only `bypass` — every other param, incl. `gain`, renders
    // BASE, with no retention of whatever the previously recalled scene rendered.
    #[test]
    fn scene_recall_renders_the_overlay_per_param_with_no_retention_from_the_prior_scene() {
        let sim = SimDevice::new().with_preset_json(PER_PARAM_OVERLAY_FIXTURE);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(6).unwrap();
        s.load_scene(0).unwrap(); // scene 0: a FULL overlay (gain: 5.0)
        assert_eq!(
            sim.rendered_param("G1", "amp", "gain"),
            Some(5.0),
            "a full overlay must mask base's gain"
        );
        s.load_scene(1).unwrap(); // scene 1: a bypass-only overlay (no gain key)
        assert_eq!(
            sim.rendered_param("G1", "amp", "gain"),
            Some(2.5),
            "a bypass-only overlay must render BASE for gain (2.5), not retain scene 0's 5.0"
        );
    }

    // (2) `ftswStates` is a derived cache the device ignores on recall — a crafted
    // fixture whose `ftswStates[0]` says the switch is active must NOT override the
    // materialized bypass state (here: the overlay bypasses the block → FS inactive).
    #[test]
    fn scene_recall_ignores_the_stored_ftsw_states_cache_and_derives_from_materialized_bypass() {
        const FIXTURE: &str = r#"{"audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"X","nodeId":"fx","dspUnitParameters":{"bypass":false}}
        ]}},
        "scenes":[
            {"guitarNodes":{"G1":{"fx":{"dspUnitParameters":{"bypass":true}}}},
             "ftswStates":[true]}
        ]}"#;
        let sim = SimDevice::new().with_preset_json(FIXTURE);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(10).unwrap();
        s.load_scene(0).unwrap();
        assert_eq!(
            sim.rendered_bypass("G1", "fx"),
            Some(true),
            "the overlay's own bypass=true must govern (FS inactive, block silent) even \
             though this fixture's ftswStates[0] — a cache the real device ignores on \
             recall — claims the switch is active"
        );
    }

    // (3) `changeParameter` accepts RAW values outside [0,1] (dB-calibrated params like
    // `ACD_Boost.gain`) with no clamp and no rejection.
    #[test]
    fn parameter_writes_accept_raw_values_outside_0_1() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(7).unwrap();
        s.change_parameter("G1", "ACD_Boost", "gain", 7.0).unwrap();
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "ACD_Boost", "gain"),
            Some(7.0),
            "a dB-calibrated raw write outside [0,1] must not be clamped or rejected"
        );
    }

    // (4) A scene-context write with no preceding Scene Edit enable, on a node whose
    // overlay in this scene is bypass-only, lands on BASE (and thus on every scene
    // sharing that knob) — the bypass-only overlay itself is left untouched, and other
    // scenes' FULL overlays are untouched too.
    #[test]
    fn scene_context_write_without_scene_edit_enable_on_a_bypass_only_overlay_lands_on_base() {
        const FIXTURE: &str = r#"{"audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"X","nodeId":"amp","dspUnitParameters":{"bypass":false,"gain":2.5}}
        ]}},
        "scenes":[
            {"guitarNodes":{"G1":{"amp":{"dspUnitParameters":{"bypass":false}}}}},
            {"guitarNodes":{"G1":{"amp":{"dspUnitParameters":{"bypass":false,"gain":9.9}}}}}
        ]}"#;
        let sim = SimDevice::new().with_preset_json(FIXTURE);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(8).unwrap();
        s.load_scene(0).unwrap(); // scene 0: bypass-only overlay on "amp" — no enable sent
        s.change_parameter("G1", "amp", "gain", 7.0).unwrap();
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "amp", "gain"),
            Some(7.0),
            "no Scene Edit enable + a bypass-only overlay must route the write to BASE"
        );
        assert_eq!(
            sim.param_write(0, "G1", "amp", "gain"),
            None,
            "the bypass-only overlay itself must stay untouched — no partial scene entry"
        );
        assert_eq!(
            sim.rendered_param("G1", "amp", "gain"),
            Some(7.0),
            "scene 0 has no overlay for gain, so it must now render the just-written base value"
        );
        // The existing enable+reseed behavior must remain green (pinned separately by
        // `enabling_scene_edit_reseeds_the_node_from_base`); here we only need scene 1's
        // OWN full overlay to be untouched by our scene-0 base write.
        s.load_scene(1).unwrap();
        assert_eq!(
            sim.rendered_param("G1", "amp", "gain"),
            Some(9.9),
            "scene 1's own full overlay must be untouched by scene 0's base-routed write"
        );
    }

    // (4, revised) A scene-context write with no preceding Scene Edit enable, on a node
    // whose overlay in this scene IS Full-shaped but doesn't yet carry the written param,
    // lands ON the overlay — extending it per-param — not on base. HW-verified fw 1.8.45,
    // crafted Full-partial overlay: a TubeScreamer scene-0 overlay carrying
    // blend/overdrive/tone but not `level` (base `level` 0.65); an enable-less
    // `changeParameter(level, 0.22)` landed IN the overlay, every sibling param survived
    // unchanged (no reseed), and base stayed 0.65.
    #[test]
    fn scene_context_write_without_scene_edit_enable_on_a_full_overlay_extends_it_per_param() {
        const FIXTURE: &str = r#"{"audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TubeScreamer","nodeId":"ts","dspUnitParameters":
                {"bypass":false,"blend":0.5,"overdrive":0.3,"tone":0.4,"level":0.65}}
        ]}},
        "scenes":[
            {"guitarNodes":{"G1":{"ts":{"dspUnitParameters":
                {"bypass":false,"blend":0.41,"overdrive":0.0,"tone":0.5}}}}}
        ]}"#;
        let sim = SimDevice::new().with_preset_json(FIXTURE);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(9).unwrap();
        s.load_scene(0).unwrap(); // scene 0: Full overlay missing `level` — no enable sent
        s.change_parameter("G1", "ts", "level", 0.22).unwrap();
        assert_eq!(
            sim.param_write(0, "G1", "ts", "level"),
            Some(0.22),
            "an enable-less write of a param absent from a FULL overlay must land IN the \
             overlay (extending it), not fall through to base"
        );
        assert_eq!(
            sim.param_write(SCENE_BASE, "G1", "ts", "level"),
            None,
            "base must stay untouched by the overlay-extending write"
        );
        assert_eq!(
            sim.rendered_param("G1", "ts", "blend"),
            Some(0.41),
            "the overlay's own sibling params must survive unchanged — no reseed"
        );
        assert_eq!(
            sim.rendered_param("G1", "ts", "level"),
            // `rendered_param` widens the stored f32 write to f64 — compare against the
            // same widened value, not the f64 literal (0.22f32 as f64 != 0.22f64).
            Some(f64::from(0.22f32)),
            "scene 0 must now render the just-extended level"
        );
    }
}

// ─── ftsw working-copy model, PLAIN (non-e2e) build ─────────────────────────────────────
//
// The e2e-only gates for field 54/55 live in `e2e_server_tests` (they need a scenario fixture
// whose `ftsw` carries real switches). These cover what those cannot: the same wire ops
// against the DEFAULT graph, which has no `ftsw` key at all — the document every plain-build
// `Session` test sees.
//
// They assert on the RENDERED field-3 body rather than through `Session::live_ftsw`, and that
// is deliberate. `best_json_payload` reassembles with `proto::reassemble_streams`, which by
// design does NOT close the stream on the trailing `0x35` frame (an interleaved single-frame
// must not truncate a large push), so the LAST ≤60 B of every field-3 document are dropped —
// the "systematic tail truncation" `reassemble_streams_final`'s doc describes. `serde_json`'s
// map is a `BTreeMap`, so in this tiny default document `ftsw` sorts LAST and lands entirely
// inside that dropped tail; in a real preset (and in every e2e scenario fixture) `ftsw` sits
// ~4 KB in, far ahead of it, which is exactly what `Session::live_ftsw`'s own doc records.
// Reading the render directly keeps these tests on the fake's contract instead of re-testing
// the session reader's documented lossiness.
#[cfg(test)]
mod ftsw_tests {
    use super::*;
    use crate::session::Session;

    /// The `param` functionJson shape the leveler's Assign branch writes, trimmed to the keys
    /// the read-back helpers match on. Carries `valueType` (numeric) because the real
    /// composer (`leveller.rs`) does too — its absence makes fw 1.8.45 silently discard the
    /// whole IMPORTED preset at its lazy commit (see `notes/gotchas.md`).
    const PARAM_FN: &str = r#"{"func":"param","groupId":"G1","nodeId":"n1","parameterId":"level","valueA":0.7,"valueB":0.2,"valueType":2}"#;

    /// The field-3 body the fake would push for the current slot, parsed.
    fn rendered(sim: &SimDevice) -> serde_json::Value {
        let mut st = sim.state.lock().expect("sim lock");
        let slot = st.current_slot;
        let body = load_echo_json(&mut st, slot);
        serde_json::from_slice(&body).expect("the rendered field-3 body is valid JSON")
    }

    /// A `setFootswitchAssignment` MATERIALIZES `ftsw` on a document that carries none, and
    /// the render carries it — the working-copy half of the Assign confirm gate in a build
    /// with no scenario fixtures.
    #[test]
    fn a_set_materializes_ftsw_on_a_graph_that_has_none() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(7).unwrap();
        assert!(
            rendered(&sim).get("ftsw").is_none(),
            "the default graph carries no ftsw key until an edit creates one"
        );
        s.set_footswitch_assignment(2, 0, PARAM_FN, false, None)
            .unwrap();
        let doc = rendered(&sim);
        let live = doc
            .get("ftsw")
            .expect("the edit must materialize an ftsw array");
        assert_eq!(
            crate::footswitch::existing_param_fn_value_a(live, 2, "n1", "level"),
            Some(0.7),
            "switch 2 must render the written param fn: {live}"
        );
        // Switches ahead of the addressed one are materialized EMPTY, never holes — the shape
        // `footswitch::existing_param_fn_index` walks.
        assert_eq!(
            live.as_array().map(Vec::len),
            Some(3),
            "the array grows to cover addr 2: {live}"
        );
        assert!(
            live[0].as_array().is_some_and(Vec::is_empty)
                && live[1].as_array().is_some_and(Vec::is_empty),
            "unaddressed switches are empty function lists: {live}"
        );
    }

    /// A `clearFootswitchAssignment` removes the function, and a fresh load discards the whole
    /// UNSAVED edit — the plain-build mirror of the device's edit-buffer semantics (a plain
    /// build has no `SavedDoc`, so nothing can persist it).
    #[test]
    fn a_clear_removes_the_function_and_a_load_discards_the_unsaved_edit() {
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.load_preset(7).unwrap();
        s.set_footswitch_assignment(1, 0, PARAM_FN, false, None)
            .unwrap();
        s.clear_footswitch_assignment(1, 0).unwrap();
        let doc = rendered(&sim);
        let live = doc
            .get("ftsw")
            .expect("ftsw stays materialized after the clear");
        assert_eq!(
            crate::footswitch::existing_param_fn_index(live, 1, "n1", "level"),
            None,
            "the cleared function must be gone: {live}"
        );
        s.set_footswitch_assignment(1, 0, PARAM_FN, false, None)
            .unwrap();
        s.load_preset(7).unwrap();
        assert!(
            rendered(&sim).get("ftsw").is_none(),
            "a fresh load discards the unsaved ftsw edit buffer"
        );
        assert_eq!(
            sim.events()
                .iter()
                .filter(|e| matches!(
                    e,
                    SimEvent::SetFootswitchAssignment { .. }
                        | SimEvent::ClearFootswitchAssignment { .. }
                ))
                .count(),
            3,
            "every ftsw wire op is recorded: {:?}",
            sim.events()
        );
    }
}
