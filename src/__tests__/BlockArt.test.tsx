// Per-form-factor render coverage for the BlockArt art engine.
//
// The engine is one procedural-SVG file dispatched by formFor():
// amp / cab / mic / treadle / round / wide / rack / desk / screen / pedal. This
// renders ONE representative icon for every form (+ HalfStackArt) and asserts an
// <svg> comes out without throwing — so a broken cross-file helper reference (e.g.
// after splitting the engine into per-form modules) surfaces here instead of only
// at runtime when that form first appears on screen.

import { describe, it, expect, beforeAll } from "vitest";
import { render } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { BlockArt, HalfStackArt } from "../ui/BlockArt";
import { ampCabHalfStack, nodeTileArt, blockArtTile } from "../models/blockArt";
import { clothFor } from "../ui/blockart/shared";

// jsdom has no SVGGraphicsElement.getBBox (HalfStackArt measures real geometry in
// a layout effect to fit head-over-cab); stub it so the component can render here.
beforeAll(() => {
  (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox =
    () => ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect;
});

function svgOf(node: React.ReactNode): SVGSVGElement {
  const { container } = render(<ThemeProvider>{node}</ThemeProvider>);
  const svg = container.querySelector("svg");
  if (!svg) throw new Error("expected the component to render an <svg>");
  return svg;
}

// One representative icon per form-factor (see formFor()).
const PER_FORM: [form: string, icon: string, tone: string][] = [
  ["amp", "combo", "tweed"],
  ["amp/head", "amp", "marshall"],
  ["amp/stack", "stack", "mesa"],
  ["cab", "cab1", "blackface"],
  ["cab/4x12", "cab4", "boutique"],
  ["mic", "mic_sm57", "slate"],
  ["treadle/wah", "wah", "ink"],
  ["treadle/whammy", "whammy", "teal"],
  ["treadle", "treadle", "ink"],
  ["round", "roundfuzz", "fuzzface"],
  ["pedal/octslider", "octslider", "chrome"],
  ["rack", "rack", "graphite"],
  ["rack/tube", "racktube", "graphite"],
  ["desk/synth", "synth", "plum"],
  ["screen", "screen", "slate"],
  ["pedal/od", "od", "green"],
  ["pedal/fuzz", "fuzz", "muff"],
  ["pedal/comp", "comp", "red"],
  ["pedal/delay", "delay", "mint"],
  ["pedal/eq", "eq7", "slate"],
  ["pedal/mod", "chorus", "blue"],
];

describe("BlockArt — every form-factor renders an svg", () => {
  it.each(PER_FORM)("%s (icon=%s) renders", (_form, icon, tone) => {
    const svg = svgOf(
      <BlockArt icon={icon} tone={tone} size={56} label={false} />,
    );
    expect(svg).not.toBeNull();
  });

  it("HalfStackArt (head over cab) renders an svg", () => {
    const svg = svgOf(
      <HalfStackArt
        topIcon="amp"
        topTone="marshall"
        cabIcon="cab4"
        cabTone="boutique"
      />,
    );
    expect(svg).not.toBeNull();
  });
});

// ============================================================================
// Firmware 1.8 Models-tab illustrations — new tones, weaves, amp/cab/pedal
// treatments, concept motifs, and new bodies. Each entry renders an <svg>
// without throwing; selected entries assert the load-bearing element of the
// treatment (skirted-knob skirt, EVH accent square, sparkle/EVH grille fill,
// chicken-head strip, gear-pedal model print, the new bodies' marks).
// ============================================================================

// New tones × every amp form they ship in (combo + head; cab where applicable).
const FW18_AMPS: [name: string, icon: string, tone: string, lab?: string][] = [
  ["silverface combo", "combo", "silverface", "68 CUSTOM DELUXE"],
  ["silverface head", "amp", "silverface", "68 CUSTOM DELUXE"],
  ["evh green combo", "combo", "evhmodern", "EVH 5150 III GREEN"],
  ["evh blue combo", "combo", "evhmodern", "EVH 5150 III BLUE"],
  ["evh red combo", "combo", "evhmodern", "EVH 5150 III RED"],
  ["evh green head", "amp", "evhmodern", "EVH 5150 III GREEN"],
  ["evh twin15 combo", "combo", "evhmodern", "65 TWIN 15"],
];

const FW18_CABS: [name: string, icon: string, tone: string][] = [
  ["silverface 1x12", "cab1", "silverface"],
  ["evh 1x12", "cab1", "evhmodern"],
  ["evh 4x12", "cab4", "evhmodern"],
  ["evh 1x15", "cab15", "evhmodern"],
];

// New pedal icons: the gear pedals (od3 across all 3 lineages), the 2 boosts,
// the 8 Fender-designed concept motifs.
const FW18_PEDALS: [name: string, icon: string, tone: string, lab?: string][] =
  [
    ["pinions od3", "od3", "yellow", "PLUMES"],
    ["runes od3", "od3", "green", "BLUMES"],
    ["lightyear od3", "od3", "blue", "LSPEED"],
    ["integrator boost", "labboost", "graphite", "INTEGRATOR"],
    ["grunt boost", "gruntboost", "black", "GRUNT"],
    ["step tremolo", "steptrem", "slate"],
    ["step filter", "stepfilter", "slate"],
    ["step filter delay", "stepfilterdelay", "slate"],
    ["pitch sequencer", "pitchseq", "slate"],
    ["prismatic delay", "prismdelay", "lavender"],
    ["spectral reverb", "spectralverb", "lavender"],
    ["cirrostratus reverb", "cirrusverb", "frost"],
    ["cirrostratus synthverb", "cirrussynthverb", "frost"],
  ];

describe("BlockArt — firmware 1.8 illustrations", () => {
  it.each(FW18_AMPS)("amp %s renders", (_n, icon, tone, lab) => {
    const svg = svgOf(
      <BlockArt icon={icon} tone={tone} lab={lab} size={56} label={false} />,
    );
    expect(svg).not.toBeNull();
  });

  it.each(FW18_CABS)("cab %s renders", (_n, icon, tone) => {
    const svg = svgOf(
      <BlockArt icon={icon} tone={tone} size={56} label={false} />,
    );
    expect(svg).not.toBeNull();
  });

  it.each(FW18_PEDALS)("pedal %s renders", (_n, icon, tone, lab) => {
    const svg = svgOf(
      <BlockArt icon={icon} tone={tone} lab={lab} size={56} label={false} />,
    );
    expect(svg).not.toBeNull();
  });

  it("Rockbox 100 (rockbox form) renders an svg", () => {
    const svg = svgOf(
      <BlockArt
        icon="rockbox"
        tone="black"
        lab="ROCKBOX 100"
        size={56}
        label={false}
      />,
    );
    expect(svg).not.toBeNull();
  });

  it("Seventy Sixer (rack form) renders an svg", () => {
    const svg = svgOf(
      <BlockArt
        icon="rack"
        tone="slate"
        lab="SEVENTY SIXER"
        size={56}
        label={false}
      />,
    );
    expect(svg).not.toBeNull();
  });

  // ---- element assertions for the load-bearing parts of each treatment ----

  it("silverface combo draws skirted Fender knobs (chrome skirt r=1.95)", () => {
    const svg = svgOf(
      <BlockArt
        icon="combo"
        tone="silverface"
        lab="68 CUSTOM"
        size={56}
        label={false}
      />,
    );
    const skirts = Array.from(svg.querySelectorAll("circle")).filter(
      (c) =>
        c.getAttribute("r") === "1.95" && c.getAttribute("fill") === "#cfd3d7",
    );
    expect(skirts.length).toBe(6); // six skirted knobs on the panel
  });

  it("silverface combo uses the 2-stop brushed-alu gradient", () => {
    const svg = svgOf(
      <BlockArt icon="combo" tone="silverface" size={56} label={false} />,
    );
    const grad = svg.querySelector("linearGradient");
    expect(grad).not.toBeNull();
    if (!grad) throw new Error("expected a <linearGradient>");
    const stops = grad.querySelectorAll("stop");
    expect(stops.length).toBe(3); // 2-stop (3-step) alu gradient only
  });

  it("silverface combo paints its (ref-derived) silver-turquoise sparkle grille base", () => {
    const svg = svgOf(
      <BlockArt icon="combo" tone="silverface" size={56} label={false} />,
    );
    const sparkleBase = Array.from(svg.querySelectorAll("rect")).some(
      (r) => r.getAttribute("fill") === clothFor("silverface").base,
    );
    expect(sparkleBase).toBe(true);
  });

  it("evh white draws the abstract EVH mark (rotated square) centered on cab AND combo", () => {
    const hasAccent = (svg: SVGSVGElement) =>
      Array.from(svg.querySelectorAll("rect")).some((r) =>
        (r.getAttribute("transform") ?? "").startsWith("rotate(45"),
      );
    const cab = svgOf(
      <BlockArt icon="cab4" tone="evhmodern" size={56} label={false} />,
    );
    expect(hasAccent(cab)).toBe(true);
    // the ivory EVH combo carries the same centered logo (logo centered on
    // combos AND cabs)
    const combo = svgOf(
      <BlockArt
        icon="combo"
        tone="evhmodern"
        lab="EVH 5150 III GREEN"
        size={56}
        label={false}
      />,
    );
    expect(hasAccent(combo)).toBe(true);
  });

  it("evh blue combo colours the channel jewel blue", () => {
    const svg = svgOf(
      <BlockArt
        icon="combo"
        tone="evhmodern"
        lab="EVH 5150 III BLUE"
        size={56}
        label={false}
      />,
    );
    const blueJewel = Array.from(svg.querySelectorAll("circle")).some(
      (c) => c.getAttribute("fill") === "#3f78c0",
    );
    expect(blueJewel).toBe(true);
  });

  it("evh red combo colours the channel jewel red", () => {
    const svg = svgOf(
      <BlockArt
        icon="combo"
        tone="evhmodern"
        lab="EVH 5150 III RED"
        size={56}
        label={false}
      />,
    );
    const redJewel = Array.from(svg.querySelectorAll("circle")).some(
      (c) => c.getAttribute("fill") === "#cf3a2e",
    );
    expect(redJewel).toBe(true);
  });

  it("evh 1x12 cab uses the (ref-derived) near-black EVH grille base", () => {
    const svg = svgOf(
      <BlockArt icon="cab1" tone="evhmodern" size={56} label={false} />,
    );
    const evhBase = Array.from(svg.querySelectorAll("rect")).some(
      (r) => r.getAttribute("fill") === clothFor("evhmodern").base,
    );
    expect(evhBase).toBe(true);
  });

  it("gear pedal (od3) is text-free — no model name printed", () => {
    const svg = svgOf(
      <BlockArt
        icon="od3"
        tone="yellow"
        lab="PLUMES"
        size={56}
        label={false}
      />,
    );
    const printed = Array.from(svg.querySelectorAll("text")).map(
      (t) => t.textContent,
    );
    expect(printed).not.toContain("PINIONS");
    expect(printed).toHaveLength(0);
  });

  it("lightyear (blue) gear pedal prints no text (block art carries no wordmarks)", () => {
    const svg = svgOf(
      <BlockArt icon="od3" tone="blue" lab="LSPEED" size={56} label={false} />,
    );
    const printed = Array.from(svg.querySelectorAll("text")).map(
      (t) => t.textContent,
    );
    expect(printed).not.toContain("LIGHTYEAR");
    expect(printed).toHaveLength(0);
  });

  it("integrator boost (labboost) is text-free — no wordmark or knob labels", () => {
    const svg = svgOf(
      <BlockArt
        icon="labboost"
        tone="chrome"
        lab="INTEGRATOR"
        size={56}
        label={false}
      />,
    );
    const printed = Array.from(svg.querySelectorAll("text")).map(
      (t) => t.textContent,
    );
    expect(printed).not.toContain("gain");
    expect(printed).toHaveLength(0);
  });

  it("Seventy Sixer rack draws a lit cream VU meter", () => {
    const svg = svgOf(
      <BlockArt
        icon="rack"
        tone="slate"
        lab="SEVENTY SIXER"
        size={56}
        label={false}
      />,
    );
    const creamVu = Array.from(svg.querySelectorAll("rect")).some(
      (r) => r.getAttribute("fill") === "#efe7cf",
    );
    expect(creamVu).toBe(true);
  });

  // half-stack composites for the new amp lineages (head over their default cab)
  it.each([
    ["silverface", "silverface"],
    ["evhmodern", "evhmodern"],
  ])("HalfStackArt %s head over cab renders", (topTone, cabTone) => {
    const svg = svgOf(
      <HalfStackArt
        topIcon="amp"
        topTone={topTone}
        cabIcon="cab1"
        cabTone={cabTone}
        topLab="EVH 5150 III GREEN"
        cabLab="EVH 5150"
      />,
    );
    expect(svg).not.toBeNull();
  });
});

describe("ampCabHalfStack / nodeTileArt — device-driven half-stack", () => {
  it("an amp carrying a cabsimid (combo/half-stack) → head-over-cab spec", () => {
    // Preset 003: HIWAY head (`...CabIR`) on a British 4×12 (cabsimid Mar1960aV30Alt).
    const head = blockArtTile("ACD_HiwattDR103CanModCabIR");
    const hs = ampCabHalfStack(head, "Mar1960aV30Alt");
    expect(hs).toMatchObject({
      topIcon: "amp",
      topTone: "hiwatt",
      topLab: "DR103",
      cabIcon: "cab4", // the real cabinet, from the device's cabsimid
    });
  });

  it("a bare head (no cabsimid) → no half-stack", () => {
    // Preset 001: HIWAY head with NO cab (feeds a separate dual-cab block).
    const head = blockArtTile("ACD_HiwattDR103CanMod");
    expect(ampCabHalfStack(head, undefined)).toBeUndefined();
    expect(ampCabHalfStack(head, "")).toBeUndefined();
  });

  it("nodeTileArt: amp+cab → half-stack; bare amp → none; CabSim → named cab", () => {
    expect(
      nodeTileArt("ACD_HiwattDR103CanModCabIR", "Mar1960aV30Alt", false)
        .halfStack,
    ).toBeDefined();
    expect(
      nodeTileArt("ACD_HiwattDR103CanMod", undefined, false).halfStack,
    ).toBeUndefined();
    const cab = nodeTileArt("ACD_CabSimTMS", "Mar1960aV30Alt", false);
    expect(cab.halfStack).toBeUndefined();
    expect(cab.name).toBe("M4 V30"); // the standalone cab block is NAMED, not stacked
  });

  it("a COMBO amp with a cabsimid → NO half-stack when isCombo is set", () => {
    // The blonde '65 Twin Reverb is a COMBO: its built-in speaker IS the cabsimid,
    // so on `cabsimid` alone it would wrongly stack (head-over-cab). The isCombo flag
    // is what renders it as the single combo tile instead. Drives the reported bug.
    const cab = "G1265Creamback";
    expect(
      nodeTileArt("ACD_TwinReverb65BlondeNoFx", cab, false).halfStack,
    ).toBeDefined(); // cabsimid alone still stacks — the old (buggy) behavior
    expect(
      nodeTileArt("ACD_TwinReverb65BlondeNoFx", cab, true).halfStack,
    ).toBeUndefined(); // isCombo → single combo tile, no synthesized cab
    // and the combo tile is still the amp's own art (icon resolves to a combo chassis)
    expect(nodeTileArt("ACD_TwinReverb65BlondeNoFx", cab, true).icon).toBe(
      "combo",
    );
  });

  it("cab-driven covering: the attached cab overrides the amp tile's tolex", () => {
    // The reported bug: a blackface '65 Twin HEAD circuit on the cream Creamback cab
    // renders BLONDE on the unit (firmware DUBS_extender.json), but the app showed
    // blackface. The cabsimid now drives the covering.
    expect(
      nodeTileArt("ACD_TwinReverb65NoFxCabIR", "FenTwnRvbTMCream_g12neo", true)
        .tone,
    ).toBe("blonde");
    // the default (Jensen) cab keeps the amp's own blackface covering — no false swap
    expect(
      nodeTileArt("ACD_TwinReverb65NoFxCabIR", "FenTwnRvb_Jensen_C12K", true)
        .tone,
    ).toBe("blackface");
    // Blues Jr IV + a tweed (Jensen) cab → tweed
    expect(
      nodeTileArt("ACD_BluesJrIVCabIR", "BlsJrIV_Jensen_C12N", true).tone,
    ).toBe("tweed");
    // cabsimid carrying the ACD_ prefix still matches
    expect(
      nodeTileArt(
        "ACD_TwinReverb65NoFxCabIR",
        "ACD_FenTwnRvbTMCream_g12neo",
        true,
      ).tone,
    ).toBe("blonde");
    // an amp NOT in the covering whitelist keeps its catalog tone (no override)
    expect(
      nodeTileArt(
        "ACD_HiwattDR103CanModCabIR",
        "FenTwnRvbTMCream_g12neo",
        false,
      ).tone,
    ).toBe(blockArtTile("ACD_HiwattDR103CanModCabIR").tone);
  });
});
