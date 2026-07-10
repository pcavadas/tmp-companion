// useUpdater — the auto-update state machine, with the plugin seam
// (../lib/updater) fully mocked so no Tauri plugin JS ever loads.
//
// REAL timers (repo gotcha: RTL waitFor hangs under vitest fake timers).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";

import type { FoundUpdate } from "../lib/updater";
import type { AppInfo, Store } from "../lib/types";

const h = vi.hoisted(() => {
  return {
    checkForUpdate: vi.fn<() => Promise<FoundUpdate | null>>(),
    relaunchApp: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    appInfo: vi.fn<() => Promise<{ name: string; version: string }>>(),
    getStore: vi.fn<() => Promise<unknown>>(),
    setAutoInstallUpdates: vi.fn<(on: boolean) => Promise<void>>(() =>
      Promise.resolve(),
    ),
  };
});

vi.mock("../lib/updater", () => ({
  checkForUpdate: h.checkForUpdate,
  relaunchApp: h.relaunchApp,
}));

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    isTauri: () => true,
    appInfo: h.appInfo,
    getStore: h.getStore,
    setAutoInstallUpdates: h.setAutoInstallUpdates,
  };
});

// Imported AFTER the mocks so the hook picks up the mocked seams.
import { useUpdater, formatReleaseNotes } from "../lib/useUpdater";

const info = (version: string): AppInfo => ({ name: "TMP Companion", version });

const storeWith = (auto: boolean): Store => ({
  profiles: [],
  profile_by_slot: {},
  targets: [],
  playback_level: "stage",
  auto_install_updates: auto,
});

/** A controllable FoundUpdate: exposes the percent callback + settle handles. */
function mkUpdate(version: string, notes: string | null) {
  let onPct: (pct: number) => void = () => undefined;
  let resolveDl: () => void = () => undefined;
  let rejectDl: (e: Error) => void = () => undefined;
  const download = vi.fn((cb: (pct: number) => void) => {
    onPct = cb;
    return new Promise<void>((res, rej) => {
      resolveDl = res;
      rejectDl = rej;
    });
  });
  const found: FoundUpdate = { version, notes, download };
  return {
    found,
    download,
    emit: (pct: number) => {
      onPct(pct);
    },
    finish: () => {
      resolveDl();
    },
    fail: () => {
      rejectDl(new Error("network"));
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  h.appInfo.mockResolvedValue(info("1.0.0"));
  h.getStore.mockResolvedValue(storeWith(false));
});

describe("useUpdater — found update, manual path", () => {
  it("available → reviewing → downloading (percent) → ready", async () => {
    const u = mkUpdate("1.1.0", "- Fixed a thing");
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("available");
    });
    expect(result.current.version).toBe("1.1.0");
    expect(result.current.currentVersion).toBe("1.0.0");

    act(() => {
      result.current.review();
    });
    expect(result.current.phase).toBe("reviewing");

    act(() => {
      result.current.cancelReview();
    });
    expect(result.current.phase).toBe("available");

    act(() => {
      result.current.startDownload();
    });
    expect(result.current.phase).toBe("downloading");
    expect(result.current.percent).toBe(0);

    act(() => {
      u.emit(42);
    });
    expect(result.current.percent).toBe(42);

    act(() => {
      u.finish();
    });
    await waitFor(() => {
      expect(result.current.phase).toBe("ready");
    });
  });
});

describe("useUpdater — auto-install", () => {
  it("skips straight to downloading when the store preference is on", async () => {
    const u = mkUpdate("1.1.0", null);
    h.getStore.mockResolvedValue(storeWith(true));
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("downloading");
    });
    expect(result.current.autoInstall).toBe(true);
    expect(u.download).toHaveBeenCalledTimes(1);
  });
});

describe("useUpdater — download failure + retry", () => {
  it("rejection lands in error; retry() downloads again", async () => {
    const u = mkUpdate("1.1.0", null);
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("available");
    });

    act(() => {
      result.current.startDownload();
    });
    act(() => {
      u.fail();
    });
    await waitFor(() => {
      expect(result.current.phase).toBe("error");
    });

    act(() => {
      result.current.retry();
    });
    expect(result.current.phase).toBe("downloading");
    expect(u.download).toHaveBeenCalledTimes(2);
  });
});

describe("useUpdater — dev-build guard", () => {
  it("never checks when the app version is the dev placeholder", async () => {
    h.appInfo.mockResolvedValue(info("0.0.0-development"));

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.currentVersion).toBe("0.0.0-development");
    });
    // Let the mount effect fully settle, then assert no check happened.
    await act(async () => {
      await Promise.resolve();
    });
    expect(h.checkForUpdate).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("idle");
  });
});

describe("formatReleaseNotes", () => {
  it("strips headings, links and bullets, drops blanks, caps at 10", () => {
    expect(
      formatReleaseNotes(
        "## What's new\n\n- Fixed [a bug](https://x.test/1)\n* Faster scans\n• Nicer toasts\n",
      ),
    ).toEqual(["What's new", "Fixed a bug", "Faster scans", "Nicer toasts"]);

    const many = Array.from({ length: 14 }, (_, i) => `line ${String(i)}`);
    expect(formatReleaseNotes(many.join("\n"))).toHaveLength(10);

    expect(formatReleaseNotes("\r\n  \r\n")).toEqual([]);
  });
});
