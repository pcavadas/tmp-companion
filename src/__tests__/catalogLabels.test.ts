// The Catalog's words for catalog data (views/catalog/catalogLabels): sentence-case
// labels that keep acronyms, the "All …" effect-type choice, the singular Type fact, the
// real-unit line without em dashes, and the detail's "Models the …" line — with the
// Fender-original and utility wording.

import { describe, it, expect } from "vitest";

import { MODELS, type ModelRecord } from "../models/catalog";
import {
  allOfSub,
  modelsLine,
  realLine,
  sentenceCase,
  typeFact,
} from "../views/catalog/catalogLabels";

const row = (pred: (r: ModelRecord) => boolean) => {
  const r = MODELS.find(pred);
  if (!r) throw new Error("no such catalog row");
  return r;
};

describe("catalog labels", () => {
  it("puts category and effect-type labels in sentence case, keeping acronyms", () => {
    expect(sentenceCase("Combo Amps")).toBe("Combo amps");
    expect(sentenceCase("FX Loops")).toBe("FX loops");
    expect(sentenceCase("Noise Gate")).toBe("Noise gate");
    expect(sentenceCase("Graphic EQ")).toBe("Graphic EQ");
    expect(sentenceCase("IR")).toBe("IR");
  });

  it("names the Effect type dropdown's first choice after the subcategory", () => {
    expect(allOfSub("Stompboxes")).toBe("All stompboxes");
    expect(allOfSub("EQ")).toBe("All EQ");
  });

  it("gives the Type fact in the singular, or subcategory · type for an effect", () => {
    expect(typeFact(row((r) => r.cat === "Combo Amps"))).toBe("Combo amp");
    expect(typeFact(row((r) => r.cat === "Half Stacks"))).toBe("Half stack");
    expect(typeFact(row((r) => r.cat === "FX Loops"))).toBe("FX loop");
    const spring = row((r) => r.sub === "Reverb" && r.et === "Spring");
    expect(typeFact(spring)).toBe("Reverb · Spring");
    const gate = row((r) => r.et === "Noise Gate");
    expect(typeFact(gate)).toBe(`${String(gate.sub)} · Noise gate`);
  });

  it("joins the real unit's parts with commas, never an em dash", () => {
    const dashed = row((r) => r.real.includes(" — "));
    expect(realLine(dashed)).not.toContain("—");
    expect(realLine(dashed)).toBe(dashed.real.replace(" — ", ", "));
    for (const r of MODELS) expect(modelsLine(r)).not.toContain("—");
  });

  it("says what a model models, a Fender original's origin, and a utility as it is", () => {
    const amp = row((r) => r.real.startsWith("Fender '57 Deluxe"));
    expect(modelsLine(amp)).toBe(`Models the ${amp.real}`);
    const original = row((r) => r.real.startsWith("Fender original"));
    expect(modelsLine(original)).toBe(
      "Fender original, made for the Tone Master Pro",
    );
    const loop = row((r) => r.cat === "FX Loops");
    expect(modelsLine(loop)).toBe(realLine(loop));
    expect(modelsLine(loop)).not.toMatch(/^Models/);
  });
});
