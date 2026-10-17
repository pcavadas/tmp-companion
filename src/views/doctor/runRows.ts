// src/views/doctor/runRows.ts — what Doctor's Check page says, pure: each row's mark and
// state word, and the line in the now card.

import type { DoctorInputArg } from "../../lib/types";
import { MAIN_SOUND } from "../level/leveling";
import type { StatusMarkKind } from "../level/runModel";
import {
  runEnded,
  type DoctorFlowStatus,
  type DoctorRunStatus,
} from "./useDoctorFlow";

/** A checked sound's two names: its preset, and the sound inside it. */
export interface RunName {
  preset: string;
  sound: string;
}

export interface CheckRowState {
  mark: StatusMarkKind;
  word: string;
  tone: "good" | "text" | "secondary";
}

/** One row of the Check page. Once the run has ended, a sound still in flight or still
 *  waiting was never checked. */
export function checkRowState(
  status: DoctorFlowStatus,
  rowStatus: DoctorRunStatus | undefined,
): CheckRowState {
  const ended = runEnded(status);
  switch (rowStatus) {
    case "done":
      return { mark: "done", word: "Checked", tone: "good" };
    case "error":
      return { mark: "caution", word: "Couldn't check", tone: "text" };
    case "active":
      return ended
        ? { mark: "waiting", word: "Not checked", tone: "secondary" }
        : { mark: "listening", word: "Listening", tone: "text" };
    default:
      return ended
        ? { mark: "waiting", word: "Not checked", tone: "secondary" }
        : { mark: "waiting", word: "Waiting", tone: "secondary" };
  }
}

/** "Tweed Warm" for a main sound, "Chorus in Plexi Crunch" for any other. */
export function soundInPreset(n: RunName): string {
  return n.sound === MAIN_SOUND ? n.preset : `${n.sound} in ${n.preset}`;
}

/** The now card's line. */
export function checkNowLine(
  status: DoctorFlowStatus,
  items: readonly DoctorInputArg[],
  statusByKey: Readonly<Record<string, DoctorRunStatus>>,
  nameOf: (key: string) => RunName,
): string {
  if (status === "failed")
    return "Nothing changed on your Tone Master Pro. Try again, or go back to your presets.";
  if (status === "done")
    return items.length === 1
      ? "The sound is checked."
      : `All ${String(items.length)} sounds checked.`;
  // The sound in flight; before the first one starts, the first one waiting.
  const current =
    items.find((it) => statusByKey[it.key] === "active") ??
    items.find((it) => (statusByKey[it.key] ?? "queued") === "queued") ??
    (items.length > 0 ? items[items.length - 1] : null);
  const who = current ? soundInPreset(nameOf(current.key)) : "";
  if (status === "stopping" || status === "stopped")
    return `Finishing ${who} first.`;
  return `Listening to ${who}. Nothing changes on your Tone Master Pro.`;
}
