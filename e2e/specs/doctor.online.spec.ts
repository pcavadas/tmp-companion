import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  reampCounters,
  reampOff,
} from "../fixtures/scenario";

// ONLINE Doctor consolidation (8-file → 4-file suite shrink). Two oracles that both need
// the real device, merged into one file so the online run pays ONE Playwright boot/rest
// for Doctor instead of three (doctor.spec.ts's online half + doctor-oracle.online.spec.ts +
// doctor-apply.online.spec.ts, which stays its own spec — see its own header).
//
// (a) is doctor-oracle.online.spec.ts moved here (its two wire-seam-only rows, FIZZY and
//     BRIGHT, are covered numerically by src-tauri/src/doctor_audio_oracle.rs) — the executable proof that all 13 of E2E Doctor Oracle's (407) spectral checks
//     actually diagnose on real audio, not just that the fixture carries the right shape
//     (that structural half stays pinned offline by fixture_gates.rs).
// (b) is E2E Edge's (402) baked 2.6 kHz EQ ring on real audio, by raw invoke: base plus two
//     consecutive scenes. (a)'s RESONANT row is a q14 +24 dB spike far over the ring gate;
//     Edge's ring is the near-threshold one (a hedged chip still counts), and its scenes are
//     the only online Doctor sounds that run the SCENE path — the second scene skips its
//     preset load (`doctor_skip_load`). It used to drive the Doctor UI over all ~10 Edge
//     sounds behind a whole-library backup scan; the chip's rendering, its scoping to its own
//     card and the select → check → Results flow are `DoctorResults.test.tsx`'s and
//     doctor.spec.ts's offline.
test.describe("Doctor online — spectral oracle (407) + Edge EQ ring (402)", () => {
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
    diags: { key: string; label: string }[];
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

  // The 13 switches, in `ftsw` row order (== the 0-based `footswitch` value to send). One
  // entry per src-tauri/src/fixture_gates.rs's `fx_doctor_oracle_fires_nothing_in_base_and_carries_
  // its_defect_table` `table` row.
  const SWITCHES: { row: number; label: string; verdict: Verdict }[] = [
    { row: 1, label: "CONTROL", verdict: { kind: "zero" } },
    { row: 2, label: "MUDDY", verdict: { kind: "contains", diagKey: "muddy" } },
    // BOOMY and HARSH fire on the pitched-pluck stimulus (#199): the remote device read
    // BOOMY's lows local 7.0 against the 4.1 gate (dark co-fires) and HARSH's high-mids
    // local 4.6 against 3.9.
    { row: 3, label: "BOOMY", verdict: { kind: "contains", diagKey: "boomy" } },
    { row: 4, label: "HARSH", verdict: { kind: "contains", diagKey: "harsh" } },
    // FIZZY and BRIGHT stay seam-only: the cab IR leaves nothing above ~10 kHz to lift,
    // and the amp's treble knob changes nothing audible (fixture_gates.rs's defect table). Their
    // verdicts are covered on captured audio by doctor_audio_oracle.rs instead.
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
    //   14 sounds (1 base + 13 switches) x 18 s/capture (doctor.spec.ts's own
    //   documented 12-18 s/capture online range, worst end) = 252 s.
    //   + up to 3 floor-suspect retries (FLOOR_RETRY_GAP_MS 5 s + one recapture
    //     18 s each, leveller.rs) = 69 s.
    //   + ONE live field-8 isolation read for the whole run (nodes/footswitches
    //     are sent empty below, so every sound falls to the cached-per-list-index
    //     legacy read) = ~5 s.
    //   + a cold `ensureScenario` seed, worst case (its own 240_000 ms request
    //     timeout) = 240 s.
    //   + the run-end restore-active-preset reload = ~5 s.
    //   Baseline ~= 252 + 69 + 5 + 240 + 5 = 571 s. >=2x headroom, PLUS the ceiling
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
      "all 14 requested sounds must come back, none dropped mid-run",
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

    // Specificity: the localized ring and the wash tail are each owned by one switch, and
    // no other sound comes near either gate on the remote device (the tallest other peak
    // reads 11.9 dB against the 13.5 dB ring gate), so neither may fire anywhere else.
    const OWNED: Record<string, string> = {
      resonant: "RESONANT",
      washed: "WASHED",
    };
    for (const [i, s] of SWITCHES.entries()) {
      const sound = preset.sounds[i + 1];
      const keys = diagKeys(sound);
      for (const [key, owner] of Object.entries(OWNED)) {
        if (s.label !== owner) {
          expect(
            keys,
            `${s.label} (switch ${String(s.row)}) must not fire "${key}", which only ${owner} owns`,
          ).not.toContain(key);
        }
      }
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
          // error already checked above). See the SWITCHES table for why FIZZY
          // and BRIGHT stay inert on this chain.
          expect(
            Number.isFinite(sound.integratedLufs),
            `${s.label} (switch ${String(s.row)}) must have captured real audio (finite integratedLufs)`,
          ).toBe(true);
          break;
      }
    }

    await expectReampBalanced(page, reampBase);
  });

  // (b) — COVERAGE row 31.
  test("E2E Edge: the 2.6 kHz EQ ring fires on base and on two consecutive scenes", async ({
    page,
  }) => {
    test.skip(
      !(await isOnline(page)),
      "online-only: the EQ-ring oracle needs real audio",
    );
    // 3 sounds x 18 s/capture (worst end) + up to 3 floor-suspect retries (23 s each) + a
    // cold `ensureScenario` (240 s) ≈ 360 s; the request budget sits under the ceiling so a
    // hang names itself.
    test.setTimeout(900_000);
    const DOCTOR_T = 600_000;
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    const baseItem = {
      key: "base",
      listIndex: EDGE.slot,
      scene: null as number | null,
      footswitch: null,
      label: "Base",
      tag: null,
      topologyId: "guitar-humbucker",
      calibrationLufs: null,
      profileId: null,
      nodes: [],
      footswitches: [],
    };
    // Base first, then two scenes back to back: the second scene follows a scene of the same
    // preset that wrote nothing, so it takes `doctor_skip_load`'s no-reload path.
    const items = [
      baseItem,
      { ...baseItem, key: "scene0", scene: 0, label: "Scene 1" },
      { ...baseItem, key: "scene1", scene: 1, label: "Scene 2" },
    ];
    const result = (await invoke(
      page,
      "doctor_check",
      { items, restoreListIndex: null, onResult: "__CHANNEL__:1" },
      DOCTOR_T,
    )) as DoctorCheckResult;

    expect(result.stopped, "the run must complete, not stop early").toBe(false);
    expect(result.presets.length, "exactly one preset was checked").toBe(1);
    const preset = result.presets[0];
    expect(preset.listIndex).toBe(EDGE.slot);
    expect(
      preset.sounds.map((s) => s.key),
      "every requested sound must come back, none dropped mid-run",
    ).toEqual(items.map((i) => i.key));
    // The ring lives in Edge's BASE graph, so every played sound carries it. Hedged counts:
    // the backend's key is `resonant` whatever the z (the "Possible" prefix is the UI's).
    for (const sound of preset.sounds) {
      expect(sound.error, `${sound.key} must not have errored`).toBeNull();
      const ring = sound.diags.find((d) => d.key === "resonant");
      expect(
        ring?.label,
        `${sound.key} diags [${sound.diags.map((d) => d.key).join(", ")}] must ring at 2.x kHz`,
      ).toMatch(/^Rings at 2\.\d kHz$/);
    }

    await expectReampBalanced(page, reampBase);
  });
});
