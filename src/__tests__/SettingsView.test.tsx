// Settings view — the user-owned loudness targets (handoff design_handoff_
// settings_targets) + the Playback level segmented control.
//
//   • Targets render from the store; "Add target" appends a row in rename mode
//     and persists via save_targets (no ceiling clamp — value seeds at −22.0).
//   • Deleting the last target shows the empty-state line.
//   • The Playback level control persists the picked level via set_playback_level.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SettingsView } from "../views/settings";

const SEED_TARGETS = [
  { name: "Rhythm", lufs: -26.0 },
  { name: "Crunch", lufs: -24.0 },
  { name: "Lead", lufs: -22.0 },
];

// get_store returns the seed targets; everything else keeps the setup.ts empties.
function mockStore(targets = SEED_TARGETS, playback = "stage") {
  vi.mocked(invoke).mockImplementation((command: string) => {
    if (command === "get_store")
      return Promise.resolve({
        profiles: [],
        profile_by_slot: {},
        targets,
        playback_level: playback,
      });
    if (command === "list_pickup_topologies") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

function renderView() {
  return render(
    <ThemeProvider>
      <SettingsView connected={false} />
    </ThemeProvider>,
  );
}

const lastArgs = (command: string) => {
  const calls = vi.mocked(invoke).mock.calls.filter((c) => c[0] === command);
  return calls.length ? calls[calls.length - 1][1] : undefined;
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

    // Persisted: a 4th target seeded at −22.0, no clamp.
    const saved = lastArgs("save_targets") as { targets: { lufs: number }[] };
    expect(saved.targets).toHaveLength(4);
    expect(saved.targets[3]).toEqual({ name: "New target", lufs: -22.0 });
  });

  it("shows the empty state once every target is deleted", async () => {
    const user = userEvent.setup();
    mockStore([{ name: "Only", lufs: -20.0 }]);
    renderView();
    await screen.findByText("Only");

    await user.click(screen.getByTitle("More"));
    await user.click(screen.getByText("Delete"));

    expect(await screen.findByText(/no targets yet/i)).toBeInTheDocument();
    const saved = lastArgs("save_targets") as { targets: unknown[] };
    expect(saved.targets).toHaveLength(0);
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

    const group = screen.getByRole("radiogroup", { name: /playback level/i });
    await user.click(within(group).getByRole("radio", { name: "Rehearsal" }));

    await waitFor(() => {
      expect(lastArgs("set_playback_level")).toEqual({ level: "rehearsal" });
    });
  });

  it("reflects the stored level + its compensation caption", async () => {
    mockStore(SEED_TARGETS, "quiet");
    renderView();

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
