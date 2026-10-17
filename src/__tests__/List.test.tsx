// src/__tests__/List.test.tsx — the look B list (ui/List): Group, Row and Caret.

import { describe, it, expect, vi } from "vitest";
import { fireEvent, screen } from "@testing-library/react";

import { Caret, Group, Row } from "../ui/List";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

/** The parent element, or a failed test (no non-null assertions in this repo). */
function parentOf(el: HTMLElement): HTMLElement {
  const p = el.parentElement;
  if (!p) throw new Error("no parent element");
  return p;
}

describe("Group", () => {
  it("is a white card with a border, radiusGroup and clipped corners", () => {
    renderWithTheme(
      <Group label="Presets">
        <Row>One</Row>
      </Group>,
    );
    const g = screen.getByRole("group", { name: "Presets" });
    expect(g.style.background).toBe(toRgb(light.surface));
    expect(g.style.borderRadius).toBe("10px");
    expect(g.style.overflow).toBe("hidden");
  });

  it("tucks the first row's rule under its edge so only BETWEEN rows is a line", () => {
    renderWithTheme(
      <Group label="Presets">
        <Row>One</Row>
        <>
          <Row header>Two</Row>
          <Row size="compact">Three</Row>
        </>
      </Group>,
    );
    const inner = screen.getByRole("group").firstElementChild as HTMLElement;
    expect(inner.style.marginTop).toBe("-1px");
    // Every row, fragments included, draws a separator rule above itself.
    for (const name of ["One", "Two", "Three"]) {
      const row = parentOf(screen.getByText(name));
      expect(row.style.borderTop).toContain("1px solid");
    }
  });
});

describe("Row", () => {
  function rowOf(text: string): { row: HTMLElement; line: HTMLElement } {
    const line = screen.getByText(text);
    return { row: parentOf(line), line };
  }

  it("heights: regular 36, controls 34, compact 30 (the rule inside the box)", () => {
    renderWithTheme(
      <>
        <Row>r</Row>
        <Row size="controls">c</Row>
        <Row size="compact">s</Row>
      </>,
    );
    expect(rowOf("r").line.style.height).toBe("35px");
    expect(rowOf("c").line.style.height).toBe("33px");
    expect(rowOf("s").line.style.height).toBe("29px");
  });

  it("a header row is groupHeader-filled and 34 high by default", () => {
    renderWithTheme(<Row header>Plexi Crunch</Row>);
    const { row, line } = rowOf("Plexi Crunch");
    expect(row.style.background).toBe(toRgb(light.groupHeader));
    expect(line.style.height).toBe("33px");
  });

  it("selected is the grey rowSelected, never terracotta", () => {
    renderWithTheme(<Row selected>Chosen</Row>);
    expect(rowOf("Chosen").row.style.background).toBe(
      light.rowSelected.replace(/,/g, ", "),
    );
  });

  it("a sub-row indents; a note sits under the line with a caution icon", () => {
    renderWithTheme(
      <Row
        indent={64}
        note="Couldn't read which scene to level footswitch 3 in."
      >
        Lead
      </Row>,
    );
    const { row, line } = rowOf("Lead");
    expect(line.style.padding).toBe("0px 12px 0px 64px");
    const note = screen.getByText(
      "Couldn't read which scene to level footswitch 3 in.",
    );
    expect(row.contains(note)).toBe(true);
    expect(note.parentElement?.style.fontSize).toBe("12px");
    expect(note.parentElement?.querySelector("svg")).not.toBeNull();
  });

  it("padX widens both sides of the line and its note; an indent still wins on the left", () => {
    renderWithTheme(
      <>
        <Row padX={14} note="Choose a block for it.">
          Rhythm
        </Row>
        <Row padX={14} indent={64}>
          Lead
        </Row>
      </>,
    );
    expect(rowOf("Rhythm").line.style.padding).toBe("0px 14px");
    expect(
      screen.getByText("Choose a block for it.").parentElement?.style.padding,
    ).toBe("0px 14px 10px");
    expect(rowOf("Lead").line.style.padding).toBe("0px 14px 0px 64px");
  });

  it("passes data and aria attributes through", () => {
    renderWithTheme(
      <Row data-row="p1" aria-label="Plexi Crunch">
        x
      </Row>,
    );
    expect(document.querySelector('[data-row="p1"]')).not.toBeNull();
  });
});

describe("Caret", () => {
  it("is an 18 px button named Open/Close <name> with aria-expanded", () => {
    const onToggle = vi.fn();
    const { rerender } = renderWithTheme(
      <Caret open={false} name="Plexi Crunch" onToggle={onToggle} />,
    );
    const c = screen.getByRole("button", { name: "Open Plexi Crunch" });
    expect(c.getAttribute("aria-expanded")).toBe("false");
    expect(c.style.width).toBe("18px");
    fireEvent.click(c);
    expect(onToggle).toHaveBeenCalledTimes(1);
    rerender(<Caret open name="Plexi Crunch" onToggle={onToggle} />);
    expect(
      screen
        .getByRole("button", { name: "Close Plexi Crunch" })
        .getAttribute("aria-expanded"),
    ).toBe("true");
  });
});
