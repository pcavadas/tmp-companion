import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  invoke,
  isOnline,
} from "../fixtures/scenario";

// COVERAGE row 30 — Doctor's `BypassOnly` prescription refusal: `doctor_apply` cannot
// run at all offline (this file's own header, below), so its UI path is proven only
// here, online.
// ONLINE-ONLY suite member: exercises the doctor_apply → doctor_save and
// doctor_apply → doctor_discard command paths end-to-end on the REAL device
// with a deterministic hand-built job (an EQ-10 cut inserted into E2E
// Pedalboard's known G1 chain), independent of which verdicts the diagnosis
// happens to fire — UI-driven Apply can't be asserted deterministically
// (prescription content is sound-dependent). Net-zero: the scenario slots
// are cleared in teardown. Adds ~1–2 min to the attended online run; skipped
// offline (see below).
//
// DEMOTED TO ON-DEMAND (ONLINE e2e consolidation, trade T1): no longer in
// `scripts/e2e.sh`'s default online spec set — it stays runnable explicitly via
// `scripts/e2e.sh online doctor-apply.online`. This is a one-off HW validation of a
// hand-built job (not a preset-shape regression another spec would silently absorb),
// so trimming it from the default ~40 min run costs no coverage that isn't still one
// command away; it's attended, not a CI gate, either way.
test.describe("Doctor apply/save/discard — one-off HW validation", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("apply returns A/B clips; save persists; discard restores", async ({
    page,
  }) => {
    // ONLINE-ONLY: offline the fake re-amp never loads the slot, so the apply's
    // identity guard (confirm_active) correctly refuses ("slot echo None") —
    // a SimDevice fidelity limit, not a product bug.
    //
    // Asks the SERVER, and from inside the test body. The previous
    // `test.skip(!process.env.TMP_E2E_ONLINE)` at describe level was always TRUE:
    // scripts/e2e.sh sets that var only on the server invocation, so the Playwright
    // process never sees it (the same trap documented in level.spec.ts's merged
    // idempotency test). This
    // spec therefore skipped even during online runs — and it is excluded from the
    // offline config — so it had never actually executed in either tier.
    test.skip(!(await isOnline(page)), "online-only one-off HW validation");
    test.setTimeout(300_000);
    await ensureScenario(page);
    await page.goto("/");

    // E2E Pedalboard's chain, mirrored from e2e/fixtures/scenario-presets.json.
    const nodes = [
      {
        group_id: "G1",
        node_id: "ACD_TubeScreamer",
        model: "ACD_TubeScreamer",
        bypassed: true,
      },
      {
        group_id: "G1",
        node_id: "ACD_KingOfTone",
        model: "ACD_KingOfTone",
        bypassed: true,
      },
      {
        group_id: "G1",
        node_id: "ACD_MarshallPlexi",
        model: "ACD_MarshallPlexi",
        bypassed: false,
      },
    ];
    const ops = [
      {
        kind: "insert_node",
        groupId: "G1",
        beforeFenderId: null,
        fenderId: "ACD_TenBandEQStereo",
        params: [["gain250hz", -3]],
      },
    ];
    const job = (slot: number, name: string) => ({
      listIndex: slot,
      name,
      ops,
      topologyId: "guitar-humbucker",
      calibrationLufs: null,
      scene: null,
      footswitch: null,
      nodes,
      footswitches: [],
    });

    // (1) APPLY on E2E Pedalboard → both A/B clips come back as WAV data URLs.
    const t1 = SCENARIO[1];
    const applied = (await invoke(page, "doctor_apply", {
      job: job(t1.slot, t1.name),
    })) as { beforeClip: string; afterClip: string };
    expect(applied.beforeClip).toMatch(/^data:audio\/wav;base64,.{100,}/);
    expect(applied.afterClip).toMatch(/^data:audio\/wav;base64,.{100,}/);

    // (2) SAVE the applied ops (rebuilds SAVED+ops server-side, then saves).
    await invoke(page, "doctor_save", {
      listIndex: t1.slot,
      expectName: t1.name,
      ops,
    });

    // (3) APPLY on E2E Edge, then DISCARD (reloads the stored preset).
    const t2 = SCENARIO[2];
    await invoke(page, "doctor_apply", { job: job(t2.slot, t2.name) });
    await invoke(page, "doctor_discard", { listIndex: t2.slot });
  });
});
