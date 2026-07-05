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

use std::sync::{Arc, Mutex};

use crate::hid::HidTransport;
use crate::proto;

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
// Device confirmation / echo field numbers.
const F_PRESET_LOADED: u32 = 11;
const F_NODE_INSERTED: u32 = 33;
const F_NODE_REMOVED: u32 = 36;
const F_NODE_REPLACED: u32 = 40;
const F_PRESET_ERROR: u32 = 53;
const F_PRESET_LEVEL_CHANGED: u32 = 77;

/// One device-visible action the fake observed, in order.
#[derive(Debug, Clone, PartialEq)]
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
        }
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
    pub fn with_drop_first(self) -> SimDevice {
        self.state.lock().expect("sim lock").drop_first = true;
        self
    }

    /// REJECT the `n`th structural edit (1-based) with `presetError` (never save after).
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
            return Vec::new();
        };
        let f = proto::parse(pm);
        let mut st = self.state.lock().expect("sim lock");

        if let Some(lp) = proto::first_bytes(&f, F_LOAD_PRESET) {
            let dev_slot = proto::first_varint(&proto::parse(lp), 6).unwrap_or(0);
            let slot0 = dev_slot.saturating_sub(1) as u32;
            st.events.push(SimEvent::Loaded(slot0));
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
            return vec![frame(&preset_level_changed(level))];
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
