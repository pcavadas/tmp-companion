// src/__tests__/LookBLayout.test.tsx — the look B page pieces: PageState, Steps,
// PageHead, SplitView (and its RailItems), Footer, Badge,
// the restyled EmptyState, and the Dialog card + its `xs` confirm width.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import { PageState } from "../ui/PageState";
import { Steps } from "../ui/Steps";
import { PageHead } from "../ui/PageHead";
import { PageColumn } from "../ui/PageColumn";
import { SplitView } from "../ui/SplitView";
import { RailItem } from "../ui/Rail";
import { Footer } from "../ui/Footer";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { Dialog, DialogBody } from "../ui/Dialog";
import { Button } from "../ui/primitives";
import { Toolbar } from "../ui/Toolbar";
import { light } from "../theme/tokens";
import { renderWithTheme, toRgb } from "./testKit";

describe("PageState", () => {
  it("centres a toned icon, a 15/600 h1, one body line and its buttons", () => {
    renderWithTheme(
      <PageState
        tone="danger"
        icon="alert"
        title="Something went wrong"
        body="The details are saved for a bug report."
        actions={
          <>
            <Button>Try again</Button>
            <Button variant="secondary">Reload</Button>
          </>
        }
      />,
    );
    const h1 = screen.getByRole("heading", {
      level: 1,
      name: "Something went wrong",
    });
    expect(h1.style.fontSize).toBe("15px");
    expect(h1.style.fontWeight).toBe("600");
    const svg = h1.parentElement?.querySelector("svg");
    expect(svg?.getAttribute("stroke")).toBe(light.danger);
    expect(svg?.getAttribute("width")).toBe("36");
    expect(
      screen.getByText("The details are saved for a bug report.").style.color,
    ).toBe(toRgb(light.textSecondary));
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
  });

  it("tones: caution uses cautionLine, neutral uses textDisabled", () => {
    renderWithTheme(
      <>
        <PageState tone="caution" icon="warn-tri" title="A" />
        <PageState icon="cable" title="B" />
      </>,
    );
    const icon = (name: string) =>
      screen.getByRole("heading", { name }).parentElement?.querySelector("svg");
    expect(icon("A")?.getAttribute("stroke")).toBe(light.cautionLine);
    expect(icon("B")?.getAttribute("stroke")).toBe(light.textDisabled);
  });

  it("busy is one quiet line beside a spinner, with no heading", () => {
    renderWithTheme(
      <PageState busy title="Connecting to your Tone Master Pro…" />,
    );
    const s = screen.getByRole("status");
    expect(s.textContent).toBe("Connecting to your Tone Master Pro…");
    expect(s.querySelector(".tmp-spin")).not.toBeNull();
    expect(screen.queryByRole("heading")).toBeNull();
  });

  it("an optional caption sits under the buttons", () => {
    renderWithTheme(
      <PageState
        tone="caution"
        icon="warn-tri"
        title="Untested firmware"
        actions={<Button variant="secondary">Use it anyway</Button>}
        caption="Some features may not work."
      />,
    );
    expect(screen.getByText("Some features may not work.").style.fontSize).toBe(
      "12px",
    );
  });
});

describe("Toolbar", () => {
  it("tabs are 24 high (13 px on a 16 px line) and the status cell centres its readout", () => {
    renderWithTheme(
      <Toolbar
        tabs={[
          { id: "level", label: "Level" },
          { id: "doctor", label: "Doctor" },
        ]}
        current="level"
        onSelect={() => undefined}
        status={<span>Not connected</span>}
      />,
    );
    const tab = screen.getByRole("button", { name: "Level" });
    expect(tab.style.lineHeight).toBe("16px");
    expect(tab.style.padding).toBe("4px 12px");
    // A block wrapper would sit an inline readout on its text baseline, low in the bar.
    const cell = screen.getByText("Not connected").parentElement;
    expect(cell?.style.display).toBe("flex");
  });
});

describe("Steps", () => {
  it("is an ordered list; the current step is marked and semibold, not clickable", () => {
    renderWithTheme(
      <Steps steps={["Set up", "Level", "Summary"]} current={1} />,
    );
    const list = screen.getByRole("list", { name: "Steps" });
    // The › separators are aria-hidden: VoiceOver hears the three steps only.
    const items = screen.getAllByRole("listitem");
    expect(items.map((i) => i.textContent)).toEqual([
      "Set up",
      "Level",
      "Summary",
    ]);
    expect(list.textContent).toBe("Set up›Level›Summary");
    const cur = screen.getByText("Level");
    expect(cur.getAttribute("aria-current")).toBe("step");
    expect(cur.style.fontWeight).toBe("600");
    expect(cur.style.color).toBe(toRgb(light.text));
    expect(screen.getByText("Set up").getAttribute("aria-current")).toBeNull();
    expect(list.querySelector("button")).toBeNull();
  });
});

describe("PageHead", () => {
  it("a 20/26 semibold h1 with the title tracking, a subtitle and actions", () => {
    renderWithTheme(
      <PageHead
        title="All songs"
        subtitle="10 songs on your Tone Master Pro"
        actions={<Button icon="plus">New song</Button>}
      />,
    );
    const h1 = screen.getByRole("heading", { level: 1, name: "All songs" });
    expect(h1.style.fontSize).toBe("20px");
    expect(h1.style.lineHeight).toBe("26px");
    expect(h1.style.letterSpacing).toBe("-0.01em");
    expect(
      screen.getByText("10 songs on your Tone Master Pro").style.color,
    ).toBe(toRgb(light.textSecondary));
    expect(screen.getByRole("button", { name: "New song" })).toBeTruthy();
  });

  it("holds the Steps at the right of the title", () => {
    renderWithTheme(
      <PageHead
        title="Level 6 sounds"
        actions={<Steps steps={["Set up", "Level", "Summary"]} current={0} />}
      />,
    );
    expect(screen.getByRole("list", { name: "Steps" })).toBeTruthy();
  });
});

describe("SplitView", () => {
  it("lays a named sidebar nav beside the content", () => {
    renderWithTheme(
      <SplitView sidebarLabel="Settings categories" sidebar={<span>side</span>}>
        <span>content</span>
      </SplitView>,
    );
    const nav = screen.getByRole("navigation", { name: "Settings categories" });
    expect(nav.style.width).toBe("200px");
    expect(nav.style.background).toBe(toRgb(light.window));
    expect(nav.textContent).toBe("side");
    expect(screen.getByText("content")).toBeTruthy();
  });

  it("takes a sidebar width (190–210)", () => {
    renderWithTheme(
      <SplitView sidebarLabel="Catalog" sidebarWidth={190} sidebar={null}>
        x
      </SplitView>,
    );
    expect(screen.getByRole("navigation").style.width).toBe("190px");
  });

  it("indents a child RailItem 22 px and marks the current one", () => {
    renderWithTheme(
      <>
        <RailItem label="Effects" active={false} onClick={() => undefined} />
        <RailItem
          indent
          label="Reverb"
          meta={28}
          active
          onClick={() => undefined}
        />
      </>,
    );
    const parent = screen.getByRole("button", { name: "Effects" });
    const child = screen.getByRole("button", { name: /^Reverb/ });
    expect(parent.style.padding).toBe("0px 8px");
    expect(child.style.padding).toBe("0px 8px 0px 22px");
    expect(child).toHaveAttribute("aria-current", "true");
    expect(child.style.background).toBe(light.rowSelected.replace(/,/g, ", "));
  });
});

describe("Footer", () => {
  it("is 56 high on the window grey with a border on top; the main action sits right", () => {
    const { container } = renderWithTheme(
      <Footer
        left={<Button variant="secondary">Cancel</Button>}
        right={<Button size="lg">Start leveling</Button>}
      />,
    );
    const bar = container.firstElementChild as HTMLElement;
    expect(bar.style.minHeight).toBe("56px");
    expect(bar.style.background).toBe(toRgb(light.window));
    expect(bar.style.borderTop).toContain("1px solid");
    const right = screen.getByRole("button", {
      name: "Start leveling",
    }).parentElement;
    expect(right?.style.marginLeft).toBe("auto");
  });
});

describe("Badge", () => {
  it("18 high, 12 px/500, sentence case, no border; tones paint fill + text", () => {
    renderWithTheme(
      <>
        <Badge>Loaded</Badge>
        <Badge tone="caution">Loud peaks</Badge>
        <Badge tone="danger">Couldn&apos;t read</Badge>
        <Badge tone="good" icon="check">
          Recorded
        </Badge>
      </>,
    );
    const loaded = screen.getByText("Loaded");
    expect(loaded.style.height).toBe("18px");
    expect(loaded.style.fontSize).toBe("12px");
    expect(loaded.style.fontWeight).toBe("500");
    expect(loaded.style.border).toBe("");
    expect(loaded.style.color).toBe(toRgb(light.text));
    expect(screen.getByText("Loud peaks").style.color).toBe(
      toRgb(light.caution),
    );
    expect(screen.getByText("Couldn't read").style.color).toBe(
      toRgb(light.danger),
    );
    const rec = screen.getByText("Recorded");
    expect(rec.style.color).toBe(toRgb(light.good));
    expect(rec.querySelector("svg")?.getAttribute("width")).toBe("11");
  });

  it('accent (Copy\'s "Changed"): the terracotta tint with link-coloured words', () => {
    renderWithTheme(<Badge tone="accent">Changed</Badge>);
    const el = screen.getByText("Changed");
    expect(el.style.background).toBe(light.accentFill.replace(/,/g, ", "));
    expect(el.style.color).toBe(toRgb(light.link));
  });
});

describe("EmptyState — the look B empty state", () => {
  it("a 24 px icon, a 13/600 h2 and a secondary body; page size is bigger", () => {
    renderWithTheme(
      <>
        <EmptyState
          icon="search"
          title='No presets match "fuzzz"'
          body="Search by preset name or slot number."
        />
        <EmptyState icon="music" title="No songs yet" size="page" />
      </>,
    );
    const pane = screen.getByRole("heading", {
      name: 'No presets match "fuzzz"',
    });
    expect(pane.tagName).toBe("H2");
    expect(pane.style.fontSize).toBe("13px");
    expect(
      pane.parentElement?.querySelector("svg")?.getAttribute("width"),
    ).toBe("24");
    const page = screen.getByRole("heading", { name: "No songs yet" });
    expect(page.style.fontSize).toBe("15px");
    expect(
      page.parentElement?.querySelector("svg")?.getAttribute("width"),
    ).toBe("32");
  });
});

describe("Dialog — the look B card", () => {
  it("a white radiusGroup card with the dialog shadow; xs is the confirm width", () => {
    renderWithTheme(
      <Dialog size="xs" label="Stop leveling?">
        <DialogBody>Everything matched so far stays saved.</DialogBody>
      </Dialog>,
    );
    const card = screen.getByRole("dialog", { name: "Stop leveling?" });
    expect(card.style.width).toBe("380px");
    expect(card.style.background).toBe(toRgb(light.surface));
    expect(card.style.borderRadius).toBe("10px");
    expect(card.style.border).toBe("");
  });
});

describe("PageColumn", () => {
  // Songs and Settings: the title row and the body share one width-capped column,
  // centred beside the sidebar rather than hugging it.
  it("centres the title row and the body in the same capped column", () => {
    renderWithTheme(
      <PageColumn title="Loudness targets" maxWidth={640}>
        <p>Rows</p>
      </PageColumn>,
    );
    const head = screen
      .getByRole("heading", { name: "Loudness targets" })
      .closest<HTMLElement>('[style*="max-width"]');
    const body = screen.getByText("Rows").parentElement;
    for (const col of [head, body]) {
      expect(col?.style.maxWidth).toBe("640px");
      expect(col?.style.margin).toBe("0px auto");
    }
  });
});
