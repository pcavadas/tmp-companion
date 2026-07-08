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
- Amp candidate choice has TWO filters: classify amps by stable `FenderId` / `model_id`, then allow ONLY the amp `outputLevel` parameter (verified against the fw 1.7.75 + 1.8.45 embedded amp schemas). `output` is not an amp control; `level` on `ACD_TMRumbleV3` is an amp knob but NOT the output leveling control — it must not be changed. Preamp/master/volume params (`brightvolume`, `mastervolume`, `normalvolume`, `volume`) are forbidden — they alter the preset's sound. HW-load-bearing: preset 001 Klon's Hiwatt hit `-9.610 LUFS` only once its actual `outputLevel` was at 100%; a first-active-param pick chases earlier volume params and falsely clamps.
- Scene Edit mode (`SetNodeSceneEdit` true) gives FULL per-scene isolation — a scene's `outputLevel` write does not touch the base/global value, and enabling scene mode is harmless in itself (it IS the isolation). The leak is only about ORDER: a value write landing BEFORE scene mode is enabled+confirmed hits the BASE scene (which un-scene-moded slots inherit) — a settle/race on a congested line (`SETTLE_AFTER_SCENE_EDIT_MS`=600; a 2nd convergence pass also clears it). So enable+confirm scene mode, THEN write.
- The BASE scene's blocks have no scene mode; only CHANGE a scene's `outputLevel` when it actually needs it (pure cleanliness, not safety) — `level_scenes_oneshot` measures each scene AS-IS (`measure_scene_asis`, no write) and skips scenes already at the solved level.
- `level_scenes_oneshot`/`jointk_one_scene` run a BOUNDED secant correction (`correct_iter`, cap `SCENE_CORRECT_MAX`=3, ±`BATCH_TRUST_DB` trust region) that always lands+saves the BEST point, so `apply_levels` never persists a worse number than reported. A ≥6 dB `outputLevel` change that doesn't move the USB 1/2 capture (`no_authority`) is an honest clamp with a distinct `clamp_reason` ("off-branch / off-USB"), restoring the amp to base.
- The device pushes NO field-3 while re-amp is engaged, so knob picks + per-scene knob values resolve in an un-engaged PRE-PASS (`prepass_scene_docs` + `build_scene_jobs`: one rich session loads the preset and harvests each scene's field-3 doc via loadScene → push).

### Retired: the BatchedLive closed-loop harness

`leveller::level_scenes_live_batched` is retained ONLY for the `probe --bench-scene-leveling` harness (`<slots_csv> <target> <topology> <out.json>`; it streams rows to `<out>.json.jsonl` incrementally — an end-only write once lost a 40-min run to an OOM reboot). Mechanics: the preset loads once, ONE `audio::LiveReamp` stream pair runs for the whole preset (silence between engages), each scene gets a lean engage connection (`set_knob` = scene recall + Scene Edit + start value BEFORE engage), then trust-region slope jumps (`next_live_coord(LiveHybrid)`, clamped ±6 dB/move — unclamped jumps overshoot steep knobs by ~6 LU; pure secant is noise-fragile on ±0.2 LU windows; fixed-gain proportional stalls on compressed responses) against 2 s live windows. HW: 6.7–9.4 s/scene converged (±0.3 LU gate), ~20 s on weak-authority picks.

**Why it was retired — it MIS-MEASURES on its shared stream (fw 1.8.45):** the single continuous `LiveReamp` stream read with `live_window_lufs` windows returns impossible per-scene loudness (preset 001 Klon read `-6.96 LUFS` when that amp `outputLevel`'s true range is `-40…-14`), so the trust-region loop feeds on garbage and clamps (the secant flips to the loud bound on a near-zero/negative slope — no monotonicity guard). The correct method is an ISOLATED fresh re-amp capture per measurement point (the `measure_knob_at` shape, fresh stream pair each point) — and the closed loop is unnecessary anyway: amp `outputLevel` is LINEAR in dB with FULL authority (~25 LU range HW-measured), so the one-shot open-loop above hit every scene of preset 001 within ±0.07 LU (`probe --level-preset-scenes`, isolated measure).

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
