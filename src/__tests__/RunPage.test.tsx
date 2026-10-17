// src/__tests__/RunPage.test.tsx — the leveling run page (look B): a constant title, the
// now line naming the sound in progress, one LoudnessMeter against its target (no
// numbers anywhere), "N of M done", a header per preset and one row per sound with its
// mark, target and state word, and Stop behind a "Stop leveling?" dialog that a stray
// click can't dismiss. Backend captions never reach the screen.

import { beforeEach, describe, it, expect, vi } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";

import { RunPage } from "../views/level/RunPage";
import type { RunPhase } from "../views/level/levelRunStore";
import {
  instrumentOptions,
  NO_INSTRUMENT,
  type RunItem,
} from "../views/level/leveling";
import { IDLE_METER, setLiveMeter } from "../views/level/liveMeterStore";
import { renderWithTheme } from "./testKit";

beforeEach(() => {
  setLiveMeter(IDLE_METER);
});

const sound = (over: Partial<RunItem>): RunItem => ({
  key: "s27:1",
  slot: 27,
  presetName: "E2E Hiwatt 3S",
  isBase: false,
  sceneSlot: 1,
  sceneName: "Rhythm Crunch",
  instId: "",
  targetName: "Stage",
  status: "queued",
  ...over,
});
const active = sound({ status: "active" });

/** The page with the live meter (its own store) reading `liveLufs`. */
function runPage(
  items: RunItem[],
  over: {
    liveLufs?: number | null;
    phase?: RunPhase;
    tailMessage?: string | null;
    onStop?: () => void;
    onComplete?: () => void;
  } = {},
): ReactElement {
  act(() => {
    setLiveMeter({ lufs: over.liveLufs ?? null });
  });
  return (
    <RunPage
      items={items}
      currentIndex={items.filter((x) => x.status === "result").length}
      total={items.length}
      phase={over.phase ?? "running"}
      tailMessage={over.tailMessage ?? null}
      targetLufsByName={(name) => (name === "Lead" ? -19 : -26)}
      onStop={over.onStop ?? vi.fn()}
      onComplete={over.onComplete ?? vi.fn()}
    />
  );
}

const rowOf = (key: string) => {
  const el = document.querySelector(`[data-run-row-key="${key}"]`);
  if (!(el instanceof HTMLElement)) throw new Error(`no run row ${key}`);
  return el;
};

describe("RunPage: the sound in progress", () => {
  it("a constant title, the now line, the meter against the target, and progress", () => {
    renderWithTheme(
      runPage(
        [
          sound({ key: "a", status: "result", outcome: "done", value: -26 }),
          active,
          sound({ key: "c" }),
        ],
        { liveLufs: -23.1 },
      ),
    );
    expect(
      screen.getByRole("heading", { name: "Leveling 3 sounds" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Level")).toHaveAttribute("aria-current", "step");
    expect(
      screen.getByText("Listening to Rhythm Crunch in E2E Hiwatt 3S"),
    ).toBeInTheDocument();
    const meter = screen.getByRole("meter", {
      name: "Loudness of Rhythm Crunch now, against its target",
    });
    expect(meter).toHaveAttribute("aria-valuetext", "Louder than Stage");
    expect(screen.getByText("Quieter")).toBeInTheDocument();
    expect(screen.getByText("Louder")).toBeInTheDocument();
    expect(
      screen.getByRole("progressbar", { name: "Sounds done" }),
    ).toHaveAttribute("aria-valuenow", "1");
    expect(screen.getByText("1 of 3 done")).toBeInTheDocument();
    // No loudness number, no LUFS anywhere on the run.
    expect(screen.queryByText(/LUFS/)).toBeNull();
    expect(screen.queryByText(/[−-]\d+\.\d/)).toBeNull();
  });

  // GATE (user report, preset 30 "Plumes+BD2+OCD", registry 2026-08-20): the ceiling
  // prepass measures every footswitch row before anything is solved, and one row used to
  // hold the highlight while the unit stepped through four. The highlight now follows the
  // row the backend announces, and the now line names THAT sound; its caption ("measuring")
  // is the backend's and never reaches the screen.
  it("follows the row the backend announces, in the now line and the row", () => {
    const items = [
      sound({ key: "f0", sceneName: "Pinions" }),
      sound({ key: "f1", sceneName: "Sapphire OD", status: "active" }),
    ];
    renderWithTheme(runPage(items, { liveLufs: -18.9 }));
    expect(
      screen.getByText("Listening to Sapphire OD in E2E Hiwatt 3S"),
    ).toBeInTheDocument();
    expect(within(rowOf("f1")).getByText("Listening")).toBeInTheDocument();
    expect(within(rowOf("f0")).getByText("Waiting")).toBeInTheDocument();
    expect(screen.getAllByText("Listening")).toHaveLength(1);
    expect(screen.queryByText(/measuring|leveling/)).toBeNull();
  });

  // Issue 6b (registry 2026-08-29): a batch's save/verify tail has no row of its own, so the
  // page must not look idle; it says the preset is finishing, never the backend's word.
  it("a batch's save tail reads as the preset finishing", () => {
    renderWithTheme(
      runPage(
        [sound({ key: "a", status: "result", outcome: "done", value: -26 })],
        { tailMessage: "Saving…" },
      ),
    );
    expect(screen.getByText("Finishing E2E Hiwatt 3S")).toBeInTheDocument();
    expect(screen.queryByText("Saving…")).toBeNull();
  });
});

describe("RunPage: the run list", () => {
  it("a header per preset (name and slot), the sound's own name and target on its row", () => {
    renderWithTheme(
      runPage([
        active,
        sound({
          key: "f27:0",
          sceneName: "Boost",
          sceneSlot: null,
          footswitch: {
            switchIndex: 2,
            levGroupId: "G1",
            levNodeId: "boost",
            levParameterId: "level",
            sceneContext: null,
          },
        }),
      ]),
    );
    expect(screen.getByText("E2E Hiwatt 3S")).toBeInTheDocument();
    expect(screen.getByText("028")).toBeInTheDocument();
    expect(
      within(rowOf("s27:1")).getByText("Rhythm Crunch"),
    ).toBeInTheDocument();
    expect(within(rowOf("s27:1")).getByText("Stage")).toBeInTheDocument();
    expect(
      within(rowOf("f27:0")).getByText("footswitch 3"),
    ).toBeInTheDocument();
  });

  // Every outcome's word comes from typed fields; the fallthrough must never read "On
  // target" for a miss (an unconverged row once reported as leveled).
  it.each<[string, Partial<RunItem>, string]>([
    ["done", { outcome: "done", value: -26 }, "On target"],
    [
      "full volume",
      { outcome: "clamped", value: -28, clampKind: "scene_ceiling" },
      "At full volume",
    ],
    [
      "mix floor",
      { outcome: "clamped", value: -20, clampKind: "wet_floor" },
      "Can't go quieter",
    ],
    [
      "knob bottom",
      { outcome: "clamped", value: -20, clampKind: "scene_ceiling" },
      "Can't go quieter",
    ],
    [
      "trade backed out",
      { outcome: "clamped", value: -28, clampKind: "partial_trade" },
      "Couldn't level it",
    ],
    [
      "off target",
      { outcome: "unconverged", value: -24 },
      "Didn't reach its target",
    ],
    [
      "not confirmed",
      { outcome: "unconverged", unverified: true },
      "Not confirmed",
    ],
    ["can't hear it", { outcome: "offbranch" }, "Can't hear it"],
    [
      "skipped",
      { outcome: "skipped", skipKind: "other", skipReason: "capture failed" },
      "Couldn't level it",
    ],
    [
      "stopped in flight",
      { outcome: "skipped", skipKind: "cancelled" },
      "Not reached",
    ],
    [
      "save didn't hold",
      { outcome: "done", value: -26, persistMismatch: true },
      "Save didn't hold",
    ],
  ])("%s → its word", (_, over, word) => {
    renderWithTheme(runPage([sound({ status: "result", ...over })]));
    expect(within(rowOf("s27:1")).getByText(word)).toBeInTheDocument();
    // The backend's reason goes to the log, never the row.
    expect(screen.queryByText(/capture failed/)).toBeNull();
  });
});

// BUG→GATE #287: picking instrument "None" stored the `"none"` sentinel, and a run row
// showed it raw. Look B shows no instrument on the run at all; the only place an
// instrument is shown is its Select (Level's Set up, Doctor's Instrument page), whose
// options come from `instrumentOptions`, which names the sentinel "None".
describe("RunPage row label for the no-instrument choice (#287)", () => {
  it("shows no raw id for a sound leveled with instrument None", () => {
    renderWithTheme(runPage([sound({ instId: NO_INSTRUMENT })]));
    expect(screen.queryByText(/\bnone\b/)).toBeNull();
  });

  it("names the no-instrument choice instead of echoing its id", () => {
    const none = instrumentOptions([]).find((o) => o.value === NO_INSTRUMENT);
    expect(none?.label).toBe("None");
    expect(
      instrumentOptions([
        {
          id: "p1",
          name: "Strat",
          topology_id: "guitar-single",
          calibration_lufs: null,
        },
      ]).map((o) => o.label),
    ).toEqual(["None", "Strat"]);
  });
});

describe("RunPage: Stop", () => {
  it("asks first; a click outside does nothing; Keep leveling carries on", async () => {
    const onStop = vi.fn();
    renderWithTheme(runPage([active], { onStop }));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Stop leveling?" });
    expect(dialog).toHaveTextContent("Everything matched so far stays saved.");
    // The scrim is inert: only the dialog's own buttons close it.
    const scrim = document.querySelector("[data-backdrop]");
    if (!(scrim instanceof HTMLElement)) throw new Error("no scrim");
    await user.click(scrim);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Keep leveling" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onStop).not.toHaveBeenCalled();
  });

  it("Stop in the dialog stops the run", async () => {
    const onStop = vi.fn();
    renderWithTheme(runPage([active], { onStop }));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Stop" }),
    );
    expect(onStop).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("while stopping: the now line says after which sound, and Stop is busy", () => {
    renderWithTheme(runPage([active], { phase: "stopping" }));
    expect(
      screen.getByText("Stopping after Rhythm Crunch in E2E Hiwatt 3S"),
    ).toBeInTheDocument();
    expect(within(rowOf("s27:1")).getByText("Stopping")).toBeInTheDocument();
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toBeDisabled();
    expect(stop).toHaveAttribute("aria-busy", "true");
  });

  it("the dialog closes when the run ends while it is open", async () => {
    const view = renderWithTheme(runPage([active]));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    view.rerender(
      runPage([sound({ status: "result", outcome: "done", value: -26 })], {
        phase: "done",
      }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  });

  it("a natural finish goes on to the Summary after a beat", () => {
    vi.useFakeTimers();
    try {
      const onComplete = vi.fn();
      renderWithTheme(
        runPage([sound({ status: "result", outcome: "done", value: -26 })], {
          phase: "done",
          onComplete,
        }),
      );
      expect(onComplete).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(700);
      });
      expect(onComplete).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});
