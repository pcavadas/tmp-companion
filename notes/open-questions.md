# Open questions

Device questions whose answer would change what TMP Companion does or says.
Each entry gives the question, why it matters, what would answer it, and the code that depends on it.
When one closes, delete its entry and write the answer as a plain fact in the note that owns the subject ([`README.md`](README.md)).
Link an entry as `notes/open-questions.md#<id>`; the headings are stable ids.
Facts are for firmware 1.8.58 unless a line says otherwise.

## Scene 0 via footswitch

Does a footswitch press of scene 0 reach the same state as a USB `loadScene(0)`?
On the 2-amp Guitar preset, a USB `loadScene(0)` materializes a different amp state than the physical footswitch press [HW].
The server handles every source as the same `loadScene` message, and scene 0 takes no special path; the candidates are in what the panel sends ([device/scenes.md#panel-and-footswitch-scene-changes](device/scenes.md#panel-and-footswitch-scene-changes)) [RE].

- **Why it matters:** every scene-0 reading.
  Level and Doctor do not trust it: the Doctor's first-scene advisory ("the first scene's USB reading can differ from its footswitch"), and `fixture_gates.rs` leaves scene 0 of the 2-amp fixtures unjudged.
- **What would answer it (HW, attended, no save):** from another scene, press scene 0's switch once and read `lastLoadedScene`, the amps' bypass and the footswitch states with `currentPresetDataRequest` (field 2) on a fresh connection; press it again and check whether `lastLoadedScene` becomes 8; then send USB `loadScene(0)` from the same start and compare.
- **Code:** `doctor.rs` (`scene_zero_worst_gets_verify_by_ear_advisory`), the scene-0 exclusions in `fixture_gates.rs`, the scene-0 rule in `.claude/rules/danger.md`.

## Batched save revert

What reverted the scene a batched leveling save had just leveled?
The symptom is in [device/scenes.md#batched-scene-save](device/scenes.md#batched-scene-save): `probe --level-scenes 404 -17 guitar-humbucker 0 --commit` on `E2E Hiwatt 3S` (stored `lastLoadedScene` 0) solved scene 0's `outputLevel` to 1.0 and saved 0.96, in both runs [HW].
The solve is correct; only the persisted value is wrong, and `leveller::persist_mismatches` flags it.
Under MAINTAIN CHANGES a recall keeps the scene's unsaved edits, a re-recall of the active scene included [RE].
Two causes fit:

- the unit was set to DISCARD CHANGES, where every `loadScene` rebuilds base and every overlay from the stored preset (the probe path skips the app's `scene_discard_guard`);
- the write's overlay entry had one key, so it landed on base and the recall re-applied the overlay's old value ([Scene-write landing shapes](#scene-write-landing-shapes)).

- **Why it matters:** it decides whether the `restore_scene` recall before the deferred save is safe.
  A fix must cover the three save paths that share `recall_original_scene`: `apply_levels`, `save_deferred_scene_writes` and `write_fs_values_on_session`.
  `recall_reassert_save` re-applies `presetLevel` after the recall; nothing re-applies a per-scene block write.
- **What would answer it (HW):** on the unit that reproduced it, read Scene Change Behavior (`footswitchSettingsRequest`, Settings 47 → field 12).
  If it reads MAINTAIN, repeat the probe run and compare base `outputLevel` with `scenes[0]` in a `presetDataRequest` (field 8) read: under the one-key cause, base holds 1.0 next to the overlay's 0.96.
- **Code:** `leveller::recall_original_scene` (`leveller/knob.rs`), `save_deferred_scene_writes` (`leveller/scene.rs`), `leveller::persist_mismatches`, the 410 arc in `e2e/specs/level.online.spec.ts`.

## Scene-write landing shapes

Does the device decide a scene write's landing by the overlay entry's key count at every shape?
The device rule is that a `bypass` write, or any write to an overlay entry with more than one key, lands in the scene overlay, and a write to an absent or one-key entry lands on base; at recall a `{bypass}`-only overlay merges key by key, and any other overlay replaces the node's whole parameter object [RE] ([device/scenes.md#scene-write-landing](device/scenes.md#scene-write-landing)).
Two shapes are hardware-checked: a write to a multi-key overlay that lacks the parameter extends that overlay, with its siblings kept and base untouched [HW]; a write against a `{bypass}`-only overlay lands on base [HW].
Open: where a write lands on `{bypass, bypassType}` and on a one-key non-bypass overlay such as `{outputLevel}`, and what the unit renders at recall for a parameter a multi-key overlay omits.
The app's classifier differs from the device rule at exactly those shapes ([device/scenes.md#the-apps-classifier](device/scenes.md#the-apps-classifier)).

- **Why it matters:** a misclassified overlay sends a leveling write to the wrong side (scene vs base), and offline graphs (SimDevice, the Doctor's per-sound graphs) render the wrong parameters.
- **What would answer it (HW, scratch slot, no save):** craft `{bypass}`, `{bypass, bypassType}`, `{outputLevel}` and a partial multi-key overlay; `loadScene(N)`, send a `changeParameter`, read the working copy with `currentPresetDataRequest` (field 2); then recall from a scene where the omitted parameter differs and read again.
- **Code:** `scene_jobs.rs` (`scene_overlay`, `scene_write_verdict_for_param`, `overlay_scene_onto_graph`), the SimDevice overlay model (`sim_device.rs`), `doctor::effective_nodes`.

## Re-amp across a USB unplug

Does a stuck re-amp survive a USB unplug, and does a restart of the unit clear it?
Static evidence says re-amp stays on across an unplug and a restart clears it [RE] ([device/reamp-audio.md#stuck-re-amp](device/reamp-audio.md#stuck-re-amp)).

- **Why it matters:** the user-facing recovery text.
  The app tells the user to restart the unit when a run could not read re-amp back OFF.
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

Do the Doctor's defect recipes hit, and does its localized-cut prescription work, on the shipped pitched-pluck stimulus?
The peak-Q estimator and the parametric-cut prescription for `resonant` / `boxy` have not run on the unit with that stimulus and the current constants.

- **Why it matters:** the Doctor's `resonant` / `boxy` verdicts and the fix it prescribes.
- **What would answer it (HW):** `probe --doctor-defects <slot>` (all 7 recipes must hit) and an empty-slot capture (no finding) on the current build; an AFTER capture of the `resonant_peq` / `boxy_peq` recipes with the prescribed cut entered by hand (peak reduced, no octave-wide dip).
- **Code:** `doctor.rs` (`RESONANT_MIN_Q`, `RESONANT_MAX_Q`, `localized_cut_rx`); [doctor-thresholds.md#localized-rule-gates](doctor-thresholds.md#localized-rule-gates).

## Capture-space Doctor thresholds

Are the Doctor's thresholds valid on a calibrated DI capture?
A calibrated profile's DI capture is diagnosed against the same per-family threshold table as the synthetic stimulus.
Subtracting the stimulus's band shape is exact on a clean chain and over-corrects a driven one, which reads a spurious `lost` [HW].

- **Why it matters:** every Doctor run on a calibrated instrument.
- **What would answer it (HW, attended):** a DI sweep over clean and driven presets with `probe --doctor-calib` ([doctor-thresholds.md#calibrated-di-captures](doctor-thresholds.md#calibrated-di-captures)), deriving capture-space thresholds from it.
- **Code:** `Family::thresholds()` in `doctor.rs`.

## Confirm latency after a load

How late do the save reply and the structural-edit confirms arrive after a load?
Static analysis predicts a confirm queues in the one HID reply FIFO behind the load's full push set, so it arrives after the load's `sceneLoaded` (102) [RE, inferred]; the field-3 push alone arrives 0.1–0.45 s after a load [HW].

- **Why it matters:** it sizes the confirm windows.
  A confirm still queued behind the load's pushes reads as missing and sends the edit into read-back recovery ([copy.md#read-back](copy.md#read-back)).
- **What would answer it (HW):** a recorded trace (`TMP_TRACE_DIR`, [e2e-recording.md](e2e-recording.md)) of a load followed by `replaceNode`, `insertNode` (field 34) and `saveCurrentPreset` (field 14).
- **Code:** `commands/copy_apply.rs` (`apply_copy_op`), `Session::insert_landed`.

## Save then load on one connection

Does a `saveCurrentPreset` chained before a `loadPreset` on the same connection land?
The save chain is synchronous: `saveCurrentPreset` commits the stored row before its handler returns, on the one thread that processes messages in order, so a load queued behind it runs after it [RE].
On older firmware `probe --save-load-test` landed the load and dropped the save with no `presetError` [HW]; it has not run on fw 1.8.58.
Until it does, a save ends its connection and no load follows it on the same one ([device/presets.md#save-then-load-on-one-connection](device/presets.md#save-then-load-on-one-connection)).

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

- **Mic/Line templates.** In re-amp, USB-In 4 replaces Input 2 ([device/reamp-audio.md#re-amp-toggle](device/reamp-audio.md#re-amp-toggle)) [RE], and the app drives only USB-In 3 (`REAMP_INSTRUMENT_OUT_CH`), so a preset on a `Mic/Line Series`, `Mic/Line Parallel 1` or `Mic/Line Split` template gets no stimulus [RE, inferred].
  Open: does a stimulus on USB-In 4 reach and measure those chains?
- **Loops 1 and 2.** They are relay loops switched by the preset's top-level `fxLoop` object (`fxLoop1Enabled` / `fxLoop1Bypassed`, …), not audio-graph nodes, and re-amp bypasses them [RE]; their position ahead of the instrument A/D is [RE, inferred].
  Gear in loop 1 or 2 is missing from every capture.
  Open: does a re-amp capture of a preset with loop 1 engaged and a pedal in it equal the same preset with the loop off?

- **Why it matters:** both cases read as "no signal", or as a confident number for a chain that was never fully measured.
- **What would answer it (HW):** a capture with a stimulus on USB-In 4 through a Mic/Line template; captures of one preset with loop 1 engaged (a pedal in it) and off.
- **Code:** both cases are detectable offline from the backup scan (`micNodes` vs `guitarNodes` in `audiograph.rs`; the `fxLoop` object), and nothing detects them.

## Stale load echo

Why can the first connection after a load echo the slot loaded before it?
A fresh connection opened ~0.3–1 s after `load_and_prove`'s load got a `presetLoaded` (field 11) echo naming the previous slot, although the load had landed; a connection 16 s later named the new slot [HW] ([device/presets.md#echo-of-the-previous-slot](device/presets.md#echo-of-the-previous-slot)).
Two causes fit: the load is still running when the next connection handshakes, or the handshake echo reports state the unit has not updated yet.

- **Why it matters:** it sizes the retry in `Session::connect_confirmed` and decides whether a wait after the load could replace the lapse-and-retry.
- **What would answer it (HW, scratch slot, no save):** a timing sweep of load → lapse → reconnect at several gaps, logging the echo and the field-3 `info.displayName` each time.
- **Code:** `Session::connect_confirmed`, `Session::active_matches`, `replace_inplace::load_and_prove`.

## Cabinet cap weights

Which blocks does fw 1.8.58's cabinet cap count as 2?
The unit's UI counts combos, half-stacks, bass amps, cabinets and IR blocks, exempts heads and `ACD_ExternalCab`, and adds 1 for a node with a per-node flag set [RE], likely its second cab or second mic [RE, likely].
`blockcaps.rs` counts `ACD_ExternalCab` and weights 2 only a standalone `ACD_CabSimTMS` with `cabsim2enabled` ([device/presets.md#block-caps](device/presets.md#block-caps)).

- **Why it matters:** `blockcaps.rs` is the only cap enforcement, so a wrong weight lets a third cab through or refuses a legal edit.
- **What would answer it (HW, Pro Control closed, no save):** on the unit's own screen, try to add a second cab block to a preset holding an amp with its second mic on, and to one holding `ACD_ExternalCab` and one cab block; note which add the UI refuses.
- **Code:** `blockcaps.rs` (`Counts`, `roster_from_preset`), `src/models/block-classification.json`, `validateBlockEdit.ts`.
