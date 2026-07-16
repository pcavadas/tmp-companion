//! High-level TMP control session: handshake, preset enumeration, and the
//! leveling primitives (load preset, toggle re-amp, set preset level, save).
//!
//! The load-bearing USB-protocol invariants (batchStatus grouping, re-amp latch
//! rules, HID open-lockout, slot +1 addressing, the capture window) are written
//! up in `notes/protocol.md`.
//!
//! Builds on `hid` (transport) + `proto` (wire codec). Replicates the request
//! sequence that is known-good against a real device. The
//! handshake REUSES `batchStatus` in the device's exact groups (1; 2×7; 3; 4) —
//! NOT a per-request counter: incrementing per request makes the device go silent
//! after the preset lists. Setters and heartbeats omit `batchStatus` entirely.

use serde::Serialize;

use crate::hid::{Hid, HidTransport};
use crate::proto::{self, Val};

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

    type Factory = Box<dyn Fn() -> Box<dyn HidTransport> + Send + Sync>;
    static FACTORY: OnceLock<Mutex<Option<Factory>>> = OnceLock::new();

    fn cell() -> &'static Mutex<Option<Factory>> {
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
/// lane and the monitor lane.
fn open_transport() -> Result<Box<dyn HidTransport>, String> {
    #[cfg(feature = "e2e")]
    if let Some(t) = e2e_transport::take() {
        return Ok(t);
    }
    Ok(Box::new(Hid::open()?))
}

/// One entry in the device's "My Presets" list. `slot` is the position used by
/// `LoadPreset.presetSlot`; `name` is the display name. NOTE: the list-index →
/// presetSlot mapping (0- vs 1-based) is confirmed on real hardware in M3 by
/// loading a slot and reading back the current-preset info.
#[derive(Debug, Clone, Serialize)]
pub struct PresetEntry {
    pub slot: u32,
    pub name: String,
}

/// Timing + integrity metadata from a [`Session::device_backup`] run.
#[derive(Debug, Clone, Serialize)]
pub struct BackupStats {
    /// Wall-clock from request to last chunk.
    pub elapsed_secs: f64,
    /// `numBytes` the device declared (uncompressed-on-wire archive size).
    pub num_bytes: u32,
    /// `numChunks` the device declared.
    pub num_chunks: u32,
    /// Distinct `chunkNum`s actually received.
    pub chunks_received: u32,
    /// `crc` the device stamped on every chunk (over the whole archive).
    pub crc: u32,
    /// Bytes we reassembled (should equal `num_bytes` on a clean transfer).
    pub bytes_assembled: usize,
    /// Time to the first chunk (handshake + device-side archive build).
    pub first_chunk_secs: f64,
    /// `(elapsed_secs, BackupRestoreState.State)` transitions, in order.
    pub state_log: Vec<(f64, i64)>,
    /// Max `BackupRestoreState.progressSize` (field 2) seen — non-zero iff the
    /// device reports build-phase progress (lets the "Preparing…" phase be
    /// determinate too). 0 means the device doesn't populate it.
    pub build_size: u32,
    /// Max `BackupRestoreState.progressTicks` (field 3) seen.
    pub build_ticks: u32,
}

/// Live progress for a [`Session::device_backup`] run (drives a progress bar).
#[derive(Debug, Clone, Serialize)]
pub struct BackupProgress {
    /// `"building"` (device assembling the archive, before chunks) or
    /// `"streaming"` (chunks arriving — `percent` is exact).
    pub phase: &'static str,
    /// Chunks received so far.
    pub received: u32,
    /// Total chunks (`numChunks`, known from the first chunk; 0 while building).
    pub total: u32,
    /// Archive bytes assembled so far.
    pub bytes: u64,
    /// Total archive bytes (`numBytes`; 0 while building).
    pub total_bytes: u64,
    /// 0..100, exact during streaming (`received/total`); 0 while building unless
    /// the device reports `build_size`/`build_ticks`.
    pub percent: f32,
    /// `BackupRestoreState.progressSize` (device build progress; 0 if unreported).
    pub build_size: u32,
    /// `BackupRestoreState.progressTicks`.
    pub build_ticks: u32,
}

/// True if a "My Presets" display name marks an EMPTY slot. The device shows
/// `--` at cleared positions (observed on 1.7.75); also treat a blank/`Empty`
/// name as empty defensively. Used to pick a scratch slot and to skip empties
/// when locating an imported preset.
pub fn is_empty_slot_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n == "--" || n == "\u{2014}" || n.eq_ignore_ascii_case("empty")
}

/// True iff `name` appears at EXACTLY ONE position in the My Presets `names` list and
/// that position is `list_index`. The uniqueness proof the [`Session::active_matches`]
/// name fallback needs before it may confirm a save target: a duplicated display name
/// (two slots, same name) or a name that matches no/other slot must fail closed.
pub(crate) fn name_maps_uniquely(names: &[String], name: &str, list_index: u32) -> bool {
    let mut hit = None;
    for (i, n) in names.iter().enumerate() {
        if n == name {
            if hit.is_some() {
                return false; // duplicate name ⇒ can't prove which slot is the target
            }
            hit = Some(i as u32);
        }
    }
    hit == Some(list_index)
}

/// A level-type block control discoverable from a preset's `audioGraph` — a
/// candidate leveling knob. `group_id`/`node_id`/`parameter_id` are the
/// `ChangeParameter` coordinates; `model_id` is the stable Fender block id for
/// catalog classification; `value` is the current normalized (0..1) value.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LevelBlock {
    pub group_id: String,
    pub node_id: String,
    pub model_id: String,
    pub parameter_id: String,
    pub value: f32,
}

/// One row of a Song's preset assignment list (`SongPresetListRecord`). The
/// load-bearing fields are `user_preset_slot` (the **device** slot the
/// Song row points at — the positional binding an in-place edit must preserve)
/// and `preset_scene_slot` (the bound scene's position). `is_empty` marks an
/// unassigned row.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SongPresetRecord {
    pub is_empty: bool,
    pub user_preset_slot: u32,
    pub preset_scene_slot: u32,
    pub preset_scene_name: String,
}

/// One block (DSP node) in the active preset's signal chain, for the
/// active-preset signal chain rendering. `model` is the node's `FenderId` (`ACD_*` factory / `USR_*` user IR)
/// — the identity the frontend maps to a pedal icon. `bypassed` reads
/// `dspUnitParameters.bypass`. Routing (which group, series vs parallel) is carried
/// by [`ActiveGraph`].
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub group_id: String,
    pub node_id: String,
    pub model: String,
    pub bypassed: bool,
    /// For a CabSim block: its primary cabinet id (`dspUnitParameters.cabsimid`,
    /// e.g. `Mar1960aV30Alt`) — lets the strip NAME the cab instead of the generic
    /// "CAB IR", and expand a dual-cab into two parallel tiles. `None` for non-cab
    /// nodes (omitted from the wire via `skip_serializing_if`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim_id: Option<String>,
    /// The SECOND cabinet id of a dual-cab block (`cab2simid`), set when
    /// `cab_sim2_enabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim_id2: Option<String>,
    /// Whether this CabSim runs two cabinets in parallel (`cabsim2enabled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim2_enabled: Option<bool>,
    /// ALLOWLISTED current `dspUnitParameters` values for Doctor's value-aware
    /// prescriptions: the reverb wet/dry mix names (`mix`, `wetdrymix`), the cab
    /// low/high cut (`hpf`, `lpf`), and the EQ-10 `gain*hz` band gains. Empty for
    /// every other param/node — the full param map would bloat every snapshot/
    /// backup row for nothing.
    pub params: std::collections::HashMap<String, f64>,
}

/// One ordered stage of the signal chain: a `series` run of blocks, or a `split`
/// with two parallel lanes (each a series run) joined by a mix. A chain is a
/// `Vec<Stage>` walked left→right, so any number of sequential splits with series
/// segments before/between/after them is representable (the device's 7 guitar
/// slots allow up to two). Serializes to the frontend's discriminated union:
/// `{kind:"series",blocks:[…]}` / `{kind:"split",a:[…],b:[…]}`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Stage {
    Series {
        blocks: Vec<GraphNode>,
    },
    Split {
        a: Vec<GraphNode>,
        b: Vec<GraphNode>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InputLane {
    #[serde(rename = "type")]
    pub kind: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OutputLane {
    #[serde(rename = "type")]
    pub kind: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InputPair {
    pub a: InputLane,
    pub b: InputLane,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OutputPair {
    pub a: OutputLane,
    pub b: OutputLane,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IndependentLane {
    pub input: String,
    pub output: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, PartialEq)]
struct RouteGraph {
    input_type: Option<String>,
    output_type: Option<String>,
    inputs: Option<InputPair>,
    outputs: Option<OutputPair>,
    lanes: Option<Vec<IndependentLane>>,
    stages: Vec<Stage>,
}

/// The active preset's signal-chain topology, read live from the field-3 partial
/// (the only reliable live preset read). `template` gives the active routing
/// (one of 12 signal-path templates); `split_mix` carries the available split/mix
/// controls, including inactive ones. `nodes` are
/// the blocks per group (`G1..G7` guitar, `M1..M4` mic), in stable group+array
/// order. `name`/`slot` are `None` when the device-truncated partial doesn't carry
/// preset `info` (the honest no-fabricate state — the partial truncates at
/// `"scenes"`, and `info` sits past that, so these are usually `None`).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ActiveGraph {
    pub name: Option<String>,
    pub slot: Option<u32>,
    pub template: Option<String>,
    pub split_mix: Option<serde_json::Value>,
    /// Flat block list in stable group+array order (every block, all groups).
    pub nodes: Vec<GraphNode>,
    /// Single-input source label for renderers that need to distinguish guitar
    /// vs mic/line paths. Omitted for dual-input or independent-lane templates.
    pub input_type: Option<String>,
    /// Single-output sink label for renderers that need to distinguish one
    /// summed output from split output 1/2 paths.
    pub output_type: Option<String>,
    /// Dual-input templates: each input lane may carry pre-join blocks, then
    /// the two lanes converge at a JOIN node before `stages`.
    pub inputs: Option<InputPair>,
    /// Split-output templates: `stages` feed a SPLIT node, then the two output
    /// lanes terminate independently at OUT 1 / OUT 2.
    pub outputs: Option<OutputPair>,
    /// Fully independent dual-rail templates such as `gtrMicParallel`.
    pub lanes: Option<Vec<IndependentLane>>,
    /// Routing-aware view: the ordered series/split stages the strip renders.
    /// Derived from the group slots + routing template id.
    pub stages: Vec<Stage>,
}

/// One Song's metadata, decoded from `songListResponse` (the net-new live song
/// read). `bpm_active` mirrors the device's per-song "BPM on/off" flag. Per-song
/// preset assignments are a separate read (`songPresetListResponse`); this carries
/// metadata only.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SongRecord {
    pub slot: u32,
    pub name: String,
    pub notes: String,
    pub bpm: u32,
    pub bpm_active: bool,
}

/// One Setlist's metadata, decoded from `setlistListResponse` (the net-new live
/// setlist read). A `SetlistListRecord` carries only `setlistName`; the device
/// orders the list, so `slot` is the 1-based list position. A setlist's song
/// membership is a separate read (`setlistSongListResponse`); this is names only.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SetlistRecord {
    pub slot: u32,
    pub name: String,
}

/// Burst shapes for [`Session::connect_slotread`] (field-8 investigation).
#[derive(Clone, Copy, Debug)]
pub enum SlotReadBurst {
    /// Full Pro Control burst, read appended last (the classic AC1 spike —
    /// the 0/25-on-1.8.45 baseline).
    Classic,
    /// Read injected right after `connection_request` + the My Presets list
    /// request, BEFORE the flood; the standard burst remainder follows.
    Early,
    /// `connection_request` + My Presets request + the read, nothing else —
    /// tests whether the device answers without the full load-bearing burst.
    Minimal,
}

pub struct Session {
    hid: Box<dyn HidTransport>,
    batch: u64,
    /// Raw input reports accumulated during the current high-level operation.
    /// Reassembled cumulatively (a multi-packet stream can span pump windows).
    /// Cleared at the start of each operation that reads a reply.
    pub(crate) raw: Vec<Vec<u8>>,
    /// Firmware version the device reported during the handshake (in reply to
    /// the in-burst `currentFwRequest` — a standalone request afterwards is
    /// rejected with ConnectionError). Captured at connect time because `raw`
    /// is cleared by later operations.
    fw_version: Option<String>,
}

/// FenderMessageTMS top-level oneof field numbers.
const TMS_PRESET: u32 = 2;
const TMS_SETTINGS: u32 = 3;
/// PresetMessage oneof field numbers.
const PRESET_LIST_RESPONSE: u32 = 5;
const PRESET_LOADED: u32 = 11;
const CURRENT_PRESET_INFO_CHANGED: u32 = 22;
const NODE_REPLACED: u32 = 40; // PresetMessage.nodeReplaced (device confirms a structural replace)
const NODE_REMOVED: u32 = 36; // PresetMessage.nodeRemoved (device confirms a structural remove)
const NODE_INSERTED: u32 = 33; // PresetMessage.nodeInserted (device confirms a structural insert) — SPEC field; the add-block inbound capture caught no reply, so this is unverified on HW (corroborate with a content read-back on the first run)
const NODE_JSON_RESPONSE: u32 = 120; // PresetMessage.nodeJsonResponse (a node's current JSON)
const PRESET_ERROR: u32 = 53; // PresetMessage.presetError (device REJECTS an edit — never save after this)
const PRESET_LEVEL_CHANGED: u32 = 77;
/// SettingsMessage oneof field numbers.
const SETTINGS_CURRENT_FW: u32 = 2; // SettingsMessage.currentFwResponse (reply to in-burst currentFwRequest)
const REAMP_SETTING: u32 = 30; // SettingsMessage.reampModeActive (echo of the setter)
const USB_SETTINGS_RESPONSE: u32 = 56; // SettingsMessage.usbSettingsResponse (UsbSettings)

impl Session {
    /// Open the device (seizing it) and run the full first-connect handshake
    /// with the default pump windows (no preset-data JSON fetch). For
    /// measure/capture-only connections, prefer `connect_lean()` below.
    pub fn connect() -> Result<Session, String> {
        Self::connect_inner(false, None, false, false)
    }

    /// `connect` with the handshake pump windows trimmed ×0.25 (~660 → ~165 ms).
    /// ONLY for pure measure/capture sessions: connections that (a) never read
    /// handshake- or push-accumulated data (`list_my_presets` / `active_preset_name` /
    /// `confirm_active` / slot reads) and (b) never write-then-save. The trimmed
    /// windows are HW-validated capture-byte-identical on those paths (fw 1.8.45,
    /// ~10 sessions incl. ×0.1); write/save sessions keep the full windows because
    /// their device-side timing cliffs (the ~700 ms scene-write window, chunked
    /// ftsw edits) were NOT part of that A/B — do not widen the lean set without a
    /// dedicated write-lands HW bisect.
    pub fn connect_lean() -> Result<Session, String> {
        Self::connect_inner(false, None, false, true)
    }

    /// Like `connect`, but the handshake also issues the field-78 request so the
    /// device emits the current preset's `currentPresetDataChanged` JSON (adds
    /// ~2 s). Use for block enumeration (`current_preset_blocks`), not leveling.
    pub fn connect_for_discovery() -> Result<Session, String> {
        Self::connect_inner(true, None, false, false)
    }

    /// AC1 spike: run the handshake with an extra slot-addressed read
    /// request injected INSIDE the burst (the only window the device answers
    /// data requests — standalone post-handshake requests get no reply, same as
    /// field-78). The reply (presetDataChanged 9 or exportPresetResponse 116) is
    /// then harvested from the accumulated streams by the caller.
    pub fn connect_with_burst_request(extra: &[u8]) -> Result<Session, String> {
        Self::connect_inner(false, Some(extra.to_vec()), false, false)
    }

    /// Trigger and capture the device's bulk backup archive (`BackupMessage`).
    /// Returns the raw archive bytes (a GNU-tar + LZ4-frame blob — caller decodes)
    /// plus [`BackupStats`]. This is the ONLY bulk file-egress in the protocol and
    /// the fast path to the full preset/scene library: one request streams the whole
    /// `/data` store, vs ~500 per-preset round-trips.
    ///
    /// Read-only on the device: the archive is built in tmpfs and streamed; nothing
    /// is persisted on flash (only the `SdBackupRequest` variant writes a file).
    ///
    /// We MUST heartbeat (~250 ms `ConnectionHeartbeat`) throughout: Pro Control's
    /// captured backup runs inside its dense heartbeat stream (the request is just
    /// slotted between heartbeats), and without it the session lapses during the
    /// device's multi-second archive build/stream and the device drops the backup
    /// (observed: a heartbeat-less request gets zero response, not even
    /// `BACKUP_STARTED`). Heartbeat replies are tiny single `0x35` frames that land
    /// between chunks (current=None), which the reassembler emits standalone without
    /// disturbing an open chunk stream.
    ///
    /// `on_progress` is called as the transfer advances (a `BackupProgress` per
    /// device state-change and per chunk) so a caller can drive a progress bar; the
    /// chunk percentage is exact because `numChunks` is known from the first chunk.
    pub fn device_backup<F: FnMut(BackupProgress)>(
        &mut self,
        max_secs: u64,
        mut on_progress: F,
    ) -> Result<(Vec<u8>, BackupStats), String> {
        use std::collections::BTreeMap;
        use std::time::Instant;

        let debug = std::env::var("TMP_BACKUP_DEBUG").is_ok();
        let mut debugged_first = false;
        let hexn = |b: &[u8], n: usize| -> String {
            b.iter()
                .take(n)
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Establish PC's live-controller cadence, THEN request inside it. PC sends
        // a ~250 ms ConnectionHeartbeat continuously; the backup rides that stream.
        self.raw.clear();
        let _ = self.hid.send(&proto::heartbeat());
        let start = Instant::now();
        self.send_and_collect(&proto::backup_request(), 120)?;
        let mut last_hb = Instant::now();

        // Inline reassembler (mirrors proto::reassemble_streams_final, but decodes
        // each completed stream as it closes so we never hold the whole frame set).
        let mut current: Option<Vec<u8>> = None;
        let mut chunks: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        let mut num_chunks: u32 = 0;
        let mut num_bytes: u32 = 0;
        let mut crc: u32 = 0;
        let mut last_state: i64 = -1;
        let mut state_log: Vec<(f64, i64)> = Vec::new();
        let mut build_size: u32 = 0;
        let mut build_ticks: u32 = 0;
        let mut first_chunk_secs: f64 = 0.0;
        let mut assembled_bytes: u64 = 0;
        let mut last_progress = Instant::now();
        let mut all_chunks_at: Option<Instant> = None;
        // First pass consumes the frames captured during the backup-send window;
        // subsequent passes pump fresh.
        let mut pending: Vec<Vec<u8>> = std::mem::take(&mut self.raw);

        loop {
            if start.elapsed().as_secs() >= max_secs {
                return Err(format!(
                    "device backup timed out after {max_secs}s: {}/{} chunks, last_state={last_state}",
                    chunks.len(),
                    num_chunks
                ));
            }
            // Keep the live-controller session alive (PC's ~250 ms cadence) or the
            // device aborts the backup mid-build/stream.
            if last_hb.elapsed().as_millis() >= 250 {
                let _ = self.hid.send(&proto::heartbeat());
                last_hb = Instant::now();
            }
            let reports = if pending.is_empty() {
                self.hid.pump(120)?
            } else {
                std::mem::take(&mut pending)
            };
            for data in &reports {
                if data.len() < 4 || data[0] != 0x00 {
                    continue;
                }
                let magic = data[1];
                let body_len = data[3] as usize;
                let end = (4 + body_len).min(data.len());
                let body = &data[4..end];
                // 0x33 start · 0x34 continue · 0x35 final (proto framing).
                let completed: Option<Vec<u8>> = match magic {
                    0x33 => {
                        let prev = current.take();
                        current = Some(body.to_vec());
                        prev
                    }
                    0x34 => {
                        match current {
                            Some(ref mut c) => c.extend_from_slice(body),
                            None => current = Some(body.to_vec()),
                        }
                        None
                    }
                    0x35 => match current.take() {
                        Some(mut c) => {
                            c.extend_from_slice(body);
                            Some(c)
                        }
                        None => Some(body.to_vec()),
                    },
                    _ => None,
                };
                let Some(stream) = completed else { continue };
                // TMS → backupMessage(8) → { backupRestoreData(2), backupRestoreState(3) }
                let top = proto::parse(&stream);
                let Some(bm) = proto::first_bytes(&top, 8) else {
                    continue;
                };
                if debug && !debugged_first {
                    debugged_first = true;
                    let fields: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
                    eprintln!(
                        "[backup-dbg] first stream: {}B, top fields {fields:?}, head {}",
                        stream.len(),
                        hexn(&stream, 24)
                    );
                    let bmp = proto::parse(bm);
                    if let Some(d) = proto::first_bytes(&bmp, 2) {
                        let dp = proto::parse(d);
                        let dfields: Vec<u32> = dp.iter().map(|(f, _)| *f).collect();
                        eprintln!(
                            "[backup-dbg]   backupRestoreData fields {dfields:?}; field6(chunkData) head {}",
                            proto::first_bytes(&dp, 6).map(|c| hexn(c, 24)).unwrap_or_else(|| "<none>".into())
                        );
                    }
                }
                let bm = proto::parse(bm);
                if let Some(st) = proto::first_bytes(&bm, 3) {
                    let stp = proto::parse(st);
                    if let Some(sz) = proto::first_varint(&stp, 2) {
                        build_size = build_size.max(sz as u32);
                    }
                    if let Some(tk) = proto::first_varint(&stp, 3) {
                        build_ticks = build_ticks.max(tk as u32);
                    }
                    if let Some(s) = proto::first_varint(&stp, 1) {
                        let s = s as i64;
                        if s != last_state {
                            state_log.push((start.elapsed().as_secs_f64(), s));
                            last_state = s;
                            // Pre-stream "building" progress (determinate iff the
                            // device populates build_size).
                            if chunks.is_empty() && matches!(s, 1 | 2) {
                                let pct = if build_size > 0 {
                                    (build_ticks as f32 / build_size as f32 * 100.0).min(100.0)
                                } else {
                                    0.0
                                };
                                on_progress(BackupProgress {
                                    phase: "building",
                                    received: 0,
                                    total: 0,
                                    bytes: 0,
                                    total_bytes: 0,
                                    percent: pct,
                                    build_size,
                                    build_ticks,
                                });
                            }
                        }
                    }
                }
                if let Some(d) = proto::first_bytes(&bm, 2) {
                    let d = proto::parse(d);
                    if let Some(v) = proto::first_varint(&d, 2) {
                        crc = v as u32;
                    }
                    if let Some(v) = proto::first_varint(&d, 3) {
                        num_bytes = v as u32;
                    }
                    if let Some(v) = proto::first_varint(&d, 4) {
                        num_chunks = v as u32;
                    }
                    let cn = proto::first_varint(&d, 5).map(|v| v as u32);
                    let cd = proto::first_bytes(&d, 6).map(|b| b.to_vec());
                    if let (Some(cn), Some(cd)) = (cn, cd) {
                        if chunks.is_empty() {
                            first_chunk_secs = start.elapsed().as_secs_f64();
                        }
                        // Incremental byte tally (avoids re-summing the whole map per
                        // chunk); only a NEW chunkNum advances it / resets the watchdog.
                        let cd_len = cd.len() as u64;
                        let is_new = !chunks.contains_key(&cn);
                        chunks.entry(cn).or_insert(cd); // keep-first on a dup chunkNum
                        if is_new {
                            assembled_bytes += cd_len;
                            last_progress = Instant::now();
                        }
                        let pct = if num_chunks > 0 {
                            (chunks.len() as f32 / num_chunks as f32 * 100.0).min(100.0)
                        } else {
                            0.0
                        };
                        on_progress(BackupProgress {
                            phase: "streaming",
                            received: chunks.len() as u32,
                            total: num_chunks,
                            bytes: assembled_bytes,
                            total_bytes: num_bytes as u64,
                            percent: pct,
                            build_size,
                            build_ticks,
                        });
                    }
                }
            }
            let all = num_chunks > 0 && chunks.len() as u32 >= num_chunks;
            let terminal = matches!(last_state, 4 | 10); // BACKUP_COMPLETE | BACKUP_SUCCESS
                                                         // Terminal state = clean finish (the device's BackupManager has reset);
                                                         // exit immediately so a later backup isn't ignored.
            if terminal && !chunks.is_empty() {
                break;
            }
            // All chunks in but no terminal state yet: keep draining briefly to let
            // BACKUP_COMPLETE land (a hard break here left the device non-idle and
            // the NEXT BackupRequest got no reply — HW-observed).
            if all {
                let waited = all_chunks_at.get_or_insert_with(Instant::now);
                if waited.elapsed().as_secs() >= 2 {
                    break;
                }
            }
            // Stall watchdog: stop if the stream goes silent after it began.
            if !chunks.is_empty() && last_progress.elapsed().as_secs() >= 5 {
                break;
            }
        }

        let mut blob = Vec::with_capacity(num_bytes as usize);
        for cd in chunks.values() {
            blob.extend_from_slice(cd);
        }
        let stats = BackupStats {
            elapsed_secs: start.elapsed().as_secs_f64(),
            num_bytes,
            num_chunks,
            chunks_received: chunks.len() as u32,
            crc,
            bytes_assembled: blob.len(),
            first_chunk_secs,
            state_log,
            build_size,
            build_ticks,
        };
        Ok((blob, stats))
    }

    /// Pump until the inbound stream goes quiet (`max_windows` windows of
    /// `window_ms` with no new reports, or stop growing). Used by the passive
    /// scene scan to drain the handshake flood before the first re-armed
    /// field-8 read — a read fired mid-flood is dropped device-side (the
    /// classic 0/25). NOTE: a batch-bearing `preset_list_request` is NOT
    /// answered on a minimal burst (HW-observed — the device only answers it
    /// inside the recognized full sequence), so the scan can't avoid the full
    /// handshake; it drains it instead.
    pub fn drain_until_quiet(&mut self, window_ms: u64, max_windows: u32) -> Result<(), String> {
        let mut last = self.raw.len();
        for _ in 0..max_windows {
            self.pump_collect(window_ms)?;
            if self.raw.len() == last {
                return Ok(());
            }
            last = self.raw.len();
        }
        Ok(())
    }

    /// NON-DESTRUCTIVE slot-addressed preset-JSON read: `presetDataRequest`
    /// (field 8) → `presetDataChanged` (field 9, PLAINTEXT partial). Sends NO
    /// LoadPreset — the unit's selected preset (and any unsaved edits on it)
    /// are never touched. `device_slot` is **1-based** (list index + 1).
    ///
    /// HW-proven on fw 1.8.45 (`probe --slotread-x` / `--scenes-passive`):
    /// the device answers exactly ONE data request per burst state, and a
    /// re-sent `connection_request` re-arms that state on the OPEN connection
    /// — so a whole-library sweep rides one connection (25/25, ~0.9 s/slot).
    /// The classic full-handshake placement was 0/25 ("ProductProfile
    /// collision"): the reply is dropped device-side when the read rides
    /// behind the ~480-frame preset-list/ProductProfile flood — fire reads
    /// only on a QUIET line ([`Self::drain_until_quiet`] after a full
    /// handshake, or a minimal burst).
    /// `Ok(None)` = the device didn't answer this read (caller counts a miss).
    ///
    /// For a DEDICATED/QUIET session (every `probe_*` sweep + `scan_preset_scenes`):
    /// the leading `connection_request` re-arms the burst state the device needs
    /// before it will answer a data request. On a LIVE-CONTROLLER session (the
    /// monitor's dense ~250 ms heartbeat) that state is ALREADY armed, so the
    /// re-arm is not just redundant — the device answers it with a `connectionError`
    /// on the next heartbeat (HW: 1 error/read + ~140 ms slower). Use
    /// [`Self::read_slot_preset_json_live`] there instead.
    pub fn read_slot_preset_json(&mut self, device_slot: u32) -> Result<Option<Vec<u8>>, String> {
        self.read_slot_preset_json_inner(device_slot, false)
    }

    /// Variant for the LIVE monitor session (startup graph fallback + metadata lane):
    /// the session is already armed by its dense heartbeat, so this SKIPS the
    /// `connection_request` re-arm (which would draw a per-read `connectionError`)
    /// and instead keeps that heartbeat alive through the harvest. HW-confirmed
    /// (`probe --slotread-live`): 0 `connectionError`/read vs 1 with the re-arm,
    /// and ~140 ms/read faster, reliable across repeated reads.
    pub fn read_slot_preset_json_live(
        &mut self,
        device_slot: u32,
    ) -> Result<Option<Vec<u8>>, String> {
        self.read_slot_preset_json_inner(device_slot, true)
    }

    fn read_slot_preset_json_inner(
        &mut self,
        device_slot: u32,
        on_live_session: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        self.raw.clear();
        // A quiet/dedicated session needs the `connection_request` re-arm before
        // the device answers a data request; a live-controller session is already
        // armed, so re-arming it only provokes a `connectionError` (HW-confirmed).
        if !on_live_session {
            self.send_and_collect(&proto::connection_request(), 100)?;
            self.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
        }
        self.send_and_collect(
            &proto::preset_data_request(1, device_slot as u64, None),
            200,
        )?;
        // Growth-stability harvest: pump in short slices until the field-9
        // payload stops growing for two slices (a 17 KB reply is ~290 frames,
        // ~1 s end-to-end; small presets land in the first slice). On a live
        // session, fire the live-controller heartbeat on a 250 ms elapsed gate
        // before each pump (the `device_backup` keepalive shape) so a long read
        // never starves the monitor's heartbeat.
        let mut last_hb = std::time::Instant::now();
        let (mut last, mut stable) = (0usize, 0u32);
        for _ in 0..24 {
            if on_live_session && last_hb.elapsed().as_millis() as u64 >= 250 {
                self.heartbeat()?;
                last_hb = std::time::Instant::now();
            }
            self.pump_collect(150)?;
            let len = self.try_preset_data_json().map(|b| b.len()).unwrap_or(0);
            if len > 0 && len == last {
                stable += 1;
                if stable >= 2 {
                    break;
                }
            } else {
                stable = 0;
            }
            last = len;
        }
        Ok(self.try_preset_data_json())
    }

    /// Slot-read investigation (`probe --slotread-x`): connect with the
    /// slot-addressed read injected at a chosen position in the burst. The
    /// classic AC1 spike appends it LAST — after the device has started
    /// streaming the three preset lists + the ~17 KB ProductProfile (fw
    /// 1.8.45), whose frames share the unkeyed `0x33/0x34/0x35` framing with
    /// the field-9 reply (the suspected "ProductProfile collision"). These
    /// variants shape the burst so the reply can't collide: `Early` fires the
    /// read before the flood requests, `Minimal` never sends the flood at all.
    /// Read-only — sends NO LoadPreset.
    pub fn connect_slotread(variant: SlotReadBurst, extra: &[u8]) -> Result<Session, String> {
        if matches!(variant, SlotReadBurst::Classic) {
            return Self::connect_with_burst_request(extra);
        }
        let hid = open_transport()?;
        let mut s = Session {
            hid,
            batch: 0,
            raw: Vec::new(),
            fw_version: None,
        };
        s.send_and_collect(&proto::connection_request(), 200)?;
        s.send_and_collect(&proto::preset_list_request(1, 1), 20)?; // My Presets
                                                                    // The read fires here — before favorites/factory/cloud/ProductProfile
                                                                    // can flood the unkeyed framing.
        s.send_and_collect(extra, 1500)?;
        if matches!(variant, SlotReadBurst::Early) {
            // Complete the standard burst so the device sees the full
            // load-bearing sequence (tests placement, not burst trimming).
            s.send_and_collect(&proto::favorite_list_request(2), 20)?;
            s.send_and_collect(&proto::preset_list_request(4, 2), 20)?;
            s.send_and_collect(&proto::preset_list_request(3, 2), 20)?;
            s.send_and_collect(&proto::product_profile_request(2), 20)?;
            s.send_and_collect(&proto::current_preset_info_request(2), 20)?;
            s.send_and_collect(&proto::settings_field66(2), 20)?;
            s.send_and_collect(&proto::userir_field2(2), 20)?;
            s.send_and_collect(&proto::current_preset_data_request(3), 300)?;
        }
        s.batch = 4;
        Ok(s)
    }

    /// Connect and request the firmware version inside the handshake burst
    /// (`currentFwRequest`, no batch — see [`proto::current_fw_request`]). The
    /// reply (`currentFwResponse`) is harvested into `fw_version`. Used by
    /// `probe --fw`; leveling uses the lean [`Session::connect`] (no fw
    /// request) since it doesn't need it.
    ///
    /// The request rides the batch-2 group, BEFORE `current_preset_data_request`
    /// — sending it after that batch-3 request makes the device drop the fw
    /// reply (HW-confirmed), so it can't reuse the generic
    /// `extra_burst` slot (which fires last).
    pub fn connect_with_firmware() -> Result<Session, String> {
        let mut s = Self::connect_inner(false, None, true, false)?;
        s.fw_version = s
            .streams()
            .iter()
            .find_map(|st| extract_fw_version(&st.body));
        Ok(s)
    }

    /// The active preset's 0-based My Presets list index, from the most recent
    /// `PresetLoaded` echo in this session's accumulated pushes (handshake +
    /// heartbeat). `None` if no echo has arrived yet.
    pub fn loaded_slot(&self) -> Option<u32> {
        self.push_bodies()
            .iter()
            .rev()
            .find_map(|body| extract_loaded_user_slot(body))
            .or_else(|| {
                self.streams()
                    .iter()
                    .rev()
                    .find_map(|st| extract_loaded_user_slot(&st.body))
            })
    }

    fn connect_inner(
        fetch_preset_json: bool,
        extra_burst: Option<Vec<u8>>,
        request_firmware: bool,
        lean: bool,
    ) -> Result<Session, String> {
        let hid = open_transport()?;
        let mut s = Session {
            hid,
            batch: 0,
            raw: Vec::new(),
            fw_version: None,
        };
        s.handshake(fetch_preset_json, extra_burst, request_firmware, lean)?;
        Ok(s)
    }

    /// Construct a `Session` over an arbitrary [`HidTransport`] WITHOUT the device
    /// handshake — the test seam that lets `sim_device::SimDevice` (an in-memory fake)
    /// stand in for real hardware, so the held-session edit/level orchestration is
    /// exercised end-to-end in `cargo test`. `batch` starts at 4 (the post-handshake
    /// value) to mirror a connected session; structural edits and setters carry no
    /// `batchStatus`, so the exact value is immaterial to the paths under test.
    #[cfg(any(test, feature = "e2e"))]
    pub(crate) fn from_transport(hid: Box<dyn HidTransport>) -> Session {
        Session {
            hid,
            batch: 4,
            raw: Vec::new(),
            fw_version: None,
        }
    }

    /// Firmware version the device reported during the handshake (e.g.
    /// "1.7.75"); only populated by [`Session::connect_with_firmware`]. None if
    /// the burst didn't carry a `currentFwResponse`.
    pub fn firmware_version(&self) -> Option<String> {
        self.fw_version.clone()
    }

    fn next_batch(&mut self) -> u64 {
        self.batch += 1;
        self.batch
    }

    /// Full first-connect handshake, replicating the device's captured 1.7.2
    /// sequence. The device only answers once it
    /// has seen this sequence. Crucially we ACCUMULATE every report into
    /// `self.raw` (never discard): the `PresetListResponse` for My Presets
    /// arrives early here, and `list_my_presets` reads it from the accumulator.
    /// Accumulating from the first send also keeps multi-packet framing intact
    /// (every `0x33` stream-start is captured).
    fn handshake(
        &mut self,
        fetch_preset_json: bool,
        extra_burst: Option<Vec<u8>>,
        request_firmware: bool,
        lean: bool,
    ) -> Result<(), String> {
        // Replicate Pro Control's first-connect sequence, INCLUDING its exact
        // batchStatus values. The device stops answering partway through if the
        // host increments the batch on every request (observed live: it went
        // silent after the Factory list, so only the preset lists ever arrived).
        // Pro Control groups the post-connect requests under batch=2, with the
        // current-preset data/json requests at 3/4. Mirroring that grouping is
        // what makes the device stream the full handshake (incl. the preset JSON).
        //
        // The request SEQUENCE is identical in both window sets — `lean` only trims
        // how long the host pumps for replies it will never read (see
        // `connect_lean`). TMP_HANDSHAKE_SCALE is a diagnostic env override for
        // probe bisects on top of either set.
        let base: [u64; 3] = if lean { [50, 5, 75] } else { [200, 20, 300] };
        let hs = |ms: u64| -> u64 {
            std::env::var("TMP_HANDSHAKE_SCALE")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(|s| ((ms as f64) * s).round() as u64)
                .unwrap_or(ms)
        };
        self.send_and_collect(&proto::connection_request(), hs(base[0]))?;
        self.send_and_collect(&proto::preset_list_request(1, 1), hs(base[1]))?; // My Presets
        self.send_and_collect(&proto::favorite_list_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::preset_list_request(4, 2), hs(base[1]))?; // Factory
        self.send_and_collect(&proto::preset_list_request(3, 2), hs(base[1]))?; // Cloud
        self.send_and_collect(&proto::product_profile_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::current_preset_info_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::settings_field66(2), hs(base[1]))?;
        self.send_and_collect(&proto::userir_field2(2), hs(base[1]))?;
        // Firmware read rides the batch-2 group (no batchStatus), BEFORE the
        // batch-3 current_preset_data_request — sending it after that makes the
        // device drop the `currentFwResponse` reply (HW-confirmed).
        // The reply is a tiny single 0x35 frame, so a short pump suffices.
        if request_firmware {
            self.send_and_collect(&proto::current_fw_request(), 200)?;
        }
        self.send_and_collect(&proto::current_preset_data_request(3), hs(base[2]))?;
        // The field-78 json request right after field-2 is what makes the device
        // emit the current preset's `currentPresetDataChanged` (field 3) JSON —
        // but it streams a multi-packet blob (~2 s), so only fetch it when the
        // caller needs it (discovery), not on every leveling connect.
        if fetch_preset_json {
            self.send_and_collect(&proto::current_preset_data_json_request(4), 1800)?;
        }
        // AC1 spike: inject a slot-addressed read inside the burst window. The
        // device only answers data requests while actively streaming this burst
        // (a drain drops it to the silent standalone state), so we send WITHOUT
        // draining and tolerate concurrent handshake streams in reassembly.
        if let Some(extra) = extra_burst {
            self.send_and_collect(&extra, 2000)?;
        }
        // Continue the batch counter past the handshake's fixed values.
        self.batch = 4;
        Ok(())
    }

    pub fn heartbeat(&mut self) -> Result<(), String> {
        self.hid.send(&proto::heartbeat())
    }

    /// Send a request and accumulate the raw reports received during `ms`.
    pub(crate) fn send_and_collect(&mut self, body: &[u8], ms: u64) -> Result<(), String> {
        let reports = self.hid.transact(body, ms)?;
        self.raw.extend(reports);
        Ok(())
    }

    /// Pump (no send) and accumulate the raw reports received during `ms`.
    pub(crate) fn pump_collect(&mut self, ms: u64) -> Result<(), String> {
        let reports = self.hid.pump(ms)?;
        self.raw.extend(reports);
        Ok(())
    }

    /// Public pump-and-accumulate for the in-burst harvest probes
    /// (`connect_with_burst_request` → pump → `harvest_*`). Lets a caller keep
    /// draining the burst window until a reply lands without exposing `raw`.
    pub fn pump_more(&mut self, ms: u64) -> Result<(), String> {
        self.pump_collect(ms)
    }

    /// Diagnostic: send `body`, collect the reply, and return a dump of the reply
    /// streams (count + per-stream top-level field numbers, with songMessage[11] /
    /// setlistMessage[12] / presetMessage[2] expanded to their inner field numbers).
    /// Used to tell a silent-ignore (no reply) from an error reply.
    pub fn send_and_dump(&mut self, body: &[u8], ms: u64) -> Result<String, String> {
        self.raw.clear();
        self.send_and_collect(body, ms)?;
        let streams = self.streams();
        let mut out = format!("  reply streams: {}\n", streams.len());
        for (i, s) in streams.iter().enumerate() {
            let top = proto::parse(&s.body);
            let mut desc = Vec::new();
            for (f, _) in &top {
                if matches!(*f, 2 | 11 | 12) {
                    if let Some(b) = proto::first_bytes(&top, *f) {
                        let inner: Vec<u32> = proto::parse(b).iter().map(|(g, _)| *g).collect();
                        desc.push(format!("{f}{inner:?}"));
                        continue;
                    }
                }
                desc.push(f.to_string());
            }
            out += &format!("    [{i}] {}B fields={}\n", s.body.len(), desc.join(","));
        }
        Ok(out)
    }

    /// Push-listener experiment (probe --listen): park on the open post-handshake
    /// connection and print every inbound message stream as it completes, with a
    /// first-seen timestamp — the discovery tool for the unit's unsolicited pushes
    /// (PresetLoaded 2[11], SceneLoaded 2[102], CurrentPresetInfoChanged 2[22], …)
    /// fired by footswitch taps / scene recalls / preset changes ON THE UNIT.
    /// `hb_ms` > 0 sends a ConnectionHeartbeat every `hb_ms` MILLISECONDS (set 0 to
    /// test whether pushes flow without one). Pro Control holds the live-controller
    /// session with a dense ~250 ms (4/sec) heartbeat; our earlier 10 000 ms cadence
    /// let the session lapse → the device answered everything with `connectionError`.
    /// This is the keepalive-cadence test: at ~250 ms do the errors stop and the
    /// unit's footswitch/scene pushes (SceneLoaded 102 / PresetLoaded 11 / …) start
    /// arriving? `poll_secs` > 0 also re-sends the current-preset requests each that
    /// many seconds. Read-only apart from the heartbeat + poll requests.
    pub fn listen_dump(&mut self, seconds: u64, hb_ms: u64, poll_secs: u64) -> Result<(), String> {
        let start = std::time::Instant::now();
        let mut last_hb = std::time::Instant::now();
        let mut last_poll = std::time::Instant::now();
        let mut poll_batch = 100u64; // distinct from the handshake's 1..4 grouping
                                     // Pump in short windows so a sub-second heartbeat can actually fire on time
                                     // (a 700 ms window would cap the cadence at ~1.4/sec). 150 ms lets ~250 ms
                                     // heartbeats land close to PC's 4/sec.
        let pump_ms = if hb_ms > 0 { hb_ms.clamp(40, 150) } else { 200 };
        let mut first_seen: Vec<f32> = Vec::new();
        let mut printed = 0usize;
        println!(
            "[listen] handshake done, parked for {seconds}s (heartbeat: {}; poll: {}). Drive the unit NOW: tap footswitches, recall scenes, change presets.",
            if hb_ms > 0 { format!("every {hb_ms}ms") } else { "OFF".to_string() },
            if poll_secs > 0 { format!("every {poll_secs}s") } else { "OFF".to_string() }
        );
        while start.elapsed().as_secs() < seconds {
            self.pump_collect(pump_ms)?;
            let streams = self.streams_final();
            for _ in first_seen.len()..streams.len() {
                first_seen.push(start.elapsed().as_secs_f32());
            }
            // Print every stream except the newest (it may still be growing).
            while printed + 1 < streams.len() {
                println!(
                    "[t+{:>7.1}s] {}",
                    first_seen[printed],
                    describe_push(&streams[printed].body)
                );
                printed += 1;
            }
            if hb_ms > 0 && last_hb.elapsed().as_millis() as u64 >= hb_ms {
                self.heartbeat()?;
                last_hb = std::time::Instant::now();
            }
            if poll_secs > 0 && last_poll.elapsed().as_secs() >= poll_secs {
                // Poll the current-preset state on the OPEN connection. If the device
                // answers these standalone (the in-handshake-only requests do NOT —
                // known gotcha), the reply streams reflect the live scene.
                self.hid
                    .send(&proto::current_preset_info_request(poll_batch))?;
                self.hid
                    .send(&proto::current_preset_data_request(poll_batch))?;
                println!(
                    "[t+{:>7.1}s] -> poll sent (currentPresetInfo + currentPresetData, batch={poll_batch})",
                    start.elapsed().as_secs_f32()
                );
                poll_batch += 1;
                last_poll = std::time::Instant::now();
            }
        }
        // Flush the tail.
        let streams = self.streams_final();
        while printed < streams.len() {
            let t = first_seen
                .get(printed)
                .copied()
                .unwrap_or_else(|| start.elapsed().as_secs_f32());
            println!("[t+{:>7.1}s] {}", t, describe_push(&streams[printed].body));
            printed += 1;
        }
        println!("[listen] done: {printed} streams in {seconds}s");
        Ok(())
    }

    /// Drop the accumulated inbound buffer (so a following `push_bodies` reflects
    /// only what arrives next). Public for diagnostics that isolate one exchange.
    pub fn clear_raw(&mut self) {
        self.raw.clear();
    }

    /// Reassemble everything accumulated so far into message streams.
    fn streams(&self) -> Vec<proto::Stream> {
        proto::reassemble_streams(&self.raw)
    }

    /// Reassemble using the 0x35-is-final rule (for complete list responses).
    fn streams_final(&self) -> Vec<proto::Stream> {
        proto::reassemble_streams_final(&self.raw)
    }

    /// The reassembled inbound message bodies accumulated so far, using the
    /// 0x35-is-final rule (same as [`Self::streams_final`], the `listen_dump`
    /// reassembly). Public so the device monitor can drain pushes off the open
    /// connection without `raw`/`streams_final` being module-private to it. The
    /// monitor tracks a "seen" index (the newest stream may still be growing).
    pub fn push_bodies(&self) -> Vec<Vec<u8>> {
        self.streams_final().into_iter().map(|s| s.body).collect()
    }

    /// Request the active preset's scene list on demand — `sceneListRequest`
    /// (field 126, no batch; addresses the CURRENT preset like `loadScene`).
    /// Harvests the `sceneListResponse` (125) `sceneList` strings. The device
    /// also pushes this UNSOLICITED on every preset load (the monitor's primary
    /// path); this is a manual / first-paint top-up for a mid-preset connect.
    pub fn request_scene_list(&mut self) -> Result<Vec<String>, String> {
        self.raw.clear();
        self.send_and_collect(&proto::scene_list_request(), 400)?;
        for _ in 0..6 {
            if let Some(names) = self.push_bodies().iter().find_map(|b| decode_scene_list(b)) {
                return Ok(names);
            }
            self.pump_collect(300)?;
        }
        self.push_bodies()
            .iter()
            .find_map(|b| decode_scene_list(b))
            .ok_or_else(|| "no sceneListResponse received from device".to_string())
    }

    /// Diagnostic: the magic/len sequence of the raw inbound reports collected so far
    /// (`<magic>/<body_len>` per frame). Reveals whether a multi-packet response ends
    /// in a 0x35 frame and whether foreign 0x33 streams are interleaved.
    pub fn raw_frame_summary(&self) -> String {
        self.raw
            .iter()
            .filter(|d| d.len() >= 4 && d[0] == 0x00)
            .map(|d| format!("{:02x}/{}", d[1], d[3]))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Enumerate the "My Presets" list (listEnum = 1). The response can be
    /// large and multi-packet, so we accumulate raw reports across pump windows
    /// and reassemble cumulatively until a `PresetListResponse` appears.
    pub fn list_my_presets(&mut self) -> Result<Vec<PresetEntry>, String> {
        // The handshake already issued preset_list_request(1) and accumulated
        // its reply; check that first. Only re-query if it isn't there yet.
        let mut names = self.best_preset_list();
        if names.is_none() {
            let b = self.next_batch();
            self.send_and_collect(&proto::preset_list_request(1, b), 1000)?;
        }
        for _ in 0..8 {
            if names.is_some() {
                break;
            }
            if let Some(found) = self.best_preset_list() {
                names = Some(found);
                break;
            }
            self.pump_collect(700)?;
        }
        let names =
            names.ok_or_else(|| "no PresetListResponse received from device".to_string())?;
        Ok(preset_entries(names))
    }

    /// Enumerate the FACTORY list (listEnum = 4). Mirrors [`list_my_presets`]: the
    /// handshake already issued `preset_list_request(4)` and accumulated its reply,
    /// so harvest from the shared accumulator first and only re-query if absent.
    pub fn list_factory_presets(&mut self) -> Result<Vec<PresetEntry>, String> {
        let mut names = best_factory_list_from_reports(&self.raw);
        if names.is_none() {
            let b = self.next_batch();
            self.send_and_collect(&proto::preset_list_request(4, b), 1000)?;
            for _ in 0..8 {
                if let Some(found) = best_factory_list_from_reports(&self.raw) {
                    names = Some(found);
                    break;
                }
                self.pump_collect(700)?;
            }
        }
        let names = names
            .ok_or_else(|| "no Factory PresetListResponse received from device".to_string())?;
        Ok(preset_entries(names))
    }

    /// Completeness-validated My-Presets list for the snapshot path. The tolerant
    /// `list_my_presets` harvest accepts a tail-truncated multi-packet response
    /// (observed: 371 of 504 records when the monitor reconnected right after a
    /// heavy field-8 sweep congested the device). This variant:
    /// 1. tries the strict harvest on the already-accumulated handshake reports;
    /// 2. retries by RE-ARMING the open session (the `read_slot_preset_json`
    ///    recipe: quiet line → `connection_request` → `preset_list_request`) —
    ///    WITHOUT clearing `raw`, because `assemble_startup_snapshot` harvests the
    ///    startup graph from these same reports after the list read; appending is
    ///    safe since strict rejects the old truncated streams and longest-complete
    ///    wins picks the fresh full response;
    /// 3. falls back to the tolerant longest-wins list with a diagnostic warning —
    ///    a warned short list beats a failed connect (truncation is tail-only, so
    ///    present entries keep correct slots, and the next monitor reconnect
    ///    re-reads the list).
    ///
    /// NOT for the leveller/probe/clear call sites — they shape their own bursts
    /// and a re-arm would clobber their accumulator timing; they stay on the
    /// tolerant `list_my_presets`.
    pub fn list_my_presets_strict(&mut self) -> Result<Vec<PresetEntry>, String> {
        let mut names = self.harvest_preset_list_strict();
        for _attempt in 0..2 {
            if names.is_some() {
                break;
            }
            self.drain_until_quiet(250, 20)?;
            self.send_and_collect(&proto::connection_request(), 100)?;
            self.send_and_collect(&proto::preset_list_request(1, 1), 200)?;
            for _ in 0..4 {
                if let Some(found) = self.harvest_preset_list_strict() {
                    names = Some(found);
                    break;
                }
                self.pump_collect(250)?;
            }
        }
        let names = match names {
            Some(n) => n,
            None => {
                let tolerant = self
                    .best_preset_list()
                    .ok_or_else(|| "no PresetListResponse received from device".to_string())?;
                log::warn!(
                    "list_my_presets_strict: no complete decode after retries; serving the \
                     tolerant longest-wins list ({} records — tail may be truncated)",
                    tolerant.len()
                );
                tolerant
            }
        };
        Ok(preset_entries(names))
    }

    /// Best My-Presets list decoded from all reports accumulated so far. Tries BOTH
    /// reassembly rules:
    /// - `streams()` keeps an interleaved 0x35 as its own stream, preserving list reads
    ///   that arrive mid-flood.
    /// - `streams_final()` folds a terminal 0x35 into the open stream, preserving the
    ///   final record tail ("Empty" was observed as "Empt" when this frame was dropped).
    ///
    /// Longest decoded list wins; complete list responses should have equal record
    /// counts except for the final-frame tail case.
    fn best_preset_list(&self) -> Option<Vec<String>> {
        best_preset_list_from_reports(&self.raw)
    }

    /// Fetch the current preset's JSON (`currentPresetDataJsonRequest` →
    /// `currentPresetDataJsonResponse.presetJson`, LZ4-block compressed).
    pub fn fetch_current_preset_json(&mut self) -> Result<String, String> {
        self.raw.clear();
        let b = self.next_batch();
        self.send_and_collect(&proto::current_preset_data_json_request(b), 1200)?;
        for _ in 0..8 {
            if let Some(j) = self.try_preset_json() {
                return Ok(j);
            }
            self.pump_collect(700)?;
        }
        self.try_preset_json()
            .ok_or_else(|| "no preset JSON received from device".to_string())
    }

    fn try_preset_json(&self) -> Option<String> {
        self.streams().iter().find_map(|s| {
            // presetMessage(2) → currentPresetDataJsonResponse(79) → presetJson(1)
            let inner = dig(&s.body, TMS_PRESET, 79)?;
            let payload = field1(&inner).and_then(|v| v.as_bytes())?;
            let raw = proto::lz4_block_decompress(payload).ok()?;
            String::from_utf8(raw).ok()
        })
    }

    /// The largest reassembled `presetJson` payload found in the accumulated
    /// streams, across all three carriers (presetMessage submessage field →
    /// presetJson inner field): currentPresetDataChanged 3→1,
    /// currentPresetDataJsonResponse 79→1, and presetDataChanged 9→3 (the
    /// slot-addressed reply). Largest wins so a complete stream beats a stray.
    fn best_json_payload(&self) -> Vec<u8> {
        const CARRIERS: [(u32, u32); 3] = [(3, 1), (79, 1), (9, 3)];
        self.streams()
            .iter()
            .filter_map(|s| {
                CARRIERS.iter().find_map(|&(pm_field, json_field)| {
                    let inner = dig(&s.body, TMS_PRESET, pm_field)?;
                    inner
                        .iter()
                        .find(|(f, _)| *f == json_field)
                        .and_then(|(_, v)| v.as_bytes())
                        .map(|b| b.to_vec())
                })
            })
            .max_by_key(|b| b.len())
            .unwrap_or_default()
    }

    /// HW / Tier-4 diagnostic: capture the FULL `currentPresetDataChanged` (field
    /// 3) preset JSON, decompressed byte-exact (not the lossy-UTF-8 prefix). Holds
    /// a dense ~250 ms ConnectionHeartbeat while pumping so the device treats us as
    /// a live controller and pushes the COMPLETE field-3 (the lean path truncates
    /// at `"scenes"`; a healthy dense-heartbeat session was observed to deliver the
    /// full ~16 KB preset). `slot` (Some) loads that preset first to trigger a fresh
    /// push (non-destructive). Returns the decompressed bytes — which may still be a
    /// device-truncated partial; the caller checks completeness.
    pub fn capture_full_preset_json(
        &mut self,
        slot: Option<u32>,
        settle_ms: u64,
    ) -> Result<Vec<u8>, String> {
        if let Some(idx) = slot {
            self.load_preset(idx)?;
        }
        let steps = (settle_ms / 250).max(1);
        for _ in 0..steps {
            self.heartbeat()?;
            self.pump_more(250)?;
        }
        let payload = self.best_json_payload();
        if payload.is_empty() {
            return Err(
                "no currentPresetDataChanged (field 3) payload captured — is a preset loaded?"
                    .to_string(),
            );
        }
        proto::lz4_block_decompress(&payload).map_err(|e| format!("LZ4 decompress failed: {e}"))
    }

    /// The current preset's tolerant-parsed JSON document, from the data the
    /// handshake fetched (`currentPresetDataChanged`, field 3 — LZ4, routinely
    /// truncated mid-object; `audioGraph` and usually `scenes` survive). The shared
    /// read under [`current_preset_blocks`] / the per-scene amp pick.
    pub fn current_preset_value(&self) -> Result<serde_json::Value, String> {
        let payload = self.best_json_payload();
        let text = decode_preset_json(&payload).ok_or_else(|| {
            "no preset JSON in handshake — currentPresetDataChanged absent (is a preset loaded?)"
                .to_string()
        })?;
        tolerant_parse_json(&text)
            .ok_or_else(|| "could not parse preset JSON, even tolerantly".to_string())
    }

    /// The current preset's level-type block controls, parsed from the data the
    /// handshake fetched. To enumerate a SPECIFIC slot, load it then open a fresh
    /// `Session` (the handshake fetches whatever preset is current, and a loaded
    /// preset stays current across reconnects).
    pub fn current_preset_blocks(&self) -> Result<Vec<LevelBlock>, String> {
        Ok(extract_level_blocks(&self.current_preset_value()?))
    }

    /// The active preset's full signal-chain graph (active-preset signal chain block strip) — every
    /// block per group plus routing template and split/mix controls, parsed from the same
    /// live field-3 partial as [`current_preset_blocks`]. Unlike that level-only
    /// reader, this surfaces every node + its model id + bypass + the routing
    /// fields the strip needs to draw series vs parallel. `name`/`slot` come from
    /// `info` when present (usually absent in the truncated partial → `None`).
    pub fn current_audio_graph(&self) -> Result<ActiveGraph, String> {
        let payload = self.best_json_payload();
        let text = decode_preset_json(&payload).ok_or_else(|| {
            "no preset JSON in handshake — currentPresetDataChanged absent (is a preset loaded?)"
                .to_string()
        })?;
        let v = tolerant_parse_json(&text)
            .ok_or_else(|| "could not parse preset JSON, even tolerantly".to_string())?;
        let template_hint = extract_partial_json_string(&text, "template");
        let mut graph = extract_active_graph(&v, template_hint.as_deref());
        if !graph.nodes.is_empty() && !is_known_routing_template(graph.template.as_deref()) {
            return Err(format!(
                "active graph truncated before a complete audioGraph.template: {:?}",
                graph.template
            ));
        }
        if graph.name.is_none() {
            graph.name = self
                .streams()
                .iter()
                .rev()
                .find_map(|s| extract_current_preset_display_name(&s.body));
        }
        if graph.slot.is_none() {
            graph.slot = self
                .streams()
                .iter()
                .rev()
                .find_map(|s| extract_loaded_user_slot(&s.body));
        }
        Ok(graph)
    }

    /// Resolve the active graph to a 0-based My Presets list index when the
    /// truncated live JSON and handshake events did not carry a slot. The
    /// CurrentPresetInfoChanged display name is still available, so a unique
    /// name match is addressable. Duplicate names deliberately stay unresolved.
    pub fn resolve_unique_my_preset_slot(&mut self, name: Option<&str>) -> Option<u32> {
        let name = name?;
        let presets = self.list_my_presets().ok()?;
        unique_preset_slot_by_name(&presets, name)
    }

    /// Debug aid: summarize every reassembled stream as its TMS top-level field
    /// numbers (and, for the presetMessage carrier, its inner field numbers), so
    /// a missing/redirected reply can be diagnosed without a capture.
    #[allow(dead_code)]
    fn stream_field_summary(&self) -> String {
        let streams = self.streams();
        if streams.is_empty() {
            return "(none)".to_string();
        }
        streams
            .iter()
            .map(|s| {
                let top = proto::parse(&s.body);
                let top_fields: Vec<String> = top
                    .iter()
                    .map(|(f, _)| {
                        // Expand presetMessage(2)/settingsMessage(3) into inner field numbers.
                        if *f == TMS_PRESET || *f == TMS_SETTINGS {
                            if let Some(b) = proto::first_bytes(&top, *f) {
                                let inner: Vec<u32> =
                                    proto::parse(b).iter().map(|(g, _)| *g).collect();
                                return format!("{f}{inner:?}");
                            }
                        }
                        f.to_string()
                    })
                    .collect();
                format!("[{}B:{}]", s.body.len(), top_fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn try_export_json(&self) -> Option<Vec<u8>> {
        // Largest wins: a complete multi-packet stream beats a truncated/early one.
        self.streams()
            .iter()
            .filter_map(|s| proto::export_response_preset_json(&s.body))
            .max_by_key(|b| b.len())
    }

    /// Diagnostic: sizes of every reassembled presetDataChanged(9) and
    /// exportPresetResponse(116) presetJson payload, plus the raw report count.
    /// Distinguishes a device-side truncation cap from a host reassembly split.
    pub fn slot_read_diagnostics(&self) -> String {
        let streams = self.streams();
        let mut f9 = Vec::new();
        let mut f116 = Vec::new();
        for s in &streams {
            if let Some(b) = proto::preset_data_changed_json(&s.body) {
                f9.push(b.len());
            }
            if let Some(b) = proto::export_response_preset_json(&s.body) {
                f116.push(b.len());
            }
        }
        format!(
            "raw_reports={} streams={} field9_payloads={f9:?} field116_payloads={f116:?}",
            self.raw.len(),
            streams.len(),
        )
    }

    /// Diagnostic for active-graph discovery: report every current-preset JSON
    /// carrier separately. A larger truncated compressed field-3 payload can be
    /// less useful than a smaller raw field-79 payload, so size alone is not
    /// enough to understand a missing active-preset signal chain graph.
    pub fn active_graph_diagnostics(&self) -> String {
        const CARRIERS: [(u32, u32); 3] = [(3, 1), (79, 1), (9, 3)];
        let streams = self.streams();
        let mut found = Vec::new();
        for s in &streams {
            for &(pm_field, json_field) in &CARRIERS {
                let Some(inner) = dig(&s.body, TMS_PRESET, pm_field) else {
                    continue;
                };
                let Some(payload) = inner
                    .iter()
                    .find(|(f, _)| *f == json_field)
                    .and_then(|(_, v)| v.as_bytes())
                else {
                    continue;
                };
                let decompressed = proto::lz4_block_decompress(payload).ok();
                let decoded = decompressed
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_else(|| String::from_utf8_lossy(payload));
                let prefix: String = decoded.chars().take(120).collect();
                let suffix: String = decoded
                    .chars()
                    .rev()
                    .take(500)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                found.push(format!(
                    "field{pm_field}->{json_field}: payload={}B lz4={} decoded={}B prefix={prefix:?} suffix={suffix:?}",
                    payload.len(),
                    decompressed.as_ref().map_or_else(|| "no".to_string(), |b| format!("{}B", b.len())),
                    decoded.len(),
                ));
            }
        }
        format!(
            "streams={} summary={} carriers=[{}]",
            streams.len(),
            self.stream_field_summary(),
            found.join("; "),
        )
    }

    pub(crate) fn try_preset_data_json(&self) -> Option<Vec<u8>> {
        // Largest wins: a complete multi-packet stream beats a truncated/early one.
        // BOTH reassembly rules are tried: `streams()` keeps an interleaved 0x35
        // separate (safe mid-flood) but DROPS the final frame of a complete
        // stream; `streams_final()` folds the trailing 0x35 in. On the passive
        // scene scan the field-9 stream ends in a 0x35 final frame — the
        // `streams()`-only harvest systematically lost the document tail
        // (`"scenes"` lives at the end; HW: 22/25 presets read "scenes unknown"
        // until the final-rule recovery).
        self.streams()
            .iter()
            .map(|s| s.body.clone())
            .chain(self.push_bodies())
            .filter_map(|b| proto::preset_data_changed_json(&b))
            .max_by_key(|b| b.len())
    }

    /// Pump until a slot-read reply stops growing, then return the largest
    /// presetDataChanged (field 9 → presetJson 3) else exportPresetResponse
    /// (field 116 → presetJson 1) payload. For the in-burst AC1 spike
    /// (`connect_with_burst_request`). Keeps pumping while the payload grows so a
    /// large multi-packet preset is fully reassembled before we read it.
    pub fn harvest_slot_read(&mut self) -> Result<Vec<u8>, String> {
        let best = |s: &Self| s.try_preset_data_json().or_else(|| s.try_export_json());
        // Pump until the payload stops growing for two windows. 20×500 ms covers
        // the largest preset observed (~17 KB / ~290 packets); a patient 60-iter
        // run confirmed the device stops at the same byte count, so this is the
        // device's full output, not a harvest-timing cut.
        let mut last_len = 0usize;
        let mut stable = 0u32;
        for _ in 0..20 {
            self.pump_collect(500)?;
            let len = best(self).map(|b| b.len()).unwrap_or(0);
            if len > 0 && len == last_len {
                stable += 1;
                if stable >= 2 {
                    break; // grew then settled — stream complete
                }
            } else {
                stable = 0;
            }
            last_len = len;
        }
        best(self).ok_or_else(|| {
            format!(
                "no slot-read reply (field 9/116). Streams seen: {}",
                self.stream_field_summary()
            )
        })
    }

    /// LoadPreset on the My Presets tab (tabEnum = 1). `slot` is the **0-based
    /// list index** (`list_my_presets` position); the device addresses presets by
    /// a **1-based** userSlot, so we send `slot + 1`. HW-confirmed on 1.7.75: the
    /// Song read reports `userPresetSlot=2` for the preset at list index 1, and a
    /// `load_preset(28)` loads list index 27 — i.e. device userSlot = list + 1.
    pub fn load_preset(&mut self, slot: u32) -> Result<(), String> {
        // Eager pump: the reports are discarded (fire-and-forget), so exit as
        // soon as the echo burst completes instead of burning the full window.
        self.hid
            .transact_eager(&proto::load_preset((slot + 1) as u64, 1), 300)?;
        Ok(())
    }

    /// Raw loadPreset with BOTH `preset_slot` and `tab_enum` passed VERBATIM (no
    /// +1, no hardcoded tab) — the `probe --load-probe` experiment for the factory
    /// bank. Same fire-and-forget `transact_eager` path as `load_preset` (which is
    /// HW-proven to change the active preset).
    pub fn load_preset_raw(&mut self, preset_slot: u64, tab_enum: u64) -> Result<(), String> {
        self.hid
            .transact_eager(&proto::load_preset(preset_slot, tab_enum), 300)?;
        Ok(())
    }

    /// Set re-amp mode via the Global Settings path. Returns the echoed
    /// `SettingsMessage.reampModeActive` value if the device reports it.
    pub fn set_reamp_mode(&mut self, active: bool) -> Result<Option<bool>, String> {
        self.raw.clear();
        self.send_and_collect(&proto::set_reamp_mode(active), 400)?;
        Ok(self.streams().iter().find_map(|s| {
            dig(&s.body, TMS_SETTINGS, REAMP_SETTING)
                .and_then(|f| field1(&f).and_then(|v| v.as_u64()))
                .map(|v| v != 0)
        }))
    }

    /// Set the whole-preset level (0.0..=1.0). Returns the echoed
    /// `PresetLevelChanged.presetLevel` if reported (the device's confirmation).
    pub fn set_preset_level(&mut self, level: f32) -> Result<Option<f32>, String> {
        self.raw.clear();
        self.send_and_collect(&proto::set_preset_level(level), 250)?;
        Ok(self.streams().iter().find_map(|s| {
            dig(&s.body, TMS_PRESET, PRESET_LEVEL_CHANGED)
                .and_then(|f| field1(&f).and_then(|v| v.as_f32()))
        }))
    }

    /// Set one block control via `ChangeParameter` (fire-and-forget setter, no
    /// reply — like `load_preset`). `group_id`/`node_id`/`parameter_id` name the
    /// `dspUnitParameters` entry (see `current_preset_blocks`); `value` is in the
    /// parameter's own units (often 0..1, but e.g. an IR `outputlevel` is in dB).
    /// HW-proven to move loudness; needs no SetNodeSceneEdit and is latched by
    /// re-amp engage, so set it BEFORE engaging (same rule as `set_preset_level`).
    pub fn change_parameter(
        &mut self,
        group_id: &str,
        node_id: &str,
        parameter_id: &str,
        value: f32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::change_parameter(group_id, node_id, parameter_id, value),
            250,
        )?;
        Ok(())
    }

    /// Set one block BOOL control (e.g. `bypass`) via `ChangeParameter.boolVal`
    /// (field 7, always emitted). Same fire-and-forget setter rule as the float
    /// [`change_parameter`]. Used to force a block active/bypassed while measuring
    /// an off-in-base footswitch block for the bake path.
    pub fn change_parameter_bool(
        &mut self,
        group_id: &str,
        node_id: &str,
        parameter_id: &str,
        value: bool,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::change_parameter_bool(group_id, node_id, parameter_id, value),
            250,
        )?;
        Ok(())
    }

    /// Establish/refresh **live-controller status** with a dense ~200 ms heartbeat.
    /// fw 1.8.45 only accepts structural edits (`replaceNode`/`replaceNodeWithBlock`)
    /// from a live controller; a lapsed session (e.g. after a silent
    /// `drain_until_quiet`) gets EVERY structural request answered with an empty
    /// `connectionError` and silently dropped. Call this AFTER the load and BEFORE
    /// the first edit — and keep editing without long quiet gaps. (HW-confirmed via
    /// a Pro Control USB capture; same reason `device_backup` heartbeats.)
    pub fn begin_live_edit(&mut self) -> Result<(), String> {
        // Warmup count is env-tunable for the perf experiments (E2); 8×200 ms = 1.6 s
        // is the validated default. The held-session architecture pays this ONCE, so
        // it is amortized across a whole bulk run.
        let n = std::env::var("TMP_WARMUP_N")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(8)
            .clamp(1, 32);
        for _ in 0..n {
            self.heartbeat()?;
            self.pump_collect(200)?;
        }
        Ok(())
    }

    /// Echo-gated wait (E3): pump (heartbeat) until `active_preset_name()` equals
    /// `name`, or the iteration budget runs out — exits the moment the device's
    /// `currentPresetInfoChanged` (field 22) for the just-loaded preset lands instead
    /// of burning a fixed window. Returns whether it matched. Used by the held-session
    /// edit to confirm the load re-attached the edit context.
    pub fn await_active_preset(&mut self, name: &str, max_iters: u32) -> bool {
        for _ in 0..max_iters {
            if self.active_preset_name().as_deref() == Some(name) {
                return true;
            }
            let _ = self.heartbeat();
            let _ = self.pump_collect(150);
        }
        self.active_preset_name().as_deref() == Some(name)
    }

    /// Send a body via the chunked (`0x33/0x34/0x35`) framing and collect the reply —
    /// the single-report `transact` panics on bodies > 60 B, which a `replaceNode`
    /// with long ids (e.g. an `…CabIR` node) overflows.
    pub(crate) fn send_chunked_collect(&mut self, body: &[u8], ms: u64) -> Result<(), String> {
        let reports = self.hid.transact_chunked(body, ms)?;
        self.raw.extend(reports);
        Ok(())
    }

    /// `nodeJsonRequest` (field 119){groupId,nodeId} — the node-edit-context preamble
    /// Pro Control sends immediately before a structural edit (device replies
    /// `nodeJsonResponse`, 120). Without it fw 1.8.45 drops the following `replaceNode`.
    /// Rides the live heartbeat cadence.
    fn node_json_request(&mut self, group: &str, node_id: &str) -> Result<(), String> {
        self.send_chunked_collect(&proto::node_json_request(group, node_id), 200)?;
        self.heartbeat()?;
        self.pump_collect(200)?;
        Ok(())
    }

    /// The active preset's display name, from the handshake's
    /// `currentPresetInfoChanged` (field 22) — used to CONFIRM the edit session is
    /// attached to the intended preset before mutating it (a load that didn't take
    /// leaves a different preset active; editing+saving it corrupts the target slot).
    pub fn active_preset_name(&self) -> Option<String> {
        self.push_bodies()
            .iter()
            .rev()
            .find_map(|b| extract_current_preset_display_name(b))
    }

    /// Pure read-only check: is the preset at 0-based `list_index` the ACTIVE one?
    /// Prefer the `PresetLoaded` slot echo (identity, immune to duplicate display
    /// names); fall back to the active-preset NAME ONLY when no slot echo has arrived
    /// — a slot echo that names a DIFFERENT slot must win over a possibly-duplicate
    /// name. This is the shared shape of every save-over guard site.
    ///
    /// The name fallback FAILS CLOSED unless the accumulated My Presets list proves
    /// `expected_name` maps to EXACTLY ONE slot and that slot is `list_index`: two
    /// presets can share a display name (or the name can be an empty-slot label), so a
    /// bare name match on a dropped load could otherwise confirm — and then save over —
    /// the WRONG preset. No proven-unique list ⇒ no confirmation.
    pub(crate) fn active_matches(&self, list_index: u32, expected_name: Option<&str>) -> bool {
        let loaded = self.loaded_slot();
        if loaded == Some(list_index) {
            return true;
        }
        if loaded.is_some() {
            return false;
        }
        let Some(n) = expected_name else {
            return false;
        };
        if n.is_empty() || is_empty_slot_name(n) || self.active_preset_name().as_deref() != Some(n)
        {
            return false;
        }
        self.best_preset_list()
            .is_some_and(|names| name_maps_uniquely(&names, n, list_index))
    }

    /// Confirm the preset at 0-based `list_index` is the ACTIVE one BEFORE a save-over
    /// write. Checks [`Self::active_matches`]; if inconclusive, re-arms the device's
    /// reply state ONCE (`connection_request` → `preset_list_request` →
    /// `current_preset_info_request`) to force a fresh `currentPresetInfoChanged`,
    /// then re-checks. `Err` when NEITHER the slot echo nor the name confirms — the
    /// caller MUST NOT save: a load that didn't take leaves a DIFFERENT preset active,
    /// and saving it overwrites the target slot with the wrong content (HW-demonstrated
    /// data loss). Only call on a fresh/quiet connection — the re-arm draws a
    /// `connectionError` on a dense-heartbeat session (the live-line re-arm gotcha).
    pub fn confirm_active(
        &mut self,
        list_index: u32,
        expected_name: Option<&str>,
    ) -> Result<(), String> {
        if self.active_matches(list_index, expected_name) {
            return Ok(());
        }
        self.send_and_collect(&proto::connection_request(), 80)?;
        self.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
        self.send_and_collect(&proto::current_preset_info_request(2), 120)?;
        if let Some(name) = expected_name {
            self.await_active_preset(name, 8);
        }
        if self.active_matches(list_index, expected_name) {
            return Ok(());
        }
        Err(format!(
            "could not confirm the preset at list index {list_index} is active before \
             saving (slot echo {:?}, active name {:?}, expected {:?}) — refusing the save \
             to avoid overwriting the wrong preset",
            self.loaded_slot(),
            self.active_preset_name(),
            expected_name,
        ))
    }

    /// True if any reply body carries `presetMessage` inner `field` (e.g. 40
    /// `nodeReplaced`, 53 `presetError`).
    fn saw_preset_field(&self, field: u32) -> bool {
        self.push_bodies().iter().any(|b| {
            proto::first_bytes(&proto::parse(b), TMS_PRESET)
                .map(|pm| proto::first_bytes(&proto::parse(pm), field).is_some())
                .unwrap_or(false)
        })
    }

    /// After sending a structural edit, pump (heartbeat) until the device CONFIRMS
    /// with `success_field` (`nodeReplaced` 40 for a replace, `nodeRemoved` 36 for a
    /// remove) → `Ok(true)`, or REJECTS with `presetError` (53) → `Ok(false)`. No reply
    /// within the window also → `Ok(false)`. The caller MUST NOT save on `Ok(false)`: a
    /// rejected/unconfirmed edit means the wrong preset is active or the node is absent,
    /// and saving would persist the WRONG content (HW: an unconfirmed save corrupted a
    /// slot).
    fn confirm_structural_edit(&mut self, success_field: u32) -> Result<bool, String> {
        for _ in 0..10 {
            self.heartbeat()?;
            self.pump_collect(200)?;
            if self.saw_preset_field(success_field) {
                return Ok(true);
            }
            if self.saw_preset_field(PRESET_ERROR) {
                return Ok(false);
            }
        }
        Ok(false)
    }

    /// Replace the node `node_id` in `dest_group` with model `fender_id` —
    /// `replaceNode` (field 39). Sends Pro Control's proven fw-1.8.45 sequence:
    /// `nodeJsonRequest` (edit context) → `replaceNode` with **NO batchStatus**, both
    /// chunked-safe and ridden on the live heartbeat the caller established via
    /// [`Self::begin_live_edit`]. **Returns `true` only when the device confirms with
    /// `nodeReplaced`** (and `false` on a `presetError`/no reply) — the caller gates
    /// the save on this.
    pub fn replace_node(
        &mut self,
        dest_group: &str,
        node_id: &str,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node(dest_group, node_id, fender_id, None),
            200,
        )?;
        self.confirm_structural_edit(NODE_REPLACED)
    }

    /// Replace the node `node_id` in `dest_group` with the user's SAVED block
    /// (`fender_id` at library `index`) — `replaceNodeWithBlock` (field 100). Same
    /// confirmed live sequence as [`Self::replace_node`]; returns `true` only on a
    /// `nodeReplaced` confirmation.
    pub fn replace_node_with_block(
        &mut self,
        dest_group: &str,
        node_id: &str,
        fender_id: &str,
        index: u64,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node_with_block(dest_group, node_id, fender_id, index, None),
            200,
        )?;
        self.confirm_structural_edit(NODE_REPLACED)
    }

    /// Remove the node `node_id` from `dest_group` — `removeNode` (field 35). Same
    /// confirmed live sequence as [`Self::replace_node`] (`nodeJsonRequest` edit-context
    /// preamble → `removeNode` with NO batchStatus, chunked-safe, on the live
    /// heartbeat); the device re-links the chain and confirms with `nodeRemoved` (36).
    /// **Returns `true` only on a `nodeRemoved` confirmation** (and `false` on a
    /// `presetError`/no reply) — the caller gates the save on this.
    pub fn remove_node(&mut self, dest_group: &str, node_id: &str) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(&proto::remove_node(dest_group, node_id, None), 200)?;
        self.confirm_structural_edit(NODE_REMOVED)
    }

    /// INSERT a new block (`fender_id`) into `dest_group` — `insertNode` (field 34).
    /// Unlike [`Self::replace_node`], Pro Control sends this **BARE**: NO
    /// `nodeJsonRequest` edit-context preamble (mirroring replace's preamble made the
    /// device REJECT the insert) and NO batchStatus — RE'd byte-exact from a PC add-block
    /// capture. `before` = the FenderId to insert AHEAD of (the device's
    /// field-2 inserts the new block BEFORE the referenced node — HW-verified fw 1.8.45:
    /// a short-anchor insert "before X" landed the new block ahead of X), or `None` to
    /// APPEND at the end of the group. Rides the live heartbeat established by
    /// [`Self::begin_live_edit`]. Returns `true` when the device confirms with
    /// `nodeInserted` (field 33, HW-confirmed), `false` on a `presetError`(53) or no reply.
    pub fn insert_node(
        &mut self,
        dest_group: &str,
        before: Option<&str>,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::insert_node(dest_group, before, fender_id, None),
            200,
        )?;
        self.confirm_structural_edit(NODE_INSERTED)
    }

    /// INSERT a block at a POSITION (`index`, group-relative) within `dest_group` —
    /// `insertNodeAtBlockIndex` (field 99), the index-based sibling of [`Self::insert_node`].
    /// Same bare framing (no `nodeJsonRequest` preamble), confirms on `nodeInserted` (33).
    /// HW (fw 1.8.45): `index = 0` into a single-block group landed the new block AFTER the
    /// existing one (it does NOT prepend), so the production Copy path uses the field-34
    /// `before` anchor in [`Self::insert_node`] instead; this stays as a TOOLING primitive
    /// (`probe --insert-map --at-index`) for characterising the index semantics.
    pub fn insert_node_at_index(
        &mut self,
        dest_group: &str,
        index: u32,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::insert_node_at_block_index(dest_group, index as u64, fender_id, None),
            200,
        )?;
        self.confirm_structural_edit(NODE_INSERTED)
    }

    /// True if the device REJECTED the last edit with `presetError` (53) — lets a caller
    /// distinguish a rejection (never retry, never save) from a silent drop (retry the
    /// cold first edit once).
    pub fn saw_preset_error(&self) -> bool {
        self.saw_preset_field(PRESET_ERROR)
    }

    /// Every distinct PresetMessage inner field number present in the accumulated reply
    /// bodies, sorted ascending — a diagnostic for discovering the device's actual
    /// structural-edit reply (the add-block inbound capture caught nothing, so the insert
    /// confirm field is unverified). Used by the `--insert-active` probe report.
    pub fn seen_preset_fields(&self) -> Vec<u32> {
        let mut seen = std::collections::BTreeSet::new();
        for b in self.push_bodies() {
            let top = proto::parse(&b);
            if let Some(pm) = proto::first_bytes(&top, TMS_PRESET) {
                for (n, _) in proto::parse(pm) {
                    seen.insert(n);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// The new node's id from the most recent `nodeReplaced`(40) reply's `nodeJson`
    /// (field 3) — needed to target a follow-up param set after a replace. `None` if no
    /// such reply is in the buffer or its JSON lacks an id.
    fn last_replaced_node_id(&self) -> Option<String> {
        self.push_bodies().iter().rev().find_map(|b| {
            let pm_fields = proto::parse(b);
            let pm = proto::first_bytes(&pm_fields, TMS_PRESET)?;
            let nr_fields = proto::parse(pm);
            let nr = proto::first_bytes(&nr_fields, NODE_REPLACED)?;
            let nr_inner = proto::parse(nr);
            let node_json = proto::first_bytes(&nr_inner, 3)?; // NodeReplaced.nodeJson
            let v: serde_json::Value = serde_json::from_slice(node_json).ok()?;
            v.get("nodeId")
                .or_else(|| v.get("FenderId"))
                .and_then(|x| x.as_str())
                .map(String::from)
        })
    }

    /// Read a STRING `dspUnitParameter` of `node_id` in `dest_group` by re-requesting
    /// its node JSON (`nodeJsonResponse` 120 → `nodeJsonString` field 1). `None` if the
    /// node/param isn't found — used to VERIFY a string param set (e.g. a user-IR file)
    /// landed before saving, so an unapplied set can never persist a half-edited node.
    fn read_node_param_str(
        &mut self,
        dest_group: &str,
        node_id: &str,
        param: &str,
    ) -> Result<Option<String>, String> {
        self.clear_raw();
        self.send_chunked_collect(&proto::node_json_request(dest_group, node_id), 200)?;
        self.heartbeat()?;
        self.pump_collect(200)?;
        Ok(self.push_bodies().iter().rev().find_map(|b| {
            let pm_fields = proto::parse(b);
            let pm = proto::first_bytes(&pm_fields, TMS_PRESET)?;
            let resp_fields = proto::parse(pm);
            let resp = proto::first_bytes(&resp_fields, NODE_JSON_RESPONSE)?;
            let resp_inner = proto::parse(resp);
            let node_json = proto::first_bytes(&resp_inner, 1)?; // nodeJsonString
            let v: serde_json::Value = serde_json::from_slice(node_json).ok()?;
            v.get("dspUnitParameters")?
                .get(param)?
                .as_str()
                .map(String::from)
        }))
    }

    /// Replace `node_id` in `dest_group` with a USER IR (`ACD_UserIRTMS`) pointing at
    /// `ir_file`. Two device edits on the held session: `replaceNode` → the IR block,
    /// then a STRING `changeParameter` setting the new node's `file` param to the chosen
    /// IR — VERIFIED by re-reading the node JSON. **Returns `true` only when the replace
    /// confirms (`nodeReplaced` 40) AND the `file` param reads back as `ir_file`**, so a
    /// rejected replace or an unapplied file never persists a fileless/half-edited IR
    /// node (the caller gates the save on this). NOTE: software-green, HW-validation
    /// pending (the string-param IR-file link is derived from the proto + preset JSON
    /// model, not a Pro Control capture).
    pub fn replace_node_with_ir(
        &mut self,
        dest_group: &str,
        node_id: &str,
        ir_file: &str,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node(dest_group, node_id, "ACD_UserIRTMS", None),
            200,
        )?;
        if !self.confirm_structural_edit(NODE_REPLACED)? {
            return Ok(false);
        }
        // The IR node's id after the replace (from nodeReplaced.nodeJson); fall back to
        // the FenderId the device assigns when no echo carries it.
        let new_id = self
            .last_replaced_node_id()
            .unwrap_or_else(|| "ACD_UserIRTMS".to_string());
        self.clear_raw();
        self.send_chunked_collect(
            &proto::change_parameter_str(dest_group, &new_id, "file", ir_file),
            200,
        )?;
        self.heartbeat()?;
        self.pump_collect(150)?;
        Ok(self
            .read_node_param_str(dest_group, &new_id, "file")?
            .as_deref()
            == Some(ir_file))
    }

    /// Persist the current edit buffer into the slot at **0-based list index**
    /// `list_index`. The device addresses user presets by a **1-based** userSlot,
    /// so we send `list_index + 1` (see [`Self::load_preset`] for the HW evidence).
    /// The device sends no save ACK; persistence is verified by reload.
    pub fn save_current_preset(&mut self, list_index: u32) -> Result<(), String> {
        self.hid
            .transact(&proto::save_current_preset((list_index + 1) as u64), 300)?;
        // Stored-preset mutation choke point: EVERY save routes through here, so this
        // one call keeps the Doctor's cached BEFORE clip correct-by-construction
        // (per-command clears were forgotten on ~7 mutating paths). Over-clearing is
        // harmless (the cache is a pure optimization). Same-crate call up into
        // commands/ — accepted for a static reset with no real coupling.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// Clear (delete) the user preset at **0-based list index** `list_index` —
    /// `clearUserPreset` (field 15). Sends the **1-based** device userSlot
    /// (`list_index + 1`). Fire-and-forget; verify by re-listing.
    pub fn clear_user_preset(&mut self, list_index: u32) -> Result<(), String> {
        self.hid
            .transact(&proto::clear_user_preset((list_index + 1) as u64), 300)?;
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// Rename the CURRENT preset — `renameCurrentPreset` (field 13). Per the Pro
    /// Control capture a rename is "save under a new name": the caller must follow
    /// this with `save_current_preset(slot)` to persist.
    /// Fire-and-forget; verify by reload.
    pub fn rename_current_preset(&mut self, name: &str) -> Result<(), String> {
        self.hid
            .transact(&proto::rename_current_preset(name), 300)?;
        Ok(())
    }

    /// Send `setFootswitchAssignment` (PresetMessage field 54): set ONE function
    /// (`index`, 0-based) on footswitch `addr` to `function_json` (the same JSON shape as
    /// a preset's `ftsw[switch][func]` object). The schema has NO dedicated confirm echo
    /// (unlike `nodeInserted`), so the caller confirms by re-reading the working-copy
    /// `ftsw` ([`Self::live_ftsw`]) or inspecting [`Self::seen_preset_fields`]. Sent
    /// chunked (a `param` functionJson exceeds the 60 B single-report limit), on the live
    /// heartbeat established by [`Self::begin_live_edit`]. `batch=None` = setter framing.
    pub fn set_footswitch_assignment(
        &mut self,
        addr: u32,
        index: u32,
        function_json: &str,
        swap: bool,
        batch: Option<u64>,
    ) -> Result<(), String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::set_footswitch_assignment(
                addr as u64,
                index as u64,
                function_json,
                swap,
                batch,
            ),
            250,
        )?;
        self.heartbeat()?;
        self.pump_collect(200)?;
        Ok(())
    }

    /// Send `clearFootswitchAssignment` (PresetMessage field 55): remove function `index`
    /// from footswitch `addr`. Same confirm model as [`Self::set_footswitch_assignment`].
    pub fn clear_footswitch_assignment(&mut self, addr: u32, index: u32) -> Result<(), String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::clear_footswitch_assignment(addr as u64, index as u64),
            250,
        )?;
        self.heartbeat()?;
        self.pump_collect(200)?;
        Ok(())
    }

    /// Re-prompt and read the live WORKING-COPY `ftsw` array (`currentPresetDataRequest`
    /// → fresh field-3 push). Reflects UNSAVED edits, so it's how a footswitch set/clear is
    /// confirmed (no dedicated echo). `ftsw` sits at byte ~4330 of field-3, before the
    /// scene-tail truncation, so it survives the partial. `None` if no field-3 lands.
    pub fn live_ftsw(&mut self) -> Option<serde_json::Value> {
        self.clear_raw();
        let _ = self.send_and_collect(&proto::current_preset_data_request(3), 300);
        for _ in 0..8 {
            let _ = self.heartbeat();
            let _ = self.pump_collect(200);
            if let Ok(v) = self.current_preset_value() {
                if let Some(ftsw) = v.get("ftsw") {
                    return Some(ftsw.clone());
                }
            }
        }
        None
    }

    /// Re-import a full preset to the device. `preset_bytes` is the
    /// raw `.preset` file content (XOR'd compact JSON, the OFFLINE codec's output).
    /// It is LZ4-block compressed and wrapped in `importPresetRequest` (field 117),
    /// then sent as a chunked `0x33/0x34*/0x35` stream — the framing + encoding
    /// reverse-engineered from a Pro Control import capture. Returns the `(listEnum, presetSlot)`
    /// the device reports in `importPresetResponse` (118) if it replies; the device
    /// chooses the slot (Pro Control then `loadPreset`s it).
    pub fn import_preset(&mut self, preset_bytes: &[u8]) -> Result<Option<(u32, u32)>, String> {
        self.raw.clear();
        let payload = proto::lz4_block_compress_stored(preset_bytes);
        let body = proto::import_preset_request(&payload);
        // Longer pump than setters (300 ms): a ~100-frame import takes longer to
        // ingest, and any importPresetResponse echo trails the whole burst.
        let reports = self.hid.transact_chunked(&body, 1500)?;
        self.raw.extend(reports);
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        // importPresetResponse: ImportPresetResponse{ presetJson=1, listEnum=2, presetSlot=3 }
        Ok(self.streams().iter().find_map(|s| {
            let fields = dig(&s.body, TMS_PRESET, 118)?;
            let slot = field_n(&fields, 3)?.as_u64()? as u32;
            let list_enum = field_n(&fields, 2).and_then(Val::as_u64).unwrap_or(0) as u32;
            Some((list_enum, slot))
        }))
    }

    /// Relocate a user preset between **0-based list indices** `old`→`new` —
    /// `moveUserPreset` (field 16). Sends the **1-based** device userSlots
    /// (`old + 1`, `new + 1`). Fire-and-forget; verify by re-listing.
    pub fn move_user_preset(&mut self, old: u32, new: u32) -> Result<(), String> {
        self.hid.transact(
            &proto::move_user_preset((old + 1) as u64, (new + 1) as u64),
            300,
        )?;
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// Assign a user preset (+ its scene) to a Song row — `assignSongPreset`.
    /// `user_list_index` is a **0-based list index**; the device `userPresetSlot` is
    /// 1-based, so `+1` is applied (consistent with every other slot setter). The
    /// `song_slot` / `song_preset_slot` / `preset_scene_slot` are song-internal
    /// positions passed through. Fire-and-forget; verify by re-reading the song.
    #[allow(clippy::too_many_arguments)]
    pub fn assign_song_preset(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
        user_list_index: u32,
        footswitch_label: &str,
        footswitch_color: u32,
        preset_scene_slot: u32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::assign_song_preset(
                song_slot as u64,
                song_preset_slot as u64,
                (user_list_index + 1) as u64,
                footswitch_label,
                footswitch_color as u64,
                preset_scene_slot as u64,
            ),
            300,
        )?;
        Ok(())
    }

    /// Reorder a Song row — `moveSongPreset`. Song-internal positions.
    pub fn move_song_preset(&mut self, song_slot: u32, old: u32, new: u32) -> Result<(), String> {
        self.hid.transact(
            &proto::move_song_preset(song_slot as u64, old as u64, new as u64),
            300,
        )?;
        Ok(())
    }

    /// Swap two Song rows — `swapSongPreset`.
    pub fn swap_song_preset(&mut self, song_slot: u32, a: u32, b: u32) -> Result<(), String> {
        self.hid.transact(
            &proto::swap_song_preset(song_slot as u64, a as u64, b as u64),
            300,
        )?;
        Ok(())
    }

    /// Empty a Song row — `clearSongPreset`.
    pub fn clear_song_preset(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::clear_song_preset(song_slot as u64, song_preset_slot as u64),
            300,
        )?;
        Ok(())
    }

    /// Activate scene `scene_slot` within the current preset — `loadScene`.
    /// Non-destructive (changes the active scene, not stored data); enables per-scene
    /// re-amp capture. Load a preset first, then drive scenes.
    pub fn load_scene(&mut self, scene_slot: u32) -> Result<(), String> {
        // Eager pump — same fire-and-forget rationale as `load_preset`.
        self.hid
            .transact_eager(&proto::load_scene(scene_slot as u64), 300)?;
        Ok(())
    }

    /// Enable/disable per-block Scene Edit on `(group_id, node_id)` for the ACTIVE
    /// scene — once enabled, a `change_parameter` on that block writes the scene
    /// overlay (per-scene), not the base. Fire-and-forget setter (no batchStatus),
    /// like `change_parameter`. Used by per-scene leveling.
    pub fn set_node_scene_edit(
        &mut self,
        group_id: &str,
        node_id: &str,
        enable: bool,
    ) -> Result<(), String> {
        self.hid
            .transact(&proto::set_node_scene_edit(group_id, node_id, enable), 300)?;
        Ok(())
    }

    /// Make a song the ACTIVE song by loading one of its footswitch presets
    /// (`LoadPreset{ tabEnum=5, songSlot, songPresetSlot, presetSlot }`). Required
    /// before `set_tap_tempo_bpm` targets that song. Fire-and-forget.
    pub fn load_song(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
        preset_slot: u32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::load_song(
                song_slot as u64,
                song_preset_slot as u64,
                preset_slot as u64,
            ),
            400,
        )?;
        Ok(())
    }

    /// Set the global tap-tempo BPM (`SettingsMessage.tapTempoBpm`, originatorId=1),
    /// which the device stores as the **active song's** BPM. Load the target song
    /// first (`load_song`). Fire-and-forget.
    pub fn set_tap_tempo_bpm(&mut self, bpm: f32) -> Result<(), String> {
        self.hid.transact(&proto::set_tap_tempo_bpm(bpm, 1), 300)?;
        Ok(())
    }

    // ─── Song CRUD (fire-and-forget setters; verify by re-reading the list) ──
    pub fn add_song(&mut self, name: &str) -> Result<(), String> {
        self.hid.transact(&proto::add_song(name), 400)?;
        Ok(())
    }
    pub fn rename_song(&mut self, song_slot: u32, name: &str) -> Result<(), String> {
        self.hid
            .transact(&proto::rename_song(song_slot as u64, name), 400)?;
        Ok(())
    }
    pub fn remove_song(&mut self, song_slot: u32) -> Result<(), String> {
        self.hid
            .transact(&proto::remove_song(song_slot as u64), 400)?;
        Ok(())
    }
    pub fn set_song_notes(&mut self, song_slot: u32, notes: &str) -> Result<(), String> {
        self.hid
            .transact(&proto::set_song_notes(song_slot as u64, notes), 400)?;
        Ok(())
    }
    pub fn set_song_bpm_active(&mut self, song_slot: u32, active: bool) -> Result<(), String> {
        self.hid
            .transact(&proto::set_song_bpm_active(song_slot as u64, active), 300)?;
        Ok(())
    }

    // ─── Setlist CRUD (fire-and-forget setters) ─────────────────────────────
    pub fn add_setlist(&mut self, name: &str) -> Result<(), String> {
        self.hid.transact(&proto::add_setlist(name), 400)?;
        Ok(())
    }
    pub fn rename_setlist(&mut self, setlist_slot: u32, name: &str) -> Result<(), String> {
        self.hid
            .transact(&proto::rename_setlist(setlist_slot as u64, name), 400)?;
        Ok(())
    }
    pub fn remove_setlist(&mut self, setlist_slot: u32) -> Result<(), String> {
        self.hid
            .transact(&proto::remove_setlist(setlist_slot as u64), 400)?;
        Ok(())
    }
    pub fn add_setlist_song(&mut self, setlist_slot: u32, song_slot: u32) -> Result<(), String> {
        self.hid.transact(
            &proto::add_setlist_song(setlist_slot as u64, song_slot as u64),
            400,
        )?;
        Ok(())
    }
    /// Remove a song from a setlist by its POSITION within the setlist
    /// (`setlist_song_slot`, NOT the global song slot). Fire-and-forget.
    pub fn remove_setlist_song(
        &mut self,
        setlist_slot: u32,
        setlist_song_slot: u32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::remove_setlist_song(setlist_slot as u64, setlist_song_slot as u64),
            400,
        )?;
        Ok(())
    }
    /// Reorder a song within a setlist by POSITION (both indices are positions
    /// within the setlist, NOT global song slots). Fire-and-forget.
    pub fn move_setlist_song(
        &mut self,
        setlist_slot: u32,
        old_pos: u32,
        new_pos: u32,
    ) -> Result<(), String> {
        self.hid.transact(
            &proto::move_setlist_song(setlist_slot as u64, old_pos as u64, new_pos as u64),
            400,
        )?;
        Ok(())
    }

    /// Decode the Song-preset reply from the accumulated streams. Largest record
    /// set wins (a complete multi-packet response beats a stray/partial frame).
    pub fn harvest_song_presets(&self) -> Vec<SongPresetRecord> {
        self.streams()
            .iter()
            .map(|s| {
                proto::song_preset_list_records(&s.body)
                    .iter()
                    .map(|r| SongPresetRecord {
                        is_empty: proto::first_varint(r, 1).unwrap_or(0) != 0,
                        user_preset_slot: proto::first_varint(r, 2).unwrap_or(0) as u32,
                        preset_scene_slot: proto::first_varint(r, 5).unwrap_or(0) as u32,
                        preset_scene_name: proto::first_bytes(r, 6)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
            .unwrap_or_default()
    }

    // ─── Strict, completeness-validated harvests ────────────────────────────
    // (The non-strict `harvest_songs`/`harvest_setlists`/`harvest_setlist_songs`
    // were removed once every read path moved to the fail-closed strict variants —
    // a tolerant decode can return a tail-truncated list, which these reads must
    // never do. The strict harvests + `*_records_strict` decoders are the only tier.)
    // Return `Some` only if a reassembled stream decodes as a COMPLETE list
    // response (no tail truncation); `None` if only truncated/absent responses
    // are present. The caller retries until `Some` (fail-closed) — this is what
    // makes the multi-packet reads reliable despite concurrent-stream truncation.

    /// Strict, completeness-validated My-Presets list: `streams_final()` only
    /// (terminal-0x35 rule) + strict parsing at every level, so a tail-truncated
    /// multi-packet response is rejected rather than clipped. Records missing a
    /// displayName map to "" (NOT dropped) so list indices — and therefore the
    /// `userSlot = index + 1` invariant — are preserved. Longest COMPLETE wins.
    pub fn harvest_preset_list_strict(&self) -> Option<Vec<String>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::preset_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .map(|r| {
                        proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }

    pub fn harvest_songs_strict(&self) -> Option<Vec<SongRecord>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::song_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .enumerate()
                    .map(|(i, r)| SongRecord {
                        slot: (i + 1) as u32,
                        name: proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                        notes: proto::first_bytes(r, 2)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                        bpm_active: proto::first_varint(r, 3).unwrap_or(0) != 0,
                        bpm: proto::first_varint(r, 4).unwrap_or(0) as u32,
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }

    pub fn harvest_setlists_strict(&self) -> Option<Vec<SetlistRecord>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::setlist_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .enumerate()
                    .map(|(i, r)| SetlistRecord {
                        slot: (i + 1) as u32,
                        name: proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }

    pub fn harvest_setlist_songs_strict(&self) -> Option<Vec<u32>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::setlist_song_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .map(|r| proto::first_varint(r, 1).unwrap_or(0) as u32)
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }
}

/// Parse a FenderMessageTMS body, descend `presetMessage → presetListResponse`,
/// and collect the `displayName` of each record. Returns None if this body
/// doesn't carry a PresetListResponse.
fn extract_preset_list(body: &[u8]) -> Option<Vec<String>> {
    // My Presets only — the handshake also collects the Factory/Cloud responses
    // on the same session (the wrong-list hazard is documented on the helper).
    // A missing listEnum is treated as My Presets (lean sessions request only 1).
    extract_preset_list_for(body, LIST_ENUM_MY_PRESETS)
}

/// Preset-list `listEnum` values (My Presets = 1, Factory = 4, Cloud = 3).
const LIST_ENUM_MY_PRESETS: u64 = 1;
const LIST_ENUM_FACTORY: u64 = 4;

/// `extract_preset_list`, gated to an explicit `listEnum` (field 1 of the
/// `PresetListResponse`). A missing listEnum defaults to My Presets, so it only
/// matches `want_enum == 1` (Factory/Cloud must be present explicitly).
fn extract_preset_list_for(body: &[u8], want_enum: u64) -> Option<Vec<String>> {
    let resp = dig(body, TMS_PRESET, PRESET_LIST_RESPONSE)?;
    if proto::first_varint(&resp, 1).unwrap_or(LIST_ENUM_MY_PRESETS) != want_enum {
        return None;
    }
    // PresetListResponse.record (field 2, repeated) → PresetListRecord.displayName (field 1).
    let names: Vec<String> = resp
        .iter()
        .filter(|(f, _)| *f == 2)
        .filter_map(|(_, v)| v.as_bytes())
        .filter_map(|rec| {
            let rf = proto::parse(rec);
            field1(&rf)
                .and_then(|v| v.as_bytes())
                .map(|s| String::from_utf8_lossy(s).into_owned())
        })
        .collect();
    Some(names)
}

/// Display names → `PresetEntry` rows: the 0-based list index IS the slot (the
/// device's 1-based `userSlot` translation happens in the setters, not here).
fn preset_entries(names: Vec<String>) -> Vec<PresetEntry> {
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| PresetEntry {
            slot: i as u32,
            name,
        })
        .collect()
}

fn best_preset_list_from_reports(reports: &[Vec<u8>]) -> Option<Vec<String>> {
    proto::reassemble_streams(reports)
        .into_iter()
        .chain(proto::reassemble_streams_final(reports))
        .filter_map(|s| extract_preset_list(&s.body))
        .max_by_key(|names| (names.len(), names.last().map_or(0, |s| s.len())))
}

/// Longest FACTORY (listEnum = 4) preset list from the accumulated reports. Same
/// dual-stream-rule harvest as `best_preset_list_from_reports`, but gated to the
/// Factory list — the handshake requests My/Factory/Cloud on one session, so this
/// picks the Factory reply out of the shared accumulator.
fn best_factory_list_from_reports(reports: &[Vec<u8>]) -> Option<Vec<String>> {
    proto::reassemble_streams(reports)
        .into_iter()
        .chain(proto::reassemble_streams_final(reports))
        .filter_map(|s| extract_preset_list_for(&s.body, LIST_ENUM_FACTORY))
        .max_by_key(|names| (names.len(), names.last().map_or(0, |s| s.len())))
}

/// Extract the firmware version from a `currentFwResponse` frame:
/// settingsMessage(3) → currentFwResponse(2) → data(1), a plain UTF-8 string
/// (e.g. "1.7.75"). The device sends this in reply to a `currentFwRequest`
/// issued in-burst with no batchStatus (see [`Session::connect_with_firmware`]);
/// a standalone request after the burst is rejected with ConnectionError.
fn extract_fw_version(body: &[u8]) -> Option<String> {
    dig(body, TMS_SETTINGS, SETTINGS_CURRENT_FW).and_then(|fields| {
        field1(&fields)
            .and_then(Val::as_bytes)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
    })
}

/// Extract the current preset's display name from the handshake's
/// `CurrentPresetInfoChanged` event. Unlike the truncated preset JSON, this
/// event is small and reliably survives the live discovery burst.
fn extract_current_preset_display_name(body: &[u8]) -> Option<String> {
    dig(body, TMS_PRESET, CURRENT_PRESET_INFO_CHANGED).and_then(|fields| {
        field1(&fields)
            .and_then(Val::as_bytes)
            .map(|s| String::from_utf8_lossy(s).into_owned())
    })
}

/// Extract a My Presets list index from `PresetLoaded`. The wire event carries
/// the 1-based device user slot; the Companion uses 0-based list indices.
fn extract_loaded_user_slot(body: &[u8]) -> Option<u32> {
    let fields = dig(body, TMS_PRESET, PRESET_LOADED)?;
    let tab_enum = field1(&fields)?.as_u64()?;
    if tab_enum != 1 {
        return None;
    }
    let device_slot = field_n(&fields, 6)?.as_u64()? as u32;
    device_slot.checked_sub(1)
}

fn unique_preset_slot_by_name(presets: &[PresetEntry], name: &str) -> Option<u32> {
    let mut matches = presets.iter().filter(|p| p.name == name);
    let slot = matches.next()?.slot;
    matches.next().is_none().then_some(slot)
}

// ─── Live-push decoders (shared by `describe_push` AND the device monitor) ─────
// PresetMessage push field numbers (the unit's unsolicited pushes, TMS field 2).
const PRESET_LOADED_PUSH: u32 = 11; // PresetLoaded
const CURRENT_PRESET_DATA_CHANGED: u32 = 3; // currentPresetDataChanged (LZ4 preset JSON)
const SCENE_LOADED: u32 = 102; // SceneLoaded { sceneSlot, sceneJson (LZ4) }
const SCENE_LIST_RESPONSE: u32 = 125; // sceneListResponse { dummy, sceneList[] }

/// Decoded `PresetLoaded` (TMS[2] → [11]) addressing fields. All slots are the
/// device's own (1-based where applicable); the caller applies the list-index
/// translation. The full set the unit pushes on a preset change.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PresetLoadedFields {
    pub tab_enum: u64,
    pub setlist_slot: u64,
    pub setlist_song_slot: u64,
    pub song_slot: u64,
    pub song_preset_slot: u64,
    pub preset_slot: u64,
    pub sync: u64,
}

/// Decode a `PresetLoaded` push (TMS[2] → [11]). `None` if this body is not one.
pub(crate) fn decode_preset_loaded(body: &[u8]) -> Option<PresetLoadedFields> {
    let f = dig(body, TMS_PRESET, PRESET_LOADED_PUSH)?;
    let g = |n| field_n(&f, n).and_then(Val::as_u64).unwrap_or(0);
    Some(PresetLoadedFields {
        tab_enum: g(1),
        setlist_slot: g(2),
        setlist_song_slot: g(3),
        song_slot: g(4),
        song_preset_slot: g(5),
        preset_slot: g(6),
        sync: g(7),
    })
}

/// Decode a `CurrentPresetInfoChanged` push (TMS[2] → [22]) into
/// `(displayName, isDirty, isFavorite)`. `None` if this body is not one.
/// Per `CurrentPresetInfoChanged.proto`: displayName(1) bytes, isDirty(2) bool,
/// isFavorite(3) bool.
pub(crate) fn decode_info_changed(body: &[u8]) -> Option<(String, bool, bool)> {
    let f = dig(body, TMS_PRESET, CURRENT_PRESET_INFO_CHANGED)?;
    let name = field_n(&f, 1)
        .and_then(Val::as_bytes)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let is_dirty = field_n(&f, 2).and_then(Val::as_u64).unwrap_or(0) != 0;
    let is_favorite = field_n(&f, 3).and_then(Val::as_u64).unwrap_or(0) != 0;
    Some((name, is_dirty, is_favorite))
}

/// Decode a `SceneLoaded` push (TMS[2] → [102]) into `(sceneSlot, sceneName)`.
/// `sceneName` is parsed from the LZ4-block `sceneJson` (field 2) — `None` when
/// the payload is absent, not LZ4, or truncated before the `sceneName` property.
/// `None` for the whole tuple if this body is not a `SceneLoaded`.
pub(crate) fn decode_scene_loaded(body: &[u8]) -> Option<(u32, Option<String>)> {
    let f = dig(body, TMS_PRESET, SCENE_LOADED)?;
    let slot = field_n(&f, 1).and_then(Val::as_u64).unwrap_or(0) as u32;
    let name = proto::first_bytes(&f, 2)
        .and_then(decode_preset_json)
        .and_then(|text| extract_partial_json_string(&text, "sceneName"));
    Some((slot, name))
}

/// Decode a `sceneListResponse` push (TMS[2] → [125]) into the live scene NAMES
/// of the ACTIVE preset (repeated string `sceneList`, field 2). `None` if this
/// body is not a `sceneListResponse`. An empty Vec is a valid result (a preset
/// with no FS scenes).
pub(crate) fn decode_scene_list(body: &[u8]) -> Option<Vec<String>> {
    let f = dig(body, TMS_PRESET, SCENE_LIST_RESPONSE)?;
    Some(
        proto::all_bytes(&f, 2)
            .into_iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect(),
    )
}

/// Decode a `currentPresetDataChanged` push (TMS[2] → [3]) into an [`ActiveGraph`]
/// — the same decode pipeline `current_audio_graph` runs (LZ4 → tolerant JSON →
/// `extract_active_graph`), with the SAME known-routing-template guard so a
/// too-truncated partial yields `None` rather than a garbage chain. `None` if
/// this body is not a `currentPresetDataChanged`, or the partial truncated before
/// a complete `audioGraph.template`. Thin wrapper over [`decode_current_preset_live`]
/// so a body is parsed ONCE when both the scene trio and the graph are wanted.
pub(crate) fn decode_current_preset_data(body: &[u8]) -> Option<ActiveGraph> {
    decode_current_preset_live(body)?.graph
}

/// Extract the [`ActiveGraph`] from an already-parsed field-3 preset JSON, applying
/// the known-routing-template guard (mirrors `current_audio_graph`). `text` supplies
/// the truncation-tolerant `template` hint.
fn active_graph_guarded(v: &serde_json::Value, text: &str) -> Option<ActiveGraph> {
    let template_hint = extract_partial_json_string(text, "template");
    let graph = extract_active_graph(v, template_hint.as_deref());
    // A non-empty chain must carry a recognized routing template, else the strip could
    // draw parallel as series; an empty-node partial (no template) is also rejected.
    if graph.nodes.is_empty() || !is_known_routing_template(graph.template.as_deref()) {
        return None;
    }
    Some(graph)
}

/// The device's base scene slot on the wire: `loadScene`/`SceneLoaded`/`lastLoadedScene`
/// address FS scenes as 0-based `scenes[]` indices (0..=7) and the base scene as the
/// CONSTANT `8` — even for a preset with zero FS scenes (HW-observed: Cello, 0 scenes,
/// base SceneLoaded sceneSlot=8; Guitar dump `lastLoadedScene: 8` while on base). NOT
/// `scene_count + 1` — that old theory is refuted by the Cello observation.
pub(crate) const BASE_SCENE_SLOT: u32 = 8;

/// The live scene metadata harvested from ONE `currentPresetDataChanged` push
/// (TMS[2] → [3], LZ4) — the same field-3 body as [`decode_current_preset_data`], but
/// surfacing the scene-facing trio instead of the graph (the monitor parses both from
/// one push). All three come from a SINGLE coherent preset-JSON document, so their
/// index spaces agree by construction:
/// - `scene_names` — `scenes[].sceneName` in slot order (the canonical scene rows),
/// - `last_loaded_scene` — the ACTIVE scene slot (0-based into `scenes[]`;
///   [`BASE_SCENE_SLOT`] = base), present in every push,
/// - `ftsw` — the footswitch assignments (`sceneSlot` indexes `scenes[]`).
///
/// Each half is `None` when the tolerant parse of a truncated partial lost it; on a
/// healthy dense-heartbeat session the payload carries all three (HW-verified: a 17 KB live field-3 truncates only inside the FINAL scene's `uuid`,
/// after every `sceneName`).
pub(crate) struct CurrentPresetLive {
    pub scene_names: Option<Vec<String>>,
    pub last_loaded_scene: Option<u32>,
    pub ftsw: Option<serde_json::Value>,
    /// The signal-chain graph from the SAME parse (template-guarded; `None` when the
    /// partial truncated before a complete `audioGraph.template`). The monitor reads
    /// both halves from one decode rather than parsing the ~17 KB body twice per push.
    pub graph: Option<ActiveGraph>,
}

/// Decode the scene-facing trio + the signal-chain graph from a
/// `currentPresetDataChanged` push, in ONE LZ4-decompress + tolerant-parse pass.
/// `None` if this is not a field-3 body or the LZ4/parse failed entirely.
pub(crate) fn decode_current_preset_live(body: &[u8]) -> Option<CurrentPresetLive> {
    let f = dig(body, TMS_PRESET, CURRENT_PRESET_DATA_CHANGED)?;
    let payload = proto::first_bytes(&f, 1)?;
    let text = decode_preset_json(payload)?;
    decode_preset_live_text(&text)
}

/// Decode the scene-facing trio + signal-chain graph from plaintext preset JSON
/// bytes, such as a slot-addressed field-9 `presetDataChanged` partial. This is the
/// same tolerant parse used for live field-3, minus the TMS/LZ4 wrapper step.
pub(crate) fn decode_plain_preset_live(json: &[u8]) -> Option<CurrentPresetLive> {
    let text = String::from_utf8_lossy(json);
    decode_preset_live_text(&text)
}

fn decode_preset_live_text(text: &str) -> Option<CurrentPresetLive> {
    let v = tolerant_parse_json(text)?;
    let scene_names = v.get("scenes").and_then(|s| s.as_array()).map(|arr| {
        arr.iter()
            .map(|sc| {
                sc.get("sceneName")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    });
    let last_loaded_scene = v
        .get("lastLoadedScene")
        .and_then(|n| n.as_u64())
        .map(|n| n as u32);
    let graph = active_graph_guarded(&v, text);
    Some(CurrentPresetLive {
        scene_names,
        last_loaded_scene,
        ftsw: v.get("ftsw").cloned(),
        graph,
    })
}

/// Scene names (slot order) from a slot-addressed field-9 **plaintext**
/// preset-JSON partial ([`Session::read_slot_preset_json`]). Same tolerant
/// parse as the live field-3 decode — large presets device-truncate inside the
/// FINAL scene's `uuid`, after every `sceneName` (HW: slot 1 = 17264 B, all 8
/// names present). `None` = the partial truncated before the `scenes` key
/// (scene presence unknown), distinct from `Some(vec![])` (no scenes).
pub(crate) fn scene_names_from_slot_json(json: &[u8]) -> Option<Vec<String>> {
    decode_plain_preset_live(json)?.scene_names
}

/// Descend two protobuf levels: parse `body` as FenderMessageTMS, take the
/// `tms_field` sub-message, parse it, take the `inner_field` sub-message, and
/// return its decoded fields. None if either level is absent.
fn dig(body: &[u8], tms_field: u32, inner_field: u32) -> Option<Vec<(u32, Val)>> {
    let top = proto::parse(body);
    let sub = proto::first_bytes(&top, tms_field)?;
    let mid = proto::parse(sub);
    let inner = proto::first_bytes(&mid, inner_field)?;
    Some(proto::parse(inner))
}

fn field1(fields: &[(u32, Val)]) -> Option<&Val> {
    field_n(fields, 1)
}

/// One-line human description of an inbound stream for the push-listener
/// experiment (`listen_dump`). Generic field-path summary like `send_and_dump`,
/// with the known push messages decoded: PresetLoaded 2[11], SceneLoaded 2[102]
/// (sceneSlot + a sceneJson size/prefix peek), currentPresetDataChanged 2[3],
/// CurrentPresetInfoChanged 2[22].
fn describe_push(body: &[u8]) -> String {
    let top = proto::parse(body);
    // presetMessage pushes get the detailed treatment.
    if let Some(pm) = proto::first_bytes(&top, TMS_PRESET) {
        let inner = proto::parse(pm);
        if let Some(pl) = decode_preset_loaded(body) {
            // Byte-identical to the previous inline decode (now via the shared
            // `decode_preset_loaded`, so the monitor and the listener agree).
            return format!(
                "PresetLoaded 2[11]: tab={} setlist={} setlistSong={} song={} songPreset={} presetSlot={} sync={}",
                pl.tab_enum, pl.setlist_slot, pl.setlist_song_slot, pl.song_slot, pl.song_preset_slot, pl.preset_slot, pl.sync
            );
        }
        if let Some(sl) = proto::first_bytes(&inner, 102) {
            let f = proto::parse(sl);
            let slot = field_n(&f, 1).and_then(Val::as_u64).unwrap_or(0);
            let json = proto::first_bytes(&f, 2).unwrap_or(&[]);
            let text = decode_preset_json(json).unwrap_or_default();
            let prefix: String = text.chars().take(160).collect();
            return format!("SceneLoaded 2[102]: sceneSlot={slot} sceneJson={}B (decoded {}B) prefix={prefix:?}", json.len(), text.len());
        }
        if let Some(ch) = proto::first_bytes(&inner, 3) {
            let f = proto::parse(ch);
            let json = proto::first_bytes(&f, 1).unwrap_or(&[]);
            let text = decode_preset_json(json).unwrap_or_default();
            let prefix: String = text.chars().take(80).collect();
            return format!(
                "currentPresetDataChanged 2[3]: payload={}B (decoded {}B) prefix={prefix:?}",
                json.len(),
                text.len()
            );
        }
        if let Some(ci) = proto::first_bytes(&inner, 22) {
            let f = proto::parse(ci);
            let parts: Vec<String> = f
                .iter()
                .map(|(n, v)| match (v.as_u64(), v.as_bytes()) {
                    (Some(u), _) => format!("{n}={u}"),
                    (_, Some(b)) => format!("{n}={:?}", String::from_utf8_lossy(b)),
                    _ => format!("{n}=?"),
                })
                .collect();
            return format!("CurrentPresetInfoChanged 2[22]: {{{}}}", parts.join(", "));
        }
        let fields: Vec<u32> = inner.iter().map(|(g, _)| *g).collect();
        return format!("presetMessage 2{fields:?} ({}B)", body.len());
    }
    // ConnectionMessage (TMS field 4): connectionRequest=1 / connectionResponse=2 /
    // connectionError=3 / connectionHeartbeat=4. Hex-dump the body so the error
    // code is visible (the heartbeat draws a connectionError — code unknown).
    if let Some(cm) = proto::first_bytes(&top, 4) {
        let inner = proto::parse(cm);
        let which = inner.first().map(|(g, _)| *g).unwrap_or(0);
        let label = match which {
            1 => "connectionRequest",
            2 => "connectionResponse",
            3 => "connectionError",
            4 => "connectionHeartbeat",
            _ => "connection?",
        };
        let detail: Vec<String> = inner
            .iter()
            .map(|(n, v)| match (v.as_u64(), v.as_bytes()) {
                (Some(u), _) => format!("{n}={u}"),
                (_, Some(b)) => format!("{n}={:?}", String::from_utf8_lossy(b)),
                _ => format!("{n}=?"),
            })
            .collect();
        return format!(
            "ConnectionMessage 4[{which}] {label} {{{}}} hex={}",
            detail.join(","),
            hexs(body)
        );
    }
    // Everything else: top-level field path, expanding one level where useful.
    let mut desc = Vec::new();
    for (f, _) in &top {
        if let Some(b) = proto::first_bytes(&top, *f) {
            let inner: Vec<u32> = proto::parse(b).iter().map(|(g, _)| *g).collect();
            if !inner.is_empty() {
                desc.push(format!("{f}{inner:?}"));
                continue;
            }
        }
        desc.push(f.to_string());
    }
    // Hex-dump small unrecognized streams (≤32B) so nothing is lost to the summary.
    let hex = if body.len() <= 32 {
        format!(" hex={}", hexs(body))
    } else {
        String::new()
    };
    format!("fields={} ({}B){hex}", desc.join(","), body.len())
}

/// Lowercase hex of a byte slice (capped to 64 bytes) for the listener's dumps.
fn hexs(b: &[u8]) -> String {
    b.iter().take(64).map(|x| format!("{x:02x}")).collect()
}

fn field_n(fields: &[(u32, Val)], field_no: u32) -> Option<&Val> {
    fields.iter().find(|(f, _)| *f == field_no).map(|(_, v)| v)
}

/// Decode a `presetJson` payload to text: try LZ4-block decompress (the field-3
/// `currentPresetDataChanged` wire form), else treat as literal UTF-8 (the
/// field-79 raw form). Lossy UTF-8 so a stream truncated mid-multibyte still
/// yields the valid prefix (the device sends a partial preset in the change
/// event — `audioGraph` lives in the first ~1.5 KB and survives).
fn decode_preset_json(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    if let Ok(raw) = proto::lz4_block_decompress(payload) {
        if !raw.is_empty() {
            return Some(String::from_utf8_lossy(&raw).into_owned());
        }
    }
    Some(String::from_utf8_lossy(payload).into_owned())
}

/// Parse possibly-TRUNCATED preset JSON. Tries a strict parse first; on failure
/// (the preset-data stream is routinely truncated mid-object), rewinds to the
/// last `,` outside a string and closes all open `[`/`{` containers in reverse
/// nesting order,
/// recovering everything up to that boundary. Port of `drive_replace_node.py`'s
/// `_tolerant_parse_partial_json` — `audioGraph.guitarNodes` sits early enough to
/// always survive this for our enumeration purposes.
pub(crate) fn tolerant_parse_json(text: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return Some(v);
    }
    let (mut in_str, mut esc) = (false, false);
    let mut stack = Vec::new();
    let mut last_cut: Option<(usize, Vec<char>)> = None; // (byte pos before ',', open containers)
    for (i, c) in text.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            _ if in_str => {}
            '{' | '[' => stack.push(c),
            '}' if stack.pop() != Some('{') => return None,
            ']' if stack.pop() != Some('[') => return None,
            ',' if !stack.is_empty() => last_cut = Some((i, stack.clone())),
            _ => {}
        }
    }
    let (pos, stack) = last_cut?;
    let mut padded = text[..pos].to_string();
    padded.extend(
        stack
            .into_iter()
            .rev()
            .map(|c| if c == '[' { ']' } else { '}' }),
    );
    serde_json::from_str(&padded).ok()
}

/// Recover a JSON string value from a truncated payload. The live active-preset
/// stream can end in the middle of `"template":"gtrParallel2"`, after enough
/// bytes to identify the routing template but before strict/tolerant parsing can
/// retain that final property.
fn extract_partial_json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let tail = text.rsplit_once(&needle)?.1.trim_start();
    let tail = tail.strip_prefix(':')?.trim_start();
    let mut chars = tail.strip_prefix('"')?.chars();
    let mut out = String::new();
    let mut escaped = false;
    for c in chars.by_ref() {
        if escaped {
            out.push(c);
            escaped = false;
        } else {
            match c {
                '\\' => escaped = true,
                '"' => break,
                _ => out.push(c),
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A control name is a leveling candidate if it reads as a level/volume/output
/// control. Deliberately EXCLUDES `gain`/drive (changes tone, not clean level)
/// and EQ — the scoping decision was "level-type controls only".
fn is_level_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("gain") || n.contains("drive") {
        return false;
    }
    n.contains("level") || n.contains("volume") || n == "vol" || n.contains("master")
}

/// Walk decoded preset JSON and collect every level-type block control under
/// `audioGraph.guitarNodes.{G1..G7}[].dspUnitParameters`. Numeric params only;
/// names filtered by `is_level_param`. Each node's id is its `nodeId` (falling
/// back to `FenderId`). Tolerant of missing branches.
pub(crate) fn extract_level_blocks(v: &serde_json::Value) -> Vec<LevelBlock> {
    let mut out = Vec::new();
    let Some(groups) = v
        .pointer("/audioGraph/guitarNodes")
        .and_then(|g| g.as_object())
    else {
        return out;
    };
    for (group_id, nodes) in groups {
        let Some(nodes) = nodes.as_array() else {
            continue;
        };
        for node in nodes {
            let node_id = node
                .get("nodeId")
                .and_then(|x| x.as_str())
                .or_else(|| node.get("FenderId").and_then(|x| x.as_str()))
                .unwrap_or("");
            if node_id.is_empty() {
                continue;
            }
            let model_id = node
                .get("FenderId")
                .and_then(|x| x.as_str())
                .unwrap_or(node_id);
            let Some(params) = node.get("dspUnitParameters").and_then(|p| p.as_object()) else {
                continue;
            };
            for (pname, pval) in params {
                if let Some(f) = pval.as_f64() {
                    if is_level_param(pname) {
                        out.push(LevelBlock {
                            group_id: group_id.clone(),
                            node_id: node_id.to_string(),
                            model_id: model_id.to_string(),
                            parameter_id: pname.clone(),
                            value: f as f32,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Walk decoded preset JSON into an [`ActiveGraph`] for the active-preset signal chain strip: every
/// node under `audioGraph.{guitarNodes,micNodes}.<group>[]` (model id + bypass),
/// in stable sorted-group then array order, plus `audioGraph.template` for
/// routing and `audioGraph.splitMix` for its controls. Tolerant of every missing branch — a
/// truncated partial that only reached `guitarNodes` still yields its nodes.
pub(crate) fn extract_active_graph(
    v: &serde_json::Value,
    template_hint: Option<&str>,
) -> ActiveGraph {
    // Collect each graph's group slots in sorted-key order (G1..G7, M1..M4).
    // A BTreeMap keeps the deterministic slot order the stage mapper relies on.
    let parse_groups = |graph: &str| -> std::collections::BTreeMap<String, Vec<GraphNode>> {
        let mut out = std::collections::BTreeMap::new();
        let Some(groups) = v
            .pointer(&format!("/audioGraph/{graph}"))
            .and_then(|g| g.as_object())
        else {
            return out;
        };
        for (group_id, val) in groups {
            let Some(arr) = val.as_array() else { continue };
            let mut blocks = Vec::new();
            for node in arr {
                let model = node
                    .get("FenderId")
                    .and_then(|x| x.as_str())
                    .or_else(|| node.get("nodeId").and_then(|x| x.as_str()))
                    .unwrap_or("");
                if model.is_empty() {
                    continue;
                }
                let node_id = node.get("nodeId").and_then(|x| x.as_str()).unwrap_or(model);
                let params = node.get("dspUnitParameters");
                let bypassed = params
                    .and_then(|p| p.get("bypass"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                // Cab-sim params. `cabsimid` NAMES the cab — present on the standalone
                // CabSim block AND on amp combos/half-stacks (their built-in cab), so the
                // strip can draw a head-over-cab half-stack. The dual-cab SPLIT fields
                // (cab2simid / cabsim2enabled) are a real SECOND parallel cab ONLY on the
                // standalone CabSim block; on an amp the same keys mean a dual MIC on ONE
                // cab — suppress them there so the strip never splits a half-stack amp.
                // Keys are the device's exact lowercase `dspUnitParameters` names.
                let is_cab_block = model == "ACD_CabSimTMS";
                let cab_sim_id = params
                    .and_then(|p| p.get("cabsimid"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let dual = params.filter(|_| is_cab_block);
                let cab_sim_id2 = dual
                    .and_then(|p| p.get("cab2simid"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let cab_sim2_enabled = dual
                    .and_then(|p| p.get("cabsim2enabled"))
                    .and_then(|b| b.as_bool());
                // Doctor's allowlist: reverb mix names + cab low/high cut
                // (hpf/lpf) + EQ-10 band gains (see the GraphNode.params doc) —
                // numeric values only.
                let keep = |k: &str| {
                    k == "mix"
                        || k == "wetdrymix"
                        || k == "hpf"
                        || k == "lpf"
                        || (k.starts_with("gain") && k.ends_with("hz"))
                };
                let node_params: std::collections::HashMap<String, f64> = params
                    .and_then(|p| p.as_object())
                    .map(|o| {
                        o.iter()
                            .filter(|(k, _)| keep(k))
                            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                            .collect()
                    })
                    .unwrap_or_default();
                blocks.push(GraphNode {
                    group_id: group_id.clone(),
                    node_id: node_id.to_string(),
                    model: model.to_string(),
                    bypassed,
                    cab_sim_id,
                    cab_sim_id2,
                    cab_sim2_enabled,
                    params: node_params,
                });
            }
            if !blocks.is_empty() {
                out.insert(group_id.clone(), blocks);
            }
        }
        out
    };

    let guitar = parse_groups("guitarNodes");
    let mic = parse_groups("micNodes");

    // Flat list (every block, guitar then mic, in slot+array order) — back-compat.
    let nodes: Vec<GraphNode> = guitar
        .values()
        .chain(mic.values())
        .flat_map(|b| b.iter().cloned())
        .collect();

    let split_mix = v.pointer("/audioGraph/splitMix").cloned();
    let template = v
        .pointer("/audioGraph/template")
        .and_then(|x| x.as_str())
        .or(template_hint)
        .map(str::to_string);
    let route = build_route_graph(template.as_deref(), &guitar, &mic);

    ActiveGraph {
        name: v
            .pointer("/info/displayName")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        slot: v
            .pointer("/info/userSlot")
            .and_then(|x| x.as_u64())
            .and_then(|s| (s as u32).checked_sub(1)),
        template,
        split_mix,
        nodes,
        input_type: route.input_type,
        output_type: route.output_type,
        inputs: route.inputs,
        outputs: route.outputs,
        lanes: route.lanes,
        stages: route.stages,
    }
}

pub(crate) fn is_known_routing_template(template: Option<&str>) -> bool {
    matches!(
        template,
        Some(
            "gtrSeries"
                | "gtrParallel1"
                | "gtrParallel2"
                | "gtrMicSeries"
                | "micSeries"
                | "micParallel1"
                | "gtrMicParallel"
                | "gtrMicMix"
                | "gtrMicMix2"
                | "gtrMicMix3"
                | "gtrSplit"
                | "micSplit"
        )
    )
}

fn build_route_graph(
    template: Option<&str>,
    guitar: &std::collections::BTreeMap<String, Vec<GraphNode>>,
    mic: &std::collections::BTreeMap<String, Vec<GraphNode>>,
) -> RouteGraph {
    let g = |key: &str| guitar.get(key).cloned().unwrap_or_default();
    let m = |key: &str| mic.get(key).cloned().unwrap_or_default();
    let concat_g = |keys: &[&str]| -> Vec<GraphNode> { keys.iter().flat_map(|k| g(k)).collect() };
    let concat_m = |keys: &[&str]| -> Vec<GraphNode> { keys.iter().flat_map(|k| m(k)).collect() };
    let all_g = || concat_g(&["G1", "G2", "G3", "G4", "G5", "G6", "G7"]);
    let all_m = || concat_m(&["M1", "M2", "M3", "M4"]);
    let mut route = RouteGraph {
        input_type: Some("guitar".to_string()),
        output_type: Some("out".to_string()),
        inputs: None,
        outputs: None,
        lanes: None,
        stages: Vec::new(),
    };

    match template {
        Some("micSeries") => {
            route.input_type = Some("mic".to_string());
            push_series(&mut route.stages, all_m());
        }
        Some("micParallel1") => {
            route.input_type = Some("mic".to_string());
            push_series(&mut route.stages, m("M1"));
            push_split(&mut route.stages, m("M2"), m("M3"));
            push_series(&mut route.stages, m("M4"));
        }
        Some("gtrParallel1") => {
            push_series(&mut route.stages, g("G1"));
            push_split(&mut route.stages, g("G2"), g("G3"));
            push_series(&mut route.stages, concat_g(&["G4", "G5", "G6", "G7"]));
        }
        Some("gtrParallel2") => {
            push_series(&mut route.stages, g("G1"));
            push_split(&mut route.stages, g("G2"), g("G3"));
            push_series(&mut route.stages, g("G4"));
            push_split(&mut route.stages, g("G5"), g("G6"));
            push_series(&mut route.stages, g("G7"));
        }
        Some("gtrMicSeries") => {
            route.input_type = None;
            route.inputs = Some(InputPair {
                a: InputLane {
                    kind: "guitar".to_string(),
                    blocks: Vec::new(),
                },
                b: InputLane {
                    kind: "mic".to_string(),
                    blocks: Vec::new(),
                },
            });
            push_series(
                &mut route.stages,
                all_g().into_iter().chain(all_m()).collect(),
            );
        }
        Some("gtrMicMix" | "gtrMicMix2" | "gtrMicMix3") => {
            route.input_type = None;
            route.inputs = Some(InputPair {
                a: InputLane {
                    kind: "guitar".to_string(),
                    blocks: all_g(),
                },
                b: InputLane {
                    kind: "mic".to_string(),
                    blocks: all_m(),
                },
            });
        }
        Some("gtrMicParallel") => {
            route.input_type = None;
            route.output_type = None;
            route.lanes = Some(vec![
                IndependentLane {
                    input: "guitar".to_string(),
                    output: "out1".to_string(),
                    blocks: all_g(),
                },
                IndependentLane {
                    input: "mic".to_string(),
                    output: "out2".to_string(),
                    blocks: all_m(),
                },
            ]);
        }
        Some("gtrSplit") => {
            // One array, read once here and passed to the unmapped-group check below
            // — a single source of truth for "which groups this arm reads" instead of
            // a second hand-maintained literal that could silently drift from it.
            const USED: [&str; 3] = ["G1", "G2", "G3"];
            push_series(&mut route.stages, g(USED[0]));
            route.output_type = None;
            route.outputs = Some(OutputPair {
                a: OutputLane {
                    kind: "out1".to_string(),
                    blocks: g(USED[1]),
                },
                b: OutputLane {
                    kind: "out2".to_string(),
                    blocks: g(USED[2]),
                },
            });
            warn_unmapped_groups("gtrSplit", guitar, &USED);
        }
        Some("micSplit") => {
            const USED: [&str; 3] = ["M1", "M2", "M3"];
            route.input_type = Some("mic".to_string());
            route.output_type = None;
            push_series(&mut route.stages, m(USED[0]));
            route.outputs = Some(OutputPair {
                a: OutputLane {
                    kind: "out1".to_string(),
                    blocks: m(USED[1]),
                },
                b: OutputLane {
                    kind: "out2".to_string(),
                    blocks: m(USED[2]),
                },
            });
            warn_unmapped_groups("micSplit", mic, &USED);
        }
        _ => push_series(&mut route.stages, all_g()),
    }

    route
}

/// Append a series stage, dropping it when there are no blocks.
fn push_series(stages: &mut Vec<Stage>, blocks: Vec<GraphNode>) {
    if !blocks.is_empty() {
        stages.push(Stage::Series { blocks });
    }
}

/// Append a split stage only when at least one lane has blocks (an all-empty
/// split slot pair is dropped rather than drawn as an empty fork).
fn push_split(stages: &mut Vec<Stage>, a: Vec<GraphNode>, b: Vec<GraphNode>) {
    if !a.is_empty() || !b.is_empty() {
        stages.push(Stage::Split { a, b });
    }
}

/// gtrSplit/micSplit only read a fixed 3-group subset (G1-G3 / M1-M3) — the
/// same shape of unverified "which groups does this template read" assumption
/// that bunched the wrong groups per lane before it was HW-corrected. Warn
/// (don't fail) if a device payload carries blocks in a group this arm never
/// reads, so a future firmware/template surprise shows up in the logs instead
/// of silently vanishing from the rendered strip.
fn warn_unmapped_groups(
    template: &str,
    groups: &std::collections::BTreeMap<String, Vec<GraphNode>>,
    used: &[&str],
) {
    for (group_id, blocks) in groups {
        if !blocks.is_empty() && !used.contains(&group_id.as_str()) {
            log::warn!(
                "build_route_graph: {template} has {} block(s) in unmapped group {group_id} \
                 — not rendered in the signal-chain strip",
                blocks.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model ids of a series stage, in order. Panics on a non-series stage
    /// (a test asserting the wrong `Stage` variant should fail loudly, not
    /// silently mismatch).
    fn stage_series(s: &Stage) -> Vec<String> {
        match s {
            Stage::Series { blocks } => blocks.iter().map(|b| b.model.clone()).collect(),
            _ => panic!("expected series, got {s:?}"),
        }
    }

    /// The model ids of each split lane, in order.
    fn stage_split(s: &Stage) -> (Vec<String>, Vec<String>) {
        match s {
            Stage::Split { a, b } => (
                a.iter().map(|x| x.model.clone()).collect(),
                b.iter().map(|x| x.model.clone()).collect(),
            ),
            _ => panic!("expected split, got {s:?}"),
        }
    }

    /// Records every body the handshake sends; replies are empty (the handshake
    /// never parses them inline — it only accumulates).
    struct RecordingTransport(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);
    impl crate::hid::HidTransport for RecordingTransport {
        fn send(&self, body: &[u8]) -> Result<(), String> {
            self.0.lock().unwrap().push(body.to_vec());
            Ok(())
        }
        fn transact(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.0.lock().unwrap().push(body.to_vec());
            Ok(Vec::new())
        }
        fn transact_chunked(&self, body: &[u8], _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.0.lock().unwrap().push(body.to_vec());
            Ok(Vec::new())
        }
        fn pump(&self, _pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
            Ok(Vec::new())
        }
        fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.transact(body, max_ms)
        }
    }

    fn handshake_sends(lean: bool) -> Vec<Vec<u8>> {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = Session {
            hid: Box::new(RecordingTransport(sent.clone())),
            batch: 0,
            raw: Vec::new(),
            fw_version: None,
        };
        s.handshake(false, None, false, lean).unwrap();
        // A binding, not a tail expression: the guard temporary must drop before `s`
        // (E0597 otherwise).
        let sends = sent.lock().unwrap().clone();
        sends
    }

    /// The lean handshake trims only the PUMP WINDOWS — the request byte sequence
    /// must stay identical to the full handshake (the device only answers after
    /// seeing the exact captured Pro Control sequence).
    #[test]
    fn lean_handshake_sends_the_identical_request_sequence() {
        assert_eq!(handshake_sends(false), handshake_sends(true));
    }

    #[test]
    fn name_fallback_only_confirms_a_uniquely_mapped_slot() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Unique name at the target slot ⇒ the fallback may confirm.
        assert!(name_maps_uniquely(
            &names(&["Cliff", "Target", "Lead"]),
            "Target",
            1
        ));
        // Same name in two slots ⇒ can't prove which is the target ⇒ fail closed.
        assert!(!name_maps_uniquely(
            &names(&["Target", "Lead", "Target"]),
            "Target",
            0
        ));
        // Name present but at a DIFFERENT slot than claimed ⇒ fail closed.
        assert!(!name_maps_uniquely(
            &names(&["Cliff", "Target"]),
            "Target",
            0
        ));
        // Name absent ⇒ fail closed.
        assert!(!name_maps_uniquely(&names(&["Cliff", "Lead"]), "Target", 0));
    }

    #[test]
    fn extract_active_graph_reads_nodes_and_routing() {
        // A parallel preset: two amp groups + a top-level template + splitMix.
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"Instrument Parallel 1",
                "splitMix":{"mode":"even"},
                "guitarNodes":{
                    "G2":[{"FenderId":"ACD_TweedDeluxeCabIR","nodeId":"ACD_TweedDeluxeCabIR","dspUnitParameters":{"bypass":false,"instvolume":0.64}}],
                    "G3":[{"FenderId":"ACD_PrincetonReverb65NoFxCabIR","dspUnitParameters":{"bypass":true}}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.template.as_deref(), Some("Instrument Parallel 1"));
        assert!(g.split_mix.is_some());
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].group_id, "G2");
        assert_eq!(g.nodes[0].model, "ACD_TweedDeluxeCabIR");
        assert!(!g.nodes[0].bypassed);
        assert_eq!(g.nodes[1].group_id, "G3");
        assert!(g.nodes[1].bypassed);
        // No `info` in this partial → honest None (no fabricated name/slot).
        assert_eq!(g.name, None);
        assert_eq!(g.slot, None);
    }

    #[test]
    fn extract_active_graph_reads_dual_cab_params() {
        // A CabSim block carrying a dual cab, serialized exactly as the device does
        // (literal lowercase keys cabsimid / cab2simid / cabsim2enabled).
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{
                "G1":[
                    {"FenderId":"ACD_HiwattDR103CanMod","dspUnitParameters":{"bypass":false}},
                    {"FenderId":"ACD_CabSimTMS","dspUnitParameters":{"bypass":false,"cabsimid":"Mar1960aV30Alt","cab2simid":"Mar1960aV30Alt","cabsim2enabled":true,"dualcabblend":0.4}}
                ]}}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.nodes.len(), 2);
        // The bare amp head carries no cab params.
        assert_eq!(g.nodes[0].cab_sim_id, None);
        assert_eq!(g.nodes[0].cab_sim2_enabled, None);
        // The CabSim carries both cabinets + the dual flag.
        assert_eq!(g.nodes[1].model, "ACD_CabSimTMS");
        assert_eq!(g.nodes[1].cab_sim_id.as_deref(), Some("Mar1960aV30Alt"));
        assert_eq!(g.nodes[1].cab_sim_id2.as_deref(), Some("Mar1960aV30Alt"));
        assert_eq!(g.nodes[1].cab_sim2_enabled, Some(true));
    }

    #[test]
    fn extract_active_graph_amp_half_stack_keeps_cab_id_but_no_split() {
        // Preset 003 (Cello): a HALF-STACK amp node (`...CabIR`) carries cabsim params.
        // `cabsimid` names its built-in cab (so the strip can draw a head-over-cab
        // half-stack), but `cabsim2enabled` here means DUAL MIC on the SAME cab (mic
        // sm57 + miccab2 r121, one cabinet) — NOT a dual cab — so the split fields must
        // stay None (only the standalone `ACD_CabSimTMS` block carries a real 2nd cab).
        // Params copied verbatim from the live device dump.
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{
                "G1":[
                    {"FenderId":"ACD_HiwattDR103CanModCabIR","dspUnitParameters":{"bypass":false,"cab2simid":"Mar1960aV30Alt","cabsim2enabled":true,"cabsimid":"Mar1960aV30Alt","dualcabblend":0.5,"mic":"sm57","miccab2":"r121"}}
                ]}}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].model, "ACD_HiwattDR103CanModCabIR");
        // The amp keeps its cab id (for the half-stack art)…
        assert_eq!(g.nodes[0].cab_sim_id.as_deref(), Some("Mar1960aV30Alt"));
        // …but the dual-cab SPLIT fields are suppressed (dual-mic, not dual-cab).
        assert_eq!(g.nodes[0].cab_sim_id2, None);
        assert_eq!(g.nodes[0].cab_sim2_enabled, None);
    }

    #[test]
    fn scene_names_from_slot_json_recovers_truncated_partial() {
        // Field-9 plaintext partial shaped like the HW reads (probe --slotread-x):
        // device-truncated INSIDE the final scene's `uuid`, after every sceneName.
        let json = br#"{"info":{"displayName":"Guitar"},"audioGraph":{"guitarNodes":{}},"scenes":[{"sceneName":"Dist","uuid":"aaa"},{"sceneName":"Celestial","uuid":"bbb"},{"sceneName":"Swell","uuid":"cc"#;
        let names = scene_names_from_slot_json(json).expect("tolerant parse");
        // The unwind cuts at the last comma INSIDE the truncated scene object —
        // after its sceneName — so ALL names survive, the final uuid doesn't
        // (matches HW: slot 1 = 17264 B, 8/8 names present, cut mid-uuid).
        assert_eq!(
            names,
            vec![
                "Dist".to_string(),
                "Celestial".to_string(),
                "Swell".to_string()
            ]
        );

        // Complete document: all names, slot order.
        let json = br#"{"scenes":[{"sceneName":"A"},{"sceneName":"B"}],"lastLoadedScene":0}"#;
        assert_eq!(
            scene_names_from_slot_json(json).unwrap(),
            vec!["A".to_string(), "B".to_string()]
        );

        // No scenes key (truncated before it) → None, NOT Some([]).
        assert_eq!(
            scene_names_from_slot_json(br#"{"info":{"displayName":"Cello"}"#),
            None
        );

        // Empty scenes array → Some([]) (definitely no scenes).
        assert_eq!(
            scene_names_from_slot_json(br#"{"scenes":[]}"#),
            Some(vec![])
        );
    }

    #[test]
    fn extract_active_graph_tolerates_truncation() {
        // Truncated partial: audioGraph present but no template/splitMix/info.
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{"guitarNodes":{"G1":[{"FenderId":"ACD_TM59Bassman"}]}}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.template, None);
        assert_eq!(g.split_mix, None);
        // No splitMix → a single series stage holding the one block.
        assert_eq!(
            g.stages,
            vec![Stage::Series {
                blocks: g.nodes.clone()
            }]
        );
    }

    #[test]
    fn extract_active_graph_normalizes_one_based_user_slot() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"info":{"displayName":"Lead","userSlot":4}}"#).unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.name.as_deref(), Some("Lead"));
        assert_eq!(g.slot, Some(3));
    }

    #[test]
    fn extract_partial_template_from_truncated_json() {
        let text = r#"{"audioGraph":{"guitarNodes":{},"splitMix":{},"template":"gtrParallel2"#;
        assert_eq!(
            extract_partial_json_string(text, "template").as_deref(),
            Some("gtrParallel2")
        );
        assert!(is_known_routing_template(Some("gtrParallel2")));
        assert!(!is_known_routing_template(Some("gtrPara")));
    }

    #[test]
    fn current_preset_events_supply_name_and_zero_based_user_slot() {
        assert_eq!(
            extract_current_preset_display_name(b"\x12\x09\xb2\x01\x06\x0a\x04Lead").as_deref(),
            Some("Lead")
        );
        assert_eq!(
            extract_loaded_user_slot(b"\x12\x06\x5a\x04\x08\x01\x30\x04"),
            Some(3)
        );
        assert_eq!(
            extract_loaded_user_slot(b"\x12\x06\x5a\x04\x08\x04\x30\x04"),
            None
        );
    }

    #[test]
    fn name_fallback_requires_a_unique_my_presets_match() {
        let presets = vec![
            PresetEntry {
                slot: 0,
                name: "Clean".to_string(),
            },
            PresetEntry {
                slot: 1,
                name: "Lead".to_string(),
            },
            PresetEntry {
                slot: 2,
                name: "Lead".to_string(),
            },
        ];
        assert_eq!(unique_preset_slot_by_name(&presets, "Clean"), Some(0));
        assert_eq!(unique_preset_slot_by_name(&presets, "Lead"), None);
        assert_eq!(unique_preset_slot_by_name(&presets, "Missing"), None);
    }

    // Pin the routing→stages mapping against REAL device-shaped JSON (template +
    // 7 guitar slots + splitMix control inventory), not a hardcoded preset. This is the
    // dual-split path from the handoff: XO Boost → split[57 Deluxe ‖ 65 Princeton]
    // → Space Echo → split[Small Hall ‖ Filtron] → Small Hall. Each series block
    // before/between/after the splits must keep its series role; both splits must
    // survive with the right lane assignment.
    #[test]
    fn build_stages_dual_split_assigns_roles() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrParallel2",
                "splitMix":{"splitPoints":[{"nodeId":"split1"},{"nodeId":"split2"}],
                            "mixPoints":[{"nodeId":"mix1"},{"nodeId":"mix2"}]},
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_EPBooster","nodeId":"n1"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe","nodeId":"n2"}],
                    "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx","nodeId":"n3"}],
                    "G4":[{"FenderId":"ACD_SpaceEcho","nodeId":"n4"}],
                    "G5":[{"FenderId":"ACD_TMSmallHall","nodeId":"n5"}],
                    "G6":[{"FenderId":"ACD_MicroTronIV","nodeId":"n6"}],
                    "G7":[{"FenderId":"ACD_TMSmallHall","nodeId":"n7"}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);

        assert_eq!(g.stages.len(), 5, "pre · split1 · mid · split2 · post");
        // 1. XO Boost stays in SERIES before the first split (the old bug pulled it
        //    into a lane).
        assert_eq!(stage_series(&g.stages[0]), vec!["ACD_EPBooster"]);
        // 2. First split: lane A = 57 Deluxe, lane B = 65 Princeton (not shifted).
        assert_eq!(
            stage_split(&g.stages[1]),
            (
                vec!["ACD_TweedDeluxe".into()],
                vec!["ACD_PrincetonReverb65NoFx".into()]
            )
        );
        // 3. Space Echo runs in SERIES between the two splits.
        assert_eq!(stage_series(&g.stages[2]), vec!["ACD_SpaceEcho"]);
        // 4. SECOND split survives (the old bug flattened it): Small Hall ‖ Filtron.
        assert_eq!(
            stage_split(&g.stages[3]),
            (
                vec!["ACD_TMSmallHall".into()],
                vec!["ACD_MicroTronIV".into()]
            )
        );
        // 5. Trailing Small Hall stays in series after the last mix.
        assert_eq!(stage_series(&g.stages[4]), vec!["ACD_TMSmallHall"]);
    }

    #[test]
    fn build_stages_single_split_pre_and_tail() {
        // One split: G1 pre-series, G2/G3 lanes, G4..G7 post-series tail. Real
        // payloads carry all three split controls regardless of active template.
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrParallel1",
                "splitMix":{"splitPoints":[{"nodeId":"split1"},{"nodeId":"split2"},{"nodeId":"split3"}],
                            "mixPoints":[{"nodeId":"mix1"},{"nodeId":"mix2"},{"nodeId":"mix3"}]},
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_KlonCentaur"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe"}],
                    "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"}],
                    "G4":[{"FenderId":"ACD_SpaceEcho"}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.stages.len(), 3);
        assert!(
            matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_KlonCentaur")
        );
        assert!(matches!(&g.stages[1], Stage::Split { a, b }
            if a[0].model == "ACD_TweedDeluxe" && b[0].model == "ACD_PrincetonReverb65NoFx"));
        assert!(
            matches!(&g.stages[2], Stage::Series { blocks } if blocks[0].model == "ACD_SpaceEcho")
        );
    }

    #[test]
    fn build_stages_series_when_no_split_points() {
        // A series preset: no splitPoints → one series run of all blocks in order.
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrSeries",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_TubeScreamer"},{"FenderId":"ACD_PlexiAmp"}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.stages.len(), 1);
        assert!(matches!(&g.stages[0], Stage::Series { blocks } if blocks.len() == 3));
    }

    #[test]
    fn route_graph_dual_inputs_join_before_series_tail() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrMicSeries",
                "guitarNodes":{"G1":[{"FenderId":"ACD_Compressor"}]},
                "micNodes":{"M1":[{"FenderId":"ACD_StudioPreamp"}]}
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);

        let inputs = g.inputs.expect("gtrMicSeries should expose a dual input");
        assert_eq!(inputs.a.kind, "guitar");
        assert_eq!(inputs.b.kind, "mic");
        assert!(inputs.a.blocks.is_empty());
        assert!(inputs.b.blocks.is_empty());
        assert_eq!(g.input_type, None);
        assert_eq!(g.output_type.as_deref(), Some("out"));
        assert!(matches!(&g.stages[0], Stage::Series { blocks }
            if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
                == vec!["ACD_Compressor", "ACD_StudioPreamp"]));
    }

    #[test]
    fn route_graph_gtr_mic_parallel_uses_independent_rails() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrMicParallel",
                "guitarNodes":{"G1":[{"FenderId":"ACD_Compressor"}]},
                "micNodes":{"M1":[{"FenderId":"ACD_StudioPreamp"}]}
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);

        assert!(g.stages.is_empty());
        assert_eq!(g.input_type, None);
        assert_eq!(g.output_type, None);
        let lanes = g.lanes.expect("parallel gtr/mic should expose rails");
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].input, "guitar");
        assert_eq!(lanes[0].output, "out1");
        assert_eq!(lanes[0].blocks[0].model, "ACD_Compressor");
        assert_eq!(lanes[1].input, "mic");
        assert_eq!(lanes[1].output, "out2");
        assert_eq!(lanes[1].blocks[0].model, "ACD_StudioPreamp");
    }

    #[test]
    fn route_graph_split_outputs_label_out1_and_out2() {
        let gtr: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrSplit",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_Compressor"}],
                    "G2":[{"FenderId":"ACD_DeluxeReverb"}],
                    "G3":[{"FenderId":"ACD_SpaceEcho"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&gtr, None);

        assert_eq!(g.output_type, None);
        assert!(
            matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_Compressor")
        );
        let outputs = g.outputs.expect("gtrSplit should expose split outputs");
        assert_eq!(outputs.a.kind, "out1");
        assert_eq!(outputs.a.blocks[0].model, "ACD_DeluxeReverb");
        assert_eq!(outputs.b.kind, "out2");
        assert_eq!(outputs.b.blocks[0].model, "ACD_SpaceEcho");

        let mic: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"micSplit",
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M2":[{"FenderId":"ACD_LA2AComp"}],
                    "M3":[{"FenderId":"ACD_RoomVerb"}]
                }
            }}"#,
        )
        .unwrap();
        let m = extract_active_graph(&mic, None);
        assert_eq!(m.input_type.as_deref(), Some("mic"));
        let m_outputs = m.outputs.expect("micSplit should expose split outputs");
        assert_eq!(m_outputs.a.kind, "out1");
        assert_eq!(m_outputs.b.kind, "out2");
    }

    // HW-confirmed (My Presets slot 27 "Split outputs", 65 Deluxe Reverb → SPLIT):
    // a device group is itself an ordered mini-chain that can hold multiple blocks
    // (adding a 2nd effect in Pro Control landed inside G2's own array, not G4), so
    // gtrSplit/micSplit assign ONE WHOLE GROUP per output lane (G2→out1, G3→out2),
    // not a bunched multi-group half like the old ["G2","G3","G4"]/["G5","G6","G7"].
    #[test]
    fn route_graph_gtr_split_lane_is_one_full_group_in_order() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrSplit",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_DeluxeReverb65NoFx"}],
                    "G2":[{"FenderId":"ACD_UserIRTMS"},{"FenderId":"ACD_TMSmallHall"}],
                    "G3":[{"FenderId":"ACD_ExternalCab"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        let outputs = g.outputs.expect("gtrSplit should expose split outputs");
        assert_eq!(outputs.a.kind, "out1");
        assert_eq!(
            outputs
                .a
                .blocks
                .iter()
                .map(|n| n.model.clone())
                .collect::<Vec<_>>(),
            vec!["ACD_UserIRTMS", "ACD_TMSmallHall"]
        );
        assert_eq!(outputs.b.kind, "out2");
        assert_eq!(outputs.b.blocks[0].model, "ACD_ExternalCab");
    }

    #[test]
    fn route_graph_mic_series_runs_all_groups_in_series() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"micSeries",
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M2":[{"FenderId":"ACD_LA2AComp"}],
                    "M3":[{"FenderId":"ACD_RoomVerb"}],
                    "M4":[{"FenderId":"ACD_TMSmallHall"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.input_type.as_deref(), Some("mic"));
        assert_eq!(g.stages.len(), 1);
        assert!(matches!(&g.stages[0], Stage::Series { blocks }
            if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
                == vec!["ACD_StudioPreamp", "ACD_LA2AComp", "ACD_RoomVerb", "ACD_TMSmallHall"]));
    }

    #[test]
    fn route_graph_mic_parallel1_splits_m2_m3_around_m1_and_m4() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"micParallel1",
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M2":[{"FenderId":"ACD_LA2AComp"}],
                    "M3":[{"FenderId":"ACD_RoomVerb"}],
                    "M4":[{"FenderId":"ACD_TMSmallHall"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.stages.len(), 3);
        assert!(
            matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_StudioPreamp")
        );
        assert!(matches!(&g.stages[1], Stage::Split { a, b }
            if a[0].model == "ACD_LA2AComp" && b[0].model == "ACD_RoomVerb"));
        assert!(
            matches!(&g.stages[2], Stage::Series { blocks } if blocks[0].model == "ACD_TMSmallHall")
        );
    }

    // gtrParallel1 with EVERY group multi/populated (G1/G2 two blocks each, G3 one,
    // G4..G7 all populated) — the direct regression guard for the old bug's
    // tail-concat path silently bunching/dropping groups beyond the split lanes.
    #[test]
    fn build_stages_single_split_multi_block_groups_and_full_tail() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrParallel1",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe"},{"FenderId":"ACD_KlonCentaur"}],
                    "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"}],
                    "G4":[{"FenderId":"ACD_SpaceEcho"}],
                    "G5":[{"FenderId":"ACD_TMSmallHall"}],
                    "G6":[{"FenderId":"ACD_MicroTronIV"}],
                    "G7":[{"FenderId":"ACD_RoomVerb"}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.stages.len(), 3);
        assert!(matches!(&g.stages[0], Stage::Series { blocks }
            if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
                == vec!["ACD_Compressor", "ACD_EPBooster"]));
        assert!(matches!(&g.stages[1], Stage::Split { a, b }
            if a.iter().map(|x| x.model.as_str()).collect::<Vec<_>>() == vec!["ACD_TweedDeluxe", "ACD_KlonCentaur"]
                && b.iter().map(|x| x.model.as_str()).collect::<Vec<_>>() == vec!["ACD_PrincetonReverb65NoFx"]));
        assert!(matches!(&g.stages[2], Stage::Series { blocks }
            if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
                == vec!["ACD_SpaceEcho", "ACD_TMSmallHall", "ACD_MicroTronIV", "ACD_RoomVerb"]));
    }

    // Same shape as `build_stages_dual_split_assigns_roles` but every group carries
    // 2 blocks — proves a populated LANE (not just a populated group) preserves its
    // own internal per-group order.
    #[test]
    fn build_stages_dual_split_multi_block_per_group() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrParallel2",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_EPBooster"},{"FenderId":"ACD_Compressor"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe"},{"FenderId":"ACD_KlonCentaur"}],
                    "G3":[{"FenderId":"ACD_PrincetonReverb65NoFx"},{"FenderId":"ACD_StudioPreamp"}],
                    "G4":[{"FenderId":"ACD_SpaceEcho"},{"FenderId":"ACD_RoomVerb"}],
                    "G5":[{"FenderId":"ACD_TMSmallHall"},{"FenderId":"ACD_LA2AComp"}],
                    "G6":[{"FenderId":"ACD_MicroTronIV"},{"FenderId":"ACD_UserIRTMS"}],
                    "G7":[{"FenderId":"ACD_TMSmallHall"},{"FenderId":"ACD_ExternalCab"}]
                }}}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert_eq!(g.stages.len(), 5);
        assert_eq!(
            stage_series(&g.stages[0]),
            vec!["ACD_EPBooster", "ACD_Compressor"]
        );
        assert_eq!(
            stage_split(&g.stages[1]),
            (
                vec!["ACD_TweedDeluxe".into(), "ACD_KlonCentaur".into()],
                vec![
                    "ACD_PrincetonReverb65NoFx".into(),
                    "ACD_StudioPreamp".into()
                ]
            )
        );
        assert_eq!(
            stage_series(&g.stages[2]),
            vec!["ACD_SpaceEcho", "ACD_RoomVerb"]
        );
        assert_eq!(
            stage_split(&g.stages[3]),
            (
                vec!["ACD_TMSmallHall".into(), "ACD_LA2AComp".into()],
                vec!["ACD_MicroTronIV".into(), "ACD_UserIRTMS".into()]
            )
        );
        assert_eq!(
            stage_series(&g.stages[4]),
            vec!["ACD_TMSmallHall", "ACD_ExternalCab"]
        );
    }

    #[test]
    fn route_graph_dual_inputs_join_multi_block_per_group() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrMicSeries",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe"}]
                },
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M2":[{"FenderId":"ACD_LA2AComp"},{"FenderId":"ACD_RoomVerb"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert!(matches!(&g.stages[0], Stage::Series { blocks }
            if blocks.iter().map(|b| b.model.as_str()).collect::<Vec<_>>()
                == vec!["ACD_Compressor", "ACD_EPBooster", "ACD_TweedDeluxe",
                        "ACD_StudioPreamp", "ACD_LA2AComp", "ACD_RoomVerb"]));
    }

    // The three gtrMicMix aliases had ZERO coverage before this test — they carry
    // NO stages at all (unlike gtrMicSeries), just the two full input rails.
    #[test]
    fn route_graph_gtr_mic_mix_aliases_expose_full_groups_as_inputs() {
        for template in ["gtrMicMix", "gtrMicMix2", "gtrMicMix3"] {
            let v: serde_json::Value = serde_json::from_str(&format!(
                r#"{{"audioGraph":{{
                    "template":"{template}",
                    "guitarNodes":{{"G1":[{{"FenderId":"ACD_Compressor"}}]}},
                    "micNodes":{{"M1":[{{"FenderId":"ACD_StudioPreamp"}}]}}
                }}}}"#
            ))
            .unwrap();
            let g = extract_active_graph(&v, None);
            assert!(
                g.stages.is_empty(),
                "{template} should carry no stages, only inputs"
            );
            let inputs = g.inputs.expect("should expose dual inputs");
            assert_eq!(inputs.a.kind, "guitar");
            assert_eq!(inputs.a.blocks[0].model, "ACD_Compressor");
            assert_eq!(inputs.b.kind, "mic");
            assert_eq!(inputs.b.blocks[0].model, "ACD_StudioPreamp");
            assert_eq!(g.outputs, None);
            assert_eq!(g.lanes, None);
        }
    }

    #[test]
    fn route_graph_gtr_mic_parallel_multi_block_rails() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"gtrMicParallel",
                "guitarNodes":{
                    "G1":[{"FenderId":"ACD_Compressor"},{"FenderId":"ACD_EPBooster"}],
                    "G2":[{"FenderId":"ACD_TweedDeluxe"}]
                },
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M3":[{"FenderId":"ACD_RoomVerb"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        let lanes = g.lanes.expect("gtrMicParallel should expose rails");
        assert_eq!(
            lanes[0]
                .blocks
                .iter()
                .map(|b| b.model.clone())
                .collect::<Vec<_>>(),
            vec!["ACD_Compressor", "ACD_EPBooster", "ACD_TweedDeluxe"]
        );
        assert_eq!(
            lanes[1]
                .blocks
                .iter()
                .map(|b| b.model.clone())
                .collect::<Vec<_>>(),
            vec!["ACD_StudioPreamp", "ACD_RoomVerb"]
        );
    }

    // The micSplit sibling of `route_graph_gtr_split_lane_is_one_full_group_in_order`
    // — the same asymmetric multi-block-per-lane guard, applied to the ONE split
    // template that never got it (its own existing test only covered a
    // single-block-per-lane shape, which is exactly the shape that hid the
    // original bug on gtrSplit until a real preset exposed it).
    #[test]
    fn route_graph_mic_split_lane_is_one_full_group_in_order() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"audioGraph":{
                "template":"micSplit",
                "micNodes":{
                    "M1":[{"FenderId":"ACD_StudioPreamp"}],
                    "M2":[{"FenderId":"ACD_LA2AComp"},{"FenderId":"ACD_RoomVerb"}],
                    "M3":[{"FenderId":"ACD_TMSmallHall"}]
                }
            }}"#,
        )
        .unwrap();
        let g = extract_active_graph(&v, None);
        assert!(
            matches!(&g.stages[0], Stage::Series { blocks } if blocks[0].model == "ACD_StudioPreamp")
        );
        let outputs = g.outputs.expect("micSplit should expose split outputs");
        assert_eq!(outputs.a.kind, "out1");
        assert_eq!(
            outputs
                .a
                .blocks
                .iter()
                .map(|n| n.model.clone())
                .collect::<Vec<_>>(),
            vec!["ACD_LA2AComp", "ACD_RoomVerb"]
        );
        assert_eq!(outputs.b.kind, "out2");
        assert_eq!(outputs.b.blocks[0].model, "ACD_TMSmallHall");
    }

    #[test]
    fn push_split_drops_stage_only_when_both_lanes_empty() {
        let mut stages: Vec<Stage> = Vec::new();
        push_split(&mut stages, Vec::new(), Vec::new());
        assert!(
            stages.is_empty(),
            "an all-empty split slot pair must not draw an empty fork/mix"
        );

        let node = GraphNode {
            group_id: "G3".to_string(),
            node_id: "n1".to_string(),
            model: "ACD_SpaceEcho".to_string(),
            bypassed: false,
            cab_sim_id: None,
            cab_sim_id2: None,
            cab_sim2_enabled: None,
            params: std::collections::HashMap::new(),
        };
        push_split(&mut stages, Vec::new(), vec![node]);
        assert_eq!(
            stages.len(),
            1,
            "one populated lane must still draw the split"
        );
    }

    // Build a synthetic PresetListResponse and confirm extraction. This pins the
    // descent logic (TMS→preset→listResponse→records→displayName) without hardware.
    #[test]
    fn extract_preset_list_parses_records() {
        // Construct from the inside out using the same wire helpers the encoder uses.
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut o = Vec::new();
            o.push(((field << 3) | 2) as u8); // field<16, single-byte tag
            o.push(inner.len() as u8);
            o.extend_from_slice(inner);
            o
        }
        fn str_field1(s: &str) -> Vec<u8> {
            ld(1, s.as_bytes())
        }
        // PresetListResponse{ listEnum=1 (field1 varint), record="65 Deluxe", record="Solo Lead" }
        let mut resp = vec![0x08, 0x01]; // field1 varint listEnum=1
        resp.extend(ld(2, &str_field1("65 Deluxe")));
        resp.extend(ld(2, &str_field1("Solo Lead")));
        // presetMessage[5] = PresetListResponse
        let preset_msg = ld(PRESET_LIST_RESPONSE, &resp);
        // FenderMessageTMS[2] = presetMessage
        let tms = ld(TMS_PRESET, &preset_msg);

        let names = extract_preset_list(&tms).expect("should parse");
        assert_eq!(
            names,
            vec!["65 Deluxe".to_string(), "Solo Lead".to_string()]
        );
    }

    // The tolerant extractor must also reject other lists' responses: after a
    // congested handshake truncated the My-Presets reply, the complete FACTORY
    // reply (listEnum=4, 249 records) was served as My Presets — silently, with
    // factory names in the Level tab.
    #[test]
    fn extract_preset_list_rejects_other_list_enums() {
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut o = Vec::new();
            o.push(((field << 3) | 2) as u8);
            o.push(inner.len() as u8);
            o.extend_from_slice(inner);
            o
        }
        let body = |list_enum: u8| {
            let mut resp = vec![0x08, list_enum];
            resp.extend(ld(2, &ld(1, b"'65 Deluxe Reverb")));
            ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
        };
        assert!(extract_preset_list(&body(4)).is_none()); // Factory
        assert!(extract_preset_list(&body(3)).is_none()); // Cloud
        assert_eq!(
            extract_preset_list(&body(1)),
            Some(vec!["'65 Deluxe Reverb".to_string()])
        );
    }

    // Longest-wins must never let a LONGER Factory response beat My Presets —
    // the incident shape (truncated 504-record My-Presets read + complete
    // 249-record Factory read in the same accumulated reports).
    #[test]
    fn best_preset_list_ignores_a_longer_factory_response() {
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut o = Vec::new();
            o.push(((field << 3) | 2) as u8);
            o.push(inner.len() as u8);
            o.extend_from_slice(inner);
            o
        }
        fn report(body: &[u8]) -> Vec<u8> {
            let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
            r.extend_from_slice(body);
            r
        }
        let list = |list_enum: u8, names: &[&str]| {
            let mut resp = vec![0x08, list_enum];
            for n in names {
                resp.extend(ld(2, &ld(1, n.as_bytes())));
            }
            ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
        };
        let reports = vec![
            report(&list(4, &["'65 Deluxe", "Unchain Ed", "Cutting"])), // Factory, longer
            report(&list(1, &["Guitar", "Cello"])),                     // My Presets
        ];
        let best = best_preset_list_from_reports(&reports).expect("my presets");
        assert_eq!(best, vec!["Guitar".to_string(), "Cello".to_string()]);
    }

    // The Factory harvest gates on listEnum == 4: it must pick the Factory reply
    // out of the shared accumulator and ignore the My-Presets (listEnum 1) reply,
    // even when My Presets is longer.
    #[test]
    fn best_factory_list_extracts_list_enum_4() {
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut o = Vec::new();
            o.push(((field << 3) | 2) as u8);
            o.push(inner.len() as u8);
            o.extend_from_slice(inner);
            o
        }
        fn report(body: &[u8]) -> Vec<u8> {
            let mut r = vec![0x00, 0x35, 0x00, body.len() as u8];
            r.extend_from_slice(body);
            r
        }
        let list = |list_enum: u8, names: &[&str]| {
            let mut resp = vec![0x08, list_enum];
            for n in names {
                resp.extend(ld(2, &ld(1, n.as_bytes())));
            }
            ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp))
        };
        let reports = vec![
            report(&list(1, &["Guitar", "Cello", "Synth"])), // My Presets, longer
            report(&list(4, &["'65 Deluxe", "Cutting"])),    // Factory
        ];
        let factory = best_factory_list_from_reports(&reports).expect("factory list");
        assert_eq!(
            factory,
            vec!["'65 Deluxe".to_string(), "Cutting".to_string()]
        );
        // And a reports set with NO factory reply yields nothing.
        assert!(best_factory_list_from_reports(&[report(&list(1, &["Guitar"]))]).is_none());
    }

    #[test]
    fn preset_list_uses_terminal_35_tail_frame() {
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut o = Vec::new();
            o.push(((field << 3) | 2) as u8);
            o.push(inner.len() as u8);
            o.extend_from_slice(inner);
            o
        }
        fn str_field1(s: &str) -> Vec<u8> {
            ld(1, s.as_bytes())
        }
        fn report(magic: u8, body: &[u8]) -> Vec<u8> {
            let mut r = vec![0x00, magic, 0x00, body.len() as u8];
            r.extend_from_slice(body);
            r
        }

        let mut resp = vec![0x08, 0x01];
        resp.extend(ld(2, &str_field1("Preset")));
        resp.extend(ld(2, &str_field1("Empty")));
        let tms = ld(TMS_PRESET, &ld(PRESET_LIST_RESPONSE, &resp));

        let split = tms.len() - 1;
        let reports = vec![report(0x33, &tms[..split]), report(0x35, &tms[split..])];

        let streams_only = proto::reassemble_streams(&reports)
            .into_iter()
            .find_map(|s| extract_preset_list(&s.body))
            .expect("tolerant streams decode");
        assert_eq!(streams_only.last().map(String::as_str), Some("Empt"));

        let best = best_preset_list_from_reports(&reports).expect("best list");
        assert_eq!(best, vec!["Preset".to_string(), "Empty".to_string()]);
    }

    #[test]
    fn dig_returns_none_for_absent_branch() {
        let body = [0x08, 0x01]; // a bare varint, no presetMessage
        assert!(dig(&body, TMS_PRESET, PRESET_LIST_RESPONSE).is_none());
    }

    // The currentFwResponse frame: settingsMessage(3) → currentFwResponse(2)
    // → data(1) = "1.7.75". Byte layout matches the frame observed live
    // (device on 1.7.75): 1a 0a 12 08 0a 06 31 2e 37 2e 37 35.
    #[test]
    fn extract_fw_version_parses_current_fw_response() {
        let body = [
            0x1a, 0x0a, // settingsMessage, len 10
            0x12, 0x08, // currentFwResponse, len 8
            0x0a, 0x06, b'1', b'.', b'7', b'.', b'7', b'5', // data = "1.7.75"
        ];
        assert_eq!(extract_fw_version(&body).as_deref(), Some("1.7.75"));
    }

    #[test]
    fn extract_fw_version_ignores_other_settings_frames() {
        // settingsMessage carrying reampModeActive (field 30), not currentFwResponse.
        let body = [0x1a, 0x05, 0xf2, 0x01, 0x02, 0x08, 0x01];
        assert_eq!(extract_fw_version(&body), None);
        // currentFwResponse with an empty data string → None, not Some("").
        let empty = [0x1a, 0x04, 0x12, 0x02, 0x0a, 0x00];
        assert_eq!(extract_fw_version(&empty), None);
    }

    #[test]
    fn is_level_param_includes_levels_excludes_gain() {
        assert!(super::is_level_param("outputLevel"));
        assert!(super::is_level_param("volume"));
        assert!(super::is_level_param("masterVolume"));
        assert!(!super::is_level_param("gain"));
        assert!(!super::is_level_param("driveLevel")); // drive wins → excluded
        assert!(!super::is_level_param("bass"));
    }

    #[test]
    fn extract_level_blocks_filters_to_level_controls() {
        let json = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                {
                    "nodeId": "ACD_TwinReverb65NoFx",
                    "dspUnitParameters": {
                        "outputLevel": 0.29, "gain": 0.43,
                        "bass": 0.22, "bypass": false, "gatePreset": "off"
                    }
                },
                { "nodeId": "VOL_Pedal", "dspUnitParameters": { "volume": 0.8 } }
            ] } }
        });
        let mut got = super::extract_level_blocks(&json);
        got.sort_by(|a, b| a.parameter_id.cmp(&b.parameter_id));
        assert_eq!(got.len(), 2, "got {got:?}");
        assert_eq!(got[0].parameter_id, "outputLevel");
        assert_eq!(got[0].node_id, "ACD_TwinReverb65NoFx");
        assert_eq!(got[0].group_id, "G1");
        assert!((got[0].value - 0.29).abs() < 1e-6);
        assert_eq!(got[1].parameter_id, "volume");
    }

    #[test]
    fn extract_level_blocks_tolerates_missing_graph() {
        assert!(super::extract_level_blocks(&serde_json::json!({})).is_empty());
    }

    // The real wire case: a preset JSON truncated mid-object after audioGraph
    // (the device sends a partial in the change event). Tolerant parsing must
    // still recover audioGraph.guitarNodes.
    #[test]
    fn tolerant_parse_recovers_truncated_audiograph() {
        let truncated = r#"{"ampControl":{"a":false},"audioGraph":{"guitarNodes":{"G1":[{"nodeId":"ACD_TM59Bassman","dspUnitParameters":{"outputLevel":0.5}}]}},"instrumentOutput":{"out1":true}},"scenes"#;
        assert!(
            serde_json::from_str::<serde_json::Value>(truncated).is_err(),
            "should be invalid"
        );
        let v = super::tolerant_parse_json(truncated).expect("tolerant parse recovers it");
        let blocks = super::extract_level_blocks(&v);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].node_id, "ACD_TM59Bassman");
        assert_eq!(blocks[0].parameter_id, "outputLevel");
    }

    #[test]
    fn tolerant_parse_unwinds_nested_array_object_order() {
        let truncated =
            r#"{"audioGraph":{"splitMix":{"splitPoints":[{"parameters":{"levelA":1.0,"x"#;
        let v = super::tolerant_parse_json(truncated).expect("nested partial recovers");
        assert_eq!(
            v["audioGraph"]["splitMix"]["splitPoints"][0]["parameters"]["levelA"],
            1.0
        );
    }

    #[test]
    fn tolerant_parse_passes_through_valid_json() {
        let v = super::tolerant_parse_json(r#"{"a":1,"b":[2,3]}"#).unwrap();
        assert_eq!(v["a"], 1);
    }

    // ─── Live-push decoders (shared by the monitor + the listener) ────────────

    /// Encode a protobuf varint into `out` (test helper — proto's encoders are
    /// module-private, so the push-decoder tests hand-build their input frames).
    fn put_varint_t(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let mut b = (n & 0x7f) as u8;
            n >>= 7;
            if n != 0 {
                b |= 0x80;
            }
            out.push(b);
            if n == 0 {
                break;
            }
        }
    }
    fn field_varint_t(out: &mut Vec<u8>, field: u32, value: u64) {
        put_varint_t(out, (field as u64) << 3); // wire type 0
        put_varint_t(out, value);
    }
    fn field_bytes_t(out: &mut Vec<u8>, field: u32, value: &[u8]) {
        put_varint_t(out, ((field as u64) << 3) | 2);
        put_varint_t(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    /// Build a `FenderMessageTMS{ presetMessage[2]{ <field>{ <inner bytes> } } }`.
    fn tms_preset(inner_field: u32, inner: &[u8]) -> Vec<u8> {
        let mut pm = Vec::new();
        field_bytes_t(&mut pm, inner_field, inner);
        let mut o = Vec::new();
        field_bytes_t(&mut o, 2, &pm); // presetMessage = TMS field 2
        o
    }

    #[test]
    fn decode_preset_loaded_reads_all_addressing_fields() {
        // PresetLoaded{ tab=1, setlist=2, setlistSong=3, song=4, songPreset=5,
        //   presetSlot=11, sync=1 } — build inner with field_varint, decode back.
        let mut inner = Vec::new();
        for (f, v) in [
            (1u32, 1u64),
            (2, 2),
            (3, 3),
            (4, 4),
            (5, 5),
            (6, 11),
            (7, 1),
        ] {
            field_varint_t(&mut inner, f, v);
        }
        let body = tms_preset(11, &inner);
        let pl = super::decode_preset_loaded(&body).expect("is a PresetLoaded");
        assert_eq!(pl.tab_enum, 1);
        assert_eq!(pl.setlist_slot, 2);
        assert_eq!(pl.song_slot, 4);
        assert_eq!(pl.preset_slot, 11);
        assert_eq!(pl.sync, 1);
        // A non-PresetLoaded body decodes to None.
        assert!(super::decode_preset_loaded(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_info_changed_reads_name_dirty_favorite() {
        // CurrentPresetInfoChanged{ displayName="Lead", isDirty=1, isFavorite=0 }.
        let mut inner = Vec::new();
        field_bytes_t(&mut inner, 1, b"Lead");
        field_varint_t(&mut inner, 2, 1);
        // isFavorite omitted (proto3 false default) → should decode as false.
        let body = tms_preset(22, &inner);
        let (name, dirty, fav) = super::decode_info_changed(&body).expect("is InfoChanged");
        assert_eq!(name, "Lead");
        assert!(dirty);
        assert!(!fav);
        // With isFavorite=1 set explicitly.
        let mut inner2 = Vec::new();
        field_bytes_t(&mut inner2, 1, b"Clean");
        field_varint_t(&mut inner2, 3, 1);
        let (name2, dirty2, fav2) = super::decode_info_changed(&tms_preset(22, &inner2)).unwrap();
        assert_eq!(name2, "Clean");
        assert!(!dirty2);
        assert!(fav2);
        assert!(super::decode_info_changed(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_scene_loaded_reads_slot_and_scene_name() {
        // SceneLoaded{ sceneSlot=2, sceneJson=LZ4("{\"sceneName\":\"Verse\"}") }.
        let json = br#"{"sceneName":"Verse","other":1}"#;
        let lz4 = proto::lz4_block_compress_stored(json);
        let mut inner = Vec::new();
        field_varint_t(&mut inner, 1, 2);
        field_bytes_t(&mut inner, 2, &lz4);
        let body = tms_preset(102, &inner);
        let (slot, name) = super::decode_scene_loaded(&body).expect("is SceneLoaded");
        assert_eq!(slot, 2);
        assert_eq!(name.as_deref(), Some("Verse"));
        // A SceneLoaded with no sceneJson → slot only, name None (handoff renders anyway).
        let mut inner2 = Vec::new();
        field_varint_t(&mut inner2, 1, 8);
        let (slot2, name2) = super::decode_scene_loaded(&tms_preset(102, &inner2)).unwrap();
        assert_eq!(slot2, 8);
        assert_eq!(name2, None);
        assert!(super::decode_scene_loaded(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_scene_list_reads_repeated_names() {
        // sceneListResponse{ dummy=1, sceneList=["Clean","Crunch","Lead"] }.
        let mut inner = Vec::new();
        field_varint_t(&mut inner, 1, 1); // dummy
        for n in ["Clean", "Crunch", "Lead"] {
            field_bytes_t(&mut inner, 2, n.as_bytes());
        }
        let body = tms_preset(125, &inner);
        let names = super::decode_scene_list(&body).expect("is sceneListResponse");
        assert_eq!(names, vec!["Clean", "Crunch", "Lead"]);
        // An empty scene list is a valid Some(vec![]) (preset with no FS scenes).
        let mut empty = Vec::new();
        field_varint_t(&mut empty, 1, 1);
        assert_eq!(
            super::decode_scene_list(&tms_preset(125, &empty)),
            Some(vec![])
        );
        assert!(super::decode_scene_list(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_current_preset_data_yields_active_graph_with_known_template() {
        // currentPresetDataChanged{ presetJson = LZ4(series preset JSON) }.
        let json = br#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{"G1":[{"FenderId":"ACD_TM59Bassman","dspUnitParameters":{"bypass":false}}]}}}"#;
        let lz4 = proto::lz4_block_compress_stored(json);
        let mut inner = Vec::new();
        field_bytes_t(&mut inner, 1, &lz4);
        let body = tms_preset(3, &inner);
        let g = super::decode_current_preset_data(&body).expect("known-template graph");
        assert_eq!(g.template.as_deref(), Some("gtrSeries"));
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].model, "ACD_TM59Bassman");
        // A payload whose template is NOT a known routing template → None (guard).
        let bad =
            br#"{"audioGraph":{"template":"gtrUnkn","guitarNodes":{"G1":[{"FenderId":"X"}]}}}"#;
        let lz4bad = proto::lz4_block_compress_stored(bad);
        let mut innerb = Vec::new();
        field_bytes_t(&mut innerb, 1, &lz4bad);
        assert!(super::decode_current_preset_data(&tms_preset(3, &innerb)).is_none());
        // Not a currentPresetDataChanged → None.
        assert!(super::decode_current_preset_data(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_current_preset_live_reads_the_scene_trio_from_one_document() {
        // currentPresetDataChanged whose preset JSON carries scenes[].sceneName (slot
        // order), lastLoadedScene and ftsw — the live-scene trio in one document.
        let json = br#"{"ftsw":[[{"func":"scene","sceneSlot":1,"isActive":true}],[]],"lastLoadedScene":1,"scenes":[{"sceneName":"Clean","uuid":"a"},{"sceneName":"Lead","uuid":"b"}]}"#;
        let lz4 = proto::lz4_block_compress_stored(json);
        let mut inner = Vec::new();
        field_bytes_t(&mut inner, 1, &lz4);
        let live = super::decode_current_preset_live(&tms_preset(3, &inner)).expect("field-3");
        assert_eq!(live.scene_names, Some(vec!["Clean".into(), "Lead".into()]));
        assert_eq!(live.last_loaded_scene, Some(1));
        assert!(live.ftsw.is_some());
        // A truncated partial that lost the tail (no scenes/lastLoadedScene/ftsw)
        // still decodes — each half is independently None (the monitor keeps caches).
        let lean = br#"{"audioGraph":{"template":"gtrSeries","guitarNodes":{}},"bpm":120}"#;
        let lz4lean = proto::lz4_block_compress_stored(lean);
        let mut innerl = Vec::new();
        field_bytes_t(&mut innerl, 1, &lz4lean);
        let live = super::decode_current_preset_live(&tms_preset(3, &innerl)).expect("field-3");
        assert_eq!(live.scene_names, None);
        assert_eq!(live.last_loaded_scene, None);
        assert!(live.ftsw.is_none());
        // Not a currentPresetDataChanged → None.
        assert!(super::decode_current_preset_live(&[0x08, 0x01]).is_none());
    }

    #[test]
    fn decode_current_preset_live_survives_the_real_truncated_dump() {
        // The REAL Tier-4 capture (probe --dump-currentpresetdata, Guitar, 1.7.75):
        // a 17,263 B field-3 payload truncated mid-final-scene-`uuid`. The tolerant
        // parse must still recover all 8 slot-ordered names + lastLoadedScene(8 =
        // base) + the 10-switch ftsw. Fixture is gitignored (device-derived) — the
        // test auto-skips when absent (fresh worktree convention).
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/currentpresetdata-guitar-1.7.75.json");
        let Ok(text) = std::fs::read(&path) else {
            eprintln!("fixture missing — skipping ({})", path.display());
            return;
        };
        let lz4 = proto::lz4_block_compress_stored(&text);
        let mut inner = Vec::new();
        field_bytes_t(&mut inner, 1, &lz4);
        let live = super::decode_current_preset_live(&tms_preset(3, &inner)).expect("field-3");
        assert_eq!(
            live.scene_names.as_deref(),
            Some(
                &[
                    "Arpeges",
                    "Reverb",
                    "Delay + Reverb",
                    "Celestial",
                    "Lofi",
                    "Dist",
                    "Swell",
                    "Klon"
                ]
                .map(String::from)[..]
            )
        );
        assert_eq!(live.last_loaded_scene, Some(super::BASE_SCENE_SLOT));
        let map = crate::footswitch::scene_fs_map(&live.ftsw.expect("ftsw"));
        // Active assignments only (Lofi slot 4 + Swell slot 6 are isActive:false).
        assert_eq!(map.get(&0), Some(&5)); // Arpeges → switch 5 (FS6)
        assert_eq!(map.get(&5), Some(&7)); // Dist → switch 7 (FS8)
        assert_eq!(map.get(&4), None); // Lofi: inactive switch → em-dash
    }
}
