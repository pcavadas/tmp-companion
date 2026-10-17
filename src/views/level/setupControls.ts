// src/views/level/setupControls.ts — what a Set up row's Block and Knob dropdowns show,
// and what picking in them does. Pure: no React, so every case is unit-testable.
//
// Both dropdowns always show the control the run will turn (danger.md: a dropdown must
// display the value that will be used):
//   - a main sound's default is the preset's own level: Block "Preset", Knob "Level"
//     (handle `null`, `presetLevel`);
//   - a scene's default is its amp's output level (handle `null`): Block names the amp
//     the run drives (`SceneDrive` from the backup scan, the run's own planner), "Both
//     amps" where two lanes are driven together; Knob "Output level";
//   - a footswitch has no default — its row always carries a real knob;
//   - a scene the run can't drive by default (no active amp) shows an EMPTY Block in
//     the attention look and waits for a choice.
// Picking a block lands on its best usable knob (`bestEnabled`). A chosen knob the list
// no longer offers shows its stored name and the row says so (`stale`).

import type { SceneDrive } from "../../lib/types";
import type { SelectOption } from "../../ui/selectValue";
import { blockStripName, paramLabel } from "./leveling";
import {
  bestEnabled,
  blockKeyOf,
  groupByBlock,
  groupHead,
  type BlockLevelCandidate,
  type BlockLevelHandle,
} from "./blockLevelGroups";
import type { CandidateFetch } from "./setupCandidates";

/** The value of the default ("Preset" / the amp) Block option and its one knob. */
export const DEFAULT_CONTROL = "default";

/** What a row levels with when nothing is chosen.
 *  - `preset`: the main sound's own level (Block "Preset", Knob "Level").
 *  - `amp`: a scene's amp output level. `ampKey` names the ONE amp block it is, so that
 *    block's other knobs sit under the same Block option; `null` for two or more amps
 *    driven together (or an amp the scan couldn't name). `ampKeys` lists every amp it
 *    turns (empty when unknown), so the knob can say it can only turn down.
 *  - `none`: no default — a footswitch (its row always carries a knob), or a scene the run
 *    can't drive on its own (`reason`, the scene's typed skip kind). */
export type DefaultControl =
  | { kind: "preset" }
  | { kind: "amp"; label: string; ampKey: string | null; ampKeys: string[] }
  | { kind: "none"; reason?: SceneDrive["skip"] };

/** A scene's default control, from what the run drives there (`SceneDrive`). No drive at
 *  all (a preset the scan didn't read) keeps the amp default under a plain "Amp" name: the
 *  run still levels the amp the backend finds. */
export function sceneDefault(drive: SceneDrive | undefined): DefaultControl {
  if (!drive) return { kind: "amp", label: "Amp", ampKey: null, ampKeys: [] };
  if (drive.skip != null || drive.amps.length === 0)
    return { kind: "none", reason: drive.skip ?? "no_active_amp" };
  if (drive.amps.length === 1) {
    const amp = drive.amps[0];
    const name = amp.fenderId ? blockStripName(amp.fenderId) : "";
    return {
      kind: "amp",
      label: name || "Amp",
      ampKey: blockKeyOf(amp),
      ampKeys: [blockKeyOf(amp)],
    };
  }
  return {
    kind: "amp",
    label:
      drive.amps.length === 2
        ? "Both amps"
        : `All ${String(drive.amps.length)} amps`,
    ampKey: null,
    ampKeys: drive.amps.map(blockKeyOf),
  };
}

/** One dropdown's state. */
export interface SelectModel {
  value: string;
  options: SelectOption[];
  /** The stored value's own name when the options no longer contain it. */
  staleLabel?: string;
  /** The options are still being read. */
  loading: boolean;
  /** A line under the options (a failed read). */
  listNote?: string;
  disabled: boolean;
}

/** A row's Block and Knob dropdowns. */
export interface ControlModel {
  block: SelectModel;
  knob: SelectModel;
  /** A chosen knob the (read) list no longer offers. */
  stale: boolean;
  /** No default and nothing chosen: a block must be chosen before Start. */
  needsBlock: boolean;
  /** `needsBlock`, but the scene has no usable knob at all, so there is nothing to
   *  choose: the run will report it, and Start doesn't wait for it. */
  nothingToChoose: boolean;
  /** The handle a Block pick leads to (`null` = the default). `undefined`: no change. */
  pickBlock: (value: string) => BlockLevelHandle | null | undefined;
  /** The handle a Knob pick leads to (`null` = the default). `undefined`: no change. */
  pickKnob: (value: string) => BlockLevelHandle | null | undefined;
}

const KNOB_READ_FAILED = "Couldn't read this preset's knobs.";

const sameHandle = (a: BlockLevelHandle, b: BlockLevelHandle): boolean =>
  a.groupId === b.groupId &&
  a.nodeId === b.nodeId &&
  a.parameterId === b.parameterId;

const handleOf = (c: BlockLevelCandidate): BlockLevelHandle => ({
  groupId: c.groupId,
  nodeId: c.nodeId,
  parameterId: c.parameterId,
});

/** A knob's note in the open list: why it can't be used, that it can only turn the sound
 *  down (said even on the block's best knob — a sound that needs to get louder can't get
 *  there with it), or how it levels. */
export function knobNote(
  c: BlockLevelCandidate,
  best: BlockLevelCandidate | undefined,
): string | undefined {
  if (c.disabled) return c.disabledTitle;
  if (c.lowersOnly) return "Can only turn down";
  const tone = c.paramClass === "wet_mix";
  if (c === best)
    return tone ? "Best here, also changes the tone" : "Volume only, best";
  if (tone) return "Also changes the tone";
  return undefined;
}

function knobOptions(group: BlockLevelCandidate[]): SelectOption[] {
  const best = bestEnabled(group);
  return group.map((c) => ({
    value: c.parameterId,
    label: paramLabel(c.parameterId),
    note: knobNote(c, best),
    disabled: c.disabled === true ? true : undefined,
  }));
}

/** The amp's output level knob — the scene default's own knob. */
const AMP_OUTPUT = "outputLevel";

export function controlModel(
  fetch: CandidateFetch,
  /** The row's control: a chosen knob, or `null` for the default. */
  handle: BlockLevelHandle | null,
  def: DefaultControl,
): ControlModel {
  const resolved = fetch.status === "resolved";
  const loading = fetch.status === "unfetched" || fetch.status === "loading";
  const listNote = fetch.status === "error" ? KNOB_READ_FAILED : undefined;
  const groups = groupByBlock(resolved ? fetch.list : []);
  const keyOfGroup = (g: BlockLevelCandidate[]) => {
    const head = groupHead(g);
    return head ? blockKeyOf(head) : "";
  };
  const groupFor = (key: string) => groups.find((g) => keyOfGroup(g) === key);
  // The one amp a scene defaults to: its block shows as the default option, never twice.
  const ampKey = def.kind === "amp" ? def.ampKey : null;
  const ampGroup = ampKey != null ? groupFor(ampKey) : undefined;

  const blockOptions: SelectOption[] = [];
  if (def.kind === "preset")
    blockOptions.push({ value: DEFAULT_CONTROL, label: "Preset" });
  if (def.kind === "amp")
    blockOptions.push({ value: DEFAULT_CONTROL, label: def.label });
  for (const g of groups) {
    const head = groupHead(g);
    if (!head || blockKeyOf(head) === ampKey) continue;
    const usable = bestEnabled(g) != null;
    blockOptions.push({
      value: blockKeyOf(head),
      label: blockStripName(head.fenderId),
      ...(usable ? {} : { disabled: true, note: head.disabledTitle }),
    });
  }

  // Which Block option the row is on.
  const onDefaultBlock =
    handle == null
      ? def.kind !== "none"
      : ampKey != null && blockKeyOf(handle) === ampKey;
  const blockValue =
    handle == null
      ? def.kind === "none"
        ? ""
        : DEFAULT_CONTROL
      : onDefaultBlock
        ? DEFAULT_CONTROL
        : blockKeyOf(handle);

  // The knobs of that block.
  let knobOpts: SelectOption[] = [];
  let knobGroup: BlockLevelCandidate[] = [];
  if (blockValue === DEFAULT_CONTROL) {
    if (def.kind === "preset")
      knobOpts = [
        { value: DEFAULT_CONTROL, label: "Level", note: "Volume only, best" },
      ];
    else if (def.kind === "amp") {
      knobGroup = (ampGroup ?? []).filter((c) => c.parameterId !== AMP_OUTPUT);
      // An amp already at the top of its output can only turn the scene down.
      const topped =
        resolved &&
        fetch.list.some(
          (c) =>
            c.parameterId === AMP_OUTPUT &&
            c.lowersOnly === true &&
            def.ampKeys.includes(blockKeyOf(c)),
        );
      knobOpts = [
        {
          value: DEFAULT_CONTROL,
          label: paramLabel(AMP_OUTPUT),
          note: topped ? "Can only turn down" : "Volume only, best",
        },
        ...knobGroup.map((c) => ({
          value: c.parameterId,
          label: paramLabel(c.parameterId),
          note: knobNote(c, undefined),
          disabled: c.disabled === true ? true : undefined,
        })),
      ];
    }
  } else if (blockValue !== "") {
    knobGroup = groupFor(blockValue) ?? [];
    knobOpts = knobOptions(knobGroup);
  }
  const knobValue =
    handle == null
      ? blockValue === ""
        ? ""
        : DEFAULT_CONTROL
      : onDefaultBlock && handle.parameterId === AMP_OUTPUT
        ? DEFAULT_CONTROL
        : handle.parameterId;

  // A chosen knob the read list no longer has (never while the list is still unread).
  const offered =
    handle == null ||
    (onDefaultBlock && handle.parameterId === AMP_OUTPUT) ||
    (resolved && fetch.list.some((c) => sameHandle(handleOf(c), handle)));
  const stale = resolved && !offered;

  const needsBlock = def.kind === "none" && handle == null;
  const nothingToChoose =
    needsBlock && resolved && groups.every((g) => bestEnabled(g) == null);

  const pickBlock = (value: string): BlockLevelHandle | null | undefined => {
    if (value === DEFAULT_CONTROL)
      return def.kind === "none" ? undefined : null;
    const best = bestEnabled(groupFor(value) ?? []);
    return best ? handleOf(best) : undefined;
  };
  const pickKnob = (value: string): BlockLevelHandle | null | undefined => {
    if (value === DEFAULT_CONTROL)
      return def.kind === "none" ? undefined : null;
    const c = knobGroup.find((k) => k.parameterId === value);
    return c && c.disabled !== true ? handleOf(c) : undefined;
  };

  return {
    block: {
      value: blockValue,
      options: blockOptions,
      staleLabel: handle ? handle.nodeId : undefined,
      loading,
      listNote,
      disabled: nothingToChoose,
    },
    knob: {
      value: knobValue,
      options: knobOpts,
      staleLabel: handle ? paramLabel(handle.parameterId) : undefined,
      loading,
      listNote,
      disabled: blockValue === "",
    },
    stale,
    needsBlock,
    nothingToChoose,
    pickBlock,
    pickKnob,
  };
}

/** The target a sound gets "by name": the target whose name the sound's name starts
 *  with, ignoring case — the longest such name when several do ("Lead Boost 2" gets
 *  "Lead Boost" over "Lead") — else the first target. `""` when there are no targets. */
export function targetByName(
  soundName: string,
  targetNames: readonly string[],
): string {
  const name = soundName.toLowerCase();
  let best: string | undefined;
  for (const t of targetNames) {
    if (t === "" || !name.startsWith(t.toLowerCase())) continue;
    if (best === undefined || t.length > best.length) best = t;
  }
  return best ?? (targetNames.length > 0 ? targetNames[0] : "");
}
