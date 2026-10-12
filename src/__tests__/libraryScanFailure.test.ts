// src/__tests__/libraryScanFailure.test.ts — #310: the library scan's failure paths
// and its progress store. A progress-listen failure must not strand the scan, a
// failed backup read must say so (`error`) instead of reading as an empty library
// and must be retryable (Copy shows it and its Retry re-runs the scan), a reset
// mid-listen starts no stale read, and a progress tick must not notify DATA
// subscribers.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { renderHook } from "@testing-library/react";

import type { BackupProgress, BackupReadResult } from "../lib/types";

const h = vi.hoisted(() => ({
  onBackupProgress:
    vi.fn<(cb: (p: BackupProgress) => void) => Promise<() => void>>(),
}));

vi.mock("../lib/liveEvents", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/liveEvents")>();
  return { ...actual, onBackupProgress: h.onBackupProgress };
});

// Imported AFTER the mock so the store picks up the controllable listen.
import {
  ensureLibraryScan,
  getLibraryScan,
  getLibraryScanPercent,
  resetLibraryScan,
  retryLibraryScan,
  subscribeLibraryScan,
  subscribeLibraryScanPercent,
} from "../device/libraryScan";
import { useCopyLibrary } from "../views/copy/useCopyLibrary";

const EMPTY_BACKUP: BackupReadResult = {
  members: [],
  db_bytes: 0,
  total_rows: 0,
  scene_mode: "test",
  presets: [],
  song_presets: [],
  songs: [],
  setlists: [],
  setlist_songs: [],
};

/** Route `read_library_via_backup` through `impl`; every other command → undefined. */
function mockBackup(impl: () => Promise<unknown>) {
  vi.mocked(invoke).mockImplementation((cmd: string) =>
    cmd === "read_library_via_backup" ? impl() : Promise.resolve(undefined),
  );
}

describe("libraryScan — failure paths (#310)", () => {
  beforeEach(() => {
    resetLibraryScan();
    vi.mocked(invoke).mockReset();
    h.onBackupProgress.mockReset();
    h.onBackupProgress.mockResolvedValue(() => undefined);
  });

  it("a progress-listen failure doesn't strand the scan: the read still runs and settles", async () => {
    h.onBackupProgress.mockRejectedValue(new Error("listen failed"));
    mockBackup(() => Promise.resolve(EMPTY_BACKUP));

    await ensureLibraryScan();

    const s = getLibraryScan();
    expect(s.scanning).toBe(false);
    expect(s.ready).toBe(true);
    expect(s.error).toBeNull();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([c]) => c === "read_library_via_backup"),
    ).toBe(true);
  });

  it("a failed backup read sets `error` (not a silent empty library) and a retry runs it again", async () => {
    mockBackup(() => Promise.reject(new Error("backup timed out")));

    await ensureLibraryScan();
    let s = getLibraryScan();
    expect(s.ready).toBe(true);
    expect(s.scanning).toBe(false);
    expect(s.error).toBe("backup timed out");

    // Retry: the scan is NOT treated as done for this connection.
    mockBackup(() => Promise.resolve(EMPTY_BACKUP));
    retryLibraryScan();
    await vi.waitFor(() => {
      expect(getLibraryScan().ready).toBe(true);
    });
    s = getLibraryScan();
    expect(s.error).toBeNull();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([c]) => c === "read_library_via_backup"),
    ).toHaveLength(2); // the failed read, then the retry's
  });

  it("a reset while a failing progress-listen is pending starts no stale backup read", async () => {
    let rejectListen: (e: Error) => void = () => undefined;
    h.onBackupProgress.mockImplementation(
      () =>
        new Promise((_res, rej) => {
          rejectListen = rej;
        }),
    );
    mockBackup(() => Promise.resolve(EMPTY_BACKUP));

    const scan = ensureLibraryScan();
    await vi.waitFor(() => {
      expect(h.onBackupProgress).toHaveBeenCalled();
    });
    resetLibraryScan(); // the device detached mid-listen
    rejectListen(new Error("listen failed"));
    await scan;

    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([c]) => c === "read_library_via_backup"),
    ).toBe(false);
  });

  it("Copy surfaces a failed scan as its error (it can't place blocks from empty graphs)", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "read_library_via_backup")
        return Promise.reject(new Error("backup timed out"));
      if (cmd === "list_presets") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const { result } = renderHook(() => useCopyLibrary(true, null));
    await ensureLibraryScan();
    await vi.waitFor(() => {
      expect(result.current.loaded).toBe(true);
    });
    expect(result.current.error).toBe("backup timed out");

    // Its Retry re-runs the failed scan, and the error clears once it succeeds.
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "read_library_via_backup")
        return Promise.resolve(EMPTY_BACKUP);
      if (cmd === "list_presets") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    await result.current.refresh();
    await vi.waitFor(() => {
      expect(result.current.error).toBeNull();
    });
  });

  it("a successful scan is still idempotent (no second read)", async () => {
    mockBackup(() => Promise.resolve(EMPTY_BACKUP));
    await ensureLibraryScan();
    await ensureLibraryScan();
    retryLibraryScan();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([c]) => c === "read_library_via_backup"),
    ).toHaveLength(1);
  });

  it("progress ticks go to the percent store only — data subscribers aren't notified", async () => {
    let tick: (p: BackupProgress) => void = () => undefined;
    h.onBackupProgress.mockImplementation((cb) => {
      tick = cb;
      return Promise.resolve(() => undefined);
    });
    let release: (r: BackupReadResult) => void = () => undefined;
    mockBackup(
      () =>
        new Promise((res) => {
          release = res;
        }),
    );

    const scan = ensureLibraryScan();
    await vi.waitFor(() => {
      expect(h.onBackupProgress).toHaveBeenCalled();
    });

    const dataSub = vi.fn();
    const pctSub = vi.fn();
    const offData = subscribeLibraryScan(dataSub);
    const offPct = subscribeLibraryScanPercent(pctSub);
    const at = (percent: number): BackupProgress => ({
      phase: "streaming",
      received: 0,
      total: 0,
      bytes: 0,
      total_bytes: 0,
      percent,
      build_size: 0,
      build_ticks: 0,
    });
    tick(at(30));
    tick(at(60));
    expect(getLibraryScanPercent()).toBe(60);
    expect(pctSub).toHaveBeenCalledTimes(2);
    expect(dataSub).not.toHaveBeenCalled();

    release(EMPTY_BACKUP);
    await scan;
    expect(getLibraryScanPercent()).toBe(100);
    offData();
    offPct();
  });
});
