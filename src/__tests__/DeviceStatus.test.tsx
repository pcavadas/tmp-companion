// Locks the toolbar's device readout (look B): a Dot (or a spinner) and one short phrase,
// `role="status"`, no hover titles and no click target. A (re)connect always passes
// through "Connecting…" for the 900 ms minimum, then resolves to "Connected · firmware
// <v>" (or "Untested firmware <v>" in caution); "In use by another app" for a busy unit;
// "Not connected" with a hollow dot otherwise. Unplugging returns to off at once and a
// replug re-runs the whole cycle.

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, screen } from "@testing-library/react";

import { light } from "../theme/tokens";
import { DeviceStatus, type DeviceStatusState } from "../ui/DeviceStatus";
import { renderWithTheme, toRgb } from "./testKit";

function renderStatus(
  state: DeviceStatusState,
  firmwareVersion: string | null,
) {
  return renderWithTheme(
    <DeviceStatus state={state} firmwareVersion={firmwareVersion} />,
  );
}

function wait900() {
  act(() => {
    vi.advanceTimersByTime(900);
  });
}

describe("DeviceStatus", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("off — hollow dot and 'Not connected', no firmware ever shown", () => {
    renderStatus("off", null);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent(/^Not connected$/);
    expect(status.querySelector("[title]")).toBeNull();
  });

  it("connecting — spinner + 'Connecting…'", () => {
    renderStatus("connecting", null);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connecting…$/);
    expect(document.querySelector(".tmp-spin")).not.toBeNull();
  });

  it("connect — 'Connecting…' for the 900 ms minimum, then 'Connected · firmware <v>'", () => {
    renderStatus("connected", "1.8.45");
    expect(screen.getByRole("status")).toHaveTextContent(/^Connecting…$/);
    expect(screen.queryByText(/1\.8\.45/)).toBeNull();
    wait900();
    expect(screen.getByRole("status")).toHaveTextContent(
      /^Connected · firmware 1\.8\.45$/,
    );
    expect(document.querySelector(".tmp-spin")).toBeNull();
  });

  it("connected without a version — plain 'Connected'", () => {
    renderStatus("connected", null);
    wait900();
    expect(screen.getByRole("status")).toHaveTextContent(/^Connected$/);
  });

  it("untested — 'Untested firmware <v>' in the caution colour", () => {
    renderStatus("untested", "1.6.3");
    wait900();
    const words = screen.getByText("Untested firmware 1.6.3");
    expect(words.style.color).toBe(toRgb(light.caution));
    expect(screen.queryByText(/^Connected/)).toBeNull();
  });

  it("busy — 'In use by another app', never the backend text", () => {
    renderStatus("busy", null);
    expect(screen.getByRole("status")).toHaveTextContent(
      /^In use by another app$/,
    );
  });

  it("unplug → off immediately; replug re-runs 'Connecting…'", () => {
    const view = renderStatus("connected", "1.8.45");
    wait900();
    expect(screen.getByText("Connected · firmware 1.8.45")).toBeTruthy();

    // Unplug: straight to off — no cached version.
    view.rerender(<DeviceStatus state="off" firmwareVersion={null} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Not connected$/);

    // Replug: the cycle replays from "Connecting…".
    view.rerender(<DeviceStatus state="connected" firmwareVersion="1.8.45" />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connecting…$/);
    wait900();
    expect(screen.getByText("Connected · firmware 1.8.45")).toBeTruthy();
  });
});
