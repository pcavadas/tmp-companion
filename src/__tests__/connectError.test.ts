// Locks the connect-error classification (App shell banner vs friendly gate).
// The cold-boot case is the one that bit us: the TMP's HID interface enumerates
// (hotplug fires, open/seize succeeds) seconds before its USB stack accepts
// reports, so the first handshake send times out with
// `IOHIDDeviceSetReport failed: 0xe00002d6` — that must NOT flash the red banner.

import { describe, it, expect } from "vitest";
import { actionableError } from "../lib/connectError";

describe("actionableError", () => {
  it("suppresses 'no TMP found' (nothing plugged in → friendly gate)", () => {
    expect(
      actionableError(
        "no TMP found (VID 0x1ED8 / PID 0x44) — is it plugged in?",
      ),
    ).toBeNull();
  });

  it("suppresses a SetReport timeout (unit present but still booting → gate)", () => {
    expect(
      actionableError("IOHIDDeviceSetReport failed: 0xe00002d6"),
    ).toBeNull();
    // Any SetReport failure code is the not-ready/went-away case, not actionable.
    expect(
      actionableError("IOHIDDeviceSetReport failed: 0xe00002eb"),
    ).toBeNull();
  });

  it("surfaces the actionable 'close Pro Control' open failure (red banner)", () => {
    const msg =
      "IOHIDDeviceOpen failed: 0xe00002c5 — close Fender Pro Control (it holds the device) and retry";
    expect(actionableError(msg)).toBe(msg);
  });

  it("surfaces any other unexpected error verbatim", () => {
    expect(actionableError("connect task failed: panic")).toBe(
      "connect task failed: panic",
    );
  });
});
