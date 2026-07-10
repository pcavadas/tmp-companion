// Smoke + behavior tests for the design foundation (theme/ + ui/).
//
// Covers the light-only token set, the icon catalogs (Icon + the shared
// BlockArt illustration engine), and the a11y roles on the interactive
// primitives (Checkbox / Toggle / MenuItem).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { light, microLabel } from "../theme/tokens";
import { Icon } from "../ui/Icon";
import { ICONS } from "../ui/iconNames";
import { BlockArt } from "../ui/BlockArt";
import { Checkbox, Toggle, MenuItem, Toast } from "../ui/primitives";
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
    expect(ICONS).toContain("play"); // Doctor tab (WP0)
    expect(ICONS).toContain("pause");
    expect(ICONS).toContain("download"); // Toast update-available status
    expect(ICONS).toContain("info");
    expect(ICONS.length).toBe(40);
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

describe("primitives — a11y roles (VoiceOver)", () => {
  it("Checkbox exposes role=checkbox with aria-checked", () => {
    under(<Checkbox checked />);
    expect(screen.getByRole("checkbox").getAttribute("aria-checked")).toBe(
      "true",
    );
  });

  it("Checkbox indeterminate → aria-checked=mixed", () => {
    under(<Checkbox indeterminate />);
    expect(screen.getByRole("checkbox").getAttribute("aria-checked")).toBe(
      "mixed",
    );
  });

  it("Toggle exposes role=switch with aria-checked", () => {
    under(<Toggle on onClick={() => undefined} />);
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe(
      "true",
    );
  });

  it("MenuItem exposes role=menuitem", () => {
    under(<MenuItem label="Delete" onClick={() => undefined} />);
    expect(screen.getByRole("menuitem")).toBeTruthy();
  });
});

describe("Toast — update-lifecycle statuses", () => {
  it("resolves the label per explicit status (available/success/error)", () => {
    under(<Toast status="available" title="Version 1.4 is ready" />);
    expect(screen.getByText("UPDATE AVAILABLE")).toBeTruthy();

    under(<Toast status="success" title="Done" />);
    expect(screen.getByText("READY")).toBeTruthy();

    under(<Toast status="error" title="Failed" />);
    expect(screen.getByText("FAILED")).toBeTruthy();
  });

  it("downloading has no label row and shows the live percent", () => {
    under(
      <Toast status="downloading" title="Downloading update…" percent={42} />,
    );
    expect(screen.queryByText("UPDATE AVAILABLE")).toBeNull();
    expect(screen.getByText("42%")).toBeTruthy();
  });

  it("legacy warn maps to an amber NOTICE, not the red FAILED tone", () => {
    under(<Toast kind="warn" message="Saved, but BPM didn't stick: x" />);
    expect(screen.getByText("NOTICE")).toBeTruthy();
    expect(screen.queryByText("FAILED")).toBeNull();
  });

  it("legacy err maps to FAILED, with message falling back into the title slot", () => {
    under(<Toast kind="err" message="rename refused" />);
    expect(screen.getByText("FAILED")).toBeTruthy();
    expect(screen.getByText("rename refused")).toBeTruthy();
  });

  it("renders actions and dismisses via the aria-label", () => {
    const onAction = vi.fn();
    const onDismiss = vi.fn();
    under(
      <Toast
        status="success"
        title="Update downloaded"
        actions={[{ label: "Restart now", onClick: onAction, primary: true }]}
        onDismiss={onDismiss}
      />,
    );
    fireEvent.click(screen.getByText("Restart now"));
    expect(onAction).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByLabelText("Dismiss"));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("suppresses the dismiss button while downloading", () => {
    under(
      <Toast
        status="downloading"
        title="Downloading update…"
        percent={10}
        onDismiss={() => undefined}
      />,
    );
    expect(screen.queryByLabelText("Dismiss")).toBeNull();
  });
});
