// src/__tests__/SelectStaleValue.test.tsx — the danger.md Pick/BlockPick trap, ported
// from PickStaleValue.test.tsx to the look B `Select` (which replaces Pick): a stored
// `value` the current `options` no longer contain must DISPLAY that value, never
// silently fall back to `options[0]` — the documented incident ("a store without a
// Crunch target displayed Rhythm while the run got crunch, a silent −30 fallback").
// Select goes one step further than Pick did: an EMPTY value shows a placeholder too
// (Pick showed options[0] for it), so a dropdown never shows a value it won't use.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Select } from "../ui/Select";
import { resolveSelectValue } from "../ui/selectValue";
import { renderWithTheme } from "./testKit";

const options = [
  { value: "rhythm", label: "Rhythm" },
  { value: "lead", label: "Lead" },
];

describe("Select — stored value not in the current options", () => {
  it("shows the raw stored value, never options[0]'s label", () => {
    renderWithTheme(
      <Select
        label="Target"
        value="crunch"
        options={options}
        onChange={() => undefined}
      />,
    );
    // The old bug: this would have silently rendered "Rhythm" (options[0]).
    expect(screen.queryByText("Rhythm")).toBeNull();
    expect(screen.getByText("crunch")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Target: crunch" }),
    ).toBeInTheDocument();
  });

  it("names a stale value with staleLabel when the caller knows it", () => {
    renderWithTheme(
      <Select
        label="Knob"
        value="ACD_Old:outputLevel"
        staleLabel="Output level"
        options={options}
        onChange={() => undefined}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Knob: Output level" }),
    ).toBeTruthy();
    expect(screen.queryByText("Rhythm")).toBeNull();
  });

  it("ticks no option for a stale value (nothing offered is what will be used)", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label="Target"
        value="crunch"
        options={options}
        onChange={() => undefined}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Target: crunch" }));
    for (const o of screen.getAllByRole("option"))
      expect(o.getAttribute("aria-selected")).toBe("false");
  });

  it("tells the caller the value is stale, so its row note can say why", () => {
    expect(resolveSelectValue("crunch", options)).toEqual({ kind: "stale" });
    expect(resolveSelectValue("lead", options)).toEqual({
      kind: "option",
      option: options[1],
    });
    expect(resolveSelectValue("", options)).toEqual({ kind: "empty" });
  });

  it("renders the matched option normally when the value IS present", () => {
    renderWithTheme(
      <Select
        label="Target"
        value="lead"
        options={options}
        onChange={() => undefined}
      />,
    );
    expect(screen.getByText("Lead")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Target: Lead" })).toBeTruthy();
  });

  it("an empty value shows the placeholder, not the first option", () => {
    renderWithTheme(
      <Select
        label="Target"
        value=""
        placeholder="Choose a target"
        options={options}
        onChange={() => undefined}
      />,
    );
    expect(screen.queryByText("Rhythm")).toBeNull();
    expect(screen.getByText("Choose a target")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Target: not chosen" }),
    ).toBeInTheDocument();
  });
});
