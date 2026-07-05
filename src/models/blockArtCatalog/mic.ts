import type { CatalogCategory } from "./types";

export const MIC_CATEGORY: CatalogCategory = {
  key: "mic",
  label: "Microphones",
  blurb: "Cab-mic models (a parameter list, not DSP blocks).",
  blocks: [
    ["MIC_C414", "C414", "mic_c414", "chrome", "CONDENSER C414", "AKG C414"],
    [
      "MIC_M23",
      "M23",
      "mic_pencil",
      "chrome",
      "CONDENSER M23",
      "Earthworks Audio M23",
    ],
    [
      "MIC_MD421",
      "MD421",
      "mic_421",
      "black",
      "DYNAMIC MD421",
      "Sennheiser MD 421",
    ],
    [
      "MIC_R121",
      "R121",
      "mic_ribbon",
      "chrome",
      "RIBBON R121",
      "Royer Labs R-121",
    ],
    [
      "MIC_RE20",
      "RE20",
      "mic_re20",
      "black",
      "DYNAMIC RE20",
      "Electro-Voice RE20",
    ],
    ["MIC_SM7B", "SM7B", "mic_sm7b", "black", "DYNAMIC SM7B", "Shure SM7B"],
    ["MIC_SM57", "SM57", "mic_sm57", "graphite", "DYNAMIC SM57", "Shure SM57"],
  ],
};
