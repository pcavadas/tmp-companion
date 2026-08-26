// src/__tests__/PickStaleValue.test.tsx — the danger.md Pick/BlockPick trap: a stored
// `value` the current `options` set no longer contains must DISPLAY that value with a
// warning, never silently fall back to `options[0]` — the documented incident ("a store
// without a Crunch target displayed Rhythm while the run got crunch, a silent −30
// fallback").

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { Pick } from "../views/overlays/Pick";

const options = [
  { id: "rhythm", label: "Rhythm" },
  { id: "lead", label: "Lead" },
];

describe("Pick — stored value not in the current options", () => {
  it("shows the raw stored value in a warning state, never options[0]'s label", () => {
    render(
      <ThemeProvider>
        <Pick value="crunch" options={options} onChange={() => undefined} />
      </ThemeProvider>,
    );
    // The old bug: this would have silently rendered "Rhythm" (options[0]).
    expect(screen.queryByText("Rhythm")).toBeNull();
    expect(screen.getByText("crunch")).toBeInTheDocument();
    expect(
      screen.getByTitle('"crunch" is no longer offered'),
    ).toBeInTheDocument();
  });

  it("renders the matched option normally when the value IS present", () => {
    render(
      <ThemeProvider>
        <Pick value="lead" options={options} onChange={() => undefined} />
      </ThemeProvider>,
    );
    expect(screen.getByText("Lead")).toBeInTheDocument();
    expect(screen.queryByTitle(/no longer offered/)).toBeNull();
  });

  it("an empty value falls back to the first option quietly (not a stale pick)", () => {
    render(
      <ThemeProvider>
        <Pick value="" options={options} onChange={() => undefined} />
      </ThemeProvider>,
    );
    expect(screen.getByText("Rhythm")).toBeInTheDocument();
    expect(screen.queryByTitle(/no longer offered/)).toBeNull();
  });
});
