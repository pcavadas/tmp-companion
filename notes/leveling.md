# Leveling

Read this when changing the leveling model, the base lane, run order, outcomes or Stop.

Leveling drives the unit in re-amp mode (no guitar plugged in): it plays a synthetic pickup-shaped stimulus through a preset's full DSP chain, captures the processed USB-Out and measures integrated (gated) LUFS.
The device rules it obeys live in [device/reamp-audio.md](device/reamp-audio.md) (engage, capture, re-amp OFF), [device/scenes.md](device/scenes.md) (scene context, write landing, recall) and [device/presets.md#saving](device/presets.md#saving) (saves and reloads).
This note is the app logic of the model and the base lane; the scene and footswitch lanes are in [leveling-scenes.md](leveling-scenes.md), and the stimulus, playback compensation and metering in [leveling-calibration.md](leveling-calibration.md).
Facts are for firmware 1.8.58 unless a line says otherwise.

Leveling commands take the 0-based list index, while `read_library_via_backup` rows carry the 1-based device slot; a guard must sit in the mutation's address space ([device/presets.md#slot-addressing](device/presets.md#slot-addressing)).

## Preset leveling

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`.
So the lane measures once, solves `C` and computes the exact level.
Per preset, `leveller::level_preset`:

1. Loads the preset (`load_preset(slot)`) on its own connection, settles and drops the seize.
2. On a fresh connection, recalls base, sets `presetLevel` to the preset's saved level (clamped to 0.1–1; 0.5 when unknown, `base_ref_level`), engages re-amp once, captures and solves `C`.
   A clipped reference capture is re-taken once at half the level.
   A footswitch preset (whose verify is skipped, [Base definition](#base-definition)) adds a cross-check capture and takes the median of three when the two disagree on `C` (`measure_c_checked`).
3. On a fresh connection, sets the final level, takes an optional verify capture and saves, always on a connection that never captured.
   After a save with base active it does a real reload (`leveller::reload_after_level_save`), because a base recall re-applies the old level until one ([device/scenes.md#presetlevel-on-recall](device/scenes.md#presetlevel-on-recall)).

The three connections are the conservative shape the code keeps; a set after a load on one connection holds [HW] ([device/presets.md#loading](device/presets.md#loading)).

`C` (the loudness at level 1.0) is the preset's maximum reachable loudness, and a target louder than `C` clamps (surfaced in the UI).
The ceiling is preset- and model-specific, not a fixed dB rule.
To level several presets to one loudness, pick a target below the quietest preset's measured maximum.
A base row that cannot reach its target at `presetLevel` 1.0 can also raise the base amp's `outputLevel` (`headroom_trade::plan_level_pair`); the headroom trade and the clamp taxonomy (`ClampKind`) are documented in the `headroom_trade.rs` module header.

A base re-run that is already on target reports done, not clamped: `level_preset_impl`'s `already_on_target` skip fires when the saved `presetLevel` already lands within `ACCEPT_TOL_LU` (0.25) of target, and writes nothing.

## Base definition

Base is the preset with no footswitch engaged, by definition.
Every footswitch-owned on-off block is forced off whatever its saved bypass: `doctor_force_bypass(ftsw, preset, None)` (`all_onoff_blocks` → `bypass = true`), the same derivation the Doctor's base sound and the `e2e_server` validation yardstick use.
A preset that recalls with a pedal on reads above the base target unless that switch's own row is leveled too; that is deliberate, with no warning.
The `ftsw` list must arrive whole, because a short list leaves pedals on and saves a `presetLevel` solved for the wrong sound; a `presetDataRequest` (field 8) read cut by a session lapse can lose it, so a read without its final `0x35` frame is never accepted ([device/presets.md#reading-presets](device/presets.md#reading-presets)).
So the read is `read_slot_preset_complete(slot, &["ftsw"])` (a backup re-read on truncation, refused when that cannot be addressed), which also supplies the revert anchor (`presetLevel`) and the save's `restore_scene`.

The force list has two consequences:

- The apply path reloads first (`reload_preset`), because forced bypasses persist across HID reconnects.
- The verify capture is skipped by design.
  Isolation would have to be written after the recall and before the engage, into the very working copy `recall_reassert_save` then saves from, so one dropped reload would save every pedal off into the player's preset.
  The cross-check capture stands in for it, and the UI shows `predicted_lufs`, which is exact for a post-chain linear control.

The block-handle base lane (`leveller::level_preset_block`, used when the player picks a specific block parameter as the base handle) measures un-isolated: it takes no `force_bypass`.

The Doctor differs only for scenes: a scene sound is measured as played, riding its saved overlay, because a scene's own bypass overrides define it.

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
A recall writes only the footswitches' `isActive` flags, never block bypass, so a scene renders the base chain's own state for any node its overlay has no entry for [HW+RE] ([device/scenes.md#footswitch-state-on-recall](device/scenes.md#footswitch-state-on-recall)).
A footswitch's handle is often a block the preset leaves on in base, so leveling such a switch after the scenes moves every scene already on target: cutting a base-on `ACD_TubeScreamer.level` by 2.0 dB moved all three scenes of a preset by 2.0 dB, with every lane reporting `clamped: false`, `saved: true`, `persist_mismatch: false` [HW].
Base stays on target, because a base capture forces every footswitch-owned block off, which hides the drift.
The dependency is one-way (a base-context switch is measured in base, so a scene's amp overlay never moves it), so ordering fixes it with no re-solve pass.
A switch a scene enables (`scenes[].ftswStates`) is solved after that scene.
One extra re-measure of a single scene right after its own batch save, before any later lane writes, separates "mis-solved by its own batch" from "moved by a later lane".

## Outcomes and acceptance

The outcome taxonomy for preset and footswitch rows:

- **Success**: the target is hit.
- **Clamped at X**: any headroom or convergence clamp (the knob has real, measured effect but cannot reach the target); `clamped` with no reason.
- **Not on USB 1/2**: the capture had no signal there (user-configurable output routing).
  `clamp_reason` is `"no signal on USB 1/2"` (the silent-first-capture early return) and maps to the UI's `offbranch` outcome; on these rows no other case sets it.

The scene lanes (joint-k, user handle and rebalance) make the same conversion: a silent as-is capture becomes the `"no signal on USB 1/2"` clamp of kind `NoAuthority` (`scene_prologue_or_silent` → `no_signal_scene_solve`, shared reason `NO_SIGNAL_REASON`), writes nothing, and is never a failed row.
The commonest cause is authored: a scene saved with its amp `outputLevel` at 0, which is deep digital silence on the unit ([device/reamp-audio.md#outputlevel-0](device/reamp-audio.md#outputlevel-0); offline fixture 403 "Clean").
The ceiling prepass leaves a silent scene unmeasured (no ceiling, no trade vote), so its solve re-captures once and converts a second silent read.
Scene rows also carry a `clamp_reason` for a no-authority clamp with signal present (`no_authority_reason`: "route it to USB 1/2 or level it manually"), under the same `offbranch` label ([leveling-scenes.md#correction-and-dropped-writes](leveling-scenes.md#correction-and-dropped-writes)).

One acceptance band, `ACCEPT_TOL_LU` = 0.25: the footswitch lane (`solve_footswitch`, `classify_fs_outcome`, `switch_at_target`), the base lane's idempotency skip and verify correction, and the scene lane's at-target checks all accept within ±0.25 LU of target (≈ 2σ of ~0.12 LU capture noise; `fs_solve_noise_tests`).
`KNOB_TOL_LU` = 0.3 stays for flatness, no-authority and the block-knob lane.
The online strict spec (`level.online.spec.ts`) keeps a wider ±1.0 LU footswitch band, which is capture-noise headroom, not the lane's own acceptance number.

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

Footswitch isolation must be re-sent on every capture ([leveling-scenes.md#isolation](leveling-scenes.md#isolation)); sending it once is a correctness bug, not a speedup.
Levers measured and not taken:

- the adaptive early-exit capture: −3.8 s per capture but +0.17 LU on a dry preset, a re-baseline ([leveling-calibration.md#capture-window](leveling-calibration.md#capture-window));
- audio stream reuse across captures: the stream costs only ~211 ms per capture (`TMP_AUDIO_TIMING=1`);
- a wider footswitch acceptance band (0.6 LU): −15% time for a 0.50 LU worst miss;
- a smarter footswitch secant: −13% time alone in an offline replay, nothing once stacked with the short far-step windows (`footswitch::CoarseFs`) and the 50 ms command windows;
- dropping the base verify capture: a policy decision, since the floor guard covers inject failure independently.

## Re-amp run end

The code sets the level and recalls the scene before each engage, with one engage per capture on a fresh connection, because only the first capture after an engage is accurate ([device/reamp-audio.md#engage-and-capture-accuracy](device/reamp-audio.md#engage-and-capture-accuracy)).
A zero-write pre-engage shape gets a heartbeat to break the idle ([device/reamp-audio.md#idle-gap-before-an-engage](device/reamp-audio.md#idle-gap-before-an-engage)), and every measurement is floor-guarded ([device/reamp-audio.md#silent-captures](device/reamp-audio.md#silent-captures)): a still-flat read after the quiet-gap retry is refused as `FLOOR_READ_ERR`.
The guard trips on a 400 ms momentary-loudness spread at or below `FLOOR_TRIP_LU` = 0.4: a stationary floor reads ~0.1 LU, while a stimulus reads ≥ 0.67 LU even through a saturated chain (shipped stimuli 7.3–10.6 LU, Doctor slices 6.7–11.7 LU) [HW].
It is disarmed for a stimulus whose own spread is under `STATIONARY_STIM_LU` = 2.0.
Check a "no signal captured" with `probe --channels <slot>`: if it reads finite loudness, the failing tool's connection shape is to blame.

An OFF that never reads back OFF reaches the run result typed (`reamp_still_on`, and `reampStillOn` on a failed leveling command's error) beside the `reamp_warning` text.
The two batched commands return `{ rows, reampStillOn, reampWarning }`, the verdict beside the rows, because a batch can return no rows at all: a Stop during the ceiling prepass (which engages re-amp once per sound), or a batch whose every row failed.
Each row of the batch carries the flag, the ones never reached included (for the log), and the run returns one verdict (`runEngine`'s `RunVerdict`): still on if any command said so, off if at least one command read it back off, and no reading when no command returned one.
The Summary's restart Notice reads the flow's latest reading (`LevelFlowState.reampStillOn`), which a run overwrites only when it has one, so a clean Try again clears the Notice and a run that never reached the unit leaves it as it was.

## Progress caption

The backend streams `leveller::PREPASS_ACTIVE_MSG` as the active row's message during the ceiling prepass (both lanes emit a prepass tick per row); the run page words its rows from typed fields only (`runModel.ts`: `runRowState`, `nowLine`), never from the backend's message.
