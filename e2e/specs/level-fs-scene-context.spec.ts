import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  levelButton,
  openLevel,
  openPreset,
  reampCounters,
  reampOff,
  simEvents,
  soundBox,
  startLeveling,
  summaryDone,
} from "../fixtures/scenario";

// PR #144 NEW COVERAGE — the D3 scene context (Set up's "Level it in scene" dropdown, a
// footswitch row's `scene:<rowKey>` Select) drives `FootswitchLevelJob.sceneContext` on the
// wire, and a USER OVERRIDE of the auto-suggested scene must actually ride the call — not
// just the suggestion happening to be right. E2E Combined Level (406)'s BOOST switch
// (`ftsw[2]`, `ACD_KingOfTone`, G4) is enabled by exactly one scene (`LEAD`, `scenes[2]`),
// so `list_footswitch_scene_contexts` suggests it by default (`FsSceneContext.suggested`).
// This spec overrides that suggestion to SCRATCH (`scenes[0]`, a scene BOOST is NOT enabled
// in — allowed, flagged under the row, never blocked) and proves the override — not the
// suggestion — is what the backend actually recalls before capturing.
//
// Danger.md's latch nuance is the physics this pins: `loadScene` mid-engage is INAUDIBLE (the
// active scene latches at ENGAGE), so the scene recall MUST happen before `ReAmp(true)`, not
// merely "at some point in the run" — a `LoadScene` fired AFTER the engage would be a bug that
// captures the wrong scene's audio while reporting the requested one.
//
// Observation: the WIRE WRITES the run makes are this file's proof — `/sim/events`'
// ordering, as `.claude/rules/e2e.md` prescribes — not the row the run streams back.
// COVERAGE row 39 — the scene-that-enables-an-FS third of 406's combined-leveling trio
// (BOOST/LEAD), driven through the real wizard UI with the D3 override riding the wire.
test.describe("Level Setup — footswitch scene-context override rides the wire (406, UI-driven)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  interface LoadSceneEvent {
    LoadScene: number;
  }
  interface ReAmpEvent {
    ReAmp: boolean;
  }
  function isLoadScene(e: unknown): e is LoadSceneEvent {
    return typeof e === "object" && e !== null && "LoadScene" in e;
  }
  function isReAmpOn(e: unknown): e is ReAmpEvent {
    return (
      typeof e === "object" &&
      e !== null &&
      "ReAmp" in e &&
      (e as ReAmpEvent).ReAmp
    );
  }

  test("overriding BOOST's suggested LEAD to SCRATCH recalls scene 0, not scene 2, before the engage", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the fixture's authored scene-enable shape",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const combined = SCENARIO[6]; // 406, E2E Combined Level
    expect(combined.name).toBe("E2E Combined Level");
    await openLevel(page);

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(combined.name);
    await openPreset(page, combined.name);
    // BOOST alone — never Base, never a scene row — so the run's event slice belongs to
    // this one footswitch only. Addressed by its sub-row's own accessible name ("Choose
    // BOOST in E2E Combined Level"): the app's persistent signal-chain strip chrome
    // renders its own compact "BOOST" chip for the device's currently-loaded preset
    // regardless of what the Level tab's filtered list shows, so a bare page-wide text
    // match would collide with it.
    await soundBox(page, "BOOST", combined.name).click();
    await filter.fill("");

    // The list's main action counts the chosen SOUNDS: this one footswitch.
    await levelButton(page, 1).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // `f406:sw2` (setupRowHookKey, leveling.ts): BOOST is `ftsw[2]`.
    const boostRow = page.locator('[data-setup-row="f406:sw2"]');
    const ctxTrigger = boostRow.locator('[data-select="scene:f406:sw2"]');
    // The suggestion is resolved as Set up opens (off the backup scan), never on a click.
    await expect(
      ctxTrigger,
      "the dropdown shows the suggested scene before anything is clicked",
    ).toContainText("LEAD");
    await ctxTrigger.click();
    // The list portals to <body>, detached from the row's own DOM — the
    // `data-select-option` hook (not row scoping) is what disambiguates it.
    const scratchOption = page.locator(
      '[data-select-option="scene:f406:sw2:0"]',
    );
    await expect(
      scratchOption,
      "SCRATCH (scenes[0]) must be offered even though BOOST isn't enabled there",
    ).toHaveCount(1);
    await scratchOption.click();
    await expect(
      ctxTrigger,
      "the dropdown must reflect the override before the run starts",
    ).toContainText("SCRATCH");
    // The non-enabling note must follow the override (BOOST's own `ftswStates[2]` is
    // false in SCRATCH — confirmed against the fixture JSON directly, not re-derived here).
    await expect(
      boostRow.getByText(
        "Footswitch 3 doesn't turn its effect on in SCRATCH. Choose another scene for it.",
      ),
    ).toBeVisible();

    // Baseline AFTER every Setup-time interaction (a scene-context fetch that falls back
    // to the device reads the preset and can emit load-class events) — the slice below is
    // the RUN's own.
    const from = (await simEvents(page)).length;

    await startLeveling(page).click();
    // Per this file's header, the per-row OUTCOME is not observable offline (the Channel
    // seam) — wait for the terminal state without asserting what it says.
    await expect(summaryDone(page)).toBeVisible({ timeout: 240_000 });

    const events = (await simEvents(page)).slice(from);
    const reAmpIdx = events.findIndex(isReAmpOn);
    const scratchIdx = events.findIndex(
      (e) => isLoadScene(e) && e.LoadScene === 0,
    );
    expect(
      scratchIdx,
      `the override's own scene must be recalled at all: ${JSON.stringify(events)}`,
    ).toBeGreaterThanOrEqual(0);
    expect(
      reAmpIdx,
      `the run must actually engage: ${JSON.stringify(events)}`,
    ).toBeGreaterThanOrEqual(0);
    expect(
      scratchIdx,
      "SCRATCH must be recalled BEFORE the engage (re-amp latches the active scene at " +
        "engage — danger.md)",
    ).toBeLessThan(reAmpIdx);

    // BOOST's block (`ACD_KingOfTone`) is bypassed in base and LEAD is the one scene that
    // enables it, so under the assign gate (user directive, 2026-08-19) BOOST — a bare
    // on-off with no `param` fn of its own — plans `Bake`, not `Assign`. A Bake's own save
    // mirrors the solved value into every scene whose overlay RESTATES base
    // (`footswitch.rs`'s `mirror_scenes`), and LEAD does exactly that here — so a LATE
    // `LoadScene(2)`, once every measurement capture is already complete, is the Bake
    // persisting correctly, not a leak. What the override actually promises is narrower and
    // UNCHANGED: LEAD must never be the scene ACTIVE AT AN ENGAGE (danger.md: re-amp
    // latches the active scene at engage) — the override, not the suggestion, must be what
    // every CAPTURE hears. Scoping the check to everything up to and including the LAST
    // engage (there is one per measurement probe, all at SCRATCH) keeps that promise exactly
    // as strong as it was, without failing on the Bake's own after-the-fact mirror write.
    const engageIndices = events
      .map((e, i) => (isReAmpOn(e) ? i : -1))
      .filter((i) => i >= 0);
    const lastEngageIdx = engageIndices[engageIndices.length - 1] ?? -1;
    const scenesThroughLastEngage = events
      .slice(0, lastEngageIdx + 1)
      .filter(isLoadScene)
      .map((e) => e.LoadScene);
    expect(
      scenesThroughLastEngage,
      "the SUGGESTED scene (LEAD, scenes[2]) must never be ACTIVE AT AN ENGAGE — the " +
        "override, not the suggestion, must be what every capture hears: " +
        JSON.stringify(events),
    ).not.toContain(2);

    await expectReampBalanced(page, reampBase);
  });
});
