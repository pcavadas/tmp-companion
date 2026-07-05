import type { CatalogCategory } from "./types";
import { COMBO_CATEGORY } from "./combo";
import { HALFSTACK_CATEGORY } from "./halfstack";
import { BASS_CATEGORY } from "./bass";
import { PREAMP_CATEGORY } from "./preamp";
import { CAB_CATEGORY } from "./cabGuitar";
import { DRIVE_CATEGORY } from "./drive";
import { MOD_CATEGORY } from "./mod";
import { DELAY_CATEGORY } from "./delay";
import { REVERB_CATEGORY } from "./reverb";
import { DYN_CATEGORY } from "./dyn";
import { EQ_CATEGORY } from "./eq";
import { FILTER_CATEGORY } from "./filter";
import { PITCH_CATEGORY } from "./pitch";
import { SYNTH_CATEGORY } from "./synth";
import { FW18_CATEGORY } from "./fw18";
import { MIC_CATEGORY } from "./mic";
import { UTIL_CATEGORY } from "./util";

// Order is load-bearing: BY_ID/BY_NAME in blockArt.ts iterate in array order.
export const TMP_CATALOG: readonly CatalogCategory[] = [
  COMBO_CATEGORY,
  HALFSTACK_CATEGORY,
  BASS_CATEGORY,
  PREAMP_CATEGORY,
  CAB_CATEGORY,
  DRIVE_CATEGORY,
  MOD_CATEGORY,
  DELAY_CATEGORY,
  REVERB_CATEGORY,
  DYN_CATEGORY,
  EQ_CATEGORY,
  FILTER_CATEGORY,
  PITCH_CATEGORY,
  SYNTH_CATEGORY,
  FW18_CATEGORY,
  MIC_CATEGORY,
  UTIL_CATEGORY,
];
