import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  openLevel,
  reampCounters,
  reampOff,
  simEvents,
} from "../fixtures/scenario";

// PR #144 NEW COVERAGE — the D3 scene-context picker (`FsRowControls` in SetupPage.tsx)
// drives `FootswitchLevelJob.sceneContext` on the wire, and a USER OVERRIDE of the
// auto-suggested scene must actually ride the call — not just the suggestion happening to
// be right. E2E Combined Level (406)'s BOOST switch (`ftsw[2]`, `ACD_KingOfTone`, G4) is
// enabled by exactly one scene (`LEAD`, `scenes[2]`), so `list_footswitch_scene_contexts`
// suggests it by default (`FsSceneContext.suggested`). This spec overrides that suggestion
// to SCRATCH (`scenes[0]`, a scene BOOST is NOT enabled in — the picker allows this,
// flagged, never blocked: `FsRowControls`'s own doc) and proves the override — not the
// suggestion — is what the backend actually recalls before capturing.
//
// Danger.md's latch nuance is the physics this pins: `loadScene` mid-engage is INAUDIBLE (the
// active scene latches at ENGAGE), so the scene recall MUST happen before `ReAmp(true)`, not
// merely "at some point in the run" — a `LoadScene` fired AFTER the engage would be a bug that
// captures the wrong scene's audio while reporting the requested one.
//
// Sanctioned observation: the run's own per-row outcome is unobservable offline (the
// Channel-streaming seam), but the WIRE WRITES the run makes are real regardless — `/sim/
// events`' ordering is this file's proof, exactly as `.claude/rules/e2e.md` prescribes.
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
    await page
      .getByTitle(/Show Base/)
      .first()
      .click();
    // BOOST alone — never Base, never a scene row — so the run's event slice belongs to
    // this one footswitch only. Scoped to the list's OWN scroll body
    // (`[data-preset-list]`, PresetList.tsx): the app's persistent signal-chain strip
    // chrome renders its own compact "BOOST" chip for the device's currently-loaded
    // preset regardless of what the Level tab's filtered list shows, so a bare
    // page-wide text match collides with it.
    const list = page.locator("[data-preset-list]");
    await list.getByText("BOOST", { exact: true }).click();
    await filter.fill("");

    // The OPENING trigger counts distinct PRESETS represented in the selection (one, 406,
    // regardless of how many of its child rows are ticked) — the RUNNING button below is
    // what counts actual leveling rows ("sound").
    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // `f406:sw2` (setupRowHookKey, leveling.ts): BOOST is `ftsw[2]`.
    const boostRow = page.locator('[data-setup-row="f406:sw2"]');
    const ctxTrigger = boostRow.locator('[data-pick="fsctx:2"]');
    await ctxTrigger.click(); // fires the lazy list_footswitch_scene_contexts fetch
    // The option list PORTALS to the wizard card, detached from the row's own DOM — the
    // `data-pick-option` hook (not row scoping) is what disambiguates it.
    const scratchOption = page.locator('[data-pick-option="fsctx:2:0"]');
    await expect(
      scratchOption,
      "SCRATCH (scenes[0]) must be offered even though BOOST isn't enabled there",
    ).toHaveCount(1);
    await scratchOption.click();
    await expect(
      ctxTrigger,
      "the trigger must reflect the override before the run starts",
    ).toContainText("SCRATCH");
    // The non-enabling warning must follow the override (BOOST's own `ftswStates[2]` is
    // false in SCRATCH — confirmed against the fixture JSON directly, not re-derived here).
    await expect(
      boostRow.getByText(/doesn.t turn on in that scene/),
    ).toBeVisible();

    // Baseline AFTER every Setup-time interaction (the lazy scene-context fetch above reads
    // the preset and can itself emit load-class events) — the slice below is the RUN's own.
    const from = (await simEvents(page)).length;

    await page.getByRole("button", { name: /Start.*1 sound/ }).click();
    // Per this file's header, the per-row OUTCOME is not observable offline (the Channel
    // seam) — wait for the terminal state without asserting what it says.
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 240_000 });

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
