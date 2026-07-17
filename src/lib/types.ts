// TMP Companion — shared TypeScript type surface.
//
// A byte-accurate mirror of the Rust serde types the Tauri backend returns and
// the enum-tagged payloads it accepts. Field names are snake_case because that
// is exactly how serde serializes them (the Rust structs use no `rename_all`).
//
// Mirrors the serde types the Rust backend returns/accepts.
//
// WIRE-CASING RULE (load-bearing): top-level invoke arg KEYS are camelCase
// (Tauri auto-converts to the Rust snake_case param); field keys NESTED inside a
// JSON payload object (job/op/spec/recipe/filter/edit/store) stay snake_case and
// are passed through to serde unchanged. The interfaces below model the *wire*
// payloads, so their fields are snake_case to match serde.

// ─── Connection + app ─────────────────────────────────────────────────────────

/** App identity (mirrors lib::AppInfo). Name is "TMP Companion". */
export interface AppInfo {
  name: string;
  version: string;
}

/** One device "My Presets" row (mirrors lib::PresetEntry). `slot` is the
 * 0-based list index shown in the UI. */
export interface PresetEntry {
  slot: number;
  name: string;
}

/** One block-acting function on a footswitch (`footswitch::FootswitchFn`). */
export interface FootswitchFn {
  func: "on-off" | "param";
  group_id: string;
  node_id: string;
  fender_id: string;
  parameter_id: string | null;
  value_a: number | null;
  value_b: number | null;
  /** The assignment's own `isActive` — for an on-off function, the CURRENT
   *  engaged state at save time. Drives the Doctor's offline force-bypass
   *  isolation derivation (no live preset read). */
  is_active: boolean;
}

/** A continuous block parameter the leveler can target (`footswitch::LevelParamCandidate`). */
export interface LevelParamCandidate {
  group_id: string;
  node_id: string;
  fender_id: string;
  parameter_id: string;
  current: number;
}

/** A block-acting footswitch (on/off + parameter change) with its leveling-candidate
 * params (`footswitch::FootswitchInfo`). `switch` is the `ftsw` array index. */
export interface FootswitchInfo {
  switch: number;
  label: string;
  link_group: number | null;
  functions: FootswitchFn[];
  level_params: LevelParamCandidate[];
}

export interface PresetScenes {
  scenes: string[];
  fs: (number | null)[];
  /** Block-acting footswitches (empty when the preset has none). */
  footswitches: FootswitchInfo[];
}

// ─── Loudness leveling ──────────────────────────────────────────────────────

/** A level-job sent to `level_preset` inside `{ job: ... }`. Keys are snake_case
 * (mirrors lib::LevelJob, a serde Deserialize struct). All three `block_*`
 * coords set ⇒ closed-loop leveling on that block knob. */
export interface LevelJob {
  slot: number;
  target_lufs: number;
  /** Opt-in persist (SaveCurrentPreset). No window.confirm guard. */
  save: boolean;
  /** Pickup topology (or alias) id → its bundled stimulus WAV (an alias
   * resolves to its parent topology's WAV). */
  topology_id: string | null;
  /** Tier-2 measured dry-instrument loudness (K-weighted LUFS). */
  calibration_lufs: number | null;
  /** Chosen instrument profile id — backend resolves its Tier-2 calibration capture
   * WAV as the re-amp stimulus when present. */
  profile_id: string | null;
  /** Explicit stimulus override (wins over topology_id). */
  stimulus_path?: string | null;
  /** Block-knob leveling coordinates (from `list_level_blocks`). */
  block_group_id: string | null;
  block_node_id: string | null;
  block_parameter_id: string | null;
  /** Current value, picks closed-loop search bounds. */
  block_value: number | null;
}

/** One entry in a setlist common-target job (mirrors lib::SetlistJobEntry).
 * Keys snake_case (nested inside `{ entries: [...] }`). */
export interface SetlistJobEntry {
  slot: number;
  topology_id: string | null;
  calibration_lufs: number | null;
}

/** Result of leveling one preset (mirrors leveller::LevelResult). */
export interface LevelResult {
  slot: number;
  ref_level: number;
  measured_lufs: number;
  constant_c: number;
  final_level: number;
  target_lufs: number;
  predicted_lufs: number;
  clamped: boolean;
  saved: boolean;
  /** Independent re-measure at final_level (null if verify skipped). */
  verify_lufs: number | null;
  /** Capture iterations used (1 = one-shot presetLevel; 2..N = closed-loop block). */
  iterations: number;
  /** Short-term-max − integrated of the measure capture (LU). Large (≳6 LU) = a
   * dynamic preset whose gated reading understates its peaks — verify by ear.
   * Null when the measuring path has no full-capture meter. */
  dynamic_spread_lu: number | null;
  /** Set with `clamped` for the "no authority" case — the amp's outputLevel doesn't
   * reach the USB 1/2 capture (off-branch / off-USB). Shown verbatim instead of a
   * generic clamp. Null for the preset-level path / an ordinary headroom clamp. */
  clamp_reason: string | null;
  /** Rebalance "verify by ear": the lane-mute floor was too shallow to trust the
   * equal-solo balance (the overall target still landed). ORed with the spread flag. */
  verify_by_ear: boolean;
  /** The preset's saved `presetLevel` BEFORE this run wrote it — the revert anchor
   * for "Restore original". Null when the pre-run read failed or the path doesn't
   * write `presetLevel` (block-knob / scene rows). */
  previous_level: number | null;
  /** PREDICTED true peak (dBTP) at final_level, extrapolated from the reference
   * capture's measured true peak — an ESTIMATE, never a re-measurement. Only the
   * one-shot presetLevel path (level_preset) sets this; null otherwise. */
  true_peak_dbtp: number | null;
}

/** Result of leveling one block-acting footswitch's engaged state
 * (mirrors `leveller::FootswitchLevelResult`, snake_case). */
export interface FootswitchLevelResult {
  switch: number;
  /** Engaged loudness at the low reference seed. */
  measured_lufs: number;
  /** Solved switch-ON value written as the `param` function's `valueA`. */
  final_value: number;
  target_lufs: number;
  /** Achieved engaged loudness at `final_value`. */
  predicted_lufs: number;
  clamped: boolean;
  clamp_reason: string | null;
  saved: boolean;
  verify_lufs: number | null;
  iterations: number;
  dynamic_spread_lu: number | null;
  /** `"baked"` (value written onto the block) or `"assigned"` (param-change function written). */
  method: string;
}

/** Result of leveling a whole setlist to one common target
 * (mirrors leveller::SetlistResult). */
export interface SetlistResult {
  target_lufs: number;
  results: LevelResult[];
}

/** A level-type block control discoverable from a preset
 * (mirrors session::LevelBlock). */
export interface LevelBlock {
  group_id: string;
  node_id: string;
  model_id: string;
  parameter_id: string;
  value: number;
}

// ─── Instrument profiles + persisted store (Settings window) ──────────────────

/** A user-defined instrument profile (mirrors profiles::Profile). */
export interface Profile {
  id: string;
  name: string;
  topology_id: string;
  /** K-weighted loudness (LUFS) of the dry instrument; null until calibrated. */
  calibration_lufs: number | null;
}

/** What one Tier-2 calibration measured + its quality caveats (mirrors
 * lib::CalibrateResult). */
export interface CalibrateResult {
  /** Measured dry-instrument loudness (K-weighted LUFS), stored on the profile. */
  lufs: number;
  /** Dry capture hit 0 dBFS — measurement biased low; re-calibrate softer. */
  clipped: boolean;
  /** LU the topology stimulus falls short of reproducing `lufs` (peak-capped);
   * null when reachable — always null when the capture was stored as the stimulus. */
  stimulus_shortfall_lu: number | null;
  /** Short-term-max − integrated (LU) of the dry capture — how dynamic the take was. */
  spread_lu: number;
  /** Per-band excitation of the capture (same family band layout as the Doctor
   * engine); true = the band was actually played. Index-aligned with `band_labels`. */
  band_coverage: boolean[];
  /** Player-facing labels for `band_coverage`, index-aligned. */
  band_labels: string[];
}

/** A named loudness target the user can apply per preset (mirrors profiles::Target). */
export interface Target {
  name: string;
  lufs: number;
}

/** The playback loudness leveling compensates for (mirrors profiles::PlaybackLevel).
 * Equal-LUFS is equal-loudness at one SPL only; below stage volume bass presets
 * get a hotter target (Fletcher–Munson compensation). */
export type PlaybackLevel = "quiet" | "rehearsal" | "stage";

/** The persisted profile/target store (mirrors profiles::Store). */
export interface Store {
  profiles: Profile[];
  /** slot → profile id (sparse — only slots with an assigned profile appear). */
  profile_by_slot: Partial<Record<number, string>>;
  /** User-editable named loudness targets (the live levels). */
  targets: Target[];
  /** Playback loudness the leveling targets are compensated for. */
  playback_level: PlaybackLevel;
  /** Download + install app updates automatically (Settings toggle). */
  auto_install_updates: boolean;
}

/** A shipped pickup topology or alias row (mirrors commands/settings.rs
 * TopologyInfo). */
export interface TopologyInfo {
  id: string;
  label: string;
  instrument: string;
}

/** A bundled stimulus WAV (mirrors lib::SampleInfo). */
export interface SampleInfo {
  name: string;
  path: string;
}

// ─── Library ───────────────────────────────────────────────────────────────

/** Folder↔device reconciliation summary (mirrors library::ReconcileReport). */
export interface ReconcileReport {
  matched: number;
  unmatched_files: string[];
  unmatched_slots: string[];
  ambiguous: string[];
}

/** Filter args for `library_filter`, sent inside `{ filter: ... }`. All
 * optional, snake_case (mirrors lib::FilterArgs). */
export interface FilterArgs {
  name_substr?: string;
  amp?: string;
  block?: string;
  ir?: string;
  sic?: string;
  level_lt?: number;
  level_gt?: number;
}

// ─── Bulk run engine ──────────────────────────────────────────────────────────

/** Revert result for one preset (mirrors bulkrun::RevertEntry). */
export interface RevertEntry {
  list_index: number;
  restored: boolean;
  error: string | null;
}

/** One rename apply result (mirrors lib::RenameApplyRow). */
export interface RenameApplyRow {
  list_index: number;
  new_name: string;
  applied: boolean;
  error: string | null;
}

// ─── Block templates ──────────────────────────────────────────────────────────

/** A saved block template (mirrors blocklib::BlockTemplate). */
export interface BlockTemplate {
  name: string;
  model: string;
  params: Record<string, unknown>;
}

// ─── Spectral analysis + audition ─────────────────────────────────────────────

/** Per-band energies + tonal flags for one preset (mirrors lib::SpectrumResult). */
export interface SpectrumResult {
  bands: number[];
  flags: string[];
}

/** EQ-match result: source vs reference spectra + match deltas
 * (mirrors lib::EqMatchResult). */
export interface EqMatchResult {
  source_bands: number[];
  reference_bands: number[];
  distance: number;
  deltas: number[];
  matched_bands: number[];
}

/** A candidate ranked by spectral distance to a target (mirrors spectrum::SicRank). */
export interface SicRank {
  sicid: string;
  distance: number;
}

// ─── Loudness audit + migration ───────────────────────────────────────────────

/** One loudness-audit finding (mirrors lint::Finding). */
export interface Finding {
  list_index: number;
  rule: string;
  message: string;
}

/** One preset affected by a firmware migration (mirrors lib::MigrationRow). */
export interface MigrationRow {
  list_index: number | null;
  name: string;
  affected_blocks: string[];
}

/** One preset's migration-apply outcome (mirrors lib::MigrationApplyRow). */
export interface MigrationApplyRow {
  list_index: number;
  swaps: number;
  applied: boolean;
  error: string | null;
}

// ─── Songs ────────────────────────────────────────────────────────────────────

/**
 * A song as read live from the device (Songs overview, read-only).
 * `presets` is a per-song list of {slot, scene} references; when the live read
 * does not populate per-song preset rows it is left undefined.
 */
export interface SongRecord {
  slot: number;
  name: string;
  notes: string;
  bpm: number;
  bpm_active?: boolean;
  presets?: { slot: number; scene: number }[];
}

/** A setlist as read live from the device (mirrors session::SetlistRecord). The
 * device model is name-only; per-setlist song membership is a separate read
 * (`list_setlist_songs` → ordered global song slots). */
export interface SetlistRecord {
  slot: number;
  name: string;
}

/** Result of a batched song transaction (create_song_full / update_song_full):
 * the fresh authoritative song list, the fresh membership of the requested
 * setlist (create with add-to-setlist only), and the best-effort BPM warning
 * (BPM is the unit's active-song tap tempo and can fail to settle — the song
 * itself is kept). */
export interface SongSaveOutcome {
  songs: SongRecord[];
  members: number[] | null;
  bpm_warning: string | null;
}

// ─── LevelView — active preset signal graph (read_active_preset) ───────────────

/** One node in the active preset's signal chain (mirrors session::GraphNode). */
export interface GraphNode {
  group_id: string;
  node_id: string;
  model: string;
  bypassed: boolean;
  /** CabSim block only: its primary cabinet id (`cabsimid`, e.g. `Mar1960aV30Alt`)
   *  — the strip names the cab from this and expands a dual-cab into two parallel
   *  tiles. Omitted (undefined) for non-cab nodes. */
  cab_sim_id?: string;
  /** The second cabinet id of a dual cab (`cab2simid`), when `cab_sim2_enabled`. */
  cab_sim_id2?: string;
  /** Whether this CabSim runs two cabinets in parallel (`cabsim2enabled`). */
  cab_sim2_enabled?: boolean;
  /** Allowlisted numeric params (reverb-mix + EQ-10 band gains) harvested from
   *  `dspUnitParameters` — Doctor's value-aware prescriptions read these.
   *  Always present server-side (empty map when none). */
  params: Record<string, number>;
}

/** One ordered stage of the chain (mirrors session::Stage). A `series` run of
 * blocks, or a `split` with two parallel lanes (each a series run) joined by a
 * mix. The chain is an ordered `Stage[]`, so any number of sequential splits with
 * series segments before/between/after is representable. */
export type Stage =
  | { kind: "series"; blocks: GraphNode[] }
  | { kind: "split"; a: GraphNode[]; b: GraphNode[] };

export interface InputLane {
  type: "guitar" | "mic";
  blocks: GraphNode[];
}

export interface OutputLane {
  type: "out1" | "out2";
  blocks: GraphNode[];
}

export interface InputPair {
  a: InputLane;
  b: InputLane;
}

export interface OutputPair {
  a: OutputLane;
  b: OutputLane;
}

export interface IndependentLane {
  input: "guitar" | "mic";
  output: "out1" | "out2";
  blocks: GraphNode[];
}

/** The active (currently-loaded) preset's signal graph as read live from the
 * device (mirrors session::ActiveGraph). Header fields are null until the live
 * read populates them; `split_mix` is an opaque JSON payload. `stages` is the
 * back-compat ordered view, while optional input/output/lanes fields let the
 * strip render dual-input, split-output, and fully independent templates. */
export interface ActiveGraph {
  name: string | null;
  slot: number | null;
  template: string | null;
  split_mix: unknown;
  nodes: GraphNode[];
  input_type?: "guitar" | "mic" | null;
  output_type?: "out" | null;
  inputs?: InputPair | null;
  outputs?: OutputPair | null;
  lanes?: IndependentLane[] | null;
  stages: Stage[];
}

/** Combined connect result: firmware + active graph in one handshake (mirrors
 * lib.rs ConnectResult). The graph is null when no preset is loaded or the
 * handshake's field-3 stream was truncated before a complete audioGraph. */
export interface ConnectResult {
  firmware: string | null;
  graph: ActiveGraph | null;
}

// Scene rows + the live scene come exclusively from the unit via the monitor events
// below. The wire is 0-based with base = BASE_SCENE_SLOT.

// ─── Live-sync monitor events (backend EMITS, frontend LISTENS) ────────────────
// The persistent device monitor (src-tauri/src/monitor.rs) holds the device with a
// dense ~250 ms heartbeat so the unit PUSHES its state changes (footswitch taps,
// scene recalls, preset changes done ON THE HARDWARE) unsolicited; the backend
// mirrors them as these Tauri events. App-initiated commands (loadScene/loadPreset)
// route through the device-op gate, which pauses the monitor for the command's
// duration; the resulting device state then comes back through the SAME event
// stream (app-initiated and device-pushed share one stream). Payload keys are
// camelCase (the backend serializes with `rename_all = "camelCase"`).

/** `tmp://live-preset` — the active preset's identity, coalesced from PresetLoaded(11)
 * + CurrentPresetInfoChanged(22). `listIndex` is the 0-based My-Presets list index
 * (`PresetLoaded.presetSlot − 1`), or null when the active preset isn't a My-Presets
 * slot (factory / song context) or before the first PresetLoaded push. */
export interface LivePresetEvent {
  listIndex: number | null;
  name: string;
  isDirty: boolean;
  isFavorite: boolean;
}

/** The device's base scene slot on the wire: `loadScene`/`SceneLoaded`/`lastLoadedScene`
 * address FS scenes as 0-based `scenes[]` indices (0..=7) and the base scene as the
 * CONSTANT 8 — even for a preset with zero FS scenes (HW-proven; NOT scene count + 1). */
export const BASE_SCENE_SLOT = 8;

/** `tmp://live-scene` — the unit's current scene. Emitted by BOTH the SceneLoaded(102)
 * echo (fast path — its embedded name is authoritative) and the field-3
 * `lastLoadedScene` (authoritative index, same document as the scene rows); last-writer
 * wins. `key` is `"base"` for the base scene (the wire's constant base slot), else the
 * numeric 0-based FS scene index into `scenes[]`. `name` null if absent/truncated. */
export interface LiveSceneEvent {
  key: "base" | number;
  name: string | null;
}

/** One row of `tmp://scene-list`. Row index = the 0-based wire sceneSlot. `fs` is the
 * real assigned footswitch (1-based, from the preset's live `ftsw`); null when the
 * scene has no active switch (the UI renders an em-dash). */
export interface SceneListRow {
  name: string;
  fs: number | null;
}

/** `tmp://scene-list` — the ACTIVE preset's live scene rows. Canonical source: the
 * field-3 preset JSON (`scenes[].sceneName` slot-ordered + `ftsw`, one document),
 * which arrives on every device change AND in the connect handshake;
 * sceneListResponse(125) is only a preset-switch top-up. Also returned by the
 * `requestSceneList` command (a manual top-up). */
export interface SceneListEvent {
  scenes: SceneListRow[];
}

/** `tmp://signal-chain` — the active preset's signal graph from currentPresetDataChanged(3).
 * Same shape as `read_active_preset` (`ActiveGraph`), so the existing chain renderer
 * works unchanged whether the graph arrives via the command or the live push. */
export type SignalChainEvent = ActiveGraph;

/** `tmp://sync` — a device-push / (re)connect is in flight; the UI shows the neutral
 * catching-up state until the first real state lands. */
export interface SyncEvent {
  syncing: boolean;
}

/** `tmp://leveling-lufs` — advisory live measured loudness (mirrors `lib::LiveLufsEvent`)
 * streamed while a leveling capture runs. Reference-level loudness as the meter converges,
 * NOT the final preset level; the run result row is the authoritative value. */
export interface LiveLufsEvent {
  lufs: number;
  /** Current hop's plain RMS in dB — decorative fuel for the live VU bars, not the solve. */
  momentary: number;
}

// ─── Device-backup fast library read (the Presets-tab two-phase load) ──────────

/** One scene of a backup-read preset (mirrors `lib::SceneInfo`): name + 1-based
 * footswitch tag (`null` when the scene has no footswitch). Same shape as the live
 * `SceneListRow`, so backup-loaded and live scenes render identically. */
export interface SceneInfo {
  name: string;
  fs: number | null;
}

/** An amp `outputLevel` leveling candidate (`lib::LevelBlockArg`, camelCase wire
 * form) — the knob per-scene leveling drives. Extracted from the backup at startup
 * so the run never needs a live block-discovery round-trip. */
export interface AmpCandidate {
  groupId: string;
  nodeId: string;
  parameterId: string;
  value: number;
}

/** One preset from the backup DB (`lib::BackupPresetRow`). `scene_count` is `-1`
 * when `presetJson` could not be parsed (rare — the DB doc is full plaintext). */
export interface BackupPresetRow {
  slot: number;
  name: string;
  scene_count: number;
  scenes: SceneInfo[];
  amp_candidates: AmpCandidate[];
  /** Every block in the preset's audioGraph (`lib::BackupBlock`). Drives the
   * per-preset CPU total + "blocks present in the selection" lists. */
  blocks: BackupBlock[];
  /** The preset's routed signal graph (`session::extract_active_graph` over the
   * backup's full presetJson) — the SAME shape as the active read, so any preset's
   * lanes/topology can render off the one backup. Empty (`stages: []`, `nodes: []`)
   * when the row's presetJson couldn't be parsed. Drives the Copy feature. */
  graph: ActiveGraph;
  /** Block-acting footswitches (on/off + parameter change) with leveling-candidate
   * params, from the same presetJson — drives the footswitch picker + preset-list
   * tags for the whole library with no extra device read. Empty otherwise. */
  footswitches: FootswitchInfo[];
  /** JSON-visible cause of a silent leveling capture (`backup_read::silence_hint`):
   * `amp_zero` = every active amp's outputLevel saved at 0 (definite silence);
   * `exp_mute` = an exp-pedal binding zeroes an amp outputLevel at one end (silence
   * only when a physical pedal sits there). Refines the "not on USB 1/2" verdict. */
  silence_hint: SilenceHint | null;
}

/** A [`BackupPresetRow.silence_hint`] value. */
export type SilenceHint = "amp_zero" | "exp_mute";

/** One block in a backup preset's audioGraph roster (`lib::BackupBlock`). */
export interface BackupBlock {
  group_id: string;
  node_id: string;
  /** Exact (possibly suffixed, e.g. `ACD_HiwattDR103CanModCabIR`) model id. */
  fender_id: string;
}

/** One saved block ("block preset") from the device store (`lib::SavedBlock`).
 * Identity + cab config only — the saved `dspUnitParameters` live on the device and
 * are applied live by `index` via `ReplaceNodeWithBlock`. */
export interface SavedBlock {
  fender_id: string;
  /** Position in this model's saved list = the `ReplaceNodeWithBlock` index. */
  index: number;
  name: string;
  favorite: boolean;
  dual_cabs_enabled: boolean;
  cab1_id: string;
  cab2_id: string;
}

/** One user impulse-response slot on the device (`lib::UserIr`). */
export interface UserIr {
  name: string;
  exists: boolean;
}

/** One preset's outcome from a live bulk-replace run (`lib::BulkReplaceItem`). */
export interface BulkReplaceItem {
  slot: number;
  name: string;
  outcome: "updated" | "skipped" | "error";
  detail: string;
}

// ─── Copy blocks between presets (`copy_apply`) ───────────────────────────────
// The save path of the Copy feature: per target preset, an ordered list of structural
// ops applied LIVE on a held session (`session::{replace_node, insert_node,
// remove_node}`), saved only when every op confirmed. Tagged enums match the Rust
// serde (`#[serde(tag = "kind", rename_all = "snake_case")]`); the nested object keys
// are camelCase because they cross `invoke` (Tauri → snake_case) like every arg key.

/** The copied-in block for a Replace / Insert op (`lib::CopyRepl`). The frontend only
 * ever sends `model` — the origin palette is built from the reference preset's node
 * FenderIds, so a stock-model id is the faithful copy. (The backend also accepts `ir`
 * / `saved`, used by no current caller.) */
export interface CopyRepl {
  kind: "model";
  fenderId: string;
}

/** One structural op applied to a target preset (`lib::CopyOp`). `nodeId` / `group`
 * address the existing block. An insert anchors BEFORE a FenderId (`beforeFenderId`,
 * field-34 insertNode — field-2 is the block to insert AHEAD of); `beforeFenderId` null
 * appends at the group end. */
export type CopyOp =
  | { kind: "replace"; group: string; nodeId: string; repl: CopyRepl }
  | {
      kind: "insert";
      group: string;
      beforeFenderId?: string | null;
      repl: CopyRepl;
    }
  | { kind: "remove"; group: string; nodeId: string };

/** One target preset's staged edits for `copy_apply` (`lib::CopyJob`). `listIndex` is
 * the 0-based My-Presets index; `ops` is the ordered op list from `diffToOps`. */
export interface CopyJob {
  listIndex: number;
  name: string;
  ops: CopyOp[];
}

/** One preset's outcome from a `copy_apply` run (`lib::CopyApplyItem`). Like
 * `BulkReplaceItem` plus the post-save signal graph, so the Copy view can patch its
 * cached library in place (no re-scan) after a write — `graph` is omitted when the
 * preset wasn't saved or its graph couldn't be read back. */
export interface CopyApplyItem {
  slot: number;
  name: string;
  outcome: "updated" | "skipped" | "error";
  detail: string;
  graph?: ActiveGraph;
}

/** One song→preset binding from the backup `SongPresets` table (`lib::SongPresetBinding`).
 * `song_slot` = device song slot (1-based positional, aligns with the live song list's
 * `slot`); `preset_slot` = bound preset's device slot (`UserPresets.slot`, = list index
 * + 1). Read-only: which songs use a preset is set ON THE UNIT (Pro Control). */
export interface SongPresetBinding {
  song_slot: number;
  preset_slot: number;
}

/** One setlist→song membership row (`lib::BackupSetlistSong`); `position` is the song's
 * 1-based order within the setlist. */
export interface BackupSetlistSong {
  setlist_slot: number;
  song_slot: number;
  position: number;
}

/** Result of `read_library_via_backup` (`lib::BackupReadResult`) — the whole user
 * library (every preset + its scenes) decoded from one ~22 s device backup.
 * NOTE: wire is snake_case — `BackupReadResult` has NO `rename_all = "camelCase"`. */
export interface BackupReadResult {
  members: [string, number][];
  db_bytes: number;
  total_rows: number;
  scene_mode: string;
  presets: BackupPresetRow[];
  /** Song→preset bindings; empty when the DB lacks `Songs`/`SongPresets`. */
  song_presets: SongPresetBinding[];
  /** Full `Songs` table (reuses the live read type); empty when the DB lacks it. */
  songs: SongRecord[];
  /** Full `Setlists` table (reuses the live read type); empty when the DB lacks it. */
  setlists: SetlistRecord[];
  /** `SetlistSongs` membership; empty when the DB lacks it. */
  setlist_songs: BackupSetlistSong[];
}

/** `tmp://backup-progress` (`session::BackupProgress`) — drives the Presets-tab scan
 * strip. `phase` is `"building"` (device assembling the archive, before chunks) or
 * `"streaming"` (chunks arriving — `percent` is exact). */
export interface BackupProgress {
  phase: "building" | "streaming";
  received: number;
  total: number;
  bytes: number;
  total_bytes: number;
  percent: number;
  build_size: number;
  build_ticks: number;
}

// ─── Doctor (tone diagnosis) ─────────────────────────────────────────────────

/** One sound for `doctor_check` (`lib::DoctorInput`, camelCase wire): a
 * preset's base (`scene: null`) or one scene (0-based `scenes[]` wire index).
 * `nodes` is the preset's chain from the backup scan's graph, passed verbatim
 * (`ActiveGraph.nodes`) so prescriptions target real blocks with no extra
 * device reads. */
export interface DoctorInputArg {
  key: string;
  listIndex: number;
  scene: number | null;
  /** 0-based `ftsw` array index for a block-acting footswitch sound; null for
   *  Base/scene sounds. */
  footswitch: number | null;
  label: string;
  tag: string | null;
  topologyId: string | null;
  calibrationLufs: number | null;
  /** Instrument profile id (null when "none"): when it has a stored Tier-2 DI
   *  capture, the Doctor reads that WAV verbatim and diagnoses in CAPTURE
   *  threshold space; else the synthetic topology sample. */
  profileId: string | null;
  nodes: GraphNode[];
  /** The preset's block-acting footswitches (empty when none/unknown), off the
   *  same backup-scan data as `nodes` — drives the backend's OFFLINE force-bypass
   *  isolation derivation (no live preset read per sound). */
  footswitches: FootswitchInfo[];
}

/** Streamed per-sound progress row (`lib::DoctorProgressItem`). Diagnoses ride
 * the command's return value (assembled once every sound is measured). */
export interface DoctorProgressItem {
  key: string;
  status: "active" | "done" | "error";
  message: string | null;
}

export type DoctorSev = "high" | "med";
export type DoctorRxKind = "oneclick" | "advisory" | "chain";

/** One concrete device edit inside a prescription (`doctor::DoctorOp`). */
export type DoctorOp =
  | {
      kind: "param";
      groupId: string;
      nodeId: string;
      param: string;
      value: number;
    }
  | {
      kind: "insert_node";
      groupId: string;
      beforeFenderId: string | null;
      fenderId: string;
      params: [string, number][];
    };

/** Chain-preview DTO on a `chain`-kind prescription: the resulting block list
 * by model id (the UI resolves art through its existing strip engine). */
export interface DoctorChainPreview {
  template: string;
  blocks: { model: string; added?: boolean }[];
}

/** One prescription (`doctor::Rx`). `ops` is empty for advisory cards. */
export interface DoctorRx {
  kind: DoctorRxKind;
  title: string;
  detail: string;
  cpuNote: string;
  ops: DoctorOp[];
  chain?: DoctorChainPreview;
}

/** One diagnosis (`doctor::LeveledDiag` — a flattened `Diag` plus `fromLevel`).
 * `bands` indexes the sound's band layout (`DoctorSoundResult.bandLabels` — 6
 * player bands, or 7 with "Sub" first for bass-vi); empty = time-domain.
 * `fromLevel` is the quietest playback level at which the finding fires (the
 * sound is diagnosed at all three): `"quiet"` = a problem at any volume,
 * `"rehearsal"`/`"stage"` = only appears at that volume and louder. */
export interface DoctorDiag {
  key: string;
  label: string;
  sev: DoctorSev;
  /** Magnitude past the fire threshold in the rule's natural unit (dB, or LU for
   *  spiky) — `metric − threshold`, ≥ 0. `< 1.0` (below `POSSIBLE_MAX_SEVERITY`)
   *  is a near-threshold "possible" verdict rendered muted. */
  severity: number;
  bands: number[];
  detail: string;
  explain: string;
  rx: DoctorRx[];
  fromLevel: PlaybackLevel;
}

/** The "does this cut through the mix?" ESTIMATE (`doctor::CutThrough`) —
 * a presence-contrast reading vs the measured 25-preset factory-bank
 * distribution, not a diagnosis: it's not in `diags` and carries no `rx`.
 * `factoryPercentile`/`advisory` are null/false outside Guitar (no bass
 * factory anchor yet). */
export interface CutThrough {
  contrastDb: number;
  factoryPercentile: number | null;
  advisory: boolean;
}

export interface DoctorSoundResult {
  key: string;
  listIndex: number;
  scene: number | null;
  /** 0-based `ftsw` array index for a block-acting footswitch sound; null for
   *  Base/scene sounds. */
  footswitch: number | null;
  label: string;
  tag: string | null;
  diags: DoctorDiag[];
  integratedLufs: number;
  tailRatioDb: number;
  balanceDb: number[];
  /** Display labels for this sound's band layout — 6 for guitar/bass ("Lows" …
   *  "Air") or 7 for bass-vi ("Sub" + the same six). `balanceDb.length` and
   *  `DoctorDiag.bands` indices both index this same layout. */
  bandLabels: string[];
  /** Null when this sound's capture failed or the ratio was degenerate (e.g.
   *  a silent capture). */
  cutThrough: CutThrough | null;
  /** Set when this sound's capture failed (no diags then); the run continued. */
  error: string | null;
}

export interface DoctorSceneDeltaRow {
  name: string;
  tag: string | null;
  deltaDb: number;
  isRef: boolean;
}

/** Scene-loudness consistency for one preset (`doctor::SceneConsistency`). */
export interface DoctorSceneConsistency {
  rows: DoctorSceneDeltaRow[];
  worstName: string;
  worstDeltaDb: number;
  rx: DoctorRx[];
}

export interface DoctorPresetResult {
  listIndex: number;
  sounds: DoctorSoundResult[];
  sceneConsistency: DoctorSceneConsistency | null;
}

export interface DoctorCheckResult {
  presets: DoctorPresetResult[];
  stopped: boolean;
}

/** One prescription's apply job (`lib::DoctorApplyJob`, camelCase wire).
 * `name` is the identity guard — apply refuses if the loaded slot's name
 * doesn't match. Scene-trim ops are NOT accepted here (the frontend routes
 * them through the existing scene-leveling command). */
export interface DoctorApplyJob {
  listIndex: number;
  name: string;
  ops: DoctorOp[];
  topologyId: string | null;
  calibrationLufs: number | null;
  /** The diagnosed sound's instrument-profile id (null when "none") — a profile
   *  with a stored Tier-2 DI capture auditions the A/B on that same capture,
   *  mirroring `doctor_check`'s stimulus resolution. */
  profileId: string | null;
  /** The diagnosed sound's own scene (0-based `scenes[]` wire index) — null
   *  for Base/footswitch. The A/B captures recall this scene so the player
   *  auditions the fix in the state that was actually diagnosed. */
  scene: number | null;
  /** The diagnosed sound's own block-acting footswitch (0-based `ftsw`
   *  index) — null for Base/scene. */
  footswitch: number | null;
  /** The preset's chain (same data threaded into `DoctorInputArg.nodes`) —
   *  drives the backend's OFFLINE force-bypass isolation derivation for the
   *  A/B captures, mirroring `doctor_check`'s isolation exactly. */
  nodes: GraphNode[];
  /** The preset's block-acting footswitches, paired with `nodes` for the
   *  same isolation derivation. */
  footswitches: FootswitchInfo[];
}

/** Result of a live (unsaved) prescription apply: before/after audition clips
 * as `data:audio/wav;base64,…` URLs, rendered in the same command so the A/B
 * compares the stored state against the applied-but-unsaved edit buffer. */
export interface DoctorApplyResult {
  beforeClip: string;
  afterClip: string;
}
