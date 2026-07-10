// Summary "Restore original" — a saved Base row carrying the pre-run presetLevel
// offers a Restore button that writes it back via restore_preset_level; rows
// without a revert anchor (scene rows, failed pre-run read) offer nothing.

import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));
import { invoke } from "@tauri-apps/api/core";

import { SummaryBody } from "../views/overlays/SummaryBody";
import type { RunItem } from "../views/level/leveling";

const base = (over: Partial<RunItem>): RunItem => ({
  key: "p3",
  slot: 3,
  presetName: "Guitar",
  isBase: true,
  sceneSlot: null,
  sceneName: "",
  tag: null,
  footswitch: null,
  instId: "none",
  targetName: "Lead",
  label: "Guitar",
  status: "result",
  outcome: "done",
  value: -22,
  ...over,
});

const renderSummary = (items: RunItem[]) =>
  render(
    <ThemeProvider>
      <SummaryBody
        items={items}
        stopped={false}
        onAccept={() => undefined}
        onRelevel={() => undefined}
      />
    </ThemeProvider>,
  );

describe("Summary restore-original", () => {
  it("restores a Base row's previous level through the device command", async () => {
    renderSummary([base({ previousLevel: 0.62 })]);
    const btn = screen.getByRole("button", { name: /restore/i });
    await userEvent.click(btn);
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("restore_preset_level", {
        slot: 3,
        level: 0.62,
      });
    });
    // The row reflects the restore (and can't double-fire).
    await screen.findByText(/restored/i);
  });

  it("offers no restore without a revert anchor", () => {
    renderSummary([base({ previousLevel: null })]);
    expect(screen.queryByRole("button", { name: /restore/i })).toBeNull();
  });
});
