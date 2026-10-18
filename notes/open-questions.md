# Open questions

Device questions whose answer would change what TMP Companion does or says.
Each entry gives the question, why it matters, what would answer it, and the code that depends on it.
When one closes, delete its entry and write the answer as a plain fact in the note that owns the subject ([`README.md`](README.md)).
Link an entry as `notes/open-questions.md#<id>`; the headings are stable ids.
Facts are for firmware 1.8.58 unless a line says otherwise.

## Scene 0 via footswitch

Does a footswitch press of scene 0 reach the same state as a USB `loadScene(0)`?
On the 2-amp Guitar preset, a USB `loadScene(0)` materializes a different amp state than the physical footswitch press [HW].
The server handles every source as the same `loadScene` message, and scene 0 takes no special path [RE].
The differences are in what the panel sends:

- A scene-switch press toggles its entry's `isActive` before executing it: with the flag clear it loads the switch's scene, with the flag set it loads base (`loadScene(8)`) [RE].
- The active scene's switch has `isActive` set only after an earlier press, or after a recall whose `ftswStates` set it; otherwise a press re-loads the scene [RE].
- A linked active scene switch is cleared and sends 8 before the pressed one sends its slot [RE, inferred].
- A touch on the already-active scene in the unit's UI sends nothing, while a USB `loadScene(0)` on an active scene 0 always runs the scene-change policy [RE, inferred].
- Every recall writes the target scene's `ftswStates` into the switches' `isActive` flags only, with no audible change at the recall ([device/scenes.md#footswitch-state-on-recall](device/scenes.md#footswitch-state-on-recall)).

- **Why it matters:** every scene-0 reading. Level and Doctor do not trust it: the Doctor's first-scene advisory ("the first scene's USB reading can differ from its footswitch"), and `fixture_gates.rs` leaves scene 0 of the 2-amp fixtures unjudged.
- **What would answer it (HW, attended, no save):** from another scene, press scene 0's switch once and read `lastLoadedScene`, the amps' bypass and the footswitch states with `currentPresetDataRequest` (field 2) on a fresh connection; press it again and check whether `lastLoadedScene` becomes 8; then send USB `loadScene(0)` from the same start and compare.
- **Code:** `doctor.rs` (`scene_zero_worst_gets_verify_by_ear_advisory`), the scene-0 exclusions in `fixture_gates.rs`, the scene-0 rule in `.claude/rules/danger.md`.

## Batched save revert

What reverted the scene a batched leveling save had just leveled?
A batched scene-leveling save stored the old value of the one scene it had just leveled when that scene was also `restore_scene`, the preset's on-load scene recalled right before the save [HW]: `probe --level-scenes 404 -17 guitar-humbucker 0 --commit` on `E2E Hiwatt 3S` (stored `lastLoadedScene` 0) solved scene 0's `outputLevel` to 1.0 and saved 0.96, twice.
The solve is correct; only the persisted value is wrong, and `persist_mismatches` flags it, so it is never silent.
Under MAINTAIN CHANGES a recall keeps the scene's unsaved edits, a re-recall of the active scene included [RE].
Two causes fit:

- the unit was set to DISCARD CHANGES, where every `loadScene` rebuilds base and every overlay from the stored preset (the probe path skips the app's `scene_discard_guard`);
- the write's overlay entry had one key, so it landed on base and the recall re-applied the overlay's old value ([Scene-write landing shapes](#scene-write-landing-shapes)).

- **Why it matters:** it decides whether the `restore_scene` recall before the deferred save is safe. A fix must cover the three save paths that share `recall_original_scene`: `apply_levels`, `save_deferred_scene_writes` and `write_fs_values_on_session`. `recall_reassert_save` re-applies `presetLevel` after the recall; nothing re-applies a per-scene block write.
- **What would answer it (HW):** on the unit that reproduced it, read Scene Change Behavior (`footswitchSettingsRequest`, Settings 47 → field 12). If it reads MAINTAIN, repeat the probe run and compare base `outputLevel` with `scenes[0]` in a `presetDataRequest` (field 8) read: under the one-key cause, base holds 1.0 next to the overlay's 0.96.
- **Code:** `leveller::recall_original_scene` (`leveller/knob.rs`), `save_deferred_scene_writes` (`leveller/scene.rs`), `leveller::persist_mismatches`, the 410 arc in `e2e/specs/level.online.spec.ts`.

## Scene-write landing shapes

Does the device decide a scene write's landing by the overlay entry's key count at every shape?
The device rule is that a `bypass` write, or any write to an overlay entry with more than one key, lands in the scene overlay, and a write to an absent or one-key entry lands on base; at recall a `{bypass}`-only overlay merges key by key, and any other overlay replaces the node's whole parameter object [RE] ([device/scenes.md#scene-write-landing](device/scenes.md#scene-write-landing)).
Two shapes are hardware-checked: a write to a multi-key overlay that lacks the parameter extends that overlay, with its siblings kept and base untouched [HW]; a write against a `{bypass}`-only overlay lands on base [HW].
Open: `{bypass, bypassType}`, a one-key non-bypass overlay such as `{outputLevel}`, a partial multi-key overlay, and the merge-or-replace behaviour at recall.
The app's classifier differs at exactly those shapes: `scene_jobs::scene_overlay` tests for the bypass family (`BYPASS_ONLY_KEYS`), not the key count, so `{bypass, bypassType}` classifies `BypassOnly` and `{outputLevel}` classifies `Full`, and `overlay_scene_onto_graph` shallow-merges.

- **Why it matters:** a misclassified overlay sends a leveling write to the wrong side (scene vs base), and offline graphs (SimDevice, the Doctor's per-sound graphs) render the wrong parameters.
- **What would answer it (HW, scratch slot, no save):** craft `{bypass}`, `{bypass, bypassType}`, `{outputLevel}` and a partial multi-key overlay; `loadScene(N)`, send a `changeParameter`, read the working copy with `currentPresetDataRequest` (field 2); then recall from a scene where the omitted parameter differs and read again.
- **Code:** `scene_jobs.rs` (`scene_overlay`, `scene_write_verdict_for_param`, `overlay_scene_onto_graph`), the SimDevice overlay model (`sim_device.rs`), `doctor::effective_nodes`.

## Re-amp across a USB unplug

Does a stuck re-amp survive a USB unplug, and does a restart of the unit clear it?
Static evidence says re-amp stays on across an unplug and a restart clears it [RE] ([device/reamp-audio.md#stuck-re-amp](device/reamp-audio.md#stuck-re-amp)).

- **Why it matters:** the user-facing recovery text. The app tells the user to restart the unit when a run could not read re-amp back OFF.
- **What would answer it (HW, attended):** engage re-amp, unplug and replug, play the guitar and read Mixer `reAmpModeRequest` (4) → field 9; then restart the unit and read again.
- **Code:** the `reamp_still_on` flag and the "Restart your Tone Master Pro" Notice (`leveller::ReampRunGuard`, `error.rs`), the stuck re-amp rule in `.claude/rules/danger.md`.

## Instrument pad and the dry clamp

Does the −6 dB instrument pad keep hard picks under full scale on the dry USB 3 send?
Hard picks pin the dry USB 3 send at full scale while USB 1/2 and the unit's meter show no clipping [HW]: the send is the averaged instrument ADC pair times a fixed +11.95 dB trim, saturated to 24 bit [RE].
The pad sits on the analog input, ahead of that send.

- **Why it matters:** it is the on-unit fix a refused calibration take could offer.
- **What would answer it (HW):** hard picks with the pad off and on; compare USB 3 peaks and flat-top runs (`probe --capture-wav`).
- **Code:** `stimulus::clip_verdict` and the refused-take message.

## Doctor recipes on the shipped stimulus

Do the Doctor's defect recipes still hit, and its localized-cut prescription still work, on the shipped pitched-pluck stimulus?
The peak-Q estimator and the parametric-cut prescription for `resonant` / `boxy` have not run on the unit with that stimulus and the current constants.

- **Why it matters:** the Doctor's `resonant` / `boxy` verdicts and the fix it prescribes.
- **What would answer it (HW):** `probe --doctor-defects <slot>` (all 7 recipes must hit) and an empty-slot capture (no finding) on the current build; an AFTER capture of the `resonant_peq` / `boxy_peq` recipes with the prescribed cut entered by hand (peak reduced, no octave-wide dip).
- **Code:** `doctor.rs` (`RESONANT_MIN_Q`, `RESONANT_MAX_Q`, `localized_cut_rx`); [doctor.md#localized-rules](doctor.md#localized-rules).

## Capture-space Doctor thresholds

Are the Doctor's thresholds valid on a calibrated DI capture?
A calibrated profile's DI capture is diagnosed against the same per-family threshold table as the synthetic stimulus.
Subtracting the stimulus's band shape is exact on a clean chain and over-corrects a driven one, which reads a spurious `lost` [HW].

- **Why it matters:** every Doctor run on a calibrated instrument.
- **What would answer it (HW, attended):** a DI sweep over clean and driven presets with `probe --doctor-calib` ([doctor.md#calibrated-di-captures](doctor.md#calibrated-di-captures)), deriving capture-space thresholds from it.
- **Code:** `Family::thresholds()` in `doctor.rs`.

## Confirm latency after a load

How late do the save reply and the structural-edit confirms arrive after a load?

- **Why it matters:** it sizes the confirm windows. A confirm still queued behind the load's pushes reads as missing and sends the edit into read-back recovery ([copy.md#read-back](copy.md#read-back)).
- **What would answer it (HW):** a recorded trace (`TMP_TRACE_DIR`, [e2e-recording.md](e2e-recording.md)) of a load followed by `replaceNode`, `insertNode` (field 34) and `saveCurrentPreset` (field 14).
- **Code:** `commands/copy_apply.rs` (`apply_copy_op`), `Session::insert_landed`.

## Save then load on one connection

Does a `saveCurrentPreset` chained before a `loadPreset` on the same connection land?
The save chain is synchronous: `saveCurrentPreset` commits the stored row before its handler returns, on the one thread that processes messages in order, so a load queued behind it runs after it [RE].
A hardware test on firmware older than 1.8.58 (`probe --save-load-test`) saw the load land and the save dropped, with no `presetError`; it has not been re-run on fw 1.8.58.
The code keeps the conservative shape: a save ends its connection, and no load follows it on the same one ([device/presets.md#saving](device/presets.md#saving)).

- **Why it matters:** a load sharing the save's connection would save a reconnect on every leveled and copied preset.
- **What would answer it (HW):** `probe --save-load-test`.
- **Code:** every save path (`leveller`, `commands/copy_apply.rs`, `replace_inplace.rs`).

## USB 1/2 pre-flight

Which USB 1/2 mixer states should a capture refuse?
The leveller and the Doctor measure USB 1/2, a full mixer channel with its own fader, mute and solo, plus AUX and Bluetooth injection.
`settingsBackup.mixerSaveData.usb12` carries the strip, and the app does not decode it.
The only measurements predate fw 1.8.58: a muted `usb12`, or another channel soloed while `usb12` is not, captures silence; a solo elsewhere does not set `usb12`'s own `soloActive`; a fader off unity offsets every capture by a constant that solve-then-verify cannot see; AUX or Bluetooth injection mixes outside audio into the capture.
The master knob is not in the USB 1/2 path ([device/reamp-audio.md#output-routing](device/reamp-audio.md#output-routing)).

- **Why it matters:** a silent capture is blamed on a dropped re-amp engage, and an off-unity fader levels every preset wrong with no error.
- **What would answer it (HW):** each state in turn (mute, solo another channel, fader at −6 dB, AUX with a source) with a capture and a fresh settings backup.
- **Code:** `backup_read::usb3_strip` is the model for a `usb12` pre-flight.

## Mic/Line templates and loops 1/2

Can the Companion measure presets the instrument stimulus does not reach?

- **Mic/Line templates.** The stimulus goes only to USB-In 3, the instrument channel (`REAMP_INSTRUMENT_OUT_CH`); re-amp USB-In 4 feeds the mic/line channel. A preset on a `Mic/Line Series`, `Mic/Line Parallel 1` or `Mic/Line Split` template captures silence. Open: does a stimulus on USB-In 4 reach those chains?
- **Loops 1 and 2.** They are relay loops switched by the preset's top-level `fxLoop` object (`fxLoop1Enabled` / `fxLoop1Bypassed`, …), not audio-graph nodes, and re-amp bypasses them [RE]; their position ahead of the instrument A/D is [RE, inferred]. Gear in loop 1 or 2 is missing from every capture. Open: does a re-amp capture of a preset with loop 1 engaged and a pedal in it equal the same preset with the loop off?

- **Why it matters:** both cases read as "no signal", or as a confident number for a chain that was never fully measured.
- **What would answer it (HW):** a capture with a stimulus on USB-In 4 through a Mic/Line template; captures of one preset with loop 1 engaged (a pedal in it) and off.
- **Code:** both cases are detectable offline from the backup scan (`micNodes` vs `guitarNodes` in `audiograph.rs`; the `fxLoop` object), and nothing detects them.
