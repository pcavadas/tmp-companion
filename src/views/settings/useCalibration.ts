// src/views/settings/useCalibration.ts — one instrument row's Tier-2 calibration (its
// "recording", on screen) as a single tagged union (#313, after #301's per-attempt token
// fix):
//   idle{result, warnings} → countdown{n} 3·2·1 → recording{secs} → idle | error
// so an impossible mix of flags (a countdown showing a stale result, an error beside a
// warning) cannot be expressed. A failed attempt carries no text: the page says the
// same thing whatever went wrong, and the backend's own text is in the log.
//
// Every attempt gets its own token (`attemptRef`): a calibrate_profile promise applies
// its result only while its own attempt is still the current one, so a cancelled /
// superseded / unmounted attempt never applies its result, clears a newer attempt's
// timers, or fires a spurious `onCalibrated`. It does NOT stop the backend capture
// itself — that still runs to completion on the device and may persist; the
// recording-cancel path re-reads the store to surface any value the device saved.

import { useEffect, useRef, useState } from "react";

import { calibrateProfile, logUnlessCommandError } from "../../lib/invoke";
import type { CalibrateResult, Profile } from "../../lib/types";

// Tier-2 capture length passed to calibrate_profile (backend clamps 2..30).
export const CALIBRATE_SECS = 8;

export type Calibration =
  /** At rest. `result` is the last SUCCESSFUL take this session (drives the
   *  "what was captured" readout — session-only, never persisted); `warnings` its
   *  non-fatal quality caveats (clip / stimulus ceiling), one sentence pair each. */
  | { kind: "idle"; result: CalibrateResult | null; warnings: string[] }
  | { kind: "countdown"; n: number }
  /** `secs` drives the DETERMINATE bar (0 → CALIBRATE_SECS over ~8 s) while the real
   *  backend capture runs concurrently; the backend result wins the phase. */
  | { kind: "recording"; secs: number }
  | { kind: "error" };

const IDLE: Calibration = { kind: "idle", result: null, warnings: [] };

/** What leveling re-amps for this instrument, shown on the row at all times
 *  (#361): the player's stored recording, or the synthetic test tone. */
export function stimulusSource(profile: Profile): string {
  return profile.has_capture === true
    ? "Leveling uses your recording."
    : "Leveling uses the test tone.";
}

/** The take's non-fatal caveats, one line each — empty when it was clean.
 *
 * #361: the unit's dry USB 3 send clips on hard picks from a normal guitar, so a
 * mildly clipped take is stored without comment. A take past the severity rule is
 * explained as hard picks being too loud for the unit's USB recording. Never tell the
 * player to play softer and never point at the unit's mixer meter (its RMS ballistics
 * hide sub-ms flat tops); an on-unit lever is named here only once it has been checked
 * on a real unit. No LU figure: the shortfall is said in words. */
export function warningsOf(res: CalibrateResult): string[] {
  const warns: string[] = [];
  if (res.clip === "severe")
    warns.push(
      res.kept_previous_capture
        ? "Hard picks were too loud for the Tone Master Pro's USB recording, so this take wasn't kept. Leveling still uses your earlier recording."
        : "Hard picks were too loud for the Tone Master Pro's USB recording, so this take wasn't kept. Leveling uses the test tone at your loudness.",
    );
  if (res.stimulus_shortfall_lu != null)
    warns.push(
      "You play louder than the test tone can go, so your sounds are tested quieter than you play.",
    );
  return warns;
}

export function useCalibration(profileId: string, onCalibrated: () => void) {
  const [cal, setCal] = useState<Calibration>(IDLE);

  const countdownRef = useRef<number | null>(null);
  const recIntervalRef = useRef<number | null>(null);
  const attemptRef = useRef(0);

  function clearTimers() {
    if (countdownRef.current != null) {
      window.clearTimeout(countdownRef.current);
      countdownRef.current = null;
    }
    if (recIntervalRef.current != null) {
      window.clearInterval(recIntervalRef.current);
      recIntervalRef.current = null;
    }
  }

  // On unmount, retire the in-flight attempt too (not just the timers) so its
  // .then()/.catch() can't fire a spurious onCalibrated.
  useEffect(
    () => () => {
      attemptRef.current += 1;
      clearTimers();
    },
    [],
  );

  // Recording: the bar advances in 0.1 s steps while the real capture runs.
  function startRecording(attempt: number) {
    setCal({ kind: "recording", secs: 0 });
    const stepMs = 100;
    const stepSecs = stepMs / 1000; // seconds advanced per step
    recIntervalRef.current = window.setInterval(() => {
      setCal((c) =>
        c.kind === "recording"
          ? {
              kind: "recording",
              secs: Math.min(CALIBRATE_SECS, +(c.secs + stepSecs).toFixed(1)),
            }
          : c,
      );
    }, stepMs);

    calibrateProfile(profileId, CALIBRATE_SECS)
      .then((res) => {
        if (attemptRef.current !== attempt) return;
        clearTimers();
        setCal({ kind: "idle", result: res, warnings: warningsOf(res) });
        onCalibrated();
      })
      .catch((e: unknown) => {
        logUnlessCommandError("instrument recording", e);
        if (attemptRef.current !== attempt) return;
        clearTimers();
        setCal({ kind: "error" });
      });
  }

  /** Countdown 3 → 2 → 1 at ~850 ms steps, then start recording. */
  function start() {
    attemptRef.current += 1;
    const attempt = attemptRef.current;
    clearTimers();
    let n = 3;
    setCal({ kind: "countdown", n });
    const tick = () => {
      n -= 1;
      if (n <= 0) {
        startRecording(attempt);
        return;
      }
      setCal({ kind: "countdown", n });
      countdownRef.current = window.setTimeout(tick, 850);
    };
    countdownRef.current = window.setTimeout(tick, 850);
  }

  function cancel() {
    const wasRecording = cal.kind === "recording";
    attemptRef.current += 1;
    clearTimers();
    setCal(IDLE);
    // The backend calibrate_profile may have ALREADY persisted before the user hit
    // cancel; re-read the store so a just-saved value surfaces instead of staying "not
    // calibrated". (The in-flight promise's .then() is retired, so this can't
    // double-apply.)
    if (wasRecording) onCalibrated();
  }

  return { cal, start, cancel };
}
