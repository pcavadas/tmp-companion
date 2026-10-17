// src/views/doctor/estimateSecsLeft.ts — pure helpers for the Check page's live
// "About 30 seconds left": the estimate and its words. Split out of DoctorRun.tsx (a
// component file must export only the component, or Fast Refresh —
// react-refresh/only-export-components — stops working for it).

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

/** The time-left caption: "About 30 seconds left", and from a minute up in whole minutes,
 *  rounded up ("About 2 minutes left"). */
export function timeLeftText(secs: number): string {
  if (secs < 60) {
    const s = Math.max(1, Math.round(secs));
    return `About ${String(s)} ${s === 1 ? "second" : "seconds"} left`;
  }
  const m = Math.ceil(secs / 60);
  return `About ${String(m)} ${m === 1 ? "minute" : "minutes"} left`;
}
