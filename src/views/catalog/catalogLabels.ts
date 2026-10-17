// src/views/catalog/catalogLabels.ts — the Catalog's words for catalog data (look B):
// category and effect-type labels in sentence case (the catalog's keys stay as they are),
// the singular "Type" fact, the "All …" effect-type choice, a model's real-unit line and
// the detail's "Models the …" line. Model names themselves come from the generated map
// behind `displayName` (models/catalog.ts).

import type { ModelRecord } from "../../models/catalog";

/** An all-caps word ("EQ", "FX", "IR") is an acronym and keeps its capitals. */
const isAcronym = (word: string) => /^[A-Z]{2,}$/.test(word);

/** Sentence case for a catalog label: the first word as it is, later words in lowercase
 *  unless they are acronyms ("Combo Amps" → "Combo amps", "FX Loops" → "FX loops",
 *  "Noise Gate" → "Noise gate", "Graphic EQ" stays). */
export function sentenceCase(label: string): string {
  return label
    .split(" ")
    .map((w, i) => (i === 0 || isAcronym(w) ? w : w.toLowerCase()))
    .join(" ");
}

/** Every word in lowercase except acronyms ("Stompboxes" → "stompboxes", "EQ" stays):
 *  for a label that sits mid-sentence. */
function lowerCase(label: string): string {
  return label
    .split(" ")
    .map((w) => (isAcronym(w) ? w : w.toLowerCase()))
    .join(" ");
}

/** The first choice of the Effect type dropdown ("All stompboxes", "All EQ"). */
export function allOfSub(sub: string): string {
  return `All ${lowerCase(sub)}`;
}

/** One model of each top-level category, for the detail's "Type" fact. Effects use
 *  their subcategory and effect type instead (`typeFact`). */
const CAT_SINGULAR: Partial<Record<string, string>> = {
  "Combo Amps": "Combo amp",
  "Amp Heads": "Amp head",
  "Half Stacks": "Half stack",
  "Bass Amps": "Bass amp",
  Cabinets: "Cabinet",
  Microphones: "Microphone",
  "FX Loops": "FX loop",
  IR: "IR",
};

/** The detail's "Type" fact: "Combo amp", or "Reverb · Spring" for an effect. */
export function typeFact(r: ModelRecord): string {
  if (r.cat === "Effects") {
    const sub = r.sub ?? sentenceCase(r.cat);
    return r.et ? `${sub} · ${sentenceCase(r.et)}` : sub;
  }
  return CAT_SINGULAR[r.cat] ?? sentenceCase(r.cat);
}

/** The real-unit lines the app words itself, by catalog id, until the catalog's own
 *  text is fixed upstream: these carry a semicolon, a note about the block's id, or a
 *  word in capitals for stress, none of which belongs on screen. */
const REAL_LINE_OVERRIDES: Partial<Record<string, string>> = {
  ACD_Bogner_412STU_Celestion_G12H__product:
    "Bogner Uberkab 4x12 (Celestion G12T-75)",
  ACD_Rockman__product:
    "Scholz Rockman X100 (1982 original, not the MXR reissue)",
  ACD_TMSpring63Conv__product:
    "Fender '63 spring reverb (convolution, Fender original)",
  ACD_TMSpring65Conv__product:
    "Fender blackface spring reverb (convolution, Fender original)",
};

/** A model's real-unit line as the app shows it: the app's own wording where it has
 *  one, else the catalog's, with its " — " joins as commas (no em dashes on screen). */
export function realLine(r: ModelRecord): string {
  return REAL_LINE_OVERRIDES[r.cid] ?? r.real.replace(/\s+—\s+/g, ", ");
}

/** The rows whose real-unit line starts with a plain word, not a brand ("Clean studio
 *  mixing-desk preamp"), by catalog id. Mid-sentence, after "Models the", that word
 *  takes a small letter. Listed one by one: a brand never loses its capital. */
const GENERIC_REAL_LINE = new Set([
  "ACD_StudioPreamp__head",
  "ACD_StudioTubePreamp__head",
]);

/** The detail's second line. A Fender original models nothing else, and a utility
 *  block (FX loops, the IR loader, the external cab) is described as it is. */
export function modelsLine(r: ModelRecord): string {
  if (r.real.startsWith("Fender original"))
    return "Fender original, made for the Tone Master Pro";
  if (r.real.startsWith("Fender utility")) return realLine(r);
  const real = realLine(r);
  return GENERIC_REAL_LINE.has(r.cid)
    ? `Models the ${real.charAt(0).toLowerCase()}${real.slice(1)}`
    : `Models the ${real}`;
}
