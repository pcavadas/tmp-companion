// src/models/catalog.ts — the Models page data layer.
//
// Ingests the unified block catalog (tmp-model-guide.json) into the flat record
// list + taxonomy that drives the Models tab. The catalog carries all fields
// including `since` (introducing firmware version). We keep only `available` rows
// and map each to the documented record shape. Amps intentionally appear twice
// (combo + head) — different `cid`/`cat`, never de-duped.

import catalog from "./tmp-model-guide.json";
import { DASH } from "../lib/format";
import { LINEAGE_BY_BID } from "./lineage";
import { cpuForBid } from "./cpu";
// catalog→blockArt is the SAFE import direction (blockArt is a leaf importing nothing
// back). The FORBIDDEN cycle is blockArt→catalog→cpu→blockArt — we never add that.
import { resolveDeviceId } from "./blockArt";

export type Channels = "mono" | "stereo";
export type Form =
  | "combo"
  | "head"
  | "half_stack"
  | "product"
  | "loop"
  | "ir"
  | "ext_speaker";

/** A single raw row from the Model Guide export (`blocks[]`). */
interface RawBlock {
  block_id: string | null;
  block_name: string;
  available: boolean;
  catalog_id: string;
  real_unit: string;
  category: string;
  subcategory: string | null;
  effect_type: string | null;
  form: string;
  paired_cabinet: string | null;
  image: string | null;
  channels: string;
  convolution?: boolean;
  since?: string;
}

/** A mapped, page-ready model record (documented shape; see the handoff README). */
export interface ModelRecord {
  cid: string; // catalog_id — unique row id, React key
  bid: string | null; // block_id (ACD_*); null for the 7 mics
  name: string; // exact Fender public model name (UPPERCASE as printed)
  real: string; // real-world unit it emulates
  cat: string; // top-level category
  sub: string | null; // Effects only: subcategory
  et: string | null; // Effects only: fine effect type
  form: Form;
  ch: Channels;
  conv: boolean; // Reverb only: convolution engine
  cpu: number | null; // REAL DSP cost (% of preset budget); null for mics / FX-loop markers
  since: string; // introducing firmware version ("1.0" launch default; from catalog JSON)
  image: string | null; // web path to the real artwork tile (public/); null for 1.8 (no tiles yet)
  /** half_stack only: the paired cabinet's tile path, for the stacked composite. */
  pcabImage: string | null;
  lineage: string; // brand-lineage label (carried over; DASH for mics)
  /** precomputed lowercased search haystack (name·real·et·sub·cat·lineage). */
  search: string;
}

// ── Taxonomy (ordering + grouping rules; ported from catalog-tax.jsx) ─────────

/** Top-level category order — render the rail / "all" grouping in exactly this order. */
export const CAT_ORDER = [
  "Combo Amps",
  "Amp Heads",
  "Half Stacks",
  "Bass Amps",
  "Cabinets",
  "Effects",
  "Microphones",
  "FX Loops",
  "IR",
] as const;

/** Effects subcategory order (the only category with subcategories). */
export const SUB_ORDER = [
  "Stompboxes",
  "Modulation",
  "Delay",
  "Reverb",
  "Dynamics",
  "EQ",
  "Filter",
  "Pitch",
  "Synth",
] as const;

/** effect_type display priority within a subcategory; unlisted sorts after, A–Z. */
const ET_PRIORITY = [
  // stompboxes
  "Boost",
  "Overdrive",
  "Distortion",
  "Fuzz",
  "Bass Overdrive",
  // modulation
  "Chorus",
  "Flanger",
  "Phaser",
  "Tremolo",
  "Rotary",
  "Uni-Vibe",
  "Vibrato",
  "Panner",
  // delay
  "Tape",
  "Analog",
  "Digital",
  "Multi-Tap",
  "Ping-Pong",
  "Reverse",
  "Modulated",
  "Ambient",
  "Freeze",
  "Looper",
  "Doubler",
  "Feedback",
  // reverb
  "Spring",
  "Room",
  "Hall",
  "Plate",
  "Chamber",
  "Shimmer",
  // dynamics
  "Compressor",
  "Noise Gate",
  "Volume",
  "Volume Swell",
  "Swell",
  "Slow Attack",
  // eq
  "Graphic EQ",
  "Parametric EQ",
  "Hi-Lo Cut",
  "High Cut",
  "Low Cut",
  "Notch",
  // pitch
  "Octave",
  "Pitch Shifter",
  "Detune",
  "Harmonizer",
  "Whammy",
  "Capo",
  "Arpeggiator",
  "Pitch",
  // filter / synth
  "Wah",
  "Envelope Filter",
  "Filter",
  "Polysynth",
  "Synth",
  "Wave-Morph",
];
const ET_RANK = new Map(ET_PRIORITY.map((e, i) => [e, i]));

/** Sort comparator for effect types: priority order, then alphabetical. */
export function etSort(a: string, b: string): number {
  const ra = ET_RANK.get(a) ?? 999;
  const rb = ET_RANK.get(b) ?? 999;
  return ra !== rb ? ra - rb : a.localeCompare(b);
}

/** Cabinet size token parsed from the start of a name (e.g. "4X12 …" → "4X12"). */
export function cabSize(name: string): string {
  const m = /^(\d+X\d+)/i.exec(name);
  return m ? m[1].toUpperCase() : "Other";
}

/** Cabinet size-group display order. */
export const CAB_SIZE_ORDER = [
  "1X10",
  "1X12",
  "2X10",
  "2X12",
  "3X10",
  "4X10",
  "4X12",
  "6X10",
  "8X10",
  "1X15",
];

// ── Ingest ────────────────────────────────────────────────────────────────

const CABINET_TILE_DIR = "tmp_blocks/Cabinets/";

function toRecord(r: RawBlock): ModelRecord {
  const lineage =
    (r.block_id != null ? LINEAGE_BY_BID[r.block_id] : null) ?? DASH;
  return {
    cid: r.catalog_id,
    bid: r.block_id,
    name: r.block_name,
    real: r.real_unit,
    cat: r.category,
    sub: r.subcategory,
    et: r.effect_type,
    form: r.form as Form,
    ch: r.channels === "stereo" ? "stereo" : "mono",
    conv: r.convolution === true,
    cpu: cpuForBid(r.block_id),
    since: r.since ?? "1.0",
    image: r.image,
    // Half-stacks render the head tile (r.image) stacked over the paired
    // cabinet's own tile, mirroring the device. The extracted cab tile is named
    // by its block_id under Cabinets/.
    pcabImage:
      r.form === "half_stack" && r.paired_cabinet
        ? CABINET_TILE_DIR + r.paired_cabinet + ".png"
        : null,
    lineage,
    search:
      `${r.block_name} ${r.real_unit} ${r.effect_type ?? ""} ${r.subcategory ?? ""} ${r.category} ${lineage}`.toLowerCase(),
  };
}

/** All available model records from the unified catalog, in source order. */
export const MODELS: ModelRecord[] = (catalog.blocks as RawBlock[])
  .filter((r) => r.available)
  .map(toRecord);

/** Total available count — the toolbar's "/ N" denominator. */
export const TOTAL = MODELS.length;

// Which form-factors each block_id appears in. An amp catalogued as BOTH a combo
// and a head shares ONE block_id + identical `name`, so its two rows need a form
// marker to read differently (see `displayName`).
const FORMS_BY_BID = new Map<string, Set<Form>>();
for (const r of MODELS) {
  if (!r.bid) continue;
  const s = FORMS_BY_BID.get(r.bid) ?? new Set<Form>();
  s.add(r.form);
  FORMS_BY_BID.set(r.bid, s);
}

/** Whether a block_id is a half-stack form on the device — a SINGLE block the unit
 *  represents as a head-on-cab stack (the flip-top / stack amps, e.g. the British
 *  Plexi), as opposed to a head amp + a separate cab block. The signal-path strip
 *  uses this to stay faithful to the unit: it stacks a half_stack block but never
 *  merges a head + a separate cab. */
export function isHalfStackBid(bid: string | null | undefined): boolean {
  return bid != null && (FORMS_BY_BID.get(bid)?.has("half_stack") ?? false);
}

/** Whether a device model id resolves to a COMBO form (a built-in speaker). A combo
 *  node carries a `cab_sim_id` (its modeled speaker) just like a half-stack head with
 *  a baked cab, so the signal-strip uses this to keep a combo as a single combo tile
 *  instead of a synthesized head-over-cab stack. Resolved through the SAME
 *  suffix-stripping the tile art uses (`resolveDeviceId`) so the form-test and the art
 *  never disagree on the id. `combo` and `half_stack` are disjoint per id, so a true
 *  half-stack never matches. */
export function isComboBid(bid: string | null | undefined): boolean {
  if (bid == null) return false;
  const id = resolveDeviceId(bid, (m) => FORMS_BY_BID.has(m));
  return FORMS_BY_BID.get(id)?.has("combo") ?? false;
}

/** The user-facing title for a model row. Amps catalogued in BOTH a combo and a
 *  head form share an identical `name`, so append "(Combo)" / "(Head)" to tell the
 *  two otherwise-identical rows apart. Head-only / combo-only amps keep the plain
 *  name. */
export function displayName(r: ModelRecord): string {
  if (r.bid && (r.form === "combo" || r.form === "head")) {
    const forms = FORMS_BY_BID.get(r.bid);
    if (forms?.has("combo") && forms.has("head")) {
      return `${r.name} (${r.form === "combo" ? "Combo" : "Head"})`;
    }
  }
  return r.name;
}

// ── Derived counts + lookups ────────────────────────────────────────────────

/** Records per top-level category. */
export const CAT_COUNT: Record<string, number> = {};
for (const r of MODELS) CAT_COUNT[r.cat] = (CAT_COUNT[r.cat] ?? 0) + 1;

/** Records per Effects subcategory. */
export const SUB_COUNT: Record<string, number> = {};
/** Effect types present per Effects subcategory, in canonical display order. */
const ETYPES_BY_SUB = new Map<string, string[]>();
{
  const sets = new Map<string, Set<string>>();
  for (const r of MODELS) {
    if (r.cat !== "Effects" || !r.sub) continue;
    SUB_COUNT[r.sub] = (SUB_COUNT[r.sub] ?? 0) + 1;
    if (r.et) {
      let set = sets.get(r.sub);
      if (!set) {
        set = new Set();
        sets.set(r.sub, set);
      }
      set.add(r.et);
    }
  }
  for (const [sub, set] of sets) ETYPES_BY_SUB.set(sub, [...set].sort(etSort));
}

/** The effect types present in a subcategory, in canonical display order. */
export function etypesFor(sub: string): string[] {
  return ETYPES_BY_SUB.get(sub) ?? [];
}
