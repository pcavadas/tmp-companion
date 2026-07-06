// src/ui/blockart/sharedIds.ts — the icon/tone/form id vocabularies + form
// resolution and the cab speaker grid. Data/helpers only (no components), split
// from ./shared so the module stays a Fast-Refresh-safe data module. The chassis-
// tone palette lives in ./sharedTones; the grille cloth in ./sharedCloth.

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
