// The app-wide notice for a failed library scan (look B; #310's surface): a caution
// Notice under the toolbar in plain words — never the failure's own text, which goes to
// the log — whose "Try again" re-runs the scan, and which hides while that retry runs.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ensureLibraryScan, resetLibraryScan } from "../device/libraryScan";
import type { BackupReadResult } from "../lib/types";
import { LibraryScanNotice } from "../views/LibraryScanNotice";
import { makeBackupRead, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const WORDS =
  "Couldn't read your preset details, so scenes, footswitches, signal paths and songs are missing here.";

describe("LibraryScanNotice", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
  });

  it("says nothing while the scan hasn't failed", async () => {
    mockCommands({ read_library_via_backup: () => makeBackupRead() });
    await ensureLibraryScan();
    renderWithTheme(<LibraryScanNotice />);
    expect(screen.queryByText(WORDS)).toBeNull();
  });

  it("a failed scan shows the plain words, never the failure's text", async () => {
    mockCommands({
      read_library_via_backup: () =>
        Promise.reject(new Error("backup timed out (0xe00002d6)")),
    });
    await ensureLibraryScan();
    renderWithTheme(<LibraryScanNotice />);
    expect(screen.getByRole("status")).toHaveTextContent(WORDS);
    expect(screen.queryByText(/backup timed out/)).toBeNull();
  });

  it("Try again re-runs the scan; the notice hides while it runs and stays gone once it succeeds", async () => {
    mockCommands({
      read_library_via_backup: () => Promise.reject(new Error("timed out")),
    });
    await ensureLibraryScan();
    renderWithTheme(<LibraryScanNotice />);

    let release: (r: BackupReadResult) => void = () => undefined;
    mockCommands({
      read_library_via_backup: () =>
        new Promise<BackupReadResult>((res) => {
          release = res;
        }),
    });
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.queryByText(WORDS)).toBeNull(); // running: hidden

    await act(async () => {
      release(makeBackupRead());
      await Promise.resolve();
    });
    expect(screen.queryByText(WORDS)).toBeNull();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([c]) => c === "read_library_via_backup"),
    ).toHaveLength(2);
  });
});
