import type { CatalogCategory } from "./types";

export const UTIL_CATEGORY: CatalogCategory = {
  key: "util",
  label: "Utility / Routing",
  blurb: "FX-loop inserts, external cab & IR routing.",
  blocks: [
    // Loops 1 & 2 are the analog (pre-A/D) instrument-path loops; 3 & 4 are
    // digital. All four share the fxloop icon, distinguished by tone.
    [
      "ACD_FxLoop1",
      "FX-1",
      "fxloop",
      "olive",
      "FX LOOP 1",
      "Fender utility — effects-loop send/return",
      false,
    ],
    [
      "ACD_FxLoop2",
      "FX-2",
      "fxloop",
      "red",
      "FX LOOP 2",
      "Fender utility — effects-loop send/return",
      false,
    ],
    [
      "ACD_FxLoop3",
      "FX-3",
      "fxloop",
      "chrome",
      "FX LOOP 3",
      "Fender utility — effects-loop send/return",
      false,
    ],
    [
      "ACD_FxLoop4",
      "FX-4",
      "fxloop",
      "blue",
      "FX LOOP 4",
      "Fender utility — effects-loop send/return",
      false,
    ],
    [
      "ACD_FxLoop3_4",
      "FX-3+4",
      "fxloop",
      "green",
      "FX LOOP 3+4 STEREO",
      "Fender utility — combined stereo effects loop",
      false,
    ],
    [
      "ACD_ExternalCab",
      "EXT-CB",
      "extcab",
      "ink",
      "EXTERNAL CABINET",
      "Fender utility — external cab / 4-cable-method",
      false,
    ],
    [
      "ACD_UserIRTMS",
      "IR",
      "ir",
      "ink",
      "IMPULSE RESPONSE",
      "Fender utility — user impulse-response (IR) loader",
      false,
    ],
  ],
};
