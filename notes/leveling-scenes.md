# Scene and footswitch leveling

Read this when changing the scene or footswitch lanes, the parallel-amp rebalance, the verify-by-ear causes or the post-save check.

The model, the base lane, run order and outcomes are in [leveling.md](leveling.md); the device rules for scene context, write landing and recall are in [device/scenes.md](device/scenes.md).
Facts are for firmware 1.8.58 unless a line says otherwise.

## Scene leveling

Amp `outputLevel` is linear in dB with full authority, so each scene is leveled like a preset: measure the scene as-is (an isolated re-amp capture), solve `C`, set the scene's active amp `outputLevel`; base solves land within ±0.03 LU and scenes within ±0.05 LU [HW].
Base is leveled first through `presetLevel`, which scales every scene; each scene then moves only its own amp's `outputLevel`.
The app command `level_scenes_apply_batched` runs `leveller::level_scenes_oneshot`: measure each scene as-is, `solve_level`, then `apply_level`, a bounded correction that always lands the best point.
A scene that cannot reach the target clamps honestly ("clamped at <LUFS>").
Every base-context write or measure recalls base first ([device/scenes.md#scene-context](device/scenes.md#scene-context)).

### Amp choice

The amp is picked per scene from the scene's graph (a bypassed amp's knob measures flat and clamps).
Two filters choose the parameter: amps are classified by stable `FenderId` / `model_id`, then only the amp `outputLevel` parameter is allowed (embedded amp schemas).
`output` is not an amp control, and `level` on `ACD_TMRumbleV3` is an amp knob but not the output leveling control, so it is never changed.
Preamp, master and volume parameters (`brightvolume`, `mastervolume`, `normalvolume`, `volume`) are forbidden: they alter the preset's sound.
A first-active-parameter pick chases those earlier volume params and falsely clamps [HW].
An amp whose preset document omits `outputLevel` is refused as level-manually (seen on `ACD_SVTBlueLineCabIR`).
Amp ids carrying merged cab / IR / convolution suffixes are matched check-first, then stripped (`is_amp_model_id`, `resolveDeviceId`), so an amp+cab combo is still detected (`.claude/rules/models-catalog.md`).

### Scene Edit and write landing

`set_knobs` decides per node from the run's one field-8 read, classified by `scene_jobs::scene_overlay` over the raw `scenes[]` (after `overlay_scene_onto_graph` a base value is indistinguishable from an overlaid one).
The classifier, its Scene-Edit enable rule, its refusals and its pre-write gap are in [device/scenes.md#the-apps-classifier](device/scenes.md#the-apps-classifier).
A `BypassOnly` refusal has one exception: a write that would be audible only in this scene (`shared_write_is_scene_local`) is written to base directly.
The isolation matrix the classifier was built against is in `probe_api/slot_write.rs`'s `--set-scene-param` doc.

### Scene documents

Knob picks and per-scene knob values resolve before any engage.
The batched scene run derives each scene's document from the run's one complete field-8 read, each scene's stored overlay applied over base (`prepass_scene_docs_via` → `scene_docs_from_saved`), which matches the live walk [HW].
The read is complete-or-fail for `audioGraph` and `scenes` (and `ftsw` with base in the batch), so a cut tail is re-read from a device backup or refused.
A saved document that still cannot answer (no `audioGraph`, or a requested scene index outside `scenes[]`) is refused with a readable reason before anything reaches the unit (`saved_scene_docs_refusal`).
That bound matters: a `loadScene` on a preset with no scenes substitutes the empty template and answers `presetError` 6 [RE], and a `loadScene` index past a preset's scenes likely aborts the device server [RE, likely] ([device/scenes.md#scene-index-bounds](device/scenes.md#scene-index-bounds)).
The live walk (`prepass_scene_docs`) is probe-only.

`classify_scene_knobs` refuses a scene whose document answers for no amp at all, rather than classifying it against the base graph: a live field-3 document cut before the amp nodes drives the base amp in a scene whose own overlay swaps to another [HW].
The refusal is repaired from the saved preset through `read_slot_preset_complete`'s backup-backed read (`audioGraph` + `scenes`); a scene the saved preset cannot answer either is skipped and reported, never leveled against base.
Gates: `a_truncated_swap_scene_levels_its_own_amp_not_the_base_one`, `a_scene_whose_doc_never_arrived_skips_instead_of_classifying_against_base`, `a_scene_whose_doc_is_partial_skips_instead_of_classifying_against_base`, `a_scene_whose_saved_overlay_is_unclassifiable_skips_instead_of_losing_its_headroom_silently`.

For the scene already active (the saved `lastLoadedScene`), the probe-only live walk reads the post-load document instead of recalling it, because re-recalling the active scene pushes no field 3 and would leave that scene unclassifiable [HW+RE] ([device/scenes.md#scene-context](device/scenes.md#scene-context)).
The prepass is also the runners' preset load: `level_scenes_oneshot` and `level_scenes_rebalance` never re-load the preset, and the UI batches adjacent same-preset scene rows that share instrument and target into one `level_scenes_apply_batched` call.
Together they stop the unit from flashing back to the preset's base twice per scene.

### One save per preset

Scene writes accumulate unsaved in the working copy (`LevelOptions.defer`: no per-apply save, no restore), and the runner persists them with one `saveCurrentPreset` (field 14) at batch end (`save_deferred_scene_writes`).
That save first recalls the preset's original `lastLoadedScene` (`recall_original_scene`), so it stamps the scene the preset was loaded in, then saves on a connection that never re-amped, with one retry.
It also fires on cancel, so already-reported scenes persist.
One save works because MAINTAIN CHANGES keeps unsaved overlay writes across recalls and reconnects; under DISCARD CHANGES that one pre-save recall would wipe every deferred write ([device/scenes.md#scene-change-behavior](device/scenes.md#scene-change-behavior)).
The deferred-write lanes (`level_scenes_apply_batched`, `level_footswitches_apply`) therefore refuse a positively read DISCARD before any device write (`scene_discard_guard`, read from the startup settings backup).
Never add a preset load before `save_deferred_scene_writes`: it would be a real reload that wipes the deferred writes ([device/presets.md#save-then-load-on-one-connection](device/presets.md#save-then-load-on-one-connection)).
The batch save can revert the one scene it leveled when that scene is also `restore_scene` ([device/scenes.md#batched-scene-save](device/scenes.md#batched-scene-save)).

### Correction and dropped writes

`level_scenes_oneshot` and `jointk_one_scene` run a bounded secant correction (`correct_iter`, capped at `MEASURE_CORRECT_MAX` = 3, inside a ±`BATCH_TRUST_DB` trust region) that always lands and saves the best point, so `apply_levels` never persists a worse number than it reports.
The open-loop solve alone undershoots by up to 5.3 LU through a compressed chain (the Bass VI presets' synth chains), the error growing with the size of the move, and the correction absorbs it [HW].
A scene already at the solved level is not written (`measure_scene_asis` takes the as-is reading with no write).
A ≥ 6 dB `outputLevel` change (`NO_AUTHORITY_MIN_DB`) that does not move the USB 1/2 capture by `KNOB_TOL_LU` is an honest `no_authority` clamp with its own `clamp_reason` (`no_authority_reason`), and the amp is restored to base.
The first applied move is whatever the open-loop solve asked for, so a scene 4 dB from target gets a 4 dB move and a zero-authority knob would fall through to a reason-less headroom clamp.
`correct_iter` therefore takes one confirmation capture when the first move produced a flat response: it steps down a full `NO_AUTHORITY_MIN_DB` and looks again (down only, because a raise can clip and a saturating limiter makes an upward non-response ambiguous); if that probe moves the capture, it seeds the secant.
A healthy knob moves ~1 LU per dB, so this never fires on a working solve (`level-trade.spec.ts`'s zero-authority taxonomy spec).

The first verified apply re-applies once when a ≥ 2 dB intended move (`SUSPECT_DROP_MIN_DB`) reads back nearly unchanged (< `KNOB_TOL_LU`) (`apply_first_verified`).
A scene write can drop silently ([device/scenes.md#the-apps-classifier](device/scenes.md#the-apps-classifier)) and a verify capture can mis-measure once by several LU (one read 2.9 LU low) [HW]; one retry disambiguates, and a genuine no-authority amp stays flat on the retry and takes the honest clamp.
Even 6:1 compression passes ~0.33 LU of a 2 dB move, so the signature is not compression.
The scene verify capture stays: it is also the correction-pass trigger and the leak-to-base detector.

`probe --bench-scene-leveling` runs the closed-loop `level_scenes_live_batched`; it is a bench only, and it mis-measures on its shared stream.

## Footswitch leveling

A block-acting footswitch toggles a block on or off.
`level_footswitches_apply` levels each switch (jobs from `footswitchesPerIndex`) by sweeping the user-chosen block parameter (`levGroupId` / `levNodeId` / `levParameterId`) with a secant search in parameter space, since the parameter is not necessarily a linear gain.
The command measures every switch first, then `write_footswitch_values` commits all of them on one live-edit session with one save; every write is confirm-gated, and any unconfirmed write aborts before the save, so nothing half-applied persists.

### Assign or bake

`plan_footswitch_jobs` assigns iff the switch already carries a `param` function on the selected (node, parameter): that function's `valueA` is edited.
Everything else bakes: a direct `changeParameter` on the block.
Leveling never creates or deletes a footswitch function, and a block bypassed in base that this switch does not enable is a clamp.
A bake carries `mirror_scenes`, the scenes whose overlay restates the base value (`scenes_restating_base`, `SCENE_PARAM_EPS`), and `write_fs_values_on_session` writes the solved value into each of those overlays after the base write (one recall per scene, no Scene Edit enable since the overlay exists).
A full-parameter overlay masks base [HW], so the bake lands in every uniform scene and every authored divergence survives.
The write session then recalls the preset's original `lastLoadedScene` before its one save, because a save stamps the active scene [HW]; `level_preset`'s save does the same through `LevelOptions::restore_scene`.
The method is internal and never surfaced.

### Isolation

Each footswitch levels in isolation: every other block-acting switch's on-off block is forced off (`siblings_off_excluding`, fed by `onoff_nodes` / `all_onoff_blocks`), and the target switch's own blocks get their engaged state (`engaged_bypass_for_switch`).
The engaged state is `isActive`-aware, not a flip of base: an on-off switch is a latching toggle whose `isActive` records whether the preset was saved with it active, so the engaged bypass is the saved bypass when `isActive` is set and the flip otherwise [HW]; the Doctor's `doctor_force_bypass` shares this helper.
The isolation list is re-sent on every capture (`leveller::arm_measurement`), because every capture recalls base first and a recall re-asserts the scene's own bypass state; sending it once measures every later point non-isolated (gate: `solve_footswitch_resends_the_isolation_list_on_every_capture`).

`plan_footswitch_jobs` must also assert a base-on block's own bypass, not only its siblings'.
A sibling row's isolation writes this block's `bypass = true` into the working copy, and every capture's base recall re-asserts that mutated copy, so a row built from `siblings_off_excluding` alone measures with the very block it levels switched off [HW].
Its handle then has no authority, `solve_param_secant` sees an unmoving reading and reports a reason-less `clamped scene_ceiling`, in any batch with more than one row, whatever the order.
The tell is `measured_lufs == predicted_lufs` to four decimals: the row measured a sound its knob does not affect.
The ceiling prepass goes through `footswitch::switch_states`, which does assert the switch's own nodes, and `plan_on_in_base_still_asserts_its_own_block_bypass` asserts the plan builds the same list.
When a preset's base sound is a switch's engaged sound, the right answer for that row once base is leveled is the authored value it already had, which can look like a no-op.

### Secant seeds

`solve_param_secant`'s seed 1 is the switch's current value when it has one (an on-target reading there is an idempotency skip), else 25% of the parameter's range.
Seed 2 is the linear-law prediction when it is plausible (inside 5–95% of the range, expected gap ≥ `FS_MIN_SEED_GAP_LU` = 1), else the complement fraction (75% or 25%).
A flat seed pair triggers `fs_bracket_expansion` toward the bound the target needs.
The correction loop is an Illinois-damped bracketing secant capped at `FS_CORRECT_MAX` = 16 captures, and extrapolating steps are trust-clamped (`fs_trust_clamp`) so one noisy pair cannot fling the knob across its range.

### Saturated-amp response

A drive pedal feeding a saturated amp has a silent → cliff → plateau response on its own `level` knob: silent at `level ≤ 0.10`, a ~42 LU cliff by 0.20, then a ~3 LU compressed plateau (preset 024) [HW].
Targets on the cliff are reachable, but both seeds land on the plateau, so the solve must search back down through silence.

A silent capture at any non-first measurement point is data, not a failure.
The first seed keeps its routing-clamp conversion (`LevelError::NoSignal` → the `MUTE_FLOOR_SILENT_LUFS` report and "no signal on USB 1/2").
Every later silent capture (second seed, bracket-expansion probe or correction step) becomes a geometry-only pseudo point at `fs_silent_geometry(min_real)`: `FS_SILENT_GEOMETRY_LUFS` (−50), floored `FS_SILENT_MARGIN_LU` (10) below the quietest real capture of the solve, so the secant has a point to walk the cliff from.
Not −120: the literal floor overruns the `FS_CORRECT_MAX` budget, and the relative floor keeps the sentinel below every real capture on whole-curve-quiet chains (a sentinel above a real point inverts the secant's slope).
The pseudo point never reaches `improve_best`, and only a real capture may satisfy the at-target break.

The pedal-on sound sits on the amp's compression ceiling while base tracks input linearly, so their gap shrinks the harder the instrument is played, ~0.55 LU per dB on preset 024 [HW].
A one-shot solve locks in the gap the calibration capture showed; recalibrating the instrument profile (`calibrate_profile`) at the intensity actually played tightens it, because the DI capture becomes the leveling stimulus verbatim ([leveling-calibration.md#instrument-profiles](leveling-calibration.md#instrument-profiles)).

## Post-save verification

`verify_persisted_writes` / `persist_mismatches` re-read the preset after the batch save with a complete-or-fail field-8 read (`read_saved_preset_complete`) and stamp `persist_mismatch` on each result.
They detect only "my own writes did not persist": a dropped chunked edit, an idle-gap lapse or a rejection.
The footswitch verify (`verify_fs_persisted_writes`) also compares the preset's stored `presetLevel`, read at the start of the batch, with the same re-read: the batch writes no `presetLevel`, so a different value means its save reverted the base level, and every switch in the batch is stamped mismatched.

A re-read that cannot answer says UNCONFIRMED, never "did not persist".
A truncated read loses `scenes` first ([device/presets.md#reading-presets](device/presets.md#reading-presets)), so a missing section is unverifiable, not absent: a cut re-read can miss a scene whose saved state re-measures on target [HW].
The grading keeps `Unverifiable` distinct from `Absent` (`SceneOverlay::Unknown` carries the distinction into `persisted_value`), because a document can arrive whole and still carry no `scenes`.
It is not an amnesty: when the section is present and covers the slot, a genuinely absent overlay stays a miss.
`a_truncated_reread_is_unconfirmed_not_a_lost_write` and its footswitch twin assert both directions.
`--slot-json` pretty-prints a parsed value without `preserve_order`, so its key order says nothing about the wire order behind the `ftsw` scene-count oracle.

## Parallel-amp rebalance

Rebalance is opt-in per preset (Set up's "Balance its two amps first", shown only for a preset the backend marks `rebalanceable`) and applies only to a path merge: two amps in parallel that re-merge.
For each such scene, `level_scenes_rebalance` mutes one lane (`outputLevel = 0`) and measures the other to get each lane's solo ceiling `C`, sets the two `outputLevel`s for equal solo loudness (the quieter lane pinned at max, the louder attenuated), then scales both lanes' `outputLevel` by one joint factor `k` to hit the combined target.
A muted lane is digital silence, which the solo measurement treats as the `leveller::MUTE_FLOOR_SILENT_LUFS` sentinel ([device/reamp-audio.md#outputlevel-0](device/reamp-audio.md#outputlevel-0)).
If a muted lane bleeds into the solo (a small solo-above-floor margin), the scene is flagged verify by ear: the combined target is still hit, only the lane balance is approximate.
Non-mergeable scenes skip the isolation step and use plain joint-k.
A mergeable scene first takes the same as-is reading the other scene lanes open with (the prepass reading when the batch paid for it): a silent one, both lane amps saved at `outputLevel` 0, is the `"no signal on USB 1/2"` routing clamp before any lane solo, as in joint-k.
The off-branch no-authority case is unit-tested, not reproduced on hardware.

## Verify-by-ear causes

The Summary marks a row Check by ear for any of three causes (`RunItem.verifyByEar`: `"envelope" | "wet_floor" | "rebalance"` in `leveling.ts`), with a neutral badge and one sentence for its cause under the row, or one info Notice when two or more sounds share the cause (`summaryModel.ts`):

- **envelope**: the preset contains an envelope-follower effect (`effect_type: "Envelope Filter"`; expression-driven wahs excluded), whose response tracks the stimulus's attack and decay, so the synthetic clip can park it in a different regime than real playing; flagged however clean the numbers look (`ENVELOPE_BIDS` in `runEngine.ts`).
- **wet_floor**: a footswitch's wet-mix clamp is pinned at the 25% floor, not at headroom.
- **rebalance**: parallel amps balanced by approximate isolation ([Parallel-amp rebalance](#parallel-amp-rebalance)).

There is no dynamics cause: a `dynamic_spread_lu` ≥ 6 LU rule cannot fire on the shipped stimuli, and the momentary statistic that could replace it trips on gates, swells and tremolo.
`dynamic_spread_lu` stays on results as a diagnostic.
Separately, a base row whose predicted true peak (`LevelResult.true_peak_dbtp`, extrapolated from the reference capture, never re-measured) exceeds the clip threshold earns a "Loud peaks" badge and sentence (one caution Notice when two or more sounds have it).
