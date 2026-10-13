// Guards the explicit block-art `variant` contract (#318) and the per-instance SVG
// ids the art engine mints.
//
// 1. Art used to be chosen by regex/`.includes` on the display caption (`short`).
//    The `ART_VARIANT` table replaced those matches; this pins it, row by row,
//    against the caption rules it replaced (scoped to the form-factor each rule's
//    renderer actually saw), so the refactor provably changed no pick. A new row
//    whose caption happens to contain e.g. "BOG" no longer changes its art — but
//    a variant added or dropped without updating this oracle fails here.
// 2. Every BlockArt / HalfStackArt instance must define its gradient/clip/pattern
//    ids under its own prefix: two tiles on one screen sharing an id makes one
//    tile's `url(#…)` paint the other's defs.

import { describe, it, expect } from "vitest";
import { BlockArt, HalfStackArt, type ArtVariant } from "../ui/BlockArt";
import { formFor } from "../ui/blockart/shared";
import { resolveBlockArt, type BlockArtSpec } from "../models/blockArt";
import { TMP_CATALOG } from "../models/blockArtCatalog";
import { ART_VARIANT } from "../models/blockArtCatalog/variants";
import { renderWithTheme } from "./testKit";

/** Every catalogued id once, resolved the way the Catalog and strips resolve it. */
function allSpecs(): BlockArtSpec[] {
  const seen = new Set<string>();
  const out: BlockArtSpec[] = [];
  for (const cat of TMP_CATALOG)
    for (const [id] of cat.blocks) {
      if (seen.has(id)) continue;
      seen.add(id);
      const art = resolveBlockArt(id);
      if (art) out.push(art);
    }
  return out;
}

/** The caption rules the renderers applied before #318, verbatim, scoped to the
 *  form-factor whose renderer evaluated them. */
function legacyVariant(art: BlockArtSpec): ArtVariant | undefined {
  const lab = art.short;
  switch (formFor(art.icon)) {
    case "amp": {
      if (lab.includes("SLO100")) return "slo100-panel";
      if (/^BJR$/i.test(lab)) return "bluesjr";
      if (/^BJR/i.test(lab)) return "bluesjr-ltd";
      if (/^BBRK/i.test(lab)) return "bassbreaker";
      if (/PETROL|VH4/i.test(lab)) return "diezel-petrol";
      if (/BASSTV|BASSMAN TV/i.test(lab)) return "bassman-tv";
      if (/59B|59 BASSMAN/i.test(lab)) return "bassman-59";
      if (lab.toUpperCase().includes("5150")) {
        const L = lab.toUpperCase();
        if (L.includes("BLUE") || /(^|[ -])B($|[ ])/.test(L)) return "evh-blue";
        if (L.includes("RED") || /(^|[ -])R($|[ ])/.test(L)) return "evh-red";
        return "evh-green";
      }
      return undefined;
    }
    case "cab":
      if (lab.includes("BOG")) return "cab-bogner";
      if (lab.includes("MES V30")) return "cab-recto-trad";
      if (lab.includes("SWR 210")) return "cab-swr-210";
      if (lab.includes("M4 JV30")) return "cab-silver-grille";
      if (lab.includes("5150 112")) return "cab-evh-112";
      if (lab.includes("VK 3")) return "cab-vibroking";
      return undefined;
    case "rack":
      return /SEVENTY ?SIXER|^76\b|1176/i.test(lab) ? "rack-1176" : undefined;
    case "pedal":
      if (/GLOOP/i.test(lab)) return "glooper";
      if (/BMP NYC|MISTRESS|SMSTONE|DMM/i.test(lab)) return "black-well";
      if (art.icon === "bigmuff" && /RUSS/i.test(lab)) return "muff-russian";
      if (art.icon === "bigmuff" && /\bRAM|HORN/i.test(lab)) return "muff-rams";
      if (art.icon === "od3" && /BLUMES|PINIONS/i.test(lab))
        return "gear-green-inset";
      if (art.icon === "od3" && /LSPEED|LIGHTYEAR|LIGHTSPEED/i.test(lab))
        return "gear-blue-inset";
      return undefined;
    default:
      return undefined;
  }
}

describe("block-art variant — explicit, and identical to the old caption picks", () => {
  const specs = allSpecs();

  it("covers the whole catalog", () => {
    expect(specs.length).toBeGreaterThan(300);
  });

  it.each(specs.map((s) => [s.id, s] as const))(
    "%s carries the variant its caption used to select",
    (_id, art) => {
      expect(art.variant).toBe(legacyVariant(art));
    },
  );

  it("every ART_VARIANT key is a catalogued id", () => {
    const ids = new Set(specs.map((s) => s.id));
    expect(Object.keys(ART_VARIANT).filter((id) => !ids.has(id))).toEqual([]);
  });
});

describe("block-art SVG ids — unique per instance", () => {
  it("every [id] is unique and every url(#…) resolves, with ALL art in one document", () => {
    const specs = allSpecs();
    const { container } = renderWithTheme(
      <>
        {specs.map((art) => (
          <BlockArt
            key={art.id}
            icon={art.icon}
            tone={art.tone}
            lab={art.short}
            variant={art.variant}
            footswitch={art.footswitch}
            bodyColor={art.body}
            accentColor={art.accent}
            panelColor={art.panel}
            label={false}
          />
        ))}
        {/* every row twice — the clash a shared literal id (gloopclip) made */}
        {specs.map((art) => (
          <BlockArt
            key={`dup-${art.id}`}
            icon={art.icon}
            tone={art.tone}
            variant={art.variant}
            label={false}
          />
        ))}
        {specs
          .filter((art) => formFor(art.icon) === "amp")
          .map((art) => (
            <HalfStackArt
              key={`hs-${art.id}`}
              topIcon={art.icon}
              topTone={art.tone}
              topVariant={art.variant}
              cabIcon="cab4"
              cabTone={art.tone}
            />
          ))}
      </>,
    );
    const ids = Array.from(container.querySelectorAll("[id]"), (el) => el.id);
    expect(ids.length).toBeGreaterThan(0);
    const dupes = ids.filter((id, i) => ids.indexOf(id) !== i);
    expect([...new Set(dupes)]).toEqual([]);

    const idSet = new Set(ids);
    const refs = Array.from(container.querySelectorAll("*")).flatMap((el) =>
      Array.from(el.attributes).flatMap((a) =>
        Array.from(a.value.matchAll(/url\(#([^)]+)\)/g), (m) => m[1]),
      ),
    );
    expect(refs.length).toBeGreaterThan(0);
    expect(refs.filter((r) => !idSet.has(r))).toEqual([]);
  });
});
