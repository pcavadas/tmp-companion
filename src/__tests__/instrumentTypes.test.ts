// lib/instrumentTypes — the on-screen names of the backend's instrument types
// (guitar / bass / bass-vi) and the "{type} · {pickup}" caption, which never repeats a
// type the pickup already names ("Bass VI", never "Bass VI · Bass VI").

import { describe, it, expect } from "vitest";

import { instrumentCaption, instrumentTypeLabel } from "../lib/instrumentTypes";

describe("instrumentTypeLabel", () => {
  it("names the three types", () => {
    expect(instrumentTypeLabel("guitar")).toBe("Guitar");
    expect(instrumentTypeLabel("bass")).toBe("Bass");
    expect(instrumentTypeLabel("bass-vi")).toBe("Bass VI");
  });

  it("shows a type it doesn't know readable, not raw", () => {
    expect(instrumentTypeLabel("baritone")).toBe("Baritone");
    expect(instrumentTypeLabel("lap-steel")).toBe("Lap steel");
  });
});

describe("instrumentCaption", () => {
  it("is '{type} · {pickup}'", () => {
    expect(
      instrumentCaption({
        id: "guitar-humbucker",
        label: "Humbucker",
        instrument: "guitar",
      }),
    ).toBe("Guitar · Humbucker");
    expect(
      instrumentCaption({
        id: "guitar-acoustic",
        label: "Acoustic / piezo",
        instrument: "guitar",
      }),
    ).toBe("Guitar · Acoustic / piezo");
  });

  it("lets a pickup that already names its type stand alone", () => {
    expect(
      instrumentCaption({
        id: "bass-vi",
        label: "Bass VI",
        instrument: "bass-vi",
      }),
    ).toBe("Bass VI");
    expect(
      instrumentCaption({
        id: "bass-humbucker",
        label: "Bass humbucker",
        instrument: "bass",
      }),
    ).toBe("Bass humbucker");
  });

  it("only matches the type as a whole word", () => {
    expect(
      instrumentCaption({ id: "x", label: "Bassman coil", instrument: "bass" }),
    ).toBe("Bass · Bassman coil");
  });
});
