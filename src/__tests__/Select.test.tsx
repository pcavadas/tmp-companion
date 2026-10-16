// src/__tests__/Select.test.tsx — the look B dropdown (ui/Select): it always shows the
// value that will be used, says when a choice is needed (the attention look), and names
// itself "<what>: <value>" for VoiceOver. The two Pick gate ports live beside it:
// SelectStaleValue.test.tsx (the danger.md options[0] trap) and
// SelectLateContent.test.tsx (the list re-places when its rows land late).

import { describe, it, expect, vi } from "vitest";
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Select } from "../ui/Select";
import type { SelectOption } from "../ui/Select";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

const targets: SelectOption[] = [
  { value: "rhythm", label: "Rhythm" },
  { value: "crunch", label: "Crunch" },
  { value: "lead", label: "Lead" },
];

const LABEL = "Target for Plexi Crunch, Rhythm";

function trigger(): HTMLElement {
  return screen.getByRole("button", { name: new RegExp(`^${LABEL}`) });
}

describe("Select — shows the value that will be used", () => {
  it("shows the chosen option's label, and its accessible name says it", () => {
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={targets}
        onChange={() => undefined}
      />,
    );
    expect(trigger().getAttribute("aria-label")).toBe(`${LABEL}: Crunch`);
    expect(trigger().textContent).toBe("Crunch");
  });

  it("follows the value prop (a controlled dropdown never keeps a stale label)", () => {
    const { rerender } = renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={targets}
        onChange={() => undefined}
      />,
    );
    rerender(
      <Select
        label={LABEL}
        value="lead"
        options={targets}
        onChange={() => undefined}
      />,
    );
    expect(trigger().textContent).toBe("Lead");
  });

  it("a 'for all' select over rows that differ shows Mixed, never a guess", () => {
    renderWithTheme(
      <Select
        label="Target for all sounds"
        value="rhythm"
        mixed
        options={targets}
        onChange={() => undefined}
      />,
    );
    const b = screen.getByRole("button", {
      name: "Target for all sounds: Mixed",
    });
    expect(b.textContent).toBe("Mixed");
    expect(b.hasAttribute("data-attention")).toBe(false);
  });

  it("Mixed is never an option and nothing is ticked in its list", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label="Target for all sounds"
        value="rhythm"
        mixed
        options={targets}
        onChange={() => undefined}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Target for all/ }));
    const opts = screen.getAllByRole("option");
    expect(opts.map((o) => o.textContent)).toEqual([
      "Rhythm",
      "Crunch",
      "Lead",
    ]);
    expect(opts.every((o) => o.getAttribute("aria-selected") === "false")).toBe(
      true,
    );
  });
});

describe("Select — the attention look (a choice is needed first)", () => {
  it("an empty value shows the placeholder in secondary text, attention on", () => {
    renderWithTheme(
      <Select
        label="Scene to level footswitch 2 in"
        value=""
        placeholder="Choose a scene"
        options={targets}
        onChange={() => undefined}
      />,
    );
    const b = screen.getByRole("button", {
      name: "Scene to level footswitch 2 in: not chosen",
    });
    expect(b.textContent).toBe("Choose a scene");
    expect(b.hasAttribute("data-attention")).toBe(true);
    expect(b.style.border).toContain(toRgb(light.cautionLine));
    expect(b.style.boxShadow).toContain("2px");
    const text = screen.getByText("Choose a scene");
    expect(text.style.color).toBe(toRgb(light.textSecondary));
  });

  it("a chosen value has no attention look by default", () => {
    renderWithTheme(
      <Select
        label={LABEL}
        value="lead"
        options={targets}
        onChange={() => undefined}
      />,
    );
    expect(trigger().hasAttribute("data-attention")).toBe(false);
    expect(trigger().style.border).toContain(toRgb(light.controlBorder));
  });

  it("the caller can force or clear the attention look", () => {
    renderWithTheme(
      <>
        <Select
          label="A"
          value="lead"
          attention
          options={targets}
          onChange={() => undefined}
        />
        <Select
          label="B"
          value=""
          attention={false}
          options={targets}
          onChange={() => undefined}
        />
      </>,
    );
    expect(
      screen
        .getByRole("button", { name: "A: Lead" })
        .hasAttribute("data-attention"),
    ).toBe(true);
    expect(
      screen
        .getByRole("button", { name: "B: not chosen" })
        .hasAttribute("data-attention"),
    ).toBe(false);
  });

  it("a disabled select is never in the attention look and does not open", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label="Knob for Lead"
        value=""
        disabled
        options={targets}
        onChange={() => undefined}
      />,
    );
    const b = screen.getByRole("button", { name: "Knob for Lead: not chosen" });
    expect(b).toBeDisabled();
    expect(b.hasAttribute("data-attention")).toBe(false);
    await user.click(b);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("an open list closes when the select turns busy", async () => {
    const user = userEvent.setup();
    const ui = (busy: boolean) => (
      <Select
        label={LABEL}
        value="crunch"
        busy={busy}
        options={targets}
        onChange={() => undefined}
      />
    );
    const { rerender } = renderWithTheme(ui(false));
    await user.click(trigger());
    expect(screen.getByRole("listbox")).toBeTruthy();
    rerender(ui(true));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger().getAttribute("aria-expanded")).toBe("false");
  });

  it("busy shows a spinner and no value, names it as reading, and does not open", () => {
    renderWithTheme(
      <Select
        label="Scene to level footswitch 3 in"
        value=""
        busy
        options={[]}
        onChange={() => undefined}
      />,
    );
    const b = screen.getByRole("button", {
      name: "Scene to level footswitch 3 in: reading",
    });
    expect(b).toBeDisabled();
    expect(b.getAttribute("aria-busy")).toBe("true");
    expect(b.textContent).toBe("");
    expect(b.querySelector(".tmp-spin")).not.toBeNull();
    expect(b.hasAttribute("data-attention")).toBe(false);
  });
});

describe("Select — ARIA and the open list", () => {
  it("is a listbox popup button whose aria-expanded follows the list", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={targets}
        onChange={() => undefined}
      />,
    );
    expect(trigger().getAttribute("aria-haspopup")).toBe("listbox");
    expect(trigger().getAttribute("aria-expanded")).toBe("false");
    await user.click(trigger());
    expect(trigger().getAttribute("aria-expanded")).toBe("true");
    const list = screen.getByRole("listbox", { name: LABEL });
    expect(list).toBeTruthy();
    const picked = screen.getByRole("option", { name: "Crunch" });
    expect(picked.getAttribute("aria-selected")).toBe("true");
    expect(picked.querySelector("svg")).not.toBeNull(); // the tick
    expect(
      screen
        .getByRole("option", { name: "Lead" })
        .getAttribute("aria-selected"),
    ).toBe("false");
  });

  it("choosing an option reports it and closes the list", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={targets}
        onChange={onChange}
      />,
    );
    await user.click(trigger());
    await user.click(screen.getByRole("option", { name: "Lead" }));
    expect(onChange).toHaveBeenCalledWith("lead");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger().getAttribute("aria-expanded")).toBe("false");
  });

  it("clicking the trigger again closes it; a press outside closes it too", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <>
        <Select
          label={LABEL}
          value="crunch"
          options={targets}
          onChange={() => undefined}
        />
        <button type="button">Elsewhere</button>
      </>,
    );
    await user.click(trigger());
    await user.click(trigger());
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.click(trigger());
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("options carry a grey note after the label", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label="Knob for Plexi Crunch, Crunch"
        value="out"
        options={[
          { value: "out", label: "Output level", note: "Volume only, best" },
          { value: "master", label: "Master", note: "Also changes the tone" },
        ]}
        onChange={() => undefined}
      />,
    );
    await user.click(screen.getByRole("button", { name: /^Knob for/ }));
    const note = screen.getByText("Also changes the tone");
    expect(note.style.color).toBe(toRgb(light.textSecondary));
    // The trigger shows the label only, never the note.
    expect(screen.getByRole("button", { name: /^Knob for/ }).textContent).toBe(
      "Output level",
    );
  });

  it("a disabled option is shown but cannot be chosen", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={[...targets, { value: "x", label: "Gain", disabled: true }]}
        onChange={onChange}
      />,
    );
    await user.click(trigger());
    const off = screen.getByRole("option", { name: "Gain" });
    expect(off.getAttribute("aria-disabled")).toBe("true");
    await user.click(off);
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("fires onOpen each time it opens (a lazy read), and its clicks stay its own", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const onRow = vi.fn();
    renderWithTheme(
      <div onClick={onRow}>
        <Select
          label={LABEL}
          value="crunch"
          options={targets}
          onOpen={onOpen}
          onChange={() => undefined}
        />
      </div>,
    );
    await user.click(trigger());
    expect(onOpen).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("option", { name: "Lead" }));
    await user.click(trigger());
    expect(onOpen).toHaveBeenCalledTimes(2);
    expect(onRow).not.toHaveBeenCalled();
  });

  it("while its options are read, the open list says so", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        options={[]}
        loading
        loadingText="Reading knobs…"
        staleLabel="Output level"
        onChange={() => undefined}
      />,
    );
    await user.click(trigger());
    expect(screen.getByText("Reading knobs…")).toBeTruthy();
    expect(screen.queryByRole("option")).toBeNull();
  });

  it("an empty list says there is nothing to choose", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select label={LABEL} value="" options={[]} onChange={() => undefined} />,
    );
    await user.click(trigger());
    expect(screen.getByText("Nothing to choose")).toBeTruthy();
  });

  it("carries the e2e hooks on the trigger and each option", async () => {
    const user = userEvent.setup();
    renderWithTheme(
      <Select
        label={LABEL}
        value="crunch"
        tid="target:P1"
        options={targets}
        onChange={() => undefined}
      />,
    );
    expect(trigger().getAttribute("data-select")).toBe("target:P1");
    await user.click(trigger());
    expect(
      document.querySelector('[data-select-option="target:P1:lead"]'),
    ).not.toBeNull();
  });
});

describe("Select — sizes", () => {
  it("sm is 22 high with 12 px text, md is 28 high with 13 px text", () => {
    renderWithTheme(
      <>
        <Select
          label="S"
          size="sm"
          value="lead"
          options={targets}
          onChange={() => undefined}
        />
        <Select
          label="M"
          value="lead"
          options={targets}
          onChange={() => undefined}
        />
      </>,
    );
    const s = screen.getByRole("button", { name: "S: Lead" });
    const m = screen.getByRole("button", { name: "M: Lead" });
    expect(s.style.height).toBe("22px");
    expect(s.style.fontSize).toBe("12px");
    expect(m.style.height).toBe("28px");
    expect(m.style.fontSize).toBe("13px");
  });

  it("a fixed width applies to the trigger's box", () => {
    renderWithTheme(
      <Select
        label="W"
        width={130}
        value="lead"
        options={targets}
        onChange={() => undefined}
      />,
    );
    const b = screen.getByRole("button", { name: "W: Lead" });
    expect(b.parentElement?.style.width).toBe("130px");
    fireEvent.click(b);
    expect(screen.getByRole("listbox")).toBeTruthy();
  });
});
