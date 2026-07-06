// src/ui/blockart/shared.tsx — barrel re-exporting the block-art engine's shared
// data/helpers, split across ./sharedIds (icon/tone/form vocabularies + cab grid),
// ./sharedTones (chassis-tone palette + tone lookups) and ./sharedCloth (grille
// cloth + combo livery + luminance helpers) to keep each file cohesive and ≤500
// lines. Re-export-only, so it stays Fast-Refresh-safe.
export type { FormId, IconId, ToneId } from "./sharedIds";
export {
  formFor,
  AMP_ICONS,
  CAB_ICONS,
  MIC_ICONS,
  CAB_GRID,
} from "./sharedIds";
export type { PedalTone } from "./sharedTones";
export { PEDAL_TONES, toneOf, toneBodyHex } from "./sharedTones";
export type { Cloth, ComboLivery } from "./sharedCloth";
export {
  CLOTH,
  TWEED_BODY,
  TONE_CLOTH,
  clothFor,
  lum,
  ptrColor,
  comboLivery,
  evhAccentColor,
} from "./sharedCloth";
