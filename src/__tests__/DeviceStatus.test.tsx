// Locks the device-status indicator's 3-phase detection model (design handoff:
// firmware_status). The dot carries link state (hollow → amber pulse → green),
// the label carries the firmware the unit reported: off renders "disconnected",
// a (re)connect always passes through "reading firmware…" for the 900 ms
// minimum, then resolves to `connected · <version>`; unplugging returns to off
// and a replug re-runs the whole handshake cycle.

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { DeviceStatus } from "../ui/DeviceStatus";

function renderStatus(connected: boolean, firmwareVersion: string | null) {
  return render(
    <ThemeProvider>
      <DeviceStatus connected={connected} firmwareVersion={firmwareVersion} />
    </ThemeProvider>,
  );
}

describe("DeviceStatus", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("off — hollow dot, 'disconnected', no firmware ever shown", () => {
    renderStatus(false, null);
    expect(screen.getByText("disconnected")).toBeTruthy();
    expect(screen.getByTitle("No unit connected")).toBeTruthy();
    expect(document.querySelector(".tmp-fwpulse")).toBeNull();
  });

  it("connect — reads for the 900 ms minimum (amber pulse), then connected · version", () => {
    renderStatus(true, "1.7.75");
    // Phase: reading — pulsing dot + reading copy, version not yet shown.
    expect(screen.getByText("reading firmware…")).toBeTruthy();
    expect(screen.getByTitle("Reading firmware from unit…")).toBeTruthy();
    expect(document.querySelector(".tmp-fwpulse")).toBeTruthy();
    expect(screen.queryByText(/1\.7\.75/)).toBeNull();

    act(() => {
      vi.advanceTimersByTime(900);
    });

    // Phase: ready — labeled format, tooltip carries the full identity.
    expect(screen.getByText("connected")).toBeTruthy();
    expect(screen.getByText(/·\s*1\.7\.75/)).toBeTruthy();
    expect(screen.getByTitle("Tone Master Pro · firmware 1.7.75")).toBeTruthy();
    expect(document.querySelector(".tmp-fwpulse")).toBeNull();
  });

  it("ready without a version — plain 'connected', no separator (fallback)", () => {
    renderStatus(true, null);
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(screen.getByText("connected")).toBeTruthy();
    expect(screen.queryByText(/·/)).toBeNull();
    expect(screen.getByTitle("Tone Master Pro")).toBeTruthy();
  });

  it("ready on below-floor firmware — warn-toned 'untested · version', floor in tooltip", () => {
    renderStatus(true, "1.6.3");
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(screen.getByText("untested")).toBeTruthy();
    expect(screen.getByText(/·\s*1\.6\.3/)).toBeTruthy();
    expect(screen.queryByText("connected")).toBeNull();
    expect(
      screen.getByTitle(
        "Tone Master Pro · firmware 1.6.3 — untested below 1.7",
      ),
    ).toBeTruthy();
  });

  it("ready on supported firmware stays 'connected' (no untested label)", () => {
    renderStatus(true, "1.7.75");
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(screen.getByText("connected")).toBeTruthy();
    expect(screen.queryByText("untested")).toBeNull();
  });

  it("unplug → off immediately; replug re-runs the reading handshake", () => {
    const view = renderStatus(true, "1.7.75");
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(screen.getByText("connected")).toBeTruthy();

    // Unplug: straight to off — no cached version.
    view.rerender(
      <ThemeProvider>
        <DeviceStatus connected={false} firmwareVersion={null} />
      </ThemeProvider>,
    );
    expect(screen.getByText("disconnected")).toBeTruthy();
    expect(screen.queryByText(/1\.7\.75/)).toBeNull();

    // Replug: the detection cycle replays from "reading".
    view.rerender(
      <ThemeProvider>
        <DeviceStatus connected={true} firmwareVersion="1.7.75" />
      </ThemeProvider>,
    );
    expect(screen.getByText("reading firmware…")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(screen.getByText(/·\s*1\.7\.75/)).toBeTruthy();
  });
});
