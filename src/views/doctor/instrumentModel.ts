// src/views/doctor/instrumentModel.ts — which instrument each chosen preset is checked
// with, pure.
//
// One instrument per preset (like Level), remembered in the app's own store on this
// computer (`profile_by_slot`, never on the unit). A preset's instrument is KNOWN when one
// is saved for it and still offered — an explicit "None" counts, it was chosen. When every
// chosen preset's instrument is known, Doctor skips its Instrument page and checks at once.

import type { Store } from "../../lib/types";
import type { SelectOption } from "../../ui/selectValue";
import type { SetupOption } from "../level/leveling";

/** One preset the chosen sounds span. */
export interface ChosenPreset {
  slot: number;
  name: string;
}

/** The presets the chosen sounds span, in slot order. */
export function presetsOf(chosen: readonly SetupOption[]): ChosenPreset[] {
  const by = new Map<number, ChosenPreset>();
  for (const o of chosen) {
    if (!by.has(o.slot)) by.set(o.slot, { slot: o.slot, name: o.presetName });
  }
  return [...by.values()].sort((a, b) => a.slot - b.slot);
}

/** The instrument saved for `slot`, when it is still one of `options`; null otherwise. */
export function savedInstrument(
  store: Store | null,
  slot: number,
  options: readonly SelectOption[],
): string | null {
  const id = store?.profile_by_slot[slot];
  return id != null && options.some((o) => o.value === id) ? id : null;
}

/** Each preset's saved instrument, or "" (not chosen yet). */
export function initialInstruments(
  presets: readonly ChosenPreset[],
  store: Store | null,
  options: readonly SelectOption[],
): Map<number, string> {
  return new Map(
    presets.map((p) => [p.slot, savedInstrument(store, p.slot, options) ?? ""]),
  );
}

/** Every preset already has a value: the Instrument page has nothing to ask. */
export function everyInstrumentChosen(
  instBySlot: ReadonlyMap<number, string>,
): boolean {
  return [...instBySlot.values()].every((v) => v !== "");
}

const TAIL = "Doctor needs it to judge the tone.";

/** The line under the Instrument page's title, naming the presets with no instrument
 *  yet; null when every preset has one. */
export function missingLine(names: readonly string[]): string | null {
  if (names.length === 0) return null;
  if (names.length === 1) return `${names[0]} doesn't have one yet. ${TAIL}`;
  if (names.length === 2)
    return `${names[0]} and ${names[1]} don't have one yet. ${TAIL}`;
  return `${String(names.length)} presets don't have one yet. ${TAIL}`;
}

/** The footer link that points at the first preset with no instrument. */
export function missingLink(names: readonly string[]): string {
  return names.length === 1
    ? `Choose an instrument for ${names[0]}`
    : `Choose an instrument for ${String(names.length)} presets`;
}
