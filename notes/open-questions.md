# Open questions

Device questions still open whose answer would change what TMP Companion does or says. Each entry
names the question, why it matters to the Companion, what would answer it, and the code that
depends on it. When one closes, delete its entry and write the answer as a plain fact in the doc
that owns the subject (`.claude/rules/danger.md`, `gotchas.md`, `protocol.md`, `leveling.md`,
`doctor.md`, …).

---

## 1. Does a footswitch press of scene 0 reach the same state as USB `loadScene(0)`?

On the 2-amp Guitar preset, a USB `loadScene(0)` materialises a different amp state than the
physical footswitch press. The device handles both as the same `loadScene` message (static RE, fw
1.8.58), so the difference is in what the panel sends. Narrowed candidates (static RE, fw 1.8.58):
a scene footswitch whose `isActive` flag is already set sends `loadScene(8)` (base), and a touch on
the already-active scene sends nothing. A read-only check could not settle it: it needs presses on
the unit.

- **Why it matters:** every scene-0 reading. Level and Doctor do not trust it: Doctor's
  first-scene advisory in `doctor.rs` ("the first scene's USB reading can differ from its
  footswitch"), and `fixture_gates.rs` leaves scene 0 of the 2-amp fixtures unjudged.
- **What would answer it (HW, attended):** from another scene, press scene 0's switch once and read
  `lastLoadedScene`, the amps' bypass and the footswitch states on a fresh connection; press it
  again and check `lastLoadedScene` → 8; then send USB `loadScene(0)` from the same start and
  compare.
- **Code:** `doctor.rs` (`scene_zero_worst_gets_verify_by_ear_advisory`), the scene-0 exclusions in
  `fixture_gates.rs`, the OPEN item in `danger.md`.

## 2. What reverted the scene a batched leveling save had just leveled?

A batched scene-leveling save reverted the ONE scene it had just leveled when that scene was also
`restore_scene`, the preset's on-load scene recalled right before the save (HW, fw 1.8.58, twice;
`probe --level-scenes` on `E2E Hiwatt 3S` scene 0, flagged by `persist_mismatches`). Under
MAINTAIN CHANGES a recall keeps the scene's unsaved edits (static RE, fw 1.8.58), so the narrowed
candidates are: a unit set to DISCARD CHANGES (the probe path skips the app's
`scene_discard_guard`), or a write that landed on base (see question 3). A read-only check could
not settle it: it needs the reproducing run.

- **Why it matters:** decides whether the `restore_scene` recall before the deferred save is safe,
  and how `danger.md` words the rule.
- **What would answer it (HW):** on the unit that reproduced it, read Scene Change Behavior
  (`footswitchSettingsRequest`, Settings 47 → field 12). If it reads MAINTAIN, repeat
  `probe --level-scenes 404 <target> <topology> 0 --commit` and compare base `outputLevel` with
  `scenes[0]` in a `presetDataRequest` (field 8) read.
- **Code:** `leveller::recall_original_scene` and `save_deferred_scene_writes`
  (`leveller/scene.rs`), `leveller::persist_mismatches`, `e2e/specs/level.online.spec.ts`'s 410 arc.

## 3. Does the device decide a scene write's landing by the overlay entry's size?

Static RE (fw 1.8.58): a bypass write, or any write to an overlay entry with more than one key,
lands in the scene overlay; a write to an absent or one-key entry lands on base. At recall a
`{bypass}`-only overlay merges key by key, and any other overlay REPLACES the node's whole parameter
object. The code disagrees at edge shapes: `scene_jobs.rs` classifies an overlay by its
bypass-family keys (`BYPASS_ONLY_KEYS`, `scene_overlay`, `scene_write_verdict_for_param`), and
`overlay_scene_onto_graph` shallow-merges. Not HW-checked.

- **Why it matters:** a misclassified overlay sends a leveling write to the wrong side (scene vs
  base), and offline graphs (SimDevice, Doctor's per-sound graphs) render the wrong parameters.
- **What would answer it (HW, scratch slot, no save):** craft `{bypass}`, `{bypass, bypassType}`,
  `{outputLevel}` and a partial multi-key overlay; `loadScene(N)`, send a `changeParameter`, read
  the working copy with `currentPresetDataRequest` (field 2); then recall from a scene where the
  omitted parameter differs and read again.
- **Code:** `scene_jobs.rs`, the SimDevice overlay model (`sim_device.rs`),
  `doctor::effective_nodes`.

## 4. Does a stuck re-amp survive a USB unplug, and does a restart clear it?

Static evidence (fw 1.8.58) says re-amp stays on across an unplug and a restart of the unit clears
it. Not HW-checked.

- **Why it matters:** the user-facing recovery text. The app tells the user to restart the unit
  when a run could not read re-amp back OFF.
- **What would answer it (HW, attended):** engage re-amp, unplug and replug, play the guitar and
  read Mixer `reAmpModeRequest` (4) → field 9; then restart the unit and read again.
- **Code:** the `reamp_still_on` flag and the "Restart your Tone Master Pro" Notice
  (`leveller::ReampRunGuard`, `error.rs`), the recovery line in `danger.md`.

## 5. Does the −6 dB instrument pad keep hard picks under full scale on the dry USB 3 send?

Hard picks pin the dry USB 3 send at full scale while USB 1/2 and the unit's meter show no clipping
(HW, fw 1.8.58). The pad sits on the analog input, ahead of that send.

- **Why it matters:** it is the on-unit fix a refused calibration take could offer.
- **What would answer it (HW):** hard picks with the pad off and on; compare USB 3 peaks and
  flat-top runs (`probe --capture-wav`).
- **Code:** `stimulus::clip_verdict` and the refused-take message.

## 6. Does the Doctor's defects suite still hold on the shipped stimulus?

The peak-Q estimator and the parametric-cut prescription for `resonant` / `boxy` were never run on
the unit with the pitched-pluck stimulus.

- **Why it matters:** the Doctor's verdicts and its prescribed fix.
- **What would answer it (HW):** `probe --doctor-defects <slot>` (all 7 recipes must hit) and an
  empty-slot capture (no finding) on the current build; an AFTER capture of the `resonant_peq` /
  `boxy_peq` recipes with the prescribed cut entered by hand (peak reduced, no octave-wide dip).
- **Code:** `doctor.rs` (`RESONANT_MIN_Q` / `RESONANT_MAX_Q`, the parametric-cut prescription);
  `doctor.md`.

## 7. Are the Doctor's thresholds valid on a calibrated DI capture?

A calibrated profile's DI capture is diagnosed against the same per-family threshold table as the
synthetic stimulus. The only sweep so far was unattended and exploratory: exact on a clean chain,
over-corrected on driven ones.

- **Why it matters:** every Doctor run on a calibrated instrument.
- **What would answer it (HW, attended):** a DI sweep over clean and driven presets
  (`probe --doctor-calib`), deriving the capture-space thresholds from it.
- **Code:** `Family::thresholds()` in `doctor.rs`; `doctor-calibration.md`.

## 8. How late do the save reply and the structural-edit confirms arrive after a load?

- **Why it matters:** sizes the confirm windows. A confirm still queued behind the load's pushes
  reads as missing and sends the edit into read-back recovery.
- **What would answer it (HW):** a recorded trace (`TMP_TRACE_DIR`, see `.claude/rules/e2e.md`) of a
  load followed by `replaceNode`, `insertNode` (field 34) and `saveCurrentPreset` (field 14).
- **Code:** `commands/copy_apply.rs` (`apply_copy_op`), `Session::insert_landed`.

## 9. Does a save chained before a load on one connection land on fw 1.8.58?

Untested on fw 1.8.58. The code keeps the conservative shape: a save ends its connection, and no
load follows it on the same one (`danger.md`).

- **Why it matters:** a load sharing the save's connection would save a reconnect on every
  leveled and copied preset.
- **What would answer it (HW):** `probe --save-load-test` (`notes/perf.md`).
- **Code:** every save path (`leveller`, `commands/copy_apply.rs`, `replace_inplace.rs`).

## 10. Which USB 1/2 mixer states should a capture refuse?

The leveller and the Doctor measure USB 1/2, a full mixer channel with its own fader, mute and solo,
plus AUX and Bluetooth injection. `settingsBackup.mixerSaveData.usb12` carries the strip, and the
app does not read it. The only measurements predate fw 1.8.58: a muted `usb12`, or another channel
soloed while `usb12` is not, captures silence; a solo elsewhere does not set `usb12`'s own
`soloActive`; a fader off unity offsets every capture by a constant that solve-then-verify cannot
see; AUX or Bluetooth injection would mix outside audio into the capture.

- **Why it matters:** today a silent capture is blamed on a dropped re-amp engage, and an off-unity
  fader levels every preset wrong with no error.
- **What would answer it (HW, fw 1.8.58):** each state in turn (mute, solo another channel, fader
  at −6 dB, AUX with a source) with a capture and a fresh settings backup.
- **Code:** `backup_read::usb3_strip` is the model for a `usb12` pre-flight; nothing decodes
  `usb12` yet.

## 11. Can the Companion measure presets the instrument stimulus does not reach?

- **Mic/Line templates.** The stimulus goes only to USB-In 3, the instrument channel
  (`REAMP_INSTRUMENT_OUT_CH`); re-amp USB-In 4 feeds the mic/line channel. A preset on a
  `Mic/Line Series`, `Mic/Line Parallel 1` or `Mic/Line Split` template captures silence. Open:
  does a stimulus on USB-In 4 reach those chains?
- **Loops 1 and 2.** They are relay loops switched by the preset's top-level `fxLoop` object
  (`fxLoop1Enabled` / `fxLoop1Bypassed`, …), not audio-graph nodes, and re-amp bypasses them
  (static RE, fw 1.8.58; their position ahead of the instrument A/D is inferred). Gear in loop 1 or
  2 is missing from every capture. Open: does a re-amp capture of a preset with loop 1 engaged and
  a pedal in it equal the same preset with the loop off?
- **Why it matters:** both cases read as "no signal" or a confident number for a chain that was
  never fully measured. Both are detectable offline from the backup scan (`micNodes` vs
  `guitarNodes` in `audiograph.rs`; the `fxLoop` object); nothing detects them yet.

## 12. What does a preset whose `info.version` is ahead of the firmware do on import and load?

Static RE (fw 1.8.58): a version ahead of the firmware makes the load substitute the empty preset
and answer `presetError` (field 53) 6. What the unit's UI shows (the "newer firmware revision"
banner) for it is untraced.

- **Why it matters:** an imported file made on newer firmware, and any path that composes preset
  JSON.
- **What would answer it (HW, scratch slot):** import a fixture copy with `info.version` raised
  past the unit's, load it, read the `presetError` and the working copy, and note the banner.
- **Code:** `replace_inplace::load_and_prove` (catches the empty substitution at load), the
  preset-schema fidelity rule in `write-safety.md`.
