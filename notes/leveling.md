# Leveling (Level tab)

Leveling drives the device in **re-amp mode** (no guitar plugged in): it plays a synthetic pickup-shaped stimulus through a preset's full DSP chain, captures the processed USB-Out, and measures integrated (gated) LUFS.

## Preset leveling — one-shot open-loop

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`. So measure once, solve `C`, compute the exact level. Per preset, `leveller::level_preset`:

1. `load_preset(slot)`, settle, drop the seize.
2. fresh connect → `set_preset_level(ref = 0.5)` → engage re-amp once → capture → measure → solve the final level.
3. fresh connect → `set_preset_level(final)` → optional verify capture → optional `save_current_preset`.

`C` (loudness at level 1.0) is each preset's max reachable loudness; a target louder than `C` clamps (surfaced in the UI). The ceiling is preset/model-specific, not a fixed dB rule.

## Scene leveling — one-shot open-loop on the active amp

Amp `outputLevel` is linear in dB with full authority, so each scene is leveled the same way as a preset: measure the scene as-is (isolated re-amp capture), solve `C`, set the scene's active amp `outputLevel`. Level the base scene via `presetLevel` first (it scales all scenes), then each footswitch scene via its amp `outputLevel`. A scene that can't reach the target clamps honestly.

Per-scene rules:

- The amp is picked per scene from the live audioGraph (a bypassed amp's knob measures flat → clamp).
- Only the amp `outputLevel` parameter is changed (preamp/master/volume are forbidden — they alter the sound).
- Scene Edit mode gives full per-scene isolation; enable + confirm it before writing a scene value.

## Footswitch leveling — the chosen block parameter

A block-acting footswitch toggles a block on/off. Each one is leveled one at a time (`level_footswitches_apply`, jobs from `footswitchesPerIndex`) by sweeping the **user-chosen block parameter** (`levGroupId`/`levNodeId`/`levParameterId`) — a secant search in _parameter_ space (not amplitude, since the parameter is not necessarily a linear gain). The solved value is applied either **baked** (a direct `changeParameter` on the block) or **assigned** (the footswitch's param-change function carries it as `valueA`); the backend picks based on whether the block is the sole owner of that parameter. Method is internal, never surfaced.

Each footswitch levels in **isolation**: every other block-acting footswitch's on/off block is forced off (`siblings_off_excluding`, fed by the `onoff_nodes`/`all_onoff_blocks` helpers) so sibling pedals don't color the capture, while the target switch's own block(s) get their engaged state (`engaged_bypass_for_switch`, per-node flip-of-base — handles multi-node mixed-direction switches). "Base" (`level_preset`) means **all footswitches off**, NOT the as-saved state: the command builds a `force_bypass` list from `all_onoff_blocks` and threads it through `measure_c`/`measure_at_level`/`measure_knob_at`. The apply path reloads the preset first (`reload_preset`) so forced bypasses are never persisted, and skips the verify capture when forcing (it would re-measure the un-isolated state after the reload; the UI falls back to `predicted_lufs`).

**Outcome taxonomy (preset & footswitch):** success = target hit; **clamped at X** = any headroom/convergence clamp (the knob has real, measured effect but can't reach target) — `clamped` with NO reason; **not on USB 1/2** = the capture had no signal there (user-configurable output routing) — `clamp_reason` is set ONLY in this case (`"no signal on USB 1/2"`, the silent-first-capture early return) and maps to the UI's `offbranch` outcome. The scene path additionally stamps `clamp_reason` for its no-authority off-branch case (big amp `outputLevel` change, no capture response) — signal present, same `offbranch` label, unchanged by this taxonomy.

## Parallel-amp rebalance — equalize lanes, then joint-level

Opt-in ("Even out parallel amps"), only on a path MERGE (two amps in parallel that re-merge). For each such scene (`level_scenes_rebalance`): mute one lane (`outputLevel = 0`) and measure the other to get each lane's solo ceiling `C`, set the two `outputLevel`s for **equal solo loudness** (quieter lane pinned at max, louder attenuated), then scale BOTH lanes' `outputLevel` by one joint factor `k` to hit the combined target. If a muted lane bleeds into the solo (small solo-above-floor margin, `probe --mute-floor`), the scene is flagged **verify by ear** — the combined target is still hit, only the lane balance is approximate. Non-mergeable scenes skip the isolation step and use the plain joint-k.

## Common-target leveling

"Level all to a common target" measures every preset's ceiling `C` and levels them all to `min(C) − headroom`, so switching presets/instruments on stage causes no loudness jump.

## Instrument profiles & calibration

A profile links a real instrument to a shipped pickup topology whose stimulus WAV is re-amped when leveling a preset the profile is assigned to. Tier-2 calibration captures the dry instrument (K-weighted LUFS) and scales the stimulus to that loudness so the amp is driven like the real instrument.

## Playback-level (Fletcher–Munson) compensation

Equal-LUFS is equal-loudness only near one SPL. The store carries a playback level (Quiet / Rehearsal / Stage); below stage volume, bass-instrument targets get a small positive offset so low-frequency energy is not under-credited. Stage is the default (zero offset).

## Capture window

The capture window (≈6 s stimulus + 0.8 s tail) is load-bearing: TMP presets are not stationary under gated-integrated LUFS (delay/reverb buildup + tail), so trimming the window shifts the measured value. The leveling default uses the full capture.

## Re-amp protocol facts

- Re-amp toggle = `SettingsMessage → reampModeActive`.
- Re-amp latches the preset/scene state at engage: set the level (or recall the scene) BEFORE engaging. `changeParameter` is audible mid-engage; `loadScene` mid-engage is not — so per-scene leveling needs one engage per scene.
- Re-amp engages reliably only once per connection — fresh-connect per engage; never disengage→re-engage on a held connection.
- A leveling run ends with a guaranteed re-amp OFF on a fresh connection (recover a stranded input-muted unit with `probe --reamp-off`).
