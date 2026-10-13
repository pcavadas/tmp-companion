// Locks the connect-error classification (App shell banner vs friendly gate), keyed on
// the backend's typed error KIND (#269), never on the message text. The cold-boot case
// is the one that bit us: the TMP's HID interface enumerates (hotplug fires, open/seize
// succeeds) seconds before its USB stack accepts reports, so the first handshake send
// fails — `not_ready` — and that must NOT flash the red banner.

import { describe, it, expect } from "vitest";
import {
  connectBannerMessage,
  errorKind,
  isCommandErrorKind,
  isConnectGateKind,
} from "../lib/connectError";
import { CommandError } from "../lib/invoke";

describe("connectBannerMessage", () => {
  it("suppresses not_connected (nothing plugged in → friendly gate)", () => {
    expect(
      connectBannerMessage(
        "not_connected",
        "no TMP found (VID 0x1ED8 / PID 0x44) — is it plugged in?",
      ),
    ).toBeNull();
  });

  it("suppresses not_ready (unit present but still booting → gate)", () => {
    expect(
      connectBannerMessage(
        "not_ready",
        "IOHIDDeviceSetReport failed: 0xe00002d6",
      ),
    ).toBeNull();
  });

  it("surfaces busy — the actionable 'close Pro Control' open failure (red banner)", () => {
    const msg =
      "IOHIDDeviceOpen failed: 0xe00002c5 — close Fender Pro Control (it holds the device) and retry";
    expect(connectBannerMessage("busy", msg)).toBe(msg);
  });

  it("surfaces any other error verbatim", () => {
    expect(connectBannerMessage("other", "connect task failed: panic")).toBe(
      "connect task failed: panic",
    );
  });

  it("classifies by kind, not by text: a gate-like message of kind other surfaces", () => {
    // The old regexes would have hidden this; only the kind decides now.
    expect(connectBannerMessage("other", "no TMP found")).toBe("no TMP found");
  });
});

describe("isConnectGateKind", () => {
  it("is true exactly for the two gate kinds", () => {
    expect(isConnectGateKind("not_connected")).toBe(true);
    expect(isConnectGateKind("not_ready")).toBe(true);
    expect(isConnectGateKind("busy")).toBe(false);
    expect(isConnectGateKind("other")).toBe(false);
  });
});

describe("errorKind", () => {
  it("reads a CommandError's kind", () => {
    expect(errorKind(new CommandError("connect_device", "x", "busy"))).toBe(
      "busy",
    );
  });

  it("reads a raw { kind, message } rejection", () => {
    expect(errorKind({ kind: "not_ready", message: "x" })).toBe("not_ready");
  });

  it("falls back to other for a bare string, a plain Error, or an unknown kind", () => {
    expect(errorKind("no TMP found")).toBe("other");
    expect(errorKind(new Error("no TMP found"))).toBe("other");
    expect(errorKind({ kind: "exploded", message: "x" })).toBe("other");
    expect(errorKind(null)).toBe("other");
  });

  it("knows exactly the backend's kinds", () => {
    for (const k of ["not_connected", "busy", "not_ready", "other"]) {
      expect(isCommandErrorKind(k)).toBe(true);
    }
    expect(isCommandErrorKind("cancelled")).toBe(false);
    expect(isCommandErrorKind(3)).toBe(false);
  });
});
