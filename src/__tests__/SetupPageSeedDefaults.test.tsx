// src/__tests__/SetupPageSeedDefaults.test.tsx — BUG→GATEs for what Set up submits when
// the user changes nothing.
//
// #284: "Run again" / "Re-run off target" reopened Set up with every row (and both bulk
// pickers) seeded from the FIRST profile + FIRST target, so one Start click re-leveled a
// sound at the wrong loudness. A re-level row now carries what it was leveled with.
// #286: a footswitch row's scene context — which decides the scene it is measured in and
// its run rank — defaulted to base and was upgraded to the suggested enabling scene only
// if its picker had been OPENED. The default now comes from the backup scan's data.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupPage, type SetupChoice } from "../views/level/SetupPage";
import { WithCard } from "./pickCardTestUtils";
import {
  chosenFrom,
  fswKey,
  NO_INSTRUMENT,
  optionToRunItem,
  runItemToOption,
  type SetupOption,
} from "../views/level/leveling";
import { resetLibraryScan } from "../views/level/libraryScan";
import type { FootswitchInfo, FsSceneContext, SceneInfo } from "../lib/types";
import type { PickOption } from "../views/overlays/Pick";

const instrumentOptions: PickOption[] = [
  { id: NO_INSTRUMENT, label: "None" },
  { id: "tele", label: "Telecaster" },
  { id: "lp", label: "Les Paul" },
];
const targetOptions: PickOption[] = [
  { id: "Rhythm", label: "Rhythm −27" },
  { id: "Lead", label: "Lead −23" },
];

function renderSetup(options: SetupOption[], isRelevel: boolean) {
  const onStart = vi.fn<(c: SetupChoice[]) => void>();
  render(
    <ThemeProvider>
      <WithCard>
        <SetupPage
          options={options}
          isRelevel={isRelevel}
          instrumentOptions={instrumentOptions}
          targetOptions={targetOptions}
          defaultInst="tele"
          defaultTarget="Rhythm"
          onCancel={vi.fn()}
          onStart={onStart}
        />
      </WithCard>
    </ThemeProvider>,
  );
  return onStart;
}

async function start(isRelevel: boolean) {
  const user = userEvent.setup();
  if (!isRelevel)
    await user.click(screen.getByText(/I.ve backed up with Pro Control/i));
  await user.click(screen.getByRole("button", { name: /^start/i }));
  return user;
}

const baseOption: SetupOption = {
  key: "p0",
  slot: 0,
  presetName: "Plexi",
  isBase: true,
  sceneSlot: null,
  sceneName: "Whole preset",
  tag: null,
  hasScenes: false,
};

beforeEach(() => {
  resetLibraryScan();
  vi.mocked(invoke).mockClear();
});

describe("#284 — a re-run reuses each sound's own instrument + target", () => {
  it("submits the ORIGINAL target and profile, not the first ones", async () => {
    // Leveled at Lead with instrument None; the defaults are Rhythm + Telecaster.
    const leveled = {
      ...optionToRunItem(baseOption, NO_INSTRUMENT, "Lead"),
      status: "result" as const,
      outcome: "unconverged" as const,
    };
    const onStart = renderSetup([runItemToOption(leveled)], true);
    // The row shows what the sound was leveled with…
    expect(screen.getAllByText("Lead −23").length).toBeGreaterThan(0);
    expect(screen.queryByText("Rhythm −27")).not.toBeInTheDocument();
    // …and one Start click carries it into the job.
    await start(true);
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(onStart.mock.calls[0][0][0]).toMatchObject({
      instId: NO_INSTRUMENT,
      targetName: "Lead",
    });
  });

  it("keeps per-row values when the re-run rows differ", async () => {
    const a = runItemToOption({
      ...optionToRunItem(baseOption, "lp", "Lead"),
      status: "result",
    });
    const b = runItemToOption({
      ...optionToRunItem(
        { ...baseOption, key: "p1", slot: 1 },
        "tele",
        "Rhythm",
      ),
      status: "result",
    });
    const onStart = renderSetup([a, b], true);
    await start(true);
    expect(
      onStart.mock.calls[0][0].map((c) => [
        c.option.key,
        c.instId,
        c.targetName,
      ]),
    ).toEqual([
      ["p0", "lp", "Lead"],
      ["p1", "tele", "Rhythm"],
    ]);
  });

  it("falls back to the defaults for a profile/target that no longer exists", async () => {
    const gone = runItemToOption({
      ...optionToRunItem(baseOption, "deleted-profile", "Deleted target"),
      status: "result",
    });
    const onStart = renderSetup([gone], true);
    await start(true);
    expect(onStart.mock.calls[0][0][0]).toMatchObject({
      instId: "tele",
      targetName: "Rhythm",
    });
  });
});

describe("#286 — a footswitch row's scene context is data, not UI history", () => {
  const scenes: SceneInfo[] = [
    { name: "Clean", fs: 1 },
    { name: "Lead", fs: 2 },
  ];
  const footswitch = (suggested: number | null): FootswitchInfo => ({
    switch: 4,
    label: "Boost",
    link_group: null,
    functions: [],
    level_params: [
      {
        group_id: "G1",
        node_id: "ACD_Boost",
        parameter_id: "gain",
        class: "level_db",
        current: 2.5,
        fender_id: "ACD_Boost",
      },
    ],
    suggested_scene: suggested,
  });
  const optionsFor = (suggested: number | null) =>
    chosenFrom(
      new Set([fswKey(0, 0)]),
      [{ slot: 0, name: "Rig", empty: false }],
      new Map([[0, scenes]]),
      new Map([[0, [footswitch(suggested)]]]),
    );

  // The lazy picker read answers "scene 1" — the value the OLD code switched to only once
  // the picker had been opened.
  function mockPickerRead() {
    const live: FsSceneContext[] = [
      { switch: 4, enablingScenes: [1], suggested: 1 },
    ];
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "list_footswitch_scene_contexts" ? live : null),
    );
  }

  async function submittedContext(
    suggested: number | null,
    openPicker: boolean,
  ): Promise<number | null | undefined> {
    mockPickerRead();
    const onStart = renderSetup(optionsFor(suggested), false);
    const user = userEvent.setup();
    if (openPicker) {
      const trigger = document.querySelector('[data-pick="fsctx:4"]');
      if (!(trigger instanceof HTMLElement)) throw new Error("no fsctx picker");
      await user.click(trigger);
      await screen.findByText("Clean");
      // The lazy read really ran (and settled) — so the old picker-driven upgrade
      // would have had its answer before Start.
      await waitFor(() => {
        expect(
          vi
            .mocked(invoke)
            .mock.calls.some((c) => c[0] === "list_footswitch_scene_contexts"),
        ).toBe(true);
      });
      await act(async () => {
        await Promise.resolve();
      });
      await user.click(trigger); // close it again, choosing nothing
    }
    await start(false);
    const sent = onStart.mock.calls[0][0][0];
    document.body.innerHTML = "";
    return sent.option.footswitch?.sceneContext;
  }

  it.each([null, 1])(
    "opening the picker or not submits the same context (scan suggests %j)",
    async (suggested) => {
      const untouched = await submittedContext(suggested, false);
      const opened = await submittedContext(suggested, true);
      expect(opened).toBe(untouched);
      // …and that context is the scan's own data.
      expect(untouched).toBe(suggested);
    },
  );

  it("the row is ranked by that context from the start", () => {
    // A scene-context switch runs after the scenes (rank 3); a base one before (rank 1).
    expect(optionsFor(1)[0].footswitch?.sceneContext).toBe(1);
    expect(optionsFor(null)[0].footswitch?.sceneContext).toBeNull();
  });
});
