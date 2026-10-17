// Settings — the never-silent load/save (#300) and click-only reorder + slider
// cancel (#302), on the look B pages.
//
//   • #300: a failed get_store shows a danger Notice with "Try again" and offers no Add
//     (an add against the initial empty arrays would overwrite the whole stored list);
//     a failed save_targets is reported and the list re-reads to what is stored. The
//     page names what failed, never the backend's text (that goes to the log). On
//     Instruments with the unit away, the failed-save Notice also says why Record is
//     off, so the page shows one Notice, not two.
//   • #302: targets reorder by ⋯ Move up / Move down; the slider's pointercancel
//     ends the drag without committing and restores the starting value.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SettingsView } from "../views/settings";
import { TargetRow } from "../views/settings/TargetRow";
import type { UpdaterApi } from "../lib/useUpdater";
import type { Store, Target } from "../lib/types";
import { callsOf, makeStore, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const updater: UpdaterApi = {
  phase: "idle",
  version: null,
  notes: null,
  percent: 0,
  autoInstall: false,
  currentVersion: "1.2.3",
  hasUpdateChannel: true,
  setAutoInstall: vi.fn(),
  check: vi.fn(() => Promise.resolve("none" as const)),
  review: vi.fn(),
  cancelReview: vi.fn(),
  startDownload: vi.fn(),
  dismiss: vi.fn(),
  retry: vi.fn(),
  restart: vi.fn(),
};

const store = (targets: Target[]) => makeStore({ targets });

function renderView() {
  renderWithTheme(<SettingsView connected={false} updater={updater} />);
}

const savedTargets = () => callsOf("save_targets").map((a) => a.targets);

describe("SettingsView — load and save are never silent (#300)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("a failed load shows the error with Retry and offers no Add; Retry loads", async () => {
    let fail = true;
    mockCommands({
      get_store: () => {
        if (fail) throw new Error("store unreadable");
        return store([{ name: "Rhythm", lufs: -26 }]);
      },
    });
    renderView();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t read your settings.");
    expect(alert).not.toHaveTextContent(/store unreadable/);
    expect(screen.getByRole("button", { name: "Add target" })).toBeDisabled();
    expect(screen.queryByText(/No targets yet/)).toBeNull();

    fail = false;
    await userEvent.click(
      within(alert).getByRole("button", { name: "Try again" }),
    );
    expect(await screen.findByText("Rhythm")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add target" })).toBeEnabled();
    expect(screen.queryByRole("alert")).toBeNull();
    // Nothing was ever saved over the unread list.
    expect(savedTargets()).toHaveLength(0);
  });

  it("a failed save is reported and the list re-reads to what is stored", async () => {
    mockCommands({
      get_store: () => store([{ name: "Rhythm", lufs: -26 }]),
      save_targets: () => {
        throw new Error("disk full");
      },
    });
    renderView();
    await screen.findByText("Rhythm");

    await userEvent.click(screen.getByRole("button", { name: "Add target" }));
    // The optimistic row appears, the save fails, the alert says so…
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Couldn’t save your loudness targets. Showing what’s saved.",
    );
    expect(alert).not.toHaveTextContent(/disk full/);
    // …and the re-read drops the row that never landed.
    await vi.waitFor(() => {
      expect(screen.queryByDisplayValue("New target")).toBeNull();
    });
    expect(screen.getByText("Rhythm")).toBeInTheDocument();
  });

  it("a failed instrument save while the unit is away shows one Notice saying both", async () => {
    const profile = (id: string, name: string) => ({
      id,
      name,
      topology_id: "guitar-singlecoil",
      calibration_lufs: null,
    });
    mockCommands({
      get_store: () =>
        makeStore({ profiles: [profile("a", "Strat"), profile("b", "Tele")] }),
      save_profiles: () => {
        throw new Error("disk full");
      },
    });
    renderView();
    const user = userEvent.setup();
    const nav = await screen.findByRole("navigation", {
      name: "Settings categories",
    });
    await user.click(within(nav).getByRole("button", { name: "Instruments" }));
    await screen.findByText("Strat");
    // Unit away, nothing failed yet: the page's own Notice says why Record is off.
    expect(
      screen.getByText("Recording needs your Tone Master Pro connected."),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "More for Strat" }));
    await user.click(screen.getByRole("menuitem", { name: "Move down" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Couldn’t save your instruments. Showing what’s saved. Recording needs your Tone Master Pro connected.",
    );
    expect(alert).not.toHaveTextContent(/disk full/);
    // One Notice on the page: the danger one, saying both.
    expect(screen.queryAllByRole("alert")).toHaveLength(1);
    expect(
      screen.queryByText("Recording needs your Tone Master Pro connected."),
    ).toBeNull();
    expect(screen.queryAllByRole("status")).toHaveLength(0);
  });

  it("a recovery read overtaken by a newer save keeps that save's list", async () => {
    let reads = 0;
    let releaseRead: () => void = () => undefined;
    let saves = 0;
    let releaseSave: () => void = () => undefined;
    const stored = store([{ name: "Rhythm", lufs: -26 }]);
    mockCommands({
      get_store: () => {
        reads++;
        if (reads === 1) return stored;
        // The recovery read stays pending until released.
        return new Promise<Store>((res) => {
          releaseRead = () => {
            res(stored);
          };
        });
      },
      save_targets: () => {
        saves++;
        if (saves === 1) throw new Error("disk full");
        return new Promise<undefined>((res) => {
          releaseSave = () => {
            res(undefined);
          };
        });
      },
    });
    renderView();
    await screen.findByText("Rhythm");

    // Save 1 fails → a recovery read starts and stays pending.
    await userEvent.click(screen.getByRole("button", { name: "Add target" }));
    await screen.findByRole("alert");
    await vi.waitFor(() => {
      expect(reads).toBe(2);
    });
    // Save 2 starts while that read is pending (Rhythm + 2 new rows).
    await userEvent.click(screen.getByRole("button", { name: "Add target" }));
    const all = savedTargets();
    expect(all[all.length - 1]).toHaveLength(3);

    // The stale read lands, then save 2 succeeds: its list stays.
    releaseRead();
    releaseSave();
    await vi.waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
    expect(screen.getByText("Rhythm")).toBeInTheDocument();
    expect(
      screen.getAllByText("New target").length +
        screen.queryAllByDisplayValue("New target").length,
    ).toBe(2);
  });
});

describe("Settings — click-only reorder and slider cancel (#302)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("targets reorder with ⋯ Move down / Move up (no drag needed)", async () => {
    mockCommands({
      get_store: () =>
        store([
          { name: "Rhythm", lufs: -26 },
          { name: "Lead", lufs: -22 },
        ]),
    });
    renderView();
    await screen.findByText("Rhythm");
    const user = userEvent.setup();

    // First row: Move down offered, Move up not (it's the top).
    await user.click(screen.getByRole("button", { name: "More for Rhythm" }));
    expect(screen.queryByRole("menuitem", { name: "Move up" })).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: "Move down" }));
    expect(savedTargets().pop()).toEqual([
      { name: "Lead", lufs: -22 },
      { name: "Rhythm", lufs: -26 },
    ]);

    // Rhythm is now second: Move up puts it back.
    await user.click(screen.getByRole("button", { name: "More for Rhythm" }));
    expect(screen.queryByRole("menuitem", { name: "Move down" })).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: "Move up" }));
    expect(savedTargets().pop()).toEqual([
      { name: "Rhythm", lufs: -26 },
      { name: "Lead", lufs: -22 },
    ]);
  });

  it("the slider's pointercancel ends the drag without committing", () => {
    const onChange = vi.fn();
    const onCommit = vi.fn();
    renderWithTheme(
      <TargetRow
        name="Rhythm"
        lufs={-21}
        onRename={vi.fn()}
        onChange={onChange}
        onCommit={onCommit}
        onDelete={vi.fn()}
        onGrab={vi.fn()}
        onDropOn={vi.fn()}
        onDragEnd={vi.fn()}
      />,
    );
    const track = screen.getByRole("slider", { name: "Rhythm target" });
    // A 160 px track: 10 px per LU over the −29…−13 domain.
    track.getBoundingClientRect = () =>
      ({ left: 0, width: 160, top: 0, height: 5 }) as DOMRect;

    fireEvent.pointerDown(track, { clientX: 40, pointerId: 1 });
    fireEvent.pointerMove(track, { clientX: 60, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(-23);
    fireEvent.pointerCancel(track, { clientX: 60, pointerId: 1 });
    expect(onCommit).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenLastCalledWith(-21); // back where it started
    // A later move with no button down does nothing.
    fireEvent.pointerMove(track, { clientX: 100, pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(-21);

    // A normal drag still commits once, on release.
    fireEvent.pointerDown(track, { clientX: 40, pointerId: 2 });
    fireEvent.pointerUp(track, { clientX: 120, pointerId: 2 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(-17);
  });
});
