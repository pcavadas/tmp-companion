// src/views/doctor/estimateSecsLeft.ts — pure helper for DoctorRun's live
// "about Ns left" estimate. Split out of DoctorRun.tsx (a component file must
// export only the component, or Fast Refresh — react-refresh/only-export-components —
// stops working for it).

/** Live remaining-time estimate: remaining sounds × the measured per-sound
 *  rate, counting down through the current sound but never dipping below the
 *  queued sounds' worth (so an overrunning sound holds rather than hitting 0
 *  with work left). */
export function estimateSecsLeft(
  remaining: number,
  avgMs: number,
  elapsedOnCurrentMs: number,
): number {
  if (remaining <= 0) return 0;
  const raw = remaining * avgMs - elapsedOnCurrentMs;
  const floor = (remaining - 1) * avgMs;
  return Math.ceil(Math.max(raw, floor) / 1000);
}

/** Measured per-sound rate with a pseudo-count prior: the prior counts as one
 *  observation, so early completions NUDGE the rate instead of replacing it
 *  (a slow first sound — per-preset read + connect retries — must not
 *  multiply the whole remaining estimate). Zero-length gaps (batched events)
 *  are not real durations and are skipped. */
export function avgSoundMs(priorMs: number, doneAts: number[]): number {
  let sum = priorMs;
  let n = 1;
  for (let i = 1; i < doneAts.length; i++) {
    const d = doneAts[i] - doneAts[i - 1];
    if (d > 0) {
      sum += d;
      n += 1;
    }
  }
  return sum / n;
}
