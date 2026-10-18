import type { ArtVariant } from "../../ui/BlockArt";

// Per-model art variant, keyed by FenderId: the explicit token the BlockArt bodies
// branch on for a treatment the row's icon + tone can't express. Art is selected by
// this token, NEVER by matching the display caption (`short`) — a caption is copy and
// can change; a variant is the art contract. `blockArtVariants.test.tsx` pins this
// table row by row.
//
// `bassman-59` and `muff-rams` have art but no model here: no catalogued Big Muff is a
// Ram's Head, and `ACD_TM59Bassman` stays unassigned — "bassman-59" would give that
// combo a thin frame, an art change.
export const ART_VARIANT: Partial<Record<string, ArtVariant>> = {
  // amps
  ACD_SLO100: "slo100-panel",
  ACD_BluesJrIV: "bluesjr",
  ACD_BluesJrIVLTD: "bluesjr-ltd",
  ACD_Bassbreaker15: "bassbreaker",
  ACD_Bassbreaker15High: "bassbreaker",
  ACD_Bassbreaker15Med: "bassbreaker",
  ACD_Evh100SGreen: "evh-green",
  ACD_Evh100SBlue: "evh-blue",
  ACD_Evh100SRed: "evh-red",
  ACD_HypersonicAmp6L6Green: "evh-green",
  ACD_HypersonicAmp6L6GreenCabIR: "evh-green",
  ACD_HypersonicAmp6L6Blue: "evh-blue",
  ACD_HypersonicAmp6L6BlueCabIR: "evh-blue",
  ACD_HypersonicAmp6L6Red: "evh-red",
  ACD_HypersonicAmp6L6RedCabIR: "evh-red",
  ACD_DiezelVh4Ch3: "diezel-petrol",
  ACD_TMBassmanTV: "bassman-tv",
  // cabs
  ACD_Bogner_412STU_Celestion_G12H: "cab-bogner",
  ACD_Bogner_412STU_Celestion_G12_V30: "cab-bogner",
  ACD_Mbg_RectifierTrad_CelestionV30: "cab-recto-trad",
  ACD_Redhead210: "cab-swr-210",
  ACD_Mar1960aV30: "cab-silver-grille",
  ACD_Hypersonic_112: "cab-evh-112",
  ACD_FenVibKng_p10rf: "cab-vibroking",
  // rack
  ACD_UA1176: "rack-1176",
  // pedals
  ACD_Glooper: "glooper",
  ACD_BigFuzz: "black-well",
  ACD_ElectricMistress: "black-well",
  ACD_SmallStone: "black-well",
  ACD_MemoryMan: "black-well",
  ACD_MemoryManStereo: "black-well",
  ACD_GreenRussianBmp: "muff-russian",
  ACD_Blumes: "gear-green-inset",
  ACD_Lightspeed: "gear-blue-inset",
};
