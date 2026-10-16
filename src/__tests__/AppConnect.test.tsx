// App — #305: every connect goes through ONE single-flight guard, so no two attempts ever
// overlap. The manual "Scan for device" that bug was found on is gone (look B: connection
// is automatic); the guard still serves the two paths left — the 3 s poll and the
// hotplug attach event — and this gate drives both.
//
// Also locks how a failed connect reaches the screen: its typed KIND (never its message
// text) picks the device tab's page and the toolbar's words.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

import { DISCLAIMER_PERM_KEY } from "../lib/gates";
import type { CommandErrorKind, ConnectResult } from "../lib/types";
import { forceTauri, mockCommands, mockLiveEvents } from "./commandMocks";
import App from "../App";
import { renderWithTheme } from "./testKit";

const live = mockLiveEvents();

const connects = () =>
  vi.mocked(invoke).mock.calls.filter(([c]) => c === "connect_device").length;

/** `connect_device` stays pending until the test settles it. */
function holdConnects(): { fail: (e: unknown) => void } {
  const held = { fail: (_e: unknown): void => undefined };
  mockCommands({
    connect_device: () =>
      new Promise<ConnectResult>((_res, rej) => {
        held.fail = rej;
      }),
  });
  return held;
}

function renderApp() {
  localStorage.setItem(DISCLAIMER_PERM_KEY, "1"); // skip the first-run page
  return renderWithTheme(<App />);
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  vi.mocked(invoke).mockReset();
});

describe("App — the 3 s poll shares the single-flight guard (#305)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("a poll tick while the mount attempt is still in flight starts no second connect", async () => {
    const held = holdConnects();
    renderApp();
    expect(connects()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(connects()).toBe(1);

    // Once that attempt settles (no unit), the next tick connects again.
    await act(async () => {
      // The typed `connect_device` rejection (#269): nothing plugged in.
      held.fail({ kind: "not_connected", message: "no TMP found" });
      await Promise.resolve();
    });
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(connects()).toBe(2);
  });
});

describe("App — the hotplug attach shares the single-flight guard (#305)", () => {
  let undoTauri: () => void = () => undefined;
  beforeEach(() => {
    undoTauri = forceTauri();
  });
  afterEach(() => {
    undoTauri();
  });

  it("an attach while an attempt is in flight starts none; once it settles, an attach connects at once", async () => {
    const held = holdConnects();
    renderApp();
    expect(connects()).toBe(1);

    const attached = live.get("tmp://device-attached");
    expect(attached).toBeDefined();
    act(() => {
      attached?.({ payload: null });
    });
    expect(connects()).toBe(1);

    await act(async () => {
      held.fail({ kind: "not_connected", message: "no TMP found" });
      await Promise.resolve();
    });
    act(() => {
      attached?.({ payload: null });
    });
    expect(connects()).toBe(2);
  });
});

describe("App — a failed connect's KIND picks the page and the toolbar words", () => {
  /** `connect_device` rejects carrying the backend AppError's typed `kind`. */
  function failWith(kind: CommandErrorKind, message: string) {
    mockCommands({
      connect_device: () =>
        Promise.reject(Object.assign(new Error(message), { kind })),
    });
  }

  it("busy → 'Another app is using your Tone Master Pro', never the backend text", async () => {
    failWith(
      "busy",
      "IOHIDDeviceOpen failed: 0xe00002c5 — close Fender Pro Control (it holds the device) and retry",
    );
    renderApp();
    expect(
      await screen.findByRole("heading", {
        name: "Another app is using your Tone Master Pro",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Quit Pro Control or the other TMP Companion window, and it connects here.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("In use by another app")).toBeInTheDocument();
    expect(screen.queryByText(/0xe00002c5/)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull(); // no red banner
  });

  it("other → 'Couldn't connect to your Tone Master Pro', even with a no-unit message", async () => {
    // Kind, not text: the message reads like "nothing plugged in", the kind says otherwise.
    failWith("other", "no TMP found");
    renderApp();
    expect(
      await screen.findByRole("heading", {
        name: "Couldn't connect to your Tone Master Pro",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Not connected")).toBeInTheDocument();
    expect(screen.queryByText("no TMP found")).toBeNull();
  });

  it("not_connected → 'Plug in your Tone Master Pro', with no Scan button", async () => {
    failWith("not_connected", "no TMP found");
    renderApp();
    expect(
      await screen.findByRole("heading", {
        name: "Plug in your Tone Master Pro",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Not connected")).toBeInTheDocument();
    expect(screen.queryByText(/scan for device/i)).toBeNull();
  });
});
