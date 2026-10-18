# Leveling

Leveling drives the unit in re-amp mode (no guitar plugged in): it plays a synthetic pickup-shaped stimulus through a preset's full DSP chain, captures the processed USB-Out and measures integrated (gated) LUFS.
The device rules it obeys live in [device/reamp-audio.md](device/reamp-audio.md) (engage, capture, re-amp OFF), [device/scenes.md](device/scenes.md) (scene context, write landing, recall) and [device/presets.md#saving](device/presets.md#saving) (saves and reloads).
This note is the app logic: what each lane measures, writes and saves, and why.
Facts are for firmware 1.8.58 unless a line says otherwise.

Leveling commands take the 0-based list index, while `read_library_via_backup` rows carry the 1-based device slot; a guard must sit in the mutation's address space ([device/presets.md#slot-addressing](device/presets.md#slot-addressing)).

## Preset leveling

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`.
So the lane measures once, solves `C` and computes the exact level.
Per preset, `leveller::level_preset`:

1. Loads the preset (`load_preset(slot)`) on its own connection, settles and drops the seize.
2. On a fresh connection, recalls base, sets `presetLevel` to the preset's saved level (clamped to 0.1–1; 0.5 when unknown, `base_ref_level`), engages re-amp once, captures and solves `C`. A clipped reference capture is re-taken once at half the level. A footswitch preset (whose verify is skipped, [Base definition](#base-definition)) adds a cross-check capture and takes the median of three when the two disagree on `C` (`measure_c_checked`).
3. On a fresh connection, sets the final level, takes an optional verify capture and saves, always on a connection that never captured. After a save with base active it does a real reload (`leveller::reload_after_level_save`), because a base recall re-applies the old level until one ([device/scenes.md#presetlevel-on-recall](device/scenes.md#presetlevel-on-recall)).

The three connections are the conservative shape the code keeps; on fw 1.8.58 a set after a load on one connection holds [HW] ([device/reamp-audio.md#engage-and-capture-accuracy](device/reamp-audio.md#engage-and-capture-accuracy)).

`C` (the loudness at level 1.0) is the preset's maximum reachable loudness, and a target louder than `C` clamps (surfaced in the UI).
The ceiling is preset- and model-specific, not a fixed dB rule.
A base row that cannot reach its target at `presetLevel` 1.0 can also raise the base amp's `outputLevel` (`headroom_trade::plan_level_pair`); the headroom trade and the clamp taxonomy (`ClampKind`) are documented in the `headroom_trade.rs` module header.

A base re-run that is already on target reports done, not clamped: `level_preset_impl`'s `already_on_target` skip fires when the saved `presetLevel` already lands within `ACCEPT_TOL_LU` (0.25) of target, and writes nothing.

## Base definition

Base is the preset with no footswitch engaged, by definition.
Every footswitch-owned on-off block is forced off whatever its saved bypass: `doctor_force_bypass(ftsw, preset, None)` (`all_onoff_blocks` → `bypass = true`), the same derivation the Doctor's base sound and the `e2e_server` validation yardstick use.
A preset that recalls with a pedal on reads above the base target unless that switch's own row is leveled too; that is deliberate, with no warning.
The `ftsw` list must arrive whole, because a short list leaves pedals on and saves a `presetLevel` solved for the wrong sound; `ftsw` sits at the tail, exactly where a large preset's `presetDataRequest` (field 8) read is cut.
So the read is `read_slot_preset_complete(slot, &["ftsw"])` (a backup re-read on truncation, refused when that cannot be addressed), which also supplies the revert anchor (`presetLevel`) and the save's `restore_scene`.

The force list has two consequences:

- The apply path reloads first (`reload_preset`), because forced bypasses persist across HID reconnects.
- The verify capture is skipped by design. Isolation would have to be written after the recall and before the engage, into the very working copy `recall_reassert_save` then saves from, so one dropped reload would save every pedal off into the player's preset. The cross-check capture stands in for it, and the UI shows `predicted_lufs`, which is exact for a post-chain linear control.

The block-handle base lane (`leveller::level_preset_block`, used when the player picks a specific block parameter as the base handle) measures un-isolated: it takes no `force_bypass`.

The Doctor differs only for scenes: a scene sound is measured as played, riding its saved overlay, because a scene's own bypass overrides define it.

## Scene leveling

Amp `outputLevel` is linear in dB with full authority, so each scene is leveled like a preset: measure the scene as-is (an isolated re-amp capture), solve `C`, set the scene's active amp `outputLevel`; base solves land within ±0.03 LU and scenes within ±0.05 LU [HW].
Base is leveled first through `presetLevel`, which scales every scene; each scene then moves only its own amp's `outputLevel`.
The app command `level_scenes_apply_batched` runs `leveller::level_scenes_oneshot`: measure each scene as-is, `solve_level`, then `apply_level`, a bounded correction that always lands the best point.
A scene that cannot reach the target clamps honestly ("clamped at <LUFS>").
Every base-context write or measure recalls base (`session::BASE_SCENE_SLOT`) first, at the top of its connection, because a preset loads into its saved `lastLoadedScene` ([device/scenes.md#scene-context](device/scenes.md#scene-context)); `capture_full_at`, `set_knob`, `set_knobs`, `write_footswitch_values` and `commands/doctor.rs`'s `apply_ops_under_scene` all do.

### Amp choice

The amp is picked per scene from the scene's graph (a bypassed amp's knob measures flat and clamps).
Two filters choose the parameter: amps are classified by stable `FenderId` / `model_id`, then only the amp `outputLevel` parameter is allowed (embedded amp schemas).
`output` is not an amp control, and `level` on `ACD_TMRumbleV3` is an amp knob but not the output leveling control, so it is never changed.
Preamp, master and volume parameters (`brightvolume`, `mastervolume`, `normalvolume`, `volume`) are forbidden: they alter the preset's sound.
A first-active-parameter pick chases those earlier volume params and falsely clamps: preset 001's Hiwatt reached its target only once its own `outputLevel` was at 100% [HW].
An amp whose preset document omits `outputLevel` is refused as level-manually (seen on `ACD_SVTBlueLineCabIR`).
Amp ids carrying merged cab / IR / convolution suffixes are matched check-first, then stripped (`is_amp_model_id`, `resolveDeviceId`), so an amp+cab combo is still detected (`.claude/rules/models-catalog.md`).

### Scene Edit and write landing

A scene-context write stays out of base only through the node's scene overlay, and enabling Scene Edit (`setNodeSceneEdit`) reseeds that node's whole overlay from base, wiping any other scene-edited parameter on it.
So the enable is sent only for a node with no overlay in the target scene, where the reseed has nothing to lose.
`set_knobs` decides per node from the run's one field-8 read, classified by `scene_jobs::scene_overlay` over the raw `scenes[]` (after `overlay_scene_onto_graph` a base value is indistinguishable from an overlaid one), through `scene_write_verdict_for_param`:

- `Full` (the overlay carries a key outside the bypass family, `BYPASS_ONLY_KEYS`): write without the enable; the write lands in the overlay and extends it.
- `Absent` (no overlay entry): send the enable, then write.
- `BypassOnly` (the overlay carries only bypass-family keys, so the node's knobs are shared with base): refuse, because enabling reseeds the overlay and omitting the enable leaks the write to base. The one exception is a write that would be audible only in this scene (`shared_write_is_scene_local`), which is written to base directly.
- `Unknown` (a cut or missing read): refuse, since either guess can corrupt the preset.

The device decides the landing by the overlay entry's key count, not by the bypass family, so the two differ at edge shapes: a one-key `{outputLevel}` overlay classifies `Full`, but its write lands on base [RE] ([device/scenes.md#scene-write-landing](device/scenes.md#scene-write-landing), [open](open-questions.md#scene-write-landing-shapes)).
The isolation matrix the classifier was built against is in `probe_api/slot_write.rs`'s `--set-scene-param` doc.

### Write timing

Every pre-write idle gap stays at 300 ms or less (`SETTLE_AFTER_SCENE_RECALL_MS` 150, plus `SETTLE_AFTER_SCENE_EDIT_MS` 300 only when an enable was sent), because a write to a lapsed session is dropped with no `presetError` ([device/session.md#session-lapse](device/session.md#session-lapse)).

### Scene documents

Knob picks and per-scene knob values resolve before any engage.
The batched scene run derives each scene's document from the run's one complete field-8 read, each scene's stored overlay applied over base (`prepass_scene_docs_via` → `scene_docs_from_saved`), which matches the live walk [HW].
The read is complete-or-fail for `audioGraph` and `scenes` (and `ftsw` with base in the batch), so a cut tail is re-read from a device backup or refused.
A saved document that still cannot answer (no `audioGraph`, or a requested scene index outside `scenes[]`) is refused with a readable reason before anything reaches the unit (`saved_scene_docs_refusal`).
That bound matters: a `loadScene` on a preset with no scenes substitutes the empty template and answers `presetError` 6 [RE], and a `loadScene` index past a preset's scenes likely aborts the device server [RE, likely] ([device/scenes.md#scene-index-bounds](device/scenes.md#scene-index-bounds)).
The live walk (`prepass_scene_docs`) is probe-only.

`classify_scene_knobs` refuses a scene whose document answers for no amp at all, rather than classifying it against the base graph: a live field-3 document cut before the amp nodes once drove the base amp in a scene whose own overlay swaps to another [HW].
The refusal is repaired from the saved preset through `read_slot_preset_complete`'s backup-backed read (`audioGraph` + `scenes`); a scene the saved preset cannot answer either is skipped and reported, never leveled against base.
Gates: `a_truncated_swap_scene_levels_its_own_amp_not_the_base_one`, `a_scene_whose_doc_never_arrived_skips_instead_of_classifying_against_base`, `a_scene_whose_doc_is_partial_skips_instead_of_classifying_against_base`, `a_scene_whose_saved_overlay_is_unclassifiable_skips_instead_of_losing_its_headroom_silently`.

For the scene already active (the saved `lastLoadedScene`), the prepass reads the post-load document instead of recalling it, because a recall that changes nothing pushes no field 3 and would leave that scene unclassifiable [RE].
The prepass is also the runners' preset load: `level_scenes_oneshot` and `level_scenes_rebalance` never re-load the preset, and the UI batches adjacent same-preset scene rows that share instrument and target into one `level_scenes_apply_batched` call.
Together they stop the unit from flashing back to the preset's base twice per scene.

### One save per preset

Scene writes accumulate unsaved in the working copy (`LevelOptions.defer`: no per-apply save, no restore), and the runner persists them with one `saveCurrentPreset` (field 14) at batch end (`save_deferred_scene_writes`).
That save first recalls the preset's original `lastLoadedScene` (`recall_original_scene`), so it stamps the scene the preset was loaded in, then saves on a connection that never re-amped, with one retry.
It also fires on cancel, so already-reported scenes persist.
Under MAINTAIN CHANGES unsaved scene-edit writes survive scene recalls and reconnects, and one save persists every accumulated overlay [HW+RE].
Under DISCARD CHANGES every `loadScene` rebuilds base and every overlay from the stored preset, so that one pre-save recall would wipe every deferred write [RE] ([device/scenes.md#scene-change-behavior](device/scenes.md#scene-change-behavior)).
The deferred-write lanes (`level_scenes_apply_batched`, `level_footswitches_apply`) therefore refuse a positively read DISCARD before any device write (`scene_discard_guard`, read from the startup settings backup).
Never add a preset load before `save_deferred_scene_writes`: it would be a real reload that wipes the deferred writes ([device/presets.md#save-then-load-on-one-connection](device/presets.md#save-then-load-on-one-connection)).
A batched save can still store the old value of the one scene it leveled when that scene is also `restore_scene`; `persist_mismatches` catches it ([device/scenes.md#batched-scene-save](device/scenes.md#batched-scene-save), [open](open-questions.md#batched-save-revert)).

### Correction and dropped writes

`level_scenes_oneshot` and `jointk_one_scene` run a bounded secant correction (`correct_iter`, capped at `MEASURE_CORRECT_MAX` = 3, inside a ±`BATCH_TRUST_DB` trust region) that always lands and saves the best point, so `apply_levels` never persists a worse number than it reports.
A scene already at the solved level is not written (`measure_scene_asis` takes the as-is reading with no write).
A ≥ 6 dB `outputLevel` change (`NO_AUTHORITY_MIN_DB`) that does not move the USB 1/2 capture by `KNOB_TOL_LU` is an honest `no_authority` clamp with its own `clamp_reason` ("off-branch / off-USB"), and the amp is restored to base.
The first applied move is whatever the open-loop solve asked for, so a scene 4 dB from target gets a 4 dB move and a zero-authority knob would fall through to a reason-less headroom clamp.
`correct_iter` therefore takes one confirmation capture when the first move produced a flat response: it steps down a full `NO_AUTHORITY_MIN_DB` and looks again (down only, because a raise can clip and a saturating limiter makes an upward non-response ambiguous); if that probe moves the capture, it seeds the secant.
A healthy knob moves ~1 LU per dB, so this never fires on a working solve (`level-trade.spec.ts`'s zero-authority taxonomy spec).

The first verified apply re-applies once when a ≥ 2 dB intended move (`SUSPECT_DROP_MIN_DB`) reads back nearly unchanged (< `KNOB_TOL_LU`) (`apply_first_verified`).
A scene write can drop silently ([Write timing](#write-timing)) and a verify capture can mis-measure once by several LU (one read 2.9 LU low) [HW]; one retry disambiguates, and a genuine no-authority amp stays flat on the retry and takes the honest clamp.
Even 6:1 compression passes ~0.33 LU of a 2 dB move, so the signature is not compression.
The scene verify capture stays: it is also the correction-pass trigger and the leak-to-base detector.

`probe --bench-scene-leveling` runs the closed-loop `level_scenes_live_batched`; it is a bench only, and it mis-measures on its shared stream.

## Capture level

Every capture re-asserts the run's own `presetLevel` after its recall, so a level the run holds unsaved (a landed headroom trade's raise) survives a base recall ([device/scenes.md#presetlevel-on-recall](device/scenes.md#presetlevel-on-recall)); with no trade the re-assert writes the saved level, read from the batch's field-8 document.
Gated offline by `a_capture_renders_at_the_run_level_not_the_recall_reverted_one`, against SimDevice's recall revert, which applies once the slot has been saved in the run.

The prepass and the solve are coupled: `prepass_scene_ceilings` produces the as-is reading that `correct_iter` takes as its `measured0`, and compares it with a post-apply capture.
If the two render at different `presetLevel`s, the first "response" is the level difference rather than the knob's, and the solve reads authority where there is none (offline, 9.9 dB of such response on fixture 403 swamped `no_authority`'s 0.3 LU tolerance and turned a routing clamp into a reason-less headroom clamp).
Both sides go through one seam, `leveller::scene_capture_level`; never re-inline either.
The prepass runs before the headroom trade decides anything, so it passes `hold = None` and gets the saved level, while the solve passes the real hold; the two agree whenever no trade lands.
The scene lane's choice has its own pure-logic gate (`a_scene_batch_captures_at_the_saved_level_when_no_trade_holds_one`), because SimDevice reverts a recall's level only for a slot the run has already saved, and the scene specs level slots they never saved first.

## Run order

Within a preset the run goes base → base-context footswitches → scenes → scene-context footswitches (`runRank`, `views/level/leveling.ts`).
A recall writes only the footswitches' `isActive` flags, never block bypass, so a scene renders the base chain's own state for any block its overlay does not carry ([device/scenes.md#footswitch-state-on-recall](device/scenes.md#footswitch-state-on-recall)).
A footswitch's handle is often a block the preset leaves on in base, so leveling such a switch after the scenes moves every scene already on target: cutting a base-on `ACD_TubeScreamer.level` by 2.0 dB moved all three scenes of a preset by 2.0 dB, with every lane reporting `clamped: false`, `saved: true`, `persist_mismatch: false` [HW].
Base stays on target, because a base capture forces every footswitch-owned block off, which hides the drift.
The dependency is one-way (a base-context switch is measured in base, so a scene's amp overlay never moves it), so ordering fixes it with no re-solve pass.
A switch a scene enables (`scenes[].ftswStates`) is solved after that scene.
One extra re-measure of a single scene right after its own batch save, before any later lane writes, separates "mis-solved by its own batch" from "moved by a later lane".

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
The isolation list is re-sent on every capture (`leveller::arm_measurement`), because every capture recalls base first and a recall re-asserts the scene's own bypass state; sending it once measured every later point non-isolated (gate: `solve_footswitch_resends_the_isolation_list_on_every_capture`).

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
A one-shot solve locks in the gap the calibration capture showed; recalibrating the instrument profile (`calibrate_profile`) at the intensity actually played tightens it, because the DI capture becomes the leveling stimulus verbatim ([Instrument profiles](#instrument-profiles)).

## Outcomes and acceptance

The outcome taxonomy for preset and footswitch rows:

- **Success**: the target is hit.
- **Clamped at X**: any headroom or convergence clamp (the knob has real, measured effect but cannot reach the target); `clamped` with no reason.
- **Not on USB 1/2**: the capture had no signal there (user-configurable output routing). `clamp_reason` is set only in this case (`"no signal on USB 1/2"`, the silent-first-capture early return) and maps to the UI's `offbranch` outcome.

The scene lanes (joint-k, user handle and rebalance) make the same conversion: a silent as-is capture becomes the `"no signal on USB 1/2"` clamp of kind `NoAuthority` (`scene_prologue_or_silent` → `no_signal_scene_solve`, shared reason `NO_SIGNAL_REASON`), writes nothing, and is never a failed row.
The commonest cause is authored: a scene saved with its amp `outputLevel` at 0, which is deep digital silence on the unit ([device/reamp-audio.md#silent-captures](device/reamp-audio.md#silent-captures); offline fixture 403 "Clean").
The ceiling prepass leaves a silent scene unmeasured (no ceiling, no trade vote), so its solve re-captures once and converts a second silent read.
The scene path also stamps `clamp_reason` for its no-authority off-branch case with signal present ("route it to USB 1/2 or level it manually"), under the same `offbranch` label.

One acceptance band, `ACCEPT_TOL_LU` = 0.25: the footswitch lane (`solve_footswitch`, `classify_fs_outcome`, `switch_at_target`), the base lane's idempotency skip and verify correction, and the scene lane's at-target checks all accept within ±0.25 LU of target (≈ 2σ of ~0.12 LU capture noise; `fs_solve_noise_tests`).
`KNOB_TOL_LU` = 0.3 stays for flatness, no-authority and the block-knob lane.
The online strict spec (`level.online.spec.ts`) keeps a wider ±1.0 LU footswitch band, which is capture-noise headroom, not the lane's own acceptance number.

## Post-save verification

`verify_persisted_writes` / `persist_mismatches` re-read the preset after the batch save with a complete-or-fail field-8 read (`read_saved_preset_complete`) and stamp `persist_mismatch` on each result.
They detect only "my own writes did not persist": a dropped chunked edit, an idle-gap lapse or a rejection.
The footswitch verify (`verify_fs_persisted_writes`) also compares the preset's stored `presetLevel`, read at the start of the batch, with the same re-read: the batch writes no `presetLevel`, so a different value means its save reverted the base level, and every switch in the batch is stamped mismatched.

A re-read that cannot answer says UNCONFIRMED, never "did not persist".
A truncated read loses `scenes` first, so a missing section is unverifiable, not absent; a false "did not persist" once reported a scene as lost whose re-measure of the saved state read on target [HW].
The grading keeps `Unverifiable` distinct from `Absent` (`SceneOverlay::Unknown` carries the distinction into `persisted_value`), because a document can arrive whole and still carry no `scenes`.
It is not an amnesty: when the section is present and covers the slot, a genuinely absent overlay stays a miss.
`a_truncated_reread_is_unconfirmed_not_a_lost_write` and its footswitch twin assert both directions.
`ftsw` sorts ahead of `scenes` in a stored row's keys [RE], so it can survive the cut that takes `scenes` and works as a scene-count oracle; `--slot-json` pretty-prints a parsed value without `preserve_order`, so its key order says nothing about wire order.

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

`dynamic_spread_lu` stays on results as a diagnostic.
Separately, a base row whose predicted true peak (`LevelResult.true_peak_dbtp`, extrapolated from the reference capture, never re-measured) exceeds the clip threshold earns a "Loud peaks" badge and sentence (one caution Notice when two or more sounds have it).

## Stop and cancellation

Stop in the Run stage fires all three cancel lanes (`cancel_{preset,scene,footswitch}_leveling`: per-lane `AtomicBool`s read through a `cancelled` closure threaded into the leveller) and a process-global abort flag (`device_gate::OP_ABORT`, sound because `DEVICE_OP_LOCK` already serializes device work to one op at a time).
A tripped flag returns `LevelError::Cancelled` (`error.rs`; it stays a cancel even when the restore after it fails), which the runner treats as a skip.
`sleep_abortable` / `sleep_or_cancel` poll the abort flag on every long wait on the leveling and Doctor path (the capture window, the settles around it, both floor-retry gaps), each bailing out within one poll tick (`ABORT_POLL_MS` = 50 ms).
Two things never bail early: anything past the re-amp engage, because a dropped OFF strands the unit input-muted ([device/reamp-audio.md#stuck-re-amp](device/reamp-audio.md#stuck-re-amp)), and the post-cancel preset restore and deferred saves, which always run to completion.
That bounds only the in-process bail-out; the end-to-end Stop-to-UI latency is unmeasured on hardware.

Levels written before the stop stay saved, following each item kind's persistence model: a base row saves `presetLevel` as it completes; scene and footswitch writes are deferred and persisted by the single batch-end save (`save_deferred_scene_writes` / `write_footswitch_values`), which the cancel lane also fires.
The save after a Stop is verified like any other: a stopped scene batch that saved returns the scenes it leveled, each with the save's `persist_mismatch` verdict.
A batch save that failed, stopped or not, marks every scene or switch it wrote as not held (`leveller::report_batch_save`, `level_footswitch::mark_fs_unsaved`).
That verify is the same complete-or-fail re-read, so on a preset whose read comes back cut it takes the whole-library backup re-read, and the Stop can take noticeably longer to land.
A stopped scene batch whose save failed leaves its writes unsaved in the working copy: its rows say the save did not hold, and the next load of the preset (a Try again's prepass included) discards them.
The frontend merges that verdict from the command's returned rows, since the streamed rows predate the save.
The cancelled item itself persists nothing: `restore_after_unsaved_error` reloads the stored preset to discard the measurement edits (through `leveller::restore_saved_preset`, which loads a neighbour slot first).
There is no run-level rollback and no in-app revert; the Pro Control backup acknowledged in Set up is the only way back.
Stop asks first ("Stop leveling?", a dialog only its own buttons close); the UI then shows "Stopping after <sound> in <preset>" and lands on the Summary by itself once the run has wound down.
The sounds the run had not reached read "Not reached" there, and the Summary's "Try again" covers them along with any sound whose save did not hold.

## Instrument profiles

A profile links a real instrument to a shipped pickup topology ([pickup-topologies.md](pickup-topologies.md)) whose stimulus WAV is re-amped when leveling a preset the profile is assigned to.
Tier-2 calibration captures the dry instrument (K-weighted LUFS) and stores the capture as the profile's leveling stimulus (`<app_config>/captures/<profile_id>.wav`), re-amped verbatim with no scaling (`calibration_lufs` is ignored when the capture resolves).
Even the pitched-pluck stimulus mis-levels presets against a real DI by up to ~1.8 LU spread, because crest, spectrum and harmonic structure drive nonlinear presets, not integrated LUFS (`probe --stim-ab`) [HW].
A calibration without a capture falls back to the topology WAV scaled to the stored loudness.
`store_capture` writes temp + rename before the scalar is saved.
A take within the clip severity rule is stored; a worse one stores nothing and never deletes the earlier capture, and when an earlier capture exists the scalar stays too.
Settings shows on each instrument whether it levels with its recording or the test signal (`get_store` fills `Profile::has_capture`), and `save_profiles` unlinks captures on profile delete or topology change.
An activity-ratio gate (500 ms windows, ≥ 50% within 30 dB of the loudest) rejects mostly silent takes.

The dry instrument is tapped on USB-Out 3 (`audio::DRY_INSTRUMENT_IN_CH`).
`resolve_stimulus`'s precedence is: e2e env → explicit → profile capture → topology WAV → env → default.

**The USB 3 strip.** The send defaults to PRE (fader-independent).
`calibrate_profile` reads the strip (`backup_read::usb3_strip`) from the startup settings snapshot, which can be stale, so it acts asymmetrically: a muted strip only explains a silent take (with the on-unit fix), and POST with an off-unity fader refuses a take that landed, which would otherwise drive every later re-amp at the fader-scaled level.
The app never writes the mixer.

**K-weighted.** Peak is pick-transient-dominated and barely tracks output; flat RMS under-counts bright pickups (a bright guitar that drives the preamp hard reads low in broadband RMS).
K-weighting matches perceived drive and puts input and output on one metric.

**Clipping is a user-facing instruction.** The dry USB 3 send is the instrument ADC pair, averaged, times a fixed 3.96 (+11.95 dB) trim, saturated to 24 bit [RE], so hard picks from a normal passive guitar pin it while the guitar, the ADC and the preset input are not overloaded; the unit's USB 3 meter is windowed RMS and does not show it ([open](open-questions.md#instrument-pad-and-the-dry-clamp)).
`calibrate_profile` flags a take with a flat-top run-length detector (≥ 4 consecutive full-scale samples, `lufs::is_clipped`; a lone transient at full scale is not clipping), and storage follows `stimulus::clip_verdict`: Mild (≤ 0.5% of samples at full scale, longest run ≤ 2 ms) is stored, Severe is not.
Play exactly as you really play and never roll the guitar volume back: that under-drives the amp and darkens the tone (volume-pot HF rolloff), corrupting the brightness K-weighting exists to capture.
The too-quiet case is guarded separately.

The dry instrument is capturable (`probe --capture-input`), and the re-amp inject is not AGC'd: its amplitude drives the chain, and the apparent compression is the amp's own nonlinearity (`probe --agc-test <clean-slot>` linearizes at low drive; a clean Twin gave ~1.65 LU output per 6 dB input) [HW].
Re-validate with those two probe arms.

## Playback-level compensation

Equal LUFS is equal loudness only near one SPL.
The store carries a playback level (Quiet / Rehearsal / Stage, default Stage), and `profiles::playback_offset_lu` adds a per-family LU offset to the target: bass and Bass VI get Quiet 0, Rehearsal −1.0, Stage −1.5 LU; guitar is 0 everywhere.

Quiet is the anchor because K-weighting was fitted to listening tests run at about 60 dBA: ITU-R BS.1770-5, Annex 1, Attachment 1, §4.1 describes them (Benjamin 2004, AES preprint 6233; Soulodre 2004, "Evaluation of objective measures of loudness", _Canadian Acoustics_ 32(3), pp. 152–153: 97 subjects, the reference reproduced at 60 dBA).
No source makes stage volume its design point.
So equal LUFS is equal loudness near bedroom level, and the ISO 226 contours flatten as playback gets louder: the ear credits low-frequency energy more than K-weighting does, and a bass preset at equal LUFS sounds bigger.
Bass therefore gets a negative offset that grows with level.
The magnitudes are deliberately coarse: contours vary by listener and rig, and a bass preset's loudness shift depends on how much of its energy sits below ~200 Hz.
The Doctor's threshold offsets use the same contours, derived per band ([doctor.md#playback-level](doctor.md#playback-level)).

## Capture window

The leveling default is the full capture (`audio::MeasureOpts::full`), because the window is load-bearing ([device/reamp-audio.md#capture-window](device/reamp-audio.md#capture-window)).
The adaptive early-exit capture (`audio::MeasureOpts::adaptive`) is opt-in only; adopting it is a re-baseline and a product decision, not a drop-in speedup.
A steady-state window did not beat the current window against real-DI playing on 12 presets (median error 1.60 vs 1.50 LU); the stimulus spectrum moves the error ~10× more [HW].
Conclude a bias only from a many-sample A/B against the full-capture oracle ([Re-validating a timing change](#re-validating-a-timing-change)).

## Metering convention

Companion measures the processed USB pair as standard 2-channel BS.1770 (`lufs::measure_stereo`, an energy sum over capture channels 0 and 1, the dry channel 2+ excluded), matching ffmpeg's independent `ebur128` within 0.009 LU.
On the unit's mirrored dual-mono USB output (`ch0 ≡ ch1`) this reads exactly +3.0103 dB (`10·log10(2)`) above a loudest-single-channel meter (measured +3.010 LU) [HW].
Stores saved under the single-channel convention are migrated once (`profiles::Store::meter_convention`), shifting their targets +3 LU to keep the same audible loudness; relative quantities (spreads, deltas, the `presetLevel` solve) are invariant to the convention.
The input side (self-measurement, dry-DI calibration, the floor-guard spread) uses `lufs::measure_mono`.
The dev Doctor probe arm (`probe --doctor`) reports mono-on-mixdown loudness (`SoundProfile::from_capture_with_psd_loudness` with `stereo_loudness: None`); the production Doctor path (`commands/doctor.rs` → `leveller::doctor_capture_with_loudness`) reports the stereo measure.

### Playback outputs

A rig that plays the unit in mono (one cab, or a mono PA fed L+R) or from output 1 alone hears a different signal whenever a preset is genuinely stereo (a ping-pong delay, a wide reverb, a stereo split).
Settings → Playback → Outputs (`profiles::Store::playback_channels`, default `stereo`; a store without the field loads as `stereo`) picks what the leveling meter hears (`audio::metered_frame`):

- `stereo`: the 2-channel BS.1770 power sum over USB 1/2, as in [Metering convention](#metering-convention).
- `mono_sum`: `(L+R)/2` metered as mono, +3.0103 dB.
- `left_only`: `L` metered as mono, +3.0103 dB.

The +3.0103 dB is applied by putting the one signal on both meter channels, so the gates run on the same shifted scale and a dual-mono capture reads bit-identically in all three modes: the default and every mirrored-mono preset change nothing, and targets mean the same loudness in every mode.
Only decorrelated stereo differs: uncorrelated equal-power L/R reads 3.01 dB lower summed than in stereo, and left-only reads the left side's own level.
The choice is process-wide (`audio::meter_channels`, synced from the store at startup and by `set_playback_channels`) and reaches every output-side hub: `leveller::measure_processed` (every full-capture leveling measurement) and the streaming `audio::PairMeter` behind the live-LUFS readout and the adaptive `reamp_measure`.
The external-validation WAV dump (`validate_log`, `probe --dump-wav`) writes the pair as the meter heard it, so the ffmpeg judge grades the same signal.
The Doctor is unaffected: its analysis capture pins the stereo meter (`leveller::doctor_capture_with_loudness`), and its band and PSD analysis stays on the averaged mixdown.
Gates: `leveller::playback_channels_meter_tests`, `profiles::tests::playback_channels_default_stereo_and_round_trip`, `SettingsView.test.tsx` ("playback outputs").

### Parity harness

`scripts/meter-parity.sh` checks the metering convention against an independent BS.1770 implementation, ffmpeg's `ebur128` filter.
It is a developer tool, not a gate: not wired into `scripts/gates.sh`, never a CI dependency, and ffmpeg is never linked into or invoked by the shipped app.

```bash
bash scripts/meter-parity.sh                 # generate deterministic synthetic fixtures via ffmpeg lavfi and compare
bash scripts/meter-parity.sh <dir-of-wavs>   # compare an existing directory, e.g. real captures from `probe --dump-wav`
```

Both arms run the same WAV through two meters, `probe --measure-wav` (`lufs.rs`'s production output-side path: `measure_stereo` for a ≥ 2-channel file, `measure_mono` for 1 channel) and ffmpeg's `-af ebur128`, and assert `|Δ integrated| ≤ 0.02 LU` per file.
The ffmpeg side is read at full precision from the filter's metadata (`ebur128=metadata=1` + `ametadata=print:key=lavfi.r128.I`, the last value emitted), not from the end-of-run summary, which prints one decimal.
Agreement is ≤ 0.009 LU across the six synthetic fixtures and 0.004–0.006 LU on real captures; two conformant implementations differ slightly in how they handle the final partial gating block, so 0.02 is a tight but achievable bar.
Without `ffmpeg` on `PATH` the script prints a skip notice and exits 0.
A FAIL means `lufs.rs` disagrees with ffmpeg by more than 0.02 LU on at least one file; treat it as a `lufs.rs` regression first (channel or hop handling, most likely the 2-channel true-peak-per-channel or the frame-vs-sample hop math its header documents), unless the file is a real capture, where device noise or a DC offset is also plausible.

`probe --dump-wav <dir>`, passed alongside `probe --measure-current` or `--measure-scene`, also writes the captured processed pair (never the dry channel) to a 32-bit-float WAV in `<dir>`, named from slot and scene, and changes nothing else.
Re-running the same slot and scene overwrites the earlier dump.

## Re-validating a timing change

The waits around a capture (`SETTLE_AFTER_LOAD_MS` = 200, `recall_settle_ms` 150, `pre_engage_settle_ms` 100, `settle_after_reamp_ms` 100, one breaker heartbeat) are HW-bisected defaults: every capture within 0.01 LU of longer waits, identical iteration counts, every ffmpeg-judge row passing, no floor reads [HW].
Each has a diagnostic env override (`TMP_SETTLE_AFTER_LOAD_MS`, `TMP_SETTLE_AFTER_REAMP_MS` and friends in `leveller/mod.rs`) for A/B runs, and a shorter default lands only with that evidence.
`SETTLE_BEFORE_WRITE_MS` stays 600 on purpose: on the no-verify write path a dropped write is maximally silent.

Run the oracles after any timing change, with the leveling stimulus pinned:

```bash
cd src-tauri && export TMP_LEVELLER_STIMULUS=$PWD/resources/samples/guitar-humbucker.wav
cargo run --bin probe -- --levelpreset <dry-slot> -30         # verify error ≤ 0.05 LU
cargo run --bin probe -- --levelpreset <wet-slot> -30         # verify error ≤ 0.12 LU (wet class)
cargo run --bin probe -- --doctor <dry-slot> guitar-humbucker # verdicts and balance unchanged
cargo run --bin probe -- --level-footswitch <slot> <switch> <group> <node> <param> -30  # engaged reading unchanged
```

`--levelpreset` saves only with `save`, and `--level-footswitch` only with `--commit`.
Validate any measurement change against the full-capture oracle (`probe --measure-adaptive`, `--measure-prefix-sweep`), never a level → verify round trip that uses the same method: that is self-consistent and hides the offset.
Compare absolute scene LUFS within one session, not across power cycles.

Footswitch isolation must be re-sent on every capture ([Isolation](#isolation)); sending it once was a correctness bug, not a speedup.
Levers measured and not taken:

- the adaptive early-exit capture: −3.8 s per capture but +0.17 LU on a dry preset, a re-baseline ([Capture window](#capture-window));
- audio stream reuse across captures: the stream costs only ~211 ms per capture (`TMP_AUDIO_TIMING=1`);
- a wider footswitch acceptance band (0.6 LU): −15% time for a 0.50 LU worst miss;
- dropping the base verify capture: a policy decision, since the floor guard covers inject failure independently.

## Re-amp run end

The code sets the level and recalls the scene before each engage, with one engage per capture on a fresh connection, because only the first capture after an engage is accurate ([device/reamp-audio.md#engage-and-capture-accuracy](device/reamp-audio.md#engage-and-capture-accuracy)).
A zero-write pre-engage shape gets a heartbeat to break the idle ([device/reamp-audio.md#idle-gap-before-an-engage](device/reamp-audio.md#idle-gap-before-an-engage)), and every measurement is floor-guarded ([device/reamp-audio.md#silent-captures](device/reamp-audio.md#silent-captures)): a still-flat read after the quiet-gap retry is refused as `FLOOR_READ_ERR`.
The guard's momentary statistic separates a stimulus from the floor by ~20× (shipped stimuli 7.3–10.6 LU, Doctor slices 6.7–11.7 LU, saturated chains ≥ 0.67 LU, a stationary floor ~0.1 LU), disarmed for a stimulus whose own spread is under `STATIONARY_STIM_LU` = 2.0.
Check a "no signal captured" with `probe --channels <slot>`: if it reads finite loudness, the failing tool's connection shape is to blame.

An OFF that never reads back OFF reaches the run result typed (`reamp_still_on`, and `reampStillOn` on a failed leveling command's error) beside the `reamp_warning` text.
The two batched commands return `{ rows, reampStillOn, reampWarning }`, the verdict beside the rows, because a batch can return no rows at all: a Stop during the ceiling prepass (which engages re-amp once per sound), or a batch whose every row failed.
Each row of the batch carries the flag, the ones never reached included (for the log), and the run returns one verdict (`runEngine`'s `RunVerdict`): still on if any command said so, off if at least one command read it back off, and no reading when no command returned one.
The Summary's restart Notice reads the flow's latest reading (`LevelFlowState.reampStillOn`), which a run overwrites only when it has one, so a clean Try again clears the Notice and a run that never reached the unit leaves it as it was.

## Progress caption

The backend streams `leveller::PREPASS_ACTIVE_MSG` as the active row's message during the ceiling prepass (both lanes emit a prepass tick per row); the run page words its rows from typed fields only (`runModel.ts`: `runRowState`, `nowLine`), never from the backend's message.
