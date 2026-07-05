import type { IconId, ToneId } from "../../ui/BlockArt";

// One catalog row: [fenderId, terse caption, icon, chassis tone, full name,
// blurb, available?]. icon/tone are validated against the renderer's id unions
// at compile time, so a typo in a catalog row is a tsc error.
export type BlockRow = readonly [
  id: string,
  short: string,
  icon: IconId,
  tone: ToneId,
  name: string,
  blurb: string,
  available?: boolean,
];
export interface CatalogCategory {
  key: string;
  label: string;
  blurb: string;
  blocks: readonly BlockRow[];
}
