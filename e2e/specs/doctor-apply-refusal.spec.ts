import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  ensureScenario,
  invoke,
  isOnline,
  simEvents,
} from "../fixtures/scenario";

// COVERAGE row 30, through the real command: a Doctor apply into a scene whose overlay
// for the target node is bypass-only refuses BEFORE touching the device. E2E Rig (400)
// scene 3 carries `ACD_Boost` as a bypass-only overlay while base runs it un-bypassed,
// so a `gain` write there would leak into base: `scene_write_verdict_for_param` refuses
// it, and `doctor_apply` surfaces that reason verbatim.
//
// The oracle is the rejection plus the fake's wire log: the refusal must come after the
// one field-8 read and before any capture, edit or save, so `/sim/events` gains no
// re-amp toggle, parameter write, bypass, load or save. The policy itself is pinned by
// the `bypass_only_conflict` units in `commands/doctor_tests.rs`; this spec proves the
// command reaches that policy first. Offline only: the wire log is the fake's.
test.describe("Doctor apply — bypass-only scene refusal (400 scene 3, offline)", () => {
  test("refuses with the policy's reason and writes nothing", async ({
    page,
  }) => {
    test.skip(await isOnline(page), "the wire-log oracle is offline-only");
    await ensureScenario(page);
    const rig = SCENARIO[0];
    expect(rig.slot).toBe(400);

    const before = (await simEvents(page)).length;
    const job = {
      listIndex: rig.slot,
      name: rig.name,
      ops: [
        {
          kind: "param",
          groupId: "G1",
          nodeId: "ACD_Boost",
          param: "gain",
          value: 4.0,
        },
      ],
      topologyId: "guitar-humbucker",
      calibrationLufs: null,
      scene: 3,
      footswitch: null,
      nodes: [],
      footswitches: [],
    };

    const err = await invoke(page, "doctor_apply", { job }).then(
      () => null,
      (e: unknown) => String(e),
    );
    expect(
      err,
      "doctor_apply must refuse the bypass-only scene write",
    ).not.toBeNull();
    expect(err).toContain("ACD_Boost");

    const written = (await simEvents(page)).slice(before).filter((e) => {
      const kind =
        typeof e === "string"
          ? e
          : Object.keys(e as Record<string, unknown>)[0];
      return [
        "ReAmp",
        "ChangeParameter",
        "Bypass",
        "SceneEdit",
        "Insert",
        "Replace",
        "Remove",
        "PresetLevel",
        "LoadScene",
        "Loaded",
        "Saved",
      ].includes(kind);
    });
    expect(written, "the refusal must precede every device write").toEqual([]);
  });
});
