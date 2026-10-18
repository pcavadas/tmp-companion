# Scenes and footswitches

Read this before any code that recalls a scene, writes a parameter while a scene is active, or saves scene or footswitch state.
It covers where a write lands, what a recall keeps and what it reverts.

Facts are for firmware 1.8.58 unless a line says otherwise.

## Scene context

Scene index 8 is base; any other index is a scene (`session::BASE_SCENE_SLOT`).
A preset loads into its stored `lastLoadedScene`, and a load applies that scene exactly as saved, without the [Scene Change Behavior](#scene-change-behavior) policy, which only later recalls use [RE].
A write or measure with no preceding `loadScene` lands in whatever scene the unit currently holds, never base by default [HW+RE].
So every base-context write or measure recalls base first, hoisted to the top of its connection before the connection's first write: `leveller`'s `capture_full_at`, `set_knob`, `set_knobs` and `write_footswitch_values`, and `commands/doctor.rs`'s `apply_ops_under_scene`.
`SimDevice` models the stored `lastLoadedScene` offline, so a missing recall is gate-catchable.
`loadScene` (101) pushes field 3 and `sceneLoaded` (102) only when the scene index changes; re-recalling the active scene sends neither [HW+RE].
So a host that harvests scene documents by recall reads the already-active scene from the post-load document instead, or that scene stays unclassifiable.
Every `loadScene` sets the dirty flag, even one that fails [RE].

### Across reconnects

The active scene and the Scene-Edit state belong to the preset model, which no connection event touches, so both survive a reconnect [RE].
Working-copy edits survive a reconnect too: a forced `bypass` measured the same on a fresh zero-write connection [HW].
Re-asserting scene state on a new connection is not free:

- A repeated `loadScene(8)` while base is current, under MAINTAIN CHANGES, reverts an unsaved `presetLevel` ([presetLevel on recall](#presetlevel-on-recall)).
- Any `loadScene` under DISCARD CHANGES discards unsaved edits ([Scene Change Behavior](#scene-change-behavior)).
- A repeated Scene-Edit enable on an overlay that is already Full reseeds it from base ([Scene-write landing](#scene-write-landing)).
- A `loadScene(N ≠ 8)` on a preset with no scenes swaps in the Empty template ([Scene index bounds](#scene-index-bounds)).

A capture that recalls a scene must re-send its forced isolation every time, because the recall re-applies the params that scene's overlays carry.

## Scene Change Behavior

The unit-wide setting decides what a scene recall does with unsaved edits [RE]:

- 0, MAINTAIN CHANGES: the working copy carries over; re-recalling the active scene keeps its unsaved edits, with the one base exception in [presetLevel on recall](#presetlevel-on-recall).
- 1, DISCARD CHANGES: every `loadScene`, base included and whatever the index, first rebuilds base and every overlay from the stored copy, discarding every unsaved block, overlay and footswitch-assignment edit.
  Only the global fields carry over: `audioGraph.presetLevel`, `bpm`, `inputImpedanceMode`, `outputMixerSettings`, the template, `onLoadMidiMsgs` and the footswitch colours.

Read it with `footswitchSettingsRequest` (Settings 47), answered with `footswitchSettingsResponse` (48) to the requester; field 12 is the value, left out on the wire for 0 [HW+RE].
The unit's own UI sets it with Settings `sceneChangeBehavior` (83), whose echo goes to every host [RE].
The app reads it from the backup's `settingsBackup` (`backup_read::scene_change_behavior`), and `commands::level_scenes::scene_discard_guard` refuses scene and footswitch leveling on a positively read DISCARD; probe paths skip that guard.
Under MAINTAIN, unsaved scene-overlay writes survive scene recalls and reconnects, and one save persists all of them [HW+RE].

## Scene-write landing

A scene-context `changeParameter` changes the live DSP at once and is recorded in at most one place [HW+RE]:

- In the scene's overlay, when the param is `bypass` and the overlay entry for that `nodeId` has a `bypass` key.
- In the scene's overlay, when the entry has more than one key (`bypass` and `bypassType` count): insert-or-assign, so an entry that lacks the param gains it, its siblings survive and base is untouched.
- On base otherwise (no entry, or a one-key entry such as `{outputLevel}`), which changes every scene that shares the node's knobs.
- Nowhere, for a `bypass` write into an entry with no `bypass` key: the unit sounds changed until the next recall or save, which loses it.

There is no separate Scene-Edit flag: the Scene-Edit state is the overlay's shape, more than one key meaning ON [RE].
`setNodeSceneEdit` (107) enable reseeds the node's whole overlay from base, wiping any other scene-edited param on it; disable shrinks it to `{bypass}` and restores the node from base; both are refused on base [RE].
So never "restore" Scene Edit by disabling it after a write: that reverts the scene's values for the node to base.
The overlay is keyed by the request's instance `nodeId`, not the FenderId, so for duplicate blocks the keys differ [RE].
At recall, a `{bypass}`-only overlay merges onto base key by key [HW+RE].
Any other overlay rebuilds the node from the overlay alone, and every param it lacks takes the block's built-in default, not base: amp knobs 0.5, amp `outputLevel` 0.25 [HW+RE].
So a `{bypass, bypassType}` overlay plays every other param of its node at the default.
The `audioGraph` in `currentPresetDataRequest` (field 2) and in the field-3 push is the running unit, so it shows base's stored values only while base is current [HW+RE].

### The app's classifier

`scene_jobs::scene_overlay` (`SceneOverlay::{Full, BypassOnly, Absent, Unknown}`) calls an overlay OFF when every key is in the bypass family (`BYPASS_ONLY_KEYS`), not when it has one key.
`Unknown` is a cut or missing read (or a base slot, which has no overlay), and a write against it is refused, since either guess can corrupt the preset.
So a `{bypass, bypassType}` overlay (`BypassOnly`, where the unit takes writes into it) and a one-key `{outputLevel}` overlay (`Full`, where the unit lands writes on base) classify opposite to the device rule; `scene_jobs::scene_write_verdict_for_param`, `leveller::set_knobs`' write shape and `SimDevice` all follow the classifier.
`scene_jobs::overlay_scene_onto_graph` merges every overlay onto base, so an offline graph keeps base values where the unit plays block defaults.
`set_knobs` sends the Scene-Edit enable only for a node with no overlay in the target scene, where the reseed has nothing to lose, and refuses a scene-scoped knob write against a `BypassOnly` or `Unknown` overlay, since enabling reseeds and omitting the enable leaks to base.
Every pre-write gap stays at 300 ms or less (`SETTLE_AFTER_SCENE_RECALL_MS` 150, plus `SETTLE_AFTER_SCENE_EDIT_MS` 300 only when an enable was sent), because a write after 750 ms of host silence is dropped with no `presetError` ([Session lapse](session.md#session-lapse)).

## presetLevel on recall

A scene recall carries the working copy's unsaved `presetLevel` forward, in both Scene Change Behavior modes, with one exception [RE].
Under MAINTAIN CHANGES, `loadScene(8)` while base is already current re-applies the base snapshot's level [HW+RE].
That snapshot's level is the one of the last real load, scene recall or base structural edit, not the saved row; a save never refreshes it [HW+RE], nor does `setPresetLevel` [RE, inferred].
A preset levelled to −20 and saved with base active played −27.5 on the next base recall until it was reloaded [HW].
So a run that saves a new `presetLevel` with base active ends with a real reload (`leveller::reload_after_level_save`), or the user's next base tap plays the old level and a save from there stores it again.
How a leveling run keeps its own unsaved level through a base recall is in [leveling.md#capture-level](../leveling.md#capture-level).
Footswitch runs dirty the working copy with their isolation writes, so their next same-slot load is a real reload anyway.

## Footswitch state on recall

Every recall applies the target scene's `ftswStates` to the footswitches' stored `isActive` flags only [RE].
It changes no block bypass and nothing audible at the recall [RE, inferred].
A scene renders the base chain's own state for any node its overlay has no entry for, so an all-false `ftswStates` does not bypass a block the base leaves ON [HW+RE].
Levelling a footswitch handle on a base-on block therefore moves every scene of the preset by the same dB: cutting a base-on `ACD_TubeScreamer.level` by 2.0 dB moved all three scenes by 2.0 dB [HW].
Under MAINTAIN, a recall first refreshes the outgoing scene's `ftswStates` from the live footswitch state [RE].
A scene footswitch ignores its stored `isActive`, which can disagree with its LED ([Panel and footswitch scene changes](#panel-and-footswitch-scene-changes)) [HW].
`SimDevice` models the `isActive` write.

### Panel and footswitch scene changes

The touch screen, the footswitches and USB all send the same `loadScene`, and the server treats scene 0 like any other scene [RE].
The differences are in what the unit's client sends:

- A scene footswitch counts as active only while its scene is the current scene, which only `sceneLoaded` (102) updates; the stored `isActive` and `ftswStates` play no part [HW+RE].
- A press of another scene's switch sends a `setFootswitchAssignment` (54) update, then that scene's `loadScene`, and reaches the same state as a USB `loadScene` of it [HW].
- A press of the current scene's switch sends the 54 update, then `loadScene(8)`, and returns to base [HW].
- A touch on the active scene sends nothing [HW]; a touch on another scene sends only its `loadScene` [RE].
- A linked, active scene switch in the pressed switch's `linkGroup` sends base before the pressed one sends its slot [RE, inferred].

So a USB `loadScene(0)` stands in for selecting scene 0 from another scene, and scene 0 measures like any other scene [HW+RE].

## Scene index bounds

`loadScene(N ≠ 8)` on a preset with no scenes replaces the working copy with the factory "Empty" template under the preset's name, broadcasts `presetError` 6 and sets the dirty flag, leaving the scene index unchanged [RE].
A later save persists the empty body, the [rejected-at-load](presets.md#rejected-at-load) data-loss class.
`loadScene(8)` on such a preset takes the base path and is safe.
On a preset with scenes, an index at or past the scene count throws out of range with no catch and likely aborts the server [RE, likely].
`Session::load_scene` and `Session::recall_scene_doc` bound any recall but 8 by the scene count proven on that connection: the latest `sceneListResponse` (125) since its last `presetLoaded` (11) push or host load or import, else a `sceneListRequest` (126); they refuse an index at or past it unsent, or when no reply lands; never save after a `loadScene` that drew `presetError` 6.
On a fresh connection that reply lands 1.1–1.3 s after the request, behind the handshake drain [HW].

## Batched scene save

A batched scene-leveling save recalls `restore_scene`, the preset's on-load scene, right before `saveCurrentPreset`.
Two cases store a leveled scene's old value [HW+RE]:

- Under DISCARD CHANGES, that recall rebuilds every overlay from the stored preset and throws the write away.
- With a one-key overlay entry, the write lands on base, and the recall puts the overlay's old value back.

The stored row tells them apart: DISCARD leaves base unchanged, while a one-key overlay leaves base holding the solved value [HW].
Under MAINTAIN CHANGES with an overlay entry of two or more keys the leveled value is kept, even when the leveled scene is the one recalled before the save [HW].
`commands::level_scenes::scene_discard_guard` refuses the DISCARD case in the app and nothing refuses the one-key case; `leveller::persist_mismatches` stamps `persist_mismatch` on either, and the online re-measure reads it as a miss.
