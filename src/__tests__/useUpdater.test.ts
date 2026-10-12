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
    appInfo:
      vi.fn<() => Promise<{ name: string; version: string; os: string }>>(),
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

const info = (version: string, os = "macos"): AppInfo => ({
  name: "TMP Companion",
  version,
  os,
});

const storeWith = (auto: boolean): Store => ({
  profiles: [],
  profile_by_slot: {},
  targets: [],
  playback_level: "stage",
  auto_install_updates: auto,
  meter_convention: 1,
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

// A finished download must release the single-flight guard, or an update found
// after dismissing "ready" could never download.
describe("useUpdater — a finished download releases the guard", () => {
  it("success → dismiss → a later update downloads again", async () => {
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
      u.finish();
    });
    await waitFor(() => {
      expect(result.current.phase).toBe("ready");
    });

    act(() => {
      result.current.dismiss();
    });
    await act(async () => {
      await result.current.check();
    });
    expect(result.current.phase).toBe("available");
    act(() => {
      result.current.startDownload();
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

// latest.json carries darwin-* keys and nothing else, so macOS is the only
// platform with a channel. This is an allow-list rather than "not linux"
// precisely so a new target (the in-flight Windows port) does not silently
// inherit the permanent "up to date" bug this guard exists to prevent.
describe("useUpdater — update-channel allow-list", () => {
  it("reports a channel on macOS and checks on mount", async () => {
    h.appInfo.mockResolvedValue(info("1.0.0", "macos"));
    h.checkForUpdate.mockResolvedValue(null);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.hasUpdateChannel).toBe(true);
    });
    expect(h.checkForUpdate).toHaveBeenCalled();
  });

  it.each(["linux", "windows"])(
    "reports no channel on %s and never checks",
    async (os) => {
      h.appInfo.mockResolvedValue(info("1.0.0", os));

      const { result } = renderHook(() => useUpdater());
      await waitFor(() => {
        expect(result.current.hasUpdateChannel).toBe(false);
      });
      // Settle the mount effect, then assert the pointless round-trip never ran.
      await act(async () => {
        await Promise.resolve();
      });
      expect(h.checkForUpdate).not.toHaveBeenCalled();
      expect(result.current.phase).toBe("idle");
    },
  );
});

describe("useUpdater — manual check()", () => {
  it("returns found and transitions to available", async () => {
    const u = mkUpdate("1.1.0", null);
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("available");
    });

    h.checkForUpdate.mockClear();
    const other = mkUpdate("1.2.0", null);
    h.checkForUpdate.mockResolvedValue(other.found);

    let outcome: "found" | "none" | "error" | undefined;
    await act(async () => {
      outcome = await result.current.check();
    });
    expect(outcome).toBe("found");
    expect(result.current.version).toBe("1.2.0");
  });

  it("returns none and stays idle when nothing is available", async () => {
    h.checkForUpdate.mockResolvedValue(null);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.currentVersion).toBe("1.0.0");
    });

    let outcome: "found" | "none" | "error" | undefined;
    await act(async () => {
      outcome = await result.current.check();
    });
    expect(outcome).toBe("none");
    expect(result.current.phase).toBe("idle");
  });

  it("returns error when the check rejects", async () => {
    h.checkForUpdate.mockResolvedValueOnce(null);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.currentVersion).toBe("1.0.0");
    });

    h.checkForUpdate.mockRejectedValueOnce(new Error("offline"));
    let outcome: "found" | "none" | "error" | undefined;
    await act(async () => {
      outcome = await result.current.check();
    });
    expect(outcome).toBe("error");
    expect(result.current.phase).toBe("idle");
  });

  it("no-ops while a download is already in progress", async () => {
    const u = mkUpdate("1.1.0", null);
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("available");
    });

    act(() => {
      result.current.startDownload();
    });
    expect(result.current.phase).toBe("downloading");

    h.checkForUpdate.mockClear();
    let outcome: "found" | "none" | "error" | undefined;
    await act(async () => {
      outcome = await result.current.check();
    });
    expect(outcome).toBe("found");
    expect(h.checkForUpdate).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("downloading");
    expect(u.download).toHaveBeenCalledTimes(1);
  });
});

// #311: with auto-install on, a manual Check while the startup check is still
// pending used to reach start() twice — two concurrent downloads.
describe("useUpdater — overlapping checks with auto-install (#311)", () => {
  it("two checks resolving together start ONE download", async () => {
    const u = mkUpdate("1.1.0", null);
    h.getStore.mockResolvedValue(storeWith(true));
    const pending: ((f: FoundUpdate | null) => void)[] = [];
    h.checkForUpdate.mockImplementation(
      () =>
        new Promise<FoundUpdate | null>((res) => {
          pending.push(res);
        }),
    );

    const { result } = renderHook(() => useUpdater());
    // The startup check is in flight (auto-install read, check pending).
    await waitFor(() => {
      expect(pending).toHaveLength(1);
    });
    expect(result.current.autoInstall).toBe(true);
    expect(result.current.phase).toBe("idle");

    // A manual Check while it's pending — both are now in flight.
    let manual: Promise<"found" | "none" | "error"> | undefined;
    act(() => {
      manual = result.current.check();
    });
    expect(pending).toHaveLength(2);

    await act(async () => {
      for (const res of pending) res(u.found);
      await manual;
    });
    expect(result.current.phase).toBe("downloading");
    expect(u.download).toHaveBeenCalledTimes(1);
  });
});

describe("useUpdater — dismiss / restart / setAutoInstall", () => {
  it("dismiss() resets any dismissible phase to idle", async () => {
    const u = mkUpdate("1.1.0", null);
    h.checkForUpdate.mockResolvedValue(u.found);

    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.phase).toBe("available");
    });

    act(() => {
      result.current.dismiss();
    });
    expect(result.current.phase).toBe("idle");
  });

  it("restart() calls relaunchApp", async () => {
    h.checkForUpdate.mockResolvedValue(null);
    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.currentVersion).toBe("1.0.0");
    });

    act(() => {
      result.current.restart();
    });
    expect(h.relaunchApp).toHaveBeenCalledTimes(1);
  });

  it("setAutoInstall() updates state optimistically and persists it", async () => {
    h.checkForUpdate.mockResolvedValue(null);
    const { result } = renderHook(() => useUpdater());
    await waitFor(() => {
      expect(result.current.currentVersion).toBe("1.0.0");
    });
    expect(result.current.autoInstall).toBe(false);

    act(() => {
      result.current.setAutoInstall(true);
    });
    expect(result.current.autoInstall).toBe(true);
    await waitFor(() => {
      expect(h.setAutoInstallUpdates).toHaveBeenCalledWith(true);
    });
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
