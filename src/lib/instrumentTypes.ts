// src/lib/instrumentTypes.ts — what an instrument TYPE is called on screen.
//
// The backend groups its pickup topologies by `instrument` ("guitar" | "bass" |
// "bass-vi", `topologies.rs`); those values are keys, not words. The types are Guitar,
// Bass and Bass VI. Acoustic is not a type: it is a pickup choice under Guitar
// ("Acoustic / piezo"). A value the map doesn't know yet (a type a newer backend adds)
// is shown readable rather than raw: "baritone-vi" → "Baritone vi".

import type { TopologyInfo } from "./types";

const TYPE_LABEL: Partial<Record<string, string>> = {
  guitar: "Guitar",
  bass: "Bass",
  "bass-vi": "Bass VI",
};

/** The on-screen name of an instrument type ("bass-vi" → "Bass VI"). */
export function instrumentTypeLabel(instrument: string): string {
  const known = TYPE_LABEL[instrument];
  if (known != null) return known;
  const words = instrument.replace(/[-_]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** An instrument's "{type} · {pickup}" caption ("Guitar · Humbucker"). A pickup that
 *  already starts with its type's name says both on its own, so it stands alone: a
 *  Bass VI reads "Bass VI", never "Bass VI · Bass VI", and a bass pickup "Bass
 *  humbucker", never "Bass · Bass humbucker". */
export function instrumentCaption(topology: TopologyInfo): string {
  const type = instrumentTypeLabel(topology.instrument);
  const pickup = topology.label;
  const lower = pickup.toLowerCase();
  const typeLower = type.toLowerCase();
  return lower === typeLower || lower.startsWith(`${typeLower} `)
    ? pickup
    : `${type} · ${pickup}`;
}
