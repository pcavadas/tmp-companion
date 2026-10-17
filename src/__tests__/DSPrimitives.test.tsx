// Smoke + behavior tests for the design foundation (theme/ + ui/).
//
// Covers the light-only token set, the icon catalogs (Icon + the shared
// BlockArt illustration engine), and the a11y roles on the interactive
// primitives (Checkbox / CheckboxMark / Toggle / MenuItem).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { light, microLabel, sectionLabel } from "../theme/tokens";
import { Icon } from "../ui/Icon";
import { ICONS } from "../ui/iconNames";
import { BlockArt } from "../ui/BlockArt";
import {
  Checkbox,
  CheckboxMark,
  Toggle,
  MenuItem,
  Toast,
  Button,
  SearchInput,
} from "../ui/primitives";
import { Tag } from "../ui/Tag";
import { EmptyState } from "../ui/EmptyState";
import { PaneEmpty } from "../ui/PaneEmpty";
import { Spinner } from "../ui/Spinner";
import { Dot } from "../ui/Dot";
import { renderWithTheme, toRgb } from "./testKit";

beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    /* jsdom localStorage edge — non-fatal */
  }
});

/** Every .ts/.tsx source under `dir`, test files excluded. */
function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "__tests__" ? [] : sourceFiles(p);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

/** The signal-path diagram's files: the only readers `bgAlt` may have. */
const DIAGRAM_FILES = [
  /^ui\/BlockArt\.tsx$/,
  /^ui\/blockart\//,
  /^models\/blockArt\.ts$/,
  /^views\/SignalChainView\.tsx$/,
  /^views\/ActiveSignalChainView\.tsx$/,
  /^views\/signalChain\//,
];

describe("tokens — light-only set, look B", () => {
  it("carries the B values on the single token object", () => {
    expect(light.window).toBe("#f5f5f7");
    expect(light.surface).toBe("#ffffff");
    expect(light.groupHeader).toBe("#fafafa");
    expect(light.text).toBe("#1d1d1f");
    expect(light.textSecondary).toBe("#6e6e73");
    expect(light.accent).toBe("#d97757");
    expect(light.action).toBe("#bd5632");
    expect(light.fontSans).toBe("'Inter', system-ui, sans-serif");
    expect(light.fontTitle).toBe(20);
    expect(light.sp24).toBe(24);
    expect(light.radiusGroup).toBe(10);
  });

  it("gives the four same-name tokens their B value", () => {
    expect(light.good).toBe("#246b3a");
    expect(light.track).toBe("#e8e8ed");
    expect(light.scrim).toBe("rgba(0,0,0,0.25)");
    expect(light.shadowMenu).toBe(
      "0 8px 24px rgba(0,0,0,0.14), 0 0 0 0.5px rgba(0,0,0,0.12)",
    );
  });

  it("keeps the signal-path tokens frozen, except bgAlt, which turns white", () => {
    expect(light.bgAlt).toBe("#ffffff"); // the diagram sits on white
    expect(light.bg).toBe("#ffffff");
    expect(light.ink).toBe("#0f1115");
    expect(light.mutedInk).toBe("#6b7280");
    expect(light.hairline).toBe("rgba(15,17,21,0.09)");
    expect(light.hairlineStrong).toBe("rgba(15,17,21,0.18)");
    expect(light.warn).toBe("#a7461f");
    expect(light.sevWarn).toBe("#b07d1c");
  });

  it("keeps the old aliases at their old values until their readers move", () => {
    expect(light.onInk).toBe("#ffffff");
    expect(light.goodSoft).toBe("rgba(63,125,78,0.10)");
    expect(light.fsTitle).toBe(24);
    expect(light.rMd).toBe(7);
    expect(light.rPill).toBe(999);
    expect(light.shadowModal).toBe("0 40px 80px -16px rgba(15,17,21,0.28)");
  });

  it("nothing outside the signal-path diagram reads bgAlt (it is white now)", () => {
    const src = join(process.cwd(), "src");
    const readers = sourceFiles(src)
      .map((f) => relative(src, f).split("\\").join("/"))
      .filter((rel) => !DIAGRAM_FILES.some((re) => re.test(rel)))
      .filter((rel) => /\.bgAlt\b/.test(readFileSync(join(src, rel), "utf8")));
    expect(readers).toEqual([]);
  });

  it("sectionLabel is sentence-case 12 px semibold secondary text", () => {
    const sl = sectionLabel(light);
    expect(sl.textTransform).toBeUndefined();
    expect(sl.fontSize).toBe(12);
    expect(sl.fontWeight).toBe(600);
    expect(sl.color).toBe(light.textSecondary);
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
    expect(ICONS).toContain("link"); // Doctor chain prescription + shared-block caption
    expect(ICONS).toContain("chev-updown"); // look B Select chevron
    expect(ICONS).toContain("minus"); // look B mixed Checkbox
    expect(ICONS).toContain("alert"); // look B danger Notice / PageState
    expect(ICONS).toContain("usb"); // look B "Plug in your Tone Master Pro" page
    expect(ICONS).toContain("stop"); // look B leveling run's Stop
    // look B Doctor's Quiet, Rehearsal and Stage volumes
    expect(ICONS).toEqual(
      expect.arrayContaining(["headphones", "speaker", "pulse"]),
    );
    expect(ICONS.length).toBe(49);
    const { container } = render(<Icon name="search" />);
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("BlockArt (shared illustration engine) renders an svg, no baked label text", () => {
    const { container } = renderWithTheme(
      <BlockArt icon="combo" tone="tweed" size={58} label={false} />,
    );
    expect(container.querySelector("svg")).not.toBeNull();
  });
});

describe("primitives — a11y roles (VoiceOver)", () => {
  it("CheckboxMark is the visual box only — its owning button carries the role", () => {
    const { container } = renderWithTheme(<CheckboxMark checked />);
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull();
  });

  it("Checkbox is a real, named <button role=checkbox>", () => {
    renderWithTheme(
      <Checkbox
        checked={false}
        label="Choose Plexi Crunch"
        labelHidden
        onChange={() => undefined}
      />,
    );
    const box = screen.getByRole("checkbox", { name: "Choose Plexi Crunch" });
    expect(box.tagName).toBe("BUTTON");
    expect(box.getAttribute("aria-checked")).toBe("false");
  });

  it("Toggle is a named <button role=switch> with aria-checked", () => {
    renderWithTheme(
      <Toggle on label="Auto-install" onClick={() => undefined} />,
    );
    const sw = screen.getByRole("switch", { name: "Auto-install" });
    expect(sw.tagName).toBe("BUTTON");
    expect(sw.getAttribute("aria-checked")).toBe("true");
  });

  it("display-only Toggle renders no control", () => {
    renderWithTheme(<Toggle on />);
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("MenuItem is a <button role=menuitem>", () => {
    renderWithTheme(<MenuItem label="Delete" onClick={() => undefined} />);
    expect(screen.getByRole("menuitem", { name: "Delete" }).tagName).toBe(
      "BUTTON",
    );
  });

  it("Button passes native props through and hands onClick the event", () => {
    const seen: string[] = [];
    renderWithTheme(
      <Button
        variant="bare"
        aria-label="Close"
        title="Close panel"
        onClick={(e) => {
          seen.push(e.type);
        }}
      >
        x
      </Button>,
    );
    const b = screen.getByRole("button", { name: "Close" });
    expect(b.getAttribute("title")).toBe("Close panel");
    fireEvent.click(b);
    expect(seen).toEqual(["click"]);
  });

  it("SearchInput's clear (×) is a named button", () => {
    const changes: string[] = [];
    renderWithTheme(
      <SearchInput
        value="abc"
        clearable
        onChange={(v) => {
          changes.push(v);
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(changes).toEqual([""]);
  });
});

describe("Toast — update-lifecycle statuses", () => {
  it("has no uppercase status chip: the title says it, the role carries the tone", () => {
    renderWithTheme(<Toast status="available" title="Version 1.4 is ready" />);
    expect(screen.getByRole("status").textContent).toBe("Version 1.4 is ready");
    expect(screen.queryByText("UPDATE AVAILABLE")).toBeNull();
  });

  it("an error toast is an alert; success and available are statuses", () => {
    renderWithTheme(<Toast status="error" title="Failed" />);
    expect(screen.getByRole("alert").textContent).toContain("Failed");
    expect(screen.queryByText("FAILED")).toBeNull();
  });

  it("downloading shows the live percent and a progress bar", () => {
    renderWithTheme(
      <Toast status="downloading" title="Downloading update…" percent={42} />,
    );
    expect(screen.getByText("42%")).toBeTruthy();
  });

  it("legacy warn maps to the caution notice, not the danger alert", () => {
    renderWithTheme(
      <Toast kind="warn" message="Saved, but BPM didn't stick: x" />,
    );
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("legacy err maps to an alert, with message falling back into the title slot", () => {
    renderWithTheme(<Toast kind="err" message="rename refused" />);
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(screen.getByText("rename refused")).toBeTruthy();
  });

  it("renders actions and dismisses via the aria-label", () => {
    const onAction = vi.fn();
    const onDismiss = vi.fn();
    renderWithTheme(
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
    renderWithTheme(
      <Toast
        status="downloading"
        title="Downloading update…"
        percent={10}
        onDismiss={() => undefined}
      />,
    );
    expect(screen.queryByLabelText("Dismiss")).toBeNull();
  });

  it("sits at the bottom centre, on white with the menu shadow", () => {
    renderWithTheme(<Toast status="success" title="Saved" />);
    const card = screen.getByRole("status");
    const strip = card.parentElement;
    expect(strip?.style.justifyContent).toBe("center");
    expect(strip?.style.position).toBe("fixed");
    expect(card.style.background).toBe(toRgb(light.surface));
    expect(card.style.boxShadow).toContain("24px");
  });
});

describe("Tag — the old chip, a shim over Badge", () => {
  it("renders its children text verbatim", () => {
    renderWithTheme(<Tag>FS1</Tag>);
    expect(screen.getByText("FS1")).toBeTruthy();
  });

  it("maps its tones onto Badge tones (warn → caution, neutralFill → neutral)", () => {
    renderWithTheme(
      <>
        <Tag tone="accent">a</Tag>
        <Tag tone="good">g</Tag>
        <Tag tone="warn">w</Tag>
        <Tag tone="neutralFill">n</Tag>
      </>,
    );
    expect(screen.getByText("a").style.color).toBe(toRgb(light.link));
    expect(screen.getByText("g").style.color).toBe(toRgb(light.good));
    expect(screen.getByText("w").style.color).toBe(toRgb(light.caution));
    expect(screen.getByText("n").style.color).toBe(toRgb(light.text));
  });

  it("size has no Badge equivalent: one size", () => {
    renderWithTheme(
      <>
        <Tag size="sm">a</Tag>
        <Tag size="md">b</Tag>
      </>,
    );
    expect(screen.getByText("a").style.fontSize).toBe("12px");
    expect(screen.getByText("b").style.fontSize).toBe("12px");
  });

  it("uppercase is ignored — sentence case, and the child string is never edited", () => {
    renderWithTheme(<Tag uppercase>Rhythm</Tag>);
    const el = screen.getByText("Rhythm");
    expect(el.style.textTransform).toBe("");
    expect(el.textContent).toBe("Rhythm");
  });

  it("a custom fg colour has no Badge equivalent: the badge renders neutral", () => {
    renderWithTheme(
      <Tag tone="accent" fg={light.warn}>
        x
      </Tag>,
    );
    const el = screen.getByText("x");
    expect(el.style.color).toBe(toRgb(light.text));
    expect(el.style.border).toBe("");
  });
});

describe("Spinner", () => {
  it("wraps a spinner Icon in the .tmp-spin sweep", () => {
    const { container } = renderWithTheme(<Spinner />);
    const span = container.querySelector("span.tmp-spin");
    expect(span).not.toBeNull();
    expect(span?.querySelector("svg")).not.toBeNull();
  });

  it("renders a custom icon name", () => {
    const { container } = renderWithTheme(<Spinner name="refresh" />);
    expect(container.querySelector("span.tmp-spin svg")).not.toBeNull();
  });
});

describe("EmptyState (was PaneEmpty)", () => {
  it("renders the icon medallion, title, and body", () => {
    const { container } = renderWithTheme(
      <EmptyState icon="music" title="Nothing here" body="Add some things." />,
    );
    expect(screen.getByText("Nothing here")).not.toBeNull();
    expect(screen.getByText("Add some things.")).not.toBeNull();
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("renders a CTA when passed, and none otherwise", () => {
    renderWithTheme(
      <EmptyState
        icon="music"
        title="t"
        body="b"
        cta={<Button>Add songs</Button>}
      />,
    );
    expect(screen.getByText("Add songs")).not.toBeNull();

    const { container } = renderWithTheme(
      <EmptyState icon="wave" title="t2" body="b2" />,
    );
    expect(container.querySelector("button")).toBeNull();
  });

  it("PaneEmpty stays the same component until the clean-up step", () => {
    expect(PaneEmpty).toBe(EmptyState);
  });
});

describe("Dot", () => {
  it("renders an 8px aria-hidden pip by default with the given colour", () => {
    const { container } = renderWithTheme(<Dot color={light.good} />);
    const el = container.querySelector("span");
    expect(el?.style.width).toBe("8px");
    expect(el?.style.height).toBe("8px");
    expect(el?.style.background).toBe(toRgb(light.good));
    expect(el?.getAttribute("aria-hidden")).toBe("true");
  });

  it("honors a size override", () => {
    const { container } = renderWithTheme(<Dot color={light.ink} size={12} />);
    expect(container.querySelector("span")?.style.width).toBe("12px");
  });

  it("a tone wins over a colour; hollow draws a ring", () => {
    const { container } = renderWithTheme(
      <Dot tone="danger" color={light.good} hollow />,
    );
    const el = container.querySelector("span");
    expect(el?.style.background).toBe("transparent");
    expect(el?.style.border).toContain(toRgb(light.danger));
  });
});
