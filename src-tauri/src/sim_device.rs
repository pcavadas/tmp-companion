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
//! offline at all. The doc is `presetLevel` PLUS a footswitch bake's own baked param
//! (`SavedDoc`) — narrower than a full merged presetJson (scene overlays and `ftsw`
//! ASSIGN edits do NOT round-trip through a save; `SavedDoc`'s doc comment has the
//! deviation rationale). See `saved_levels`' field doc for the exact read/load asymmetry.
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
/// DEVIATION from the plan's full "merged presetJson" (module header, `record_save`'s
/// doc): scene-overlay writes and `ftsw` ASSIGNMENT edits are NOT merged in — only a
/// BAKED (`func: "on-off"`, no assign) footswitch's own leveled param survives a
/// save→load round trip. No offline spec exercises a scene-outputLevel or an ASSIGN
/// switch's `valueA` through a save→load round trip (the ASSIGN wire op itself,
/// `SetFootswitchAssignment`, isn't modeled by `handle` at all — an assign write falls
/// through to a silent no-op reply, same as today), so widening past what
/// `level-fs-preset24.spec.ts` actually needs was left undone rather than half-built.
#[cfg(feature = "e2e")]
#[derive(Clone, Default)]
struct SavedDoc {
    preset_level: f32,
    /// Baked `(group, node, param) → value` overlay — a footswitch bake's own knob,
    /// SCENE_BASE-scoped only (see the deviation note above).
    params: HashMap<(String, String, String), f32>,
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
            current_slot: 0,
            current_scene: None,
            saved_scene: HashMap::new(),
            preset_level: 1.0,
            param_writes: HashMap::new(),
            bypass_writes: HashMap::new(),
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
    /// bake), becomes `slot0`'s pending doc, landing after [`SimState::commit_latency`].
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
        for ((scene, group, node, param), v) in &self.param_writes {
            if *scene == SCENE_BASE {
                params.insert((group.clone(), node.clone(), param.clone()), *v);
            }
        }
        let deadline = std::time::Instant::now() + self.commit_latency();
        self.pending_level_entry(slot0).pending = Some((
            SavedDoc {
                preset_level: level,
                params,
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
/// param)` overlay into a preset-JSON BODY TEXT — the fields the lazy-commit model
/// synthesizes into the field-3/field-8 JSON string itself (the rest of that TEXT —
/// `scenes`, `ftsw`, scene-scoped param overlays — stays exactly the committed scenario
/// body; no offline caller reads THOSE back through a save round trip, so deep-merging
/// them is out of scope here — see the module header's deviation note). Patching the
/// baked params into this TEXT (not just `SimState::param_writes`, which is what
/// `model_lufs` actually reads) matters for two READERS of the text itself:
/// `leveller::witness_value_in_doc` compares a `SaveWitness::Param` against the
/// FIELD-3 echo, and `verify_fs_persisted_writes`'s post-save field-8 read expects the
/// just-baked value to show up in the SAVED document, not the pristine fixture body —
/// without this, both would see the pre-bake value forever and report a false
/// persist-mismatch / a permanently-stale witness compare. Falls back to the original
/// bytes on a parse failure, which never happens for a committed fixture but a caller
/// must still get SOMETHING. Patches `audioGraph.guitarNodes` groups ONLY: a `micNodes`
/// baked param would silently patch nothing — no current fixture routes one; extend the
/// group lookup here FIRST if a mic-path fixture ever bakes a param, or its persist
/// verify reads the pristine value and reports a false mismatch with no diagnostic.
#[cfg(feature = "e2e")]
fn with_patched_doc(
    json: &str,
    level: f32,
    params: &HashMap<(String, String, String), f32>,
) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    if let Some(ag) = v.get_mut("audioGraph") {
        ag["presetLevel"] = serde_json::json!(level);
        if let Some(groups) = ag.get_mut("guitarNodes").and_then(|g| g.as_object_mut()) {
            for ((group, node, param), value) in params {
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
        self.state.lock().expect("sim lock").preset_json = json.to_string();
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
                    self.state.lock().expect("sim lock").reamp_on = on;
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
            st.current_scene = match st.saved_scene.get(&slot0).copied() {
                Some(scene) => scene,
                // No recorded save this run → the FIXTURE's own `lastLoadedScene`, so a
                // scenario preset saved in a scene (404) activates it offline exactly as the
                // unit does, with no per-test setup.
                None => fixture_last_loaded_scene(slot0),
            };
            st.param_writes.clear();
            st.bypass_writes.clear();
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
            // Push `currentPresetDataChanged`(3) so the un-engaged scene-leveling PRE-PASS can
            // classify each scene's routing (the real device pushes the scene graph on a scene
            // CHANGE). The sim carries one graph, and the physics model reads the written
            // `outputLevel` node-agnostically, so re-serving the same graph per scene is enough
            // for the amp pick to resolve — without it, the pre-pass harvests nothing and every
            // scene fails to classify ("read failed").
            let current_slot = st.current_slot;
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
                    scene,
                    group: group.clone(),
                    node: node.clone(),
                    param: param.clone(),
                    value: v,
                });
                st.param_writes.insert((scene, group, node, param), v);
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
// What offline-green STILL does NOT prove: a scene-outputLevel CLAMP verdict, an ASSIGN-path
// footswitch (only BAKE is modeled — no `SetFootswitchAssignment` wire op in `handle`), and
// parallel-lane summation (joint-k) — still online-only classes.

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
        match model_lufs(&st, sidecar(), stored_levels()) {
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
    st: &SimState,
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
    // Leveled-param response (preset-024-class drive pedal into a saturated amp):
    // overrides the flat C law for exactly the ENGAGED declared block's own knob — Base
    // and every OTHER switch's isolated measurement (this block bypassed) fall through
    // to the ordinary formula below unperturbed.
    if let Some(lp) = entry.and_then(|e| {
        e.leveled_params
            .iter()
            .find(|lp| st.bypass_writes.get(&lp.node).copied() == Some(false))
    }) {
        let key = (
            st.scene_key(),
            lp.group.clone(),
            lp.node.clone(),
            lp.param.clone(),
        );
        let v = st.param_writes.get(&key).copied()?;
        let c = saturated_pedal_lufs(v)?;
        let preset_term = 20.0 * f64::from(st.preset_level.max(1e-6)).log10();
        return Some(c + preset_term);
    }
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
    let preset_term = 20.0 * f64::from(st.preset_level.max(1e-6)).log10();
    Some(c + preset_term + ol_term)
}

/// Scale `stimulus` so its measured integrated LUFS lands exactly at `l_model`
/// (`s = 10^((l_model − l_stim)/20)`). Deterministic. A silent stimulus (non-finite
/// `l_stim`) can't be modeled → returned verbatim.
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
    let s = 10f64.powf((l_model - l_stim) / 20.0) as f32;
    crate::audio::Capture {
        interleaved: stimulus.iter().map(|x| x * s).collect(),
        channels: 1,
        sample_rate: rate,
    }
}

/// One mono channel of the stimulus, verbatim (the pre-physics fallback).
#[cfg(feature = "e2e")]
fn passthrough(stimulus: &[f32], rate: u32) -> crate::audio::Capture {
    crate::audio::Capture {
        interleaved: stimulus.to_vec(),
        channels: 1,
        sample_rate: rate,
    }
}

/// `n` samples of silence — `loudest_loudness` reports "no signal captured" on it, exactly
/// as a real silent USB return does, driving the leveller's no-signal / off-branch verdict.
#[cfg(feature = "e2e")]
fn silent_capture(n: usize, rate: u32) -> crate::audio::Capture {
    crate::audio::Capture {
        interleaved: vec![0.0; n.max(1)],
        channels: 1,
        sample_rate: rate,
    }
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
    /// Blocks/params whose OWN knob drives loudness through [`saturated_pedal_lufs`]
    /// instead of the flat `C + preset_term + ol_term` law — a drive pedal feeding a
    /// saturated amp (preset-024-class; notes/leveling.md). Activates ONLY while that
    /// block is ENGAGED (`bypass_writes[node] == Some(false)` — i.e. under ITS OWN
    /// isolated footswitch measurement); every other slot's empty default vector leaves
    /// this branch dead, so it can never perturb an existing scenario's numbers.
    #[serde(default)]
    leveled_params: Vec<LeveledParam>,
}

/// One block/param whose knob feeds [`saturated_pedal_lufs`] instead of the flat C law —
/// see [`SlotLoudness::leveled_params`].
#[cfg(feature = "e2e")]
#[derive(serde::Deserialize, Clone)]
struct LeveledParam {
    group: String,
    node: String,
    param: String,
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
fn saturated_pedal_lufs(v: f32) -> Option<f64> {
    let v = f64::from(v);
    if v <= 0.10 {
        return None;
    }
    if v <= 0.20 {
        // The cliff: 42 LU over this 0.10 span.
        return Some(-62.0 + 420.0 * (v - 0.10));
    }
    if v <= 0.30 {
        // Shoulder — the cliff's top IS the plateau's own floor (continuous, no dip).
        return Some(-20.0);
    }
    // The compressed plateau: +3 LU across the remaining 0.70 of knob travel.
    let t = ((v - 0.30) / 0.70).min(1.0);
    Some(-20.0 + 3.0 * t)
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
    /// The base amp `outputLevel` (the single guitarNodes node carrying one).
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
/// The base graph (`guitarNodes.G1`) is an ARRAY of node objects; a scene overlay
/// (`scenes[i].guitarNodes.G1`) is a MAP of `nodeId → { dspUnitParameters }`.
#[cfg(feature = "e2e")]
fn stored_from_preset(pj: &serde_json::Value) -> StoredLevels {
    let g1 = pj
        .get("audioGraph")
        .and_then(|a| a.get("guitarNodes"))
        .and_then(|g| g.get("G1"));
    let base = g1
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.iter().find_map(node_output_level));
    let scenes = pj
        .get("scenes")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .map(|sc| {
                    sc.get("guitarNodes")
                        .and_then(|g| g.get("G1"))
                        .and_then(|nodes| nodes.as_object())
                        .and_then(|m| m.values().find_map(node_output_level))
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
        return with_patched_doc(j, doc.preset_level, &doc.params).into_bytes();
    }
    let _ = slot0;
    st.preset_json.as_bytes().to_vec()
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
        saved_slot_json(slot0).map(|j| with_patched_doc(j, doc.preset_level, &doc.params))
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
        Ok(()) // fire-and-forget (heartbeat) — no reply
    }
    fn transact(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.handle(body))
    }
    fn transact_chunked(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(self.handle(body))
    }
    fn pump(&self, _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        Ok(Vec::new()) // replies are delivered synchronously from the send
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
            &SimState {
                current_slot: 401,
                preset_level: 0.5,
                ..Default::default()
            },
            &sc,
            &stored,
        )
        .unwrap();
        let quiet = model_lufs(
            &SimState {
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
            model_lufs(&st, &sc, &stored).unwrap()
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
            model_lufs(&st, &sc, &stored).is_none(),
            "bypassing the routed amp must silence the capture"
        );
        st.bypass_writes.insert("ACD_Amp".into(), false);
        assert!(
            model_lufs(&st, &sc, &stored).is_some(),
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
            model_lufs(&st, &sc, &stored)
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
        let rate = 48_000u32;
        let cap = scale_stimulus(&tone(rate), rate, -20.0);
        let measured = crate::lufs::measure_mono(&cap.channel(0), rate)
            .unwrap()
            .integrated_lufs;
        assert!(
            (measured + 20.0).abs() < 0.3,
            "scaled to hit -20 LUFS, measured {measured}"
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
        // Our two FS fixture targets (-26/-24) must land ON this cliff, not the flat
        // plateau — the whole reason the fixture is solvable at a tight ±0.1 LU offline.
        assert!(
            (lo..=hi).contains(&-26.0) && (lo..=hi).contains(&-24.0),
            "targets -26/-24 must fall inside the cliff [{lo}, {hi}]"
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
        // Not engaged (no write at all) — the ordinary flat C, unperturbed by the pedal
        // curve: the "don't touch existing scenarios" guarantee, pinned at the boundary.
        assert!(
            (model_lufs(&st, &sc, &stored).unwrap() - (-10.0)).abs() < 1e-6,
            "unengaged must fall through to the flat C"
        );
        // Engaged (bypass=false) but nothing written yet for the leveled param → no
        // signal, never a silent fallback to the flat C (which would mask a real bug).
        st.bypass_writes.insert("pedal".into(), false);
        assert!(
            model_lufs(&st, &sc, &stored).is_none(),
            "engaged with no written value must not silently read as the flat C"
        );
        // Engaged with a mid-cliff value → the curve's own arithmetic.
        st.param_writes.insert(
            (SCENE_BASE, "G1".into(), "pedal".into(), "level".into()),
            0.1857,
        );
        let lufs = model_lufs(&st, &sc, &stored).unwrap();
        assert!(
            (lufs - (-26.0)).abs() < 0.2,
            "v=0.1857 should land near -26 LUFS, got {lufs}"
        );
        // Forced OFF again (a sibling's own isolated measurement) — back to the flat C.
        st.bypass_writes.insert("pedal".into(), true);
        assert!((model_lufs(&st, &sc, &stored).unwrap() - (-10.0)).abs() < 1e-6);
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
}
