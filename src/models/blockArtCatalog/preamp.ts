import type { CatalogCategory } from "./types";

export const PREAMP_CATEGORY: CatalogCategory = {
  key: "preamp",
  label: "Preamps",
  blurb: "Acoustic & studio preamps (no power amp or speaker).",
  blocks: [
    [
      "ACD_Acoustasonic",
      "ACOUST",
      "amp",
      "acoustasonic",
      "FENDER ACOUSTASONIC",
      "Fender Acoustasonic preamp",
    ],
    [
      "ACD_StudioPreamp",
      "STUPRE",
      "rack",
      "chrome",
      "STUDIO PREAMP",
      "Clean studio mixing-desk preamp (uncolored)",
    ],
    [
      "ACD_StudioTubePreamp",
      "TUBEPR",
      "racktube",
      "slate",
      "TUBE PREAMP",
      "Tube console-style studio preamp",
    ],
  ],
};
