// Guards the Models catalog ingest against the device-authoritative counts.
// The unified catalog (tmp-model-guide.json) carries all firmware versions with
// inline `since` fields. A drift here means the source JSON changed or the
// ingest mapping regressed.

import { describe, it, expect, beforeAll } from "vitest";
import { render } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { BlockArt } from "../ui/BlockArt";
import {
  MODELS,
  TOTAL,
  CAT_COUNT,
  SUB_COUNT,
  etypesFor,
  isComboBid,
  isHalfStackBid,
} from "../models/catalog";
import { resolveBlockArt, resolveBlockArtByName } from "../models/blockArt";

describe("isComboBid — combo-form discriminator (keeps combos off the half-stack path)", () => {
  it("is true for a combo amp id and false for a half-stack-only id", () => {
    // The blonde '65 Twin Reverb is catalogued combo (+ head); the '66 Flip Top is
    // half_stack-only. The two form sets are disjoint, so the discriminator is clean.
    expect(isComboBid("ACD_TwinReverb65BlondeNoFx")).toBe(true);
    expect(isComboBid("ACD_Ampeg66B15")).toBe(false);
    expect(isHalfStackBid("ACD_Ampeg66B15")).toBe(true);
  });

  it("resolves a suffixed device id the same way the tile art does", () => {
    // A baked-cab combo whose device id carries a CabIR/ConvRvb suffix must still
    // resolve to its combo form (mirrors resolveBlockArt's check-first-then-strip).
    expect(isComboBid("ACD_PrincetonReverb68CabIRConvRvb")).toBe(true);
  });

  it("is false for null / unknown ids", () => {
    expect(isComboBid(null)).toBe(false);
    expect(isComboBid(undefined)).toBe(false);
    expect(isComboBid("ACD_SomeWeirdPedal")).toBe(false);
  });
});

describe("Models catalog ingest", () => {
  it("keeps only available rows", () => {
    expect(TOTAL).toBe(346);
    expect(MODELS).toHaveLength(346);
  });

  it("matches the documented per-category counts", () => {
    expect(CAT_COUNT).toEqual({
      "Combo Amps": 27,
      "Amp Heads": 48,
      "Half Stacks": 18,
      "Bass Amps": 8,
      Cabinets: 63,
      Effects: 169,
      Microphones: 7,
      "FX Loops": 5,
      IR: 1,
    });
  });

  it("matches the documented Effects subcategory counts", () => {
    expect(SUB_COUNT).toEqual({
      Stompboxes: 38,
      Modulation: 28,
      Delay: 32,
      Reverb: 28,
      Dynamics: 12,
      EQ: 11,
      Filter: 6,
      Pitch: 11,
      Synth: 3,
    });
  });

  it("carries a real image tile path (null only for blocks with no tile in the rcc bundle)", () => {
    const nullImage = MODELS.filter((r) => r.image === null);
    for (const r of MODELS) {
      if (r.image !== null) {
        expect(r.image, r.cid).toMatch(/^tmp_blocks\/.+\.png$/);
      }
    }
    expect(nullImage.length).toBeLessThan(10);
  });

  it("keeps amps in BOTH Combo Amps and Amp Heads (no de-dupe)", () => {
    const bid = "ACD_TweedDeluxe";
    const cats = MODELS.filter((r) => r.bid === bid)
      .map((r) => r.cat)
      .sort();
    expect(cats).toEqual(["Amp Heads", "Combo Amps"]);
    expect(new Set(MODELS.map((r) => r.cid)).size).toBe(TOTAL);
  });

  it("half-stacks resolve a paired-cabinet tile for the composite", () => {
    const hs = MODELS.filter((r) => r.form === "half_stack");
    // 24 half_stack-FORM records: the 18 in the "Half Stacks" category + 6
    // head/cab stacks filed under other categories (incl. the '66 Flip Top
    // Ampeg B-15, a head-on-1x15 stack that stays under Bass Amps).
    expect(hs).toHaveLength(24);
    expect(MODELS.filter((r) => r.cat === "Half Stacks")).toHaveLength(18);
    for (const r of hs)
      expect(r.pcabImage, r.cid).toMatch(/^tmp_blocks\/Cabinets\/.+\.png$/);
  });

  it("every Microphone resolves to mic art by name (bid is null — must not fall through to the generic pedal icon)", () => {
    // Mirrors ModelTile's resolution: bid ? resolveBlockArt(bid) : byName(name).
    const mics = MODELS.filter((r) => r.cat === "Microphones");
    expect(mics).toHaveLength(7);
    for (const r of mics) {
      expect(r.bid, r.cid).toBeNull();
      const art = resolveBlockArtByName(r.name);
      expect(art, r.cid).not.toBeNull();
      if (art === null) throw new Error(`${r.cid}: expected mic art by name`);
      // formFor routes any "mic"-prefixed icon to the MicBody renderer.
      expect(art.icon.startsWith("mic"), `${r.cid} → ${art.icon}`).toBe(true);
    }
  });

  it("every available model resolves to real art (none fall through to the generic pedal icon)", () => {
    // Exactly ModelTile's resolution. A null here = a card rendered as the
    // "knobs2" placeholder — the class of bug that left all 7 mics blank.
    const unresolved = MODELS.filter((r) =>
      r.bid ? !resolveBlockArt(r.bid) : !resolveBlockArtByName(r.name),
    ).map((r) => r.cid);
    expect(unresolved).toEqual([]);
  });

  it("non-mic records carry a brand-lineage label; mics fall back to a dash", () => {
    for (const r of MODELS) {
      if (r.bid) expect(r.lineage, r.cid).not.toBe("—");
      else expect(r.lineage).toBe("—");
    }
  });

  it("Stompboxes expose the documented effect types in priority order", () => {
    expect(etypesFor("Stompboxes")).toEqual([
      "Boost",
      "Overdrive",
      "Distortion",
      "Fuzz",
      "Bass Overdrive",
    ]);
  });

  it("flags Reverb convolution models", () => {
    const conv = MODELS.filter((r) => r.sub === "Reverb" && r.conv);
    expect(conv.length).toBeGreaterThan(0);
    expect(MODELS.filter((r) => r.conv).every((r) => r.sub === "Reverb")).toBe(
      true,
    );
  });
});

describe("device-id resolution — NoFx bridge", () => {
  it("resolves a wet amp id with no NoFx token to the catalogued …NoFx base", () => {
    // Device preset / saved-block id for the '65 Deluxe Reverb Blonde Vibrato: amp
    // + cab IR + convolution reverb. It strips to the bare …BlondeVibrato, which the
    // catalog only carries as …BlondeVibratoNoFx — the +NoFx bridge must resolve it
    // (else the UI shows the raw ACD_ id, e.g. "saved ACD_DeluxeReverb65…").
    const wet = resolveBlockArt("ACD_DeluxeReverb65BlondeVibratoCabIRConvRvb");
    const base = resolveBlockArt("ACD_DeluxeReverb65BlondeVibratoNoFx");
    expect(base).not.toBeNull();
    expect(wet).not.toBeNull();
    if (base === null || wet === null)
      throw new Error("expected both catalogued");
    expect(wet.icon).toBe(base.icon);
  });

  it("does not conjure a false match by appending NoFx", () => {
    expect(resolveBlockArt("ACD_TotallyMadeUpBlockXyz")).toBeNull();
  });
});

describe("Model since-firmware version", () => {
  const sinceFor = (bid: string) => {
    const recs = MODELS.filter((r) => r.bid === bid);
    expect(recs.length, bid).toBeGreaterThan(0);
    return [...new Set(recs.map((r) => r.since))];
  };

  it("maps known post-launch families to their introducing firmware", () => {
    expect(sinceFor("ACD_HiwattDR103CanMod")).toEqual(["1.7"]);
    expect(sinceFor("ACD_SLO100")).toEqual(["1.3"]);
    expect(sinceFor("ACD_TMCust59Bassman")).toEqual(["1.4"]);
    expect(sinceFor("ACD_SuperBassman")).toEqual(["1.4"]);
  });

  it("defaults launch-era + mic records to 1.0", () => {
    expect(sinceFor("ACD_TwinReverb65NoFx")).toEqual(["1.0"]);
    const mics = MODELS.filter((r) => r.bid === null);
    expect(mics.length).toBeGreaterThan(0);
    for (const m of mics) expect(m.since, m.cid).toBe("1.0");
  });

  it("gives every record a well-formed dotted version", () => {
    for (const r of MODELS) expect(r.since, r.cid).toMatch(/^\d+\.\d+$/);
  });
});

describe("Firmware 1.8 catalog additions", () => {
  const fw18 = MODELS.filter((r) => r.since === "1.8");

  const INVENTORY_MENU_EXPOSED_NEW = [
    "ACD_Blumes",
    "ACD_Cirrostratus",
    "ACD_CirrostratusLite",
    "ACD_DeluxeReverb68CustomCabIRConvRvb",
    "ACD_Evh412G12H30",
    "ACD_Fen57Champ",
    "ACD_Fen65DlxGB",
    "ACD_Fen68DlxG12V70",
    "ACD_Fen68PrinceG10R30",
    "ACD_FenPrincGB",
    "ACD_FenTwinEmi15",
    "ACD_Hypersonic_112",
    "ACD_HypersonicAmp6L6Blue",
    "ACD_HypersonicAmp6L6Green",
    "ACD_HypersonicAmp6L6Red",
    "ACD_Lightspeed",
    "ACD_PitchSequencer",
    "ACD_Plumes",
    "ACD_PrincetonReverb68CabIRConvRvb",
    "ACD_Rockman",
    "ACD_SpectralDelay",
    "ACD_SpectralReverb",
    "ACD_StepFilter",
    "ACD_StepFilterDelay",
    "ACD_StepTremolo",
    "ACD_TCIntegratedPre",
    "ACD_TCIntegratedPreStatic",
    "ACD_TMChamp57",
    "ACD_TMRumbleV3",
    "ACD_TwinReverbCustom15NoFxCabIR",
    "ACD_UA1176",
  ];

  it("includes the menu-exposed-new 1.8 ids", () => {
    expect(fw18.length).toBeGreaterThan(0);
    const fw18Bids = new Set(fw18.map((r) => r.bid));
    for (const id of INVENTORY_MENU_EXPOSED_NEW) {
      expect(fw18Bids.has(id), id).toBe(true);
    }
  });

  it("stamps every addition since 1.8", () => {
    for (const r of fw18) expect(r.since, r.cid).toBe("1.8");
  });

  it("resolves a icon + tone in the BlockArt engine for every addition", () => {
    for (const r of fw18) {
      if (r.bid === null) throw new Error(`${r.cid}: 1.8 addition has no bid`);
      const art = resolveBlockArt(r.bid);
      expect(art, r.cid).not.toBeNull();
      if (art === null) throw new Error(`${r.cid}: expected resolved art`);
      expect(art.icon, r.cid).toBeTruthy();
      expect(art.tone, r.cid).toBeTruthy();
    }
  });

  it("places every addition in a known category (product_profile authority)", () => {
    const CATS = new Set([
      "Combo Amps",
      "Amp Heads",
      "Bass Amps",
      "Cabinets",
      "Effects",
    ]);
    for (const r of fw18) expect(CATS.has(r.cat), r.cid).toBe(true);
  });

  it("gives every Effects addition a subcategory; amps/cabs carry none", () => {
    for (const r of fw18) {
      if (r.cat === "Effects") expect(r.sub, r.cid).toBeTruthy();
      else expect(r.sub, r.cid).toBeNull();
    }
  });

  it("never flags convolution on a non-Reverb addition", () => {
    for (const r of fw18) if (r.conv) expect(r.sub, r.cid).toBe("Reverb");
  });

  describe("renders through BlockArt (catalog → art → engine)", () => {
    beforeAll(() => {
      (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox =
        () => ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect;
    });

    const PICK: [label: string, bid: string][] = [
      ["combo", "ACD_HypersonicAmp6L6Green"],
      ["cab", "ACD_Evh412G12H30"],
      ["cab 1x15", "ACD_FenTwinEmi15"],
      ["gear pedal (od3)", "ACD_Plumes"],
      ["lab boost", "ACD_TCIntegratedPre"],
      ["grunt boost", "ACD_TCIntegratedPreStatic"],
      ["rockbox", "ACD_Rockman"],
      ["rack compressor", "ACD_UA1176"],
      ["concept motif", "ACD_SpectralReverb"],
    ];

    it.each(PICK)("%s renders an svg", (_label, bid) => {
      const r = MODELS.find((m) => m.bid === bid);
      expect(r, bid).toBeDefined();
      const art = resolveBlockArt(bid);
      if (art === null) throw new Error(`${bid}: expected resolved art`);
      const { container } = render(
        <ThemeProvider>
          <BlockArt
            icon={art.icon}
            tone={art.tone}
            lab={art.short}
            size={56}
            label={false}
          />
        </ThemeProvider>,
      );
      expect(container.querySelector("svg"), bid).not.toBeNull();
    });
  });
});
