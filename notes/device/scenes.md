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
So a run reads the already-active scene from the post-load document instead of recalling it, or that scene would stay unclassifiable.
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

A scene-context `changeParameter` changes the live DSP at once and is recorded in exactly one place [RE]:

- In the scene's overlay, when the overlay has an entry for that `nodeId` and the param is `bypass`.
- In the scene's overlay, when the entry has more than one key: insert-or-assign, so a Full overlay that lacks the param gains it, its siblings survive and base is untouched.
- On base otherwise (no entry, or a one-key entry and a non-bypass param), which changes every scene that shares the node's knobs.

There is no separate Scene-Edit flag: the Scene-Edit state is the overlay's shape, more than one key meaning ON [RE].
`setNodeSceneEdit` (107) enable reseeds the node's whole overlay from base, wiping any other scene-edited param on it; disable shrinks it to `{bypass}`; both are refused on base [RE].
The overlay is keyed by the request's instance `nodeId`, not the FenderId, so for duplicate blocks the keys differ [RE].
At recall, a `{bypass}`-only overlay merges key by key, and any other overlay replaces the node's whole `dspUnitParameters` [RE].
Hardware-checked shapes: a write against a `{bypass}`-only overlay landed on base (base gain 2.5 → 7.0), and a write to a Full overlay lacking the param was inserted into the overlay with base untouched [HW].
Open shapes: a `{bypass, bypassType}` overlay, a one-key non-bypass overlay such as `{outputLevel}`, and how a partial Full overlay renders at recall ([open question](../open-questions.md#scene-write-landing-shapes)).

### The app's classifier

`scene_jobs::scene_overlay` (`SceneOverlay::{Full, BypassOnly, Absent, Unknown}`) calls an overlay OFF when every key is in the bypass family (`BYPASS_ONLY_KEYS`), not when it has one key.
So a `{bypass, bypassType}` overlay and a one-key `{outputLevel}` overlay classify opposite to the device rule; `scene_jobs::scene_write_verdict_for_param`, `overlay_scene_onto_graph`, `leveller::set_knobs`' write shape and `SimDevice` all follow the classifier.
`set_knobs` sends the Scene-Edit enable only for a node with no overlay in the target scene, where the reseed has nothing to lose, and refuses a scene-scoped knob write against a `BypassOnly` or `Unknown` overlay, since enabling reseeds and omitting the enable leaks to base.
Every pre-write gap stays at 300 ms or less (`SETTLE_AFTER_SCENE_RECALL_MS` 150, plus `SETTLE_AFTER_SCENE_EDIT_MS` 300 only when an enable was sent), because a write after 750 ms of host silence is dropped with no `presetError` ([Session lapse](session.md#session-lapse)).

## presetLevel on recall

A scene recall carries the working copy's unsaved `presetLevel` forward, in both Scene Change Behavior modes, with one exception [RE].
Under MAINTAIN CHANGES, `loadScene(8)` while base is already current re-applies the base snapshot's level [HW+RE].
That snapshot's level is the one of the last real load, scene recall or base structural edit, not the saved row; `setPresetLevel` and a save never refresh it [RE].
A preset levelled to −20 and saved with base active played −27.5 on the next base recall until it was reloaded [HW].
So a run that saves a new `presetLevel` with base active ends with a real reload (`leveller::reload_after_level_save`), or the user's next base tap plays the old level and a save from there stores it again.
Every capture re-asserts the run's own level after its recall (`leveller::recall_reassert_save` and the capture arms), so an unsaved level the run holds survives a base recall.
Footswitch runs dirty the working copy with their isolation writes, so their next same-slot load is a real reload anyway.

## Footswitch state on recall

Every recall applies the target scene's `ftswStates` to the footswitches' `isActive` flags only: each switch's LED and what its next press does [RE].
It changes no block bypass and nothing audible at the recall [RE, inferred].
A scene renders the base chain's own state for anything its overlay does not carry, so an all-false `ftswStates` does not bypass a block the base leaves ON [HW+RE].
Levelling a footswitch handle on a base-on block therefore moves every scene of the preset by the same dB: cutting a base-on `ACD_TubeScreamer.level` by 2.0 dB moved all three scenes by 2.0 dB [HW].
Under MAINTAIN, a recall first refreshes the outgoing scene's `ftswStates` from the live footswitch state [RE].

### Panel and footswitch scene changes

The touch screen, the footswitches and USB all send the same `loadScene`, and the server treats scene 0 like any other scene [RE].
The differences are in what the unit's client sends:

- A scene footswitch toggles its `isActive` flag on each press, then sends its scene when the flag turned on and `loadScene(8)` (base) when it turned off [RE].
- The active scene's switch has its flag set only after an earlier press or a recall whose `ftswStates` set it; otherwise a press re-loads that scene [RE].
- A touch on the active scene sends nothing, so no policy step runs [RE, inferred].
- A linked, active scene switch in the pressed switch's `linkGroup` sends base before the pressed one sends its slot [RE, inferred].

On the 2-amp Guitar preset a USB `loadScene(0)` reached a different amp state than a physical press, and the cause is open; do not trust scene-0 leveling until it is resolved ([open question](../open-questions.md#scene-0-via-footswitch)).

## Scene index bounds

`loadScene(N ≠ 8)` on a preset with no scenes replaces the working copy with the factory "Empty" template under the preset's name, broadcasts `presetError` 6 and sets the dirty flag, leaving the scene index unchanged [RE].
A later save persists the empty body, the [rejected-at-load](presets.md#rejected-at-load) data-loss class.
`loadScene(8)` on such a preset takes the base path and is safe.
On a preset with scenes, an index at or past the scene count throws out of range with no catch and likely aborts the server [RE, likely].
`Session::load_scene` does not check the index, so its caller must; never save after a `loadScene` that drew `presetError` 6.

## Batched scene save

A batched scene-leveling save reverted the one scene it had just levelled when that scene was also `restore_scene`, the preset's on-load scene [HW].
`level_scenes_oneshot`'s final save (`save_deferred_scene_writes` → `recall_reassert_save` → `recall_original_scene`) recalls `restore_scene` right before `saveCurrentPreset`.
`probe --level-scenes 404 -17 guitar-humbucker 0 --commit` on "E2E Hiwatt 3S" (stored `lastLoadedScene` 0) solved scene 0's `outputLevel` to 1.0 and saved 0.96, deterministically [HW].
It is not silent: `leveller::persist_mismatches` / `verify_persisted_writes` stamp `persist_mismatch`, and the online re-measure reads it as a miss.
Under MAINTAIN CHANGES a re-recall of the active scene keeps its unsaved edits [RE], so two causes remain:

- The unit was set to DISCARD CHANGES, where every recall rebuilds from the stored preset; the probe path skips `scene_discard_guard`.
- The write's overlay entry had one key, so the write landed on base and the recall re-applied the overlay's old value ([Scene-write landing](#scene-write-landing)).

A post-save `presetDataRequest` (field 8) read separates them: under the second, base holds 1.0 next to the overlay's 0.96 ([open question](../open-questions.md#batched-save-revert)).
A fix must cover three save paths: `recall_original_scene` is shared by `apply_levels`, `save_deferred_scene_writes` and `write_fs_values_on_session`; `recall_reassert_save` re-applies `presetLevel` after the recall, and nothing re-applies a per-scene block write.
