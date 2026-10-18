# Presets on the device

Read this before any code that reads, loads, saves, imports, clears or structurally edits a preset on the unit; several rules here guard against silent data loss.

Facts are for firmware 1.8.58 unless a line says otherwise.

## Slot addressing

The device's `userSlot` is the 0-based list index + 1 [HW].
`list_my_presets` is 0-based; `loadPreset.presetSlot`, `saveCurrentPreset.userSlot`, `clearUserPreset`, `moveUserPreset` and the Song read's `userPresetSlot` are 1-based.
`session/preset.rs` owns the translation: `load_preset`, `save_current_preset`, `clear_user_preset` and `move_user_preset` take a 0-based `ListIndex` and send `+1`.
An empty slot lists as `--` (or "Empty").
`requestNextEmptyPresetSlot` (81) is answered by a broadcast field 82 (−1 when full) [RE]; the app does not use it.
`moveUserPreset` (16) draws no `presetError`, and a list read on the same connection served the pre-move order on older firmware, so verify a move on a fresh connection [HW].

### Address spaces

`read_library_via_backup`'s rows carry `.slot` as the 1-based device slot, while `level_preset`, `level_scenes_apply_batched` and `level_footswitches_apply` take the 0-based list index.
Confirm a slot mapping with a non-destructive read before any `clear`, `move` or save-over, with the guard in the mutation's address space: a list-index guard on a 1-based `clear` deletes a real preset.

## Reading presets

A USB read is complete only when its stream ends in its final `0x35` frame; a lapse cuts it ([Session lapse](session.md#session-lapse)).
Write paths build from the offline `.preset` file or the device backup; USB reads serve live state.

- `currentPresetDataRequest` (field 2) is answered, to its sender only, with `currentPresetDataChanged` (field 3): the whole working copy as one LZ4 block [RE].
  The lenient `proto::lz4_block_decompress` yields a prefix of a cut stream, so readers must still detect a cut.
- `presetDataRequest` (field 8, no `batchStatus`) is answered with `presetDataChanged` (field 9, plaintext): the stored row, not the working copy, whole when the session is kept alive [HW+RE].
- `currentPresetInfoRequest` (field 1) is answered with `presetLoaded` (field 11), then `currentPresetInfoChanged` (field 22, carrying `isDirty`); it does not trigger field 3 [RE].
- `exportPresetRequest` (115) is answered with `exportPresetResponse` (116): the slot JSON encrypted, then LZ4, complete but opaque [RE].
  A hardware test on older firmware drew no reply, and it has not been re-run on fw 1.8.58.

A message over 65,535 bytes is dropped whole; there is no smaller cap [RE].
A stored row's keys are emitted sorted, so a cut loses `scenes` first, and `ftsw`, which sorts ahead of it, can survive as a scene-count oracle [RE].
A re-sent `connectionRequest` on the monitor's live session draws only `connectionError` [HW], so the monitor's graph fallback reads with `read_slot_preset_json_live`, which skips the re-arm.
The unit never truncates a stored row, so a "too large to read over USB" error (`slot_read::read_slot_preset_complete`) means a lapse cut the field-8 read and the backup re-read failed, never an oversized preset.
`Session::latest_json_payload` takes the latest carrier, so after a field-8 read the "current" document can be the stored row; `raw.clear()` before a harvest that must see the working copy.

### Preset lists

`list_my_presets` keeps the longer of the `streams()` and `streams_final()` decodes, because the last record's final byte can ride the closing frame.
Longest-wins still accepts a tail-truncated list, so the startup snapshot uses `list_my_presets_strict` (`harvest_preset_list_strict`, re-arms, then the tolerant read with a warning).
A list read on the same session that just streamed a large field-8 body can come back without the slot asked about [HW].
So `read_slot_preset_complete` names a slot from `monitor::startup_preset_name`, the partial's `info.displayName` or a fresh-session list, and refuses when none answers.

### Device backup

The backup streams at the unit's report pace (~28 KB/s) and is mostly `acdDefaultsBackup` and IR files [HW+RE].
`Session::device_backup`'s `max_secs` bounds the gap between progress events, not the transfer; add no wall-clock cap.
Global settings (`sampleRate`, the mixer strips, `sceneChangeBehavior`) are in the backup's `settingsBackup` JSON.
The app reads scenes lazily: the active preset from field 3, other rows from the startup backup scan (`BackupPresetRow.scenes`); nothing loads a preset at startup or during setup.

## Saving

`saveCurrentPreset` (14) is synchronous and durable when it returns; reads from 0.5 s to 200 s later all showed the saved value [HW+RE].
A save clears the dirty flag, mints a new `preset_id` ([Preset identity](#preset-identity)) and makes its slot the load identity [RE].

### Same-slot load short-circuit

A `loadPreset` of the slot already loaded is a no-op when the dirty flag is clear: it keeps the live working copy and DSP, stale after another writer's save, and still sends its full push set [HW+RE].
The skip compares the load identity and the current scene against the stored `lastLoadedScene`, never `preset_id`, name or content [RE].
An edit sets the dirty flag and turns the same-slot load into a real reload of the stored row (~0.3 s) [HW].
Device-checked dirty setters: `setPresetLevel`, `changeParameter` (bool `bypass` and float `level`) and an `insertNode` append; re-check with `probe --restore-check` [HW].
Also: every `loadScene` and `removeNode` (even a failed one), `setNodeSceneEdit` outside base, a rename to a new name, and a load that substitutes the empty template for a `version` it cannot use [RE].
`setFootswitchAssignment`, `clearFootswitchAssignment`, an insert and a replace set it only when they succeed [RE].
A same-name rename and a move set nothing; a real load and a save clear it [RE].
`SimDevice` models these rules, except that it marks every rename dirty (`same_slot_load_tests`).
`isDirty` rides field 22 (`Session::active_preset_dirty`), but a `currentPresetInfoRequest` is answered ~1.1 s later, dearer than a reload [HW].

### Discarding unsaved edits

Never discard through a bare same-slot load: `leveller::restore_saved_preset` loads a neighbour slot, then the target, which holds even over a stale working copy with a clear flag (`restore_loads_a_neighbour_before_the_target`).

### Save then load on one connection

The save handler is synchronous and messages run in order, so static analysis expects a `loadPreset` chained after a save on the same connection to run after the save lands [RE].
The hardware check is pending, so the code ends a save's connection before any load ([open question](../open-questions.md#save-then-load-on-one-connection)).
Never add a preset load before `save_deferred_scene_writes`: its unsaved writes set the dirty flag, so the load would be a real reload that wipes them.

### No revert in the app

A save cannot be undone from the app; leveling and Copy saves sit behind the "I've backed up with Pro Control" acknowledgment (`ui/BackupAckLabel`), and the Summary's "Try again" (`levelRunStore.retryLeveling`) does not ask again.

## Loading

A `loadPreset` sends `presetError` 6 if the body was rejected, `presetChanged` (25) if the row changed, then the full push set (field 3, `presetLoaded` (11), field 22, `sceneListResponse` (125), `sceneLoaded` (102)), even after a no-op load [RE].
Field 3 arrives 0.1–0.45 s after a load [HW].
A preset loads into its stored `lastLoadedScene`, applied exactly as saved ([Scene context](scenes.md#scene-context)).
A `setPresetLevel` after a load on the same connection held every time [HW] (within ~1 s untested); the code still loads on its own connection, a conservative choice.

### Echo of the previous slot

The first connection after a load can echo the slot loaded before it in its `presetLoaded` (11), although the load landed [HW]; why is open ([open question](../open-questions.md#stale-load-echo)).
The echo is identity: `Session::active_matches` lets it win over a matching name, and must keep doing so.
Every load → reconnect → confirm path ends the load's connection with `Session::lapse` and confirms through `Session::connect_confirmed`, which retries a refused confirm on fresh connections.
A refused save-over removes its scratch only after the stored row re-proves the landing (`replace_inplace::rollback_scratch`), never by name.
Gates: `session::tests::{a_stale_load_echo_is_retried_on_a_fresh_connection, an_echo_that_keeps_naming_another_slot_still_refuses}`.

## Importing

`importPresetRequest` (117) carries no slot: an import always lands in the lowest empty user slot [HW+RE].
With no empty slot it draws `presetError` 26 [RE].
The import checks only LZ4 (300,000-byte limit), decryption and JSON syntax, and stores the body verbatim, `preset_id` included [HW+RE].
`importPresetResponse` (118) did not arrive within 5 s on the unit [HW], so never wait on it.
Callers prove the landing from the stored row: the predicted slot's field-8 row reads the empty template before and the file's `preset_id` and name after (`replace_inplace::StoredProof`); a caller that saves afterwards also runs `load_and_prove`.
An import onto the loaded slot reloads it with the imported body, on that connection and a fresh one, and sends no `presetError` [HW+RE] (`probe --import-reload-check`).
The unit fills unauthored `dspUnitParameters` with active defaults at load: an `ACD_FiveBandParamEQ` authored with filters 2–4 came back with filters 1 and 5 active, costing ~20 dB [HW].
So a fixture that enables a multi-filter block pins every unused filter (`filterNbypass: true`).

### Top-level key set

A body missing a top-level key every real preset carries shows "this preset was created using a newer firmware revision", even with matching versions [HW].
Composed preset JSON must carry the full key set of a real export: `ampControl`, `audioGraph`, `bpm`, `exp`, `expAutoOffTime`, `ftsw`, `fxLoop`, `info`, `inputImpedanceMode`, `lastLoadedScene`, `onLoadMidiMsgs`, `outputMixerSettings`, `presetFootswitchColorActive`, `presetFootswitchColorInactive`, `scenes`.
Its `info` must carry the eight keys the unit writes, or the body is rejected at load: `displayName`, `product_id`, `author`, `timestamp`, `created_at`, `preset_id`, `source_id`, `version` [RE].
A real preset has no top-level `ftswStates`; the e2e scenario fixtures carry the full set.

## Rejected at load

Shape checks run at the first real load of a stored body, never at import and never later [HW+RE].
A check that throws is caught: the working copy becomes the factory "Empty" template (`preset_id` `af08d1ac-…`, `library::TEMPLATE_PRESET_ID`, 0 blocks) under the stored display name, and that `loadPreset` broadcasts `presetError` 6 [HW+RE].
The stored row keeps the imported body, so field 8 returns it unchanged and only a load shows the rejection.
A later save persists the empty body under the original name, and slot + name confirms pass on it [RE].
Load-time rejects [RE]: a footswitch entry that breaks the [Footswitch JSON](#footswitch-json) rules, a `version` ahead of the firmware or not parseable as an integer, an incomplete `info` ([Top-level key set](#top-level-key-set)) or a non-string `preset_id`, and anything else that throws while the body is applied.
`loadScene(N ≠ 8)` on a preset with no scenes substitutes the same template ([Scene index bounds](scenes.md#scene-index-bounds)).
The integer parse reads a leading integer, so a `"5.0"` version loads as 5 [RE].
Static RE also names a `ftsw` row count other than 10 or 20 as a reject, but the unit loads the scenario fixtures that carry 9- and 5-row `ftsw` and pads them to 20 rows [HW].

### Proving a load before a save

Every import → load → save path proves the loaded body.
`replace_inplace_with` (offline edits, migration, `probe --restore`) saves a loaded scratch import over the original, so `replace_inplace::load_and_prove` refuses the save unless the working copy's per-group blocks match the file's, failing closed on a short read.
An import that landed straight on its empty target is loaded and proven the same way, and a proven rejection is cleared, since its stored row reads pristine.
When "my import changes nothing", count populated nodes in field 3 after a real load first ([Presets with no blocks](reamp-audio.md#presets-with-no-blocks)).

## Preset identity

`info.preset_id` is not the unit's identity for anything: the server reads it only when it parses or writes a body [RE].
The load short-circuit compares the load identity, songs and favorites reference the `UserPresets` row id, and no SQL touches `preset_id` [RE].
Every `saveCurrentPreset` mints a fresh `preset_id`, same slot included, with no field-3 push [HW+RE]; edits, renames, import, move and restore keep it, and clear blanks the row to `{}` [RE].
Ids are not unique: importing one file twice gives two slots one id [RE].
Empty slots and rejected loads show the template id, which `library::identity_preset_id_of` and `library::refuse_template_id` never treat as an identity.
An empty-string `preset_id` loads as `""` [RE].
So `preset_id` serves only as a "did this slot's body change" check keyed by slot, re-read after every save.

### Song links

Song rows reference the preset's `UserPresets` row, not its slot number or `preset_id`, so a `moveUserPreset` carries them and changes their `userPresetSlot` [RE].
A save to a slot re-maps that slot's song rows onto the saved body's scenes by scene name: a row whose scene name the body lacks is cleared, which empties the Song row, and a base-scene row is kept [RE, likely].
Overwriting a song-bound slot with a different preset emptied its Song row [HW].
`assignSongPreset` refuses a scene index at or past the preset's scene count with SongError 5 [RE].
The app's link-safe edits keep the slot and its scene names: live `changeParameter` + save (`preset_io::LiveIo`), or the in-place re-import (`preset_io::OfflineIo` → `replace_inplace::replace_inplace_core`).
`preset_io::assert_identity_preserved` refuses an offline edit that changes `info.preset_id`, a conservative host-side check, and `ReplaceOutcome::binding_preserved` compares Song 1's rows before and after.

## Footswitch JSON

Every entry in a `ftsw` row needs `func` [RE].
A `func: "param"` entry needs `groupId`, `nodeId`, `parameterId`, `valueType`, `valueA`, `valueB`, `colorA`, `colorB`, `customLabel`, `switchType` and `isActive`, with string fields as strings, numbers as numbers and `isActive` a boolean; `linkGroup` is optional [RE].
`valueA` and `valueB` must match `valueType`: 1 integer and 2 float each take any number or a boolean, 3 takes a boolean only, 4 a string [RE].
A missing `valueType` rejects the whole body at load [HW+RE]; a `valueType` of 0 or an unknown value only logs [RE].
An `on-off` entry reads its optional keys with defaults, but a wrong type still rejects [RE].
A row may carry at most 5 entries, and nothing checks combinations: an on-off and a param entry on one row are legal [HW+RE].
An imported body with such a row loaded intact with no `presetError`; its entries were not compared [HW].
Real exports carry a string `"valueType": "float"` in the `exp` block's entries; never gate that shape.
A malformed `setFootswitchAssignment` (54) `functionJson` installs a default entry with no `presetError` [RE].

### What the app enforces

`footswitch::validate_import_body` refuses a param entry lacking a numeric `valueType` and, conservatively, any multi-entry row; it checks none of the other keys, types or the 5-entry cap.
`fixture_gates.rs`'s `every_param_footswitch_in_every_fixture_carries_a_numeric_value_type` gates every committed fixture, with a negative check so it cannot pass vacuously.

## Structural edits

The ops, all on one held session re-armed per preset (`copy_apply`, `session::{replace_node, insert_node, remove_node}`):

- `replaceNode` (39) for a stock model or a user IR (`ACD_UserIRTMS` + a string `changeParameter` on `file`); `replaceNodeWithBlock` (100) for a saved block or dual cab.
- `removeNode` (35).
- `insertNode` (34), sent bare: `groupId` is the group key, field 2 (`nodeIdInsertLocation`) the same-group node id to insert before, omitted to append ([Insert abort](#insert-abort)).

Stock models are filtered to `since` ≤ the connected firmware; saved blocks are metadata-only (`blockPresetsMap`, no params), so saved-block and IR inserts are live-only.

### Confirms

`nodeReplaced` (40), `nodeRemoved` (36) and `nodeInserted` (33) are broadcast [RE].
A replace over the CPU budget or naming an unknown node gets `presetError` (field 53) code 14 and no `nodeReplaced`; an insert never gets a `presetError` [RE].
Never save on a `presetError` or an unconfirmed edit: a wrong-content save corrupts the slot.
No traced handler drops a correctly framed structural edit, so a missing confirm is one still queued behind the load's pushes, a lapse, a backup freeze or an insert abort [RE, inferred].
Read an unconfirmed edit back (`Session::insert_landed`, `read_working_copy`) and re-send it only when it visibly did not land; a blind re-send duplicates an insert or names a replaced node that is gone.
The unit pushes no field 3 after a structural edit; a field-2 re-prompt on the same session (`Session::live_preset_value`) answers with the post-edit working copy [HW+RE], and [Copy](../copy.md) builds its read-back on it.

### Node ids

A group keeps its nodes in a map keyed by node id [RE].
An insert or replace mints the model's FenderId, then the first free `<FenderId>_1`, `_2`… in the group (`audiograph::mint_node_id`); a replace frees the old id first, so replacing X with X keeps "X" [HW+RE].
`nodeInserted` and `nodeReplaced` carry the new node, with its minted id, in field 3; their field 2 echoes the anchor as sent or the old id [HW+RE].
A load keeps stored ids as they are (`cab1` in the scenario fixtures); a stored duplicate id overwrites the first node's map entry and reads back once [RE].
So never assume a FenderId is a node id: `copy_apply` resolves each anchor through `DeviceIds` (the confirms' `nodeJson` or a working-copy read) and refuses one it cannot resolve.
`diffToOps` emits removes, then replaces, then inserts right to left, so each insert's anchor is still present when it lands (`copyModel.test.ts` "INV-A").

### Three keyed places

Block-mutation code (`audiograph.rs`) must touch all three places a block lives:

- `audioGraph.{guitarNodes, micNodes}`: the roster, each node carrying its `FenderId` and instance `nodeId`.
- `scenes[].<group>.<nodeId>`: per-scene overlays, keyed by instance id [RE].
- `ftsw[i][].nodes[].nodeId`: footswitch assignments, by exact `nodeId`.

Model-level ops match by exact FenderId (`audiograph::Node::matches_model`), instance-level ops by `Node::matches_instance`; the backend never normalizes ids, so a frontend-normalized id (`resolveDeviceId`) silently matches nothing.

## Insert abort

An `insertNode` whose anchor is not a node id in its group, or that goes over the CPU budget, gets no reply and aborts the device server [RE].
The anchor is looked up in the group's node map, with no fallback to the FenderId [RE].
The budget check rounds the new total to the nearest integer in float32 (`frinta`) and fails when that exceeds the budget, 76.5 in `model-cpu.json`, so any total of 76.5 or more fails [RE].
The cost table matches `model-cpu.json`; an id the firmware lacks costs 20 [RE].
The exception escapes the message thread and `std::terminate` aborts the server; systemd restarts it and the HID session drops [RE], and a third crash within 60 s leaves it down [RE, inferred].
`insertNodeAtBlockIndex` (99) aborts the same way; `moveNode` (37) never throws, and an over-budget load only logs and still adds the node [RE].
`blockcaps.rs` is the only enforcement of the block caps: check every insert with `blockcaps::check_op` first.

### Block caps

The four count caps (convolution reverb, cabinets, Glooper, FX-loop coexistence) live only in the unit's own UI; the server checks the CPU budget alone [RE].
`blockcaps.rs` copies them with membership sets from older firmware (`src/models/block-classification.json`), a known gap on the cabinet cap: it counts `ACD_ExternalCab`, which fw 1.8.58 exempts [RE], and under-counts an amp with a second mic [RE, likely] ([open question](../open-questions.md#cabinet-cap-weights)).
