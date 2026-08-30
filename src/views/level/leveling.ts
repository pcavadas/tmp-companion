// src/views/level/leveling.ts — types + helpers for the unified leveling flow.
//
// The unit of leveling is a SCENE. A preset's BASE scene carries cross-preset
// loudness ("matches this preset’s loudness to your other presets" → preset
// `presetLevel`); an FS scene is leveled within its preset ("matches this scene's
// loudness to the preset’s base sound" → amp `outputLevel` in scene mode). The
// mechanism is never exposed: no block /
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
  ClampKind,
  FootswitchInfo,
  LevelJob,
  LevelParamCandidate,
  ParamClass,
  Profile,
  SceneInfo,
  SilenceHint,
  TradeSummary,
} from "../../lib/types";
import type { PresetRow } from "../PresetList";
import type { PickOption } from "../overlays/Pick";
// `SceneHandlePick` is declared ONCE, as the `levelScenesApplyBatched` wire type in
// invoke.ts (its `SceneLevelJobWire.handle` field) — re-exported below so
// `SetupOption`/`RunItem` and the scene-handle-picker wiring can import it from
// either module without a second, driftable copy.
import type { SceneHandlePick } from "../../lib/invoke";
import { blockArtTile, shortFallback } from "../../models/blockArt";
import { stripNameFor } from "../../models/catalog";
import { slotLabel } from "../../lib/format";

export type { SceneHandlePick };

// ── selection scene-key helpers (shared by the list + the flow) ─────────────

/** The wire scene slot the device uses for a preset's BASE (a constant, NOT a `scenes[]`
 *  index — mirrors `session::BASE_SCENE_SLOT`). Redistribution levels the base amp at this
 *  slot alongside the FS scenes. Re-exported from the wire mirror so the value can't drift. */
export { BASE_SCENE_SLOT } from "../../lib/types";

/** The Base scene key for a preset slot (selecting the whole preset includes it). */
export const baseKey = (slot: number): string => `p${String(slot)}`;
/** The key for the i-th (0-based) footswitch scene of a preset slot. */
export const sceneKeyOf = (slot: number, i: number): string =>
  `s${String(slot)}:${String(i)}`;
/** The key for the i-th (0-based) FOOTSWITCH of a preset slot — `i` is that switch's
 *  ORIGINAL POSITION in the backup-cached per-preset footswitch array (the full
 *  roster, levelable or not, since BUG 1: a switch with no level control still gets a
 *  row, disabled), so the key stays stable across the list, selection, and the flow
 *  regardless of which siblings are levelable. */
export const fswKey = (slot: number, i: number): string =>
  `f${String(slot)}:${String(i)}`;
/** THE ONE shared definition of "this footswitch can be leveled" — a real level-class
 *  candidate to solve. Every consumer that decides whether a footswitch row is
 *  selectable/counted (`childKeys`, `footswitchTarget`) or disabled-with-a-reason
 *  (`PresetRow`'s row builder) must call this, not re-derive `level_params.length`
 *  itself — that duplication is exactly how BUG 1 (a "PHASER" switch with no level
 *  control silently vanishing between the list and the wizard) happened: `childKeys`
 *  counted the row as selectable while the row builder dropped it. */
export function footswitchLevelable(f: FootswitchInfo): boolean {
  return f.level_params.length > 0;
}

/** Every selectable child key for a preset: Base, then one per FS scene, then one per
 *  levelable footswitch. Scenes and footswitches share the key space (distinct
 *  prefix). A footswitch with no level control (`!footswitchLevelable`) is NOT
 *  selectable — it is shown in the list disabled instead (`PresetRow`) — but its
 *  SIBLINGS keep their true array position: this flatMaps over the full array rather
 *  than filtering first, so a later levelable switch's `fswKey` index can't shift
 *  when an earlier one is skipped. */
export function childKeys(
  slot: number,
  scenes: SceneInfo[],
  footswitches: FootswitchInfo[],
): string[] {
  return [
    baseKey(slot),
    ...scenes.map((_, i) => sceneKeyOf(slot, i)),
    ...footswitches.flatMap((f, i) =>
      footswitchLevelable(f) ? [fswKey(slot, i)] : [],
    ),
  ];
}

/** The leveling coordinates a footswitch row carries into `levelFootswitchesApply`: the
 *  `ftsw` switch index + the block param to solve + the scene context it is measured in
 *  (D3). Every row levels now — the old verify-only "no handle" mode is gone backend-side
 *  ("every row levels" — `FootswitchLevelJob`'s doc), so this is a plain shape, not a
 *  discriminated union. */
export interface FootswitchTarget {
  /** 0-based `ftsw` array index (the wire footswitch address). */
  switchIndex: number;
  levGroupId: string;
  levNodeId: string;
  levParameterId: string;
  /** THE SCENE CONTEXT this switch's sound is measured and solved in (D3): a 0-based
   *  `scenes[]` wire slot, or `null` = the preset's BASE sound (the historical default).
   *  `null` until `list_footswitch_scene_contexts` resolves the picker's `suggested`
   *  scene AND the user (or the picker's own default-fill) picks it. */
  sceneContext: number | null;
}

/** Rank a candidate's WIRE-CARRIED class for `defaultParamIndex`: a genuine level
 *  control (linear or dB) ranks above a wet/dry mix (which changes loudness but also
 *  the effect's presence). There is no "unclassified" rank to skip — the backend
 *  (`footswitch::level_candidates_for_node`) admits only params the classifier
 *  recognises, so every candidate the frontend ever sees already carries a real class. */
const CLASS_RANK: Record<ParamClass, number> = {
  level_linear: 0,
  level_db: 0,
  wet_mix: 1,
};

/** Friendly labels for the technical parameter ids (fallback: capitalize the id).
 *  Shared by `FsParamPick` and `SceneLevelPick` — the two "which block parameter"
 *  pickers — so the dictionary can't drift between them. */
const PARAM_LABELS: Partial<Record<string, string>> = {
  level: "Level",
  outputLevel: "Output level",
  output: "Output",
  mix: "Mix",
  volume: "Volume",
  gain: "Gain",
  drive: "Drive",
  tone: "Tone",
  fuzz: "Fuzz",
  treble: "Treble",
  bass: "Bass",
  presence: "Presence",
};
export function paramLabel(p: string): string {
  return PARAM_LABELS[p] ?? (p ? p.charAt(0).toUpperCase() + p.slice(1) : "");
}

/** The tone-safe default candidate index: the best-classified (level over wet-mix)
 *  candidate off the WIRE `class` field, tie-broken to the first match. Returns `-1`
 *  only for an EMPTY list — every candidate the backend offers already carries a real
 *  (never "other") class, so a non-empty list always has a valid default; callers with
 *  a non-empty `params` can treat the result as always `>= 0`. */
export function defaultParamIndex(params: LevelParamCandidate[]): number {
  let bestIdx = -1;
  let bestRank = Number.POSITIVE_INFINITY;
  params.forEach((c, i) => {
    const rank = CLASS_RANK[c.class];
    if (rank < bestRank) {
      bestRank = rank;
      bestIdx = i;
    }
  });
  return bestIdx;
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

/** Build a footswitch target from a specific candidate (the user's explicit pick, or the
 *  tone-safe default). The backend classifies bake vs assign from these ids.
 *  `sceneContext` — see `FootswitchTarget.sceneContext` (D3); `null` = base. */
export function targetFromCandidate(
  switchIndex: number,
  sceneContext: number | null,
  c: LevelParamCandidate,
): FootswitchTarget {
  return {
    switchIndex,
    levGroupId: c.group_id,
    levNodeId: c.node_id,
    levParameterId: c.parameter_id,
    sceneContext,
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
 *  is useless, so fall back to e.g. "Tube Screamer" from the leveled block's id).
 *
 *  Never sent to the backend (the assign gate only ever edits an EXISTING `param` fn or
 *  refuses — it never writes a switch's on-device `customLabel`), but still not an
 *  arbitrary pick: it names the row after the tone-safe DEFAULT level param (the same
 *  one Set up recommends), and only falls further back to the switch's own
 *  toggled/adjusted block (its first function's `fender_id`) when there is no
 *  classifiable level param at all — so the displayed name stays a meaningful guess. */
/** The fallback row name for an UNLABELED switch, given the candidate that will actually
 *  be leveled — the block that candidate lives on.
 *
 *  Split out of [`footswitchName`] because the name is chosen TWICE at different moments:
 *  once at list-build time (`chosenFrom`, which only knows the tone-safe DEFAULT candidate)
 *  and again at run-start, if the user overrode that default in Set up. The second call
 *  keeps the DISPLAYED row name honest about what is actually being leveled — never sent
 *  to the backend, but naming it after the default while leveling something else would
 *  still mislead the player reading their own Level list. */
export function footswitchNameForCandidate(c: LevelParamCandidate): string {
  return blockStripName(c.fender_id);
}

/** The name the UNIT prints under a footswitch for a block, for rows whose switch has
 *  no `customLabel` of its own — so the Level list reads the same as the hardware the
 *  player is looking at.
 *
 *  BUG→FIX (2026-08-20, "Plumes+BD2+OCD"): this used to be `shortFallback(fender_id)`,
 *  which merely de-camel-cases the internal id — `ACD_BluesDriver` → "Blues Driver".
 *  That is the name of the pedal Fender EMULATES, not any name the device shows: the
 *  unit's strip reads "Sapphire OD" and the control picker one column over already read
 *  "SAPPHIRE DRIVE". Three names for one block, none of them matching the unit.
 *
 *  Order: the device's own strip name (`name8`), else the Model Guide name, else the
 *  old de-camel-cased id so an uncatalogued or user block still gets something. */
function blockStripName(fenderId: string): string {
  return (
    stripNameFor(fenderId) ??
    blockArtTile(fenderId).fullName ??
    shortFallback(fenderId)
  );
}

export function footswitchName(f: FootswitchInfo): string {
  const label = f.label.trim();
  if (label) return label;
  if (f.level_params.length > 0) {
    // Every candidate the backend offers already carries a real (never "other") class,
    // so `idx` is always >= 0 here in practice — the `undefined` guard exists only so a
    // future loosening of that backend guarantee fails to the function fallback below
    // instead of silently naming the row after `level_params[0]`.
    const idx = defaultParamIndex(f.level_params);
    const picked = idx >= 0 ? f.level_params[idx] : undefined;
    if (picked) return footswitchNameForCandidate(picked);
  }
  if (f.functions.length > 0) return blockStripName(f.functions[0].fender_id);
  return "Footswitch";
}

/** Resolve a levelable footswitch's DEFAULT row target: the tone-safe default candidate
 *  (D2 — every row levels against a combined block+param dropdown, best candidate
 *  pre-selected), base scene context (D3's `suggested` scene isn't known synchronously —
 *  the combined picker's lazy fetch fills it in once opened). `null` when the footswitch
 *  is not `footswitchLevelable` (no leveling candidate at all) — callers must NOT rely
 *  on this being pre-filtered upstream: the list shows such a switch too, disabled with
 *  a reason (BUG 1), so `chosenFrom` still has to cope with one reaching it. */
function footswitchTarget(f: FootswitchInfo): FootswitchTarget | null {
  if (!footswitchLevelable(f)) return null;
  const idx = defaultParamIndex(f.level_params);
  if (idx < 0) return null;
  return targetFromCandidate(f.switch, null, f.level_params[idx]);
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
  /** Display name: "Base Preset" / "Whole preset" / the scene name. */
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
  /** Footswitch rows only: the switch carries NO `customLabel` on the device, so
   *  `sceneName` above is a derived fallback naming the DEFAULT candidate's block — not
   *  the player's own name for the switch.
   *
   *  Re-checked at run-start, not just at list-build: `sceneName` is the Level list's row
   *  name (never sent to the backend). If the user picks a different candidate in Set up,
   *  the row must be RE-NAMED after the block actually being leveled (`SetupBody.start`),
   *  or the DISPLAYED row keeps naming a block the run never touched. A LABELED switch is
   *  never renamed: that string is the player's, not ours. */
  fsUnlabeled?: boolean;
  /** Scene rows only: the user's chosen leveling control, INSTEAD of the active amp's
   *  `outputLevel` — undefined/null = the amp default (every existing caller). */
  sceneHandle?: SceneHandlePick | null;
  /** Base rows only: the user's chosen leveling control, INSTEAD of the master
   *  `presetLevel` — undefined/null = the "Preset level" pseudo-handle default (D2). */
  baseHandle?: BaseHandlePick | null;
  /** Footswitch rows only: this switch's preset's own scene NAMES, index-aligned with
   *  the wire `scenes[]` slots — the scene-context picker's label source (D3). Undefined
   *  for Base/scene rows. */
  fsSceneNames?: string[];
}

/** A user-chosen BASE leveling control — the block param `level_preset` should drive
 *  INSTEAD of the master `presetLevel` (mirrors `LevelJob.block_*`). Carries no reading:
 *  `buildLevelJob` always sends `block_value: null` for EITHER source (backup-derived or
 *  device-fallback) — the run's own fresh saved-doc read anchors the wet floor instead
 *  (`level_preset.rs`'s block-value fallback, the same read that already serves
 *  classification, at zero extra device I/O), so there is nothing to carry here or
 *  re-resolve on a carried-forward re-level pick. */
export interface BaseHandlePick {
  groupId: string;
  nodeId: string;
  parameterId: string;
}

/** The e2e-hook identity for a setup row (`PresetOptionRow`'s `data-setup-row`) —
 *  DELIBERATELY DISTINCT from `SetupOption.key` (the SELECTION key: `sel`/`rows` Map
 *  lookups and the React list key, unchanged by this function). A footswitch's `key`
 *  is `fswKey`'s POSITION within the levelable-filtered footswitch list, so a fixture
 *  edit that adds/removes an earlier switch's level candidate silently shifts every
 *  LATER switch's position — and hence a spec's `f<slot>:<i>` selector, with no
 *  signal that it now points at a different row. The hook instead names the row by
 *  the DEVICE SWITCH NUMBER (`FootswitchTarget.switchIndex`, sourced from
 *  `FootswitchInfo.switch` — see `footswitchTarget`), which is stable under any
 *  filtered-list reshuffle. A scene row's hook stays `s<slot>:<sceneSlot>`:
 *  `sceneSlot` is already the wire `scenes[]` index (`chosenFrom`'s "the row index IS
 *  the 0-based wire sceneSlot"), i.e. already an IDENTITY, not a filtered-list
 *  position, so it needs no translation. Base rows keep `p<slot>` (nothing to
 *  disambiguate). */
export function setupRowHookKey(o: SetupOption): string {
  if (o.footswitch != null) {
    return `f${String(o.slot)}:sw${String(o.footswitch.switchIndex)}`;
  }
  if (o.sceneSlot != null) return sceneKeyOf(o.slot, o.sceneSlot);
  return baseKey(o.slot);
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
      // ONLY footswitches still shows "Base Preset" vs "Whole preset" as a true scene-less case.
      const hasChildren = scenes.length > 0 || footswitches.length > 0;
      if (sel.has(baseKey(r.slot))) {
        items.push({
          key: baseKey(r.slot),
          slot: r.slot,
          presetName: r.name,
          isBase: true,
          sceneSlot: null,
          sceneName: hasChildren ? "Base Preset" : "Whole preset",
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
            fsUnlabeled: f.label.trim() === "",
            fsSceneNames: scenes.map((sc) => sc.name),
          });
        }
      });
    });
  return items;
}

// ── run / summary: one item per chosen scene ────────────────────────────────

// `offbranch` is its OWN outcome (not a flavor of `clamped`): the amp doesn't reach the
// USB 1/2 capture, so re-leveling can't fix it — only a routing change on the unit can.
//
// `unconverged` is likewise its own outcome, and the distinction from `clamped` is the
// user's next action: a CLAMPED sound is at the end of its knob and cannot reach target
// however often it runs, while an UNCONVERGED one still had knob room and simply ran out
// of measurement captures — running it again improves it. Backed by
// `FootswitchLevelResult.unconverged` (footswitch rows only today). Folding it into
// `clamped` would also feed a non-ceiling into `ceilingOf` → the derived common target.
export type Outcome =
  "done" | "clamped" | "unconverged" | "offbranch" | "skipped";

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
  // live + final:
  status: "queued" | "active" | "result";
  /** Backend-supplied caption for an active row, rendered two ways by `RunBody`'s
   *  `rowStatus` depending on whether a capture is streaming:
   *   - streaming -> it is the VERB before the live number. The ceiling prepass sends
   *     "measuring", giving `measuring · −18.9`; a message-less solve row reads `leveling · …`.
   *   - not streaming -> it is a NOTE, shown verbatim (the freshness barrier's "waiting for
   *     the device to commit the previous save…" — a same-slot load can land inside the TMP's
   *     lazy `saveCurrentPreset` commit window); absent one the row reads "connecting…".
   *  A message sent while a capture streams is therefore a verb by construction — that is the
   *  contract the backend's `leveller::PREPASS_ACTIVE_MSG` documents on its side. Scene/
   *  footswitch channel items only; cleared when the row resolves or a cancelled sweep
   *  reverts it, so a later re-run's default is never shadowed by a stale caption. */
  activeMessage?: string | null;
  outcome?: Outcome;
  /** Measured (predicted) loudness, or null. */
  value?: number | null;
  /** Scene rows only: this row's handle pick, carried from Set up into the dispatch
   *  (mirrors `SetupOption.sceneHandle`). */
  handle?: SceneHandlePick | null;
  /** Base rows only: this row's handle pick, carried from Set up into the dispatch
   *  (mirrors `SetupOption.baseHandle`). */
  baseHandle?: BaseHandlePick | null;
  /** The clamp's CAUSE from the shared taxonomy, when clamped — render
   *  `CLAMP_MESSAGES[clampKind]` verbatim. Null/undefined on a non-clamped row. */
  clampKind?: ClampKind | null;
  /** THE HEADROOM TRADE this row's batch made (or, on a preview, WOULD make) — see
   *  `TradeSummary`. Stamped on every row of a batch that traded. Null/undefined
   *  otherwise. */
  trade?: TradeSummary | null;
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
   *  above the gated average; `wet_floor` = a footswitch's wet-mix clamp is pinned at the
   *  25% floor, not headroom (`FootswitchLevelResult.wet_floor`); `rebalance` = shallow
   *  lane-mute isolation made the parallel balance approximate. Resolved to a single
   *  cause when the RunItem is built. */
  verifyByEar?: "envelope" | "dynamic" | "wet_floor" | "rebalance";
  /** The preset's backup-scan silence hint, stamped at item build — refines the
   *  offbranch row status (see `offbranchStatus`). */
  silenceHint?: SilenceHint;
  /** The sound's MEASURED raw ceiling (max-reachable LUFS), set on Base rows from the
   *  result's `constant_c`. Feeds the reachable-common-target derivation (a clamped row's
   *  ceiling is its measured `value` instead — it sits at max). Undefined until measured. */
  ceilingLufs?: number | null;
  /** Set by the reachable-common-target fallback: an explicit numeric target that OVERRIDES
   *  `targetLufsByName(targetName)` in the run loop's dispatch (pre-offset; the runner adds
   *  the playback offset). Normal runs never set it. */
  targetOverrideLufs?: number;
  /** Set by the reachable-common-target fallback for a row it does NOT re-level (off-branch,
   *  no ceiling): the run loop leaves the row's existing outcome untouched so it stays
   *  visible/counted in the Summary without wasting a re-capture on a signal-less sound. */
  skipRelevel?: boolean;
}

/** A finished row's MEASURED raw ceiling for the reachable-common-target derivation, or null
 *  when unknown. A CLAMPED row sits at max, so its measured `value` IS its ceiling; a done
 *  row's ceiling is `ceilingLufs` (Base rows carry `constant_c`; done scene/footswitch rows
 *  have none → excluded, their true ceiling is ≥ their reached target so they don't bind).
 *
 *  EXCEPT a clamped FOOTSWITCH row: preset/scene clamps are top-rail only (`LEVEL_MIN` is
 *  0.0 and `ideal = 10^x > 0`, so `ideal < LEVEL_MIN` is unreachable — a preset/scene can
 *  only clamp because it's TOO QUIET to reach target, never too loud), so their clamped
 *  `value` genuinely IS a ceiling. `measure_footswitch`'s clamp is direction-agnostic (a
 *  switch CAN clamp because it's too LOUD), so treating it the same way would feed a FLOOR
 *  into `min(ceiling)` and drag the whole library's derived common target down. Accepted
 *  loss: a genuinely quiet clamped switch stops binding the common target — it still shows
 *  its own clamped outcome, just doesn't drag every OTHER sound's target down with it. */
export const ceilingOf = (it: RunItem): number | null => {
  const c =
    it.outcome === "clamped" && !it.footswitch ? it.value : it.ceilingLufs;
  return c != null && Number.isFinite(c) ? c : null;
};

/** The offbranch ("silent capture") row status, refined by the preset's JSON-visible
 *  cause when the backup scan found one. Rendered verbatim in RunBody + SummaryBody. */
export function offbranchStatus(hint: SilenceHint | undefined): string {
  if (hint === "amp_zero") return "amp output at zero";
  if (hint === "exp_mute") return "exp pedal may mute";
  return "not on USB 1/2";
}

/** The sound's preset line — the mono sub-line under its name. Rendered verbatim in
 *  RunBody + SummaryBody, so it lives here rather than being retyped on both. */
export const presetLine = (it: RunItem): string =>
  `${slotLabel(it.slot)} · ${it.presetName}`;

/** The LUFS a row is ACTUALLY aiming at. The reachable-common-target fallback stamps an
 *  explicit override that wins over the named target — the run loop's dispatch and the
 *  run table's Target cell must resolve it the same way, so both call this. */
export const resolvedTargetLufs = (
  it: RunItem,
  targetLufsByName: (name: string | null) => number,
): number => it.targetOverrideLufs ?? targetLufsByName(it.targetName);

/** Turn a checked setup row into a run item with its resolved instrument + target. */
export function optionToRunItem(
  o: SetupOption,
  instId: string,
  targetName: string,
): RunItem {
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
    status: "queued",
    handle: o.sceneHandle,
    baseHandle: o.baseHandle,
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
    sceneHandle: it.handle,
    baseHandle: it.baseHandle,
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
  /** The row's user-chosen leveling control (D2), instead of the master `presetLevel`.
   *  `null`/undefined = the "Preset level" pseudo-handle default. */
  handle?: BaseHandlePick | null,
): LevelJob {
  return {
    slot,
    target_lufs: targetLufs,
    save,
    topology_id: profile?.topology_id ?? null,
    calibration_lufs: profile?.calibration_lufs ?? null,
    profile_id: profile?.id ?? null,
    block_group_id: handle?.groupId ?? null,
    block_node_id: handle?.nodeId ?? null,
    block_parameter_id: handle?.parameterId ?? null,
    // ALWAYS null — see `BaseHandlePick`'s doc: the run's own fresh saved-doc read
    // anchors the wet floor, for either candidate source.
    block_value: null,
  };
}
