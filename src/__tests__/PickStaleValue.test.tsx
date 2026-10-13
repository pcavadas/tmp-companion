// src/__tests__/PickStaleValue.test.tsx — the danger.md Pick/BlockPick trap: a stored
// `value` the current `options` set no longer contains must DISPLAY that value with a
// warning, never silently fall back to `options[0]` — the documented incident ("a store
// without a Crunch target displayed Rhythm while the run got crunch, a silent −30
// fallback").

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { Pick } from "../views/overlays/Pick";
import { renderWithTheme } from "./testKit";

const options = [
  { id: "rhythm", label: "Rhythm" },
  { id: "lead", label: "Lead" },
];

describe("Pick — stored value not in the current options", () => {
  it("shows the raw stored value in a warning state, never options[0]'s label", () => {
    renderWithTheme(
      <Pick value="crunch" options={options} onChange={() => undefined} />,
    );
    // The old bug: this would have silently rendered "Rhythm" (options[0]).
    expect(screen.queryByText("Rhythm")).toBeNull();
    expect(screen.getByText("crunch")).toBeInTheDocument();
    expect(
      screen.getByTitle('"crunch" is no longer offered'),
    ).toBeInTheDocument();
  });

  it("renders the matched option normally when the value IS present", () => {
    renderWithTheme(
      <Pick value="lead" options={options} onChange={() => undefined} />,
    );
    expect(screen.getByText("Lead")).toBeInTheDocument();
    expect(screen.queryByTitle(/no longer offered/)).toBeNull();
  });

  it("an empty value falls back to the first option quietly (not a stale pick)", () => {
    renderWithTheme(
      <Pick value="" options={options} onChange={() => undefined} />,
    );
    expect(screen.getByText("Rhythm")).toBeInTheDocument();
    expect(screen.queryByTitle(/no longer offered/)).toBeNull();
  });
});
