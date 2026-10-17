// src/__tests__/LookBPrimitives.test.tsx — the look B kinds and the new primitives in
// ui/primitives.tsx: Button kinds/sizes/busy, the real Checkbox (and its <label>),
// Radio, Toggle, Notice, TextField, TextArea, Slider, SegmentedControl, and the Modal
// confirm (a backdrop click never cancels it).

import { describe, it, expect, vi } from "vitest";
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import {
  Button,
  Checkbox,
  MenuItem,
  Modal,
  Notice,
  Radio,
  SegmentedControl,
  Slider,
  TextArea,
  TextField,
  Toggle,
} from "../ui/primitives";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

describe("Button — look B kinds and sizes", () => {
  it("primary: action fill, white semibold text, 28 high by default", () => {
    renderWithTheme(<Button>Start leveling</Button>);
    const b = screen.getByRole("button", { name: "Start leveling" });
    expect(b.style.background).toBe(toRgb(light.action));
    expect(b.style.color).toBe(toRgb(light.onAction));
    expect(b.style.fontWeight).toBe("600");
    expect(b.style.height).toBe("28px");
    expect(b.style.borderRadius).toBe("6px");
  });

  it("secondary (and its permanent alias ghost): white with a controlBorder edge", () => {
    renderWithTheme(
      <>
        <Button variant="secondary">Cancel</Button>
        <Button variant="ghost">Retry</Button>
      </>,
    );
    for (const name of ["Cancel", "Retry"]) {
      const b = screen.getByRole("button", { name });
      expect(b.style.background).toBe(toRgb(light.surface));
      expect(b.style.border).toBe(`1px solid ${toRgb(light.controlBorder)}`);
      expect(b.style.fontWeight).toBe("500");
    }
  });

  it("danger: danger fill, white text", () => {
    renderWithTheme(<Button variant="danger">Delete setlist</Button>);
    const b = screen.getByRole("button", { name: "Delete setlist" });
    expect(b.style.background).toBe(toRgb(light.danger));
    expect(b.style.color).toBe(toRgb(light.onAction));
  });

  it("plain has no edge; link is link-coloured with no side padding", () => {
    renderWithTheme(
      <>
        <Button variant="plain">Add songs</Button>
        <Button variant="link">Record yours in Settings</Button>
      </>,
    );
    const plain = screen.getByRole("button", { name: "Add songs" });
    expect(plain.style.background).toBe("transparent");
    expect(plain.style.color).toBe(toRgb(light.text));
    const link = screen.getByRole("button", {
      name: "Record yours in Settings",
    });
    expect(link.style.color).toBe(toRgb(light.link));
    expect(link.style.padding).toBe("0px");
  });

  it("bare stays the no-padding native-button reset (NOT the sized plain kind)", () => {
    renderWithTheme(<Button variant="bare">row</Button>);
    const b = screen.getByRole("button", { name: "row" });
    expect(b.style.padding).toBe("0px");
    expect(b.style.height).toBe("");
    expect(b.style.fontSize).toBe("inherit");
  });

  it("sizes: sm 22 (12 px), md 28 (13 px), lg 32 (13 px)", () => {
    renderWithTheme(
      <>
        <Button size="sm">S</Button>
        <Button size="md">M</Button>
        <Button size="lg">L</Button>
      </>,
    );
    const s = screen.getByRole("button", { name: "S" });
    expect(s.style.height).toBe("22px");
    expect(s.style.fontSize).toBe("12px");
    expect(screen.getByRole("button", { name: "M" }).style.height).toBe("28px");
    const l = screen.getByRole("button", { name: "L" });
    expect(l.style.height).toBe("32px");
    expect(l.style.fontSize).toBe("13px");
  });

  it("a disabled primary turns track with textDisabled text", () => {
    renderWithTheme(<Button disabled>Level</Button>);
    const b = screen.getByRole("button", { name: "Level" });
    expect(b).toBeDisabled();
    expect(b.style.background).toBe(toRgb(light.track));
    expect(b.style.color).toBe(toRgb(light.textDisabled));
  });

  it("busy keeps its words, adds a spinner before them and is disabled", () => {
    const onClick = vi.fn();
    renderWithTheme(
      <Button variant="secondary" busy onClick={onClick}>
        Level
      </Button>,
    );
    const b = screen.getByRole("button", { name: "Level" });
    expect(b.textContent).toBe("Level");
    expect(b).toBeDisabled();
    expect(b.getAttribute("aria-busy")).toBe("true");
    expect(b.firstElementChild?.classList.contains("tmp-spin")).toBe(true);
    fireEvent.click(b);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("an icon-only button is square and takes its name from aria-label", () => {
    renderWithTheme(
      <Button
        variant="secondary"
        icon="more"
        aria-label="More for Higher Ground"
      />,
    );
    const b = screen.getByRole("button", { name: "More for Higher Ground" });
    expect(b.style.width).toBe("28px");
    expect(b.style.height).toBe("28px");
    expect(b.querySelector("svg")).not.toBeNull();
  });

  it("the primary button darkens while pressed", () => {
    renderWithTheme(<Button>Go</Button>);
    const b = screen.getByRole("button", { name: "Go" });
    fireEvent.pointerDown(b);
    expect(b.style.background).toBe(toRgb(light.actionPressed));
    fireEvent.pointerUp(b);
    expect(b.style.background).toBe(toRgb(light.action));
  });
});

describe("Checkbox — a real control with an accessible name", () => {
  it("off: white with a controlBorder edge; on: action fill with a tick", () => {
    const { rerender } = renderWithTheme(
      <Checkbox
        checked={false}
        label="Choose Lead"
        labelHidden
        onChange={() => undefined}
      />,
    );
    const box = screen.getByRole("checkbox", { name: "Choose Lead" });
    expect(box.style.background).toBe(toRgb(light.surface));
    expect(box.style.border).toBe(`1px solid ${toRgb(light.controlBorder)}`);
    expect(box.querySelector("svg")).toBeNull();
    rerender(
      <Checkbox
        checked
        label="Choose Lead"
        labelHidden
        onChange={() => undefined}
      />,
    );
    expect(box.style.background).toBe(toRgb(light.action));
    expect(box.querySelector("svg")).not.toBeNull();
  });

  it("mixed is aria-checked=mixed and a click turns it on", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <Checkbox
        checked={false}
        indeterminate
        label="Select all 10 shown"
        labelHidden
        onChange={onChange}
      />,
    );
    const box = screen.getByRole("checkbox", { name: "Select all 10 shown" });
    expect(box.getAttribute("aria-checked")).toBe("mixed");
    await user.click(box);
    expect(onChange.mock.calls[0]?.[0]).toBe(true);
  });

  it("a visible label wraps box and words: a click on the words ticks it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <Checkbox
        checked={false}
        label="Balance its two amps first"
        onChange={onChange}
      />,
    );
    const box = screen.getByRole("checkbox", {
      name: "Balance its two amps first",
    });
    expect(box.closest("label")).not.toBeNull();
    await user.click(screen.getByText("Balance its two amps first"));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange.mock.calls[0]?.[0]).toBe(true);
    // A click on the box itself also ticks it once (no double toggle via the label).
    await user.click(box);
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it("disabled cannot be ticked", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <Checkbox
        checked={false}
        disabled
        label="No volume knob to level"
        onChange={onChange}
      />,
    );
    const box = screen.getByRole("checkbox", {
      name: "No volume knob to level",
    });
    expect(box).toBeDisabled();
    await user.click(screen.getByText("No volume knob to level"));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Radio", () => {
  it("is a named <button role=radio>; on fills the circle in action around a white dot", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const { rerender } = renderWithTheme(
      <Radio checked={false} label="Plexi Crunch" onSelect={onSelect} />,
    );
    const r = screen.getByRole("radio", { name: "Plexi Crunch" });
    expect(r.tagName).toBe("BUTTON");
    expect(r.getAttribute("aria-checked")).toBe("false");
    await user.click(screen.getByText("Plexi Crunch"));
    expect(onSelect).toHaveBeenCalledTimes(1);
    rerender(<Radio checked label="Plexi Crunch" onSelect={onSelect} />);
    expect(r.getAttribute("aria-checked")).toBe("true");
    expect(r.style.background).toBe(toRgb(light.action));
    const dot = r.firstElementChild as HTMLElement | null;
    expect(dot?.style.background).toBe(toRgb(light.onAction));
  });
});

describe("MenuItem", () => {
  it("keeps the 12 px check column before its label, as a Select's list does", () => {
    renderWithTheme(
      <div role="menu">
        <MenuItem label="Edit song" onClick={vi.fn()} />
      </div>,
    );
    const item = screen.getByRole("menuitem", { name: "Edit song" });
    const gutter = item.firstElementChild as HTMLElement | null;
    expect(gutter?.getAttribute("aria-hidden")).toBe("true");
    expect(gutter?.style.width).toBe("12px");
    expect(item.style.gap).toBe("6px");
    expect(item.style.padding).toBe("0px 8px");
  });
});

describe("Toggle — the 26×16 switch", () => {
  it("is 26×16, action when on and switchOff when off", () => {
    const { rerender } = renderWithTheme(
      <Toggle
        on
        label="Install updates automatically"
        onClick={() => undefined}
      />,
    );
    const sw = screen.getByRole("switch", {
      name: "Install updates automatically",
    });
    expect(sw.style.width).toBe("26px");
    expect(sw.style.height).toBe("16px");
    expect(sw.style.background).toBe(toRgb(light.action));
    rerender(
      <Toggle
        on={false}
        label="Install updates automatically"
        onClick={() => undefined}
      />,
    );
    expect(sw.style.background).toBe(toRgb(light.switchOff));
  });
});

describe("Notice — the one inline message", () => {
  it("caution: cautionFill, a status with a bold first sentence and one action", () => {
    const onUse = vi.fn();
    renderWithTheme(
      <Notice
        tone="caution"
        title="Untested firmware"
        action={
          <Button variant="secondary" size="sm" onClick={onUse}>
            Use it anyway
          </Button>
        }
      >
        Your Tone Master Pro runs firmware 1.6.2.
      </Notice>,
    );
    const n = screen.getByRole("status");
    expect(n.style.background).toBe(light.cautionFill.replace(/,/g, ", "));
    expect(screen.getByText("Untested firmware").style.fontWeight).toBe("600");
    fireEvent.click(screen.getByRole("button", { name: "Use it anyway" }));
    expect(onUse).toHaveBeenCalledTimes(1);
  });

  it("danger is an alert; info is a grey status with a border", () => {
    renderWithTheme(
      <>
        <Notice tone="danger">Couldn&apos;t save your settings.</Notice>
        <Notice>Recording needs your Tone Master Pro connected.</Notice>
      </>,
    );
    expect(screen.getByRole("alert").textContent).toContain(
      "Couldn't save your settings.",
    );
    const info = screen.getByRole("status");
    expect(info.style.background).toBe(toRgb(light.groupHeader));
    expect(info.style.border).toContain("1px solid");
  });

  it("an optional close button is named and calls back", () => {
    const onClose = vi.fn();
    renderWithTheme(<Notice onClose={onClose}>Hello</Notice>);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("TextField / TextArea", () => {
  it("TextField: the label above names a 28-high field; typing reports the text", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <TextField
        label="Tempo"
        value=""
        placeholder="Optional"
        onChange={onChange}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Tempo" });
    expect(input.getAttribute("placeholder")).toBe("Optional");
    expect(input.style.height).toBe("28px");
    expect(input.style.border).toBe(`1px solid ${toRgb(light.controlBorder)}`);
    await user.type(input, "9");
    expect(onChange).toHaveBeenCalledWith("9");
    const label = screen.getByText("Tempo");
    expect(label.style.fontSize).toBe("12px");
    expect(label.style.fontWeight).toBe("600");
    expect(label.style.color).toBe(toRgb(light.textSecondary));
  });

  it("TextArea: a named, free-height field", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithTheme(
      <TextArea
        label="What happened?"
        value=""
        placeholder="What did you expect to happen?"
        onChange={onChange}
      />,
    );
    const area = screen.getByRole("textbox", { name: "What happened?" });
    expect(area.tagName).toBe("TEXTAREA");
    expect(area.style.height).toBe("96px");
    // A fixed-height field in a fixed dialog: no resize grip to drag the layout out.
    expect(area.style.resize).toBe("none");
    await user.type(area, "x");
    expect(onChange).toHaveBeenCalledWith("x");
  });
});

describe("Slider", () => {
  function stubTrack(el: HTMLElement) {
    el.getBoundingClientRect = () => ({
      left: 100,
      top: 0,
      right: 300,
      bottom: 18,
      width: 200,
      height: 18,
      x: 100,
      y: 0,
      toJSON: () => ({}),
    });
  }

  it("exposes role=slider with min/max/now and the value as words", () => {
    renderWithTheme(
      <Slider
        label="Rhythm target"
        value={-23}
        min={-29}
        max={-13}
        valueText="−23.0 dB"
        onChange={() => undefined}
        onCommit={() => undefined}
      />,
    );
    const s = screen.getByRole("slider", { name: "Rhythm target" });
    expect(s.getAttribute("aria-valuemin")).toBe("-29");
    expect(s.getAttribute("aria-valuemax")).toBe("-13");
    expect(s.getAttribute("aria-valuenow")).toBe("-23");
    expect(s.getAttribute("aria-valuetext")).toBe("−23.0 dB");
    expect(screen.getByText("Quieter")).toBeTruthy();
    expect(screen.getByText("Louder")).toBeTruthy();
    expect(screen.getByText("−23.0 dB")).toBeTruthy();
  });

  it("a drag reports live values and commits once on release", () => {
    const onChange = vi.fn();
    const onCommit = vi.fn();
    renderWithTheme(
      <Slider
        label="Rhythm target"
        value={-23}
        min={-29}
        max={-13}
        valueText="−23.0 dB"
        onChange={onChange}
        onCommit={onCommit}
      />,
    );
    const s = screen.getByRole("slider");
    stubTrack(s);
    fireEvent.pointerDown(s, { pointerId: 1, clientX: 200 }); // the middle → −21
    expect(onChange).toHaveBeenLastCalledWith(-21);
    fireEvent.pointerMove(s, { pointerId: 1, clientX: 300 }); // the end → −13
    expect(onChange).toHaveBeenLastCalledWith(-13);
    expect(onCommit).not.toHaveBeenCalled();
    fireEvent.pointerUp(s, { pointerId: 1, clientX: 300 });
    expect(onCommit).toHaveBeenCalledWith(-13);
  });

  it("a cancelled drag commits nothing and restores where it started", () => {
    const onChange = vi.fn();
    const onCommit = vi.fn();
    renderWithTheme(
      <Slider
        label="Rhythm target"
        value={-23}
        min={-29}
        max={-13}
        valueText="−23.0 dB"
        onChange={onChange}
        onCommit={onCommit}
      />,
    );
    const s = screen.getByRole("slider");
    stubTrack(s);
    fireEvent.pointerDown(s, { pointerId: 1, clientX: 120 });
    fireEvent.pointerCancel(s, { pointerId: 1 });
    expect(onChange).toHaveBeenLastCalledWith(-23);
    expect(onCommit).not.toHaveBeenCalled();
  });
});

describe("SegmentedControl — look B", () => {
  it("the selected segment is white with the segment shadow, on a segmentTrack track", () => {
    const onChange = vi.fn();
    renderWithTheme(
      <SegmentedControl
        ariaLabel="Browse by"
        value="setlists"
        options={[
          { value: "setlists", label: "Setlists" },
          { value: "presets", label: "Presets" },
        ]}
        onChange={onChange}
      />,
    );
    const group = screen.getByRole("radiogroup", { name: "Browse by" });
    expect(group.style.background).toBe(light.segmentTrack.replace(/,/g, ", "));
    const on = screen.getByRole("radio", { name: "Setlists" });
    expect(on.getAttribute("aria-checked")).toBe("true");
    expect(on.style.backgroundColor).toBe(toRgb(light.surface));
    fireEvent.click(screen.getByRole("radio", { name: "Presets" }));
    expect(onChange).toHaveBeenCalledWith("presets");
  });

  it("a disabled segment is greyed and can't be chosen", () => {
    const onChange = vi.fn();
    renderWithTheme(
      <SegmentedControl
        ariaLabel="Sound"
        size="sm"
        value="all"
        options={[
          { value: "all", label: "All" },
          { value: "stereo", label: "Stereo", disabled: true },
        ]}
        onChange={onChange}
      />,
    );
    const stereo = screen.getByRole("radio", { name: "Stereo" });
    expect(stereo).toBeDisabled();
    expect(stereo.style.color).toBe(toRgb(light.textDisabled));
    fireEvent.click(stereo);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Modal — the confirm, in Dialog's look", () => {
  it("a backdrop click never cancels it; its buttons do", () => {
    const onCancel = vi.fn();
    const onApply = vi.fn();
    renderWithTheme(
      <Modal
        open
        label="Delete Higher Ground?"
        headline="Delete Higher Ground?"
        body="This can't be undone here."
        applyLabel="Delete song"
        applyVariant="danger"
        onApply={onApply}
        onCancel={onCancel}
      />,
    );
    const dialog = screen.getByRole("dialog", {
      name: "Delete Higher Ground?",
    });
    const backdrop = dialog.previousElementSibling;
    if (!backdrop) throw new Error("no backdrop");
    fireEvent.click(backdrop);
    expect(onCancel).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: "Delete Higher Ground?" }).style
        .fontSize,
    ).toBe("15px");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    const del = screen.getByRole("button", { name: "Delete song" });
    expect(del.style.background).toBe(toRgb(light.danger));
    fireEvent.click(del);
    expect(onApply).toHaveBeenCalledTimes(1);
  });
});
