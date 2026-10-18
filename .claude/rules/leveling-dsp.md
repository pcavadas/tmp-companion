---
paths:
  - "src-tauri/src/leveller/**"
  - "src-tauri/src/lufs.rs"
  - "src-tauri/src/audio.rs"
  - "src-tauri/src/psd.rs"
  - "src-tauri/src/doctor*.rs"
  - "src-tauri/src/commands/level_*.rs"
  - "src-tauri/src/commands/doctor*.rs"
---

# Leveling and DSP rules

Applies while editing the measurement, leveling or Doctor paths.
The re-amp and destructive-write rules are in [`danger.md`](danger.md); the leveling model is [`notes/leveling.md`](../../notes/leveling.md), and the Doctor's is [`notes/doctor.md`](../../notes/doctor.md).

- **The capture window (6 s stimulus + 0.8 s tail) is load-bearing.**
  Presets are not stationary under gated-integrated LUFS (time-effect build-up, reverb decay), so an early exit, tail drop or preroll skip shifts the reading preset-dependently by up to 0.3 LU: a re-baseline, not a free speedup.
  Validate any change against a full-capture oracle, never self-consistently.
  The one sanctioned short window is the footswitch solve's far steps (`leveller::footswitch::CoarseFs`), whose readings are never accepted, skipped on or reported.
  [→](../../notes/device/reamp-audio.md#capture-window)
- **Scene leveling is one-shot open-loop on the active amp's `outputLevel`.**
  `level_scenes_apply_batched` calls `leveller::level_scenes_oneshot`: an isolated fresh capture per scene, then `solve_level` → `apply_level` with a bounded secant correction.
  Only the amp's `outputLevel` changes; preamp, master and volume are forbidden.
  [→](../../notes/leveling.md)
- **Pick the amp per scene from the live audioGraph**; a bypassed amp's knob measures flat and clamps.
  Amp ids match check-first, then strip ([`models-catalog.md`](models-catalog.md)).
- **Output Assign is per preset and re-applied to the global mixer on every preset load.**
  [→](../../notes/device/reamp-audio.md#output-routing)
- **A run always writes** (it is past the backup acknowledgment); a per-item failure becomes "skipped" and never aborts the run.
- **`clamp_reason` on a level result means the leveled signal does not reach USB 1/2**: a silent capture (`NO_SIGNAL_REASON`) or a scene amp whose `outputLevel` does not move the capture (`no_authority_reason`).
  The UI maps either to its `offbranch` outcome; a headroom clamp carries no reason.
