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
