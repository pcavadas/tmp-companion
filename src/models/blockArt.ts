// Block-art catalog ported from the design handoff
// (illustration_engine/catalog-data.jsx): the authoritative per-model
// icon + chassis tone + terse label, keyed by firmware FenderId. This is
// what BlockArt needs to draw the real modeled unit — resolved BY ID (not
// by broad category), so e.g. Filtron gets the envelope-filter icon.

import type { ToneId } from "../ui/BlockArt";
import { BLOCK_BODY, BLOCK_ACCENT } from "../ui/blockart/blockColors.generated";
import { CAB_COVERING } from "./cabCovering.generated";
import { TMP_CATALOG } from "./blockArtCatalog";

// Factory default head → cabinet pairing for each half-stack amp head, matching
// the Tone Master Pro's shipped defaults (each head with its own brand's cab).
const HALF_STACK_DEFAULTS = {
  ACD_Bassbreaker15High: "ACD_FenBassBreaker412V30",
  ACD_Bassbreaker15Med: "ACD_FenBassBreaker412V30",
  ACD_JTM45Head: "ACD_Mar1960tvGB",
  ACD_MarshallPlexi: "ACD_Mar1960tvGB",
  ACD_JCM800TMS: "ACD_Marshall_JCM800_1960A",
  ACD_Jubilee: "ACD_Mar1960aV30",
  ACD_JubileeClip: "ACD_Mar1960aV30",
  ACD_JubileeLead: "ACD_Mar1960aV30",
  ACD_HiwattDR103CanMod: "ACD_Hiwatt412Fane",
  ACD_BE100: "ACD_Fried_BE4x12_V30_Greenback",
  ACD_Evh100SGreen: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_Evh100SBlue: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_Evh100SRed: "ACD_Evh5150iii_4x12_Celestion_G12_EVH",
  ACD_SLO100: "ACD_Sol_4x12_Slant_G12H",
  ACD_OrangeRockerverb50MKIII: "ACD_OrangePPC412",
  ACD_MarkIICClassAB: "ACD_Mbg412HBBSClosed",
  ACD_DualRectifier: "ACD_Mbg_RectifierTrad_CelestionV30",
  ACD_DiezelVh4Ch3: "ACD_Diezel412FV",
  ACD_Uberschall: "ACD_Bogner_412STU_Celestion_G12_V30",
  // '66 Flip Top (Ampeg B-15): a separate head on a closed 1x15 cab — the
  // flip-top, not a Fender combo. Renders as a head-on-cab stack (form
  // half_stack in the Model Guide) over the B-15's own 1x15 cab.
  ACD_Ampeg66B15: "ACD_FlipTop",
};

// Per-head override of the PAIRED CAB's chassis tone for a half-stack, when the
// factory cab's catalogued tone differs from the head's livery. The Silver
// Jubilee heads ship on a SILVER cab, but their shared 4x12 cab row
// (ACD_Mar1960aV30) is catalogued black for its own standalone Cabinets listing
// — so tint just the stacked cab here, leaving the standalone cab unchanged.
export const HALF_STACK_CAB_TONE: Record<string, ToneId> = {
  ACD_Jubilee: "jubilee",
  ACD_JubileeClip: "jubilee",
  ACD_JubileeLead: "jubilee",
};

// ── Flatten → by-id map ──────────────────────────────────────────────────────
export interface BlockArtSpec {
  id: string;
  icon: string;
  tone: string;
  /** terse on-strip caption (uppercase, single line) */
  short: string;
  /** full Fender model name */
  name: string;
  fam: string;
  /** stompbox footswitch style (pedal-form blocks only) — matched per ref:
   *  plate = big black-rubber treadle plate (Boss + the metal gate + Ibanez TS-10);
   *  metal = small metallic rectangle switch (Ibanez TS808);
   *  round = chrome button (everything else, the default). */
  footswitch: "plate" | "metal" | "round";
  /** ref-derived per-block body color (pedals) — overlays the tone default in
   *  BlockArt. Sampled deterministically; see blockColors.generated.ts. */
  body?: string;
  /** Fender reverb-chassis accent (footswitch-band colour) — present iff the block
   *  is one of the 8 cream-chassis reverbs; drives the colored footswitch section. */
  accent?: string;
  /** ref-derived colour of a recessed control panel behind the knobs/sliders — set
   *  only for the few pedals whose ref shows a distinct coloured panel (the MEGA
   *  EQ-5 blue slider bed, the FILTRON blue control band). */
  panel?: string;
}

// Footswitch overrides keyed by id (the brand/ref doesn't follow a clean rule):
// the big black-rubber plate is worn by Boss pedals (detected via blurb) plus the
// metal gate + the Ibanez TS-10; the Ibanez TS808 has a small metal-rectangle switch.
// The Fender bass/parametric EQs are Boss-style enclosures (rubber treadle) whose
// blurb says "Fender original", so they need an explicit override.
const FS_PLATE = new Set([
  "ACD_ChromeGate",
  "ACD_Greenbox10",
  "ACD_NobelsOdr1",
  "ACD_TMBassGraphicEQ7",
  "ACD_TMGraphicEQ7Wide",
  "ACD_MustangPEQ",
]);
const FS_METAL = new Set(["ACD_TubeScreamer"]);
// Recessed coloured control panel behind the knobs/sliders, keyed by id (ref-sampled
// blue). The MEGA EQ-5's body is black with a blue slider bed; the FILTRON is grey
// with a blue control band — so the panel is distinct from the enclosure body.
const KNOB_PANEL: Record<string, string> = {
  ACD_MustangFiveBandEq1: "#3c6c84",
  ACD_MicroTronIV: "#24549c",
};
function footswitchOf(id: string, blurb: string): "plate" | "metal" | "round" {
  if (FS_METAL.has(id)) return "metal";
  if (FS_PLATE.has(id) || /\bBoss\b/.test(blurb)) return "plate";
  return "round";
}

const BY_ID: Record<string, BlockArtSpec | undefined> = {};
// Secondary index by full model name (first-wins), for catalog rows that carry no
// FenderId to resolve by — the 7 Microphones have block_id=null (they're cab
// parameters, not DSP blocks), so they reach their art via this name index. The
// blockArt `name` field equals the catalog `block_name` for these rows.
const BY_NAME: Record<string, BlockArtSpec> = {};
for (const cat of TMP_CATALOG) {
  for (const [id, label, icon, tone, name, blurb] of cat.blocks) {
    const art: BlockArtSpec = {
      id,
      icon,
      tone,
      short: normalizeShort(label),
      name,
      fam: cat.key,
      footswitch: footswitchOf(id, blurb),
      body: BLOCK_BODY[id],
      accent: BLOCK_ACCENT[id],
      panel: KNOB_PANEL[id],
    };
    BY_ID[id] = art;
    if (name && !(name in BY_NAME)) BY_NAME[name] = art;
  }
}

/** Terse label → strip caption: uppercase, dashes→spaces, split digit↔letter runs
 * ("57DLX" → "57 DLX") so captions read cleanly on one line. */
function normalizeShort(label: string): string {
  return label
    .toUpperCase()
    .replace(/-/g, " ")
    .replace(/(\d)([A-Z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
}

// Device FenderIds carry cab/IR/convolution suffixes the catalog id omits
// (e.g. ACD_TweedDeluxeCabIR → ACD_TweedDeluxe). Strip them one at a time,
// checking after each. NoFx is part of real base ids so it is NOT stripped.
const SUFFIX = /(ConvRvb|CabIR|NoCab|Cab|IR)$/;

/** Resolve a model's block art by its full name — the fallback for catalog rows
 *  with no FenderId (the Microphones). Returns null if the name isn't catalogued. */
export function resolveBlockArtByName(name: string): BlockArtSpec | null {
  return BY_NAME[name] ?? null;
}

// Resolve a device FenderId to a catalog id by stripping cab/IR/convolution suffixes one
// at a time via the canonical SUFFIX, CHECKING `inSet` BEFORE each strip — so an id
// already catalogued WITH a suffix (the `…CabIRConvRvb` reverb amps) matches directly and
// is never over-stripped, while a bare-catalogued amp discovered with an extra suffix
// (`ACD_HiwattDR103CanModCabIR` → `ACD_HiwattDR103CanMod`) still matches. The last-gap
// bridge appends `NoFx` once: a device "wet" amp id (…BlondeVibratoCabIRConvRvb) strips to
// a bare id (…BlondeVibrato) the catalog only carries WITH the NoFx token
// (…BlondeVibratoNoFx); NoFx is never stripped, so it must be re-added to match. Returns
// the first form satisfying `inSet`, else the fully-stripped id. The shared core of
// resolveBlockArt + resolveDeviceId (mirrored in the Rust `is_amp_model_id`), so the
// strip+NoFx rule lives in ONE place.
function resolveCatalogId(
  model: string,
  inSet: (id: string) => boolean,
): string {
  let m = model;
  for (let i = 0; i < 6; i++) {
    if (inSet(m)) return m;
    const next = m.replace(SUFFIX, "");
    if (next === m) break;
    m = next;
  }
  if (inSet(m)) return m;
  if (!m.endsWith("NoFx") && inSet(m + "NoFx")) return m + "NoFx";
  return m;
}

/** Resolve a device model id to its block art, or null if uncatalogued. */
export function resolveBlockArt(model: string): BlockArtSpec | null {
  return BY_ID[resolveCatalogId(model, (m) => Boolean(BY_ID[m]))] ?? null;
}

/** Terse model→caption fallback for an uncatalogued block — spaces the camelCase
 * id, never a raw mid-word slice. NON-uppercase: callers that want an uppercase
 * strip caption apply `.toUpperCase()` themselves (the Copy editor / EditGraph
 * naming paths rely on the cased form). */
export function shortFallback(model: string): string {
  return model
    .replace(/^(ACD_|USR_)/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

/** The art-derived fields a signal-chain strip tile needs to render a block
 * through `BlockArt` — the SINGLE source the strip adapters share so they can't
 * drift from each other or from the Catalog (which feeds BlockArt the same set).
 * Returns a plain object (a structural superset of which is a `StripBlock`). */
export interface BlockArtFields {
  icon?: string;
  tone?: string;
  body?: string;
  panel?: string;
  footswitch?: "plate" | "metal" | "round";
  accent?: string;
  /** = `art.short`; the engine's caption + 1.8 dispatch token (undefined when
   *  uncatalogued — matches the Catalog's `lab={art?.short ?? ""}`). */
  lab?: string;
  /** visible strip caption, with an uppercase fallback for uncatalogued blocks. */
  name: string;
  /** the fuller Pro-Control-style model name (e.g. "HIWAY 105", "4X12 BRITISH
   *  V30") — shown on hover; the tile keeps the terse `name`/`lab` caption.
   *  Undefined when uncatalogued. */
  fullName?: string;
}

/** Map a device model id to its strip-tile art fields (resolves art once). */
export function blockArtTile(model: string): BlockArtFields {
  const art = resolveBlockArt(model);
  return {
    icon: art?.icon,
    tone: art?.tone,
    body: art?.body,
    panel: art?.panel,
    footswitch: art?.footswitch,
    accent: art?.accent,
    lab: art?.short,
    name: art?.short ?? shortFallback(model).toUpperCase(),
    fullName: art?.name,
  };
}

/** Which model id to look up strip-tile art for: a CabSim block names its tile
 *  from its actual cabinet (`ACD_<cabSimId>`) instead of the generic CAB IR;
 *  everything else (and a CabSim with no cab id) uses its own `model`. Shared by
 *  the hero strip, the Copy strip, and the dual-cab split so the resolution rule
 *  lives in one place. */
export function cabArtModel(
  cabSimId: string | undefined,
  model: string,
): string {
  return cabSimId != null && cabSimId !== "" ? `ACD_${cabSimId}` : model;
}

/** Head-over-cab art for an amp that carries its own cab (a combo / half-stack).
 *  `topIcon/topTone/topLab` = the amp head; `cabIcon/cabTone` = the cabinet. */
export interface HalfStackSpec {
  topIcon?: string;
  topTone?: string;
  topLab: string;
  cabIcon: string;
  cabTone?: string;
}

/** Build the head-over-cab spec from an amp head's ALREADY-RESOLVED art + the device's
 *  `cabsimid` (its built-in cab — a combo/half-stack like preset 003's HIWAY on a
 *  British 4×12). Takes the resolved `head` (not a model id) so the caller resolves
 *  the head art once. The cab art comes from the DEVICE's actual `cabsimid`, so the
 *  strip mirrors the unit. Returns `undefined` for a bare head (no cab id). */
export function ampCabHalfStack(
  head: BlockArtFields,
  cabSimId: string | undefined,
): HalfStackSpec | undefined {
  if (cabSimId == null || cabSimId === "") return undefined;
  const cab = blockArtTile(`ACD_${cabSimId}`);
  return {
    topIcon: head.icon,
    topTone: head.tone,
    topLab: head.lab ?? "",
    cabIcon: cab.icon ?? "cab4",
    cabTone: cab.tone,
  };
}

/** The art fields for a graph/edit node's strip tile, branching on node kind:
 *  the standalone CabSim block is NAMED from its cabinet; an amp carrying its own
 *  cab becomes a head-over-cab half-stack; everything else is its plain block art. */
export function nodeTileArt(
  model: string,
  cabSimId: string | undefined,
  // REQUIRED (no default) on purpose: a combo amp carries a cab_sim_id like a
  // half-stack head does, so every device-node caller must declare combo-ness
  // (`isComboBid(model)`) — a forgotten flag would silently re-stack combos, the
  // exact bug this fixes. Required → that omission is a compile error, not a regression.
  isCombo: boolean,
): BlockArtFields & { halfStack?: HalfStackSpec } {
  if (model === "ACD_CabSimTMS") {
    return blockArtTile(cabArtModel(cabSimId, model));
  }
  const tile = blockArtTile(model);
  // Cab-driven covering (tolex): the unit derives an amp's displayed covering from
  // amp_id + cabsimid (firmware DUBS_extender.json), so a blackface '65 Twin head on
  // the cream Creamback cab renders BLONDE. Mirror that — override the amp tile's tone
  // when the attached cab maps to a different covering (CAB_COVERING is a whitelist of
  // covering-changing amps; absent → keep the catalog tone). cabsimid arrives without
  // the ACD_ prefix, as the device sends it.
  const covering = CAB_COVERING[model]?.[(cabSimId ?? "").replace(/^ACD_/, "")];
  const base: BlockArtFields = covering ? { ...tile, tone: covering } : tile;
  // A combo's built-in speaker IS its cabsim — render the single combo tile, never a
  // synthesized head-over-cab stack. Heads carry no cab_sim_id so they never reach the
  // stack branch below; only combo-vs-half_stack needs disambiguating, and the caller
  // resolves it (blockArt must not import catalog → no form lookup here).
  if (isCombo) return base;
  const halfStack = ampCabHalfStack(base, cabSimId);
  return halfStack ? { ...base, halfStack } : base;
}

/** Resolve a device FenderId to its catalog id (see {@link resolveCatalogId}). */
export function resolveDeviceId(
  model: string,
  inSet: (id: string) => boolean,
): string {
  return resolveCatalogId(model, inSet);
}

export const HALF_STACK_PAIR: Record<string, string> = HALF_STACK_DEFAULTS;
