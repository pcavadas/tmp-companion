---
paths:
  - "src-tauri/src/leveller.rs"
  - "src-tauri/src/lufs.rs"
  - "src-tauri/src/audio.rs"
  - "src-tauri/src/psd.rs"
  - "src-tauri/src/doctor.rs"
  - "src-tauri/src/commands/level_*.rs"
  - "src-tauri/src/commands/doctor.rs"
---

# Leveling and DSP rules

Applies while editing the measurement, leveling or Doctor paths. **Re-amp engage/latch behaviour and the destructive-write rules live in `danger.md`** (always loaded) — read that first. The full model is `notes/leveling.md`.

- **The capture WINDOW (6 s stimulus + 0.8 s tail) is LOAD-BEARING.** Trimming it is a ≤0.3 LU **re-baseline**, not a free speedup (HW-A/B'd via `probe --measure-adaptive`). TMP presets are NOT stationary under gated-integrated LUFS: time-effect and reverb presets have a quiet buildup at the start and a decay tail the full capture integrates, so early-exit, tail-drop, or a preroll-skip each shift the measured loudness preset-dependently (clean +0.30 / delay +0.16 / reverb −0.02 LU). Validate any change against a **full-capture oracle**, never self-consistently. [→ evidence](../../notes/gotchas.md#the-leveling-capture-window-6-s-stimulus--08-s-tail-is-load-bearing--trimming-it-is-a-03-lu-re-baseline-not-a-free-speedup-hw-abd-probe---measure-adaptive)
- **Scene leveling is ONE-SHOT open-loop on the active amp's `outputLevel`.** `level_scenes_apply_batched` calls `leveller::level_scenes_oneshot` — measure each scene as-is via an ISOLATED fresh re-amp capture, then `solve_level` → `apply_level` with a bounded secant correction. Only the amp's `outputLevel` changes; preamp, master and volume are forbidden. [→ evidence](../../notes/gotchas.md#scene-leveling-is-one-shot-open-loop-on-the-active-amps-outputlevel)
- **Pick the amp PER SCENE from the live audioGraph.** A bypassed amp's knob measures flat and clamps.
- **Amp-id matching must be CHECK-FIRST then strip** (`CabIR`/`ConvRvb`) — a discovered amp block can carry merged suffixes the catalog's bare bid lacks. [→ evidence](../../notes/gotchas.md#amp-id-matching-must-be-check-first-then-strip-cabirconvrvb)
- **Output Assign is per-preset and is re-applied to the global mixer on every preset LOAD** — loading a preset overwrites the unit's output-assign matrix from that preset's `outputMixerSettings`. [→ evidence](../../notes/gotchas.md#output-assign-is-per-preset-and-is-applied-to-the-global-mixer-on-every-preset-load)
- **A run always WRITES** (it is post-disclaimer); a per-item failure becomes "skipped" and never aborts the run.
- `clamp_reason` on a level result means ONLY "no signal on USB 1/2" (a silent capture → off-branch in the UI). Headroom clamps are reason-less.
