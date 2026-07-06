import type { CatalogCategory } from "./types";

export const SYNTH_CATEGORY: CatalogCategory = {
  key: "synth",
  label: "Synth",
  blurb: "Fender's own guitar-synth algorithms.",
  blocks: [
    [
      "ACD_GuitarSynth",
      "CERBRS",
      "synth",
      "orange",
      "CERBERUS POLYSYNTH",
      "Fender original (3-voice guitar polysynth)",
    ],
    [
      "ACD_GuitarSynthLite",
      "AETHON",
      "synth",
      "black",
      "AETHON POLYSYNTH",
      "Fender original (single-voice guitar synth)",
    ],
    [
      "ACD_WaveMorphSynth",
      "WAVMOR",
      "synth",
      "chrome",
      "WAVEMORPH",
      "Fender original (wave-morphing synth)",
    ],
  ],
};
