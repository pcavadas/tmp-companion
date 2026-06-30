// Smoke + behavior tests for the design foundation (theme/ + ui/).
//
// Covers the light-only token set, the icon catalogs (Icon + the shared BlockArt
// illustration engine), and the design-system primitives used by the app
// (Select / Tag / Panel).

import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { light, microLabel } from "../theme/tokens";
import { Icon } from "../ui/Icon";
import { ICONS } from "../ui/iconNames";
import { BlockArt } from "../ui/BlockArt";
import { Select, Tag, Panel } from "../ui/primitives";
import type { ReactNode } from "react";

function under(node: ReactNode) {
  return render(<ThemeProvider>{node}</ThemeProvider>);
}

beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    /* jsdom localStorage edge — non-fatal */
  }
});

describe("tokens — light-only set (README Part 1.1)", () => {
  it("carries the documented light values on the single token object", () => {
    expect(light.bg).toBe("#ffffff");
    expect(light.onInk).toBe("#ffffff");
    expect(light.bgAlt).toBe("#f6f7f9"); // alt
    expect(light.warn).toBe("#a7461f");
    expect(light.good).toBe("#3f7d4e"); // green status
    expect(light.goodSoft).toBe("rgba(63,125,78,0.10)");
    expect(light.accent).toBe("#d97757");
    expect(light.hairlineStrong).toBe("rgba(15,17,21,0.18)"); // hairStrong
    expect(light.hairline).toBe("rgba(15,17,21,0.09)"); // hair
    expect(light.fsTitle).toBe(24); // page header
    expect(light.rMd).toBe(7); // buttons/inputs
    expect(light.rPill).toBe(999);
    expect(typeof light.scrim).toBe("string");
    expect(typeof light.shadowModal).toBe("string");
  });

  it("preserves the repo's existing field names + severity model", () => {
    expect(light.mutedInk).toBe("#6b7280");
    expect(light.sevWarn).toBe("#b07d1c"); // amber
    expect(light.err).toBe("#a7461f");
    expect(light.err).toBe(light.warn); // Toast kind='err' === sevColor(t,'err')
  });

  it("microLabel is an uppercase mono style with the 0.14em kicker tracking", () => {
    const ml = microLabel(light);
    expect(ml.textTransform).toBe("uppercase");
    expect(ml.fontSize).toBe(light.fsMicro);
    expect(ml.letterSpacing).toBe("0.14em");
  });
});

describe("icons — fuller catalogs", () => {
  it("ICONS lists the light-only catalog (no sun/moon) and Icon renders an svg", () => {
    expect(ICONS).toContain("search");
    expect(ICONS).toContain("footswitch");
    expect(ICONS).toContain("gauge"); // prototype page icons added
    expect(ICONS).toContain("cable");
    expect(ICONS).toContain("mic"); // SignalChain mic endpoint node (moved off inline SVG)
    expect(ICONS).toContain("undo"); // Copy offline undo/redo toolbar
    expect(ICONS).toContain("redo");
    expect(ICONS).not.toContain("sun"); // dark toggle removed
    expect(ICONS).not.toContain("moon");
    expect(ICONS).toContain("shield");
    expect(ICONS.length).toBe(36);
    const { container } = render(<Icon name="search" />);
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("BlockArt (shared illustration engine) renders an svg, no baked label text", () => {
    const { container } = under(
      <BlockArt icon="combo" tone="tweed" size={58} label={false} />,
    );
    expect(container.querySelector("svg")).not.toBeNull();
  });
});

describe("primitives — mount + interaction", () => {
  it("Select renders its control + options", () => {
    under(<Select options={["A", "B"]} value="A" />);
    expect(screen.getByRole("combobox")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "B" })).toBeInTheDocument();
  });

  it("Tag renders its label", () => {
    under(<Tag tone="warn">over</Tag>);
    expect(screen.getByText("over")).toBeInTheDocument();
  });

  it("Panel renders title + body", () => {
    under(
      <Panel title="META">
        <span>panel-body</span>
      </Panel>,
    );
    expect(screen.getByText("META")).toBeInTheDocument();
    expect(screen.getByText("panel-body")).toBeInTheDocument();
  });
});
