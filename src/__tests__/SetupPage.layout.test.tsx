// src/__tests__/SetupPage.layout.test.tsx — the Set up page's row geometry: every sound
// row and preset header is padded 14 px each side, the column heads sit one px further
// in (past the list's border) so each head lines up with its column, and a link that
// says what Start waits for carries its icon in the link's own colour.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";

import { makeSetupOption } from "./commandMocks";
import { renderSetupPage } from "./setupPageTestUtils";

describe("Set up — row geometry", () => {
  it("pads rows and preset headers 14 px and the column heads 15 px", () => {
    renderSetupPage([makeSetupOption({ presetName: "Plexi Crunch" })]);
    const row = document.querySelector("[data-setup-row]");
    const line = row?.firstElementChild as HTMLElement | null;
    expect(line?.style.padding).toBe("0px 14px");
    const header = document.querySelector("[data-setup-preset]");
    const headerLine = header?.firstElementChild as HTMLElement | null;
    expect(headerLine?.style.padding).toBe("0px 14px");
    const heads = screen.getByText("Sound").parentElement;
    expect(heads?.style.padding).toBe("0px 15px 6px");
  });

  it("draws the waiting link's icon at 14 px in the link colour", () => {
    renderSetupPage([makeSetupOption({ presetName: "Plexi Crunch" })], {
      targetNames: [],
      onAddTarget: vi.fn(),
    });
    const link = screen.getByRole("button", {
      name: "Add a target in Settings",
    });
    const icon = link.querySelector("svg");
    expect(icon?.getAttribute("width")).toBe("14");
    expect(icon?.getAttribute("stroke")).toBe("currentColor");
  });
});
