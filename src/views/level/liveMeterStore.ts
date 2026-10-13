// src/views/level/liveMeterStore.ts — the leveling run's live meter, in its own tiny
// module store.
//
// The backend streams a loudness reading ~5 times a second while a capture runs. Only
// `RunPage` shows it (the header VU + readout and the active row's live number), so it
// lives apart from `levelRunStore`: a tick re-renders RunPage alone, never LevelView, the
// preset list or the footer. `levelRunStore` writes it; RunPage reads it with `useLiveMeter`.

import { useSyncExternalStore } from "react";

export interface LiveMeter {
  /** Advisory live measured loudness, or null when nothing is measuring. */
  lufs: number | null;
  /** Rolling per-hop momentary levels (dB) for the live VU bars — last 24 hops. */
  trace: number[];
}

const TRACE_HOPS = 24;
export const IDLE_METER: LiveMeter = { lufs: null, trace: [] };

let meter: LiveMeter = IDLE_METER;
const subs = new Set<() => void>();

/** Replace the meter (IDLE_METER clears it). A no-op when already idle. */
export function setLiveMeter(next: LiveMeter): void {
  if (next === meter) return;
  meter = next;
  for (const cb of subs) cb();
}

/** Append one streamed reading. */
export function pushLiveMeter(lufs: number, momentary: number): void {
  setLiveMeter({ lufs, trace: [...meter.trace, momentary].slice(-TRACE_HOPS) });
}

const subscribe = (cb: () => void) => {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
};

export function useLiveMeter(): LiveMeter {
  return useSyncExternalStore(subscribe, () => meter);
}
