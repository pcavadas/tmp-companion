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
  openLevel,
  reampCounters,
  reampOff,
  selectBaseOnly,
  simEvents,
} from "../fixtures/scenario";

// New P4-B (rebuilt-fixture) coverage that lives at the SETUP step (before any run) or is
// provable via a raw command invoke, rather than through a post-run Summary render — the
// per-scene/per-footswitch Channel-streaming seam is not UI-observable offline; see
// .claude/rules/e2e.md's "The Channel-streaming seam" for the deliberate-seam rationale
// and the sanctioned raw-invoke + /sim/events observation path this file uses instead.
//
// Fixture map (e2e/fixtures/COVERAGE.md): SCENARIO[0] "E2E Rig" (400) carries the Other-
// class wah (WAH, sw8), the unlabeled raw-dB Boost (sw2), the wet-mix SPRING (sw3), and the
// isolated/shared_with_base/lowers_only scene-overlay spread across its 4 scenes.
//
// PR #144 REWORK: the verify-only footswitch default + its "Make level-neutral" opt-in,
// and the scene row's match/offset target-mode chip, are BOTH GONE — every row (Base/Scene/
// Footswitch) now levels against ONE user-chosen `BlockLevelPick` handle (D2). Base rows
// default to the "Preset level" pseudo-option, Scene rows to "Amp output level"; footswitch
// rows always carry a real pre-seeded handle (no pseudo-option). The picker's own two
// triggers (`title="Choose this sound's leveling block"` then `title="Choose this
// sound's leveling parameter"` — a later split of what was originally one flat
// block+param dropdown) replaced the old target-mode chip trigger.

interface FootswitchLevelResult {
  switch: number;
  clamped: boolean;
  unconverged: boolean;
  clamp_reason: string | null;
  wet_floor: boolean;
  /** The clamp's cause from the shared taxonomy (mirrors `headroom_trade::ClampKind`) —
   *  see `src/lib/types.ts`'s `ClampKind`/`CLAMP_MESSAGES`. Null when not clamped. */
  clamp_kind: string | null;
  saved: boolean;
  final_value: number;
  predicted_lufs: number;
  method: string; // "baked" | "assigned"
}

interface SetFootswitchAssignmentEvent {
  SetFootswitchAssignment: {
    addr: number;
    index: number;
    function_json: string;
    swap: boolean;
  };
}
function isSetFootswitchAssignment(
  e: unknown,
): e is SetFootswitchAssignmentEvent {
  return typeof e === "object" && e !== null && "SetFootswitchAssignment" in e;
}

/** `changeParameter`(12) — a Bake's own wire shape (`sim_device::SimEvent::ChangeParameter`,
 *  no `rename_all` on the enum, so the JSON keys are the Rust field names verbatim). This is
 *  where a Bake's write lands: straight on the block, never through `ftsw` — the twin of
 *  `SetFootswitchAssignmentEvent` above for the OTHER plan branch. */
interface ChangeParameterEvent {
  ChangeParameter: {
    scene: number;
    group: string;
    node: string;
    param: string;
    value: number;
  };
}
function isChangeParameter(e: unknown): e is ChangeParameterEvent {
  return typeof e === "object" && e !== null && "ChangeParameter" in e;
}

// openLevel now lives in ../fixtures/scenario.ts (shared with level-defaults.spec.ts).

/** Dismiss any currently-open Pick/FsParamPick/SceneLevelPick dropdown by clicking its
 *  own backdrop directly (`data-pick-backdrop`, PickPortalMenu.tsx) — a full-card
 *  `inset:0` div with no text content. Clicking a visible-text landmark instead does NOT
 *  work even though the backdrop visually covers it and would receive the click in
 *  effect: Playwright's own actionability check resolves the text locator to the element
 *  BENEATH the backdrop and then refuses to click through the thing covering it, retrying
 *  "<div></div> intercepts pointer events" for the full timeout instead of ever landing
 *  the click. Targeting the backdrop's own element sidesteps the check entirely. Needed
 *  between rows: a still-open menu's backdrop sits above every other row's own trigger and
 *  would otherwise swallow the next click. */
async function closeAnyOpenPicker(page: Page): Promise<void> {
  await page.locator("[data-pick-backdrop]").click();
}

/** `BlockLevelPick`'s two triggers (D2/Part C's two-dropdown split), scoped by a setup
 *  row's `data-setup-row` hook (`setupRowHookKey`, leveling.ts). The BLOCK trigger opens
 *  first (`data-block-pick` rows, one per block); picking a block auto-selects its
 *  best-ranked ENABLED candidate and reveals the CONTROL trigger (`data-block-param-pick`
 *  rows, that one block's own params only) — hidden entirely until a block is picked. */
function blockTrigger(page: Page, key: string) {
  return page.locator(
    `[data-setup-row="${key}"] div[title="Choose this sound's leveling block"]`,
  );
}
function controlTrigger(page: Page, key: string) {
  return page.locator(
    `[data-setup-row="${key}"] div[title="Choose this sound's leveling parameter"]`,
  );
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
    await page
      .getByTitle(/Show Base/)
      .first()
      .click();

    // The collapsed breakdown counts the WHOLE roster — all 6 block-acting switches, not the
    // 4 levelable ones (DRIVE, the unlabeled Boost, SPRING, VERB KILL). WAH and WAH SWEEP both
    // act on the all-Other-class ACD_CryBabyQ535 and carry zero level candidates, and they are
    // counted here precisely because the user must see that they exist.
    await expect(page.getByText("4 scenes · 6 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    // The unlabeled switch (customLabel: "") falls back to its block's own short name
    // ("Boost", from ACD_Boost) — never a blank row, never an arbitrary options[0].
    await expect(page.getByText("Boost", { exact: true })).toBeVisible();

    // WAH IS present — the user-reported bug was that it was not.
    await expect(page.getByText("WAH", { exact: true })).toBeVisible();
    // Both Other-class switches (WAH sw8, WAH SWEEP) say WHY they cannot be leveled rather
    // than disappearing. Counted, so a regression that drops one row is caught too.
    await expect(page.getByText("no level control")).toHaveCount(2);
    // …and neither can be selected: no pick, so no `options[0]` fallback is reachable
    // (danger.md's Pick trap, closed from the absence side as before).
    await expect(page.getByRole("checkbox", { disabled: true })).toHaveCount(2);
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
    await page
      .getByTitle(/Show Base/)
      .first()
      .click();
    for (const scene of ["Rhythm", "Ceiling", "Shared"]) {
      await page.getByText(scene, { exact: true }).click();
    }
    await filter.fill("");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Scoped by the row's OWN `data-setup-row` hook (`setupRowHookKey`, leveling.ts):
    // `s400:0` = Rhythm, `s400:2` = Ceiling, `s400:3` = Shared. A scene hook's index is
    // the `scenes[]` array order, which IS the wire sceneSlot already (`chosenFrom`'s
    // "the row index IS the 0-based wire sceneSlot" — see e2e/fixtures/scenario-
    // presets.json's slot-400 `scenes` list) — a true identity, stable under a
    // fixture edit, so it needs no translation the way a footswitch hook does
    // (`f<slot>:sw<n>`, below). `blockTrigger`/`controlTrigger` (this file's top) are
    // `BlockLevelPick`'s two triggers (D2/Part C's two-dropdown split) — every
    // unselected row's BLOCK trigger DEFAULTS to the "Amp output level" pseudo-option,
    // so a text filter on that label would collide across rows too.

    // Rhythm (s400:0): ACD_Boost's OWN overlay in this scene is FULL (isolated) — its
    // BLOCK row must carry no shared_with_base warning and must be selectable. NOTE:
    // the picker's candidate list spans every level/wet-mix node in the graph, not
    // just Boost — TubeScreamer and TwinReverb are ALSO candidates (their own
    // "level"/"outputLevel" params), and Rhythm's overlay for both is bypass-only
    // ({bypass, bypassType} only — see scenario-presets.json's slot-400 scene 0), so
    // THEIR rows legitimately DO warn shared_with_base here. Scope the assertion to
    // Boost's own BLOCK row (`data-block-pick="G1:ACD_Boost"`) rather than the whole
    // menu — ACD_Boost carries exactly ONE numeric candidate (`gain`), so the block
    // row's disabled state is the param's own, with no need to open the control
    // dropdown (which would require picking a block first, mutating the untouched
    // handle this row is asserting).
    await blockTrigger(page, "s400:0").click();
    const boostBlock = page.locator('[data-block-pick="G1:ACD_Boost"]');
    // EXISTENCE FIRST. The warning assertion below is absence-only, and `toHaveCount(0)`
    // is equally satisfied by a Boost row that never rendered — a candidate-enumeration
    // regression, or a `data-block-pick` rename, would turn this into a test that
    // asserts nothing while staying green. Pin the row's presence, then its cleanliness.
    await expect(
      boostBlock,
      "the Boost block row must be in the menu at all",
    ).toHaveCount(1);
    await expect(
      boostBlock.getByText(/shared with the base preset/),
    ).toHaveCount(0);
    // Untouched (still the "Amp output level" pseudo-default — this row's own handle was
    // never picked; the control trigger stays hidden until a block IS picked).
    await expect(blockTrigger(page, "s400:0")).toContainText(
      "Amp output level",
    );
    await expect(controlTrigger(page, "s400:0")).toHaveCount(0);
    await closeAnyOpenPicker(page);

    // Shared (s400:3): ACD_Boost's overlay is bypass-only in this scene → its OWN
    // BLOCK row warns and is disabled. Scoped the same way as Rhythm above —
    // TubeScreamer's overlay is ALSO bypass-only here (scenario-presets.json's
    // slot-400 scene 3: both Boost and TubeScreamer carry only {bypass, bypassType}),
    // so its block row legitimately warns too and a whole-menu text assertion would
    // hit a strict-mode collision.
    await blockTrigger(page, "s400:3").click();
    await expect(
      boostBlock.getByText(/shared with the base preset — changes every scene/),
    ).toBeVisible();
    await closeAnyOpenPicker(page);

    // Ceiling (s400:2): BOTH amps' outputLevel overlay sits at the range top (1.0) in this
    // scene (scenario-presets.json's slot-400 scene 2: ACD_JC120.outputLevel = 1.0 AND
    // ACD_TwinReverb65NoFx.outputLevel = 1.0 — TwinReverb is bypassed here but still
    // carries a full overlay, so its scope stays "isolated" too) — so BOTH their candidate
    // rows legitimately annotate lowers_only and a whole-menu text assertion hits a
    // strict-mode collision. `recommended` (`BlockLevelPick.tsx`) is a SINGLE candidate
    // shared across every block's dropdown, and it resolves to JC120's own `outputLevel`
    // here (empirically: opening JC120's control dropdown shows "Recommended - loudness
    // only", not the bare lowers_only text — `controlRow`'s note precedence puts the
    // Recommended branch before the `lowersOnly` one, so a recommended+lowers_only
    // candidate never renders the bare text at all). Asserting the bare "can only lower"
    // text against JC120 would therefore be VACUOUS — it would pass identically even if
    // `lowersOnly` were wrong, since the Recommended branch alone accounts for the
    // rendered text. Assert it against TwinReverb's own row instead, which is genuinely
    // lowers_only but never the globally-recommended candidate (a different node can
    // never be reference-equal to `recommended`), so this is the one row that can
    // actually FAIL if the lowers_only annotation regresses.
    await blockTrigger(page, "s400:2").click();
    await page.locator('[data-block-pick="G1:ACD_TwinReverb65NoFx"]').click();
    await controlTrigger(page, "s400:2").click();
    const twinCandidate = page.locator(
      '[data-block-param-pick="ACD_TwinReverb65NoFx:outputLevel"]',
    );
    await expect(twinCandidate.getByText("can only lower")).toBeVisible();
    // Selecting the TwinReverb block row DID commit its own best-ranked enabled candidate
    // as this row's handle (the same auto-pick JC120 gets below) — this is not a "view
    // only" click. The overall state stays correct only because the JC120 pick right
    // after this overwrites it.
    await closeAnyOpenPicker(page);

    // PICK the JC120 BLOCK — auto-picks its best-ranked enabled candidate (`outputLevel`,
    // its only level-class candidate) and opens the control trigger. No need to re-open the
    // control dropdown and click the candidate explicitly: the auto-pick already resolved
    // it, so a re-click through the same trigger cannot change the outcome. Boost's `gain`
    // = 2.5, nowhere near its [0,12] top, is not lowers_only here, so it's not a candidate
    // for either assertion.
    await blockTrigger(page, "s400:2").click();
    await page.locator('[data-block-pick="G1:ACD_JC120"]').click();
    await expect(controlTrigger(page, "s400:2")).toContainText("Output level");
  });

  // BUG→GATE (user-reported "Friedman HBE" preset 28, 2026-08-22): the picker used to
  // disable a bypass-only handle UNCONDITIONALLY as "shared with the base preset", even
  // when the leak-to-base write is provably audible in exactly ONE scene. 402 "E2E Edge"
  // carries that exact anatomy (P4-C): `ACD_Boost` is bypassed in BASE, un-bypassed ONLY
  // by scene 3 "Solo"'s bypass-only overlay, targeted by no footswitch or EXP assign, and
  // bypassed (or overlay-absent, inheriting base) in every OTHER scene — see
  // `scene_jobs::shared_write_is_scene_local` and `lib.rs`'s
  // `fx_edge_keeps_the_eq_ring_and_eight_scenes`. The fix
  // (`scene_write_verdict_for_param`'s `WriteDirect{lands_on_base:true}` arm) must offer
  // the handle ENABLED here, not disabled — the write-lands-on-base + run-completes half
  // of this gate is the raw-invoke test below ("402 Solo: leveling Boost's gain...").
  test("402 Solo: the Boost handle is offered ENABLED (isolated), not shared_with_base", async ({
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
    await page
      .getByTitle(/Show Base/)
      .first()
      .click();
    await page.getByText("Solo", { exact: true }).click();
    await filter.fill("");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // s402:3 = Solo (0-based scenes[] index == wire sceneSlot, same identity as 400's rows).
    await blockTrigger(page, "s402:3").click();
    const boostBlock = page.locator('[data-block-pick="G1:ACD_Boost"]');
    // EXISTENCE FIRST — see the Rhythm/Shared test above for why.
    await expect(
      boostBlock,
      "the Boost block row must be in the menu at all",
    ).toHaveCount(1);
    await expect(
      boostBlock.getByText(/shared with the base preset/),
      "the Solo write is provably scene-local — no shared_with_base warning",
    ).toHaveCount(0);
    // ENABLED, not just unwarned: clicking it must actually pick it (never blocked — the
    // DANGER-rule Pick trap forbids a click that silently no-ops on a disabled row).
    await boostBlock.click();
    await expect(controlTrigger(page, "s402:3")).toBeVisible();
    await expect(controlTrigger(page, "s402:3")).toContainText("Gain");
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
  // trip — and `SetupBody`'s own eager warm effect fires that fetch the moment the
  // Set-up step renders (gated on `hasBackupData`, so it provably cannot reach the
  // device). By the time the user opens a row's BLOCK dropdown, the candidates must
  // already be `resolved`: no "Loading controls…" text, ever.
  //
  // Non-vacuous: absence of "Loading controls…" alone could pass even if a device read
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

    // Baseline taken BEFORE Set-up even renders — SetupBody's own eager warm effect fires
    // the moment Set-up renders (gated on `hasBackupData`), so the window this baseline
    // opens must span that warm too, not just the later dropdown open: a regression back
    // to the device path would otherwise append its events INSIDE the baseline and the
    // delta asserted below would stay 0 even though a real round trip happened.
    const eventsBefore = await simEvents(page);

    await selectBaseOnly(page, SCENARIO[0].name); // E2E Rig

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // p400 = the base row (`baseKey`, leveling.ts).
    const key = "p400";

    await blockTrigger(page, key).click();
    // No wait, no retry loop — the instant path resolves synchronously off the scan, so
    // asserting immediately is the point: a fallback-to-device regression would still be
    // `"loading"` right after the click. `toHaveCount(0)` auto-retries and would still pass
    // even if the text flashed and then vanished, so read the count once, right after the
    // click, instead.
    expect(await page.getByText("Loading controls…").count()).toBe(0);
    // Unscoped: the dropdown MENU (`data-block-pick` rows) renders through a portal
    // (`PickPortalMenu`/`usePickAnchor`) detached from the triggering row's own
    // `data-setup-row` subtree — only the trigger itself lives inside that container.
    // Only one picker can be open at a time, so an unscoped locator is unambiguous here.
    await expect(page.locator("[data-block-pick]").first()).toBeVisible();

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
  test("400: Boost pre-seeds Gain, SPRING pre-seeds Mix — no verify-only state exists", async ({
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
    await page
      .getByTitle(/Show Base/)
      .first()
      .click();
    await page.getByText("Boost", { exact: true }).click();
    await page.getByText("SPRING", { exact: true }).click();
    await filter.fill("");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Footswitch hooks are keyed by DEVICE SWITCH NUMBER (`setupRowHookKey`,
    // leveling.ts), not filtered-list position: `f400:sw2` = Boost, `f400:sw3` =
    // SPRING (COVERAGE.md rows 20/18).
    const boostRow = page.locator('[data-setup-row="f400:sw2"]');
    const springRow = page.locator('[data-setup-row="f400:sw3"]');

    await expect(boostRow.getByText("Verify only")).toHaveCount(0);
    await expect(springRow.getByText("Verify only")).toHaveCount(0);
    // Footswitch rows always carry a real pre-seeded handle (D2: no pseudo-option), so
    // BOTH triggers render immediately — the BLOCK trigger names the block itself
    // (its catalog full name, e.g. "Boost"/"Spring Reverb"), and the CONTROL trigger
    // names the pre-picked PARAM ("Gain"/"Mix").
    // Boost's sole candidate is `gain` — pre-picked.
    await expect(controlTrigger(page, "f400:sw2")).toContainText("Gain");
    // SPRING's sole candidate is `mix` — pre-picked.
    await expect(controlTrigger(page, "f400:sw3")).toContainText("Mix");
  });
});

test.describe("Level — footswitch opted-in write path (raw invoke, command-level)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE rows 3, 17, 20's WRITE-PATH half — and row 3 ONLY that half. Stated plainly
  // because the honest scope is narrower than it looks: 400's `scenario-loudness.json`
  // entry declares `leveledParams` for `ACD_TMSpring63.mix` and NOTHING ELSE, so the
  // offline model is FLAT in `ACD_Boost.gain` — `model_lufs` returns the same C at every
  // probe of it. What that makes provable here is the plumbing: the solve terminates, a
  // resolved value reaches the wire through the Bake path, and the save persists it onto
  // the block. What it does NOT prove is that the solved gain TRACKS loudness — any target
  // converges against a flat response, so the dry run's `predicted_lufs` is a constant this
  // test then feeds back to itself. Row 3's solve-tracking half is online-only. Do not read
  // a green here as "the block-knob solve is correct"; read it as "the block-knob WRITE
  // lands and persists".
  //
  // 400/switch 2 (Boost) routes through `FsLevelPlan::Bake` (`footswitch.rs`): the assign
  // gate (user directive, 2026-08-19) plans `Assign` ONLY when the switch already carries a
  // `param` fn on the user-selected control, and Boost's switch is a bare on-off — so
  // leveling `ACD_Boost.gain` writes the block directly rather than adding a function to the
  // switch (a two-entry row is HW-proven to make the firmware silently discard the whole
  // imported preset, `danger.md`). SimDevice implements `changeParameter`(12) as a
  // fire-and-forget setter (no reply, no echo) that lands straight on the block's own
  // `dspUnitParameters` — there is nothing to "confirm by read-back" the way an Assign's
  // `ftsw` write is, so the save simply persists the block value the write already set.
  // Mirrors `e2e_server_tests.rs::bake_path_footswitch_writes_the_block_directly_and_persists_its_value`
  // at the Playwright layer: the dry run learns Boost's reachable engaged loudness (400
  // declares no `leveledParams` for `gain`, so the offline model is flat in it — any signal
  // level converges), the save:true run must complete and persist, and the wire carries the
  // solved value via `changeParameter` — never a `setFootswitchAssignment`(54), which would
  // mean the switch grew a second function. Per this file's header, the RENDERED Summary
  // still can't show this per-row offline — the wire proof is `/sim/events`.
  test("Boost's opted-in gain write reaches the fake via the Bake path and persists on the block", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the sim's Bake write behavior",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const apply = (targetLufs: number, save: boolean) =>
      invoke(
        page,
        "level_footswitches_apply",
        {
          slot: SCENARIO[0].slot,
          jobs: [
            {
              switch: 2,
              levGroupId: "G1",
              levNodeId: "ACD_Boost",
              levParameterId: "gain",
              targetLufs,
            },
          ],
          save,
          topologyId: "guitar-humbucker",
          calibrationLufs: null,
          profileId: null,
          onResult: "__CHANNEL__:1",
        },
        LEVEL_T,
      ) as Promise<FootswitchLevelResult[]>;

    const dry = await apply(-20, false);
    expect(dry[0].clamp_reason, "Boost's engaged capture has signal").toBe(
      null,
    );
    expect(Number.isFinite(dry[0].predicted_lufs)).toBe(true);
    expect(dry[0].saved, "a dry run must write nothing").toBe(false);

    const r = (await apply(dry[0].predicted_lufs, true))[0];
    expect(r.method).toBe("baked");
    expect(r.clamp_reason, "ACD_Boost is on the trunk — no routing clamp").toBe(
      null,
    );
    expect(r.saved, "the Bake save must now complete and persist").toBe(true);
    // A raw-dB gain solve must reach the wire unclamped (the `[0,12]` range's own seed).
    expect(r.final_value).toBeGreaterThan(1);

    const events = await simEvents(page);
    // A Bake must never touch the switch's own `ftsw` row — that shape (a second entry on
    // a row that already has an on-off) is the exact one `danger.md` forbids.
    const assigns = events
      .filter(isSetFootswitchAssignment)
      .map((e) => e.SetFootswitchAssignment);
    expect(
      assigns.some((a) => a.addr === 2),
      `a Bake must never write ftsw: ${JSON.stringify(assigns)}`,
    ).toBe(false);

    const bakes = events
      .filter(isChangeParameter)
      .map((e) => e.ChangeParameter);
    const boost = bakes.find(
      (b) =>
        b.node === "ACD_Boost" &&
        b.param === "gain" &&
        Math.abs(b.value - r.final_value) < 1e-3,
    );
    if (!boost)
      throw new Error(
        `no ChangeParameter write landing the solved gain for ACD_Boost: ${JSON.stringify(bakes)}`,
      );
    expect(boost.group).toBe("G1");

    await expectReampBalanced(page, reampBase);
  });
});

test.describe("Level — wet-mix footswitch outcome (SPRING, raw invoke)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE row 18 — the wet-floor outcome, now offline-provable. It needed two things
  // landed together: `scenario-loudness.json`'s `leveledParams` entry for
  // `400/G1/ACD_TMSpring63/mix` on the `wetMix` curve (so the sim's capture model gives the
  // param real authority instead of reading flat), and `model_lufs`'s widened activation
  // predicate (a Bake's isolation leaves the LEVELED block's own bypass untouched, so the
  // old `bypass_writes[node] == Some(false)` predicate never fired for it). 400's SPRING
  // switch (3) is a bare on-off with no `param` fn of its own, so under the assign gate
  // (user directive, 2026-08-19) it plans `Bake`, not `Assign` — `method` below reads
  // "baked". Mirrors
  // `e2e_server_tests.rs::wet_mix_footswitch_bakes_and_pins_at_the_wet_floor_on_an_unreachable_target`
  // + `..._bakes_and_converges_and_stays_off_the_floor_on_a_reachable_target` at the
  // Playwright layer.
  test("an unreachable target bakes and pins at the wet floor honestly; a reachable one (learned, not hard-coded) bakes, converges, and saves", async ({
    page,
  }) => {
    test.skip(await isOnline(page), "offline: pins the sim's wetMix curve");
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const apply = (targetLufs: number, save: boolean) =>
      invoke(
        page,
        "level_footswitches_apply",
        {
          slot: SCENARIO[0].slot,
          jobs: [
            {
              switch: 3,
              levGroupId: "G1",
              levNodeId: "ACD_TMSpring63",
              levParameterId: "mix",
              targetLufs,
            },
          ],
          save,
          topologyId: "guitar-humbucker",
          calibrationLufs: null,
          profileId: null,
          onResult: "__CHANNEL__:1",
        },
        LEVEL_T,
      ) as Promise<FootswitchLevelResult[]>;

    // Unreachable (-70, far below the curve's -12.02..-17.18 span): the solve pins at
    // WET_FLOOR_FRACTION x the authored mix (0.25 x 0.42 = 0.105) and reports the honest
    // "quieter ON than OFF, verify by ear" outcome — never a routing clamp (`clamp_reason`
    // stays null; that field's contract is "no signal on USB 1/2", which this capture has).
    // save:false — nothing worth persisting at a floor the target itself never asked for.
    const unreachable = (await apply(-70, false))[0];
    expect(unreachable.method).toBe("baked");
    expect(unreachable.clamped, "an unreachable target must clamp").toBe(true);
    expect(
      unreachable.wet_floor,
      `the clamp's cause must be the wet floor: ${JSON.stringify(unreachable)}`,
    ).toBe(true);
    expect(unreachable.clamp_reason).toBe(null);
    // The shared ClampKind taxonomy names the SAME cause (CLAMP_MESSAGES.wet_floor in
    // src/lib/types.ts renders this verbatim wherever a row's clamp is UI-observable —
    // this wire-level check is the twin the Channel seam allows offline; see this file's
    // header).
    expect(
      unreachable.clamp_kind,
      `clamp_kind must name the wet floor too: ${JSON.stringify(unreachable)}`,
    ).toBe("wet_floor");
    expect(
      Math.abs(unreachable.final_value - 0.105),
      `the written value must BE the floor: ${JSON.stringify(unreachable)}`,
    ).toBeLessThan(1e-3);

    // Reachable target: LEARNED from a dry run, never hard-coded. `presetLevel` shifts
    // SPRING's whole curve across a run (scenario-loudness.json's own note on the wet-mix
    // row), so a fixed LUFS picked in advance could clamp for reasons unrelated to what
    // this half proves. -16 is only the SEED for the secant search (the level-rerun.spec.ts
    // idiom): what actually gets applied is that seed's own converged/clamped
    // `predicted_lufs`, so this asks "does converging off the floor work", not "does -16
    // happen to still be reachable this run".
    const probe = (await apply(-16, false))[0];
    const target = probe.clamped ? probe.predicted_lufs : -16;
    const reachable = (await apply(target, true))[0];
    expect(reachable.method).toBe("baked");
    expect(
      reachable.clamped,
      `must actually solve, not clamp: ${JSON.stringify(reachable)}`,
    ).toBe(false);
    expect(reachable.unconverged).toBe(false);
    expect(
      reachable.wet_floor,
      "wet_floor tracks the OUTCOME, not the param's class",
    ).toBe(false);
    expect(
      reachable.clamp_kind,
      "an unclamped row carries no clamp cause",
    ).toBe(null);
    expect(reachable.saved, "an in-range target must persist").toBe(true);

    const events = await simEvents(page);
    // A Bake must never touch the switch's own `ftsw` row — the shape `danger.md` forbids.
    const assigns = events
      .filter(isSetFootswitchAssignment)
      .map((e) => e.SetFootswitchAssignment);
    expect(
      assigns.some((a) => a.addr === 3),
      `a Bake must never write ftsw: ${JSON.stringify(assigns)}`,
    ).toBe(false);

    const bakes = events
      .filter(isChangeParameter)
      .map((e) => e.ChangeParameter);
    const spring = bakes.find(
      (b) =>
        b.node === "ACD_TMSpring63" &&
        b.param === "mix" &&
        Math.abs(b.value - reachable.final_value) < 1e-3,
    );
    if (!spring) {
      throw new Error(
        `SPRING's opted-in mix write must reach the fake: ${JSON.stringify(bakes)}`,
      );
    }
    expect(spring.group).toBe("G1");

    await expectReampBalanced(page, reampBase);
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

  // The Setup-time picker half (`level-setup.spec.ts`'s "402 Solo: the Boost handle is
  // offered ENABLED...") proves the OFFER; this half proves the RUN: a leveling run over
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
      ) as Promise<SceneLevelResult[]>;

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
