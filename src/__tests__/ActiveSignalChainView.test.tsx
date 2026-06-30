// Renders the handoff's dual-split path through the real renderer + the shared
// BlockArt engine. Asserts: both splits survive with series blocks in their
// series role; captions are the curated short names from the block-art catalog
// (no baked text, no raw-id slices); and icon/tone resolve BY ID — so tweed and
// blackface combos differ and Filtron is an envelope filter, not a blank box.

import { describe, it, expect, vi, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// jsdom has no SVGGraphicsElement.getBBox; HalfStackArt measures real geometry in a
// layout effect to stack head over cab, so stub it (as BlockArt.test.tsx does).
beforeAll(() => {
  (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox =
    () => ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect;
});

import { ThemeProvider } from "../theme/ThemeProvider";
import { ActiveSignalChainView } from "../views/ActiveSignalChainView";
import { resolveBlockArt } from "../models/blockArt";
import type { ActiveGraph, GraphNode } from "../lib/types";

const blk = (model: string): GraphNode => ({
  group_id: "",
  node_id: model,
  model,
  bypassed: false,
});

// GUITAR → XO Boost → split[57 Deluxe ‖ 65 Princeton] → MIX → Space Delay
//        → split[Small Hall ‖ Filtron] → MIX → Small Hall → OUT
const DUAL_SPLIT: ActiveGraph = {
  name: "Verse — Split",
  slot: 14,
  template: "gtrParallel2",
  split_mix: { splitPoints: [{}, {}], mixPoints: [{}, {}] },
  nodes: [],
  stages: [
    { kind: "series", blocks: [blk("ACD_EPBooster")] },
    {
      kind: "split",
      a: [blk("ACD_TweedDeluxe")],
      b: [blk("ACD_PrincetonReverb65NoFx")],
    },
    { kind: "series", blocks: [blk("ACD_SpaceEcho")] },
    { kind: "split", a: [blk("ACD_TMSmallHall")], b: [blk("ACD_MicroTronIV")] },
    { kind: "series", blocks: [blk("ACD_TMSmallHall")] },
  ],
};

function renderStrip(graph: ActiveGraph) {
  return render(
    <ThemeProvider>
      <ActiveSignalChainView graph={graph} />
    </ThemeProvider>,
  );
}

const count = (hay: string, needle: string) => hay.split(needle).length - 1;

describe("ActiveSignalChainView — dual-split via shared BlockArt engine", () => {
  it("draws BOTH splits (2× SPLIT/MIX)", () => {
    const txt = renderStrip(DUAL_SPLIT).container.textContent;
    expect(count(txt, "SPLIT")).toBe(2);
    expect(count(txt, "MIX")).toBe(2);
  });

  it("keeps series blocks in series, split blocks in lanes (ordering)", () => {
    const txt = renderStrip(DUAL_SPLIT).container.textContent;
    const at = (s: string) => txt.indexOf(s);
    expect(at("GUITAR")).toBeLessThan(at("EP BST"));
    expect(at("EP BST")).toBeLessThan(at("SPLIT")); // pre-split block stays in series
    expect(at("SPLIT")).toBeLessThan(at("57 DLX"));
    expect(at("57 DLX")).toBeLessThan(at("MIX"));
    expect(at("65 PRN")).toBeLessThan(at("MIX"));
    expect(at("MIX")).toBeLessThan(at("RE 201")); // between-splits block in series
    expect(at("RE 201")).toBeLessThan(at("MUTRON")); // 2nd split not flattened
    expect(at("MUTRON")).toBeLessThan(at("OUT"));
  });

  it("captions are curated short names — no baked text, no raw-id slices", () => {
    const txt = renderStrip(DUAL_SPLIT).container.textContent;
    for (const cap of ["EP BST", "57 DLX", "65 PRN", "SM HALL", "MUTRON"]) {
      expect(txt).toContain(cap);
    }
    for (const sliced of [
      "MICRO TRON",
      "TWEED DELUX",
      "TMSMALL HAL",
      "EPBOOSTER",
    ]) {
      expect(txt).not.toContain(sliced);
    }
  });

  it("resolves icon/tone BY ID — Filtron is an envelope filter (Bug 3b)", () => {
    const flt = resolveBlockArt("ACD_MicroTronIV");
    expect(flt).toMatchObject({
      icon: "envf",
      tone: "blue",
      short: "MUTRON",
    });
  });

  it("tweed and blackface combos are different chassis tones", () => {
    expect(resolveBlockArt("ACD_TweedDeluxe")).toMatchObject({
      icon: "combo",
      tone: "tweed",
    });
    expect(resolveBlockArt("ACD_PrincetonReverb65NoFx")).toMatchObject({
      icon: "combo",
      tone: "blackface",
    });
  });

  it("resolves combo variants with cab/IR suffixes to their base art", () => {
    expect(resolveBlockArt("ACD_TweedDeluxeCabIR")).toMatchObject({
      icon: "combo",
      tone: "tweed",
    });
  });

  it("renders the human-facing one-based preset slot (badge)", () => {
    // The slot is a mono badge ("015" = one-based of the 0-based index 14),
    // no longer a "slot NNN · template" sub-line.
    const txt = renderStrip(DUAL_SPLIT).container.textContent;
    expect(txt).toContain("015");
    expect(txt).not.toContain("slot 015");
  });

  it("ghosts identity + chain while the active preset is still arriving", () => {
    const { container } = render(
      <ThemeProvider>
        <ActiveSignalChainView graph={null} presetLoading diagramLoading />
      </ThemeProvider>,
    );
    // Work-in-progress caption (not the disconnected empty state), and the
    // silhouette is ghosted (shimmer fills, no SPLIT/MIX labels).
    expect(screen.getByText("Reading active preset…")).toBeInTheDocument();
    expect(container.querySelectorAll(".tmp-skel").length).toBeGreaterThan(0);
    expect(container.textContent).not.toContain("SPLIT");
  });

  it("reads 'Loading signal chain…' once the identity has resolved but the chain has not", () => {
    render(
      <ThemeProvider>
        <ActiveSignalChainView graph={DUAL_SPLIT} diagramLoading />
      </ThemeProvider>,
    );
    // Identity (the slot badge) stays visible; the chain ghosts; caption is the
    // distinctive signal-chain wording.
    expect(screen.getByText("Loading signal chain…")).toBeInTheDocument();
    expect(screen.getByText("015")).toBeInTheDocument();
  });

  // ── R3 live-sync additions: sceneTag + diagram-fail overlay ─────────────────
  it("renders the live-scene tag (› SCENE) next to the name", () => {
    const { container } = render(
      <ThemeProvider>
        <ActiveSignalChainView
          graph={DUAL_SPLIT}
          sceneTag={{ text: "CHORUS", tone: "#a7461f" }}
        />
      </ThemeProvider>,
    );
    const txt = container.textContent;
    expect(txt).toContain("CHORUS");
    expect(txt).toContain("›"); // the caret precedes a non-neutral tag
  });

  it("the neutral syncing tag is a faint em-dash with NO caret and the catching-up tooltip", () => {
    render(
      <ThemeProvider>
        <ActiveSignalChainView
          graph={DUAL_SPLIT}
          sceneTag={{ text: "—", tone: "#9aa0a9", neutral: true }}
        />
      </ThemeProvider>,
    );
    expect(screen.getByTitle("Catching up to the unit…")).toBeInTheDocument();
  });

  it("the diagram-fail overlay shows the amber chip + a Retry that fires onRetryDiagram", async () => {
    const onRetryDiagram = vi.fn();
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <ActiveSignalChainView
          graph={DUAL_SPLIT}
          diagramError
          onRetryDiagram={onRetryDiagram}
        />
      </ThemeProvider>,
    );
    expect(screen.getByText(/signal view didn't refresh/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /retry/i }));
    expect(onRetryDiagram).toHaveBeenCalledOnce();
  });
});

// A CabSim node carrying its cab params, exactly as the device serializes them.
const cabNode = (
  cabSimId: string,
  opts: { dual?: boolean } = {},
): GraphNode => ({
  group_id: "G1",
  node_id: "ACD_CabSimTMS",
  model: "ACD_CabSimTMS",
  bypassed: false,
  cab_sim_id: cabSimId,
  cab_sim_id2: opts.dual === true ? cabSimId : undefined,
  cab_sim2_enabled: opts.dual === true,
});

// An amp node that carries its own cab (combo/half-stack), e.g. preset 003's HIWAY
// (`...CabIR`) on a British 4×12 — the device sets cabsimid but NO dual-cab split.
const ampWithCab = (model: string, cabSimId: string): GraphNode => ({
  group_id: "G1",
  node_id: model,
  model,
  bypassed: false,
  cab_sim_id: cabSimId,
});

const seriesGraph = (blocks: GraphNode[]): ActiveGraph => ({
  name: "Guitar",
  slot: 1,
  template: "gtrSeries",
  split_mix: null,
  nodes: [],
  stages: [{ kind: "series", blocks }],
});

describe("ActiveSignalChainView — cab tiles match Pro Control", () => {
  it("decomposes a dual-cab CabSim into two parallel British cabs (not the generic CAB IR)", () => {
    const txt = renderStrip(
      seriesGraph([
        blk("ACD_HiwattDR103CanMod"),
        cabNode("Mar1960aV30Alt", { dual: true }),
        blk("ACD_MemoryMan"),
      ]),
    ).container.textContent;
    expect(count(txt, "M4 V30")).toBe(2); // two British cabs, in parallel
    expect(count(txt, "SPLIT")).toBe(1);
    expect(count(txt, "MIX")).toBe(1);
    expect(txt).not.toContain("CAB IR"); // named cabs, not the generic container
    expect(txt).toContain("DR103"); // the amp head renders as a plain amp tile
  });

  it("names a single-cab CabSim from its cabinet — one tile, no split", () => {
    const txt = renderStrip(
      seriesGraph([blk("ACD_HiwattDR103CanMod"), cabNode("Mar1960aV30Alt")]),
    ).container.textContent;
    expect(count(txt, "M4 V30")).toBe(1);
    expect(count(txt, "SPLIT")).toBe(0);
    expect(txt).not.toContain("CAB IR");
  });

  it("a half-stack-form amp head renders as a plain amp tile — no phantom head-on-cab split", () => {
    const txt = renderStrip(seriesGraph([blk("ACD_HiwattDR103CanMod")]))
      .container.textContent;
    expect(txt).toContain("DR103");
    expect(count(txt, "SPLIT")).toBe(0);
    expect(count(txt, "MIX")).toBe(0);
  });

  it("shows the fuller Pro-Control name on hover (title) while the caption stays terse", () => {
    const { container } = renderStrip(
      seriesGraph([blk("ACD_HiwattDR103CanMod")]),
    );
    expect(container.querySelector('[title="HIWAY 105"]')).not.toBeNull();
    expect(container.textContent).toContain("DR103"); // terse caption unchanged
  });

  it("preset 003: a half-stack amp (head + its own cab) renders as ONE tile, NOT a dual-cab split", () => {
    // The amp carries cabsimid (its built-in British 4×12) → a head-over-cab
    // half-stack, NOT two parallel cabs. This is the exact case the old code wrongly
    // split into "M4 V30 ‖ M4 V30"; it must now be one tile, no split.
    const txt = renderStrip(
      seriesGraph([ampWithCab("ACD_HiwattDR103CanModCabIR", "Mar1960aV30Alt")]),
    ).container.textContent;
    expect(txt).toContain("DR103"); // the amp head caption (resolved through CabIR)
    expect(txt).not.toContain("M4 V30"); // cab is stacked art, not a named split tile
    expect(count(txt, "SPLIT")).toBe(0);
    expect(count(txt, "MIX")).toBe(0);
  });
});

describe("ActiveSignalChainView — slot badge", () => {
  it("uses the live active index when the live-switch graph has no slot", () => {
    // On a live preset switch the field-3 graph carries no slot; the badge must
    // come from the live-preset index (activeListIndex), passed as `slot`.
    const noSlot: ActiveGraph = {
      ...seriesGraph([blk("ACD_KlonCentaur")]),
      slot: null,
    };
    const { container } = render(
      <ThemeProvider>
        <ActiveSignalChainView graph={noSlot} slot={2} />
      </ThemeProvider>,
    );
    expect(container.textContent).toContain("003"); // one-based of index 2
  });

  it("falls back to the graph slot when no live index is given (startup)", () => {
    const { container } = render(
      <ThemeProvider>
        <ActiveSignalChainView graph={seriesGraph([blk("ACD_KlonCentaur")])} />
      </ThemeProvider>,
    );
    expect(container.textContent).toContain("002"); // seriesGraph slot 1 → label 002
  });
});
