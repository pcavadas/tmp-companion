// CatalogView interactive coverage (look B) — the device-INDEPENDENT model catalog.
// models-catalog.test.tsx pins the DATA layer (every row's taxonomy/art) and
// catalogDisplayNames.test.ts the name mapping; this drives the VIEW: the sidebar, the
// search and its no-match state, the Sound choice (and the self-disabling Stereo segment),
// the count, Sort by CPU (and its direction), the Convolution and Effect type filters, and
// the details that open on a chosen tile. (CatalogView takes no device props, so it
// renders standalone.)

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CatalogView } from "../views/CatalogView";
import {
  CAT_COUNT,
  MODELS,
  SUB_COUNT,
  TOTAL,
  displayName,
  etypesFor,
  type ModelRecord,
} from "../models/catalog";
import { CPU_BUDGET, cpuStr } from "../models/cpu";
import { renderWithTheme } from "./testKit";

// Render probe for the memoized model wall: every ModelTile resolves its art through
// `resolveBlockArt`, so the ids it is called with name exactly which tiles rendered.
// Delegates to the real resolver.
const { artCalls } = vi.hoisted(() => ({ artCalls: [] as string[] }));
vi.mock("../models/blockArt", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../models/blockArt")>();
  return {
    ...actual,
    resolveBlockArt: (bid: string) => {
      artCalls.push(bid);
      return actual.resolveBlockArt(bid);
    },
  };
});

// Each test renders the full model wall (hundreds of BlockArt SVGs) and an interaction
// re-renders it several times over; that's routinely 2-5s even locally and can blow past
// the 5s default under CI's slower runner. Scoped to this file only — a real hang
// elsewhere should still fail fast at the default.
vi.setConfig({ testTimeout: 20000 });

function renderCatalog() {
  return renderWithTheme(<CatalogView />);
}

const sidebar = () => screen.getByRole("navigation", { name: "Categories" });
const sidebarItem = (name: string) =>
  within(sidebar()).getByRole("button", {
    name: new RegExp(`^${name}\\s*\\d+$`),
  });
const tiles = () => document.querySelectorAll<HTMLElement>("[data-model]");
const tile = (r: ModelRecord) => {
  const el = document.querySelector<HTMLElement>(`[data-model="${r.cid}"]`);
  if (!el) throw new Error(`no tile for ${r.cid}`);
  return el;
};
// The wall holds every model, so a page-wide role query walks thousands of nodes and
// can take seconds under a loaded test run: scope queries to the small controls, and
// read the section heads straight from their <h2>s.
const sortBy = () => {
  const el = document.querySelector<HTMLElement>(
    '[role="radiogroup"][aria-label="Sort by"]',
  );
  if (!el) throw new Error("no Sort by control");
  return el;
};
const dirButton = (label: string) => {
  const el = screen.getByText(label).closest("button");
  if (!el) throw new Error(`no ${label} button`);
  return el;
};
const sectionHeads = () =>
  [...document.querySelectorAll("h2")].map((h) => h.textContent);
/** The tiles carrying a "Stereo" badge (the Sound segment says "Stereo" too). */
const stereoBadges = () =>
  screen
    .queryAllByText("Stereo", { exact: true })
    .filter((el) => el.closest("[data-model]") !== null);
const details = () => screen.queryByRole("region", { name: /^Details: / });

describe("CatalogView — sidebar, search, Sound, count", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("lists every category in sentence case, with Effects subcategories always shown", () => {
    renderCatalog();
    expect(sidebarItem("All models")).toHaveTextContent(String(TOTAL));
    expect(sidebarItem("Combo amps")).toHaveTextContent(
      String(CAT_COUNT["Combo Amps"]),
    );
    expect(sidebarItem("FX loops")).toBeInTheDocument();
    expect(sidebarItem("Stompboxes")).toHaveTextContent(
      String(SUB_COUNT.Stompboxes),
    );
    expect(sidebarItem("All models")).toHaveAttribute("aria-current", "true");
    // No "Category" label, and no capitalised category names.
    expect(within(sidebar()).queryByText("Category")).toBeNull();
    expect(within(sidebar()).queryByText("Combo Amps")).toBeNull();
    expect(
      screen.getByPlaceholderText(
        "Search a model, the unit it models, or a brand",
      ),
    ).toBeInTheDocument();
  });

  it("shows model names in normal case", () => {
    renderCatalog();
    const r = MODELS[0];
    expect(tile(r)).toHaveTextContent(displayName(r));
    expect(displayName(r)).not.toBe(displayName(r).toUpperCase());
  });

  it("counts the scope, then counts against it once a filter is on", async () => {
    const user = userEvent.setup();
    renderCatalog();
    expect(screen.getByText(`${String(TOTAL)} models`)).toBeInTheDocument();
    await user.click(sidebarItem("Combo amps"));
    const combos = CAT_COUNT["Combo Amps"];
    expect(screen.getByText(`${String(combos)} models`)).toBeInTheDocument();
    // Mono narrows it: "{n} of {models in Combo amps}".
    await user.click(screen.getByRole("radio", { name: "Mono" }));
    const mono = MODELS.filter(
      (r) => r.cat === "Combo Amps" && r.ch === "mono",
    ).length;
    expect(
      screen.getByText(`${String(mono)} of ${String(combos)}`),
    ).toBeInTheDocument();
  });

  it("search filters the wall; no match says so and offers Clear search", async () => {
    const user = userEvent.setup();
    renderCatalog();
    const box = screen.getByPlaceholderText(/Search a model/i);
    await user.type(box, "zzzznotamodel");
    expect(
      screen.getByRole("heading", { name: 'No models match "zzzznotamodel"' }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Check the spelling, or search by the real amp or pedal it models.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(`0 of ${String(TOTAL)}`)).toBeInTheDocument();
    // The empty state's own button (the field's × is also named "Clear search").
    const clear = screen
      .getAllByRole("button", { name: "Clear search" })
      .find((b) => b.textContent === "Clear search");
    if (!clear) throw new Error("no Clear search button");
    await user.click(clear);
    expect(box).toHaveValue("");
    expect(screen.queryByText(/No models match/)).toBeNull();
    expect(tiles().length).toBe(TOTAL);
  });

  it("Mono hides stereo models and All brings them back", async () => {
    const user = userEvent.setup();
    renderCatalog();
    expect(stereoBadges().length).toBeGreaterThan(0);
    await user.click(screen.getByRole("radio", { name: "Mono" }));
    expect(stereoBadges()).toHaveLength(0);
    await user.click(screen.getByRole("radio", { name: "All" }));
    expect(stereoBadges().length).toBeGreaterThan(0);
  });

  it("greys out Stereo in a mono-only category (Microphones)", async () => {
    const user = userEvent.setup();
    renderCatalog();
    expect(screen.getByRole("radio", { name: "Stereo" })).toBeEnabled();
    await user.click(sidebarItem("Microphones"));
    expect(screen.getByRole("radio", { name: "Stereo" })).toBeDisabled();
  });

  it("with no search, a filter that matches nothing says so without a button", async () => {
    const user = userEvent.setup();
    renderCatalog();
    await user.click(screen.getByRole("radio", { name: "Stereo" }));
    await user.click(sidebarItem("Microphones"));
    expect(
      screen.getByRole("heading", { name: "No models match these filters" }),
    ).toBeInTheDocument();
    expect(
      screen
        .queryAllByRole("button", { name: "Clear search" })
        .filter((b) => b.textContent === "Clear search"),
    ).toHaveLength(0);
  });
});

// #303: Stereo on → a category with no stereo models used to lock the grid on
// "No models match" with the active filter disabled, so it couldn't be turned off.
describe("regression: an active Stereo filter stays changeable in a mono-only category (#303)", () => {
  it("keeps the chosen Stereo segment enabled, and All clears it", async () => {
    const user = userEvent.setup();
    renderCatalog();
    await user.click(screen.getByRole("radio", { name: "Stereo" }));
    await user.click(sidebarItem("Combo amps"));
    expect(
      screen.getByRole("heading", { name: "No models match these filters" }),
    ).toBeInTheDocument();
    // The chosen segment is still live (never greyed while on).
    const stereo = screen.getByRole("radio", { name: "Stereo" });
    expect(stereo).toBeEnabled();
    expect(stereo).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "All" }));
    expect(screen.queryByText(/No models match/)).toBeNull();
    // Now off in a mono-only scope, it greys out.
    expect(screen.getByRole("radio", { name: "Stereo" })).toBeDisabled();
  });
});

describe("CatalogView — Sort by CPU", () => {
  it("shows CPU on tiles only while sorting by CPU, in one ranked section", async () => {
    const user = userEvent.setup();
    renderCatalog();
    const costed = MODELS.find((r) => r.cpu != null);
    if (!costed) throw new Error("no costed model");
    const cpuLine = `${cpuStr(costed.cpu)} CPU`;
    // Sorted by type: no CPU on the tiles, no direction button.
    expect(within(tile(costed)).queryByText(cpuLine)).toBeNull();
    expect(screen.queryByText("Highest first")).toBeNull();

    await user.click(within(sortBy()).getByRole("radio", { name: "CPU" }));
    expect(within(tile(costed)).getByText(cpuLine)).toBeInTheDocument();
    expect(sectionHeads()).toEqual(["Highest CPU first"]);
    // The highest-cost model comes first.
    const top = Math.max(...MODELS.map((r) => r.cpu ?? -1));
    expect(tiles()[0].textContent).toContain(`${cpuStr(top)} CPU`);

    // The direction button flips the order.
    await user.click(dirButton("Highest first"));
    expect(dirButton("Lowest first")).toBeInTheDocument();
    expect(sectionHeads()).toEqual(["Lowest CPU first"]);
    const low = Math.min(
      ...MODELS.filter((r) => r.cpu != null).map((r) => r.cpu ?? 0),
    );
    expect(tiles()[0].textContent).toContain(`${cpuStr(low)} CPU`);

    // Back to Type: CPU leaves the tiles again.
    await user.click(within(sortBy()).getByRole("radio", { name: "Type" }));
    expect(within(tile(costed)).queryByText(cpuLine)).toBeNull();
  });
});

describe("CatalogView — Convolution and Effect type", () => {
  it("offers Convolution only in Effects and Reverb, and it filters", async () => {
    const user = userEvent.setup();
    renderCatalog();
    expect(
      screen.queryByRole("checkbox", { name: "Convolution only" }),
    ).toBeNull();
    await user.click(sidebarItem("Reverb"));
    const conv = screen.getByRole("checkbox", { name: "Convolution only" });
    await user.click(conv);
    const convReverbs = MODELS.filter((r) => r.sub === "Reverb" && r.conv);
    expect(tiles()).toHaveLength(convReverbs.length);
    expect(
      screen.getByText(
        `${String(convReverbs.length)} of ${String(SUB_COUNT.Reverb)}`,
      ),
    ).toBeInTheDocument();
    // Leaving for a scope without convolution models drops the filter and the box.
    await user.click(sidebarItem("Combo amps"));
    expect(
      screen.queryByRole("checkbox", { name: "Convolution only" }),
    ).toBeNull();
    expect(tiles()).toHaveLength(CAT_COUNT["Combo Amps"]);
  });

  it("filters a subcategory by effect type, starting from All", async () => {
    const user = userEvent.setup();
    renderCatalog();
    expect(screen.queryByRole("button", { name: /^Effect type:/ })).toBeNull();
    await user.click(sidebarItem("Stompboxes"));
    const select = screen.getByRole("button", { name: /^Effect type:/ });
    expect(select).toHaveAccessibleName("Effect type: All stompboxes");
    await user.click(select);
    const types = etypesFor("Stompboxes");
    const ty = types[1];
    await user.click(
      document.querySelector<HTMLElement>(
        `[data-select-option="effect-type:${ty}"]`,
      ) ?? select,
    );
    const ofType = MODELS.filter((r) => r.sub === "Stompboxes" && r.et === ty);
    expect(tiles()).toHaveLength(ofType.length);
    expect(
      screen.getByText(
        `${String(ofType.length)} of ${String(SUB_COUNT.Stompboxes)}`,
      ),
    ).toBeInTheDocument();
  });
});

describe("CatalogView — details", () => {
  const amp = MODELS.find((r) => r.cat === "Combo Amps" && r.cpu != null);
  const original = MODELS.find((r) => r.real.startsWith("Fender original"));

  it("opens on a chosen tile with the model's facts, and closes", async () => {
    if (!amp) throw new Error("no combo amp");
    const user = userEvent.setup();
    renderCatalog();
    // Nothing chosen: no details at all (no prompt either).
    expect(details()).toBeNull();
    expect(screen.queryByText(/Select a model/)).toBeNull();

    await user.click(tile(amp));
    expect(tile(amp)).toHaveAttribute("aria-pressed", "true");
    const panel = details();
    if (!panel) throw new Error("no details");
    expect(
      within(panel).getByRole("heading", { name: displayName(amp) }),
    ).toBeInTheDocument();
    expect(panel).toHaveTextContent(/Models the /);
    expect(panel).toHaveTextContent("Combo amp");
    expect(panel).toHaveTextContent(
      `${cpuStr(amp.cpu)} of ${String(CPU_BUDGET)}%`,
    );
    expect(panel).toHaveTextContent(`Firmware ${amp.since}`);
    expect(panel).toHaveTextContent(amp.ch === "stereo" ? "Stereo" : "Mono");

    await user.click(
      within(panel).getByRole("button", { name: "Close details" }),
    );
    expect(details()).toBeNull();
    expect(tile(amp)).toHaveAttribute("aria-pressed", "false");
  });

  it("says a Fender original is made for the Tone Master Pro", async () => {
    if (!original) throw new Error("no Fender original");
    const user = userEvent.setup();
    renderCatalog();
    await user.click(tile(original));
    const panel = details();
    if (!panel) throw new Error("no details");
    expect(panel).toHaveTextContent(
      "Fender original, made for the Tone Master Pro",
    );
    expect(panel).not.toHaveTextContent(/Models /);
  });

  it("choosing another scope clears the chosen model", async () => {
    if (!amp) throw new Error("no combo amp");
    const user = userEvent.setup();
    renderCatalog();
    await user.click(tile(amp));
    expect(details()).not.toBeNull();
    await user.click(sidebarItem("Cabinets"));
    expect(details()).toBeNull();
  });

  describe("in a wide window", () => {
    const narrowWidth = window.innerWidth;
    afterEach(() => {
      act(() => {
        window.innerWidth = narrowWidth;
        window.dispatchEvent(new Event("resize"));
      });
    });

    it("stands beside the wall as a 300 px column", async () => {
      if (!amp) throw new Error("no combo amp");
      const user = userEvent.setup();
      act(() => {
        window.innerWidth = 1440;
        window.dispatchEvent(new Event("resize"));
      });
      renderCatalog();
      await user.click(tile(amp));
      const panel = details();
      if (!panel) throw new Error("no details");
      expect(panel.style.width).toBe("300px");
      expect(panel.style.flexDirection).toBe("column");
    });
  });
});

describe("regression: picking a model tile re-renders only the affected tiles (#315)", () => {
  // Two plain (non-half-stack) models whose block_id appears on ONE row only, so a
  // resolver call names exactly one tile.
  const once = (r: ModelRecord) =>
    r.bid !== null &&
    r.form !== "half_stack" &&
    MODELS.filter((m) => m.bid === r.bid).length === 1;
  const picks = MODELS.filter(once).slice(0, 2);

  it("a pick redraws the picked tile, not the whole wall", async () => {
    const [a, b] = picks;
    const user = userEvent.setup();
    renderCatalog();
    // The whole wall drew once — the probe sees every tile.
    expect(artCalls.length).toBeGreaterThan(300);

    // The details' own art is a fresh tile for the picked model, so a pick resolves
    // the picked id (tile + details) and nothing else.
    artCalls.length = 0;
    await user.click(tile(a));
    expect(tile(a)).toHaveAttribute("aria-pressed", "true");
    expect(new Set(artCalls)).toEqual(new Set([a.bid]));

    // Moving the pick redraws the old and the new tile only.
    artCalls.length = 0;
    await user.click(tile(b));
    expect(tile(a)).toHaveAttribute("aria-pressed", "false");
    expect(tile(b)).toHaveAttribute("aria-pressed", "true");
    expect(new Set(artCalls)).toEqual(new Set([a.bid, b.bid]));
  });
});
