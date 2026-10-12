// Settings — the never-silent load/save (#300) and click-only reorder + slider
// cancel (#302).
//
//   • #300: a failed get_store shows an error with Retry and offers no Add (an add
//     against the initial empty arrays would overwrite the whole stored list); a
//     failed save_targets is reported and the list re-reads to what is stored.
//   • #302: targets reorder by ⋯ Move up / Move down; the slider's pointercancel
//     ends the drag without committing and restores the starting value; the
//     instrument row carries no fake drag grip.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SettingsView } from "../views/settings";
import { TargetRow } from "../views/settings/TargetRow";
import type { UpdaterApi } from "../lib/useUpdater";
import type { Target } from "../lib/types";

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

const store = (targets: Target[]) => ({
  profiles: [],
  profile_by_slot: {},
  targets,
  playback_level: "stage",
});

function renderView() {
  render(
    <ThemeProvider>
      <SettingsView connected={false} updater={updater} />
    </ThemeProvider>,
  );
}

const savedTargets = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([c]) => c === "save_targets")
    .map(([, args]) => (args as { targets: Target[] }).targets);

describe("SettingsView — load and save are never silent (#300)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("a failed load shows the error with Retry and offers no Add; Retry loads", async () => {
    let fail = true;
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_store")
        return fail
          ? Promise.reject(new Error("store unreadable"))
          : Promise.resolve(store([{ name: "Rhythm", lufs: -26 }]));
      if (command === "list_pickup_topologies") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    renderView();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/store unreadable/);
    expect(screen.queryByText("Add target")).toBeNull();
    expect(screen.queryByText(/No targets yet/)).toBeNull();

    fail = false;
    await userEvent.click(within(alert).getByText("Retry"));
    expect(await screen.findByText("Rhythm")).toBeInTheDocument();
    expect(screen.getByText("Add target")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    // Nothing was ever saved over the unread list.
    expect(savedTargets()).toHaveLength(0);
  });

  it("a failed save is reported and the list re-reads to what is stored", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_store")
        return Promise.resolve(store([{ name: "Rhythm", lufs: -26 }]));
      if (command === "list_pickup_topologies") return Promise.resolve([]);
      if (command === "save_targets")
        return Promise.reject(new Error("disk full"));
      return Promise.resolve(null);
    });
    renderView();
    await screen.findByText("Rhythm");

    await userEvent.click(screen.getByText("Add target"));
    // The optimistic row appears, the save fails, the alert says so…
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /Couldn.t save your loudness targets: disk full/,
    );
    // …and the re-read drops the row that never landed.
    await vi.waitFor(() => {
      expect(screen.queryByDisplayValue("New target")).toBeNull();
    });
    expect(screen.getByText("Rhythm")).toBeInTheDocument();
  });

  it("a recovery read overtaken by a newer save keeps that save's list", async () => {
    let reads = 0;
    let releaseRead: () => void = () => undefined;
    let saves = 0;
    let releaseSave: () => void = () => undefined;
    vi.mocked(invoke).mockImplementation((command: string) => {
      const stored = store([{ name: "Rhythm", lufs: -26 }]);
      if (command === "get_store") {
        reads++;
        if (reads === 1) return Promise.resolve(stored);
        // The recovery read stays pending until released.
        return new Promise((res) => {
          releaseRead = () => {
            res(stored);
          };
        });
      }
      if (command === "list_pickup_topologies") return Promise.resolve([]);
      if (command === "save_targets") {
        saves++;
        if (saves === 1) return Promise.reject(new Error("disk full"));
        return new Promise((res) => {
          releaseSave = () => {
            res(null);
          };
        });
      }
      return Promise.resolve(null);
    });
    renderView();
    await screen.findByText("Rhythm");

    // Save 1 fails → a recovery read starts and stays pending.
    await userEvent.click(screen.getByText("Add target"));
    await screen.findByRole("alert");
    await vi.waitFor(() => {
      expect(reads).toBe(2);
    });
    // Save 2 starts while that read is pending (Rhythm + 2 new rows).
    await userEvent.click(screen.getByText("Add target"));
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
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_store")
        return Promise.resolve(
          store([
            { name: "Rhythm", lufs: -26 },
            { name: "Lead", lufs: -22 },
          ]),
        );
      if (command === "list_pickup_topologies") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    renderView();
    await screen.findByText("Rhythm");
    const user = userEvent.setup();

    // First row: Move down offered, Move up not (it's the top).
    await user.click(screen.getAllByTitle("More")[0]);
    expect(screen.queryByText("Move up")).toBeNull();
    await user.click(screen.getByText("Move down"));
    expect(savedTargets().pop()).toEqual([
      { name: "Lead", lufs: -22 },
      { name: "Rhythm", lufs: -26 },
    ]);

    // Rhythm is now second: Move up puts it back.
    await user.click(screen.getAllByTitle("More")[1]);
    expect(screen.queryByText("Move down")).toBeNull();
    await user.click(screen.getByText("Move up"));
    expect(savedTargets().pop()).toEqual([
      { name: "Rhythm", lufs: -26 },
      { name: "Lead", lufs: -22 },
    ]);
  });

  it("the slider's pointercancel ends the drag without committing", () => {
    const onChange = vi.fn();
    const onCommit = vi.fn();
    render(
      <ThemeProvider>
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
        />
      </ThemeProvider>,
    );
    const track = screen.getByTitle("Drag to set level");
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
