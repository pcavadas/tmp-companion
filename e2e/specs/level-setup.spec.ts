import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  LEVEL_T,
  levelButton,
  openLevel,
  openPreset,
  reampCounters,
  reampOff,
  selectBaseOnly,
  simEvents,
  soundBox,
} from "../fixtures/scenario";

// New P4-B (rebuilt-fixture) coverage that lives at the SETUP step (before any run) or is
// provable via a raw command invoke plus /sim/events, rather than through a post-run
// Summary render. A raw invoke carries no Channel, so its streamed rows go nowhere; the
// page does receive them (.claude/rules/e2e.md's "Events and Channels reach the page").
//
// Fixture map (e2e/fixtures/COVERAGE.md): SCENARIO[0] "E2E Rig" (400) carries the Other-
// class wah (WAH, sw8), the unlabeled raw-dB Boost (sw2), the wet-mix SPRING (sw3), and the
// isolated/shared_with_base/lowers_only scene-overlay spread across its 4 scenes.
//
// PR #144 REWORK: the verify-only footswitch default + its "Make level-neutral" opt-in,
// and the scene row's match/offset target-mode chip, are BOTH GONE — every row (main
// sound/scene/footswitch) levels against ONE leveling handle (D2). A main sound defaults to
// the preset's own level, a scene to the amp(s) its run drives; footswitch rows always
// carry a real pre-seeded handle (no default of their own).
//
// LOOK B: each Set up row shows that handle as TWO dropdowns, Block then Knob (`Select`,
// e2e hooks `data-select="block:<rowKey>"` / `"knob:<rowKey>"`, each option
// `data-select-option="<tid>:<value>"` — a Block option's value is `groupId:nodeId`, a
// Knob option's the `parameterId`, the default's `default`). Picking a block lands on its
// best knob. A knob the scene can't use is offered disabled with its reason; a block whose
// every knob is disabled is offered disabled with that reason.

/** A batched leveling command's return: the rows, with the run-end re-amp verdict beside them. */
interface BatchLevelRun<R> {
  rows: R[];
  reampStillOn: boolean;
}

// openLevel now lives in ../fixtures/scenario.ts (shared with level-defaults.spec.ts).

/** A Set up row's dropdown: `kind` is "target" | "block" | "knob" | "scene", `key` the
 *  row's `data-setup-row` hook (`setupRowHookKey`, leveling.ts). */
function trigger(page: Page, kind: string, key: string) {
  return page.locator(`[data-select="${kind}:${key}"]`);
}
/** One option of an open dropdown. The list portals to <body>, detached from the row's
 *  own DOM — the `data-select-option` hook (not row scoping) is what names it. */
function option(page: Page, tid: string, value: string) {
  return page.locator(`[data-select-option="${tid}:${value}"]`);
}
/** Close an open list the way a user does: a click anywhere outside it (the page title is
 *  never under a list). */
async function closeList(page: Page): Promise<void> {
  await page.getByRole("heading", { name: /^Level \d+ sounds?$/ }).click();
}

test.describe("Level Setup — Other-class filtering, unlabeled naming (list-level)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE rows 22, 19 — unlabeled switch rendering, plus the UI manifestation of the
  // Other-class case.
  //
  // BUG→GATE (user-reported, 2026-08-19, "Friedman HBE"): a footswitch with no level-class
  // parameter used to be filtered out of the Level tab ENTIRELY — the user's "Phaser" switch
  // simply was not there, with nothing saying why. Hiding a control the player can see on the
  // unit is the bug, not the fix: the roster must show every block-acting switch, and a switch
  // that cannot be leveled must SAY SO instead of vanishing.
  //
  // So the tree now renders the FULL roster (`usePresetData`'s `footswitchRoster: "all"`,
  // which `LevelView` opts into) while SELECTABILITY still comes from the one shared
  // `footswitchLevelable` predicate — a non-levelable row is present, labelled "no level
  // control", and its checkbox is disabled. That keeps the danger.md Pick trap closed from the
  // other side: the row can never be picked, so it can never fall back to `options[0]`.
  // The Doctor's own list is deliberately NOT changed (it stays levelable-only — a
  // non-levelable switch has no sound of its own to diagnose); the separation is pinned by
  // `src/__tests__/footswitch-roster-separation.test.tsx`, and the count-vs-buildable
  // agreement by `src/__tests__/footswitch-no-level-control.test.ts`.
  test("400: WAH (Other-class) is SHOWN but not levelable; the unlabeled Boost switch names itself from its block", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: the fixture's levelable-set shape",
    );
    await ensureScenario(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name); // E2E Rig
    await openPreset(page, SCENARIO[0].name);

    // The collapsed breakdown counts the WHOLE roster — all 6 block-acting switches, not just
    // the levelable ones. Only DRIVE and the unlabeled Boost stay levelable: WAH and WAH SWEEP
    // both act on the all-Other-class ACD_CryBabyQ535 and carry zero level candidates, and
    // SPRING and VERB KILL both act on ACD_TMSpring63, whose only classified param is `mix`
    // (WetMix) — issue 3 ("hide Mix everywhere") drops WetMix from the candidate gate
    // regardless of the acting switch's own `func` (SPRING is "on-off", VERB KILL is "param";
    // both go through the SAME per-node `level_candidates_for_node` enumeration in
    // `enumerate_block_footswitches`, so both lose their sole candidate together). All four
    // non-levelable rows are counted here precisely because the user must see that they exist.
    await expect(page.getByText("4 scenes, 6 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    // The unlabeled switch (customLabel: "") falls back to its block's own short name
    // ("Boost", from ACD_Boost) — never a blank row, never an arbitrary options[0].
    await expect(page.getByText("Boost", { exact: true })).toBeVisible();

    // WAH IS present — the user-reported bug was that it was not.
    await expect(page.getByText("WAH", { exact: true })).toBeVisible();
    // WAH/WAH SWEEP (Other-class) and SPRING/VERB KILL (WetMix-only, issue 3) all say WHY
    // they cannot be leveled rather than disappearing. Counted, so a regression that drops
    // one row is caught too.
    await expect(page.getByText("No volume knob to level")).toHaveCount(4);
    // …and none of the four can be selected: no pick, so no `options[0]` fallback is
    // reachable (danger.md's Pick trap, closed from the absence side as before).
    await expect(page.getByRole("checkbox", { disabled: true })).toHaveCount(4);
  });
});

test.describe("Level Setup — scene handle picker (isolated / shared_with_base / lowers_only)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE rows 10, 12, 13 — all Setup-time (the picker's candidate annotations), never a
  // run. Row 9 (the old target-mode chip's offset mode) is GONE with the chip itself — PR
  // #144 replaced it with the combined D2 handle picker, so there is no more "match target
  // vs keep offset" choice to assert. 400's 4 scenes carry all three overlay scopes for the
  // ACD_Boost handle: Rhythm/Lead/Ceiling are FULL overlays (isolated), Shared is
  // bypass-only (shared_with_base); Ceiling's amp `outputLevel` overlay sits at 1.0 (the
  // range top) — the lowers_only headroom case.
  test("400: Rhythm is isolated, Shared warns shared_with_base, Ceiling annotates lowers_only; picking a handle updates the trigger", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: the scene overlays are fixture-authored",
    );
    await ensureScenario(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name);
    await openPreset(page, SCENARIO[0].name);
    for (const scene of ["Rhythm", "Ceiling", "Shared"]) {
      await page.getByText(scene, { exact: true }).click();
    }
    await filter.fill("");

    await levelButton(page, 3).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Rows by their own `data-setup-row` hook (`setupRowHookKey`, leveling.ts):
    // `s400:0` = Rhythm, `s400:2` = Ceiling, `s400:3` = Shared. A scene hook's index is the
    // `scenes[]` array order, which IS the wire sceneSlot already (see
    // e2e/fixtures/scenario-presets.json's slot-400 `scenes` list) — a true identity.

    // Rhythm (s400:0): ACD_Boost's OWN overlay in this scene is FULL (isolated) — its block
    // is offered ENABLED with no shared note. (TubeScreamer's and the Twin's overlays are
    // bypass-only here, so THEIR knobs legitimately are shared; the assertion is scoped to
    // Boost's own option.) Merely opening and reading the list picks nothing.
    const rhythmBlock = trigger(page, "block", "s400:0");
    const rhythmBefore = await rhythmBlock.textContent();
    await rhythmBlock.click();
    const boostInRhythm = option(page, "block:s400:0", "G1:ACD_Boost");
    // EXISTENCE FIRST. The cleanliness assertions below are absence-only and would pass
    // for an option that never rendered.
    await expect(
      boostInRhythm,
      "the Boost block must be in the list at all",
    ).toHaveCount(1);
    await expect(boostInRhythm).not.toHaveAttribute("aria-disabled", "true");
    await expect(
      boostInRhythm.getByText("Shared with the main sound"),
    ).toHaveCount(0);
    await closeList(page);
    await expect(rhythmBlock).toHaveText(rhythmBefore ?? "");

    // Shared (s400:3): ACD_Boost's overlay is bypass-only in this scene → its one knob is
    // shared with the main sound, so its block is offered DISABLED, saying why.
    await trigger(page, "block", "s400:3").click();
    const boostInShared = option(page, "block:s400:3", "G1:ACD_Boost");
    await expect(boostInShared).toHaveAttribute("aria-disabled", "true");
    await expect(boostInShared).toContainText("Shared with the main sound");
    await closeList(page);

    // Ceiling (s400:2): BOTH amps' outputLevel overlays sit at the range top (1.0) in this
    // scene (scenario-presets.json's slot-400 scene 2) — the lowers_only headroom case. The
    // default (the amp the run drives in Ceiling) says so on its own Output level…
    await trigger(page, "knob", "s400:2").click();
    await expect(option(page, "knob:s400:2", "default")).toContainText(
      "Can only turn down",
    );
    await closeList(page);

    // …and PICKING the Twin's block lands on its best knob, its own Output level, which
    // says it too. The pick shows in both dropdowns at once.
    await trigger(page, "block", "s400:2").click();
    await option(page, "block:s400:2", "G1:ACD_TwinReverb65NoFx").click();
    await expect(trigger(page, "knob", "s400:2")).toHaveText("Output level");
    await trigger(page, "knob", "s400:2").click();
    await expect(option(page, "knob:s400:2", "outputLevel")).toContainText(
      "Can only turn down",
    );
    await closeList(page);
  });

  // Two BUG→GATEs on one row, 402 "E2E Edge" scene 3 "Solo" (P4-C): `ACD_Boost` is bypassed
  // in BASE, un-bypassed ONLY by Solo's bypass-only overlay, targeted by no footswitch or
  // EXP assign, and bypassed (or overlay-absent, inheriting base) in every OTHER scene — see
  // `scene_jobs::shared_write_is_scene_local` and `fixture_gates.rs`'s
  // `fx_edge_keeps_the_eq_ring_and_eight_scenes`.
  //  * PRESELECT (issue 5, "Boost preselect", 2026-08-29): every scene row used to default
  //    its handle to the amp's output level even when the scene's whole reason to exist is
  //    turning ON a base-bypassed block, so leveling the amp barely moved the sound Solo
  //    adds. `scene_handle_rows_scanned` now stamps `enables_block: true` on Boost's
  //    candidate for Solo (wire `enablesBlock`) and Set up's preselect (`useSetupRows`)
  //    seeds THAT handle — never overriding a user's own choice.
  //  * OFFER (user-reported "Friedman HBE" preset 28, 2026-08-22): the picker used to
  //    disable a bypass-only handle UNCONDITIONALLY as "shared with the base preset", even
  //    when the leak-to-base write is provably audible in exactly ONE scene. The fix
  //    (`scene_write_verdict_for_param`'s `WriteDirect{lands_on_base:true}` arm) must offer
  //    the handle ENABLED here — the write-lands-on-base + run-completes half of this gate
  //    is the raw-invoke test below ("402 Solo: leveling Boost's gain...").
  // The preselect is read FIRST, before either dropdown is opened, so the row shows exactly
  // what the preselect seeded and not a value this test picked.
  test("402 Solo: the scene row preselects the Boost handle, and offers it ENABLED (isolated), not shared_with_base", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: the scene overlays are fixture-authored",
    );
    await ensureScenario(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[2].name); // E2E Edge
    await openPreset(page, SCENARIO[2].name);
    await page.getByText("Solo", { exact: true }).click();
    await filter.fill("");

    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // s402:3 = Solo (0-based scenes[] index == wire sceneSlot, same identity as 400's rows).
    // PRESELECT, untouched: the Boost's block, named as the unit names it, and its best
    // knob, `gain` ("Gain") — not the amp's Output level.
    await expect(trigger(page, "block", "s402:3")).toHaveText("Boost");
    await expect(trigger(page, "knob", "s402:3")).toHaveText("Gain");

    // OFFER.
    await trigger(page, "block", "s402:3").click();
    const boostOption = option(page, "block:s402:3", "G1:ACD_Boost");
    // EXISTENCE FIRST — see the Rhythm/Shared test above for why.
    await expect(
      boostOption,
      "the Boost block must be in the list at all",
    ).toHaveCount(1);
    await expect(
      boostOption.getByText("Shared with the main sound"),
      "the Solo write is provably scene-local — no shared note",
    ).toHaveCount(0);
    // ENABLED, not just unwarned: clicking it must actually pick it (never blocked — the
    // DANGER-rule Pick trap forbids a click that silently no-ops on a disabled option).
    await boostOption.click();
    await expect(trigger(page, "block", "s402:3")).toHaveText("Boost");
    await expect(trigger(page, "knob", "s402:3")).toHaveText("Gain");
  });
});

test.describe("Level Setup — instant candidates from the backup scan (no device read)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // `useLevelBlocks`/`useSceneHandles` (src/views/level) are INSTANT-FIRST: a slot the
  // startup backup scan already covers resolves its Base/Scene candidates straight off
  // `getLibraryScan()` — no `list_level_blocks`/`list_scene_level_handles` device round
  // trip — and Set up's own eager warm effect (`useSetupRows`) fires that fetch the moment
  // the page renders (gated on `hasBackupData`, so it provably cannot reach the device).
  // By the time the user opens a row's BLOCK dropdown, the candidates must already be
  // `resolved`: no "Reading knobs…" text, ever.
  //
  // Non-vacuous: absence of "Reading knobs…" alone could pass even if a device read
  // was simply FAST rather than absent, so this also asserts the offline SimDevice's
  // own wire-event log (`/sim/events`) is UNCHANGED by opening the dropdown — a real
  // `list_level_blocks` fallback (the "backup scan has no entry" branch) issues a real
  // simulated session (heartbeats, a load) that DOES append events, so a silent
  // regression back to the device path would fail this half even if the UI still
  // "felt" instant.
  test("400 base: the block dropdown is populated immediately, no loading state, no device read", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the backup-scan-derived (no-device-read) path",
    );
    await ensureScenario(page);
    await openLevel(page);

    // Baseline taken BEFORE Set-up even renders — SetupPage's own eager warm effect fires
    // the moment Set-up renders (gated on `hasBackupData`), so the window this baseline
    // opens must span that warm too, not just the later dropdown open: a regression back
    // to the device path would otherwise append its events INSIDE the baseline and the
    // delta asserted below would stay 0 even though a real round trip happened.
    const eventsBefore = await simEvents(page);

    await selectBaseOnly(page, SCENARIO[0].name); // E2E Rig

    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // p400 = the base row (`baseKey`, leveling.ts).
    const key = "p400";

    await trigger(page, "block", key).click();
    // No wait, no retry loop — the instant path resolves synchronously off the scan, so
    // asserting immediately is the point: a fallback-to-device regression would still be
    // `"loading"` right after the click. `toHaveCount(0)` auto-retries and would still pass
    // even if the text flashed and then vanished, so read the count once, right after the
    // click, instead.
    expect(await page.getByText("Reading knobs…").count()).toBe(0);
    // A block of the preset's own is listed beside the "Preset" default.
    await expect(
      page.locator(`[data-select-option^="block:${key}:G1:"]`).first(),
    ).toBeVisible();

    const eventsAfter = await simEvents(page);
    expect(
      eventsAfter.length,
      `opening Set-up and the block dropdown must not touch the device: before=${JSON.stringify(
        eventsBefore,
      )} after=${JSON.stringify(eventsAfter)}`,
    ).toBe(eventsBefore.length);
  });
});

test.describe("Level Setup — footswitch rows pre-seed a real handle (verify-only removed)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE rows 15, 16/17/20's Setup-time half, post-PR-#144: the backend dropped the
  // verify-only footswitch mode entirely, so there is no more "Verify only" tag or
  // "Make level-neutral" opt-in to flip — every row is PRE-SEEDED with the tone-safe
  // `defaultParamIndex` candidate (leveling.ts's `chosenFrom`) at Setup-open time, shown
  // non-interactively when it's the row's only option (mirrors BlockLevelPick's own
  // doc: "a `wet_mix` candidate is flagged...", "the single best candidate...").
  // REWRITTEN (was: "SPRING pre-seeds Mix") — issue 3 ("Mix is never a leveling control",
  // 2026-08-29) removed WetMix from `is_levelable_param`, so SPRING's sole candidate
  // (`ACD_TMSpring63.mix`) no longer exists as a level control at all. SPRING can no longer
  // be pre-seeded with anything — it joins the BUG-1 "no level control" pattern instead
  // (disabled row, click is a no-op, never reaches Setup). This rewrite IS the issue-3 gate
  // for the footswitch pre-seed lane; Boost's own pre-seed (unaffected — `gain` is `level_db`,
  // never WetMix) stays as the positive control proving the describe block's premise still
  // holds for a switch that DOES carry a real level control.
  test("400: Boost pre-seeds Gain; SPRING has no level control now that Mix is hidden everywhere", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: fixture-authored footswitch shape",
    );
    await ensureScenario(page);
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name);
    await openPreset(page, SCENARIO[0].name);

    // SPRING at the LIST level: labelled "No volume knob to level" and its checkbox is
    // disabled (mirrors the WAH/WAH SWEEP pattern in "Level Setup — Other-class
    // filtering..." above, but reached by a DIFFERENT predicate — WetMix-only, not
    // Other-class). Scoped to SPRING's own row rather than the page-wide count that test
    // already covers, so this failure names SPRING specifically. The row IS the checkbox
    // (`SceneRow` renders one role="checkbox" button, "Choose SPRING in E2E Rig").
    const springRow = soundBox(page, "SPRING", SCENARIO[0].name);
    await expect(springRow.getByText("No volume knob to level")).toBeVisible();
    await expect(springRow).toBeDisabled();
    // Clicking the row is a no-op (a natively `disabled` button) — the danger-rule Pick
    // trap closed from the absence side: nothing to click through to an `options[0]`
    // fallback because the row can never become selected. `force`: Playwright's
    // actionability check would otherwise wait on the disabled row forever — the click
    // must still be SENT so this proves it changes nothing.
    await springRow.click({ force: true });
    await expect(springRow).not.toBeChecked();

    await soundBox(page, "Boost", SCENARIO[0].name).click();
    await filter.fill("");

    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Footswitch hooks are keyed by DEVICE SWITCH NUMBER (`setupRowHookKey`,
    // leveling.ts): `f400:sw2` = Boost (COVERAGE.md row 20). Footswitch rows always carry a
    // real pre-seeded handle (D2: no default of their own) — its block, and the pre-picked
    // KNOB ("Gain").
    const boostRow = page.locator('[data-setup-row="f400:sw2"]');
    await expect(boostRow.getByText("Verify only")).toHaveCount(0);
    await expect(trigger(page, "block", "f400:sw2")).toHaveText("Boost");
    await expect(trigger(page, "knob", "f400:sw2")).toHaveText("Gain");
    // SPRING (`f400:sw3`) never reached Setup — the click above never selected it, so there
    // is no row for it to render.
    await expect(page.locator('[data-setup-row="f400:sw3"]')).toHaveCount(0);
  });
});

test.describe("Level — shared_write_is_scene_local (Boost/Solo, raw invoke, BUG→GATE)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  interface SceneLevelResult {
    scene_slot: number | null;
    clamped: boolean;
    clamp_kind: string | null;
    saved: boolean;
    final_level: number;
    persist_mismatch: boolean | null;
  }
  interface SceneEditEvent {
    SceneEdit: { group: string; node: string; enable: boolean };
  }
  function isSceneEdit(e: unknown): e is SceneEditEvent {
    return typeof e === "object" && e !== null && "SceneEdit" in e;
  }
  interface SceneHandleCandidate {
    nodeId: string;
    parameterId: string;
    current: number;
    scope: string;
  }
  interface SceneHandleRow {
    sceneSlot: number;
    allCandidates: SceneHandleCandidate[];
  }

  // The Setup-time picker half (`level-setup.spec.ts`'s "402 Solo: the scene row
  // preselects the Boost handle, and offers it ENABLED...") proves the OFFER; this half proves the RUN: a leveling run over
  // Solo completes (never errors, never silently skips), and the write lands on the
  // SHARED BASE value — never in an overlay — matching `scene_write_verdict_for_param`'s
  // `WriteDirect{lands_on_base:true}` contract. 402 declares no `scenario-loudness.json`
  // `leveledParams` curve for `ACD_Boost.gain` (COVERAGE row 3's same honest-scope
  // caveat for 400's Boost), so the offline model is FLAT in it and the run clamps (a
  // reported clamp, not an error — the exact taxonomy verdict is incidental to this
  // gate) — the assertion is about the WRITE PATH and its landing, not loudness tracking.
  //
  // "Landed on base, not an overlay" is proven three ways, the last one a POSITIVE
  // CONTROL that rules out "SimDevice just never emits a SceneEdit event" trivially
  // passing the other two:
  //   1. no `SceneEdit{enable:true}` event for (G1, ACD_Boost) anywhere in the run — the
  //      write-landing policy's `WriteDirect` arm never enables Scene Edit;
  //   2. re-querying `list_scene_level_handles` afterward shows scene 0 "Verse" (which
  //      carries NO overlay for ACD_Boost — `scenario-presets.json`'s slot-402 scene 0)
  //      now reads the SOLVED value for `gain` — only possible if the write actually
  //      changed the SHARED base value, since Verse's own reading is always base's;
  //   3. the positive control: leveling `ACD_JC120.outputLevel` in the SAME scene 3 (its
  //      overlay there is Absent — `fx_edge_keeps_the_eq_ring_and_eight_scenes` pins
  //      scenes 1/3/5/6/7 ABSENT for the amp) DOES emit `SceneEdit{enable:true}` for
  //      (G1, ACD_JC120) — proving the sim records such events at all, so (1)'s absence
  //      for Boost is a real signal, not a vacuous one.
  test("402 Solo: leveling Boost's gain completes and lands on the shared base value, not an overlay", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the fixture's authored (flat-response) shape",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);
    const slot = SCENARIO[2].slot; // 402, E2E Edge

    const before = (await invoke(
      page,
      "list_scene_level_handles",
      { slot },
      LEVEL_T,
    )) as SceneHandleRow[];
    const boostBefore = before
      .find((r) => r.sceneSlot === 0)
      ?.allCandidates.find(
        (c) => c.nodeId === "ACD_Boost" && c.parameterId === "gain",
      );
    if (!boostBefore) {
      throw new Error(
        `Verse (scene 0) must offer a Boost/gain candidate: ${JSON.stringify(before)}`,
      );
    }

    // Hardcoded -20 (the fixture's flat-response model gives every target the same
    // reachable ceiling here, per this test's own header caveat) — only one solve is
    // needed since a dry run and a saving run against the same target would just repeat
    // the same clamp; go straight to the save:true run this gate actually needs.
    const boostJob = (save: boolean) =>
      invoke(
        page,
        "level_scenes_apply_batched",
        {
          slot,
          jobs: [
            {
              sceneSlot: 3,
              targetLufs: -20,
              handle: {
                groupId: "G1",
                nodeId: "ACD_Boost",
                parameterId: "gain",
              },
            },
          ],
          candidates: [],
          save,
          rebalance: false,
          topologyId: "guitar-humbucker",
          calibrationLufs: null,
          profileId: null,
          onResult: "__CHANNEL__:1",
        },
        LEVEL_T * 2,
      ).then((r) => (r as BatchLevelRun<SceneLevelResult>).rows);

    const from = (await simEvents(page)).length;
    const results = await boostJob(true);
    // Under the OLD (over-widened-away) Refuse policy, `level_scenes_apply_batched`
    // filters a failed outcome out of the array it returns rather than erroring, so
    // indexing straight into `results[0]` would throw a bare TypeError instead of naming
    // the regression. Guard the shape first.
    expect(
      results,
      "the Solo job must produce a row, not a filtered-out refusal",
    ).toHaveLength(1);
    const real = results[0];
    expect(real.scene_slot, "the row names its own scene").toBe(3);
    // A run always WRITES (`.claude/rules/leveling-dsp.md`) — a clamp is a REPORTED
    // outcome, never a silent skip. The flat-response fixture honestly clamps; the exact
    // taxonomy verdict is incidental to this gate (offline the handle may be a
    // no-authority case), so pin only that the run clamped.
    expect(
      real.clamped,
      `the flat-response fixture must honestly clamp: ${JSON.stringify(real)}`,
    ).toBe(true);
    // The post-save check reads the value where it LANDED (base), not scene 3's
    // bypass-only overlay — graded there it reported a persisted write as lost.
    expect(
      real.persist_mismatch,
      `a persisted shared-base write must not be reported as lost: ${JSON.stringify(real)}`,
    ).toBe(false);

    // 1. No Scene Edit enable for the node this write landed on.
    const events = (await simEvents(page)).slice(from);
    expect(
      events.some(
        (e) =>
          isSceneEdit(e) &&
          e.SceneEdit.group === "G1" &&
          e.SceneEdit.node === "ACD_Boost" &&
          e.SceneEdit.enable,
      ),
      `a scene-local base write must never enable Scene Edit: ${JSON.stringify(events)}`,
    ).toBe(false);

    // 2. The shared base value actually moved, visible from a scene that carries NO
    // overlay for this node (Verse) — a genuinely scene-local (overlay) write would
    // leave Verse's own reading at the ORIGINAL 2.5, never the solved value.
    const after = (await invoke(
      page,
      "list_scene_level_handles",
      { slot },
      LEVEL_T,
    )) as SceneHandleRow[];
    const boostAfter = after
      .find((r) => r.sceneSlot === 0)
      ?.allCandidates.find(
        (c) => c.nodeId === "ACD_Boost" && c.parameterId === "gain",
      );
    if (!boostAfter) {
      throw new Error(
        `Verse (scene 0) must still offer a Boost/gain candidate: ${JSON.stringify(after)}`,
      );
    }
    expect(
      Math.abs(boostAfter.current - real.final_level),
      `Verse's reading (no overlay) must track the shared base value the run wrote: ${JSON.stringify(
        { boostBefore, boostAfter, real },
      )}`,
    ).toBeLessThan(1e-3);
    expect(
      Math.abs(boostAfter.current - boostBefore.current),
      "the shared value must actually have MOVED (not a no-op run)",
    ).toBeGreaterThan(0.1);

    // 3. Positive control: JC120's outputLevel in the SAME scene has an ABSENT overlay
    // (not bypass-only) — a scene-context write there REQUIRES the Scene Edit enable
    // (`SceneWriteVerdict::NeedsEnable`), so the sim DOES emit the event this test's
    // assertion 1 above relies on being absent for Boost.
    const from2 = (await simEvents(page)).length;
    await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot,
        jobs: [
          {
            sceneSlot: 3,
            targetLufs: -20,
            handle: {
              groupId: "G1",
              nodeId: "ACD_JC120",
              parameterId: "outputLevel",
            },
          },
        ],
        candidates: [],
        save: false,
        rebalance: false,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:1",
      },
      LEVEL_T * 2,
    );
    const events2 = (await simEvents(page)).slice(from2);
    expect(
      events2.some(
        (e) =>
          isSceneEdit(e) &&
          e.SceneEdit.group === "G1" &&
          e.SceneEdit.node === "ACD_JC120" &&
          e.SceneEdit.enable,
      ),
      `an Absent-overlay scene write DOES enable Scene Edit — the sim must record it, \
       or assertion 1 above is vacuous: ${JSON.stringify(events2)}`,
    ).toBe(true);

    await expectReampBalanced(page, reampBase);
  });
});
