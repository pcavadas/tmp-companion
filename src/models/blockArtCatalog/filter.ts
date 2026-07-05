import type { CatalogCategory } from "./types";

export const FILTER_CATEGORY: CatalogCategory = {
  key: "filter",
  label: "Filter / Wah",
  blurb: "Wahs and envelope / auto filters.",
  blocks: [
    [
      "ACD_CryBabyQ535",
      "535Q",
      "wah",
      "black",
      "CUSTOM WAH",
      "Dunlop Cry Baby 535Q",
    ],
    [
      "ACD_CryBabyGCB95",
      "GCB95",
      "wah",
      "black",
      "TEARDROP WAH",
      "Dunlop Cry Baby GCB-95",
    ],
    ["ACD_CryBabyV847", "V847", "wah", "black", "VOCAL WAH", "Vox V847 Wah"],
    [
      "ACD_MicroTronIV",
      "MUTRON",
      "envf",
      "blue",
      "FILTRON",
      "Mu-Tron Micro-Tron IV (envelope filter)",
    ],
    [
      "ACD_KorgA2AutoWah",
      "KORG-A3",
      "envf",
      "chrome",
      "ENIGMA FILTER",
      "Korg A3 (filter setting)",
    ],
    [
      "ACD_EcFilter",
      "ENVFLT",
      "envf",
      "yellow",
      "ENVELOPE FILTER",
      "Fender original (envelope filter)",
      false,
    ],
  ],
};
