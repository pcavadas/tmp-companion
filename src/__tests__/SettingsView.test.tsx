// Settings view — the user-owned loudness targets (handoff design_handoff_
// settings_targets) + the Playback level segmented control + the App updates
// section.
//
//   • Targets render from the store; "Add target" appends a row in rename mode
//     and persists via save_targets (no ceiling clamp — value seeds at −19.0,
//     PR2 re-baseline: +3 from the mono-era −22.0).
//   • Deleting the last target shows the empty-state line.
//   • The Playback level control persists the picked level via set_playback_level.
//   • App updates: version readout, manual check → "up to date", auto-install
//     toggle — driven entirely off the injected `updater` prop (no invoke calls).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SettingsView } from "../views/settings";
import type { UpdaterApi } from "../lib/useUpdater";
import type { CommandName } from "../lib/commands";
import type { PlaybackLevel } from "../lib/types";
import { callsOf, makeStore, mockCommands } from "./commandMocks";

const SEED_TARGETS = [
  { name: "Rhythm", lufs: -26.0 },
  { name: "Crunch", lufs: -24.0 },
  { name: "Lead", lufs: -22.0 },
];

// get_store returns the seed targets; everything else keeps the setup.ts empties.
function mockStore(targets = SEED_TARGETS, playback: PlaybackLevel = "stage") {
  mockCommands({
    get_store: () => makeStore({ targets, playback_level: playback }),
  });
}

// Named refs (not accessed through the interface-typed object) so assertions
// don't trip `@typescript-eslint/unbound-method` on the interface's method
// signatures.
const checkFn = vi.fn<() => Promise<"found" | "none" | "error">>();
const setAutoInstallFn = vi.fn<(on: boolean) => void>();

function makeStubUpdater(): UpdaterApi {
  checkFn.mockReset().mockResolvedValue("none");
  setAutoInstallFn.mockReset();
  return {
    phase: "idle",
    version: null,
    notes: null,
    percent: 0,
    autoInstall: true,
    currentVersion: "1.2.3",
    hasUpdateChannel: true,
    setAutoInstall: setAutoInstallFn,
    check: checkFn,
    review: vi.fn(),
    cancelReview: vi.fn(),
    startDownload: vi.fn(),
    dismiss: vi.fn(),
    retry: vi.fn(),
    restart: vi.fn(),
  };
}

function renderView(updaterOverrides: Partial<UpdaterApi> = {}) {
  render(
    <ThemeProvider>
      <SettingsView
        connected={false}
        updater={{ ...makeStubUpdater(), ...updaterOverrides }}
      />
    </ThemeProvider>,
  );
}

// The rail shows one category pane at a time (default "targets"). Click a rail
// tab to swap panes — role-based, so the pane header's duplicate of the active
// label can never make the query ambiguous.
async function openCategory(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
) {
  await user.click(screen.getByRole("tab", { name: label }));
}

const lastArgs = <K extends CommandName>(command: K) => {
  const calls = callsOf(command);
  return calls.length ? calls[calls.length - 1] : undefined;
};

describe("SettingsView — loudness targets", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("renders the seed targets and their values", async () => {
    renderView();
    expect(await screen.findByText("Rhythm")).toBeInTheDocument();
    expect(screen.getByText("Crunch")).toBeInTheDocument();
    expect(screen.getByText("Lead")).toBeInTheDocument();
    expect(screen.getByText("-26.0")).toBeInTheDocument();
    expect(screen.getByText("-22.0")).toBeInTheDocument();
  });

  it("adds a target (rename mode + persists via save_targets)", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");

    await user.click(screen.getByRole("button", { name: /add target/i }));

    // The new row opens in rename mode: an input pre-filled with "New target".
    const input = await screen.findByDisplayValue("New target");
    expect(input).toBeInTheDocument();

    // Persisted: a 4th target seeded at −19.0 (PR2 re-baseline), no clamp.
    const saved = lastArgs("save_targets");
    expect(saved?.targets).toHaveLength(4);
    expect(saved?.targets[3]).toEqual({ name: "New target", lufs: -19.0 });
  });

  it("shows the empty state once every target is deleted", async () => {
    const user = userEvent.setup();
    mockStore([{ name: "Only", lufs: -20.0 }]);
    renderView();
    await screen.findByText("Only");

    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Delete"));

    expect(await screen.findByText(/no targets yet/i)).toBeInTheDocument();
    expect(lastArgs("save_targets")?.targets).toHaveLength(0);
  });
});

describe("SettingsView — playback level", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("persists the picked level via set_playback_level", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    await openCategory(user, "Playback level");

    const group = screen.getByRole("radiogroup", { name: /playback level/i });
    await user.click(within(group).getByRole("radio", { name: "Rehearsal" }));

    await waitFor(() => {
      expect(lastArgs("set_playback_level")).toEqual({ level: "rehearsal" });
    });
  });

  it("reflects the stored level + its compensation caption", async () => {
    const user = userEvent.setup();
    mockStore(SEED_TARGETS, "quiet");
    renderView();
    await screen.findByText("Rhythm");
    await openCategory(user, "Playback level");

    const group = await screen.findByRole("radiogroup", {
      name: /playback level/i,
    });
    expect(within(group).getByRole("radio", { name: "Quiet" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    // Quiet → the bass +1.5 LU chip (mirrors profiles::playback_offset_lu).
    expect(screen.getByText("bass +1.5 LU")).toBeInTheDocument();
  });
});

describe("SettingsView — app updates", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("renders the version readout, check button, and auto-install label", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    await openCategory(user, "About & updates");

    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /check for updates/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Install updates automatically"),
    ).toBeInTheDocument();
  });

  it("checks for updates and shows up to date", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    await openCategory(user, "About & updates");
    await screen.findByText("Version 1.2.3");

    await user.click(
      screen.getByRole("button", { name: /check for updates/i }),
    );

    expect(checkFn).toHaveBeenCalled();
    expect(await screen.findByText("up to date")).toBeInTheDocument();
  });

  // Linux ships .deb/.rpm and latest.json carries only darwin-* keys, so a check
  // there can only ever come back empty. Reporting that as "up to date" told
  // Linux users they were current forever.
  it("with no update channel, offers no check and no auto-install toggle", async () => {
    const user = userEvent.setup();
    renderView({ hasUpdateChannel: false });
    await screen.findByText("Rhythm");
    await openCategory(user, "About & updates");
    await screen.findByText("Version 1.2.3");

    expect(
      screen.queryByRole("button", { name: /check for updates/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Install updates automatically"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("up to date")).not.toBeInTheDocument();
    expect(screen.getByText(/releases\/latest$/)).toBeInTheDocument();
  });

  it("toggles auto-install", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    await openCategory(user, "About & updates");
    await screen.findByText("Version 1.2.3");

    await user.click(screen.getByRole("switch"));

    expect(setAutoInstallFn).toHaveBeenCalledWith(false);
  });
});

describe("SettingsView — category rail", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("renders all four category tabs after load", async () => {
    renderView();
    await screen.findByText("Rhythm");
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((el) => el.textContent)).toEqual([
      "Loudness targets",
      "Instruments",
      "Playback level",
      "About & updates",
    ]);
    expect(
      screen.getByRole("tab", { name: "Loudness targets" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("swaps the pane when a category is clicked", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    expect(
      screen.getByRole("button", { name: /add target/i }),
    ).toBeInTheDocument();

    await openCategory(user, "Instruments");

    expect(screen.queryByRole("button", { name: /add target/i })).toBeNull();
    expect(
      screen.getByRole("button", { name: /add instrument/i }),
    ).toBeInTheDocument();
    // connected={false} → the disconnected calibration note shows.
    expect(
      screen.getByText(/calibrating needs the unit connected/i),
    ).toBeInTheDocument();
  });
});
