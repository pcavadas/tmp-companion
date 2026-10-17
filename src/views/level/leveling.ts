// src/views/level/leveling.ts — types + helpers for the unified leveling flow.
//
// The unit of leveling is a SOUND: a preset's main sound carries cross-preset loudness
// (preset `presetLevel`); a scene is leveled with its amp's `outputLevel` in scene mode;
// a footswitch with its block's level knob.
//
// SELECTION lives in the list (the scene tree): the source of truth is a flat set of
// scene KEYS — main sound = `p${slot}`, scene = `s${slot}:${idx}`, footswitch =
// `f${slot}:${i}`. `chosenFrom` turns that set into the SetupOption[] the Set up page
// configures (instrument per preset; target, block and knob per sound).
//
// Flow (setup → run → summary): Set up (its footer's backup acknowledgment gates the
// commit) → run (steps the chosen sounds) → summary.

import type {
  BaseBoostSummary,
  ClampKind,
  FootswitchInfo,
  FootswitchLevelJobWire,
  LevelJob,
  LevelParamCandidate,
  ParamClass,
  Profile,
  SceneHandlePick,
  SceneInfo,
  SilenceHint,
  SkipKind,
} from "../../lib/types";
import type { SelectOption } from "../../ui/selectValue";
import { blockArtTile, shortFallback } from "../../models/blockArt";
import { stripNameFor } from "../../models/catalog";

// `SceneHandlePick` is declared ONCE, as the `levelScenesApplyBatched` wire type in
// lib/types.ts (its `SceneLevelJobWire.handle` field) — re-exported so
// `SetupOption`/`RunItem` and the scene-handle-picker wiring can import it from
// either module without a second, driftable copy.
export type { SceneHandlePick };

/** One My-Presets row as the preset lists and the leveling flow carry it: its 0-based
 *  list slot, its name, and whether the slot is empty. */
export interface PresetRow {
  slot: number;
  name: string;
  empty: boolean;
}

// ── selection scene-key helpers (shared by the list + the flow) ─────────────

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
 *  Read through `paramLabel` by Set up's Knob dropdown (`setupControls`), for both the
 *  value it shows and its options. */
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

/** Build one `levelFootswitchesApply` job from a `FootswitchTarget` — the ONE place
 * that turns the frontend's row shape into the wire's field names. The backend never
 * creates a footswitch function any more (the assign gate only ever edits an EXISTING
 * `param` fn or refuses), so no display name rides the wire shape. */
export function toFootswitchJobWire(
  target: FootswitchTarget,
  targetLufs: number,
): FootswitchLevelJobWire {
  return {
    switch: target.switchIndex,
    levGroupId: target.levGroupId,
    levNodeId: target.levNodeId,
    levParameterId: target.levParameterId,
    targetLufs,
    sceneContext: target.sceneContext,
  };
}

/** The display footswitch number for a switch index (human FS tag = index + 1 — the
 *  same +1 scene rows use, verified against `footswitch::scene_fs_map`). */
const fsTagOf = (switchIndex: number): string => `FS${String(switchIndex + 1)}`;

/** The "no instrument" choice: level or diagnose against the default reference. The one
 *  sentinel for it, in the pickers, the Set-up defaults and every run item. */
export const NO_INSTRUMENT = "none";

/** The one name of a preset's main sound (its base, `presetLevel`), with or without
 *  scenes — the one word for it on the list, Set up, the run and Doctor alike. */
export const MAIN_SOUND = "Main sound";

/** The instrument choices shared by Level's Set up and Doctor's Instrument page: "None"
 *  (the no-instrument path — level or diagnose against the default reference) followed by
 *  each saved instrument. No loudness figures: those stay in Settings. */
export function instrumentOptions(
  profiles: Profile[] | undefined,
): SelectOption[] {
  return [
    { value: NO_INSTRUMENT, label: "None" },
    ...(profiles ?? []).map((p) => ({ value: p.id, label: p.name })),
  ];
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
export function blockStripName(fenderId: string): string {
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
  /** The `loadScene` / `level_scenes_apply_batched` wire slot (0-based scenes[] index);
   *  null for the Base/whole-preset row (which levels `presetLevel`). */
  sceneSlot: number | null;
  /** Display name: "Main sound" / the scene name / the footswitch's name. */
  sceneName: string;
  /** The sound's tag, sent with a Doctor check (the backend's scene-loudness rows carry
   *  it): "BASE" | `FS${n}` | null (em-dash for an untagged named scene). No screen shows
   *  it (a footswitch says "footswitch 3"). */
  tag: string | null;
  /** False ⇒ a preset with no scenes and no footswitches. */
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
   *  the row must be RE-NAMED after the block actually being leveled (`SetupPage`'s `start`),
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

/** The e2e-hook identity for a setup row (`SetupPage`'s `data-setup-row`, and the
 *  Summary's `data-summary-row` and footswitch scene dropdown) —
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
export function setupRowHookKey(
  o: Pick<SetupOption, "slot" | "sceneSlot" | "footswitch">,
): string {
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

/** One Set up row's own picks, as `buildSetupChoices` reads them. `sceneContext`
 *  undefined = not chosen (a footswitch then takes its suggested scene). */
export interface SetupRowPick {
  inst: string;
  target: string;
  sceneContext: number | null | undefined;
}

/** The Set up page's commit, pure: one `SetupChoice` per option, with each row's
 *  effective leveling handle folded into its option. `rowOf` gives a row's resolved
 *  instrument, target and scene pick (the caller resolves every one, so there is no
 *  default to fall back on); `handleOf` resolves a row's handle (its pick, else the
 *  page's preselect); `suggestedSceneOf` a footswitch's suggested scene context (null
 *  while unread). A footswitch whose handle matches one of its level params is
 *  re-targeted at that param (and an unlabeled switch named after it); a Base row carries
 *  its pick as `baseHandle`; a Scene row as `sceneHandle`. */
export function buildSetupChoices(
  options: SetupOption[],
  rowOf: (o: SetupOption) => SetupRowPick,
  handleOf: (o: SetupOption) => SceneHandlePick | null,
  suggestedSceneOf: (o: SetupOption) => number | null,
): SetupChoice[] {
  return options.map((o) => {
    const row = rowOf(o);
    let option = o;
    if (o.footswitch != null) {
      const chosenHandle = handleOf(o);
      const candidate = chosenHandle
        ? (o.levelParams ?? []).find(
            (c) =>
              c.group_id === chosenHandle.groupId &&
              c.node_id === chosenHandle.nodeId &&
              c.parameter_id === chosenHandle.parameterId,
          )
        : undefined;
      const suggested = suggestedSceneOf(o);
      const sceneContext =
        row.sceneContext !== undefined
          ? row.sceneContext
          : (suggested ?? o.footswitch.sceneContext);
      const target: FootswitchTarget = candidate
        ? targetFromCandidate(o.footswitch.switchIndex, sceneContext, candidate)
        : { ...o.footswitch, sceneContext };
      option = {
        ...o,
        footswitch: target,
        ...(o.fsUnlabeled === true && candidate
          ? { sceneName: footswitchNameForCandidate(candidate) }
          : {}),
      };
    } else if (o.isBase) {
      const chosen = handleOf(o);
      const baseHandle: BaseHandlePick | null = chosen
        ? {
            groupId: chosen.groupId,
            nodeId: chosen.nodeId,
            parameterId: chosen.parameterId,
          }
        : null;
      option = { ...o, baseHandle };
    } else if (o.sceneSlot != null) {
      option = { ...o, sceneHandle: handleOf(o) };
    }
    return { option, instId: row.inst, targetName: row.target };
  });
}

/** A row's position in its preset's DEPENDENCY order — each rank writes a control the
 *  ranks below it render through, so running them in any other order shifts rows that
 *  are already on target:
 *
 *  - `0` Base: `presetLevel`, a global multiplier over every other row.
 *  - `1` Footswitch in BASE context (`sceneContext == null`): its handle is often a block
 *    the preset leaves ON in base, and a scene renders its whole base chain, so this
 *    base-space write moves every scene. It must land before the scenes it moves.
 *  - `2` Scene: a per-scene amp `outputLevel` overlay, scene-local — it moves nothing that
 *    any other row renders through.
 *  - `3` Footswitch in SCENE context (`sceneContext != null`): measured with its context
 *    scene recalled, so it cannot be solved until that scene is on target. `ftswStates`
 *    naming that scene means the switch is ON in the scene's recorded state, so the scene
 *    pass has already put this exact sound on target and the row measures in-tolerance and
 *    skips its write.
 *
 *  Rank order: 0 base, 1 base-context footswitch, 2 scene, 3 scene-context footswitch — the
 *  split is per-row, not "all footswitches early" or "all footswitches late". HW evidence
 *  (the online 410 arc) is in `notes/gotchas.md`'s "A scene renders the base chain's ON
 *  blocks" entry.
 *
 *  The rank only orders rows WITHIN one preset; `sortRunItems` below is the whole run
 *  order (presets in first-appearance order, then this rank). `chosenFrom` emits that
 *  order, and `levelRunStore` re-sorts its items through `sortRunItems` so the run
 *  holds however they were assembled. */
export function runRank(it: {
  isBase: boolean;
  footswitch?: unknown;
}): 0 | 1 | 2 | 3 {
  if (it.isBase) return 0;
  const fs = it.footswitch;
  if (fs == null) return 2;
  // Only a NUMERIC context ranks late: absent, `null` and an unresolved `undefined` all
  // mean base context, the conservative side (a truthiness test would mis-read scene 0).
  // `unknown` + probe rather than a typed field: the two callers pass genuinely different
  // shapes (a full `FootswitchTarget`, and a bare object with no `sceneContext` at all —
  // which the gate below exists to pin), and no single object type accepts both.
  const ctx =
    typeof fs === "object" && "sceneContext" in fs ? fs.sceneContext : null;
  return typeof ctx === "number" ? 3 : 1;
}

/** The run order: presets in the order their FIRST row appears in `items`, and within each
 *  preset `runRank` order (ties keep input order). A comparator on `runRank` alone that
 *  returns 0 across slots is NOT a consistent order — `[scene@1, base@2, base@1]` can come
 *  back unchanged, running slot 1's scene before its base — so the slot's first index is
 *  the primary key. Returns a new array; `items` is left untouched. */
export function sortRunItems<
  T extends { slot: number; isBase: boolean; footswitch?: unknown },
>(items: readonly T[]): T[] {
  const firstIndex = new Map<number, number>();
  items.forEach((it, i) => {
    if (!firstIndex.has(it.slot)) firstIndex.set(it.slot, i);
  });
  return items
    .map((item, index) => ({
      item,
      index,
      group: firstIndex.get(item.slot) ?? index,
      rank: runRank(item),
    }))
    .sort((a, b) => a.group - b.group || a.rank - b.rank || a.index - b.index)
    .map((d) => d.item);
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
      // ONLY footswitches has children too. The main sound has ONE name everywhere,
      // whether or not the preset has scenes ("Main sound").
      const hasChildren = scenes.length > 0 || footswitches.length > 0;
      if (sel.has(baseKey(r.slot))) {
        items.push({
          key: baseKey(r.slot),
          slot: r.slot,
          presetName: r.name,
          isBase: true,
          sceneSlot: null,
          sceneName: MAIN_SOUND,
          tag: hasChildren ? "BASE" : null,
          hasScenes: hasChildren,
        });
      }
      // Emitted in `runRank` order — Base, BASE-CONTEXT footswitches, scenes, then
      // SCENE-CONTEXT footswitches. Rather than restate that order as separate passes, the
      // preset's rows are built once and sorted through `runRank` itself, which owns the rule
      // and carries the HW evidence for both halves of the split. The sort is stable, so each
      // switch keeps its ORIGINAL index in `fswKey` and scenes keep their wire order.
      const children: SetupOption[] = [];
      footswitches.forEach((f, i) => {
        const target = footswitchTarget(f);
        if (target && sel.has(fswKey(r.slot, i))) {
          children.push({
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
      scenes.forEach((sc, i) => {
        if (sel.has(sceneKeyOf(r.slot, i))) {
          children.push({
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
      children.sort((a, b) => runRank(a) - runRank(b));
      items.push(...children);
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
// `FootswitchLevelResult.unconverged` (footswitch rows only today).
export type Outcome =
  "done" | "clamped" | "unconverged" | "offbranch" | "skipped";

/** Resolve a target NAME to its loudness (LUFS), or `null` when no configured target has
 *  that name: an empty name, a target deleted in Settings, or a store not loaded yet.
 *  There is no fallback loudness — the run refuses such a sound (`no_target`). */
export type TargetLufsOf = (name: string | null) => number | null;

/** WHY a run row was skipped: a backend `SkipKind`, or `no_target`, the run's own refusal
 *  of a sound whose target name matches no configured target (it never reaches the unit). */
export type RunSkipKind = SkipKind | "no_target";

export interface RunItem {
  key: string;
  /** 0-based list index of the preset. */
  slot: number;
  presetName: string;
  isBase: boolean;
  /** 0-based scenes[] wire slot, or null for the Base/whole-preset step. */
  sceneSlot: number | null;
  sceneName: string;
  /** Set ⇒ a block-acting FOOTSWITCH step (dispatched to `levelFootswitchesApply`);
   *  null/undefined ⇒ Base (`level_preset`) or FS scene (`level_scenes_apply_batched`). */
  footswitch?: FootswitchTarget | null;
  /** Chosen instrument profile id, or `NO_INSTRUMENT`. */
  instId: string;
  /** Chosen target name. */
  targetName: string;
  /** The loudness the run leveled this row to: its target's, resolved by name when the
   *  row's batch began. Absent on a row no run has reached, and on one the run refused
   *  because no configured target has its name (`no_target`). */
  targetLufs?: number;
  // live + final:
  status: "queued" | "active" | "result";
  outcome?: Outcome;
  /** The write landed but its check capture failed, so the row's number is a prediction —
   *  shown as `unconverged` ("run it again") with its own copy. */
  unverified?: boolean;
  /** WHY a row was skipped — the backend's own per-sound text (the planner's
   *  "no active amp" / a capture error / a refused scene write), for the log and the bug
   *  report. Null when the skip happened client-side with no message. The screen picks
   *  its words by `skipKind`, never by this text. */
  skipReason?: string | null;
  /** WHY a row was skipped, typed (`RunSkipKind`): from the row's `error` progress item,
   *  the failed command's own `CommandError.kind`, or the run's own `no_target` refusal.
   *  `"other"` when nothing says more. Set exactly when `outcome === "skipped"`. */
  skipKind?: RunSkipKind;
  /** The clamp's CAUSE (`ClampKind`, from the result's `clamp_kind`) — tells "at full
   *  volume" from the effect's mix floor and the trade's floors without reading text.
   *  Null on a row that did not clamp. */
  clampKind?: ClampKind | null;
  /** `true`: the run's save did not hold this row's leveled value (`persist_mismatch:
   *  true` — read back after the save, or the save itself failed), on a stopped run's
   *  save too. `false` or absent: no such report (held, or not checked). */
  persistMismatch?: boolean;
  /** Measured (predicted) loudness, or null. */
  value?: number | null;
  /** Scene rows only: this row's handle pick, carried from Set up into the dispatch
   *  (mirrors `SetupOption.sceneHandle`). */
  handle?: SceneHandlePick | null;
  /** Base rows only: this row's handle pick, carried from Set up into the dispatch
   *  (mirrors `SetupOption.baseHandle`). */
  baseHandle?: BaseHandlePick | null;
  /** PREDICTED true peak (dBTP) at the leveled setting — an estimate, never a
   *  re-measurement. Only Base rows carry a value (undefined/null elsewhere); drives
   *  the Summary "may clip" chip when > −1 dBTP. */
  truePeakDbtp?: number | null;
  /** Cause of the "verify by ear" marker (undefined = no flag): `envelope` = the preset
   *  contains an envelope-follower effect, which tracks the synthetic stimulus differently
   *  than real playing (the measurement itself is suspect); `wet_floor` = a footswitch's wet-mix clamp is pinned at the
   *  25% floor, not headroom (`FootswitchLevelResult.wet_floor`); `rebalance` = shallow
   *  lane-mute isolation made the parallel balance approximate. Resolved to a single
   *  cause when the RunItem is built. */
  verifyByEar?: "envelope" | "wet_floor" | "rebalance";
  /** The preset's backup-scan silence hint, stamped at item build — refines the
   *  offbranch row's message on the Summary (`summaryModel.ts`). */
  silenceHint?: SilenceHint;
  /** Base rows only: this row's base-pair BOOST disclosure, carried verbatim from the
   *  result's `base_boost` — see `BaseBoostSummary`'s own doc. `null`/undefined on every
   *  row whose base pair never entered the `Boost` regime (the common case) and on every
   *  non-base row. */
  baseBoost?: BaseBoostSummary | null;
  /** The run-end re-amp warning TEXT its command returned (`LevelResult.reamp_warning`),
   *  the backend's own words for the log. Whether re-amp may still be on is
   *  `reampStillOn`, never this text. */
  reampWarning?: string | null;
  /** Re-amp may still be on after this row's command (`reamp_still_on`, or a failed
   *  command's `CommandError.reampStillOn`): the unit's guitar input may still be muted
   *  until it is restarted. */
  reampStillOn?: boolean;
}

/** One preset of the run list: its slot, its name and its sounds. */
export interface PresetGroup<T> {
  slot: number;
  name: string;
  items: T[];
}

/** Group run items by preset slot, ascending — the run page's list (a header per preset,
 *  its sounds under it). */
export function groupItemsBySlot(items: RunItem[]): PresetGroup<RunItem>[] {
  const by = new Map<number, PresetGroup<RunItem>>();
  items.forEach((it) => {
    let g = by.get(it.slot);
    if (!g) {
      g = { slot: it.slot, name: it.presetName, items: [] };
      by.set(it.slot, g);
    }
    g.items.push(it);
  });
  return [...by.values()].sort((a, b) => a.slot - b.slot);
}

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
    footswitch: o.footswitch ?? null,
    instId,
    targetName,
    status: "queued",
    handle: o.sceneHandle,
    baseHandle: o.baseHandle,
  };
}

// The wizard's stage machine + run state now live in the module store
// (levelRunStore → Stage / RunState); this module just owns the per-scene types.

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
