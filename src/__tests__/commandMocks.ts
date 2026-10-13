// src/__tests__/commandMocks.ts — the TYPED invoke mock, built on the command registry.
//
// `setup.ts` mocks `@tauri-apps/api/core` globally and resolves every command through
// `resolveCommand` below, so every default here is a `Handlers` entry whose return is
// checked against `Commands[K]["ret"]` — a default (or a per-test override passed to
// `mockCommands`) with the wrong shape is a `tsc` error, not a silent drift.
//
// Per test: `mockCommands({ list_presets: () => [preset], … })` overrides only the
// commands named (args arrive typed as `Commands[K]["args"]`); every other command
// falls through to its default. A handler may return a value or a promise, or throw,
// which rejects the invoke like a backend `Err`. `callsOf("level_preset")` reads the
// typed args each call of a command was sent.
//
// Factories (`makeStore()`, `makeLevelResult()`, …) build a complete, typed return
// shape with per-test overrides.

import { vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import type {
  CommandArgs,
  CommandName,
  CommandRet,
  Commands,
} from "../lib/commands";
import type {
  ActiveGraph,
  BackupPresetRow,
  BackupReadResult,
  CalibrateResult,
  FootswitchLevelResult,
  LevelResult,
  Store,
} from "../lib/types";

/** A mock of command `K`: its typed args in, its typed return (or a promise of it) out. */
export type Handler<K extends CommandName> = (
  args: CommandArgs<K>,
) => CommandRet<K> | Promise<CommandRet<K>>;

/** One handler per command. */
export type Handlers = { [K in CommandName]: Handler<K> };

// ─── Factories ────────────────────────────────────────────────────────────────

/** The persisted store with no profiles/targets. */
export const makeStore = (over: Partial<Store> = {}): Store => ({
  profiles: [],
  profile_by_slot: {},
  targets: [],
  playback_level: "stage",
  auto_install_updates: true,
  meter_convention: 1,
  ...over,
});

/** A clean Tier-2 calibration capture. */
export const makeCalibrateResult = (
  over: Partial<CalibrateResult> = {},
): CalibrateResult => ({
  lufs: -20,
  clipped: false,
  stimulus_shortfall_lu: null,
  spread_lu: 3,
  band_coverage: [true, true, true, true, true, true],
  band_labels: ["Lo", "LoM", "Mid", "HiM", "Hi", "Air"],
  ...over,
});

/** A verified, unclamped base-leveling result for list index 0 at −26 LUFS. */
export const makeLevelResult = (
  over: Partial<LevelResult> = {},
): LevelResult => ({
  slot: 0,
  scene_slot: null,
  ref_level: 0.5,
  measured_lufs: -13,
  constant_c: -13,
  final_level: 0.18,
  target_lufs: -26,
  predicted_lufs: -26,
  clamped: false,
  saved: true,
  verify_lufs: -26,
  verification: "verified",
  iterations: 1,
  dynamic_spread_lu: 1.1,
  clamp_reason: null,
  verify_by_ear: false,
  previous_level: null,
  true_peak_dbtp: null,
  persist_mismatch: null,
  clamp_kind: null,
  trade: null,
  base_boost: null,
  ...over,
});

/** A converged, unclamped footswitch-leveling result for switch 0 at −26 LUFS. */
export const makeFootswitchLevelResult = (
  over: Partial<FootswitchLevelResult> = {},
): FootswitchLevelResult => ({
  switch: 0,
  measured_lufs: -26,
  final_value: 0.5,
  target_lufs: -26,
  predicted_lufs: -26,
  clamped: false,
  unconverged: false,
  clamp_reason: null,
  wet_floor: false,
  saved: true,
  verify_lufs: -26,
  iterations: 1,
  dynamic_spread_lu: null,
  method: "baked",
  persist_mismatch: null,
  clamp_kind: null,
  ...over,
});

/** An empty signal graph. */
export const makeActiveGraph = (
  over: Partial<ActiveGraph> = {},
): ActiveGraph => ({
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
  ...over,
});

/** A backup-scan preset row with no scenes, blocks or footswitches. */
export const makeBackupPresetRow = (
  over: Partial<BackupPresetRow> = {},
): BackupPresetRow => ({
  slot: 1,
  name: "Preset",
  preset_id: null,
  scene_count: 0,
  scenes: [],
  amp_candidates: [],
  base_active_amp_count: 0,
  blocks: [],
  graph: makeActiveGraph(),
  footswitches: [],
  silence_hint: null,
  scene_handles: [],
  base_handles: [],
  scene_overrides: [],
  fs_scene_contexts: [],
  ...over,
});

/** A whole-library backup read with nothing in it. */
export const makeBackupRead = (
  over: Partial<BackupReadResult> = {},
): BackupReadResult => ({
  members: [],
  db_bytes: 0,
  total_rows: 0,
  scene_mode: "test",
  presets: [],
  song_presets: [],
  songs: [],
  setlists: [],
  setlist_songs: [],
  ...over,
});

// ─── Defaults: the EMPTY / no-data variant of every command ─────────────────────

const none = (): undefined => undefined;

/** Every command's default — enough for any screen to mount and render its empty
 * state. `Handlers` (not `Partial`) so a new registry command must get a default. */
export const DEFAULT_HANDLERS: Handlers = {
  // connection + app
  app_info: () => ({
    name: "TMP Companion",
    version: "0.0.0-test",
    os: "macos",
  }),
  connect_device: () => ({ firmware: null, graph: null }),
  list_presets: () => [],

  // leveling
  list_level_blocks: () => [],
  level_preset: ({ job }) =>
    makeLevelResult({
      slot: job.slot,
      target_lufs: job.target_lufs,
      predicted_lufs: job.target_lufs,
      verify_lufs: job.target_lufs,
      saved: job.save,
    }),
  cancel_preset_leveling: none,
  level_scenes_apply_batched: () => [],
  cancel_scene_leveling: none,
  list_scene_level_handles: () => [],
  list_footswitch_scene_contexts: () => [],
  level_footswitches_apply: () => [],
  cancel_footswitch_leveling: none,

  // doctor
  doctor_check: () => ({ presets: [], stopped: false }),
  cancel_doctor_check: none,
  doctor_apply: () => ({ beforeClip: "", afterClip: "" }),
  doctor_save: none,
  doctor_discard: none,

  // profiles / store / support
  get_store: () => makeStore(),
  save_profiles: none,
  save_targets: none,
  set_playback_level: none,
  set_auto_install_updates: none,
  calibrate_profile: () => makeCalibrateResult(),
  list_pickup_topologies: () => [],
  save_support_bundle: () => ({ path: "/tmp/support-bundle.tar" }),
  build_support_bundle: () => new ArrayBuffer(0),

  // active preset + library
  read_active_preset: () => makeActiveGraph(),
  current_graph: () => null,
  read_library_via_backup: () => makeBackupRead(),
  copy_apply: () => [],

  // songs & setlists — every read and write returns a (fresh) list
  list_songs: () => [],
  read_setlists: () => [],
  list_setlist_songs: () => [],
  remove_song: () => [],
  add_setlist: () => [],
  rename_setlist: () => [],
  remove_setlist: () => [],
  remove_setlist_song: () => [],
  move_setlist_song: () => [],
  create_song_full: () => ({ songs: [], members: null, bpm_warning: null }),
  update_song_full: () => ({ songs: [], members: null, bpm_warning: null }),
  add_setlist_songs: () => [],
};

const isCommand = (command: string): command is CommandName =>
  command in DEFAULT_HANDLERS;

/** Resolve one mocked invoke: the per-test handler if given, else the default. A
 * command outside the registry (a plugin's own IPC, e.g. `plugin:log|log`) resolves
 * `null`. A throwing handler rejects, like a backend `Err`. */
export function resolveCommand(
  handlers: Partial<Handlers>,
  command: string,
  args: unknown,
): Promise<unknown> {
  if (!isCommand(command)) return Promise.resolve(null);
  const handler = handlers[command] ?? DEFAULT_HANDLERS[command];
  // `args` is whatever the wrapper sent for THIS command — the registry types it.
  return new Promise((resolve) => {
    resolve(handler(args as never));
  });
}

/** Route the global `invoke` mock through `handlers` (overrides) for the rest of the
 * test, every other command falling through to its default. */
export function mockCommands(handlers: Partial<Handlers> = {}): void {
  vi.mocked(invoke).mockImplementation((command: string, args?: unknown) =>
    resolveCommand(handlers, command, args),
  );
}

/** The typed args of every call of `command` so far, in call order. */
export function callsOf<K extends CommandName>(command: K): CommandArgs<K>[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([c]) => c === command)
    .map(([, args]) => args as CommandArgs<K>);
}

/** Has `command` been invoked at all? */
export const fired = (command: keyof Commands): boolean =>
  vi.mocked(invoke).mock.calls.some(([c]) => c === command);
