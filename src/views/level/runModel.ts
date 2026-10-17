// src/views/level/runModel.ts — what the leveling run page says, pure: each row's mark and
// state word, the now line, and the clamp cause both the run page and the Summary word
// from. Every outcome is read off typed fields (`outcome`, `clampKind`, `skipKind`,
// `persistMismatch`), never off the backend's text. No loudness numbers reach the screen:
// a value is only ever compared with its target.

import type { RunPhase } from "./levelRunStore";
import type { RunItem } from "./leveling";

/** A row's 16 px status mark: done (good circle with a check), listening (a pulsing
 *  accent dot), waiting (a hollow dot), or a problem dot (caution or danger). */
export type StatusMarkKind =
  "done" | "listening" | "waiting" | "caution" | "danger";

/** Why a clamped sound stopped short, in the user's terms: at full volume (`louder`
 *  needed), as quiet as it goes (`quieter` needed: the effect's mix floor, or a knob at
 *  its bottom still louder than the target), or the trade around it was backed out
 *  (`failed`: nothing was saved, so trying again can help). */
export type ClampCause = "full" | "quieter" | "failed";

/** The clamp cause of a clamped row, by its typed `clampKind`, and for an ordinary clamp
 *  by which side of the target it stayed on: the loudness the run leveled it to
 *  (`RunItem.targetLufs`), else `targetLufs` (its target's now). With neither, the side
 *  can't be told, and the row reads as at full volume. */
export function clampCause(it: RunItem, targetLufs: number | null): ClampCause {
  if (it.clampKind === "partial_trade") return "failed";
  if (it.clampKind === "wet_floor") return "quieter";
  const target = it.targetLufs ?? targetLufs;
  return it.value != null && target != null && it.value > target
    ? "quieter"
    : "full";
}

/** The sound a stopped run never reached: no outcome, or the base row the Stop cancelled
 *  while it was in flight. */
export function notReached(it: RunItem): boolean {
  return (
    it.outcome == null ||
    (it.outcome === "skipped" && it.skipKind === "cancelled")
  );
}

/** A footswitch row's switch number as the unit prints it (1-based), else null. */
export function footswitchNo(it: Pick<RunItem, "footswitch">): number | null {
  return it.footswitch != null ? it.footswitch.switchIndex + 1 : null;
}

export interface RunRowState {
  mark: StatusMarkKind;
  word: string;
  /** The word's colour role. */
  tone: "good" | "text" | "secondary";
}

/** One run row: its mark and state word. */
export function runRowState(
  it: RunItem,
  stopping: boolean,
  targetLufs: number | null,
): RunRowState {
  if (it.status === "active")
    return {
      mark: "listening",
      word: stopping ? "Stopping" : "Listening",
      tone: "text",
    };
  if (it.status === "queued")
    return { mark: "waiting", word: "Waiting", tone: "secondary" };
  if (notReached(it))
    return { mark: "waiting", word: "Not reached", tone: "secondary" };
  if (it.persistMismatch === true)
    return { mark: "caution", word: "Save didn't hold", tone: "secondary" };
  const problem = (mark: "caution" | "danger", word: string): RunRowState => ({
    mark,
    word,
    tone: "secondary",
  });
  switch (it.outcome) {
    case "done":
      return { mark: "done", word: "On target", tone: "good" };
    case "clamped": {
      const cause = clampCause(it, targetLufs);
      return cause === "failed"
        ? problem("danger", "Couldn't level it")
        : problem(
            "danger",
            cause === "quieter" ? "Can't go quieter" : "At full volume",
          );
    }
    case "unconverged":
      return problem(
        "caution",
        it.unverified === true ? "Not confirmed" : "Didn't reach its target",
      );
    case "offbranch":
      return problem("danger", "Can't hear it");
    default:
      return problem("danger", "Couldn't level it");
  }
}

/** The run page's now line, and the row the meter follows (null before any). The words
 *  come from the rows, never the backend: a batch's save/verify tail and the gap between
 *  sounds read as the preset finishing. */
export function nowLine(
  items: readonly RunItem[],
  currentIndex: number,
  phase: RunPhase,
  tailMessage: string | null,
): { text: string; item: RunItem | null; listening: boolean } {
  const active = items.find((x) => x.status === "active") ?? null;
  if (phase === "stopping")
    return {
      text: active
        ? `Stopping after ${active.sceneName} in ${active.presetName}`
        : "Stopping",
      item: active,
      listening: false,
    };
  if (active && tailMessage == null)
    return {
      text: `Listening to ${active.sceneName} in ${active.presetName}`,
      item: active,
      listening: true,
    };
  const last =
    items.length > 0
      ? items[Math.max(0, Math.min(currentIndex, items.length) - 1)]
      : null;
  return {
    text: last ? `Finishing ${last.presetName}` : "",
    item: last,
    listening: false,
  };
}
