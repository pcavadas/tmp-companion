// Locks the tested-firmware floor logic: numeric-segment semver compare (so
// "1.10" > "1.7", which a string compare gets wrong), the null-is-supported
// rule, and the per-tab gate predicate (unit-driven tabs gated, Catalog/Settings
// never gated, off once proceeded / supported / no version).

import { describe, it, expect } from "vitest";
import {
  FW_MIN,
  semverGte,
  firmwareSupported,
  firmwareGateActive,
} from "../lib/firmware";

describe("semverGte", () => {
  it("compares by numeric segment, not lexically", () => {
    expect(semverGte("1.10", "1.7")).toBe(true); // the string-compare trap
    expect(semverGte("1.7", "1.10")).toBe(false);
  });

  it("treats missing trailing segments as 0 (equal counts as >=)", () => {
    expect(semverGte("1.7", "1.7.0")).toBe(true);
    expect(semverGte("1.7.0", "1.7")).toBe(true);
    expect(semverGte("1.7.75", "1.7")).toBe(true);
  });

  it("is false strictly below", () => {
    expect(semverGte("1.6.3", "1.7")).toBe(false);
    expect(semverGte("0.9", "1.7")).toBe(false);
  });
});

describe("firmwareSupported", () => {
  it("null (unknown) is supported — never gate without a version", () => {
    expect(firmwareSupported(null)).toBe(true);
  });
  it("tracks the FW_MIN floor", () => {
    expect(firmwareSupported(FW_MIN)).toBe(true);
    expect(firmwareSupported("1.7.75")).toBe(true);
    expect(firmwareSupported("1.6.3")).toBe(false);
  });
});

describe("firmwareGateActive", () => {
  const below = { firmware: "1.6.3", proceeded: false };

  it("gates the unit-driven tabs on below-floor firmware", () => {
    for (const tab of ["level", "copy", "songs"]) {
      expect(firmwareGateActive({ ...below, tab })).toBe(true);
    }
  });

  it("never gates Catalog or Settings", () => {
    expect(firmwareGateActive({ ...below, tab: "catalog" })).toBe(false);
    expect(firmwareGateActive({ ...below, tab: "settings" })).toBe(false);
  });

  it("is off once proceeded, when supported, or with no version", () => {
    expect(
      firmwareGateActive({ ...below, tab: "level", proceeded: true }),
    ).toBe(false);
    expect(
      firmwareGateActive({
        firmware: "1.7.75",
        tab: "level",
        proceeded: false,
      }),
    ).toBe(false);
    expect(
      firmwareGateActive({ firmware: null, tab: "level", proceeded: false }),
    ).toBe(false);
  });
});
