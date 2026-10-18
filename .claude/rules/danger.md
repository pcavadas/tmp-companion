# Danger rules

This file has no `paths:` frontmatter on purpose: it loads unconditionally and is re-injected after compaction.
Each rule guards against data loss, a wedged device or a crashed machine.
The mechanism and evidence for each rule live in the linked [`notes/device/`](../../notes/device/) section; read it before changing the behaviour a rule governs.

## Destructive writes

- **DANGER: device `userSlot` = list index + 1.**
  Before any destructive op keyed on a slot mapping (`clear`, `move`, save-over), confirm the mapping with a non-destructive read and put the guard in the mutation's own address space; a list-index guard on a 1-based `clear` deletes a real preset.
  [→](../../notes/device/presets.md#slot-addressing)
- **The first connection after a load can echo the slot loaded before it.**
  The echo is identity and must keep winning over a matching name: confirm through `Session::connect_confirmed`, never by name.
  [→](../../notes/device/presets.md#echo-of-the-previous-slot)
- **DANGER: a save is permanent and a leveling run saves automatically.**
  The only gate is the "I've backed up with Pro Control" acknowledgment in Set up; the app has no revert.
  [→](../../notes/device/presets.md#saving)
- **DANGER: a same-slot `loadPreset` over a clean working copy is a no-op**, including right after a save.
  Discard unsaved edits only through `leveller::restore_saved_preset` (it loads a neighbour slot first), and never add a preset load before `save_deferred_scene_writes`, which would wipe the run's unsaved writes.
  [→](../../notes/device/presets.md#same-slot-load-short-circuit)
- **Keep a save on its own connection** until `probe --save-load-test` passes on fw 1.8.58: no `loadPreset` follows a `saveCurrentPreset` on the same connection.
  [→](../../notes/open-questions.md#save-then-load-on-one-connection)
- **DANGER: a body the firmware rejects at load becomes a silent empty preset that keeps its name.**
  Prove the loaded working copy with `replace_inplace::load_and_prove` before every save; an import nothing saves over proves its landing with `replace_inplace::StoredProof`.
  [→](../../notes/device/presets.md#rejected-at-load)
- **DANGER: every emitted param-func footswitch entry carries a numeric `valueType`** and the other required fields, or the imported preset is rejected at load.
  `footswitch::validate_import_body` and `fixture_gates` also refuse a two-entry row (legal on the device, kept refused until a hardware-checked emitter change).
  [→](../../notes/device/presets.md#footswitch-json)
- **DANGER: an `insertNode` (field 34) whose anchor is not a node id in its group, or that exceeds the CPU budget, aborts the device server.**
  Never assume a FenderId is a node id: anchor on the id the confirm returned (`copy_apply`'s `DeviceIds`), check every insert with `blockcaps::check_op`, and read an unconfirmed insert back (`Session::insert_landed`) instead of re-sending it.
  [→](../../notes/device/presets.md#insert-abort)
- **DANGER: send a `loadScene` only for a scene the preset has.**
  On a preset with no scenes it swaps in the empty template under the preset's name; `Session::load_scene` does not check the index, and only `commands/doctor.rs` calls `session::scene_index_ok`.
  [→](../../notes/device/scenes.md#scene-index-bounds)
- **DANGER: a base recall after a `presetLevel` save plays the old level** until a real reload.
  Any path that saves `presetLevel` with base active must end with `leveller::reload_after_level_save`.
  [→](../../notes/device/scenes.md#presetlevel-on-recall)
- **OPEN: a batched scene-leveling save can revert the scene it leveled when that scene is `restore_scene`**; `leveller::persist_mismatches` flags it.
  [→](../../notes/open-questions.md#batched-save-revert)

## Re-amp and measurement

- **One engage per capture, on a fresh connection, for accuracy.**
  Only the first capture after an engage is accurate; proof of engagement is a Mixer `reAmpModeRequest` (4) → field 9 read or a capture that passes an armed `leveller::measure_floor_guarded`, never the Settings echo or a finite loudness.
  [→](../../notes/device/reamp-audio.md#engage-and-capture-accuracy)
- **DANGER: a silent or failed inject reads as the stationary output floor**, and `measure_c` would accept it as a valid `C` without the production floor guards.
  [→](../../notes/device/reamp-audio.md#silent-captures)
- **750 ms of host OUT silence lapses the session**: every message, an engage included, is dropped until a `connectionRequest` reopens it.
  Keep every idle gap ≤300 ms: `leveller::capture_on_session` heartbeats between the scene recall and the engage (recall, 150 ms, heartbeat, 100 ms, engage).
  [→](../../notes/device/reamp-audio.md#idle-gap-before-an-engage)
- **Mid-engage edits reach the audio while OUT traffic flows.**
  The code still sets the level before engaging, engages once per scene, and loads on its own connection (`measure_knob_at`), a conservative shape.
  [→](../../notes/device/reamp-audio.md#edits-during-an-engage)
- **Toggle re-amp only through `SettingsMessage` `reampModeActive` (field 30)**, never Mixer `setReAmpMode` (15); read the real state with Mixer `reAmpModeRequest` (4) → field 9.
  [→](../../notes/device/reamp-audio.md#re-amp-toggle)
- **DANGER: every re-amp run ends with a guaranteed OFF on a fresh connection.**
  A dropped OFF leaves the unit input-muted, and the user's recovery is a restart of the unit ([open question](../../notes/open-questions.md#re-amp-across-a-usb-unplug)).
  `leveller::ReampRunGuard` (held by `device_gate::with_released_seize_blocking`) is the backstop, cancel never skips it, and the user-facing error keys on `reamp_still_on`.
  [→](../../notes/device/reamp-audio.md#stuck-re-amp)
- **A write with no preceding `loadScene` lands in whatever scene the connection holds**, never base by default; a preset loads into its saved `lastLoadedScene`.
  [→](../../notes/device/scenes.md#scene-context)
- **OPEN: do not trust scene-0 leveling.**
  A USB `loadScene(0)` and a footswitch press of scene 0 reached different amp states; a scene footswitch toggles and can send base (8) [RE], while the link-group and touch paths are [RE, inferred].
  [→](../../notes/open-questions.md#scene-0-via-footswitch)
- **`outputLevel` 0 is digital silence**, and the measurement errors ("no signal captured") rather than invent a loudness.
  [→](../../notes/device/reamp-audio.md#outputlevel-0)
- **The stimulus is 48 kHz**, so `audio::pick_config` needs a 48 kHz host format; the device itself does not.
  [→](../../notes/device/reamp-audio.md#sample-rate)

## Resources and connections

- **DANGER: `audio::LiveReamp` is ring-buffered (`LIVE_RING_SECS`).**
  An unbounded capture buffer OOMs the whole Mac; reuse one stream pair per preset.
- **`0xe00002c5` means another handle holds the exclusive seize** (Pro Control, or an overlapping `Session` that `Hid::open` names); there is no device open lockout.
  Serialize every device op through `device_gate::DEVICE_OP_LOCK`.
  [→](../../notes/device/session.md#diagnosing-0xe00002c5)

## UI values

- **DANGER: a dropdown displays the value that will be used.**
  `ui/Select.tsx` shows the stored value (`resolveSelectValue`) or a placeholder, never `options[0]`; derive defaults from the live option source.
  Gates: `src/__tests__/SelectStaleValue.test.tsx`, `SelectLateContent.test.tsx`.
