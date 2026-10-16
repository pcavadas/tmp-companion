// Locks the connect-error classification (which page a device tab shows while no unit
// is connected), keyed on the backend's typed error KIND (#269), never on the message
// text. The cold-boot case is the one that bit us: the TMP's HID interface enumerates
// (hotplug fires, open/seize succeeds) seconds before its USB stack accepts reports, so
// the first handshake send fails — `not_ready` — and that must read as "plug it in /
// wait", not as a failure.

import { describe, it, expect } from "vitest";
import {
  errorKind,
  errorReampStillOn,
  errorSkipKind,
  isCommandErrorKind,
  isConnectGateKind,
  noUnitState,
} from "../lib/connectError";
import { CommandError } from "../lib/invoke";

describe("noUnitState", () => {
  it("is connecting until the first attempt settles, whatever the kind", () => {
    expect(noUnitState(true, null)).toBe("connecting");
    expect(noUnitState(true, "busy")).toBe("connecting");
  });

  it("nothing plugged in or a unit still booting → the plain plug-it-in page", () => {
    expect(noUnitState(false, null)).toBe("off");
    expect(noUnitState(false, "not_connected")).toBe("off");
    expect(noUnitState(false, "not_ready")).toBe("off");
  });

  it("busy → another app is using it", () => {
    expect(noUnitState(false, "busy")).toBe("busy");
  });

  it("any other kind → couldn't connect", () => {
    expect(noUnitState(false, "other")).toBe("failed");
  });

  it("classifies by kind, not by text: a gate-like message of kind other fails", () => {
    // The old regexes read the text; only the typed kind decides now.
    expect(noUnitState(false, errorKind(new Error("no TMP found")))).toBe(
      "failed",
    );
    expect(
      noUnitState(false, errorKind({ kind: "busy", message: "no TMP found" })),
    ).toBe("busy");
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
    // The leveling commands' kinds (`error::SkipKind`, pinned on the Rust side by
    // `a_skip_kind_serializes_as_its_wire_token`).
    for (const k of [
      "preset_unreadable",
      "footswitches_unreadable",
      "no_active_amp",
      "level_manually",
      "scene_write_refused",
      "unsupported_knob",
      "knob_missing",
      "block_off",
      "scene_change_discards",
      "no_stimulus",
      "clipped_capture",
      "no_signal",
      "connection_lost",
      "cancelled",
    ]) {
      expect(isCommandErrorKind(k)).toBe(true);
    }
    expect(isCommandErrorKind("skipped")).toBe(false);
    expect(isCommandErrorKind(3)).toBe(false);
  });
});

describe("errorSkipKind", () => {
  it("reads a leveling command's typed kind, never its text", () => {
    expect(
      errorSkipKind(
        new CommandError("level_preset", "x", "footswitches_unreadable"),
      ),
    ).toBe("footswitches_unreadable");
    expect(errorSkipKind({ kind: "connection_lost", message: "x" })).toBe(
      "connection_lost",
    );
    // Text that names a kind picks nothing.
    expect(errorSkipKind(new Error("no active guitar amp in scene"))).toBe(
      "other",
    );
    expect(errorSkipKind("preset_unreadable")).toBe("other");
  });

  it("folds the connect kinds into other: no leveling failure is one", () => {
    for (const k of ["not_connected", "busy", "not_ready"] as const)
      expect(errorSkipKind(new CommandError("level_preset", "x", k))).toBe(
        "other",
      );
  });
});

describe("errorReampStillOn", () => {
  it("is true only for a rejection that says so", () => {
    expect(
      errorReampStillOn(
        new CommandError("level_preset", "x", "no_signal", true),
      ),
    ).toBe(true);
    expect(
      errorReampStillOn({ kind: "other", message: "x", reampStillOn: true }),
    ).toBe(true);
    expect(errorReampStillOn(new CommandError("level_preset", "x"))).toBe(
      false,
    );
    expect(errorReampStillOn({ reampStillOn: "yes" })).toBe(false);
    expect(errorReampStillOn("Re-amp may still be on.")).toBe(false);
  });
});
