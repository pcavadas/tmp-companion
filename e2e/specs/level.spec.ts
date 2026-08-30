import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  baseRowKey,
  clearScenario,
  ensurePresetGroupOpen,
  ensureScenario,
  expectReampBalanced,
  isOnline,
  pickBaseTarget,
  reampCounters,
  reampOff,
  runBaseLevel,
  selectBaseOnly,
  simEvents,
} from "../fixtures/scenario";

// Level scenarios — run identically offline (fake re-amp) and online (real re-amp).
// SCENARIO[0] "E2E Rig" has BOTH footswitch scenes AND block-acting footswitches (and an
// amp flip). None of the six rebuilt fixtures is Base-only any more (see
// e2e/fixtures/COVERAGE.md) — SCENARIO[1]/[2] ("E2E Pedalboard"/"E2E Edge") now carry
// footswitches/scenes of their own, so the first test below selects each preset's Base
// row EXPLICITLY (never the whole-preset tick) to keep one selected row per preset — a
// whole-preset tick would sweep those in too and shift the terminal summary's
// Done-vs-Accept text for reasons unrelated to what this test drives.
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
test.describe("Level — plain presets + a scenes-and-footswitches preset", () => {
  // Between tests: SAFETY only (re-amp off, so an aborted capture can't strand the
  // unit input-muted for the next test). Slot cleanup happens ONCE in afterAll —
  // clearing between tests would force the next test's ensureScenario down the flaky
  // in-process re-seed (the runner seeds once per spec FILE; HW-observed: test 1
  // passed, its clear forced test 2's re-seed into the 0xe00002c5 open lockout).
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });

  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  test("levels two presets' Base to different targets, end to end", async ({
    page,
  }) => {
    // OFFLINE ONLY (ONLINE e2e consolidation): moved verbatim into
    // `level.online.spec.ts` for the online tier.
    test.skip(await isOnline(page), "moved to level.online.spec.ts");
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    // Select each preset's Base row explicitly (expand → tick "Base Preset") so exactly
    // ONE row per preset is selected — both now carry footswitches/scenes of their own,
    // so a whole-preset tick would sweep those in too and shift the terminal summary's
    // Done-vs-Accept text. The filter narrows the list to each in turn; the selection
    // persists across filters.
    const presets = [SCENARIO[1], SCENARIO[2]];
    for (const p of presets) {
      await selectBaseOnly(page, p.name);
    }

    await page.getByRole("button", { name: /Level 2 preset/ }).click();

    // The wizard opens directly at Set up; tick the inline footer ack that gates the
    // commit (there is no separate Back-up step).
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Two DIFFERENT per-preset targets — each row's own `target:<rowKey>` trigger is
    // unique by construction now, regardless of how many other rows that preset has.
    const targets = [
      { slot: SCENARIO[1].slot, label: "Crunch" },
      { slot: SCENARIO[2].slot, label: "Lead" },
    ];
    for (const { slot, label } of targets) {
      await ensurePresetGroupOpen(page, slot);
      await pickBaseTarget(page, slot, label);
    }
    // The picks must actually BIND — assert each row's trigger now reads its target
    // (guards a silent display-vs-value no-op the always-solving fake re-amp would hide).
    for (const { slot, label } of targets) {
      await expect(
        page.locator(`[data-pick="target:${baseRowKey(slot)}"]`),
      ).toContainText(label);
    }

    await page.getByRole("button", { name: /Start.*2 sound/ }).click();
    await expect(page.getByRole("button", { name: "Done" })).toBeVisible({
      timeout: 240_000,
    });

    // Standing safety gate: the app disengaged re-amp at least as often as it engaged,
    // checked BEFORE the afterEach reampOff rescue (so a stranded engage fails here).
    await expectReampBalanced(page, reampBase);
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
    // ~18-23 re-amp captures (E2E Rig base + all scenes + all footswitches) plus up to two
    // `ensure_fresh_load` commit-window stalls (COMMIT_WINDOW_SECS = 150 s each, danger.md)
    // if a same-slot load races a prior save — worst case ≈ 1200 s, matching the terminal
    // wait below; the budget here adds headroom on top.
    test.setTimeout(1_500_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await page.goto("/");
    await page.getByRole("button", { name: /backed up/i }).click(); // startup disclaimer
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[0].name); // E2E Rig

    // Reveal its children (Base + scene rows + footswitch rows), then tick the WHOLE
    // preset → every child selected.
    await page.getByTitle(/Show Base/).click();
    await page.getByTitle("Select preset to level").first().click();
    await filter.fill("");

    await page.getByRole("button", { name: /Level 1 preset/ }).click();
    // The wizard opens directly at Set up; tick the inline footer ack that gates the commit.
    await page.getByText(/I.ve backed up with Pro Control/i).click();

    // Set up must show all THREE row kinds — the redesign dropped the old per-row-kind
    // sub-text captions (design 1a's terser direction), so this proves them by their own
    // row label instead: Base, a scene ("Rhythm"), a block-acting footswitch ("DRIVE").
    await expect(page.getByText("Base").first()).toBeVisible();
    await expect(page.getByText("Rhythm").first()).toBeVisible(); // a footswitch SCENE
    await expect(page.getByText("DRIVE").first()).toBeVisible(); // a block-acting FOOTSWITCH
    // The bake/assign mechanism is never surfaced.
    await expect(page.getByText(/baked|assigned/i)).toHaveCount(0);

    // Run base + scenes + footswitches → a terminal Summary (Done OR Accept; offline
    // clamps on scenes/footswitches are fine).
    await page.getByRole("button", { name: /Start.*\d+ sound/ }).click();
    await expect(
      page.getByRole("button", { name: /^(Done|Accept)$/ }),
    ).toBeVisible({ timeout: 1_200_000 });

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
    await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
      timeout: 20_000,
    });

    const filter = page.getByPlaceholder(/Filter by name or slot/i);
    await filter.fill(SCENARIO[4].name);
    // The collapsed breakdown — the scan's own count, before any expansion.
    await expect(page.getByText("4 scenes · 4 footswitches")).toBeVisible({
      timeout: 60_000,
    });

    await page.getByTitle(/Show Base/).click();
    // Base + 4 footswitch scenes + 4 block-acting footswitches = 9 child rows.
    await expect(page.getByText("main preset sound")).toHaveCount(1);
    await expect(page.getByText("footswitch scene")).toHaveCount(4);
    await expect(page.getByText("footswitch", { exact: true })).toHaveCount(4);
    // The 4th scene is a real overlay named "Base Scene" — it must appear as its OWN row,
    // distinct from the "Base Preset" row (the sentinel).
    await expect(page.getByText("Base Scene", { exact: true })).toHaveCount(1);
    await expect(page.getByText("Base Preset", { exact: true })).toHaveCount(1);
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
  test("two identical base runs: run 2 makes no new Saved write (level_unchanged skip)", async ({
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

    // Base-only, not the whole-preset checkbox: 401 now carries footswitches of its own
    // (P4-B fixture rebuild — see level-defaults.spec.ts's header), and a whole-preset
    // tick would sweep those in too and shift the terminal summary's Done-vs-Accept text.
    const run = () =>
      runBaseLevel(page, [{ preset: SCENARIO[1], label: "Crunch" }]);

    // /sim/reset (the `page` fixture) cleared the event log, so run 1's log is the whole
    // prefix.
    await run();
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
