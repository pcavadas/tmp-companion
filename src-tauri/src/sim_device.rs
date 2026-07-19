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
const SCENE_BASE: i64 = -1;

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
    /// last `loadScene`. Reset to base on every `loadPreset`.
    current_scene: Option<u32>,
    /// The last `setPresetLevel` — the linear global multiplier the model shifts by
    /// `20·log10`. SIMPLIFICATION: a real `loadPreset` restores the slot's SAVED
    /// presetLevel, but the sim tracks no per-slot saved value, so it just PRESERVES the
    /// last-set value across a load. Faithful for the leveling flow (every `measure_c`
    /// re-sets a reference before it's ever read, and a load right after a base save leaves
    /// the last-set == the saved value), but a `ref_level = None` capture (the Doctor A/B)
    /// after leveling a DIFFERENT slot would read a leaked multiplier — add a per-slot
    /// `stored_preset_level` map when an offline Doctor spec needs cross-slot fidelity.
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
            preset_json: r#"{"audioGraph":{"guitarNodes":{"G1":[
                {"FenderId":"ACD_Twin57","nodeId":"n1"},
                {"FenderId":"ACD_ChorusCE2","nodeId":"n2"}
            ]}}}"#
                .to_string(),
            current_slot: 0,
            current_scene: None,
            preset_level: 1.0,
            param_writes: HashMap::new(),
            bypass_writes: HashMap::new(),
            reamp_on: false,
            #[cfg(feature = "e2e")]
            fail_capture_slot: None,
        }
    }
}

impl SimState {
    /// The active scene as the [`param_writes`](SimState::param_writes) `i64` key.
    fn scene_key(&self) -> i64 {
        self.current_scene.map_or(SCENE_BASE, i64::from)
    }
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

    /// Override the preset JSON a `loadPreset` echoes as `currentPresetDataChanged`(3) —
    /// the pre-edit roster the `blockcaps` guard reads. Lets a test configure a specific
    /// `audioGraph` (e.g. one already at a block-count cap) to exercise the guard's
    /// refusal end-to-end.
    #[cfg(test)]
    pub fn with_preset_json(self, json: &str) -> SimDevice {
        self.state.lock().expect("sim lock").preset_json = json.to_string();
        self
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
            // A load resets the active scene to base and discards the edit buffer (the
            // scene-scoped knob writes + forced bypasses). `preset_level` is deliberately
            // NOT reset — see its field doc (the sim has no per-slot saved value, so it
            // preserves the last-set multiplier; faithful for the leveling flow).
            st.current_slot = slot0;
            st.current_scene = None;
            st.param_writes.clear();
            st.bypass_writes.clear();
            // Echo `currentPresetDataChanged`(3) right after the load — the real device's
            // post-load push the `blockcaps` guard reads as the pre-edit roster
            // (`Session::current_preset_value`). May exceed one HID frame, so chunk it.
            let mut reports = vec![frame(&preset_loaded(dev_slot))];
            reports.extend(frame_multi(&current_preset_data_changed(
                st.preset_json.as_bytes(),
            )));
            return reports;
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
            st.events
                .push(SimEvent::Saved(dev_slot.saturating_sub(1) as u32));
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
            // LoadScene{ sceneSlot(1) } — the 0-based scenes[] wire index.
            st.current_scene = Some(proto::first_varint(&proto::parse(ls), 1).unwrap_or(0) as u32);
            return Vec::new();
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
                st.param_writes.insert((scene, group, node, param), v);
            } else if param == "bypass" {
                let on = proto::first_varint(&inner, 7).unwrap_or(0) != 0;
                st.bypass_writes.insert(node, on);
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
// FIDELITY CEILING (scene-outputLevel CLAMP verdicts — a PR3 decision, not faithful yet):
// `loadPreset` echoes a CONSTANT default graph for every slot (see the load handler), so
// offline amp discovery always finds that graph's amp, and the leveller writes `outputLevel`
// to it — a DIFFERENT node than the scenario's stored amp. The node-AGNOSTIC written-side
// match + the leveller's closed-loop verify still converge the MEASURED LUFS (the
// default-graph knob factor cancels), so solvable-row values are faithful. But the scene
// CLAMP classification is solved against the default-graph knob, unrelated to the sidecar C
// or the scenario's stored knob — so a scene-outputLevel "clamped-at-max" outcome is NOT
// faithfully authorable via the sidecar. PR3 must therefore author every CLAMP outcome on
// the base/presetLevel path (which clamps directly on C at LEVEL_MAX — fully faithful), and
// list scene-outputLevel headroom in the plan's "what offline-green does NOT prove". The
// faithful fix (echo each slot's real graph so written==stored) is deferred to PR3.

/// Arm the currently-installed fake so `slot`'s NEXT capture returns silence once (the
/// `POST /sim/fault` bridge endpoint). No-op when no fake is installed (online).
#[cfg(feature = "e2e")]
pub fn arm_capture_fault(slot: u32) {
    if let Some(dev) = LIVE.lock().expect("sim live lock").as_ref() {
        dev.state.lock().expect("sim lock").fail_capture_slot = Some(slot);
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
}
