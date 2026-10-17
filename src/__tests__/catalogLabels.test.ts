// The Catalog's words for catalog data (views/catalog/catalogLabels): sentence-case
// labels that keep acronyms, the "All …" effect-type choice, the singular Type fact, the
// real-unit line without em dashes, semicolons, id notes or capitals for stress, and the
// detail's "Models the …" line — with the Fender-original and utility wording, a small
// letter for the two lines that start with a plain word, and every brand's capital kept.

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

  it("gives the two generic preamps a small letter mid-sentence, and the tile its capital", () => {
    const studio = row((r) => r.name === "STUDIO PREAMP");
    expect(modelsLine(studio)).toBe(
      "Models the clean studio mixing-desk preamp (uncolored)",
    );
    expect(realLine(studio)).toBe(
      "Clean studio mixing-desk preamp (uncolored)",
    );
    const tube = row((r) => r.name === "TUBE PREAMP");
    expect(modelsLine(tube)).toBe(
      "Models the tube console-style studio preamp",
    );
    expect(realLine(tube)).toBe("Tube console-style studio preamp");
  });

  it("never lowercases a brand after Models the", () => {
    // Rows whose second word is in lowercase too, so a rule keyed on that would hit them.
    for (const brand of [
      "Marshall closed-back",
      "Soldano slant",
      "Fender brownface",
    ]) {
      const r = row((m) => m.real.startsWith(brand));
      expect(modelsLine(r)).toBe(`Models the ${realLine(r)}`);
      expect(modelsLine(r).startsWith(`Models the ${brand}`)).toBe(true);
    }
    // Across the catalog, only the two generic preamps start with a small letter.
    const small = MODELS.filter((r) => /^Models the [a-z]/.test(modelsLine(r)));
    expect(small.map((r) => r.name).sort()).toEqual([
      "STUDIO PREAMP",
      "TUBE PREAMP",
    ]);
  });

  it("words the four catalog lines that carry notes, semicolons or shouted words", () => {
    const byCid = (cid: string) => row((r) => r.cid === cid);
    expect(realLine(byCid("ACD_Bogner_412STU_Celestion_G12H__product"))).toBe(
      "Bogner Uberkab 4x12 (Celestion G12T-75)",
    );
    expect(realLine(byCid("ACD_Rockman__product"))).toBe(
      "Scholz Rockman X100 (1982 original, not the MXR reissue)",
    );
    expect(realLine(byCid("ACD_TMSpring63Conv__product"))).toBe(
      "Fender '63 spring reverb (convolution, Fender original)",
    );
    const spring65 = byCid("ACD_TMSpring65Conv__product");
    expect(realLine(spring65)).toBe(
      "Fender blackface spring reverb (convolution, Fender original)",
    );
    expect(modelsLine(spring65)).toBe(
      "Models the Fender blackface spring reverb (convolution, Fender original)",
    );
  });

  it("shows no semicolon and no word in capitals that isn't a brand or part name", () => {
    // Every all-caps word the catalog's lines use as a name. A new brand or part name
    // goes here; a common word in capitals ("NOT") or an id note ("ID") does not.
    const NAMES = new Set([
      "AKG",
      "B.K.",
      "CTS",
      "EMT",
      "EP",
      "EQ",
      "EVH",
      "GK",
      "II",
      "IIC",
      "IR",
      "ISP",
      "IV",
      "JBL",
      "LTD",
      "MD",
      "MXR",
      "NYC",
      "OCD",
      "POG",
      "RAT",
      "SVT",
      "SWR",
      "TC",
      "TV",
      "UREI",
      "WGS",
    ]);
    for (const r of MODELS) {
      for (const line of [realLine(r), modelsLine(r)]) {
        expect(line, r.cid).not.toContain(";");
        // Letters (and the periods of initials) only: a word with a digit is a
        // model number ("JCM800", "BD-2").
        const caps = line
          .split(/[\s,()/'+]+/)
          .filter(
            (w) => /^[A-Z.]{2,}$/.test(w) && /[A-Z]{2}|[A-Z]\.[A-Z]/.test(w),
          );
        for (const w of caps) expect(NAMES, `${r.cid}: ${w}`).toContain(w);
      }
    }
  });
});
