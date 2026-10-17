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
  presetBox,
  reampCounters,
  reampOff,
  runBaseLevel,
  simEvents,
  soundBox,
  startLeveling,
  summaryDone,
} from "../fixtures/scenario";

// Level scenarios — run identically offline (fake re-amp) and online (real re-amp).
// SCENARIO[0] "E2E Rig" has BOTH footswitch scenes AND block-acting footswitches (and an
// amp flip). None of the six rebuilt fixtures is Base-only any more (see
// e2e/fixtures/COVERAGE.md) — SCENARIO[1]/[2] ("E2E Pedalboard"/"E2E Edge") now carry
// footswitches/scenes of their own, so the first test below selects each preset's Base
// row EXPLICITLY (never the whole-preset tick) to keep one selected row per preset — a
// whole-preset tick would sweep those in too and change the Summary's outcome for
// reasons unrelated to what this test drives.
// Loudness accuracy is the device's job; these prove the multi-preset, per-preset-target
// flow AND the base+scene+footswitch flow end to end through the real backend.
//
// ONLINE e2e suite consolidation (8→4 files): tests 1 and 3 below now run OFFLINE ONLY
// (test.skip'd online) — both moved into `level.online.spec.ts` (test 1 verbatim; test 3
// alongside that file's own Hiwatt arc, since it already carries slot 404). Test 2 (base +
// scenes + footswitches on E2E Rig) is OFFLINE ONLY too, but for a different reason — trade
// T2: it was never picked up by any *.online.spec.ts file, so its online run is simply
// retired rather than moved (E2E Rig's own online leveling-flow coverage stays real via
// this file's offline tier, which drives the identical UI/backend path against SimDevice).
//
// OFFLINE suite consolidation: test 4 below is level-rerun.spec.ts's remaining offline
// test, merged in verbatim (owner decision: one offline level spec is enough). That file's
// online tests were already retired to `level.online.spec.ts`'s idempotency test — see this
// file's own describe block for the merge; level-rerun.spec.ts is deleted.
// Test 1 ("levels two presets' Base to different targets") was then folded into that last
// test: its first run IS the two-preset, two-target wizard run, so the flow pays one page
// load and one leveling run instead of two.
test.describe("Level — plain presets + a scenes-and-footswitches preset", () => {
  // Between tests: SAFETY only (re-amp off, so an aborted capture can't strand the
  // unit input-muted for the next test). Slot cleanup happens ONCE in afterAll —
  // clearing between tests would force the next test's ensureScenario down the full
  // in-process re-seed (the runner seeds once per spec FILE).
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });

  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  // COVERAGE row 1 — base presetLevel solve (indirect: 400's base solving is a
  // precondition of this run, not asserted in isolation).
  // The mandatory "both scenes and footswitches" case: E2E Rig carries a Base, 4
  // footswitch SCENES (amp outputLevel, incl. an amp flip) AND block-acting FOOTSWITCHES. Ticking
  // the whole preset sweeps in ALL of them, so the run exercises base (level_preset) +
  // scene (level_scenes_apply_batched) + footswitch (level_footswitches_apply, VERIFY
  // mode by default — measures ON/OFF delta, writes nothing) leveling in one preset.
  // Oracle: Set up shows all three row kinds (asserted via their distinct sub-text), the
  // bake/assign mechanism never leaks, and the run reaches a terminal Summary. Offline the
  // fake re-amp may clamp scenes — that's expected; the base still solves and the flow
  // completes.
  test("levels a preset with base + scenes + footswitches end to end", async ({
    page,
  }) => {
    // OFFLINE ONLY (trade T2, ONLINE e2e consolidation): this test's online run is
    // retired, not moved — no *.online.spec.ts file picked it up, since E2E Rig's
    // base+scene+footswitch flow doesn't need its own online arc on top of the Hiwatt
    // (404) one `level.online.spec.ts` already carries. The offline tier below still
    // drives the identical UI/backend path against SimDevice, so the FLOW itself (all
    // three row kinds in Set up, the bake/assign mechanism never leaking, a terminal
    // Summary) stays proven; only the real-audio loudness outcome is untested now.
    test.skip(await isOnline(page), "trade T2 — see this file's own header");
    // ~18-23 re-amp captures (E2E Rig base + all scenes + all footswitches) — the budget
    // matches the terminal wait below, with headroom on top.
    test.setTimeout(1_500_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/Connected · firmware \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name); // E2E Rig

    // Reveal its children (main sound + scene rows + footswitch rows), then tick the
    // WHOLE preset → every child selected.
    await openPreset(page, SCENARIO[0].name);
    await presetBox(page, SCENARIO[0].name).click();
    await filter.fill("");

    await levelButton(page, "any").click();
    // The wizard opens directly at Set up; tick the inline footer ack that gates the commit.
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Set up must show all THREE row kinds, proven by their own row label: the main
    // sound, a scene ("Rhythm"), a block-acting footswitch ("DRIVE"). Scoped to Set up's
    // rows: the list stays mounted (hidden) underneath and names the same sounds.
    const rows = page.locator("[data-setup-row]");
    await expect(rows.getByText("Main sound").first()).toBeVisible();
    await expect(
      rows.getByText("Rhythm", { exact: true }).first(),
    ).toBeVisible(); // a SCENE
    await expect(rows.getByText("DRIVE").first()).toBeVisible(); // a block-acting FOOTSWITCH
    // The bake/assign mechanism is never surfaced.
    await expect(page.getByText(/baked|assigned/i)).toHaveCount(0);

    // Run base + scenes + footswitches → a terminal Summary (whatever it says; offline
    // clamps on scenes/footswitches are fine).
    await startLeveling(page).click();
    await expect(summaryDone(page)).toBeVisible({ timeout: 1_200_000 });

    await expectReampBalanced(page, reampBase);
  });

  // COVERAGE row 37 — scene wipe / bake / conformance oracle.
  // BUG→GATE (2026-07-27 report — the corruption-class preset's SHAPE). SCENARIO[4]
  // "E2E Hiwatt 3S" is a real unit's preset: 3 tone scenes + a 4th literally named
  // "Base Scene" (a real overlay, NOT the base sentinel) and 4 block-acting footswitches,
  // saved `lastLoadedScene = 3`. Its outcomes are command-level gates (the per-scene/
  // per-footswitch Channel is a no-op offline — see level-defaults.spec.ts's header); what
  // ONLY the UI can prove is that the backup scan ENUMERATES this shape: 4 scene children
  // (not 3 + a swallowed "Base Scene", and not 5 with base double-counted) plus 4 footswitch
  // children. No leveling run — list rendering only, so it costs seconds.
  test("enumerates the 3-scene + Base-Scene + 4-footswitch preset in the list", async ({
    page,
  }) => {
    // OFFLINE ONLY (ONLINE e2e consolidation): moved into `level.online.spec.ts` for the
    // online tier, alongside that file's own Hiwatt (404) arc — same slot, one file.
    test.skip(await isOnline(page), "moved to level.online.spec.ts");
    await ensureScenario(page);
    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/Connected · firmware \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[4].name);
    // The collapsed breakdown — the scan's own count, before any expansion.
    await expect(page.getByText("4 scenes, 4 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    await openPreset(page, SCENARIO[4].name);
    // Main sound + 4 scenes + 4 block-acting footswitches = 9 sound sub-rows, each one
    // checkbox named "Choose <sound> in <preset>"; each footswitch says which one it is.
    await expect(
      page.getByRole("checkbox", {
        name: new RegExp(` in ${SCENARIO[4].name}$`),
      }),
    ).toHaveCount(9);
    await expect(page.getByText(/^footswitch \d+$/)).toHaveCount(4);
    // The 4th scene is a real overlay named "Base Scene" — it must appear as its OWN row,
    // distinct from the "Main sound" row (the sentinel).
    await expect(soundBox(page, "Base Scene", SCENARIO[4].name)).toHaveCount(1);
    await expect(soundBox(page, "Main sound", SCENARIO[4].name)).toHaveCount(1);
  });

  // Consecutive-runs idempotency gate — the PR #74 requirement ("2 consecutive leveling
  // runs must produce the same result") that lived only in a session prompt, never as
  // executable infrastructure, until level-rerun.spec.ts (now merged in here — offline
  // suite consolidation; its online tests were already retired to `level.online.spec.ts`'s
  // idempotency test). The SimDevice's field-8 read is READ-YOUR-WRITES (mirrors the real
  // device), so `commands/level_preset.rs`'s pre-run `read_slot_preset_parsed` populates a
  // real, non-`None` `previous_level` offline too, and `leveller::level_unchanged` is
  // reachable in both modes — not merely an online-only property (it was offline-only
  // "events-equality", `previous_level` structurally always `None`, before that fidelity
  // fix landed; see git history for that shape if reviving it).
  // It also carries the retired "levels two presets' Base to different targets" test: run 1
  // levels two presets' Base rows to DIFFERENT per-preset targets through the wizard
  // (`runBaseLevel` asserts each row's own `target:<rowKey>` pick binds), and must finish
  // with every row solved ("All 2 sounds leveled.").
  test("two presets' Base at different targets, then an identical re-run makes no new Saved write (level_unchanged skip)", async ({
    page,
  }) => {
    // Online-only equivalent: `level.online.spec.ts`'s idempotency test, on the Hiwatt
    // state its own strict-arc test just saved — skip here to avoid a second full UI wait
    // + device seize for the same property.
    test.skip(
      await isOnline(page),
      "covered online by level.online.spec.ts's idempotency test",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Base-only, not the whole-preset checkbox: 401/402 now carry footswitches of their own
    // (P4-B fixture rebuild — see level-defaults.spec.ts's header), and a whole-preset
    // tick would sweep those in too and change the Summary's outcome.
    const run = () =>
      runBaseLevel(page, [
        { preset: SCENARIO[1], label: "Crunch" },
        { preset: SCENARIO[2], label: "Lead" },
      ]);

    // /sim/reset (the `page` fixture) cleared the event log, so run 1's log is the whole
    // prefix.
    await run();
    await expect(
      page.getByRole("heading", { name: "All 2 sounds leveled." }),
    ).toBeVisible();
    const afterRun1 = await simEvents(page);
    // Non-vacuous: run 1 must actually solve + write a PresetLevel and Saved this slot,
    // else the "no new writes" check below proves nothing.
    expect(
      afterRun1.some(
        (e) => typeof e === "object" && e !== null && "PresetLevel" in e,
      ),
      "run 1 must write a PresetLevel",
    ).toBe(true);
    expect(
      afterRun1.some(
        (e) => typeof e === "object" && e !== null && "Saved" in e,
      ),
      "run 1 must save",
    ).toBe(true);

    // runBaseLevel's page.goto resets the UI (selection cleared) but NOT the SimDevice —
    // events accumulate.
    await run();
    const afterRun2 = await simEvents(page);
    const run2Delta = afterRun2.slice(afterRun1.length);

    // A real idempotency skip: run 2 still RE-MEASURES (a reference-level `PresetLevel`
    // probe write to solve C is legitimate and expected even on a skip — see
    // `leveller::measure_c`), but must write no NEW `Saved` for this slot — the
    // `level_unchanged` branch calls `restore_saved_preset` (a plain reload) instead of
    // `apply_level` + save. `Saved` is the one event a skip can never emit.
    expect(
      run2Delta.some(
        (e) => typeof e === "object" && e !== null && "Saved" in e,
      ),
      "run 2 solved the same value already saved → must skip the save",
    ).toBe(false);

    await expectReampBalanced(page, reampBase);
  });
});

// Choosing what to level, look B: the first three of the four leveling scenarios, walked
// up to an enabled "Start leveling" with every click counted (Start and Done are the last
// two). What to level is chosen once, on the list: the checkbox ticks a whole preset, the
// NAME of a preset with scenes opens it, and "Select all N shown" acts only on what the
// filter shows (a bug fix: it used to choose the whole library). Set up then needs only
// the backup tick: scenes named like a target already have it. List-and-Set-up only, so
// it costs seconds; OFFLINE ONLY like the list-rendering test above (nothing is started).
test.describe("Level list — choosing, scenario by scenario", () => {
  const FRIEDMAN = SCENARIO[10]; // E2E Friedman 3S: scenes Rhythm / Lead / Base Scene

  test.beforeEach(async ({ page }) => {
    test.skip(await isOnline(page), "list-only — offline");
    await ensureScenario(page);
    await openLevel(page);
    // The backup scan has settled once a preset's sounds are counted.
    await expect(page.getByText("4 scenes, 6 footswitches")).toBeVisible({
      timeout: 60_000,
    });
  });

  test("scenario 1, level one preset: Checkbox · Level N sounds · Backup tick (· Start · Done = 5)", async ({
    page,
  }) => {
    expect(FRIEDMAN.name).toBe("E2E Friedman 3S");
    // Not part of the walk: learn how many sounds the preset holds (every enabled sub-row),
    // then close it again.
    await openPreset(page, FRIEDMAN.name);
    const sounds = await page
      .getByRole("checkbox", { name: new RegExp(` in ${FRIEDMAN.name}$`) })
      .and(page.locator(":enabled"))
      .count();
    await page
      .getByRole("button", { name: `Close ${FRIEDMAN.name}`, exact: true })
      .click();
    expect(sounds).toBeGreaterThan(3); // main sound + 3 scenes (+ footswitches)

    let clicks = 0;
    await presetBox(page, FRIEDMAN.name).click(); // 1 Checkbox
    clicks++;
    await expect(page.getByText("1 preset", { exact: true })).toBeVisible();
    await levelButton(page, sounds).click(); // 2 Level N sounds
    clicks++;
    // Set up: the scenes named like a target already have it — nothing to retarget.
    const slot = String(FRIEDMAN.slot);
    await expect(page.locator(`[data-select="target:s${slot}:0"]`)).toHaveText(
      "Rhythm",
    );
    await expect(page.locator(`[data-select="target:s${slot}:1"]`)).toHaveText(
      "Lead",
    );
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // 3 Backup tick
    clicks++;
    await expect(startLeveling(page)).toBeEnabled(); // 4 Start leveling · 5 Done
    expect(clicks).toBe(3);
  });

  test("scenario 2, level a bank: Type · Select all shown · Level · Backup tick (· Start · Done = 6), hidden presets untouched", async ({
    page,
  }) => {
    let actions = 0;
    await page.getByPlaceholder("Filter by name or slot").fill("E2E Hiwatt"); // 1 Type
    actions++;
    await expect(page.getByText("2 of 11 shown")).toBeVisible();
    await page.getByRole("checkbox", { name: "Select all 2 shown" }).click(); // 2
    actions++;
    // Only the two shown presets were chosen (the caption counts every chosen preset,
    // hidden ones included) — the old Select all chose all eleven.
    await expect(page.getByText("2 presets", { exact: true })).toBeVisible();
    await levelButton(page, "any").click(); // 3 Level N sounds
    actions++;
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // 4 Backup tick
    actions++;
    await expect(startLeveling(page)).toBeEnabled(); // 5 Start leveling · 6 Done
    expect(actions).toBe(4);
  });

  test("scenario 3, some scenes of one preset: Name · Scene · Scene · Level 2 sounds · Backup tick (· Start · Done = 7)", async ({
    page,
  }) => {
    let clicks = 0;
    // 1 The name of a preset with scenes OPENS it; it chooses nothing by itself.
    await page
      .getByRole("button", { name: FRIEDMAN.name, exact: true })
      .click();
    clicks++;
    await expect(presetBox(page, FRIEDMAN.name)).toHaveAttribute(
      "aria-checked",
      "false",
    );
    await soundBox(page, "Rhythm", FRIEDMAN.name).click(); // 2 Scene
    clicks++;
    await soundBox(page, "Lead", FRIEDMAN.name).click(); // 3 Scene
    clicks++;
    await expect(presetBox(page, FRIEDMAN.name)).toHaveAttribute(
      "aria-checked",
      "mixed",
    );
    await levelButton(page, 2).click(); // 4 Level 2 sounds
    clicks++;
    await expect(
      page.getByRole("heading", { name: "Level 2 sounds" }),
    ).toBeVisible();
    await page.getByText(/I.ve backed up with Pro Control/i).click(); // 5 Backup tick
    clicks++;
    await expect(startLeveling(page)).toBeEnabled(); // 6 Start leveling · 7 Done
    expect(clicks).toBe(5);
  });
});
