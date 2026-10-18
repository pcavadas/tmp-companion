# Leveling (Level tab)

Leveling drives the device in **re-amp mode** (no guitar plugged in): it plays a synthetic pickup-shaped stimulus through a preset's full DSP chain, captures the processed USB-Out, and measures integrated (gated) LUFS. The re-amp engage, latch and save rules it obeys are in `.claude/rules/danger.md`.

## Preset leveling — one-shot open-loop

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`. So measure once, solve `C`, compute the exact level. Per preset, `leveller::level_preset`:

1. `load_preset(slot)` on its own connection, settle, drop the seize.
2. fresh connect → recall base → `presetLevel` = the preset's saved level (clamped to 0.1–1; 0.5 when unknown, `base_ref_level`) → engage re-amp once → capture → solve `C`. A clipped reference capture is re-taken once at half the level; a footswitch preset (whose verify is skipped, below) adds a cross-check capture and takes the median of three when the two disagree on `C` (`measure_c_checked`).
3. fresh connect → set the final level → optional verify capture → optional save, always on a fresh connection, never the one that captured → after a save on base, a real reload (`reload_after_level_save`; `danger.md`).

`C` (loudness at level 1.0) is each preset's max reachable loudness; a target louder than `C` clamps (surfaced in the UI). The ceiling is preset/model-specific, not a fixed dB rule. A base row that cannot reach its target at `presetLevel` 1.0 can also raise the base amp's `outputLevel` (`headroom_trade::plan_level_pair`); the headroom trade and the clamp taxonomy (`ClampKind`) are documented in the `headroom_trade.rs` module doc.

**A base re-run that is already on target reports DONE, not clamped.** `level_preset_impl`'s `already_on_target` skip fires when the preset's saved `presetLevel` already lands within `ACCEPT_TOL_LU` (0.25) of target, and writes nothing.

**Base = the preset with NO footswitch engaged, by definition.** Every footswitch-owned on-off block is forced OFF, whatever its saved bypass: `doctor_force_bypass(ftsw, preset, None)` (i.e. `all_onoff_blocks` → `bypass=true`), the SAME derivation the Doctor's base sound and the `e2e_server` validation yardstick use. A preset that recalls with a pedal on reads above the base target unless that switch's own row is leveled too — deliberately, with no warning. The `ftsw` the list is built from must arrive WHOLE (`ftsw` sits at the tail, exactly where a large preset's `presetDataRequest` (field 8) read is cut, and a short list leaves pedals on and SAVES a `presetLevel` solved for the wrong sound), so the read is `read_slot_preset_complete(slot, &["ftsw"])` (backup re-read on truncation, refuse if that cannot be addressed); it also supplies the revert anchor (`presetLevel`) and the save's `restore_scene`.

Two consequences of the force list: the apply path reloads first (`reload_preset`; forced bypasses persist across HID reconnects), and the **verify capture is SKIPPED by design** — isolation would have to be written after the recall and before the engage, into the very buffer `recall_reassert_save` then saves from, so one dropped reload would save _every pedal off_ into the player's preset. The cross-check capture above stands in for it, and the UI shows `predicted_lufs`, which is exact for a post-chain linear control. A safe re-enable (reload → re-set `presetLevel` → recall → save) is a deliberate, unvalidated non-goal.

**Where the Doctor diverges — scenes only.** Scene sounds are measured as-played, riding their saved overlay states, because a scene's own bypass overrides are what define it.

**KNOWN GAP:** the block-handle base lane (`leveller::level_preset_block`, used when the player picks a specific block param as the Base handle) measures un-isolated — it takes no `force_bypass`.

## Scene leveling — one-shot open-loop on the active amp

Amp `outputLevel` is linear in dB with full authority, so each scene is leveled the same way as a preset: measure the scene as-is (isolated re-amp capture), solve `C`, set the scene's active amp `outputLevel` (HW: Base solves ±0.03 LU, scenes ±0.05 LU). Level the base scene via `presetLevel` first (it scales all scenes), then each footswitch scene via its amp `outputLevel`. A scene that can't reach the target clamps honestly.

Per-scene rules:

- The amp is picked per scene from the live audioGraph (a bypassed amp's knob measures flat → clamp).
- Amp candidate choice has TWO filters: classify amps by stable `FenderId` / `model_id`, then allow ONLY the amp `outputLevel` parameter (embedded amp schemas). `output` is not an amp control; `level` on `ACD_TMRumbleV3` is an amp knob but NOT the output leveling control — it must not be changed. Preamp/master/volume params (`brightvolume`, `mastervolume`, `normalvolume`, `volume`) are forbidden — they alter the preset's sound. HW evidence: preset 001 Klon's Hiwatt reached its target only once its actual `outputLevel` was at 100%; a first-active-param pick chases earlier volume params and falsely clamps. An amp whose preset doc omits `outputLevel` is refused as level-manually (seen on `ACD_SVTBlueLineCabIR`).
- **Scene Edit.** A scene-context write isolates from base only through the node's scene overlay, and enabling Scene Edit (`SetNodeSceneEdit`) RESEEDS that node's whole overlay from base, wiping any other scene-edited param on it. `set_knobs` decides per node from the run's ONE field-8 read (`scene_jobs::scene_overlay`, raw `scenes[]` — after `overlay_scene_onto_graph` a base value is indistinguishable from an overlaid one): a node whose overlay carries knob params is written WITHOUT the enable, and the write extends the overlay; a node with no overlay gets the enable; a bypass-only overlay (Scene Edit off, knobs shared with base) or an UNKNOWN answer (truncated or missing read) REFUSES — enabling reseeds it, and omitting the enable leaks the write to base (HW; the isolation matrix is in `probe_api/slot_write.rs`'s `--set-scene-param` doc).
- **Every pre-write idle gap stays ≤ ~300 ms**, because the session lapses 750 ms after the last host OUT report and a write to a lapsed session is dropped with no `presetError` (`danger.md`). Hence `SETTLE_AFTER_SCENE_RECALL_MS` = 150 and `SETTLE_AFTER_SCENE_EDIT_MS` = 300, the latter slept only when an enable was actually sent.
- The BASE scene's blocks have no scene mode; only CHANGE a scene's `outputLevel` when it actually needs it (pure cleanliness, not safety) — `level_scenes_oneshot` measures each scene AS-IS (`measure_scene_asis`, no write) and skips scenes already at the solved level.
- **ONE save per preset (deferred writes).** Scene writes accumulate UNSAVED in the working copy (`LevelOptions.defer`: no per-apply save, no restore) and the runner persists them with ONE `saveCurrentPreset` (field 14) at batch end (`save_deferred_scene_writes`), which first recalls the preset's original `lastLoadedScene` (`recall_original_scene`) so the save stamps the scene the preset was loaded in, then saves on a never-re-amped connection (one retry). Also fired on CANCEL so already-reported scenes persist. Under MAINTAIN CHANGES unsaved scene-edit writes survive scene recalls and reconnects, and one save persists every accumulated overlay (HW). Under DISCARD CHANGES every `loadScene` rebuilds base and every overlay from the stored preset (static RE, fw 1.8.58), so that one pre-save recall would wipe every deferred write: the deferred-write lanes (`level_scenes_apply_batched`, `level_footswitches_apply`) refuse a positively-read DISCARD before any device write (`scene_discard_guard`, read from the startup settings backup). The refusal is permanent, not a stopgap. Footswitches mirror the one save: the batched command measures every switch first (each job's force list sets every sibling's bypass explicitly), then `write_footswitch_values` commits ALL switches on ONE live-edit session with ONE save (per-write confirm gates; ANY unconfirmed write aborts before the save so nothing half-applied persists).
- `level_scenes_oneshot`/`jointk_one_scene` run a BOUNDED secant correction (`correct_iter`, cap `MEASURE_CORRECT_MAX`=3, ±`BATCH_TRUST_DB` trust region) that always lands+saves the BEST point, so `apply_levels` never persists a worse number than reported. A ≥6 dB `outputLevel` change that doesn't move the USB 1/2 capture (`no_authority`) is an honest clamp with a distinct `clamp_reason` ("off-branch / off-USB"), restoring the amp to base.
- **Dropped-write retry (`apply_first_verified`, `SUSPECT_DROP_MIN_DB`=2):** the first verified apply re-applies ONCE when a ≥2 dB intended move reads back ~unchanged (< `KNOB_TOL_LU`). A scene write can drop silently (the idle-gap rule above) and a verify capture can one-off mis-measure by LU-scale (HW: 1-in-4 captures read 2.9 LU low); one retry disambiguates, and a genuine no-authority amp stays flat on the retry and takes the honest clamp. Even 6:1 compression passes ~0.33 LU of a 2 dB move, so the signature is not compression.
- **Scene docs come from the SAVED preset.** Knob picks and per-scene knob values resolve before any engage: the batched scene run derives each scene's doc from the run's one complete field-8 read, each scene's stored overlay applied over base (`prepass_scene_docs_via` → `scene_docs_from_saved`; HW: matches the live walk). The read is complete-or-fail for `audioGraph` + `scenes` (+ `ftsw` with base in the batch), so a cut tail is re-read from a device backup or refused; a saved document that still cannot answer — no `audioGraph`, or a requested scene index outside `scenes[]` — is REFUSED with a readable reason before anything reaches the unit (`saved_scene_docs_refusal`). A `loadScene` past the preset's scenes aborts the device server (static RE, fw 1.8.58). The live walk (`prepass_scene_docs`) is probe-only. The runners do NOT re-load the preset (caller contract — the pre-pass is their preset load), and the UI batches ADJACENT same-preset scene rows sharing instrument+target into ONE `level_scenes_apply_batched` call. Together they stop the unit from flashing back to the preset's base twice per scene.

`probe --bench-scene-leveling` runs the closed-loop `level_scenes_live_batched`; a bench only, it mis-measures on its shared stream.

## Run order

Within a preset the run goes base → base-context footswitches → scenes → scene-context footswitches (`runRank`, `views/level/leveling.ts`). A footswitch's handle is often a block the preset leaves ON in base, and every scene renders its base chain, so leveling such a switch after the scenes moves every scene already on target (HW: 2.0 dB on a 3-scene preset). The dependency is one-way — a base-context switch is measured in base, so a scene's amp overlay never moves it — so ordering fixes it with no re-solve pass. A switch a scene enables (`scenes[].ftswStates`) is solved after that scene.

## Footswitch leveling — the chosen block parameter

A block-acting footswitch toggles a block on/off. Each one is leveled one at a time (`level_footswitches_apply`, jobs from `footswitchesPerIndex`) by sweeping the **user-chosen block parameter** (`levGroupId`/`levNodeId`/`levParameterId`) — a secant search in _parameter_ space (not amplitude, since the parameter is not necessarily a linear gain). The solved value is applied either **assigned** or **baked** (`plan_footswitch_jobs`): **Assign iff the switch already carries a `param` function on the selected (node, param)** — that function's `valueA` is edited. Everything else BAKES (a direct `changeParameter` on the block); leveling never creates or deletes a footswitch function. A block bypassed in base that this switch does not enable is a clamp. A Bake carries `mirror_scenes` — the scenes whose overlay RESTATES the base value (`scenes_restating_base`, `SCENE_PARAM_EPS`) — and `write_fs_values_on_session` writes the solved value into each of those overlays after the base write (one recall per scene, no Scene Edit enable since the overlay exists): a full-param overlay MASKS base (HW), so the bake lands in every uniform scene and every authored divergence survives. The write session then recalls the preset's original `lastLoadedScene` before its ONE save (the save stamps the ACTIVE scene, HW); `level_preset`'s save does the same via `LevelOptions::restore_scene`. Method is internal, never surfaced.

Each footswitch levels in **isolation**: every other block-acting footswitch's on/off block is forced off (`siblings_off_excluding`, fed by the `onoff_nodes`/`all_onoff_blocks` helpers) so sibling pedals don't color the capture, while the target switch's own block(s) get their engaged state (`engaged_bypass_for_switch` — **isActive-aware, NOT a plain flip-of-base**: an on-off is a latching toggle whose `isActive` records whether the preset was saved WITH the switch active, so engaged bypass = saved bypass when `isActive`, else the flip (HW). The Doctor's `doctor_force_bypass` shares this helper).

**Secant seeds** (`solve_param_secant`): seed 1 is the switch's current value when it has one (an on-target reading there is an idempotency skip), else 25% of the param's range. Seed 2 is the linear-law prediction when plausible (inside 5–95% of the range, expected gap ≥ `FS_MIN_SEED_GAP_LU` = 1), else the complement fraction (75% or 25%). A flat seed pair triggers `fs_bracket_expansion` toward the bound the target needs. The correction loop is an Illinois-damped bracketing secant, capped at `FS_CORRECT_MAX` = 16 captures; extrapolating steps are trust-clamped (`fs_trust_clamp`) so one noisy pair cannot fling the knob across its range.

### Saturated-amp response shape

A drive pedal feeding a saturated amp has a **silent → cliff → plateau** response on its own `level` knob: silent at `level ≤ 0.10`, a ~42 LU cliff by 0.20, then a ~3 LU compressed plateau (HW, preset 024 "TR+BD2+BMP"). Targets on the cliff are reachable, but both seeds land on the plateau, so the solve must search back DOWN through silence.

**The rule: a silent capture at any non-first measurement point is DATA, not a failure.** The FIRST seed keeps its routing-clamp conversion (`LevelError::NoSignal` → the `MUTE_FLOOR_SILENT_LUFS` report + "no signal on USB 1/2"); every LATER silent capture — second seed, bracket-expansion probe, or correction step — becomes a geometry-only pseudo point at `fs_silent_geometry(min_real)` = `FS_SILENT_GEOMETRY_LUFS` (−50) floored `FS_SILENT_MARGIN_LU` (10) below the quietest real capture of the solve, so the secant has a point to walk the cliff from. Not −120: the literal floor overruns the `FS_CORRECT_MAX` budget, and the relative floor keeps the sentinel BELOW every real capture on whole-curve-quiet chains (a sentinel above a real point inverts the secant's slope). The pseudo point never reaches `improve_best`, and only a REAL capture may satisfy the at-target break.

**Drive-dependence:** the pedal-ON sound sits on the amp's compression ceiling while base tracks input linearly, so their gap shrinks the harder the instrument is played (HW: ~0.55 LU per dB on preset 024). A one-shot solve locks in the gap the calibration capture showed; recalibrating the instrument profile (`calibrate_profile`) at the intensity actually played tightens it, because the DI capture becomes the leveling stimulus verbatim (see "Instrument profiles & calibration").

## Outcomes and acceptance

**Outcome taxonomy (preset & footswitch):** success = target hit; **clamped at X** = any headroom/convergence clamp (the knob has real, measured effect but can't reach target) — `clamped` with NO reason; **not on USB 1/2** = the capture had no signal there (user-configurable output routing) — `clamp_reason` is set ONLY in this case (`"no signal on USB 1/2"`, the silent-first-capture early return) and maps to the UI's `offbranch` outcome. **The scene lanes (joint-k, user handle and rebalance) make the same conversion:** a SILENT as-is capture becomes the `"no signal on USB 1/2"` clamp of kind `NoAuthority` (`scene_prologue_or_silent` → `no_signal_scene_solve`, shared reason `NO_SIGNAL_REASON`), writes nothing, and is never a failed row. The commonest cause is authored: a scene saved with its amp `outputLevel` at 0, which is deep digital silence on the unit (offline fixture 403 "Clean"). The ceiling prepass leaves a silent scene unmeasured (no ceiling, no trade vote), so its solve re-captures once and converts a second silent read. The scene path additionally stamps `clamp_reason` for its no-authority off-branch case with signal present (a big amp `outputLevel` change, no capture response — "route it to USB 1/2 or level it manually") — same `offbranch` label.

**One acceptance band: `ACCEPT_TOL_LU = 0.25`.** The footswitch lane (`solve_footswitch`/`classify_fs_outcome`/`switch_at_target`), the base lane's idempotency skip and verify correction, and the scene lane's at-target checks all accept within ±0.25 LU of target (≈2σ of ~0.12 LU capture noise; `fs_solve_noise_tests`). `KNOB_TOL_LU = 0.3` stays for flatness/no-authority and the block-knob lane, and the online HW strict spec (`level.online.spec.ts`) keeps its wider ±1.0 LU footswitch band, which is HW capture noise headroom, not the FS lane's own acceptance number.

**Post-save persist-verify is an HONEST, narrow contract.** `verify_persisted_writes`/`persist_mismatches` (the post-batch-save field-8 re-read, stamping `persist_mismatch` on each result) detects ONLY "my own writes didn't persist" — a dropped chunked edit, an idle-gap lapse, or a rejection. The FS-lane verify (`verify_fs_persisted_writes`) additionally checks the preset's stored `presetLevel`, read at the start of the batch, against the same read: the batch writes no `presetLevel`, so a different value means its save reverted the base level, and every switch in the batch is stamped mismatched too.

**Progress caption.** The backend streams `leveller::PREPASS_ACTIVE_MSG` as the active row's message during the ceiling prepass (both lanes emit a prepass tick per row); the run page words its rows from typed fields only (`runModel.ts`: `runRowState`, `nowLine`), never from the backend's message.

## Parallel-amp rebalance — equalize lanes, then joint-level

Opt-in per preset (Set up's "Balance its two amps first", shown only for a preset the backend marks `rebalanceable`), only on a path MERGE (two amps in parallel that re-merge). For each such scene (`level_scenes_rebalance`): mute one lane (`outputLevel = 0`) and measure the other to get each lane's solo ceiling `C`, set the two `outputLevel`s for **equal solo loudness** (quieter lane pinned at max, louder attenuated), then scale BOTH lanes' `outputLevel` by one joint factor `k` to hit the combined target. If a muted lane bleeds into the solo (small solo-above-floor margin, `probe --mute-floor`), the scene is flagged **verify by ear** — the combined target is still hit, only the lane balance is approximate. Non-mergeable scenes skip the isolation step and use the plain joint-k. A mergeable scene first takes the same as-is reading the other scene lanes open with (the prepass reading when the batch paid it): a SILENT one — both lane amps saved at `outputLevel` 0 — is the `"no signal on USB 1/2"` routing clamp before any lane solo, exactly as in joint-k.

## Verify-by-ear causes

The Summary marks a row **Check by ear** for any of THREE causes (`RunItem.verifyByEar`, `"envelope" | "wet_floor" | "rebalance"` in `leveling.ts`): a neutral badge and one sentence for its cause under the row, or one info Notice when two or more sounds share the cause (`summaryModel.ts`). `dynamic_spread_lu` stays on results as a diagnostic.

- **envelope** — the preset contains an envelope-follower effect (`effect_type: "Envelope Filter"`; expression-driven wahs excluded): its response tracks the stimulus's attack/decay envelope, so the synthetic clip can park it in a different regime than real playing — flagged regardless of how clean the numbers look (`ENVELOPE_BIDS` in `runEngine.ts`).
- **wet_floor** — a footswitch's wet-mix clamp is pinned at the 25% floor, not headroom.
- **rebalance** — parallel amps balanced by approximate isolation (mute-lane bleed; see the rebalance section).

Separate from by-ear: a Base row whose PREDICTED true peak (`LevelResult.true_peak_dbtp`, extrapolated from the reference capture — an estimate, never a re-measurement) exceeds the clip threshold earns a "Loud peaks" badge and sentence in the Summary (one caution Notice when two or more sounds have it).

## Stop / cancellation

Stop in the Run stage fires all three cancel lanes (`cancel_{preset,scene,footswitch}_leveling` — per-lane `AtomicBool`s read via a `cancelled` closure threaded into the leveller) **and** a process-global abort flag (`device_gate::OP_ABORT`, justified by `DEVICE_OP_LOCK` already serializing device work to one op at a time). A tripped flag returns `LevelError::Cancelled` (`error.rs`; it stays a cancel even when the restore after it fails), which the runner treats as a **skip**. `sleep_abortable`/`sleep_or_cancel` poll the abort flag on every long wait on the leveling/Doctor path (the re-amp capture window itself, the settles bracketing it, and both floor-retry gaps), each bailing out within one poll tick (`ABORT_POLL_MS` = 50 ms, `device_gate.rs`) — except past the re-amp engage, which has no early return (a dropped OFF strands the unit input-muted), and the post-cancel preset restore / deferred scene saves, which always run to completion. That bounds only the in-process poll bail-out, not the end-to-end Stop-to-UI latency, which is unmeasured on hardware.

Contract: levels already written before the stop STAY saved, following each item kind's persistence model: **preset** (Base, via `presetLevel`) saves per item as it completes; **scene and footswitch** writes are deferred in the working copy and persisted by the single batch-end save (`save_deferred_scene_writes` / `write_footswitch_values`), which the cancel lane also fires. The save after a Stop is verified like any other: a stopped scene batch that saved RETURNS the scenes it leveled, each carrying the save's verdict (`persist_mismatch`); a batch save that failed — on either lane, stopped or not — marks every scene/switch it wrote as not held (`leveller::report_batch_save`, `level_footswitch::mark_fs_unsaved`). That verify is the same complete-or-fail field-8 re-read (`leveller::verify_persisted_writes` → `read_saved_preset_complete`), so on a preset whose read comes back cut it takes the whole-library backup re-read, and a Stop can then take noticeably longer to land. A stopped scene batch whose save failed leaves its leveled writes unsaved in the working copy on both scene lanes: its rows say the save didn't hold, and the next load of the preset, a Try again's prepass included, discards them. The frontend merges that verdict from the command's returned rows (the streamed rows predate the save). There is no run-level rollback and no in-app revert: the backup-acknowledgment checkbox in Set up is the sole revert path (restore from the Pro Control backup). The cancelled item itself persists nothing — `restore_after_unsaved_error` reloads the stored preset to discard the dirty measurement edits. Stop asks first ("Stop leveling?", a dialog only its own buttons close). The UI then flips to `stopping` ("Stopping after <sound> in <preset>") and lands on the Summary by itself once the run has wound down; the sounds the run had not reached when it stopped read "Not reached" there, and the Summary's "Try again" covers them, with any sound whose save didn't hold.

## Instrument profiles & calibration

A profile links a real instrument to a shipped pickup topology whose stimulus WAV is re-amped when leveling a preset the profile is assigned to. Tier-2 calibration captures the dry instrument (K-weighted LUFS) — and **stores the capture as the profile's leveling stimulus** (`<app_config>/captures/<profile_id>.wav`), re-amped **verbatim** (no scaling; `calibration_lufs` is ignored when the capture resolves). Why: even the pitched-pluck stimulus mis-levels presets against a real DI by up to ~1.8 LU spread (`probe --stim-ab`; crest, spectrum and harmonic structure drive nonlinear presets, not integrated LUFS). A calibration without a capture falls back to the topology WAV scaled to the stored loudness. Consistency: `store_capture` writes temp+rename before the scalar is saved. A take within the clip severity rule (below) is stored; a worse one stores nothing and never deletes the earlier capture, and when an earlier capture exists the scalar stays too. Settings shows on each instrument whether it levels with its recording or the test signal (`get_store` fills `Profile::has_capture`); `save_profiles` unlinks captures on profile delete or topology change; an activity-ratio gate (500 ms windows, ≥50% within 30 dB of the loudest) rejects mostly-silent takes. The Doctor diagnoses a calibrated profile's DI capture against the same per-family threshold table a synthetic stimulus uses; only the fizzy flatness gate branches on it.

The dry instrument is tapped on **USB-Out 3** (`audio::DRY_INSTRUMENT_IN_CH`). `resolve_stimulus` precedence: e2e env → explicit → profile capture → topology WAV → env → default.

**The USB 3 strip.** The send defaults to PRE (fader-independent). `calibrate_profile` reads the strip (`backup_read::usb3_strip`) from the startup settings snapshot, which can be stale, so it acts asymmetrically: a MUTED strip only explains a silent take (with the on-unit fix), and POST with an off-unity fader REFUSES a take that landed — the capture would otherwise drive every later re-amp at the fader-scaled level. The app never writes the mixer.

**Why K-weighted, not flat RMS or peak.** Peak is pick-transient-dominated and barely tracks output; flat RMS under-counts bright pickups (a bright guitar that drives the preamp hard reads low in broadband RMS). K-weighting matches perceived drive and unifies the metric with the leveler's output target — loudness in, loudness out.

**Clipping — this one is a user-facing instruction.** The dry USB 3 send is the instrument ADC pair, averaged, times a fixed 3.96 (+11.95 dB) trim, saturated to 24 bit (static RE, fw 1.8.58), so hard picks from a normal passive guitar pin it while the guitar, the ADC and the preset input are not overloaded; the unit's USB 3 meter is windowed RMS and does not show it. `calibrate_profile` flags a take with a **flat-top run-length detector** (≥4 consecutive full-scale samples, `lufs::is_clipped`) — a lone pick transient at full scale is NOT clipping — and storage follows `stimulus::clip_verdict`: Mild (≤0.5% of samples at full scale, longest run ≤2 ms) is stored, Severe is not. **Play EXACTLY as you really play — NEVER roll the guitar volume back:** doing so under-drives the amp versus reality AND darkens the tone (volume-pot HF rolloff), corrupting the very brightness K-weighting exists to capture. The too-quiet case is guarded separately.

**HW-validated, both gates.** The dry instrument is capturable (`probe --capture-input`), and the re-amp inject is **not AGC'd** — its amplitude drives the chain, and the apparent compression is the amp's own nonlinearity (`probe --agc-test <clean-slot>` linearizes at low drive; a clean Twin gave ~1.65 LU output per 6 dB input). Re-validate with those two `probe` subcommands.

## Playback-level (Fletcher–Munson) compensation

Equal-LUFS is equal-loudness only near one SPL. The store carries a playback level (Quiet / Rehearsal / Stage, default Stage), and `profiles::playback_offset_lu` adds a per-family LU offset to the target: bass and Bass VI get **Quiet 0, Rehearsal −1.0, Stage −1.5 LU**; guitar is 0 everywhere.

**Why Quiet is the anchor.** K-weighting was fitted to listening tests run at about 60 dBA: ITU-R BS.1770-5, Annex 1, Attachment 1, §4.1, describes them (Benjamin 2004, AES preprint 6233; Soulodre 2004, "Evaluation of objective measures of loudness", _Canadian Acoustics_ 32(3), pp. 152–153: 97 subjects, the reference reproduced at 60 dBA). No source makes stage volume its design point. So equal LUFS is equal loudness near bedroom level, and the ISO 226 contours flatten as playback gets louder: the ear credits low-frequency energy more than K-weighting does, and a bass preset at equal LUFS sounds bigger. Bass therefore gets a **negative** offset that grows with level. The magnitudes are deliberately coarse: contours vary by listener and rig, and a bass preset's loudness shift depends on how much of its energy sits below ~200 Hz. The Doctor's threshold offsets use the same contours, derived per band (`notes/doctor-calibration.md`).

## Capture window

The capture window (≈6 s stimulus + 0.8 s tail) is load-bearing: TMP presets are not stationary under gated-integrated LUFS (delay/reverb buildup + tail), so trimming the window shifts the measured value. The leveling default uses the full capture. A steady-state window did not beat the current window against real-DI playing on 12 presets (median error 1.60 vs 1.50 LU); the stimulus spectrum moves the error ~10× more (HW, fw 1.8.58).

## Metering convention

**Companion measures the processed USB pair as standard 2-channel BS.1770** (`lufs::measure_stereo`, energy sum over capture channels 0/1 — dry channel 2+ excluded), matching ffmpeg's independent `ebur128` to within 0.009 LU. On the TMP's mirrored dual-mono USB output (`ch0 ≡ ch1`) this reads exactly **+3.0103 dB** (`10·log10(2)`) above a loudest-single-channel meter (HW: +3.010 LU). Stores saved under the single-channel convention are migrated once (`profiles::Store::meter_convention`), shifting their targets +3 LU to keep the same audible loudness; relative quantities — spreads, deltas, the `presetLevel` solve itself — are invariant to the convention. The INPUT/stimulus side (self-measurement, dry-DI calibration, the floor-guard spread) uses `lufs::measure_mono`. The dev Doctor probe arm (`probe --doctor`) reports mono-on-mixdown loudness by design (`SoundProfile::from_capture_with_psd_loudness` with `stereo_loudness: None`); the PRODUCTION Doctor path (`commands/doctor.rs` → `leveller::doctor_capture_with_loudness`) reports the stereo measure.

### Playback outputs: stereo / mono (summed) / left output only

The meter above hears both outputs as a stereo pair. A rig that plays the TMP in mono
(one cab or a mono PA fed L+R) or from output 1 alone hears a different signal whenever a
preset is genuinely stereo — a ping-pong delay, a wide reverb, a stereo split — so
Settings → Playback → **Outputs** (`profiles::Store::playback_channels`, default
`stereo`; a store without the field loads as `stereo`) picks what the leveling meter
hears:

| Mode        | Meter (`audio::metered_frame`)                                        |
| ----------- | --------------------------------------------------------------------- |
| `stereo`    | 2-ch BS.1770 power sum over USB 1/2 — the convention above, unchanged |
| `mono_sum`  | `(L+R)/2` metered as mono, **+3.0103 dB**                             |
| `left_only` | `L` metered as mono, **+3.0103 dB**                                   |

The +3.0103 dB is the same `10·log10(2)` as above, and it is applied by putting
the one signal on BOTH meter channels, so the gates run on the same shifted scale. That
makes a **dual-mono capture read bit-identically in all three modes**: the default and
every mirrored-mono preset change nothing, and targets mean the same loudness in every
mode. Only decorrelated stereo differs — uncorrelated equal-power L/R reads 3.01 dB lower
summed than in stereo, and left-only reads the left side's own level (louder than stereo
for a left-heavy preset, quieter for a right-heavy one).

The choice is process-wide (`audio::meter_channels`, synced from the store at startup and
by `set_playback_channels`) and reaches every output-side hub: `leveller::measure_processed`
(every full-capture leveling measurement) and the streaming `audio::PairMeter` behind the
live-LUFS readout and the adaptive `reamp_measure`. The external-validation WAV dump
(`validate_log`, `probe --dump-wav`) writes the pair as the meter heard it, so the ffmpeg
judge grades the same signal. **The Doctor is unaffected:** its analysis capture pins the
stereo meter (`leveller::doctor_capture_with_loudness`), and its band/PSD analysis stays on
the averaged mixdown. The input side (stimulus spread, calibration) never used this meter.
Gates: `leveller::playback_channels_meter_tests`, `profiles::tests::
playback_channels_default_stereo_and_round_trip`, `SettingsView.test.tsx` ("playback
outputs").

### Parity harness (dev-only, optional)

`scripts/meter-parity.sh` ground-truths the metering convention above against an
independent BS.1770 implementation — ffmpeg's `ebur128` filter — instead of taking
`lufs.rs`'s own word for it. It is a **developer tool, not a gate**: not wired into
`scripts/gates.sh`, never a CI dependency, and ffmpeg is never linked into or invoked
by the shipped app.

```bash
bash scripts/meter-parity.sh                 # generate deterministic synthetic
                                              # fixtures via ffmpeg lavfi and compare
bash scripts/meter-parity.sh <dir-of-wavs>   # compare an existing directory instead —
                                              # e.g. real captures from `probe --dump-wav`
```

Both arms run the SAME WAV through two independent meters — `probe --measure-wav`
(`lufs.rs`'s production output-side path: `measure_stereo` for a ≥2-ch file, `measure_mono`
for 1-ch) and ffmpeg's `-af ebur128` — and assert `|Δ integrated| ≤ 0.02 LU` per file. The
ffmpeg side is read at FULL precision from the filter's metadata
(`ebur128=metadata=1` + `ametadata=print:key=lavfi.r128.I`, last value emitted), NOT from
the end-of-run summary, which prints one decimal and would cap the comparison at ~0.1 LU
while hiding the true agreement. Agreement is **≤ 0.009 LU** across the six
synthetic fixtures and **0.004-0.006 LU** on real device captures. Two conformant BS.1770
implementations still differ slightly in how the final partial gating block is handled, so
bit-exact equality is not expected and 0.02 is a deliberately tight but achievable bar. If `ffmpeg` isn't on `PATH` the script prints a skip notice and exits 0 —
absence is not a failure.

**What a FAIL means:** `lufs.rs` disagrees with the independent BS.1770 read by more than
0.02 LU on at least one file. Since ffmpeg's `ebur128` is a mature, widely-used
implementation, treat that as a `lufs.rs` regression (channel/hop handling, most likely
the 2-ch true-peak-per-channel or the frame-vs-sample hop math this module's header
documents) first — unless the file came from a real device capture, where device-side
noise or a DC offset is also a plausible explanation.

`probe --dump-wav <dir>` is the add-on that feeds the second arm: passed alongside
`probe --measure-current`/`--measure-scene`, it also writes the captured processed pair
(never the dry channel) to a 32-bit-float WAV in `<dir>`, named from slot/scene, with no
effect on the command's behavior when the flag is absent. The filename is NOT made
unique beyond slot/scene — re-running the same slot/scene overwrites the earlier dump,
so rename a take before re-measuring if it needs to survive a follow-up run.

## Performance

Timing budget and oracles: `notes/perf.md`. A save must end its connection (`danger.md`).

## Re-amp protocol facts

The re-amp toggle, latch and engage rules, the saved-then-loaded rules and the guaranteed run-end OFF are in `danger.md`; the code sets the level and recalls the scene BEFORE each engage, one engage per capture on a fresh connection, because only the first capture after an engage is accurate.

- An OFF that never reads back OFF reaches the run result typed (`reamp_still_on`, and `reampStillOn` on a failed leveling command's error) beside the `reamp_warning` text. The two batched commands return `{ rows, reampStillOn, reampWarning }`, the verdict beside the rows, because a batch can return no rows at all: a Stop during the ceiling prepass (which engages re-amp once per sound), or a batch whose every row failed. Each row of the batch carries the flag, the ones never reached included (for the log), and the run returns one verdict (`runEngine`'s `RunVerdict`): still on if any command said so, off if at least one command read it back off, and no reading at all when no command returned one (every sound refused or skipped before reaching the unit, or each command rejected without the flag, which may have been refused before anything engaged). The Summary's restart Notice reads the flow's latest reading (`LevelFlowState.reampStillOn`), which a run overwrites only when it has one, so a clean Try again clears the Notice and a run that never reached the unit leaves it as it was.
- A silent/failed inject reads as the device's stationary OUTPUT FLOOR (`danger.md`). The production path guards every measurement (`leveller::measure_floor_guarded`/`require_live`, `FLOOR_TRIP_LU` = 0.4 on the 400 ms momentary-loudness spread, disarmed for a stimulus whose own momentary spread is under `STATIONARY_STIM_LU` = 2.0; a still-flat read after the quiet-gap retry is refused as `FLOOR_READ_ERR`). The momentary statistic separates a stimulus from the floor by ~20× (shipped stimuli 7.3–10.6 LU, Doctor slices 6.7–11.7 LU, saturated chains ≥0.67 LU, a stationary floor ~0.1 LU). There is no level-shift confirm: the floor is post-DSP and tracks `presetLevel` like a real signal. `--stim-ab` stays the diagnostic sweep.
- **"no signal captured" at DEEP silence (peak ≈ −93 dBFS)** usually means the engage never landed; an engage after > 750 ms of host OUT silence is dropped. Check with `probe --channels <slot>`: if it reads finite loudness the device is fine and the failing tool's connection shape is to blame.
