// src/views/doctor/resultsModel.ts — what Doctor's Results page says, pure.
//
// One flat list across every checked preset, in four groups:
//   1. the sounds with findings and the preset-level findings (a preset's "Scenes" row
//      for a volume jump between scenes, its "Footswitches" row for assignments that
//      change more than the volume), sorted together worst first — the tone the row
//      shows (danger, then caution), then slot, then the order inside the preset;
//   2. the sounds that couldn't be checked;
//   3. the sounds a stopped check never reached;
//   4. the clean sounds ("Sounds good").
// Each row's caption is chosen from typed fields only (`diags`, `errorKind`, `status`), never
// from the backend's text. The title says how many rows need a look, or that there is
// nothing to change — and never claims "nothing to change in these sounds" while a sound
// went unchecked (#190).

import type {
  DoctorCheckResult,
  DoctorLevelingDamageHint,
  DoctorLevelingDamageKind,
  DoctorPresetResult,
  DoctorSceneConsistency,
  DoctorSoundResult,
  FootswitchInfo,
  GraphNode,
  SilenceHint,
  SkipKind,
} from "../../lib/types";
import { slotLabel } from "../../lib/format";
import { chargedTotal, overCpuBudget } from "../../models/cpu";
import type { SetupOption } from "../level/leveling";
import type { StatusMarkKind } from "../level/runModel";
import {
  bandLayoutsMatch,
  eqBandLabel,
  eqMovesFor,
  EQ10_STEREO,
  existingEq10,
  matchDeltas,
  matchResidualLarge,
} from "./matchModel";
import {
  possibleLabel,
  sceneLoudnessTone,
  sortedDiags,
  soundTone,
  toneRank,
  type FindingTone,
} from "./severity";

/** How a checked sound is named on screen: its preset, and the sound inside it ("Main
 *  sound", a scene's name, or a footswitch's name with `footswitch` set). */
export interface SoundName {
  preset: string;
  name: string;
  /** The footswitch number as the unit prints it (1-based), for a footswitch sound. */
  footswitch: number | null;
}

/** A chosen sound's name, from its list row: `chosenFrom`'s own `sceneName`, verbatim
 *  ("Main sound" for every main sound), and a footswitch's 1-based number. */
export function soundNameOf(o: SetupOption): SoundName {
  return {
    preset: o.presetName,
    name: o.sceneName,
    footswitch: o.footswitch ? o.footswitch.switchIndex + 1 : null,
  };
}

/** The sound's own words: "Main sound", "Chorus", or "Boost, footswitch 3". */
export function soundLabel(n: SoundName): string {
  return n.footswitch != null
    ? `${n.name}, footswitch ${String(n.footswitch)}`
    : n.name;
}

/** The sound's name inside a sentence: "Main sound", "Chorus", or "footswitch 3", as the
 *  opened "Scenes" row's own sentence names it. The backend writes it into the steps for a
 *  volume jump. */
export function soundInSentence(n: SoundName): string {
  return n.footswitch != null ? `footswitch ${String(n.footswitch)}` : n.name;
}

/** Every app-side string on the Results page that a model function picks. */
export const RESULTS_TEXT = {
  subtitle:
    "Worst first. Make the change on your Tone Master Pro, then check again.",
  soundsGood: "Sounds good",
  notChecked: "Not checked",
  couldntCheck: "Couldn't check this sound. Check again.",
  ampZero: "The amp's output is at zero, so there was nothing to hear.",
  expMute: "An expression pedal mutes this sound. Move it, then check again.",
  noUsb: "Nothing came out of USB 1/2, so there was nothing to hear.",
  scenes: "Scenes",
  scenesCaption: "Volume jumps between scenes",
  footswitches: "Footswitches",
  levelScenesStep: "Level this preset's scenes on the Level tab.",
  footswitchIntro:
    "These footswitches change more than the volume. Check them if you didn't set them up that way.",
  tooQuiet: "Some bands were too quiet to check.",
  shared:
    "This block is shared, so the change affects every sound in this preset.",
  whatToChange: "What to change",
  cutThrough: "Cutting through the mix",
  cutThroughWeak:
    "Less presence than most factory presets, so it gets lost in a busy mix.",
  residual: "EQ only gets part of the way. A different cab or amp gets closer.",
} as const;

/** What one footswitch assignment does, in the player's words. */
export const FOOTSWITCH_KIND_TEXT: Record<DoctorLevelingDamageKind, string> = {
  deletedEffect: "Turns the effect almost silent when you press it.",
  sweptOther: "Changes the tone, not only the volume, when you press it.",
};

/** The caption of a sound that couldn't be checked, by the failure's typed kind: a
 *  silent capture (`no_signal`) names the preset's backup-scan cause when there is one. */
export function errorLine(
  kind: SkipKind | null,
  hint: SilenceHint | undefined,
): string {
  if (kind !== "no_signal") return RESULTS_TEXT.couldntCheck;
  if (hint === "amp_zero") return RESULTS_TEXT.ampZero;
  if (hint === "exp_mute") return RESULTS_TEXT.expMute;
  return RESULTS_TEXT.noUsb;
}

/** "When it's heard": the caption beside a finding's venue icons, from its quietest
 *  playback level. */
export function heardAt(level: "quiet" | "rehearsal" | "stage"): string {
  if (level === "quiet") return "Heard at every volume";
  if (level === "rehearsal") return "Heard at rehearsal volume and up";
  return "Heard only at stage volume";
}

export type ResultRowKind = "sound" | "scenes" | "footswitches";

export interface ResultRow {
  /** Stable id: `${listIndex}|${sound key}`, `${listIndex}|consistency` or
   *  `${listIndex}|leveling-damage`. */
  id: string;
  kind: ResultRowKind;
  preset: DoctorPresetResult;
  /** The sound, for a sound row. */
  sound: DoctorSoundResult | null;
  presetName: string;
  /** The second half of the row's name ("Main sound", "Scenes", "Footswitches"). */
  soundName: string;
  mark: StatusMarkKind;
  caption: string;
  captionTone: "text" | "good" | "secondary";
  /** The row opens to something. */
  expandable: boolean;
  /** The tone the row shows, for a row with findings. */
  tone: FindingTone | null;
}

export interface ResultsModel {
  /** Groups 1 and 2 above: the rows that need a look, worst first. */
  findings: ResultRow[];
  failed: ResultRow[];
  unrun: ResultRow[];
  clean: ResultRow[];
  title: string;
  /** Only when it adds something (the findings' "Worst first…"). */
  subtitle: string | null;
  /** Every sound was checked and nothing needs a look: the footer's only action is
   *  "Check other sounds". */
  allClear: boolean;
  /** The clean rows fold into one "N more sound good" row (there are findings above). */
  foldClean: boolean;
  /** The row open on arrival: the worst finding's. */
  firstOpen: string | null;
}

export interface ResultsContext {
  /** Each checked sound's name by key; a sound missing here falls back to its preset's
   *  name and the wire label. */
  soundNames: ReadonlyMap<string, SoundName>;
  presetNames: ReadonlyMap<number, string>;
  silenceHintByIndex: ReadonlyMap<number, SilenceHint>;
}

function presetNameOf(ctx: ResultsContext, listIndex: number): string {
  return ctx.presetNames.get(listIndex) ?? `Slot ${slotLabel(listIndex)}`;
}

/** A sound's name on screen. */
export function nameOf(
  ctx: ResultsContext,
  sound: DoctorSoundResult,
): { preset: string; sound: string } {
  const n = ctx.soundNames.get(sound.key);
  return n
    ? { preset: n.preset, sound: soundLabel(n) }
    : { preset: presetNameOf(ctx, sound.listIndex), sound: sound.label };
}

/** How many distinct footswitches carry an assignment worth checking. */
export function footswitchCount(hints: readonly DoctorLevelingDamageHint[]) {
  return new Set(hints.map((h) => h.switch)).size;
}

const plural = (n: number, one: string, many: string): string =>
  `${String(n)} ${n === 1 ? one : many}`;

function soundRow(
  ctx: ResultsContext,
  preset: DoctorPresetResult,
  sound: DoctorSoundResult,
): ResultRow {
  const names = nameOf(ctx, sound);
  const base = {
    id: `${String(preset.listIndex)}|${sound.key}`,
    kind: "sound" as const,
    preset,
    sound,
    presetName: names.preset,
    soundName: names.sound,
  };
  if (sound.status === "error")
    return {
      ...base,
      mark: "caution",
      caption: errorLine(
        sound.errorKind,
        ctx.silenceHintByIndex.get(preset.listIndex),
      ),
      captionTone: "text",
      expandable: false,
      tone: null,
    };
  if (sound.status === "unrun")
    return {
      ...base,
      mark: "waiting",
      caption: RESULTS_TEXT.notChecked,
      captionTone: "secondary",
      expandable: false,
      tone: null,
    };
  const tone = soundTone(sound);
  if (tone == null)
    return {
      ...base,
      mark: "done",
      caption: RESULTS_TEXT.soundsGood,
      captionTone: "good",
      // A clean sound opens to its cut-through read and "Use as reference".
      expandable: sound.cutThrough != null,
      tone: null,
    };
  return {
    ...base,
    mark: tone,
    caption: sortedDiags(sound.diags).map(possibleLabel).join(", "),
    captionTone: "text",
    expandable: true,
    tone,
  };
}

function scenesRow(
  ctx: ResultsContext,
  preset: DoctorPresetResult,
  sc: DoctorSceneConsistency,
): ResultRow {
  const tone = sceneLoudnessTone(sc);
  return {
    id: `${String(preset.listIndex)}|consistency`,
    kind: "scenes",
    preset,
    sound: null,
    presetName: presetNameOf(ctx, preset.listIndex),
    soundName: RESULTS_TEXT.scenes,
    mark: tone,
    caption: RESULTS_TEXT.scenesCaption,
    captionTone: "text",
    expandable: true,
    tone,
  };
}

function footswitchesRow(
  ctx: ResultsContext,
  preset: DoctorPresetResult,
): ResultRow {
  const n = footswitchCount(preset.levelingDamage);
  return {
    id: `${String(preset.listIndex)}|leveling-damage`,
    kind: "footswitches",
    preset,
    sound: null,
    presetName: presetNameOf(ctx, preset.listIndex),
    soundName: RESULTS_TEXT.footswitches,
    mark: "caution",
    caption: `${plural(n, "footswitch", "footswitches")} worth checking`,
    captionTone: "text",
    expandable: true,
    tone: "caution",
  };
}

/** The whole page, from the check's result. */
export function buildResults(
  result: DoctorCheckResult,
  ctx: ResultsContext,
): ResultsModel {
  const presets = [...result.presets].sort((a, b) => a.listIndex - b.listIndex);
  // Findings carry their sort keys: tone, slot, order inside the preset.
  const ranked: {
    row: ResultRow;
    rank: number;
    slot: number;
    order: number;
  }[] = [];
  const failed: ResultRow[] = [];
  const unrun: ResultRow[] = [];
  const clean: ResultRow[] = [];
  let total = 0;
  let measured = 0;
  for (const p of presets) {
    p.sounds.forEach((s, i) => {
      total += 1;
      if (s.status === "measured") measured += 1;
      const row = soundRow(ctx, p, s);
      if (s.status === "error") failed.push(row);
      else if (s.status === "unrun") unrun.push(row);
      else if (row.tone == null) clean.push(row);
      else
        ranked.push({
          row,
          rank: toneRank(row.tone),
          slot: p.listIndex,
          order: i,
        });
    });
    if (p.sceneConsistency) {
      const row = scenesRow(ctx, p, p.sceneConsistency);
      ranked.push({
        row,
        rank: toneRank(sceneLoudnessTone(p.sceneConsistency)),
        slot: p.listIndex,
        order: p.sounds.length,
      });
    }
    if (p.levelingDamage.length > 0) {
      ranked.push({
        row: footswitchesRow(ctx, p),
        rank: toneRank("caution"),
        slot: p.listIndex,
        order: p.sounds.length + 1,
      });
    }
  }
  ranked.sort(
    (a, b) => b.rank - a.rank || a.slot - b.slot || a.order - b.order,
  );
  const findings = ranked.map((r) => r.row);

  let title: string;
  let allClear = false;
  if (findings.length > 0) {
    title =
      findings.length === 1
        ? "1 sound needs a look"
        : `${String(findings.length)} sounds need a look`;
  } else if (measured === total) {
    allClear = true;
    title =
      total === 1
        ? "Nothing to change in this sound"
        : `Nothing to change in these ${String(total)} sounds`;
  } else if (measured === 0) {
    title = result.stopped
      ? "Stopped before any sound was checked"
      : total === 1
        ? "Couldn't check this sound"
        : "Couldn't check these sounds";
  } else {
    const checked = `the ${plural(measured, "sound", "sounds")} checked`;
    title = result.stopped
      ? `Stopped. Nothing to change in ${checked}.`
      : `Nothing to change in ${checked}`;
  }

  return {
    findings,
    failed,
    unrun,
    clean,
    title,
    subtitle: findings.length > 0 ? RESULTS_TEXT.subtitle : null,
    allClear,
    foldClean: findings.length > 0 && clean.length > 0,
    firstOpen: findings.length > 0 ? findings[0].id : null,
  };
}

/** The folded clean row's words: "4 more sound good", "1 more sounds good". */
export function moreGoodLabel(n: number): string {
  return n === 1 ? "1 more sounds good" : `${String(n)} more sound good`;
}

// ── the "Scenes" row, opened ─────────────────────────────────────────────────

/** One bar of the opened "Scenes" row: a sound and how far it sits from the main sound. */
export interface SceneBar {
  label: string;
  deltaDb: number;
  isRef: boolean;
}

/** The sound a scene-consistency row names (its `name` is the sound's wire label). */
function soundByLabel(
  preset: DoctorPresetResult,
  label: string,
): DoctorSoundResult | undefined {
  return preset.sounds.find((s) => s.label === label);
}

/** The opened "Scenes" row: its sentence, its bars and its steps (the backend's, then the
 *  app's own step to level the scenes). When the louder sound is the first scene (wire
 *  scene 0), the unit's reading of it can differ from its footswitch, so the sentence says
 *  the reading isn't certain and the app adds no leveling step: the backend's only step
 *  then asks the player to check it by ear. The dot and the bars keep their tones. */
export function sceneLoudness(
  ctx: ResultsContext,
  preset: DoctorPresetResult,
  sc: DoctorSceneConsistency,
): { sentence: string; bars: SceneBar[]; steps: string[] } {
  const bars = sc.rows.map((r) => {
    const s = soundByLabel(preset, r.name);
    return {
      label: r.isRef ? "Main sound" : s ? nameOf(ctx, s).sound : r.name,
      deltaDb: r.deltaDb,
      isRef: r.isRef,
    };
  });
  const worst = soundByLabel(preset, sc.worstName);
  const name = worst ? ctx.soundNames.get(worst.key) : undefined;
  const amount = `${Math.abs(sc.worstDeltaDb).toFixed(1)} dB ${sc.worstDeltaDb < 0 ? "quieter" : "louder"}`;
  const backendSteps = sc.rx.map((rx) => rx.title);
  if (worst?.scene === 0 && sc.worstDeltaDb > 0) {
    return {
      sentence: `Your ${name?.name ?? sc.worstName} scene read ${amount} than the main sound, but that reading isn't certain.`,
      bars,
      steps: backendSteps,
    };
  }
  const who =
    name?.footswitch != null
      ? `Footswitch ${String(name.footswitch)} is`
      : `Your ${name?.name ?? sc.worstName} scene is`;
  return {
    sentence: `${who} ${amount} than the main sound. That's a big jump when you switch.`,
    bars,
    steps: [...backendSteps, RESULTS_TEXT.levelScenesStep],
  };
}

// ── the "Footswitches" row, opened ───────────────────────────────────────────

/** One line per footswitch: its name and what its assignments do. */
export function footswitchLines(
  hints: readonly DoctorLevelingDamageHint[],
): { name: string; does: string[] }[] {
  const by = new Map<number, { name: string; does: string[] }>();
  for (const h of hints) {
    let line = by.get(h.switch);
    if (!line) {
      line = {
        name:
          h.label.trim() !== ""
            ? h.label
            : `Footswitch ${String(h.switch + 1)}`,
        does: [],
      };
      by.set(h.switch, line);
    }
    const text = FOOTSWITCH_KIND_TEXT[h.kind];
    if (!line.does.includes(text)) line.does.push(text);
  }
  return [...by.entries()].sort((a, b) => a[0] - b[0]).map(([, l]) => l);
}

// ── "To sound like …" ────────────────────────────────────────────────────────

/** An EQ gain as the player sets it: "−3", "+2.5". */
function gainText(db: number): string {
  const abs = Math.abs(db);
  return `${db < 0 ? "−" : "+"}${Number.isInteger(abs) ? String(abs) : abs.toFixed(1)}`;
}

/** "a", "a and b", "a, b and c". */
function listWords(words: string[]): string {
  if (words.length <= 1) return words.join("");
  return `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`;
}

/** What to change so `sound` sounds like `reference`: the EQ step (on the chain's own
 *  10-band EQ, a new one, or — with no CPU room for one — an EQ already there) and, when
 *  an EQ can't close the gap, the line that says so. Null when the two already match or
 *  can't be compared (different band layouts). */
export function matchAdvice(
  sound: DoctorSoundResult,
  reference: DoctorSoundResult,
  nodes: readonly GraphNode[],
): { step: string | null; residual: string | null } | null {
  if (!bandLayoutsMatch(reference, sound)) return null;
  const deltas = matchDeltas(
    reference.balanceDb,
    sound.balanceDb,
    sound.bandLabels,
  );
  const moves = eqMovesFor(deltas, sound.bandLabels);
  const residualLarge = matchResidualLarge(deltas);
  if (moves.length === 0 && !residualLarge) return null;
  const set = listWords(
    moves.map((m) => `${eqBandLabel(m.controlId)} ${gainText(m.gainDb)} dB`),
  );
  let step: string | null = null;
  if (moves.length > 0) {
    if (existingEq10(nodes) != null) {
      step = `On your 10-band EQ, set ${set}.`;
    } else if (
      !overCpuBudget(chargedTotal([...nodes.map((n) => n.model), EQ10_STEREO]))
    ) {
      step = `Add a 10-band EQ after the cab and set ${set}.`;
    } else {
      step = `There isn't enough CPU left for a 10-band EQ. To get closer, set ${set} on an EQ you already have.`;
    }
  }
  return { step, residual: residualLarge ? RESULTS_TEXT.residual : null };
}

/** The cut-through line, or null with no factory comparison (bass). */
export function cutThroughLine(sound: DoctorSoundResult): string | null {
  const p = sound.cutThrough?.factoryPercentile;
  if (p == null) return null;
  return `More presence than ${String(Math.round(p))}% of the factory presets.`;
}

/** True when any fix on any of this sound's findings touches a block OUTSIDE the
 *  footswitch's own toggled set — a shared block whose change affects every sound of
 *  the preset. Only possible for a footswitch sound (`ownNodeIds != null`). */
export function touchesSharedBlock(
  sound: DoctorSoundResult,
  ownNodeIds: readonly string[] | undefined,
): boolean {
  if (ownNodeIds == null) return false;
  return sound.diags.some((d) =>
    d.rx.some((rx) =>
      rx.ops.some((op) => {
        const n = op.kind === "param" ? op.nodeId : op.fenderId;
        return !ownNodeIds.includes(n);
      }),
    ),
  );
}

/** The node ids a footswitch SOUND owns — the blocks its own switch toggles, found in the
 *  FULL footswitch roster the sound was diagnosed with. Base and scene sounds own nothing
 *  (undefined). Matched on the device switch number, never on a list position: the sound
 *  key's index counts the levelable-only list, not the full roster. */
export function ownNodeIdsFor(
  sound: DoctorSoundResult,
  roster: readonly FootswitchInfo[] | undefined,
): string[] | undefined {
  if (sound.footswitch == null) return undefined;
  return roster
    ?.find((f) => f.switch === sound.footswitch)
    ?.functions.map((f) => f.node_id);
}
