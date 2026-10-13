// CatalogView interactive coverage — the device-INDEPENDENT model catalog. models-catalog
// .test.tsx pins the DATA layer (every row's taxonomy/art); this drives the VIEW: search
// filtering + empty state, the Mono/Stereo routing facets, the self-disabling Stereo facet
// on a mono-only category, and the CPU sort toggle. (Phase-4 bug-hunt: drive the surface the
// online specs don't reach. CatalogView takes no device props, so it renders standalone.)

import { describe, it, expect, beforeEach, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CatalogView } from "../views/CatalogView";
import { MODELS, displayName, type ModelRecord } from "../models/catalog";
import { renderWithTheme } from "./testKit";

// Render probe for the memoized model wall: every ModelTile (and the detail bar)
// resolves its art through `resolveBlockArt`, so the ids it is called with name
// exactly which cards rendered. Delegates to the real resolver.
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

// Each test renders the full model wall (hundreds of BlockArt SVGs) and a search/
// facet/sort interaction re-renders it several times over; that's routinely
// 2-5s even locally and can blow past the 5s default under CI's slower runner
// (observed: this file's own "CPU sort"/"disables Stereo facet" cases already
// land at 4-5s). Scoped to this file only — a real hang elsewhere should still
// fail fast at the default.
vi.setConfig({ testTimeout: 20000 });

function renderCatalog() {
  return renderWithTheme(<CatalogView />);
}

describe("CatalogView — search + facets + sort", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders the category rail and the toolbar", () => {
    renderCatalog();
    // A few stable category rail rows (CAT_ORDER) + the search box. Category names also
    // appear as wall section headers, so assert at least one match (the rail row).
    expect(screen.getAllByText("Combo Amps").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Microphones").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Effects").length).toBeGreaterThan(0);
    expect(screen.getByPlaceholderText(/Search a model/i)).toBeInTheDocument();
  });

  it("search filters the wall and shows the empty state for no match", async () => {
    const user = userEvent.setup();
    renderCatalog();
    const box = screen.getByPlaceholderText(/Search a model/i);
    // A gibberish query → the explicit empty state (proves the q filter runs).
    await user.type(box, "zzzznotamodel");
    expect(
      screen.getByText(/No models match .*zzzznotamodel/i),
    ).toBeInTheDocument();
    // Clearing restores models (the empty state is gone).
    await user.clear(box);
    expect(screen.queryByText(/No models match/i)).toBeNull();
  });

  it("the Mono facet hides stereo models (and toggles back off)", async () => {
    const user = userEvent.setup();
    renderCatalog();
    // At "All models" some models are stereo → STEREO tags render on their cards.
    expect(screen.getAllByText("STEREO").length).toBeGreaterThan(0);
    // Mono routing filters them all out.
    await user.click(screen.getByText("Mono"));
    expect(screen.queryAllByText("STEREO")).toHaveLength(0);
    // Toggling Mono off restores them.
    await user.click(screen.getByText("Mono"));
    expect(screen.getAllByText("STEREO").length).toBeGreaterThan(0);
  });

  it("disables the Stereo facet for a mono-only category (Microphones)", async () => {
    const user = userEvent.setup();
    renderCatalog();
    // Stereo is enabled at "All models" (some stereo models in scope).
    expect(screen.getByText("Stereo").style.opacity).toBe("1");
    // Microphones are mono-only → the Stereo facet self-disables (faint, no-op).
    // The rail row is the first "Microphones" (wall section headers repeat the name).
    await user.click(screen.getAllByText("Microphones")[0]);
    const stereo = screen.getByText("Stereo");
    expect(stereo.style.opacity).toBe("0.5");
    // Clicking the disabled facet does nothing — it stays unselected (not white-on-accent).
    await user.click(stereo);
    expect(screen.getByText("Stereo").style.color).not.toBe("#fff");
  });

  // #303: Stereo on → a category with no stereo models used to lock the grid on
  // "No models match" with the active chip disabled, so it couldn't be turned off.
  it("an active Stereo facet stays clearable in a mono-only category (#303)", async () => {
    const user = userEvent.setup();
    renderCatalog();
    await user.click(screen.getByText("Stereo"));
    expect(screen.getByText("Stereo").style.color).toBe("rgb(255, 255, 255)");
    await user.click(screen.getAllByText("Combo Amps")[0]);
    expect(
      screen.getByText(/No models match these filters/i),
    ).toBeInTheDocument();
    // The active chip is still live: clicking it clears the filter.
    const stereo = screen.getByText("Stereo");
    expect(stereo.style.opacity).toBe("1");
    await user.click(stereo);
    expect(screen.queryByText(/No models match/i)).toBeNull();
    // Now off in a mono-only scope, it self-disables as before.
    expect(screen.getByText("Stereo").style.opacity).toBe("0.5");
  });

  it("CPU sort toggles direction", async () => {
    const user = userEvent.setup();
    renderCatalog();
    // First click selects CPU sort (default desc → "CPU ↓").
    await user.click(screen.getByText("CPU"));
    expect(screen.getByText("CPU ↓")).toBeInTheDocument();
    // Second click flips to ascending.
    await user.click(screen.getByText("CPU ↓"));
    expect(screen.getByText("CPU ↑")).toBeInTheDocument();
  });
});

describe("regression: picking a model card re-renders only the affected cards (#315)", () => {
  // Two plain (non-half-stack) models whose block_id appears on ONE row only, so a
  // resolver call names exactly one card.
  const once = (r: ModelRecord) =>
    r.bid !== null &&
    r.form !== "half_stack" &&
    MODELS.filter((m) => m.bid === r.bid).length === 1;
  const picks = MODELS.filter(once).slice(0, 2);
  const card = (r: ModelRecord) =>
    screen.getByTitle(`${displayName(r)} — ${r.real}`);

  it("a pick redraws the picked card (+ the detail bar), not the whole wall", async () => {
    const [a, b] = picks;
    const user = userEvent.setup();
    renderCatalog();
    // The whole wall drew once — the probe sees every card.
    expect(artCalls.length).toBeGreaterThan(300);

    artCalls.length = 0;
    await user.click(card(a));
    expect(card(a)).toHaveAttribute("aria-pressed", "true");
    expect(new Set(artCalls)).toEqual(new Set([a.bid]));

    // Moving the pick redraws the old and the new card only.
    artCalls.length = 0;
    await user.click(card(b));
    expect(card(a)).toHaveAttribute("aria-pressed", "false");
    expect(card(b)).toHaveAttribute("aria-pressed", "true");
    expect(new Set(artCalls)).toEqual(new Set([a.bid, b.bid]));
  });
});
