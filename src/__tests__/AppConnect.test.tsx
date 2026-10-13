// App — #305: the manual "Scan for device" goes through the same single-flight
// guard as the 3 s poll, so it never overlaps an attempt already in flight.
// LevelView is shallow-mocked to expose its onScan prop.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "../theme/ThemeProvider";
import { DISCLAIMER_PERM_KEY } from "../lib/gates";
import type { ConnectResult } from "../lib/types";
import { mockCommands } from "./commandMocks";
import type { LevelViewProps } from "../views/level/LevelView";

vi.mock("../views/level", () => ({
  LevelView: ({ onScan }: LevelViewProps) => (
    <button onClick={onScan}>scan-now</button>
  ),
}));

// Imported AFTER the mock so App picks up the stand-in.
import App from "../App";

describe("App — manual retry shares the single-flight guard (#305)", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    vi.mocked(invoke).mockReset();
  });

  it("a Scan while the mount attempt is still in flight starts no second connect", async () => {
    let fail: (e: Error) => void = () => undefined;
    mockCommands({
      connect_device: () =>
        new Promise<ConnectResult>((_res, rej) => {
          fail = rej;
        }),
    });
    localStorage.setItem(DISCLAIMER_PERM_KEY, "1");
    render(
      <ThemeProvider>
        <App />
      </ThemeProvider>,
    );
    const connects = () =>
      vi.mocked(invoke).mock.calls.filter(([c]) => c === "connect_device")
        .length;
    expect(connects()).toBe(1);

    await userEvent.click(await screen.findByText("scan-now"));
    expect(connects()).toBe(1);

    // Once that attempt settles (no unit), a Scan connects again at once.
    await act(async () => {
      fail(new Error("no TMP found"));
      await Promise.resolve();
    });
    await userEvent.click(screen.getByText("scan-now"));
    expect(connects()).toBe(2);
  });
});
