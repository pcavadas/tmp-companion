// src/ui/blockart/sharedCloth.ts — grille-cloth recipes + combo livery + tone
// helpers (luminance / pointer colour / EVH accent). Data/helpers only (no
// components), split from ./shared for Fast Refresh. The chassis-tone palette
// lives in ./sharedTones; the id vocabularies in ./sharedIds.
import { CLOTH_BASE } from "./blockColors.generated";
import type { PedalTone } from "./sharedTones";

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
