import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  openLevel,
  reampCounters,
  reampOff,
  runDoctorCheck,
  selectPresetsForCheck,
} from "../fixtures/scenario";

// ONLINE Doctor consolidation (8-file → 4-file suite shrink). Two oracles that both need
// the real device, merged into one file so the online run pays ONE Playwright boot/rest
// for Doctor instead of three (doctor.spec.ts's online half + doctor-oracle.online.spec.ts +
// doctor-apply.online.spec.ts, which is separately demoted to on-demand — see its own header).
//
// (a) is doctor-oracle.online.spec.ts moved here UNCHANGED (including the 5 wire-seam-only
//     rows) — the executable proof that all 14 of E2E Doctor Oracle's (407) spectral checks
//     actually diagnose on real audio, not just that the fixture carries the right shape
//     (that structural half stays pinned offline by lib.rs's fixture_gates).
// (b) is doctor.spec.ts's online UI run, moved here selecting all three fixtures exactly as
//     that run used to (E2E Rig 400 + E2E Pedalboard 401 + E2E Edge 402). Edge alone carries
//     the 2.6 kHz EQ-ring assertions (the chip must fire on every one of Edge's played
//     sounds, and opening a ringing row must surface the "cut the 2 kHz band" prescription);
//     Rig and Pedalboard carry the CROSS-CARD scoping proof (zero ring chips leak onto a
//     sibling's card). That scoping proof is a UI property distinct from (a)'s base/CONTROL
//     "healthy content never rings" property on a DIFFERENT fixture (407) — (a) proves the
//     BACKEND never diagnoses a ring on healthy audio; only a shared multi-card Results view
//     can prove the FRONTEND never renders one diagnosed on Edge onto a sibling's card — so
//     narrowing to Edge alone (an earlier version of this file's trade) dropped a real
//     property rather than a redundant one. doctor.spec.ts keeps its full 3-preset flow
//     OFFLINE (test.skip'd online there now, trade — see that file's own header); the
//     cut-through-estimate UI assertion (generic capture plumbing, not preset-specific)
//     stays covered there too.
test.describe("Doctor online — spectral oracle (407) + Edge EQ-ring UI (402)", () => {
  // Shared across both tests below (mirrors doctor-oracle.online.spec.ts's own teardown):
  // reamp-off FIRST on every test so a mid-test failure can't strand the unit
  // input-muted, and ONE afterAll clearScenario rather than a per-test clear — a per-test
  // clear would force the second test's ensureScenario down the flaky in-process re-seed
  // (the very re-import cost this consolidation exists to avoid paying twice).
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  const ORACLE = SCENARIO[7]; // E2E Doctor Oracle, slot 407
  const EDGE = SCENARIO[2]; // E2E Edge, slot 402

  interface DoctorSoundResult {
    key: string;
    diags: { key: string }[];
    integratedLufs: number;
    error: string | null;
  }
  interface DoctorPresetResult {
    listIndex: number;
    sounds: DoctorSoundResult[];
  }
  interface DoctorCheckResult {
    presets: DoctorPresetResult[];
    stopped: boolean;
  }

  type Verdict =
    | { kind: "zero" } // negative control: diags must be empty
    | { kind: "contains"; diagKey: string } // must fire this kind (co-fire allowed)
    | { kind: "seam" }; // wire-seam-only: assert a real, unerrored capture only

  // The 14 switches, in `ftsw` row order (== the 0-based `footswitch` value to send). One
  // entry per src-tauri/src/lib.rs's `fx_doctor_oracle_fires_nothing_in_base_and_carries_
  // its_defect_table` `table` row.
  const SWITCHES: { row: number; label: string; verdict: Verdict }[] = [
    { row: 1, label: "CONTROL", verdict: { kind: "zero" } },
    { row: 2, label: "MUDDY", verdict: { kind: "contains", diagKey: "muddy" } },
    { row: 3, label: "BOOMY", verdict: { kind: "seam" } },
    { row: 4, label: "HARSH", verdict: { kind: "seam" } },
    { row: 5, label: "FIZZY", verdict: { kind: "seam" } },
    { row: 6, label: "LOST", verdict: { kind: "contains", diagKey: "lost" } },
    { row: 7, label: "BRIGHT", verdict: { kind: "seam" } },
    { row: 8, label: "CUTTHRU", verdict: { kind: "zero" } },
    {
      row: 9,
      label: "RESONANT",
      verdict: { kind: "contains", diagKey: "resonant" }, // harsh may co-fire
    },
    { row: 10, label: "BOXY", verdict: { kind: "contains", diagKey: "boxy" } },
    { row: 11, label: "THIN", verdict: { kind: "contains", diagKey: "thin" } },
    { row: 12, label: "DARK", verdict: { kind: "contains", diagKey: "dark" } },
    {
      row: 13,
      label: "WASHED",
      verdict: { kind: "contains", diagKey: "washed" }, // lost may co-fire
    },
    { row: 14, label: "SPIKY", verdict: { kind: "seam" } },
  ];

  // (a) — moved verbatim from doctor-oracle.online.spec.ts (COVERAGE row 40).
  test("base fires nothing; each switch's own spectral verdict fires; seam-only rows just capture", async ({
    page,
  }) => {
    test.skip(
      !(await isOnline(page)),
      "online-only: the oracle's verdicts need real audio",
    );
    // Budget arithmetic (added up rather than eyeballed, this file's own style):
    //   15 sounds (1 base + 14 switches) x 18 s/capture (doctor.spec.ts's own
    //   documented 12-18 s/capture online range, worst end) = 270 s.
    //   + up to 3 floor-suspect retries (FLOOR_RETRY_GAP_MS 5 s + one recapture
    //     18 s each, leveller.rs) = 69 s.
    //   + ONE live field-8 isolation read for the whole run (nodes/footswitches
    //     are sent empty below, so every sound falls to the cached-per-list-index
    //     legacy read) + its RECONNECT_GAP_MS settle = ~5 s.
    //   + a cold `ensureScenario` seed, worst case (its own 240_000 ms request
    //     timeout) = 240 s.
    //   + the run-end restore-active-preset reload = ~5 s.
    //   Baseline ~= 270 + 69 + 5 + 240 + 5 = 589 s. >=2x headroom, PLUS the ceiling
    // must exceed worst-case ensureScenario (240 s) + the request budget below
    // (1080 s) = 1320 s, or a cold-seed run would die on the vaguer test timeout
    // before the request timeout could name the hang => 1_500_000 ms.
    test.setTimeout(1_500_000);
    // Single request timeout for the one doctor_check call, kept below the test's
    // own ceiling so a genuine hang reports as a clear "request timeout" rather
    // than the vaguer "Test timeout exceeded", with slack left for
    // ensureScenario/teardown either side of it.
    const DOCTOR_T = 1_080_000;

    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // Nodes/footswitches sent empty: `resolve_sound_isolation`
    // (src-tauri/src/commands/doctor.rs) falls back to the legacy live field-8
    // read (cached per list index) whenever a sound's `nodes` is empty — this
    // spec raw-invokes the command directly rather than replaying a backup scan,
    // so it exercises that fallback path deliberately, same as
    // doctor-apply.online.spec.ts's raw jobs.
    const baseItem = {
      key: "base",
      listIndex: ORACLE.slot,
      scene: null as number | null,
      footswitch: null as number | null,
      label: "Base",
      tag: null as string | null,
      topologyId: "guitar-humbucker",
      calibrationLufs: null as number | null,
      profileId: null as string | null,
      nodes: [] as unknown[],
      footswitches: [] as unknown[],
    };
    const items = [
      baseItem,
      ...SWITCHES.map((s) => ({
        ...baseItem,
        key: `fs${String(s.row)}`,
        footswitch: s.row,
        label: s.label,
      })),
    ];

    const result = (await invoke(
      page,
      "doctor_check",
      {
        items,
        restoreListIndex: null,
        onResult: "__CHANNEL__:1",
      },
      DOCTOR_T,
    )) as DoctorCheckResult;

    expect(result.stopped, "the run must complete, not stop early").toBe(false);
    expect(result.presets.length, "exactly one preset was checked").toBe(1);
    const preset = result.presets[0];
    expect(preset, "preset result must be present").toBeDefined();
    expect(preset.listIndex).toBe(ORACLE.slot);

    // Non-vacuous: every requested sound must come back, in the order requested
    // (doctor_check preserves original item order — see its own "Preserve the
    // original sound/preset order" comment) — a mid-run drop must fail here,
    // mirroring level.online.spec.ts's "no silent mid-batch drop" style.
    expect(
      preset.sounds.map((s) => s.key),
      "all 15 requested sounds must come back, none dropped mid-run",
    ).toEqual(items.map((i) => i.key));

    // No sound may have failed its capture — a failure would make its diags
    // vacuously empty and silently pass a "zero diags" row.
    for (const sound of preset.sounds) {
      expect(sound.error, `${sound.key} must not have errored`).toBeNull();
    }

    // The order-equality assertion above proves positional identity, so the sounds
    // are indexed directly: [0] = base, [i + 1] = SWITCHES[i].
    const diagKeys = (s: DoctorSoundResult): string[] =>
      s.diags.map((d) => d.key);

    // Base: zero diags (the fixture's whole point — every defect block rides
    // enabled-neutral in base). This — together with CONTROL below — is also this
    // file's proof that a healthy sound never renders a ring/resonant verdict,
    // which test (b) below leans on instead of re-selecting a second healthy
    // preset online.
    expect(diagKeys(preset.sounds[0]), "base must fire nothing").toEqual([]);

    for (const [i, s] of SWITCHES.entries()) {
      const sound = preset.sounds[i + 1];
      const keys = diagKeys(sound);
      switch (s.verdict.kind) {
        case "zero":
          expect(
            keys,
            `${s.label} (switch ${String(s.row)}) is a negative control — zero diags`,
          ).toEqual([]);
          break;
        case "contains":
          expect(
            keys.includes(s.verdict.diagKey),
            `${s.label} (switch ${String(s.row)}) diags [${keys.join(", ")}] must contain "${s.verdict.diagKey}"`,
          ).toBe(true);
          break;
        case "seam":
          // Seam-only: the write lands, but no spectral verdict is expected —
          // assert only that a real capture happened (finite LUFS, no error —
          // error already checked above). See this file's header for why each
          // of these five is inert/gate-blocked on the synthetic stimulus.
          expect(
            Number.isFinite(sound.integratedLufs),
            `${s.label} (switch ${String(s.row)}) must have captured real audio (finite integratedLufs)`,
          ).toBe(true);
          break;
      }
    }

    await expectReampBalanced(page, reampBase);
  });

  // (b) — doctor.spec.ts's online UI run, selecting all three fixtures (COVERAGE row 31 +
  // the cross-card ring-chip scoping property — see this file's own header).
  test("E2E Rig/Pedalboard/Edge: the 2.6 kHz EQ ring fires on every Edge sound and never leaks onto a sibling card", async ({
    page,
  }) => {
    test.skip(
      !(await isOnline(page)),
      "online-only: the EQ-ring oracle needs real audio",
    );
    // Budget, added up rather than eyeballed (a bare 600 s ceiling here previously left the
    // Results wait below alone eating 500 s against it — cold-seed runs died on a vague "Test
    // timeout exceeded" instead of the real assertion). All three fixtures now run (not Edge
    // alone): ~29 sounds total (doctor.spec.ts's own "~29 real sounds across the three intact
    // fixtures" figure) x 18 s/capture (worst end of doctor.spec.ts's documented 12-18 s
    // range) ≈ 520 s, + up to 3 floor-suspect recapture retries (5 s gap + 18 s recapture
    // each, leveller.rs) ≈ 69 s, + a cold `ensureScenario` reseed (240 s worst case,
    // fixtures/scenario.ts's own request budget), + the real backup scan the preset picker
    // pays before Check enables (~60 s, matching copy.spec.ts's precedent for that same
    // wait), + small UI-click slack (~10 s) ≈ 900 s. 1_200_000 ms gives headroom, matching
    // this file's other test's generous-ceiling style.
    test.setTimeout(1_200_000);
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    await openLevel(page);

    const picked = [SCENARIO[0], SCENARIO[1], EDGE]; // E2E Rig, E2E Pedalboard, E2E Edge
    await selectPresetsForCheck(page, picked);
    await runDoctorCheck(page);

    // Terminal Results, or a fast-fail error pane (see doctor.spec.ts's own header for
    // why the error pane is matched in the same wait). ~29 sounds at up to 18 s/capture
    // (~520 s) plus floor-suspect recapture retries (~69 s) ≈ 589 s. 900 s keeps real
    // headroom without eating most of this test's own ceiling — see that ceiling's budget
    // comment above.
    await expect(
      page
        .getByText(/presets? need a look|All clear|check couldn.t finish/)
        .first(),
    ).toBeVisible({ timeout: 900_000 });
    await expect(page.getByText(/check couldn.t finish/)).toHaveCount(0);

    const everything = page.getByRole("radio", { name: "Everything" });
    if (await everything.isVisible().catch(() => false)) {
      await everything.click();
    }

    await expect(page.getByText(EDGE.name).first()).toBeVisible();

    // Hedged accepted too: severity < 1.0 renders "Possible Rings at N kHz" via
    // possibleLabel() (severity.ts), and TubeScreamer-engaged Edge rows can erode the
    // margin into hedge territory — a hedged detection is still a detection.
    const ringChip = /^(?:Possible )?Rings at 2\.\d kHz$/;
    const edgeCard = page.locator(`[data-preset-card="${EDGE.name}"]`);
    await expect(edgeCard).toHaveCount(1);

    // CROSS-CARD SCOPING: the ring chip must never leak onto a sibling's card. Each
    // sibling card gets its own non-vacuity guard (`toHaveCount(1)`) — an attribute/name
    // drift must not make the zero-count check below pass trivially against a card that
    // doesn't exist.
    for (const p of [SCENARIO[0], SCENARIO[1]]) {
      const card = page.locator(`[data-preset-card="${p.name}"]`);
      await expect(card).toHaveCount(1);
      await expect(card.getByText(ringChip)).toHaveCount(0);
    }

    // AS-PLAYED SEMANTICS: the EQ ring lives in Edge's BASE graph, so it is present in
    // every one of Edge's played sounds, not one isolated row — expand the collapsed
    // "N sounds check out" healthy bucket (present whenever any sound on the card IS
    // flagged) BEFORE counting, or a healthy-rendering sound would drop out of both
    // sides of the equality and mask a regression.
    const healthySummary = edgeCard.getByText(/\d+ sounds? checks? out/);
    if (await healthySummary.isVisible().catch(() => false)) {
      await healthySummary.click();
    }
    const edgeRowCount = await edgeCard.locator("[data-sound-row]").count();
    expect(edgeRowCount).toBeGreaterThanOrEqual(9);
    await expect(edgeCard.getByText(ringChip)).toHaveCount(edgeRowCount);

    // Click the ROW HEADER's label, scoped inside a [data-sound-row] — a bare
    // page-wide getByText(name).last() resolves into the "Level jumps" advisory panel
    // below the rows instead (doctor.spec.ts's own incident note, HW run 2026-08-09).
    const edgeRingRow = edgeCard.locator("[data-sound-row]").first();
    await edgeRingRow.getByText(EDGE.name).first().click();
    await expect(
      page.getByText(/Rings at 2\.\d kHz — cut the 2 kHz band/).first(),
    ).toBeVisible();
    await edgeRingRow.getByText(EDGE.name).first().click(); // collapse
    await expect(
      page.getByText(/Rings at 2\.\d kHz — cut the 2 kHz band/),
    ).toHaveCount(0);

    await expectReampBalanced(page, reampBase);
  });
});
