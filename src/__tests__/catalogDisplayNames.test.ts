// The Catalog's model names in normal case. The unit prints them in capitals
// ("FENDER '57 DELUXE"); `scripts/catalog-display-names.ts` generates ONE mapping file
// (`models/model-display-names.json`) that the app shows instead. These gate it: every
// catalog model has a mapping, the file is what the script generates today (so a rule
// change or a catalog regen without a re-run fails here), and a mapping only ever
// changes letter case — it can never rename a model.

import { describe, it, expect } from "vitest";

import mapping from "../models/model-display-names.json";
import slimGuide from "../models/tmp-model-guide.slim.json";
import { MODELS, displayName, type ModelRecord } from "../models/catalog";
import {
  GENERATED_NOTE,
  buildDisplayNames,
  normalCase,
} from "../../scripts/catalog-display-names";

const names = mapping.names as Partial<Record<string, string>>;

describe("catalog display names (model-display-names.json)", () => {
  it("has a mapping for every catalog model", () => {
    const missing = MODELS.filter((r) => names[r.name] === undefined).map(
      (r) => r.name,
    );
    expect(missing).toEqual([]);
  });

  it("maps no name the catalog doesn't have", () => {
    const catalogNames = new Set(MODELS.map((r) => r.name));
    expect(Object.keys(names).filter((n) => !catalogNames.has(n))).toEqual([]);
  });

  it("is exactly what the script generates from the catalog (re-run it on drift)", () => {
    expect(mapping.generated).toBe(GENERATED_NOTE);
    expect(mapping.names).toEqual(buildDisplayNames(slimGuide.blocks));
  });

  it("only changes letter case", () => {
    for (const [raw, shown] of Object.entries(names)) {
      expect(shown?.toLowerCase(), raw).toBe(raw.toLowerCase());
    }
  });

  it("keeps model numbers, apostrophes and real acronyms", () => {
    expect(normalCase("FENDER '57 DELUXE")).toBe("Fender '57 Deluxe");
    expect(normalCase("FENDER '65 DELUXE REVERB BLONDE NBC")).toBe(
      "Fender '65 Deluxe Reverb Blonde NBC",
    );
    expect(normalCase("EVH 5150IIIS 6L6 GREEN")).toBe("EVH 5150IIIS 6L6 Green");
    expect(normalCase("2X12 UK30 ALNICO BLUE")).toBe("2x12 UK30 Alnico Blue");
    expect(normalCase("'70S FLANGER")).toBe("'70s Flanger");
    expect(normalCase("CSD (COMIC SANS DRIVE)")).toBe("CSD (Comic Sans Drive)");
    expect(normalCase("EQ-7W WIDE-RANGE BASS GRAPHIC")).toBe(
      "EQ-7W Wide-Range Bass Graphic",
    );
    expect(normalCase("MYTHIC DRIVE II")).toBe("Mythic Drive II");
    expect(normalCase("MARKSMAN CH1")).toBe("Marksman Ch1");
    expect(normalCase("DYNAMIC SM57")).toBe("Dynamic SM57");
  });

  it("leaves a name the unit already prints in normal case alone", () => {
    expect(normalCase("1x10 '65 Princeton GB")).toBe("1x10 '65 Princeton GB");
    expect(normalCase("FX Loop 3+4 Stereo")).toBe("FX Loop 3+4 Stereo");
  });
});

describe("displayName", () => {
  const row = (pred: (r: ModelRecord) => boolean) => {
    const r = MODELS.find(pred);
    if (!r) throw new Error("no such catalog row");
    return r;
  };

  it("shows the mapped name, with (Combo) / (Head) where an amp comes in both", () => {
    const combo = row((r) => r.bid === "ACD_TweedDeluxe" && r.form === "combo");
    const head = row((r) => r.bid === "ACD_TweedDeluxe" && r.form === "head");
    expect(displayName(combo)).toBe("Fender '57 Deluxe (Combo)");
    expect(displayName(head)).toBe("Fender '57 Deluxe (Head)");
  });

  it("shows the plain mapped name for a model in one form only", () => {
    const fuzz = row((r) => r.name === "BIG APPLE FUZZ");
    expect(displayName(fuzz)).toBe("Big Apple Fuzz");
  });
});
