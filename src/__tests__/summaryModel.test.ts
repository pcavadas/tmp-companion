// src/__tests__/summaryModel.test.ts — the leveling Summary's words and its one action,
// pure (look B, `views/level/summaryModel.ts`).
//
// Each sound lands in "Need you", "Not reached" or "Leveled" with plain sentences picked
// by TYPED fields (outcome, clampKind, skipKind, persistMismatch, silenceHint,
// verifyByEar), never by the backend's text. "Try again (N)" counts the sounds another run
// can fix: a sound that fails the same way every time (a full-volume or effect-floor clamp,
// a routing problem, a skip kind that says so) gets its message and stays out.
// A main sound tried again takes its preset's leveled sounds with it.
// A sound whose save didn't hold needs you and is tried again.

import { describe, expect, it } from "vitest";

import type { RunItem } from "../views/level/leveling";
import {
  buildSummary,
  SUMMARY_TEXT,
  type SummaryContext,
  type SummaryModel,
} from "../views/level/summaryModel";
import { base, fsRow, sceneRow } from "./summaryTestUtils";

const ctx = (over: Partial<SummaryContext> = {}): SummaryContext => ({
  stopped: false,
  reampStillOn: false,
  targetLufsByName: (name) => (name === "Lead" ? -19 : -23),
  sceneNamesOf: () => [],
  enablingScenesOf: () => null,
  sceneOverrides: {},
  hookKeyOf: (it) => it.key,
  ...over,
});

const rowsOf = (m: SummaryModel, label: string) =>
  m.sections.find((s) => s.label === label)?.rows ?? [];
const only = (m: SummaryModel) => {
  const rows = m.sections.flatMap((s) => s.rows);
  expect(rows).toHaveLength(1);
  return rows[0];
};

describe("titles", () => {
  it("all leveled: says so, and that everything is saved", () => {
    const m = buildSummary(
      [base({ key: "p3" }), base({ key: "p4", slot: 4, presetName: "Clean" })],
      ctx(),
    );
    expect(m.title).toBe("All 2 sounds leveled.");
    expect(m.subtitle).toBe("Everything is saved on your Tone Master Pro.");
    expect(m.retryKeys).toEqual([]);
    const one = buildSummary([base({})], ctx());
    expect(one.title).toBe("1 sound leveled.");
  });

  it("mixed: leveled and need you", () => {
    const m = buildSummary(
      [base({}), sceneRow(0, { outcome: "unconverged" })],
      ctx(),
    );
    expect(m.title).toBe("1 sound leveled. 1 needs you.");
    expect(m.subtitle).toBeNull();
  });

  it("nothing leveled", () => {
    const m = buildSummary(
      [
        base({ outcome: "skipped", skipKind: "preset_unreadable" }),
        sceneRow(0, { outcome: "skipped", skipKind: "preset_unreadable" }),
      ],
      ctx(),
    );
    expect(m.title).toBe("Nothing was leveled. 2 sounds need you.");
  });

  it("stopped: counts the sounds not reached, and says who needs you under it", () => {
    const items: RunItem[] = [
      base({}),
      sceneRow(0, { outcome: "unconverged" }),
      sceneRow(1, { outcome: undefined, status: "queued" }),
      sceneRow(2, { outcome: undefined, status: "queued" }),
    ];
    const m = buildSummary(items, ctx({ stopped: true }));
    expect(m.title).toBe("Stopped. 1 sound leveled, 2 not reached.");
    expect(m.subtitle).toBe("1 needs you.");
    const none = buildSummary(
      [base({ outcome: "skipped", skipKind: "cancelled" })],
      ctx({ stopped: true }),
    );
    expect(none.title).toBe("Stopped. Nothing was leveled, 1 not reached.");
  });

  it("not reached counts as needing you when the last run was not stopped", () => {
    const m = buildSummary(
      [base({}), sceneRow(0, { outcome: undefined, status: "queued" })],
      ctx(),
    );
    expect(m.title).toBe("1 sound leveled. 1 needs you.");
    expect(rowsOf(m, "Not reached")).toHaveLength(1);
  });
});

describe("one sentence per outcome, by typed fields", () => {
  it("leveled: On target, no sentence", () => {
    const r = only(buildSummary([base({})], ctx()));
    expect(r).toMatchObject({
      mark: "done",
      word: "On target",
      messages: [],
      badge: null,
      sound: "Main sound",
    });
  });

  // The two miss states differ in the next action: an unconverged row still had knob
  // room (Try again helps), a clamped one is at the end of its knob (it can't).
  it("off target and not confirmed are tried again; full volume is not", () => {
    const off = buildSummary([fsRow({ outcome: "unconverged" })], ctx());
    expect(only(off)).toMatchObject({
      mark: "caution",
      messages: [SUMMARY_TEXT.offTarget],
      word: null,
    });
    expect(off.retryKeys).toEqual(["f3:0"]);

    const unconfirmed = buildSummary(
      [base({ outcome: "unconverged", unverified: true })],
      ctx(),
    );
    expect(only(unconfirmed).messages).toEqual([SUMMARY_TEXT.notConfirmed]);
    expect(unconfirmed.retryKeys).toEqual(["p3"]);

    const full = buildSummary(
      [base({ outcome: "clamped", value: -24, clampKind: "scene_ceiling" })],
      ctx(),
    );
    expect(only(full)).toMatchObject({
      mark: "danger",
      messages: [
        "Already at full volume, so it can't reach Lead. Choose a quieter target for it.",
      ],
    });
    expect(full.retryKeys).toEqual([]);
  });

  it("tells the clamp causes apart by clamp_kind and the side of the target", () => {
    const floor = buildSummary(
      [fsRow({ outcome: "clamped", clampKind: "wet_floor", value: -17 })],
      ctx(),
    );
    expect(only(floor).messages).toEqual([
      "This effect can't go any quieter without losing it, so it stays louder than Lead.",
    ]);
    const bottom = buildSummary(
      [base({ outcome: "clamped", clampKind: "scene_ceiling", value: -15 })],
      ctx(),
    );
    expect(only(bottom).messages).toEqual([
      "Already as quiet as this knob goes, so it stays louder than Lead. Choose a louder target for it.",
    ]);
    expect(bottom.retryKeys).toEqual([]);
    // A trade backed out saved nothing: another run can help.
    const failed = buildSummary(
      [base({ outcome: "clamped", clampKind: "partial_trade", value: -24 })],
      ctx(),
    );
    expect(only(failed).messages).toEqual([SUMMARY_TEXT.notLeveled]);
    expect(failed.retryKeys).toEqual(["p3"]);
  });

  it("can't hear it: the cause from the scan's silence hint, never tried again", () => {
    for (const [hint, text] of [
      [undefined, SUMMARY_TEXT.routing],
      ["amp_zero", SUMMARY_TEXT.ampZero],
      ["exp_mute", SUMMARY_TEXT.expMute],
    ] as const) {
      const m = buildSummary(
        [base({ outcome: "offbranch", silenceHint: hint })],
        ctx(),
      );
      expect(only(m)).toMatchObject({ mark: "danger", messages: [text] });
      expect(m.retryKeys).toEqual([]);
    }
  });

  it("a skip another run can fix says what it can by its kind, and counts in Try again", () => {
    const say = (kind: NonNullable<RunItem["skipKind"]>) =>
      buildSummary([base({ outcome: "skipped", skipKind: kind })], ctx());
    expect(only(say("preset_unreadable")).messages).toEqual([
      SUMMARY_TEXT.presetUnreadable,
    ]);
    expect(only(say("footswitches_unreadable")).messages).toEqual([
      SUMMARY_TEXT.footswitchesUnreadable,
    ]);
    for (const kind of ["no_signal", "connection_lost", "other"] as const) {
      expect(only(say(kind)).messages).toEqual([SUMMARY_TEXT.notLeveled]);
      expect(say(kind).retryKeys).toEqual(["p3"]);
    }
    for (const kind of ["no_stimulus", "clipped_capture"] as const) {
      expect(only(say(kind)).messages).toEqual([SUMMARY_TEXT.notLeveled]);
      expect(say(kind).retryKeys).toEqual(["p3"]);
    }
  });

  // A skip that happens the same way every time says what to change, each kind its own
  // sentence, and never counts in Try again.
  it("a skip that recurs says what to change, by its kind, and leaves Try again", () => {
    const say = (kind: NonNullable<RunItem["skipKind"]>) =>
      buildSummary(
        [sceneRow(0, { outcome: "skipped", skipKind: kind })],
        ctx(),
      );
    const cases = [
      [
        "no_active_amp",
        "No amp is on in this sound, so its volume wasn't changed. Choose a block and knob for it when you level it again.",
      ],
      [
        "level_manually",
        "Leveling can't choose a knob for this sound's amps on its own, so its volume wasn't changed. Choose a block and knob for it when you level it again.",
      ],
      [
        "scene_write_refused",
        "This scene shares its knob with the main sound, so its volume wasn't changed. Turn on Scene Edit for that block on your Tone Master Pro, or choose another knob for it when you level it again.",
      ],
      [
        "unsupported_knob",
        "That knob isn't a volume or mix knob, so its volume wasn't changed. Choose a volume or mix knob for it when you level it again.",
      ],
      [
        "knob_missing",
        "That knob isn't in the preset any more, so its volume wasn't changed. Choose another knob for it when you level it again.",
      ],
      [
        "block_off",
        "That block is off in this sound, so its volume wasn't changed. Choose another block for it when you level it again.",
      ],
      [
        // The unit's setting is read when it connects, so the fix includes a replug.
        "scene_change_discards",
        "Your Tone Master Pro is set to throw away scene changes, so leveling can't save them. On your Tone Master Pro, set Scene Change Behavior to Maintain changes. Then unplug it, plug it back in, and level it again.",
      ],
      [
        "no_target",
        "Its target isn't in Settings any more, so its volume wasn't changed. Choose a target for it when you level it again.",
      ],
    ] as const;
    for (const [kind, text] of cases) {
      const m = say(kind);
      expect(only(m)).toMatchObject({ mark: "danger", messages: [text] });
      expect(m.retryKeys).toEqual([]);
    }
    // Every sentence is its own.
    expect(new Set(cases.map(([, text]) => text)).size).toBe(cases.length);
  });

  it("the backend's own reason never reaches the screen", () => {
    const m = buildSummary(
      [
        base({
          outcome: "skipped",
          skipKind: "other",
          skipReason: "cannot hold endpoint at unity",
        }),
      ],
      ctx(),
    );
    expect(JSON.stringify(m)).not.toContain("endpoint");
  });

  it("a footswitch says which one it is", () => {
    const r = only(buildSummary([fsRow({})], ctx()));
    expect(r.sound).toBe("Boost, footswitch 1");
  });
});

// The save after a run (a Stop's too) is checked; a sound whose save didn't
// hold needs you, whatever it measured, and Try again covers it.
describe("a save that didn't hold", () => {
  it("needs you and is tried again", () => {
    const m = buildSummary([base({ persistMismatch: true })], ctx());
    expect(only(m)).toMatchObject({
      mark: "caution",
      messages: ["Leveled, but the save didn't hold. Try again."],
      word: null,
      badge: null,
    });
    expect(m.title).toBe("Nothing was leveled. 1 sound needs you.");
    expect(m.retryKeys).toEqual(["p3"]);
  });
});

describe("badges and shared messages", () => {
  it("one sound with loud peaks: the badge and its sentence", () => {
    const r = only(buildSummary([base({ truePeakDbtp: -0.4 })], ctx()));
    expect(r.badge).toEqual({ text: "Loud peaks", tone: "caution" });
    expect(r.messages).toEqual([SUMMARY_TEXT.loudPeaks]);
    expect(r.word).toBe("On target");
  });

  it("a peak safely under the threshold, or a sound with no prediction, says nothing", () => {
    expect(only(buildSummary([base({ truePeakDbtp: -3 })], ctx())).badge).toBe(
      null,
    );
    expect(
      only(buildSummary([sceneRow(0, { truePeakDbtp: null })], ctx())).badge,
    ).toBe(null);
  });

  it("two or more with loud peaks share one Notice; the rows keep the badge", () => {
    const m = buildSummary(
      [
        base({ truePeakDbtp: -0.4 }),
        base({ key: "p4", slot: 4, presetName: "Clean", truePeakDbtp: -0.2 }),
      ],
      ctx(),
    );
    expect(m.notice).toEqual({
      tone: "caution",
      title: "Loud peaks.",
      text: SUMMARY_TEXT.loudPeaksMany,
    });
    for (const r of rowsOf(m, "Leveled")) {
      expect(r.badge?.text).toBe("Loud peaks");
      expect(r.messages).toEqual([]);
    }
  });

  it("check by ear: a neutral badge and the cause's sentence (wet floor on a leveled row)", () => {
    const r = only(
      buildSummary(
        [fsRow({ outcome: "done", verifyByEar: "wet_floor" })],
        ctx(),
      ),
    );
    expect(r.badge).toEqual({ text: "Check by ear", tone: "neutral" });
    expect(r.messages).toEqual([SUMMARY_TEXT.byEar.wet_floor]);
  });

  it("two with the same check-by-ear cause share an info Notice", () => {
    const m = buildSummary(
      [
        base({ verifyByEar: "envelope" }),
        base({
          key: "p4",
          slot: 4,
          presetName: "Clean",
          verifyByEar: "envelope",
        }),
      ],
      ctx(),
    );
    expect(m.notice).toEqual({
      tone: "info",
      title: null,
      text: SUMMARY_TEXT.byEarMany.envelope,
    });
  });

  it("loud peaks wins the one badge; both sentences stay", () => {
    const r = only(
      buildSummary(
        [base({ truePeakDbtp: -0.4, verifyByEar: "rebalance" })],
        ctx(),
      ),
    );
    expect(r.badge?.text).toBe("Loud peaks");
    expect(r.messages).toEqual([
      SUMMARY_TEXT.loudPeaks,
      SUMMARY_TEXT.byEar.rebalance,
    ]);
  });

  // The typed re-amp flag, never the text, and it wins the one Notice.
  it("re-amp may still be on: one caution Notice telling them to restart the unit", () => {
    const m = buildSummary(
      [
        base({ reampStillOn: true, truePeakDbtp: -0.4 }),
        base({ key: "p4", slot: 4, presetName: "Clean", truePeakDbtp: -0.2 }),
      ],
      ctx({ reampStillOn: true }),
    );
    expect(m.notice).toEqual({
      tone: "caution",
      title: null,
      text: "Your guitar input may still be muted. Restart your Tone Master Pro.",
    });
    // The loud-peaks message stays on the rows, since it lost the Notice.
    expect(rowsOf(m, "Leveled")[0].messages).toEqual([SUMMARY_TEXT.loudPeaks]);
    const text = buildSummary(
      [
        base({
          reampWarning: "Re-amp may still be on. Restart your Tone Master Pro.",
        }),
      ],
      ctx(),
    );
    expect(text.notice).toBeNull();
  });

  it("the restart Notice follows the flow's latest reading, not a row an earlier run marked", () => {
    // The main sound's run left re-amp on; a later Try again of the scene read it back
    // off, so the flow's reading is off while the main sound's row keeps its flag.
    const m = buildSummary(
      [base({ reampStillOn: true }), sceneRow(0, {})],
      ctx({ reampStillOn: false }),
    );
    expect(m.notice).toBeNull();
  });

  it("the base boost: words only, and an e2e hook says whether it was applied", () => {
    const applied = only(
      buildSummary(
        [
          base({
            baseBoost: { applied: true, preset_level: 1, base_amps: [] },
          }),
        ],
        ctx(),
      ),
    );
    expect(applied.messages).toEqual([
      "Turned the preset up as far as it goes, then raised the amp's output to reach Lead.",
    ]);
    expect(applied.baseBoost).toBe("applied");
    const planned = only(
      buildSummary(
        [
          base({
            outcome: "clamped",
            value: -24,
            baseBoost: { applied: false, preset_level: 1, base_amps: [] },
          }),
        ],
        ctx(),
      ),
    );
    expect(planned.baseBoost).toBe("planned");
    expect(planned.messages).toEqual([SUMMARY_TEXT.fullVolume("Lead")]);
  });
});

describe("one row for a preset whose sounds all need you for one reason", () => {
  it("shares a row, 'All N sounds'", () => {
    const m = buildSummary(
      [
        base({ outcome: "skipped", skipKind: "footswitches_unreadable" }),
        sceneRow(0, {
          outcome: "skipped",
          skipKind: "footswitches_unreadable",
        }),
        sceneRow(1, {
          outcome: "skipped",
          skipKind: "footswitches_unreadable",
        }),
      ],
      ctx(),
    );
    const r = only(m);
    expect(r).toMatchObject({
      sound: "All 3 sounds",
      messages: [SUMMARY_TEXT.footswitchesUnreadable],
      keys: ["p3", "s3:0", "s3:1"],
    });
    expect(m.retryKeys).toEqual(["p3", "s3:0", "s3:1"]);
  });

  it("keeps one row per sound when the reasons differ", () => {
    const m = buildSummary(
      [base({ outcome: "unconverged" }), sceneRow(0, { outcome: "offbranch" })],
      ctx(),
    );
    expect(rowsOf(m, "Need you")).toHaveLength(2);
  });
});

describe("Try again takes a main sound's leveled sounds with it", () => {
  it("a main sound tried again takes its preset's leveled sounds, counted in N, in run order", () => {
    const m = buildSummary(
      [
        base({ outcome: "unconverged" }),
        sceneRow(0, {}),
        sceneRow(1, { outcome: "clamped", value: -24 }),
        fsRow({}),
        base({ key: "p4", slot: 4, presetName: "Clean" }),
      ],
      ctx(),
    );
    // The full-volume scene stays out; the other preset's main sound too.
    expect(m.retryKeys).toEqual(["p3", "s3:0", "f3:0"]);
    // Nothing else in the Summary changes: the leveled sounds stay leveled.
    expect(rowsOf(m, "Leveled").map((r) => r.key)).toEqual([
      "s3:0",
      "f3:0",
      "p4",
    ]);
  });

  it("a scene tried again leaves its preset's other sounds alone", () => {
    const m = buildSummary(
      [base({}), sceneRow(0, { outcome: "unconverged" }), sceneRow(1, {})],
      ctx(),
    );
    expect(m.retryKeys).toEqual(["s3:0"]);
  });

  it("covers the sounds a Stop didn't reach", () => {
    const m = buildSummary(
      [
        base({}),
        sceneRow(0, { outcome: "skipped", skipKind: "cancelled" }),
        sceneRow(1, { outcome: undefined, status: "queued" }),
      ],
      ctx({ stopped: true }),
    );
    expect(m.retryKeys).toEqual(["s3:0", "s3:1"]);
    expect(rowsOf(m, "Not reached").map((r) => r.word)).toEqual([
      "Not reached",
      "Not reached",
    ]);
  });
});

describe("a footswitch whose effect is off in its scene", () => {
  const offIn1 = (over: Partial<SummaryContext> = {}) =>
    ctx({
      sceneNamesOf: () => ["Clean", "Crunch", "Lead"],
      enablingScenesOf: () => [2],
      ...over,
    });
  const fsInCrunch = fsRow({
    outcome: "skipped",
    skipKind: "block_off",
    footswitch: {
      switchIndex: 2,
      levGroupId: "G1",
      levNodeId: "boost",
      levParameterId: "level",
      sceneContext: 1,
    },
  });

  it("says so, offers the scene choice, and is not tried again until the scene changes", () => {
    const m = buildSummary([fsInCrunch], offIn1());
    const r = only(m);
    expect(r.mark).toBe("caution");
    expect(r.messages).toEqual([
      "Footswitch 3 doesn't turn its effect on in Crunch, so there was nothing to level. Choose another scene for it.",
    ]);
    expect(r.scene).toMatchObject({
      label: "Scene to level footswitch 3 of Guitar in",
      value: "1",
      used: "1",
    });
    expect(r.scene?.options.map((o) => o.label)).toEqual([
      "Main sound",
      "Clean",
      "Crunch",
      "Lead",
    ]);
    expect(m.retryKeys).toEqual([]);

    const changed = buildSummary(
      [fsInCrunch],
      offIn1({ sceneOverrides: { "f3:0": 2 } }),
    );
    expect(only(changed).scene?.value).toBe("2");
    expect(changed.retryKeys).toEqual(["f3:0"]);
  });

  it("only once the unit's answer is known, and never for a leveled switch", () => {
    expect(
      only(buildSummary([fsInCrunch], ctx({ sceneNamesOf: () => ["A", "B"] })))
        .scene,
    ).toBeNull();
    expect(
      only(
        buildSummary(
          [{ ...fsInCrunch, outcome: "done", skipKind: undefined }],
          offIn1(),
        ),
      ).scene,
    ).toBeNull();
  });
});
