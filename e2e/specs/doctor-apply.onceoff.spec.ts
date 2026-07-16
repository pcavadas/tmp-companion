import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  invoke,
} from "../fixtures/scenario";

// ONE-OFF attended validation (UNTRACKED — run explicitly, then delete):
// exercises the doctor_apply → doctor_save and doctor_apply → doctor_discard
// command paths end-to-end on the REAL device with a deterministic hand-built
// job (an EQ-10 cut inserted into E2E Target 1's known G1 chain), independent
// of which verdicts the diagnosis happens to fire. Net-zero: the scenario
// slots are cleared in teardown.
test.describe("Doctor apply/save/discard — one-off HW validation", () => {
  // ONLINE-ONLY: offline the fake re-amp never loads the slot, so the apply's
  // identity guard (confirm_active) correctly refuses ("slot echo None") —
  // a SimDevice fidelity limit, not a product bug.
  test.skip(!process.env.TMP_E2E_ONLINE, "online-only one-off HW validation");
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("apply returns A/B clips; save persists; discard restores", async ({
    page,
  }) => {
    test.setTimeout(300_000);
    await ensureScenario(page);
    await page.goto("/");

    // Target 1's chain, mirrored from e2e/fixtures/scenario-presets.json.
    const nodes = [
      {
        group_id: "G1",
        node_id: "ACD_TubeScreamer",
        model: "ACD_TubeScreamer",
        bypassed: true,
      },
      {
        group_id: "G1",
        node_id: "ACD_PhaserP90",
        model: "ACD_PhaserP90",
        bypassed: true,
      },
      {
        group_id: "G1",
        node_id: "ACD_TMLargePlate",
        model: "ACD_TMLargePlate",
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

    // (1) APPLY on Target 1 → both A/B clips come back as WAV data URLs.
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

    // (3) APPLY on Target 2, then DISCARD (reloads the stored preset).
    const t2 = SCENARIO[2];
    await invoke(page, "doctor_apply", { job: job(t2.slot, t2.name) });
    await invoke(page, "doctor_discard", { listIndex: t2.slot });
  });
});
