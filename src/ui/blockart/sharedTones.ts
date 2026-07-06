// src/ui/blockart/sharedTones.ts — the physical chassis-tone palette + tone
// lookups. Data/helpers only (no components), split from ./shared for Fast
// Refresh. The id vocabularies live in ./sharedIds; the grille cloth + livery in
// ./sharedCloth.
import { TONE_BODY } from "./blockColors.generated";
import type { ToneId } from "./sharedIds";

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
