// CatalogView interactive coverage — the device-INDEPENDENT model catalog. models-catalog
// .test.tsx pins the DATA layer (every row's taxonomy/art); this drives the VIEW: search
// filtering + empty state, the Mono/Stereo routing facets, the self-disabling Stereo facet
// on a mono-only category, and the CPU sort toggle. (Phase-4 bug-hunt: drive the surface the
// online specs don't reach. CatalogView takes no device props, so it renders standalone.)

import { describe, it, expect, beforeEach, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { CatalogView } from "../views/CatalogView";

// jsdom has no SVGGraphicsElement.getBBox (HalfStackArt measures real geometry in a layout
// effect); stub it so the catalog's block art can render here.
beforeAll(() => {
  (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox =
    () => ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect;
});

function renderCatalog() {
  return render(
    <ThemeProvider>
      <CatalogView />
    </ThemeProvider>,
  );
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
