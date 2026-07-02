// Ported VERBATIM from the design handoff illustration engine
// (design_handoff_signal_chain_v2/illustration_engine/pedals.jsx) — the brand-
// accurate BlockArt art shared by the Models tab + the signal-chain strip.
// Adapted only for ESM (window→exports); the art is unchanged. JSX uses the
// automatic runtime (tsconfig "jsx":"react-jsx"), so no React import is needed.
import { TONE_BODY, CLOTH_BASE } from "./blockColors.generated";

// ---- Shared art vocabulary types -------------------------------------------
// A chassis tone (one PEDAL_TONES entry): body/knob/text are required; the rest
// are per-lineage accents some amps/cabs carry.
export interface PedalTone {
  body: string;
  knob: string;
  text: string;
  jewel?: string;
  panel?: string;
  trim?: string;
}
// A grille-cloth recipe (one CLOTH entry): a base colour + a tiling stroke weave.
export interface Cloth {
  base: string;
  ring: string;
  weave: string;
  line: string;
  lineW: number;
  op: number;
  border?: string;
  spark?: string;
  sparkLt?: string;
  sparkDk?: string;
}
// The form-factor a icon resolves to (drives which body renderer is used).
export type FormId =
  | "amp"
  | "ir"
  | "extcab"
  | "cab"
  | "mic"
  | "treadle"
  | "round"
  | "rack"
  | "desk"
  | "screen"
  | "rockbox"
  | "pedal";
// A combo's front livery — logo plaque + grille piping (brand combos) OR a
// top-mounted control panel (Fender-family). Fields are per-variant, so optional.
export interface ComboLivery {
  lx?: number;
  lw?: number;
  logo?: string;
  pipe?: string;
  pipeH?: number;
  script?: boolean;
  panel?: boolean;
  panelFill?: string;
  pknob?: string;
  noJewel?: boolean;
}

// ============================================================================
// TMP Companion DS — Pedal / Amp / Cab iconography  (extended catalog engine)
// ----------------------------------------------------------------------------
// <BlockArt … /> renders a flat, geometry-only block icon for ANY of the
// blocks the Tone Master Pro ships (firmware v1.7.75 — see catalog.jsx).
//
//   • TONES are *physical* chassis colors keyed by modeled-brand lineage
//     (Fender tweed/blackface, Marshall, Vox, Mesa, EHX-muff, Boss-blue …).
//     They do NOT follow light/dark — a block reads the same on any surface.
//   • ICONS are sub-variants: amp head / combo / half-stack; 1×12…8×10 cabs;
//     OD / fuzz / boost / dist; chorus / phaser / flanger / tremolo / rotary /
//     uni-vibe; tape & digital delay; spring / plate / hall / shimmer reverb;
//     graphic & parametric EQ; wah / envelope; octave / whammy; synth; fx-loop.
//
// Resolve order:  explicit props  →  legacy BLOCK_MAP[code]  →  CATALOG_BY_ID.
// Legacy short codes (COMP, AMP-T, CAB-G, DLY-D …) still resolve unchanged so
// data.jsx / the gallery / the table keep working.
// ============================================================================

// ---- Chassis tone palette — fixed, theme-independent -----------------------
export const PEDAL_TONES: Record<ToneId, PedalTone> = {
  // — legacy generic tones (kept for back-compat) —
  ink: { body: "#1f1d1a", knob: "#e9e3d2", text: "#f5f1e8" },
  cream: { body: "#efe9d9", knob: "#2b2925", text: "#1f1d1a" },
  mint: { body: "#3fb883", knob: "#19241d", text: "#10241a" }, // Fender Digital Delay (stereo) — spring/mint green
  rust: { body: "#a7461f", knob: "#f3efe2", text: "#f5f1e8" },
  ochre: { body: "#c89a3a", knob: "#1f1d1a", text: "#1f1d1a" },
  slate: { body: "#3a4250", knob: "#e9e3d2", text: "#f5f1e8" },
  plum: { body: "#5a3a55", knob: "#e9e3d2", text: "#f5f1e8" },
  lake: { body: "#2d5d7a", knob: "#e9e3d2", text: "#f5f1e8" },

  // — amp / cab brand lineages —
  tweed: { body: "#cda23f", knob: "#dcc9a4", text: "#1c160e" }, // lacquered mustard twill; controls on rear chassis
  blackface: {
    body: "#16171b",
    knob: "#cfd3d8",
    text: "#eef1f5",
    jewel: "#cf3a2e",
  },
  brownface: { body: "#6a4426", knob: "#ecdcc0", text: "#f3e9d6" },
  blonde: {
    body: "#ded2b6",
    knob: "#cfd3d8",
    text: "#1f1810",
    panel: "#17181c",
  }, // blonde tolex, black blackface panel, dark oxblood grille
  marshall: {
    body: "#141009",
    knob: "#d8b24a",
    text: "#e7c45e",
    panel: "#c69a35",
  },
  // JTM45 'Bluesbreaker' family (the trem combo + its matching 2x12 cabs): black
  // levant tolex, a grey gold-piped grille, gold script — but NO gold bottom
  // faceplate (deliberately NOT in the marshall bottom-face set, so the head reads
  // tolex-top / grille-bottom with a dark fascia, matching the ref).
  bluesbreaker: {
    body: "#262626",
    knob: "#d8b24a",
    text: "#e7c45e",
    panel: "#1c1c1c",
  },
  vox: { body: "#1b1712", knob: "#e7d9b8", text: "#e7c45e", panel: "#9a7838" }, // black tolex, brass panel, fawn diamond grille
  mesa: { body: "#19150f", knob: "#b59a6a", text: "#e7dcc4" },
  orange: { body: "#dd7016", knob: "#1a1208", text: "#1a1208" },
  hiwatt: {
    body: "#131313",
    knob: "#e8e8e8",
    text: "#f0f0f0",
    jewel: "#d8d8d8",
  },
  boutique: {
    body: "#232027",
    knob: "#b9bcc2",
    text: "#eceef2",
    jewel: "#6ea0d8",
  },
  friedman: {
    body: "#1b1a1e",
    knob: "#2a2620",
    text: "#1f1810",
    panel: "#c9a23e",
  }, // Friedman BE-100 — black head, gold/cream control faceplate
  ampeg: {
    body: "#18181a",
    knob: "#3f78c0",
    text: "#dbe6f5",
    jewel: "#3f78c0",
  }, // Ampeg SVT-810 — neutral black tolex, grey salt-pepper grille
  wine: { body: "#39231f", knob: "#d8cdb4", text: "#f0ead6" }, // Fender TM-Cream / Vibro-King cabs — dark oxblood/wine tolex + oxblood grille
  marshallvint: { body: "#161616", knob: "#cdcabf", text: "#eceae4" }, // Marshall Alnico/Bluesbreaker cab — black tolex, tan grille + cream frame
  roland: {
    body: "#20242b",
    knob: "#8fa4b8",
    text: "#e6edf4",
    jewel: "#6f9ec8",
  },
  bass: { body: "#2a2e37", knob: "#d8cdb4", text: "#f0ead6" },
  jubilee: {
    body: "#c2c5c9",
    knob: "#d8b24a",
    text: "#1a1a1a",
    panel: "#c0c4c8",
  }, // Marshall Silver Jubilee — silver/grey tolex, SILVER control panel, gold knobs, black grille
  acoustasonic: {
    body: "#b9824a",
    knob: "#3f2c20",
    text: "#f2ead6",
    panel: "#3a201a",
  }, // Fender Acoustasonic — copper tolex, dark-brown panels
  gk: { body: "#1a1a1c", knob: "#c4c8cc", text: "#eef0f2", panel: "#b6babe" }, // Gallien-Krueger — black steel, silver control strip
  recto: {
    body: "#141414",
    knob: "#cdd0d2",
    text: "#eceae4",
    panel: "#cdd0d2",
  }, // Mesa Dual Rectifier — diamond-plate steel front
  svt: {
    body: "#161616",
    knob: "#cfd3d7",
    text: "#e8eef5",
    panel: "#dfe3e6",
    jewel: "#3f78c0",
  }, // Ampeg SVT — silver blue-line panel
  b15: { body: "#161616", knob: "#cfd3d7", text: "#e8eef5", panel: "#c4c8cc" }, // Ampeg B-15 — exposed chassis, silver control plate
  fliptop: { body: "#45483f", knob: "#cfd3d7", text: "#e8eef5" }, // Ampeg B-15 1x15 cab — dark olive-grey tolex, wide frame, green-grey grille
  fenderbass: { body: "#1a1a1c", knob: "#cfd3d7", text: "#e8eef5" }, // Fender Bassman Pro Neo 610/810 — black tolex, silver/grey grille
  swr: {
    body: "#18181a",
    knob: "#cdd2d8",
    text: "#eaf0f8",
    panel: "#b23a3a",
    jewel: "#5b8fd6",
  }, // SWR Redhead — black tolex, signature RED control panel, chrome knobs

  // — 1.8 amp / cab lineages (per the 1.8 Models-tab illustration handoff) —
  silverface: {
    body: "#17181c",
    knob: "#1c1d20",
    text: "#eef1f5",
    panel: "#a7abaf",
    jewel: "#d24a3a",
    trim: "#a7abaf",
  }, // '68 Custom — black tolex, brushed-alu panel+trim bezel, silver-turquoise sparkle grille
  evhmodern: {
    body: "#dad8d1",
    knob: "#d2d4d8",
    text: "#1a1a1c",
    jewel: "#3f7d4e",
  }, // EVH 5150 III 50W (ivory) — silver tolex, black control strip + black grille (jewel = channel accent)
  evhblack: {
    body: "#1a1a1c",
    knob: "#d2d4d8",
    text: "#eef1f5",
    jewel: "#3f7d4e",
  }, // EVH 5150 cab — BLACK cabinet, diagonal-stripe black grille + centered EVH mark

  // — iconic pedal-color lineages —
  black: { body: "#1b1b1b", knob: "#d9d5cb", text: "#ededed" },
  chrome: { body: "#c7cbcf", knob: "#2a2d31", text: "#1b1e22" }, // hammertone grey (Rangemaster, clean boost)
  graphite: { body: "#34373b", knob: "#cfd2d6", text: "#edeff1" }, // dark grey (SD Palladium, rack)
  purple: { body: "#5b4b9a", knob: "#ece9f5", text: "#f3f1fa" }, // King of Tone
  olive: { body: "#5c5f31", knob: "#ecead2", text: "#f3f1e0" }, // military green (Green Russian Muff)
  ramshead: { body: "#4a4250", knob: "#e6e1ea", text: "#f1eef4" }, // dark violet-grey (Ram's Head Muff)
  green: { body: "#3f6f37", knob: "#f0ecd8", text: "#f4f1e6" },
  gold: { body: "#c39a3e", knob: "#2a2113", text: "#1f1810" },
  rat: { body: "#1c1c1c", knob: "#565656", text: "#ececec" }, // ProCo RAT — black box, black knobs
  muff: { body: "#d3ccba", knob: "#2a2620", text: "#1f1c16" },
  amber: { body: "#c5731c", knob: "#1c1408", text: "#1c1408" },
  yellow: { body: "#e3b62a", knob: "#2a2410", text: "#221d0c" },
  blue: { body: "#2f6c98", knob: "#e7eef3", text: "#f1f5f8" },
  fuzzface: { body: "#c62f28", knob: "#15110f", text: "#f6eef0" },
  siliconfuzz: { body: "#1f74c0", knob: "#15110f", text: "#eef5fb" },
  red: { body: "#b23a3a", knob: "#f0e2e2", text: "#f6eeee" },
  teal: { body: "#2f6b66", knob: "#e3efec", text: "#eef5f3" },

  // — extended per-unit physical pedal colours (spec-accurate) —
  navy: { body: "#27396e", knob: "#e3b94a", text: "#eef1fa", jewel: "#e3b94a" }, // Korg SDD-3000 — navy body, yellow knobs
  cyan: { body: "#4ea7c1", knob: "#eef8fb", text: "#f3fafc" }, // Boss CE-2 analog chorus
  pink: { body: "#c65a93", knob: "#fdeef6", text: "#fdf3f9" }, // digital delay / polyhedron / doubler
  wood: { body: "#6b4a2c", knob: "#e6d3b6", text: "#f1e6d2" }, // Leslie 122 rotary
  woodlt: { body: "#8a6336", knob: "#ecdcbf", text: "#f3e8d4" }, // Leslie 147 rotary
  vinyl: { body: "#574736", knob: "#d9c9ad", text: "#ece0cc" }, // brownface harmonic tremolo
  ice: { body: "#a8cedd", knob: "#26343c", text: "#1d2930", jewel: "#3f7d94" }, // EHX Freeze (icy)
  frost: { body: "#5f8a99", knob: "#e8f2f5", text: "#f1f8fa" }, // EHX Deep Freeze
  lavender: { body: "#b1a2d2", knob: "#2b2440", text: "#231d36" }, // ethereal / cathedral convolution
  slatebl: { body: "#46587a", knob: "#e6ecf5", text: "#f1f5fb" }, // convolution room / atmosphere
};

// A chassis tone looked up by an arbitrary id — falls back to slate for an
// unknown/empty id (the lookup is genuinely possibly-absent for a free-form key).
// Amp/cab tolex is overlaid from the ref-derived per-lineage TONE_BODY (see
// blockColors.generated.ts); everything else (knob/text/jewel/panel) stays as the
// hand-authored tone.
export function toneOf(tone: string | undefined): PedalTone {
  const base =
    (PEDAL_TONES as Partial<Record<string, PedalTone>>)[tone ?? ""] ??
    PEDAL_TONES.slate;
  const body = tone ? TONE_BODY[tone] : undefined;
  return body ? { ...base, body } : base;
}

// Chassis body (#hex) for a tone id — for callers that need the raw chassis
// colour rather than a rendered icon (e.g. the Models inspector tints its icon
// box with the selected model's chassis colour). Falls back to slate.
export function toneBodyHex(tone?: string): string {
  return toneOf(tone).body;
}

// ---- icon → chassis form --------------------------------------------------
// Most blocks are upright stompboxes ("pedal"). The Tone Master Pro models a
// few real-world form-factors that read differently, so each gets its own
// chassis: amp heads/combos/stacks, speaker cabs, treadle pedals (wah / whammy
// / volume), the round Fuzz-Face, wide Klon-style drives, 19″ rack preamps,
// desktop synth modules, and the on-screen plug-in panels (cut / notch / 5-band
// parametric EQ that render as a software GUI rather than hardware).
export const AMP_ICONS = new Set(["amp", "combo", "stack"]);
export const CAB_ICONS = new Set([
  "cab",
  "cab1",
  "cab2",
  "cab3",
  "cab4",
  "cab6",
  "cab8",
  "cab15",
]);
export const MIC_ICONS = new Set(["mic", "micstick", "micpencil"]);

// Public id unions — the icon/tone vocabularies this engine renders. Exported so
// the by-id block-art catalog (models/blockArt.ts) is validated against them at
// compile time (a typo in a 300-row table becomes a tsc error, not a silent
// fall-through to the default motif / slate tone). IconId = the form-factor sets
// above + every PedalBody motif; ToneId = the PEDAL_TONES keys.
export type IconId =
  | "amp"
  | "combo"
  | "stack"
  | "cab"
  | "cab1"
  | "cab2"
  | "cab3"
  | "cab4"
  | "cab6"
  | "cab8"
  | "cab15"
  // user impulse-response — a neutral cab + bold "IR" stamp (IRBody), so a user
  // IR reads as distinct from the stock speaker cabinets it sits beside.
  | "ir"
  // external cab / 4-cable-method routing — same neutral cab, "EXT CAB" stamp.
  | "extcab"
  // any "mic"-prefixed id renders the mic form (formFor matches the prefix; the
  // suffix is just the mic model, e.g. mic_sm57 / mic_c414 — cosmetic to the art).
  | `mic${string}`
  | "wah"
  | "whammy"
  | "treadle"
  | "roundfuzz"
  | "rack"
  | "racktube"
  | "synth"
  | "screen"
  // 1.8: Rockbox 100 (RBX) — a stompbox (blue slider panel + footswitch)
  | "rockbox"
  | "knob1"
  | "boost"
  | "knobs2"
  | "knobs3"
  | "od"
  | "knobs4"
  | "knobs6"
  | "dist"
  | "fuzz"
  | "bigmuff"
  | "comp"
  | "gate"
  | "vol"
  | "eq"
  | "eq5"
  | "eq7"
  | "eq10"
  | "peq"
  | "envf"
  | "chorus"
  | "phaser"
  | "flanger"
  | "tremolo"
  | "rotary"
  | "univibe"
  | "delay"
  | "spring"
  | "plate"
  | "hall"
  | "shimmer"
  | "octave"
  | "octslider"
  | "fxloop"
  // 1.8 pedal motifs: gear-inspired (Pinions/Runes/Lightyear share `od3`;
  // Integrator/Grunt boosts) + the 8 Fender-designed concept marks.
  | "od3"
  | "labboost"
  | "gruntboost"
  | "steptrem"
  | "stepfilter"
  | "stepfilterdelay"
  | "pitchseq"
  | "prismdelay"
  | "spectralverb"
  | "cirrusverb"
  | "cirrussynthverb";
export type ToneId =
  | "ink"
  | "cream"
  | "mint"
  | "rust"
  | "ochre"
  | "slate"
  | "plum"
  | "lake"
  | "tweed"
  | "blackface"
  | "brownface"
  | "blonde"
  | "wine"
  | "marshall"
  | "bluesbreaker"
  | "marshallvint"
  | "vox"
  | "mesa"
  | "orange"
  | "hiwatt"
  | "boutique"
  | "friedman"
  | "ampeg"
  | "roland"
  | "bass"
  | "jubilee"
  | "acoustasonic"
  | "gk"
  | "recto"
  | "svt"
  | "b15"
  | "fliptop"
  | "fenderbass"
  | "swr"
  | "silverface"
  | "evhmodern"
  | "evhblack"
  | "black"
  | "chrome"
  | "graphite"
  | "purple"
  | "olive"
  | "ramshead"
  | "green"
  | "gold"
  | "rat"
  | "muff"
  | "amber"
  | "yellow"
  | "blue"
  | "fuzzface"
  | "siliconfuzz"
  | "red"
  | "teal"
  | "navy"
  | "cyan"
  | "pink"
  | "wood"
  | "woodlt"
  | "vinyl"
  | "ice"
  | "frost"
  | "lavender"
  | "slatebl";

export function formFor(g: string): FormId {
  if (AMP_ICONS.has(g)) return "amp";
  if (g === "ir") return "ir"; // user IR — neutral cab + "IR" stamp (IRBody)
  if (g === "extcab") return "extcab"; // external cab — neutral cab + "EXT CAB" stamp
  if (CAB_ICONS.has(g)) return "cab";
  if (g.startsWith("mic")) return "mic";
  if (g === "wah" || g === "whammy" || g === "treadle") return "treadle";
  if (g === "roundfuzz") return "round";
  if (g === "rack" || g === "racktube") return "rack";
  if (g === "synth") return "desk";
  if (g === "screen") return "screen";
  if (g === "rockbox") return "rockbox"; // 1.8 Rockbox 100 (RBX) — stompbox form
  return "pedal";
}
// speaker grid per cab icon: [cols, rows, speakerRadius]. CabBody derives the
// cabinet SILHOUETTE from cols:rows (e.g. 8×10 = 2×4 portrait fridge, 2×12 = wide),
// so the orientation here is load-bearing, not just the count.
export const CAB_GRID: Record<string, [number, number, number]> = {
  cab: [1, 1, 13],
  cab1: [1, 1, 13],
  cab15: [1, 1, 15],
  cab2: [2, 1, 9],
  cab3: [3, 1, 6.6],
  cab4: [2, 2, 8],
  cab6: [2, 3, 6.4], // 6×10 bass — 2 wide × 3 tall (portrait)
  cab8: [2, 4, 5], // 8×10 SVT fridge — 2 wide × 4 tall (portrait)
};

// ============================================================================
// Grille cloth — the brand-accurate speaker-cloth weave behind every amp,
// combo and cab. Keyed to the modeled-brand lineage (same tone the chassis
// uses), so a Vox combo gets its diamond grille, a tweed Deluxe its lacquered
// twill, a Marshall stack its salt-and-pepper basket, etc. Drawn flat as a
// base colour + a tiling stroke weave — keeps the sketch language intact.
// ============================================================================

export const CLOTH: Record<string, Cloth> = {
  // weave: twill | grid | diamond | speckle | basket
  tweed: {
    base: "#46291a",
    ring: "rgba(18,10,5,0.6)",
    weave: "grid",
    line: "#6b4128",
    lineW: 0.5,
    op: 0.55,
  }, // tweed-amp oxblood-brown grille (covering is mustard twill — see TWEED_BODY)
  silver: {
    base: "#a3a6aa",
    ring: "rgba(26,30,36,0.5)",
    weave: "grid",
    line: "#6a7178",
    lineW: 0.4,
    op: 0.5,
    spark: "#9fb0bd",
  }, // blackface silver
  wheat: {
    base: "#c5ab7a",
    ring: "rgba(58,38,20,0.5)",
    weave: "grid",
    line: "#8a6b40",
    lineW: 0.42,
    op: 0.5,
  }, // brown/blonde wheat
  oxblood: {
    base: "#37231f",
    ring: "rgba(220,205,188,0.2)",
    weave: "grid",
    line: "#52332c",
    lineW: 0.42,
    op: 0.5,
  }, // dark oxblood (blonde / blackface FSR)
  saltpep: {
    base: "#2f3236",
    ring: "rgba(18,20,23,0.5)",
    weave: "speckle",
    line: "#cdcabf",
    lineW: 0,
    op: 0.38,
    border: "#ece7da",
  }, // Marshall dark salt & pepper + white piping
  diamond: {
    base: "#5c2b30",
    ring: "rgba(20,8,10,0.5)",
    weave: "lattice",
    line: "#b89243",
    lineW: 0.55,
    op: 0.72,
    border: "#c79e3c",
  }, // Vox oxblood diamond-lattice + gold piping
  blackjute: {
    base: "#34373a",
    ring: "rgba(222,212,190,0.28)",
    weave: "grid",
    line: "#4a4e52",
    lineW: 0.46,
    op: 0.6,
  }, // Mesa / boutique black
  basket: {
    base: "#c2a86b",
    ring: "rgba(70,46,18,0.4)",
    weave: "basket",
    line: "#7a5a2e",
    lineW: 0.6,
    op: 0.62,
    border: "#efe7d2",
  }, // Orange tan/wheat basketweave + cream picture-frame+ cream picture-frame
  grey: {
    base: "#6a6754",
    ring: "rgba(20,19,14,0.46)",
    weave: "basket",
    line: "#494636",
    lineW: 0.5,
    op: 0.55,
  }, // Hiwatt warm olive-brown basketweave
  black: {
    base: "#100e0b",
    ring: "rgba(239,231,214,0.26)",
    weave: "grid",
    line: "#2a2620",
    lineW: 0.4,
    op: 0.6,
  }, // generic / Ampeg / bass
  jcgrey: {
    base: "#2c2f32",
    ring: "rgba(18,20,22,0.5)",
    weave: "grid",
    line: "#444a4f",
    lineW: 0.42,
    op: 0.6,
  }, // Roland JC dark-charcoal grid
  acoustmesh: {
    base: "#141210",
    ring: "rgba(230,222,205,0.18)",
    weave: "diamond",
    line: "#352f27",
    lineW: 0.34,
    op: 0.7,
  }, // Acoustasonic fine black mesh grille
  svtcloth: {
    base: "#3c3f44",
    ring: "rgba(18,20,22,0.4)",
    weave: "speckle",
    line: "#cfd3d7",
    lineW: 0,
    op: 0.5,
  }, // Ampeg SVT silver-fleck grille
  // 1.8 silverface grille — tinted-silver base, fine grid + light/dark sparkle flecks (ref: sf-spark).
  silverturq: {
    base: "#8ba3a1",
    ring: "rgba(20,30,30,0.45)",
    weave: "sparkle",
    line: "#6c8482",
    lineW: 0.4,
    op: 0.5,
    sparkLt: "#d2dedb",
    sparkDk: "#5d736f",
  },
  // Fender Bassman Pro Neo 610/810 — fine medium-grey salt-pepper grille on black tolex.
  bassgrille: {
    base: "#8b8e92",
    ring: "rgba(20,22,24,0.4)",
    weave: "speckle",
    line: "#3a3d40",
    lineW: 0,
    op: 0.4,
  },
  // Ampeg B-15 flip-top 1x15 — green-grey speckle grille behind a wide dark frame.
  fliptopgrille: {
    base: "#565b4c",
    ring: "rgba(20,22,16,0.45)",
    weave: "speckle",
    line: "#3c4035",
    lineW: 0,
    op: 0.42,
  },
  // 1.8 EVH grille — near-black base, fine 2.1px dark grid (ref: evh-grille).
  evhgrille: {
    base: "#131311",
    ring: "rgba(8,8,7,0.5)",
    weave: "evhgrid",
    line: "#2b2925",
    lineW: 0.42,
    op: 0.7,
  },
  // Modern metal cab grille — black baffle behind a diamond-mesh metal guard
  // (Mesa Rectifier / Diezel / Bogner closed-back cabs). Lighter metal lines so the
  // diamond lattice reads as a perforated steel grille, not cloth.
  metaldiamond: {
    base: "#161619",
    ring: "rgba(8,8,8,0.5)",
    weave: "diamond",
    line: "#52525a",
    lineW: 0.7,
    op: 0.85,
  },
  // JTM45 Bluesbreaker — medium-grey salt&pepper grille with thin gold piping.
  jtmgrille: {
    base: "#474747",
    ring: "rgba(18,18,18,0.5)",
    weave: "speckle",
    line: "#cfcabb",
    lineW: 0,
    op: 0.32,
    border: "#b08d3e",
  },
  // SWR Redhead — bright white grid lines on a near-black baffle.
  whitegrid: {
    base: "#17191c",
    ring: "rgba(8,9,11,0.5)",
    weave: "grid",
    line: "#d6dadf",
    lineW: 0.55,
    op: 0.7,
  },
};

// tone (brand lineage) → cloth
// Tweed amps are covered in lacquered cotton twill (mustard/golden with a fine
// diagonal weave); that lives on the BODY (see TWEED_BODY + ChassisBody). The
// grille in front of the speaker is a separate dark-brown cloth (CLOTH.tweed).
export const TWEED_BODY = {
  base: "#cda23f",
  line: "#9a6a24",
  lineW: 0.45,
  op: 0.5,
};

export const TONE_CLOTH: Partial<Record<string, string>> = {
  tweed: "tweed",
  blackface: "silver",
  brownface: "wheat",
  blonde: "oxblood",
  wine: "oxblood",
  marshall: "saltpep",
  bluesbreaker: "jtmgrille",
  marshallvint: "basket",
  vox: "diamond",
  mesa: "blackjute",
  orange: "basket",
  hiwatt: "grey",
  boutique: "blackjute",
  friedman: "blackjute",
  ampeg: "svtcloth",
  roland: "jcgrey",
  bass: "black",
  acoustasonic: "acoustmesh",
  gk: "black",
  recto: "black",
  svt: "svtcloth",
  b15: "black",
  fliptop: "fliptopgrille",
  fenderbass: "bassgrille",
  swr: "black",
  jubilee: "black",
  jc: "jcgrey",
  silverface: "silverturq",
  evhmodern: "evhgrille",
  evhblack: "evhgrille",
};
export function clothFor(tone: string): Cloth {
  const name = TONE_CLOTH[tone];
  const cl = name !== undefined ? CLOTH[name] : CLOTH.black;
  // overlay the ref-derived per-lineage grille base (keeps the hand weave/line/ring)
  const base = name !== undefined ? CLOTH_BASE[name] : undefined;
  return base ? { ...cl, base } : cl;
}

// relative luminance of a #rrggbb colour → pick a knob pointer that contrasts.
export function lum(hex: string): number {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex);
  if (!m) return 0.5;
  const n = parseInt(m[1], 16);
  return (
    (0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) /
    255
  );
}
// Knob pointer/mark color — contrasts with the knob: a dark knob gets a WHITE
// mark, a light knob a dark mark. Near-opaque so the mark reads clearly.
export function ptrColor(c: PedalTone): string {
  return lum(c.knob) > 0.55 ? "rgba(0,0,0,0.55)" : "rgba(244,245,247,0.95)";
}

// combo front livery — logo plaque position/colour + grille piping per lineage.
// Real combos carry their controls on a TOP-mounted panel, so the front face is
// just the brand logo over an opaque grille cloth (no front knobs / speakers).
export function comboLivery(t: string): ComboLivery {
  switch (t) {
    case "vox":
      return { lx: 12, lw: 15, logo: "#d8b24a", pipe: "#c79e3c", pipeH: 1.9 };
    case "marshall":
      return {
        lx: 24,
        lw: 16,
        logo: "#ece7da",
        pipe: "#ece7da",
        pipeH: 1.4,
        script: true,
      };
    case "roland":
      return {
        lx: 12,
        lw: 16,
        logo: "#c4c8cc",
        pipe: "rgba(0,0,0,0.4)",
        pipeH: 1.2,
      };
    case "mesa":
      return {
        lx: 39,
        lw: 13,
        logo: "#cbb88e",
        pipe: "rgba(0,0,0,0.42)",
        pipeH: 1.2,
      };
    case "bass":
      return {
        lx: 12,
        lw: 16,
        logo: "#c4c8cc",
        pipe: "rgba(0,0,0,0.42)",
        pipeH: 1.2,
      };
    case "swr":
      // SWR Redhead — royal-blue tolex, signature RED control panel, chrome knobs
      return {
        panel: true,
        panelFill: "#b23a3a",
        pknob: "#dfe3e6",
        noJewel: true,
      };
    // Fender-family combos wear their control panel on top (knob row reads on the front-top)
    case "tweed":
      return {
        panel: true,
        panelFill: "rgba(40,26,14,0.92)",
        pknob: "#d8d2c2",
        noJewel: true,
      };
    case "brownface":
      return {
        panel: true,
        panelFill: "rgba(30,20,10,0.94)",
        pknob: "#d8d2c2",
      };
    case "blonde":
      return {
        panel: true,
        panelFill: "rgba(22,20,16,0.94)",
        pknob: "#d8d2c2",
      };
    default:
      return {
        panel: true,
        panelFill: "rgba(16,17,21,0.96)",
        pknob: "#cfd3d7",
      }; // Fender blackface
  }
}

// ============================================================================
// 1.8 silverface ('68 Custom) + EVH 5150 III shared parts. Translated coord-for-
// coord from the design handoff references (68-custom-deluxe-reverb-combo.svg,
// 1x12-evh-5150-g12h.svg). The brushed-alu trim/panel uses the ONLY permitted
// gradient (2-stop "sf-alu"); everything else stays flat per the style system.
// ============================================================================

// EVH channel-accent colour resolver — the label encodes the 5150 III voicing
// (green / blue / red), defaulting to green. Drives the channel jewel + accent.
// EVH 5150 channel accent — keyed off the voicing word OR the single-letter
// channel suffix in the lab (e.g. "5150C-G"/"5150 H B"/"5150C R" → green/blue/red).
export function evhAccentColor(lab: string): string {
  const L = lab.toUpperCase();
  if (L.includes("BLUE") || /(^|[ -])B($|[ ])/.test(L)) return "#3f78c0";
  if (L.includes("RED") || /(^|[ -])R($|[ ])/.test(L)) return "#cf3a2e";
  return "#3f7d4e"; // green (default / "-G")
}

// ============================================================================
// Sub-icon bodies (each draws into a 0..64 viewBox)
// ============================================================================
