// src/views/level/leveling.ts — types + helpers for the unified leveling flow.
//
// The unit of leveling is a SCENE. A preset's BASE scene carries cross-preset
// loudness ("levels this preset against the others" → preset `presetLevel`); an FS
// scene is leveled within its preset ("levels this scene against the preset's base"
// → amp `outputLevel` in scene mode). The mechanism is never exposed: no block /
// parameter selector, the target is implicit and fixed.
//
// SELECTION lives in the list (the scene tree): the source of truth is a flat set of
// scene KEYS — Base = `p${slot}`, FS scene = `s${slot}:${idx}`. `chosenFrom` turns
// that set into the SetupOption[] the setup dialog configures (instrument + target).
//
// Flow (one persistent wizard, body swaps per stage): setup (set instrument + target
// for everything picked in the list; its footer's backup acknowledgment gates the
// commit) → run (steps the chosen scenes) → summary.

import type {
  FootswitchInfo,
  LevelJob,
  LevelParamCandidate,
  Profile,
  SceneInfo,
  SilenceHint,
} from "../../lib/types";
import type { PresetRow } from "../PresetList";
import type { PickOption } from "../overlays/Pick";
import { shortFallback } from "../../models/blockArt";

// ── selection scene-key helpers (shared by the list + the flow) ─────────────

/** The wire scene slot the device uses for a preset's BASE (a constant, NOT a `scenes[]`
 *  index — mirrors `session::BASE_SCENE_SLOT`). Redistribution levels the base amp at this
 *  slot alongside the FS scenes. */
export const BASE_SCENE_SLOT = 8;

/** The Base scene key for a preset slot (selecting the whole preset includes it). */
export const baseKey = (slot: number): string => `p${String(slot)}`;
/** The key for the i-th (0-based) footswitch scene of a preset slot. */
export const sceneKeyOf = (slot: number, i: number): string =>
  `s${String(slot)}:${String(i)}`;
/** The key for the i-th (0-based) levelable FOOTSWITCH of a preset slot. `i` indexes
 *  the SAME levelable footswitch list everywhere (the backup-cached, level-params-
 *  filtered one), so the key is stable across the list, selection, and the flow. */
export const fswKey = (slot: number, i: number): string =>
  `f${String(slot)}:${String(i)}`;
/** Every selectable child key for a preset: Base, then one per FS scene, then one per
 *  levelable footswitch. Scenes and footswitches share the key space (distinct prefix). */
export function childKeys(
  slot: number,
  scenes: SceneInfo[],
  footswitches: FootswitchInfo[],
): string[] {
  return [
    baseKey(slot),
    ...scenes.map((_, i) => sceneKeyOf(slot, i)),
    ...footswitches.map((_, i) => fswKey(slot, i)),
  ];
}

/** The leveling coordinates a footswitch row carries into `levelFootswitchesApply`:
 *  the `ftsw` switch index + the block param to solve (the backend classifies bake vs
 *  assign). Built from `FootswitchInfo` (switch + its first level candidate). */
export interface FootswitchTarget {
  /** 0-based `ftsw` array index (the wire footswitch address). */
  switchIndex: number;
  levGroupId: string;
  levNodeId: string;
  levParameterId: string;
}

/** Block params that change loudness without changing the SOUND — the tone-safe
 *  leveling targets. Anything else (gain/tone/drive…) also alters the tone. */
const LOUDNESS_PARAMS = new Set([
  "level",
  "outputLevel",
  "output",
  "mix",
  "volume",
]);
/** True when adjusting this parameter changes loudness only (not the tone). */
export const isLoudnessParam = (p: string): boolean => LOUDNESS_PARAMS.has(p);
/** The tone-safe default candidate index: the first loudness-only param, else the
 *  first candidate. Replaces the old alphabetical-first `[0]` pick, which could land
 *  on a gain/tone knob and change the sound while leveling. */
export function defaultParamIndex(params: LevelParamCandidate[]): number {
  const i = params.findIndex((c) => isLoudnessParam(c.parameter_id));
  return i >= 0 ? i : 0;
}

/** The apply-to-all instrument's place on the good → better → best ladder that drives
 *  the Set up step's instrument nudge: `none` (no instrument → levels against the
 *  default reference) → `uncal` (instrument, no stored calibration) → `cal`
 *  (calibrated). An unknown / empty id is treated as `none`. */
export function instCalState(
  id: string,
  options: PickOption[],
): "none" | "uncal" | "cal" {
  if (!id || id === "none") return "none";
  const o = options.find((x) => x.id === id);
  if (!o) return "none";
  return o.calibrated ? "cal" : "uncal";
}

/** Build leveling coordinates from a specific candidate (the user's chosen param, or
 *  the default). The backend classifies bake vs assign from these ids. */
export function targetFromCandidate(
  switchIndex: number,
  c: LevelParamCandidate,
): FootswitchTarget {
  return {
    switchIndex,
    levGroupId: c.group_id,
    levNodeId: c.node_id,
    levParameterId: c.parameter_id,
  };
}

/** The display footswitch number for a switch index (human FS tag = index + 1 — the
 *  same +1 scene rows use, verified against `footswitch::scene_fs_map`). */
const fsTagOf = (switchIndex: number): string => `FS${String(switchIndex + 1)}`;

/** The instrument `Pick` options shared by the Level and Doctor setup steps:
 *  "None" (the no-instrument path — level/diagnose against the default reference)
 *  followed by each saved profile, calibrated ones flagged with their reference dB. */
export function instrumentOptions(
  profiles: Profile[] | undefined,
): PickOption[] {
  return [
    { id: "none", label: "None" },
    ...(profiles ?? []).map((p) => {
      const cal = p.calibration_lufs;
      return {
        id: p.id,
        label: p.name,
        sub: cal != null ? `${cal.toFixed(1)} dB` : undefined,
        calibrated: cal != null,
      };
    }),
  ];
}

/** Resolve an instrument profile id → its display name (the run-row chip); falls
 *  back to the raw id for an unknown/removed profile. */
export function instrumentName(
  profiles: Profile[] | undefined,
  id: string,
): string {
  return (profiles ?? []).find((p) => p.id === id)?.name ?? id;
}

/** The row name for a footswitch: the player's own `customLabel` when set, else the
 *  toggled block's friendly name (many presets leave the label blank — a nameless row
 *  is useless, so fall back to e.g. "Tube Screamer" from the leveled block's id). */
export function footswitchName(f: FootswitchInfo): string {
  const label = f.label.trim();
  if (label) return label;
  if (f.level_params.length > 0)
    return shortFallback(
      f.level_params[defaultParamIndex(f.level_params)].fender_id,
    );
  return "Footswitch";
}

/** Resolve a levelable footswitch's DEFAULT leveling coordinates (its tone-safe
 *  candidate). The Set up step can override the candidate per row; null when the
 *  footswitch has no candidate (it should have been filtered out upstream). */
function footswitchTarget(f: FootswitchInfo): FootswitchTarget | null {
  // Length-guard rather than `!candidate` — the array index type lies (no
  // noUncheckedIndexedAccess), so the truthiness check reads as "always truthy".
  if (f.level_params.length === 0) return null;
  return targetFromCandidate(
    f.switch,
    f.level_params[defaultParamIndex(f.level_params)],
  );
}

// ── setup: one selectable row (Base or an FS scene) ─────────────────────────

export interface SetupOption {
  /** Unique key: `p${slot}` for Base, `s${slot}:${idx}` for a scene. */
  key: string;
  /** 0-based list index of the owning preset. */
  slot: number;
  presetName: string;
  /** Base scene (cross-preset) vs an FS scene (within-preset). */
  isBase: boolean;
  /** The `loadScene` / `level_scenes_apply` wire slot (0-based scenes[] index);
   *  null for the Base/whole-preset row (which levels `presetLevel`). */
  sceneSlot: number | null;
  /** Display name: "Base" / "Whole preset" / the scene name. */
  sceneName: string;
  /** Tag chip: "BASE" | `FS${n}` | null (em-dash for an untagged named scene). */
  tag: string | null;
  /** False ⇒ a scene-less preset, whose Base row renders "Whole preset". */
  hasScenes: boolean;
  /** Set ⇒ this row is a block-acting FOOTSWITCH (not Base/scene); carries the coords
   *  for `levelFootswitchesApply`. null/undefined for Base + scene rows. */
  footswitch?: FootswitchTarget | null;
  /** The footswitch's full levelable-parameter candidates (drives the Set up param
   *  picker). Present only on footswitch rows; the chosen one is baked into
   *  `footswitch` when the run starts. */
  levelParams?: LevelParamCandidate[];
}

/** A chosen setup row + its resolved instrument id and target name (the setup
 *  dialog emits one per option on "Level"; the flow turns each into a RunItem). */
export interface SetupChoice {
  option: SetupOption;
  instId: string;
  targetName: string;
}

/** Resolve the scene keys SELECTED in the list into the setup rows to configure.
 *  Walks every non-empty preset (sorted, Base-first) and emits a SetupOption for
 *  each of its keys present in `sel`. Everything returned WILL be leveled — the
 *  setup dialog only sets each sound's instrument + target, never re-gates it. */
export function chosenFrom(
  sel: Set<string>,
  rows: PresetRow[],
  sceneInfo: Map<number, SceneInfo[]>,
  footswitchInfo: Map<number, FootswitchInfo[]>,
): SetupOption[] {
  const items: SetupOption[] = [];
  [...rows]
    .filter((r) => !r.empty)
    .sort((a, b) => a.slot - b.slot)
    .forEach((r) => {
      const scenes = sceneInfo.get(r.slot) ?? [];
      const footswitches = footswitchInfo.get(r.slot) ?? [];
      // A footswitch row reads like a scene (the user picks "a sound"), so a preset with
      // ONLY footswitches still shows "Base" vs "Whole preset" as a true scene-less case.
      const hasChildren = scenes.length > 0 || footswitches.length > 0;
      if (sel.has(baseKey(r.slot))) {
        items.push({
          key: baseKey(r.slot),
          slot: r.slot,
          presetName: r.name,
          isBase: true,
          sceneSlot: null,
          sceneName: hasChildren ? "Base" : "Whole preset",
          tag: hasChildren ? "BASE" : null,
          hasScenes: hasChildren,
        });
      }
      scenes.forEach((sc, i) => {
        if (sel.has(sceneKeyOf(r.slot, i))) {
          items.push({
            key: sceneKeyOf(r.slot, i),
            slot: r.slot,
            presetName: r.name,
            isBase: false,
            sceneSlot: i, // the row index IS the 0-based wire sceneSlot
            sceneName: sc.name,
            tag: sc.fs != null ? `FS${String(sc.fs)}` : "—",
            hasScenes: true,
          });
        }
      });
      footswitches.forEach((f, i) => {
        const target = footswitchTarget(f);
        if (target && sel.has(fswKey(r.slot, i))) {
          items.push({
            key: fswKey(r.slot, i),
            slot: r.slot,
            presetName: r.name,
            isBase: false,
            sceneSlot: null,
            sceneName: footswitchName(f),
            tag: fsTagOf(f.switch),
            hasScenes: true,
            footswitch: target,
            levelParams: f.level_params,
          });
        }
      });
    });
  return items;
}

// ── run / summary: one item per chosen scene ────────────────────────────────

// `offbranch` is its OWN outcome (not a flavor of `clamped`): the amp doesn't reach the
// USB 1/2 capture, so re-leveling can't fix it — only a routing change on the unit can.
export type Outcome = "done" | "clamped" | "offbranch" | "skipped";

/** Dynamics-spread flag threshold (LU): short-term-max − integrated above this
 *  marks a DYNAMIC sound — the gated reading understates its peaks vs a
 *  compressed one, so the leveled result deserves an ear-check. */
export const DYNAMIC_SPREAD_LU = 6;

export interface RunItem {
  key: string;
  /** 0-based list index of the preset. */
  slot: number;
  presetName: string;
  isBase: boolean;
  /** 0-based scenes[] wire slot, or null for the Base/whole-preset step. */
  sceneSlot: number | null;
  sceneName: string;
  tag: string | null;
  /** Set ⇒ a block-acting FOOTSWITCH step (dispatched to `levelFootswitchesApply`);
   *  null/undefined ⇒ Base (`level_preset`) or FS scene (`level_scenes_apply_batched`). */
  footswitch?: FootswitchTarget | null;
  /** Chosen instrument profile id ("" when none). */
  instId: string;
  /** Chosen target name. */
  targetName: string;
  /** "<preset> · <scene>" or "<preset>" (scene-less). */
  label: string;
  // live + final:
  status: "queued" | "active" | "result";
  outcome?: Outcome;
  /** Measured loudness (verify/predicted), or null. */
  value?: number | null;
  /** Dynamics spread of the measure capture (LU); drives the "dynamic" by-ear cause. */
  spreadLu?: number | null;
  /** The preset's saved `presetLevel` before this run wrote it — enables the Summary
   *  "Restore original" (Base rows only; scene/footswitch writes aren't revertable). */
  previousLevel?: number | null;
  /** PREDICTED true peak (dBTP) at the leveled setting — an estimate, never a
   *  re-measurement. Only Base rows carry a value (undefined/null elsewhere); drives
   *  the Summary "may clip" chip when > −1 dBTP. */
  truePeakDbtp?: number | null;
  /** Cause of the "verify by ear" marker (undefined = no flag): `envelope` = the preset
   *  contains an envelope-follower effect, which tracks the synthetic stimulus differently
   *  than real playing (the measurement itself is suspect); `dynamic` = peaks ride
   *  above the gated average; `rebalance` = shallow lane-mute isolation made the parallel
   *  balance approximate. Resolved to a single cause when the RunItem is built. */
  verifyByEar?: "envelope" | "dynamic" | "rebalance";
  /** The preset's backup-scan silence hint, stamped at item build — refines the
   *  offbranch row status (see `offbranchStatus`). */
  silenceHint?: SilenceHint;
}

/** The offbranch ("silent capture") row status, refined by the preset's JSON-visible
 *  cause when the backup scan found one. Rendered verbatim in RunBody + SummaryBody. */
export function offbranchStatus(hint: SilenceHint | undefined): string {
  if (hint === "amp_zero") return "amp output at zero";
  if (hint === "exp_mute") return "exp pedal may mute";
  return "not on USB 1/2";
}

/** Turn a checked setup row into a run item with its resolved instrument + target. */
export function optionToRunItem(
  o: SetupOption,
  instId: string,
  targetName: string,
): RunItem {
  const label = o.isBase
    ? o.hasScenes
      ? `${o.presetName} · Base`
      : o.presetName
    : `${o.presetName} · ${o.sceneName}`;
  return {
    key: o.key,
    slot: o.slot,
    presetName: o.presetName,
    isBase: o.isBase,
    sceneSlot: o.sceneSlot,
    sceneName: o.sceneName,
    tag: o.tag,
    footswitch: o.footswitch ?? null,
    instId,
    targetName,
    label,
    status: "queued",
  };
}

/** Rebuild a setup row from a (clamped) run item — for "Re-level clamped…", which
 *  reopens setup pre-loaded with just the clamped scenes, all checked, no scan.
 *  ponytail: the RunItem doesn't carry `levelParams`, so a re-leveled footswitch keeps
 *  its already-chosen param but can't be re-picked (the param column renders empty).
 *  Add `levelParams` to RunItem if re-pick-on-relevel is ever wanted. */
export function runItemToOption(it: RunItem): SetupOption {
  return {
    key: it.key,
    slot: it.slot,
    presetName: it.presetName,
    isBase: it.isBase,
    sceneSlot: it.sceneSlot,
    sceneName: it.sceneName,
    tag: it.tag,
    hasScenes: !it.isBase || it.tag != null,
    footswitch: it.footswitch ?? null,
  };
}

// The wizard's stage machine + run state now live in the flow hook
// (useLevelingFlow → Stage / RunState); this module just owns the per-scene types.

// ── the preset-level (Base) job builder ─────────────────────────────────────

/** Build a `level_preset` job (Base / whole-preset leveling via `presetLevel`).
 *  FS scenes use `level_scenes_apply_batched` instead (amp `outputLevel`). */
export function buildLevelJob(
  slot: number,
  targetLufs: number,
  profile: Profile | null,
  save: boolean,
): LevelJob {
  return {
    slot,
    target_lufs: targetLufs,
    save,
    topology_id: profile?.topology_id ?? null,
    calibration_lufs: profile?.calibration_lufs ?? null,
    profile_id: profile?.id ?? null,
    block_group_id: null,
    block_node_id: null,
    block_parameter_id: null,
    block_value: null,
  };
}
