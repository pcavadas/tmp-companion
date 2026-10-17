// src/views/level/summaryModel.ts — what the leveling Summary says, pure.
//
// Every sound a run of the flow ran lands in one of three sections, in run order: "Need
// you" (something to know or do), "Not reached" (a Stop came first) and "Leveled". Each
// row carries one plain sentence per fact (no loudness numbers, no knob values), at most
// one badge, and the Summary's one action is "Try again (N)": the sounds another run can
// fix, run again straight away on the same settings. A sound that fails the same way
// every time (a typed skip kind that says so, a sound that can't reach its target, a
// routing problem) gets its message and stays out of Try again.
//
// Words are chosen by typed fields only (`outcome`, `clampKind`, `skipKind`,
// `persistMismatch`, `silenceHint`, `verifyByEar`, and the flow's `reampStillOn`), never
// by the backend's text, which goes to the log.
//
// Two rules beyond one row per sound:
// - A main sound tried again moves every other sound of its preset (its level is a
//   multiplier over all of them), so its preset's leveled sounds join the same Try again,
//   counted in N, and the run puts the main sound first (`runRank`).
// - When every sound of a preset needs you for the same reason, they share one row
//   ("All 5 sounds").
// One Notice at most: re-amp may still be on (the flow's latest reading), else a message
// two or more sounds share (loud peaks, then one check-by-ear cause), which then leaves
// those rows.

import type { SelectOption } from "../../ui/selectValue";
import {
  MAIN_SOUND,
  type RunItem,
  type RunSkipKind,
  type TargetLufsOf,
} from "./leveling";
import {
  clampCause,
  footswitchNo,
  notReached,
  type StatusMarkKind,
} from "./runModel";

/** A predicted true peak above this (dBTP) earns "Loud peaks". */
const TRUE_PEAK_WARN_DBTP = -1;

/** Skips that happen the same way on every run: their sounds stay out of Try again, and
 *  each says what to change (`SUMMARY_TEXT.sameEveryTime`). */
type SameEveryTimeKind = Extract<
  RunSkipKind,
  | "no_active_amp"
  | "level_manually"
  | "scene_write_refused"
  | "unsupported_knob"
  | "knob_missing"
  | "block_off"
  | "scene_change_discards"
  | "no_target"
>;

type ByEar = NonNullable<RunItem["verifyByEar"]>;

export const SUMMARY_TEXT = {
  saveDidntHold: "Leveled, but the save didn't hold. Try again.",
  offTarget: "Didn't reach its target this time.",
  notConfirmed:
    "Volume set, but the final check didn't finish. Try again to confirm it.",
  fullVolume: (target: string) =>
    `Already at full volume, so it can't reach ${target}. Choose a quieter target for it.`,
  effectFloor: (target: string) =>
    `This effect can't go any quieter without losing it, so it stays louder than ${target}.`,
  knobBottom: (target: string) =>
    `Already as quiet as this knob goes, so it stays louder than ${target}. Choose a louder target for it.`,
  routing:
    "This sound doesn't reach USB 1/2 on your Tone Master Pro, so leveling can't hear it. Route it to USB 1/2, or set its volume by ear.",
  ampZero:
    "Its amp's output is saved at zero, so leveling can't hear it. Turn it up on your Tone Master Pro, then level it again.",
  expMute:
    "An expression pedal turns its amp's output down to zero at one end. Move the pedal to the other end, then level it again.",
  sceneOff: (n: number, scene: string) =>
    `Footswitch ${String(n)} doesn't turn its effect on in ${scene}, so there was nothing to level. Choose another scene for it.`,
  presetUnreadable:
    "Couldn't read this preset from your Tone Master Pro, so its volume wasn't changed.",
  footswitchesUnreadable:
    "Couldn't read this preset's footswitches, so its volume wasn't changed.",
  /** A sound another run can fix: a skip that may not happen again (a capture that
   *  failed, a lost connection, anything untyped), or a trade that was backed out. */
  notLeveled: "Couldn't level this sound, so its volume wasn't changed.",
  /** A skip that happens the same way every time: what happened, then what to change. A
   *  knob or block is chosen on Set up, so the fix is for the next time it is leveled. */
  sameEveryTime: {
    no_active_amp:
      "No amp is on in this sound, so its volume wasn't changed. Choose a block and knob for it when you level it again.",
    level_manually:
      "Leveling can't choose a knob for this sound's amps on its own, so its volume wasn't changed. Choose a block and knob for it when you level it again.",
    scene_write_refused:
      "This scene shares its knob with the main sound, so its volume wasn't changed. Turn on Scene Edit for that block on your Tone Master Pro, or choose another knob for it when you level it again.",
    unsupported_knob:
      "That knob isn't a volume or mix knob, so its volume wasn't changed. Choose a volume or mix knob for it when you level it again.",
    knob_missing:
      "That knob isn't in the preset any more, so its volume wasn't changed. Choose another knob for it when you level it again.",
    block_off:
      "That block is off in this sound, so its volume wasn't changed. Choose another block for it when you level it again.",
    scene_change_discards:
      "Your Tone Master Pro is set to throw away scene changes, so leveling can't save them. On your Tone Master Pro, set Scene Change Behavior to Maintain changes. Then unplug it, plug it back in, and level it again.",
    no_target:
      "Its target isn't in Settings any more, so its volume wasn't changed. Choose a target for it when you level it again.",
  } satisfies Record<SameEveryTimeKind, string>,
  loudPeaks:
    "It peaks close to the maximum. If your interface or speaker distorts on it, level it to a quieter target.",
  loudPeaksMany:
    "These sounds peak close to the maximum. If your interface or speaker distorts on them, level them to a quieter target.",
  byEar: {
    envelope:
      "It has an envelope filter, which reacts to the test tone differently from your playing. Listen to check it.",
    rebalance: "Its two amps were balanced roughly. Listen to check it.",
    wet_floor:
      "Its effect is at the lowest mix that keeps it in the sound. Listen to check it.",
  } satisfies Record<ByEar, string>,
  byEarMany: {
    envelope:
      "These presets have an envelope filter, which reacts to the test tone differently from your playing. Listen to check them.",
    rebalance:
      "The two amps in these presets were balanced roughly. Listen to check them.",
    wet_floor:
      "These effects are at the lowest mix that keeps them in the sound. Listen to check them.",
  } satisfies Record<ByEar, string>,
  baseBoost: (target: string) =>
    `Turned the preset up as far as it goes, then raised the amp's output to reach ${target}.`,
  reampStillOn:
    "Your guitar input may still be muted. Restart your Tone Master Pro.",
} as const;

export type SummarySection = "Need you" | "Not reached" | "Leveled";

export interface SummaryBadge {
  text: "Loud peaks" | "Check by ear";
  tone: "caution" | "neutral";
}

/** A footswitch row's "Level it in scene" dropdown. */
export interface SummarySceneChoice {
  /** The row's key (the store's `summaryScenes` key). */
  key: string;
  /** e2e hook (`data-select`): `summary-scene:<hookKey>`. */
  tid: string;
  label: string;
  /** `"base"` (the main sound) or the scene's 0-based wire index. */
  value: string;
  options: SelectOption[];
  /** The scene the run used, as a value. */
  used: string;
}

export interface SummaryRowModel {
  /** React key: the sound's key, or `all:<slot>` for a shared row. */
  key: string;
  /** The sounds the row stands for. */
  keys: string[];
  /** One sound's e2e identity (`setupRowHookKey`); null on a shared row. */
  hookKey: string | null;
  preset: string;
  /** "Main sound" / the scene / "Boost, footswitch 3" / "All 5 sounds". */
  sound: string;
  mark: StatusMarkKind;
  badge: SummaryBadge | null;
  messages: string[];
  /** "On target" (Leveled) / "Not reached"; none on a Need you row. */
  word: "On target" | "Not reached" | null;
  scene: SummarySceneChoice | null;
  /** e2e hook on a main sound whose preset level ran out: was the amp's output raised
   *  (`applied`) or only planned (`planned`)? */
  baseBoost: "applied" | "planned" | null;
}

export interface SummaryNotice {
  tone: "caution" | "info";
  title: string | null;
  text: string;
}

export interface SummaryModel {
  title: string;
  subtitle: string | null;
  notice: SummaryNotice | null;
  sections: { label: SummarySection; rows: SummaryRowModel[] }[];
  /** The sounds "Try again" runs, in run order; its count is N. */
  retryKeys: string[];
}

export interface SummaryContext {
  /** The last run ended by a Stop. */
  stopped: boolean;
  /** Re-amp may still be on: the flow's latest reading (`LevelFlowState.reampStillOn`),
   *  never a row's own flag, which an earlier run may have set. */
  reampStillOn: boolean;
  targetLufsByName: TargetLufsOf;
  /** The preset's scene names, index-aligned with the wire `scenes[]` slots. */
  sceneNamesOf: (slot: number) => readonly string[];
  /** The scenes whose overlay turns a switch's effect on, or null when unknown. */
  enablingScenesOf: (slot: number, switchIndex: number) => number[] | null;
  /** Footswitch scenes changed on the Summary (row key → scene, null = main sound). */
  sceneOverrides: Partial<Record<string, number | null>>;
  /** e2e identity of a sound. */
  hookKeyOf: (it: RunItem) => string;
}

/** One sound before rows are shared. */
interface Verdict {
  it: RunItem;
  section: SummarySection;
  mark: StatusMarkKind;
  /** The outcome's own sentence, if any. */
  primary: string | null;
  extras: string[];
  loudPeaks: boolean;
  byEar: ByEar | null;
  retry: boolean;
  scene: SummarySceneChoice | null;
}

const isSameEveryTime = (kind: RunSkipKind): kind is SameEveryTimeKind =>
  Object.prototype.hasOwnProperty.call(SUMMARY_TEXT.sameEveryTime, kind);

const sceneValue = (ctx: number | null): string =>
  ctx === null ? "base" : String(ctx);

function verdictOf(it: RunItem, c: SummaryContext): Verdict {
  const target = it.targetName;
  const base: Omit<Verdict, "section" | "mark" | "primary" | "retry"> = {
    it,
    extras: [],
    loudPeaks: false,
    byEar: null,
    scene: null,
  };
  if (notReached(it))
    return {
      ...base,
      section: "Not reached",
      mark: "waiting",
      primary: null,
      retry: true,
    };
  if (it.persistMismatch === true)
    return {
      ...base,
      section: "Need you",
      mark: "caution",
      primary: SUMMARY_TEXT.saveDidntHold,
      retry: true,
    };

  const v: Verdict = {
    ...base,
    section: "Need you",
    mark: "danger",
    primary: null,
    retry: false,
  };
  // A sound that reached its result: its by-ear cause, loud peaks and base boost.
  if (it.outcome === "done" || it.outcome === "clamped") {
    const cause = it.verifyByEar;
    if (
      cause != null &&
      (it.outcome === "done" || cause === "envelope" || cause === "rebalance")
    )
      v.byEar = cause;
    v.loudPeaks =
      it.isBase &&
      it.truePeakDbtp != null &&
      it.truePeakDbtp > TRUE_PEAK_WARN_DBTP;
    if (it.baseBoost?.applied === true)
      v.extras.push(SUMMARY_TEXT.baseBoost(target));
  }

  // A footswitch whose effect is off in the scene it was leveled in: choose another.
  const fs = it.footswitch;
  const n = footswitchNo(it);
  const names = c.sceneNamesOf(it.slot);
  if (fs != null && n != null && names.length > 0 && it.outcome !== "done") {
    const used = fs.sceneContext;
    const enabling = c.enablingScenesOf(it.slot, fs.switchIndex);
    if (
      typeof used === "number" &&
      enabling != null &&
      !enabling.includes(used)
    ) {
      const labelOf = (i: number) =>
        i < names.length && names[i] ? names[i] : `Scene ${String(i + 1)}`;
      const override = c.sceneOverrides[it.key];
      const usedValue = sceneValue(used);
      const value = override !== undefined ? sceneValue(override) : usedValue;
      v.mark = "caution";
      v.primary = SUMMARY_TEXT.sceneOff(n, labelOf(used));
      v.retry = value !== usedValue;
      v.scene = {
        key: it.key,
        tid: `summary-scene:${c.hookKeyOf(it)}`,
        label: `Scene to level footswitch ${String(n)} of ${it.presetName} in`,
        value,
        used: usedValue,
        options: [
          { value: "base", label: MAIN_SOUND },
          ...names.map((_, i) => ({ value: String(i), label: labelOf(i) })),
        ],
      };
      return v;
    }
  }

  switch (it.outcome) {
    case "done":
      return { ...v, section: "Leveled", mark: "done" };
    case "unconverged":
      return {
        ...v,
        mark: "caution",
        primary:
          it.unverified === true
            ? SUMMARY_TEXT.notConfirmed
            : SUMMARY_TEXT.offTarget,
        retry: true,
      };
    case "clamped": {
      const cause = clampCause(it, c.targetLufsByName(target));
      if (cause === "failed")
        return { ...v, primary: SUMMARY_TEXT.notLeveled, retry: true };
      return {
        ...v,
        primary:
          cause === "full"
            ? SUMMARY_TEXT.fullVolume(target)
            : it.clampKind === "wet_floor"
              ? SUMMARY_TEXT.effectFloor(target)
              : SUMMARY_TEXT.knobBottom(target),
      };
    }
    case "offbranch":
      return {
        ...v,
        primary:
          it.silenceHint === "amp_zero"
            ? SUMMARY_TEXT.ampZero
            : it.silenceHint === "exp_mute"
              ? SUMMARY_TEXT.expMute
              : SUMMARY_TEXT.routing,
      };
    default: {
      const kind = it.skipKind ?? "other";
      if (isSameEveryTime(kind))
        return { ...v, primary: SUMMARY_TEXT.sameEveryTime[kind] };
      return {
        ...v,
        primary:
          kind === "preset_unreadable"
            ? SUMMARY_TEXT.presetUnreadable
            : kind === "footswitches_unreadable"
              ? SUMMARY_TEXT.footswitchesUnreadable
              : SUMMARY_TEXT.notLeveled,
        retry: true,
      };
    }
  }
}

const soundsWord = (n: number) =>
  `${String(n)} ${n === 1 ? "sound" : "sounds"}`;
const needYou = (n: number) =>
  `${String(n)} ${n === 1 ? "needs" : "need"} you.`;

/** The Summary of `items` (every sound the flow ran, in run order). */
export function buildSummary(
  items: readonly RunItem[],
  c: SummaryContext,
): SummaryModel {
  const verdicts = items.map((it) => verdictOf(it, c));

  // One Notice at most: re-amp, else a message two or more sounds share.
  const loud = verdicts.filter((v) => v.loudPeaks).length;
  const byEarCount = (cause: ByEar) =>
    verdicts.filter((v) => v.byEar === cause).length;
  const sharedByEar =
    (["envelope", "rebalance", "wet_floor"] as const).find(
      (cause) => byEarCount(cause) >= 2,
    ) ?? null;
  let notice: SummaryNotice | null = null;
  let loudShared = false;
  let byEarShared: ByEar | null = null;
  if (c.reampStillOn)
    notice = { tone: "caution", title: null, text: SUMMARY_TEXT.reampStillOn };
  else if (loud >= 2) {
    loudShared = true;
    notice = {
      tone: "caution",
      title: "Loud peaks.",
      text: SUMMARY_TEXT.loudPeaksMany,
    };
  } else if (sharedByEar != null) {
    byEarShared = sharedByEar;
    notice = {
      tone: "info",
      title: null,
      text: SUMMARY_TEXT.byEarMany[sharedByEar],
    };
  }

  const rowOf = (v: Verdict): SummaryRowModel => {
    const { it } = v;
    const n = footswitchNo(it);
    const messages = [
      ...(v.primary != null ? [v.primary] : []),
      ...v.extras,
      ...(v.loudPeaks && !loudShared ? [SUMMARY_TEXT.loudPeaks] : []),
      ...(v.byEar != null && v.byEar !== byEarShared
        ? [SUMMARY_TEXT.byEar[v.byEar]]
        : []),
    ];
    const badge: SummaryBadge | null = v.loudPeaks
      ? { text: "Loud peaks", tone: "caution" }
      : v.byEar != null
        ? { text: "Check by ear", tone: "neutral" }
        : null;
    return {
      key: it.key,
      keys: [it.key],
      hookKey: c.hookKeyOf(it),
      preset: it.presetName,
      sound:
        n != null ? `${it.sceneName}, footswitch ${String(n)}` : it.sceneName,
      mark: v.mark,
      badge,
      messages,
      word:
        v.section === "Leveled"
          ? "On target"
          : v.section === "Not reached"
            ? "Not reached"
            : null,
      scene: v.scene,
      baseBoost:
        it.baseBoost == null
          ? null
          : it.baseBoost.applied
            ? "applied"
            : "planned",
    };
  };

  // Rows, with a preset whose every sound needs you for the same reason on one row.
  const bySlot = new Map<number, SummaryRowModel[]>();
  const rows = verdicts.map((v) => {
    const r = rowOf(v);
    const list = bySlot.get(v.it.slot) ?? [];
    list.push(r);
    bySlot.set(v.it.slot, list);
    return { v, r };
  });
  const sameReason = (list: SummaryRowModel[]) =>
    list.length >= 2 &&
    list.every(
      (r) =>
        r.word === null &&
        r.badge === null &&
        r.scene === null &&
        r.mark === list[0].mark &&
        r.messages.join("\n") === list[0].messages.join("\n"),
    );
  const shared = new Set(
    [...bySlot.entries()].filter(([, l]) => sameReason(l)).map(([s]) => s),
  );
  const sections = (["Need you", "Not reached", "Leveled"] as const)
    .map((label) => {
      const out: SummaryRowModel[] = [];
      const done = new Set<number>();
      for (const { v, r } of rows) {
        if (v.section !== label) continue;
        const slot = v.it.slot;
        if (!shared.has(slot)) {
          out.push(r);
          continue;
        }
        if (done.has(slot)) continue;
        done.add(slot);
        const list = bySlot.get(slot) ?? [r];
        out.push({
          ...r,
          key: `all:${String(slot)}`,
          keys: list.flatMap((x) => x.keys),
          hookKey: null,
          sound: `All ${String(list.length)} sounds`,
          baseBoost: null,
        });
      }
      return { label, rows: out };
    })
    .filter((s) => s.rows.length > 0);

  // Try again: the sounds another run can fix, and the leveled sounds of a preset whose
  // main sound is among them.
  const retry = new Set(verdicts.filter((v) => v.retry).map((v) => v.it.key));
  const mainSoundSlots = new Set(
    verdicts.filter((v) => v.retry && v.it.isBase).map((v) => v.it.slot),
  );
  for (const v of verdicts)
    if (v.section === "Leveled" && mainSoundSlots.has(v.it.slot))
      retry.add(v.it.key);
  const retryKeys = items.filter((it) => retry.has(it.key)).map((it) => it.key);

  // Title and subtitle, counted in sounds.
  const count = (s: SummarySection) =>
    verdicts.filter((v) => v.section === s).length;
  const leveled = count("Leveled");
  const need = count("Need you");
  const notRun = count("Not reached");
  let title: string;
  let subtitle: string | null = null;
  if (c.stopped && notRun > 0) {
    title = `Stopped. ${leveled > 0 ? `${soundsWord(leveled)} leveled` : "Nothing was leveled"}, ${String(notRun)} not reached.`;
    if (need > 0) subtitle = needYou(need);
  } else {
    const all = need + notRun;
    if (all === 0) {
      title =
        leveled === 1
          ? "1 sound leveled."
          : `All ${String(leveled)} sounds leveled.`;
      subtitle = "Everything is saved on your Tone Master Pro.";
    } else if (leveled === 0)
      title = `Nothing was leveled. ${soundsWord(all)} ${all === 1 ? "needs" : "need"} you.`;
    else title = `${soundsWord(leveled)} leveled. ${needYou(all)}`;
  }

  return { title, subtitle, notice, sections, retryKeys };
}
