import { test, expect } from "../fixtures/test";
import {
  type LevelBlock,
  SCENARIO,
  clearScenario,
  ensureScenario,
  expectReampBalanced,
  invoke,
  isOnline,
  LEVEL_T,
  reampCounters,
  reampOff,
  simEvents,
} from "../fixtures/scenario";

// PR #144 NEW COVERAGE — the headroom trade: raising base `presetLevel` while lowering a
// base amp's `outputLevel` by the same dB to buy a clamped SIBLING (a Full-overlay scene)
// real headroom. Mirrors `e2e_server_tests.rs`'s ⟦F1⟧
// (`a_batched_scene_run_persists_both_halves_of_a_landed_headroom_trade`) at the HTTP-bridge
// layer, over the SAME slot 404 (E2E Hiwatt 3S) shape those Rust tests pin: one guitar amp
// (`ACD_HiwattDR103CanMod`) and scene 3, whose Full overlay pins the SAME `outputLevel` as
// base — the one shape that benefits from a trade.
//
// The trade is disclosed on every row of a batch that traded (`LevelResult.trade`), and it is
// stamped by `level_scenes_apply_batched` — a per-scene BATCHED command whose per-row PROGRESS
// rides the Channel (`.claude/rules/e2e.md`'s Channel-streaming seam), but whose RETURN VALUE
// (this file's only observation path) is real over the mock IPC regardless: the command runs
// for real, only its live progress is unobservable offline. That is exactly the sanctioned
// twin the rule describes — this file never drives the wizard UI to prove the trade landed.
//
// NO BRIDGE COMMAND SEEDS `presetLevel` DIRECTLY (unlike the Rust tests' raw
// `Session::set_preset_level` + `save_current_preset`), so this file seeds it through the
// PUBLIC leveling command instead: a `level_preset` run with `save:true` at a target
// `constant_c - X` LU makes the live device AND the saved field-8 document agree at the
// solved level (a real `saveCurrentPreset` flips the sim's `ever_saved` gate) — the same
// agreement `trade_sim()`'s manual round trip buys the Rust tests. `X` is picked so the
// solved level lands with a few dB of `presetLevel` headroom left below 1.0 for the trade to
// spend; the exact solved level is READ BACK from the seed run's own result, never assumed.
// COVERAGE row 43 — headroom trade wire + persist on 404: applied:true lands and saves
// exactly once; applied:false (save:false) discloses the same trade and writes nothing.
test.describe("Level — headroom trade (batched scene run, raw invoke)", () => {
  test.afterEach(async ({ page }) => {
    await reampOff(page);
  });
  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await clearScenario(page);
    await page.close();
  });

  const TRADE_SLOT = SCENARIO[4].slot; // 404, E2E Hiwatt 3S — KEEP VERBATIM (danger.md-adjacent)
  const TRADE_AMP = "ACD_HiwattDR103CanMod";
  const TRADE_SCENE = 3; // the Full-overlay scene whose own outputLevel pins base's

  interface LevelPresetResult {
    saved: boolean;
    clamped: boolean;
    constant_c: number;
    final_level: number;
  }
  interface TradeAmpMove {
    group_id: string;
    node_id: string;
    parameter_id: string;
    previous_value: number;
    value: number | null;
  }
  interface TradeSummary {
    applied: boolean;
    raise_db: number;
    previous_preset_level: number;
    preset_level: number;
    base_amps: TradeAmpMove[];
    cap: string | null;
    benefiting: { kind: string; sceneSlot?: number }[];
  }
  interface SceneLevelResult {
    scene_slot: number | null;
    measured_lufs: number;
    clamped: boolean;
    clamp_kind: string | null;
    saved: boolean;
    persist_mismatch: boolean | null;
    trade: TradeSummary | null;
  }

  interface SavedEvent {
    Saved: number;
  }
  function isSaved(e: unknown): e is SavedEvent {
    return typeof e === "object" && e !== null && "Saved" in e;
  }

  const sceneRow = (rows: SceneLevelResult[], scene: number | null) =>
    rows.find((r) => r.scene_slot === scene);

  test("a batched run trades a landed raise for scene 3's headroom, and a save:false run discloses the same trade as advisory", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the fixture's authored ceilings; an online run would " +
        "permanently mutate the verbatim-404 export",
    );
    await ensureScenario(page);
    const reampBase = await reampCounters(page);

    // ── Seed: a real save that puts a few dB of `presetLevel` headroom below 1.0 ────────
    const levelJob = (target: number, save: boolean) => ({
      job: {
        slot: TRADE_SLOT,
        target_lufs: target,
        save,
        topology_id: "guitar-humbucker",
        calibration_lufs: null,
        stimulus_path: null,
        profile_id: null,
        block_group_id: null,
        block_node_id: null,
        block_parameter_id: null,
        block_value: null,
      },
    });
    const dry = (await invoke(
      page,
      "level_preset",
      levelJob(-21, false),
      LEVEL_T,
    )) as LevelPresetResult;
    // DECOY save FIRST, deliberately far from the eventual seed point: the fixture's own
    // AUTHORED default presetLevel already sits near the seed target below (the fixture was
    // tuned for exactly this demo), so a save straight at that target hits `level_unchanged`
    // (leveller.rs) and skips the write — reporting `saved:false` HONESTLY, not a bug, but
    // leaving the sim's `ever_saved` gate (trade_sim()'s own Rust-test comment) untouched.
    // Landing somewhere clearly different first forces a REAL write+save, so the next save
    // (however close to the default) also has something to actually change FROM.
    await invoke(
      page,
      "level_preset",
      levelJob(dry.constant_c - 20, true),
      LEVEL_T,
    );
    // 4.4 LU of headroom below the ceiling ⇒ presetLevel ≈ 0.6 (20·log10(0.6) ≈ -4.44),
    // matching the Rust tests' own seed point — but LEARNED from this run's own
    // `constant_c`, never hard-coded, since the ceiling model is fixture-authored.
    const seedTarget = dry.constant_c - 4.4;
    const seed = (await invoke(
      page,
      "level_preset",
      levelJob(seedTarget, true),
      LEVEL_T,
    )) as LevelPresetResult;
    expect(seed.saved, "the seed must actually persist (ever_saved gate)").toBe(
      true,
    );
    const seededLevel = seed.final_level;

    const blocks = (await invoke(
      page,
      "list_level_blocks",
      { slot: TRADE_SLOT },
      LEVEL_T,
    )) as LevelBlock[];
    const candidates = blocks
      .filter(
        (b) => b.parameter_id === "outputLevel" && b.node_id === TRADE_AMP,
      )
      .map((b) => ({
        groupId: b.group_id,
        nodeId: b.node_id,
        parameterId: b.parameter_id,
        value: b.value,
      }));
    expect(
      candidates.length,
      "the Hiwatt amp candidate must be discoverable",
    ).toBeGreaterThan(0);

    const jobs = [
      { sceneSlot: 8, targetLufs: -21 }, // 8 = BASE_SCENE_SLOT sentinel
      { sceneSlot: TRADE_SCENE, targetLufs: -21 },
    ];
    const batch = (save: boolean) =>
      invoke(
        page,
        "level_scenes_apply_batched",
        {
          slot: TRADE_SLOT,
          jobs,
          candidates,
          save,
          rebalance: false,
          topologyId: "guitar-humbucker",
          calibrationLufs: null,
          profileId: null,
          onResult: "__CHANNEL__:0",
        },
        LEVEL_T * 2,
      ) as Promise<SceneLevelResult[]>;

    // ── Baseline AFTER the seed's own save, so the counts below are the two batches' own. ──
    const from = (await simEvents(page)).length;

    // ── Advisory FIRST (save:false) — after a LANDED trade there is no room left for a
    // second one, so the advisory half must run before the apply half spends the headroom. ──
    const advisoryRows = await batch(false);
    expect(advisoryRows.length, "base + the benefiting scene both report").toBe(
      2,
    );
    for (const r of advisoryRows) {
      expect(
        r.trade?.applied,
        `an advisory (no-save) run must disclose applied:false: ${JSON.stringify(r)}`,
      ).toBe(false);
    }
    const advisoryTrade = advisoryRows[0]?.trade;
    expect(
      advisoryTrade,
      "every row carries the same trade object",
    ).toBeTruthy();
    if (advisoryTrade) {
      expect(
        advisoryTrade.base_amps[0]?.value,
        "an advisory solves nothing — the fader response is not algebraically " +
          "predictable, so `value` must stay null",
      ).toBeNull();
      expect(
        Number.isFinite(advisoryTrade.base_amps[0]?.previous_value),
        "…but the restore anchor (what's THERE now) is always known",
      ).toBe(true);
      expect(advisoryTrade.cap).toBe("preset_level_max");
      expect(advisoryTrade.benefiting).toEqual([
        { kind: "scene", sceneSlot: TRADE_SCENE },
      ]);
      expect(
        Math.abs(advisoryTrade.previous_preset_level - seededLevel),
        "the advisory's own before-state must match what the seed actually persisted, " +
          "not a hard-coded constant",
      ).toBeLessThan(1e-3);
    }

    // ── Landed (save:true) — the F1 structural mirror. ──────────────────────────────────
    const landedRows = await batch(true);
    expect(landedRows.length, "base + the benefiting scene both level").toBe(2);
    for (const r of landedRows) {
      expect(
        r.trade?.applied,
        `a save:true run must disclose applied:true: ${JSON.stringify(r)}`,
      ).toBe(true);
    }
    const trade = landedRows[0]?.trade;
    expect(trade).toBeTruthy();
    if (trade) {
      expect(trade.cap).toBe("preset_level_max");
      expect(trade.benefiting).toEqual([
        { kind: "scene", sceneSlot: TRADE_SCENE },
      ]);
      expect(
        Math.abs(trade.previous_preset_level - seededLevel),
        "the landed run's before-state must also match the seed, not the advisory's",
      ).toBeLessThan(1e-3);
      const fader = trade.base_amps[0]?.value;
      const prevFader = trade.base_amps[0]?.previous_value;
      expect(fader, "a landed trade SOLVES the fader").not.toBeNull();
      if (fader != null) {
        expect(fader, "the compensating fader went DOWN").toBeLessThan(
          prevFader,
        );
      }
    }

    const base = sceneRow(landedRows, null);
    expect(base, "a base row").toBeTruthy();
    if (base) {
      expect(
        Math.abs(base.measured_lufs + 21),
        "base is HELD at its target — the whole point of paying with the fader",
      ).toBeLessThan(0.5);
      expect(
        base.persist_mismatch,
        "the trade's own writes are in the run's verified set",
      ).toBe(false);
    }
    const scene = sceneRow(landedRows, TRADE_SCENE);
    expect(scene, "the benefiting scene row").toBeTruthy();
    if (scene) {
      expect(
        scene.clamped,
        "the raise is trimmed by presetLevel's own ceiling — still short of -21",
      ).toBe(true);
      expect(
        scene.clamp_kind,
        "the trade LANDED, so the clamp is the ordinary headroom one",
      ).toBe("scene_ceiling");
    }

    // ── Exactly ONE save since the baseline — proof the advisory wrote nothing. ─────────
    const events = await simEvents(page);
    const savedSince = events
      .slice(from)
      .filter(isSaved)
      .filter((e) => e.Saved === TRADE_SLOT);
    expect(
      savedSince.length,
      `exactly one deferred save persists the landed batch (the advisory must write ` +
        `nothing): ${JSON.stringify(events.slice(from))}`,
    ).toBe(1);

    await expectReampBalanced(page, reampBase);
  });
});

// COVERAGE row 7 — scene routing clamp (zero authority): 403's scene 2 "Clean" pins BOTH
// lane amps' `outputLevel` overlay at 0.0, so the knob has no authority over the USB 1/2
// capture at all — a routing-class clamp, not a headroom one, and `ClampKind::NoAuthority`
// is the ONE member of the taxonomy the trade spec above cannot reach (the trade fixture's
// amp always has signal). Closes this row's own "candidate for a follow-up raw-invoke spec"
// note in COVERAGE.md.
test.describe("Level — scene clamp taxonomy: zero-authority routing clamp (raw invoke)", () => {
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
    clamp_reason: string | null;
  }

  test("403 scene 2 'Clean': both lane amps at outputLevel 0 clamp as no_authority, not scene_ceiling", async ({
    page,
  }) => {
    test.skip(
      await isOnline(page),
      "offline: pins the fixture's authored zero-authority overlay",
    );
    await ensureScenario(page);
    const slot = SCENARIO[3].slot; // 403, E2E Parallel

    const blocks = (await invoke(
      page,
      "list_level_blocks",
      { slot },
      LEVEL_T,
    )) as LevelBlock[];
    const candidates = blocks
      .filter((b) => b.parameter_id === "outputLevel")
      .map((b) => ({
        groupId: b.group_id,
        nodeId: b.node_id,
        parameterId: b.parameter_id,
        value: b.value,
      }));
    expect(
      candidates.length,
      "both lane amps must be discoverable",
    ).toBeGreaterThan(0);

    const rows = (await invoke(
      page,
      "level_scenes_apply_batched",
      {
        slot,
        jobs: [{ sceneSlot: 2, targetLufs: -21 }],
        candidates,
        save: false,
        rebalance: false,
        topologyId: "guitar-humbucker",
        calibrationLufs: null,
        profileId: null,
        onResult: "__CHANNEL__:0",
      },
      LEVEL_T,
    )) as SceneLevelResult[];
    const row = rows.find((r) => r.scene_slot === 2);
    expect(row, "scene 2 must report").toBeTruthy();
    if (row) {
      expect(row.clamped).toBe(true);
      expect(
        row.clamp_kind,
        `the zero-authority overlay must name its own cause, not the ordinary ` +
          `headroom one: ${JSON.stringify(row)}`,
      ).toBe("no_authority");
      expect(
        row.clamp_reason,
        "the routing-clamp free-text field must also be set (leveling-dsp.md's " +
          "clamp_reason contract: 'no signal on USB 1/2')",
      ).not.toBeNull();
    }
  });
});
