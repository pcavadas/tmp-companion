# Copy blocks

The Copy tab copies signal-chain blocks from one source preset into one or more target presets, with per-target placement: replace a block, insert one before or after a position, or remove one.
Device facts it relies on live in [device/presets.md#structural-edits](device/presets.md#structural-edits) and [device/presets.md#insert-abort](device/presets.md#insert-abort).

## Flow

1. **Choose presets.** Pick the source and the targets ("Select all N shown" ticks the rows the search shows).
2. **Place blocks.** Choose a block on the source strip; it stays chosen. Click a "+" gap in a target's strip to insert it there (before the block to the gap's right, or after the last block of a row), or a target block's menu to replace it with the chosen block or remove it. A gap or a replace that would break a firmware limit (`validateBlockEdit.ts::checkOp`, the same predicates as `blockcaps.rs`) is greyed or left out, with the reason under the strip. Undo and Redo span every target and never touch the unit. `copyModel.ts` diffs each edited target against its current graph into an ordered op list (`diffToOps`).
3. **Save.** Behind the "I've backed up with Pro Control" tick, only the targets that changed become jobs, and `copy_apply` runs each job's op list over a held USB session. The dialog names the ticked presets it skips as unchanged.

A run has no cancel: the Copy tab has no Stop button.

The "+" gaps are the one approved hook into the signal-path drawing: `StripGraph.gap` / `StripBlock.uid` (`views/signalChain/stripTypes.ts`), drawn by `views/copy/GapPlus.tsx` through `CopyPath`.
A strip with gaps is drawn exactly as one without them, so a dual cab keeps its two parallel cab tiles while a block is being placed.
The upper cab tile carries the cab's `uid`, so its "+" before and after place the block before or after the whole cab ("at the end" after a trailing one); the lower tile has no `uid` and draws no "+".
Those upper-lane "+" repeat the series "+" on either side of the cab, so no insert position is lost.

Targets that are not the active preset are drawn from `BackupPresetRow.graph` (the backup scan taken on connect), so drawing them needs no device read.

## Live edits

`copy_apply` runs each target's op list (`replaceNode`, `replaceNodeWithBlock`, `insertNode`, `removeNode`) on one held session, re-armed per preset; the wire shapes, confirms and node-id minting are in [device/presets.md#structural-edits](device/presets.md#structural-edits).
Every insert is checked with `blockcaps::check_op` first, and its anchor (an original block's `nodeId`, or an earlier insert's key) is resolved to the device's current id through `DeviceIds`, because a missing anchor or an over-budget insert aborts the device server ([device/presets.md#insert-abort](device/presets.md#insert-abort)).
`diffToOps` orders the ops so each insert's anchor is still present when it lands ([device/presets.md#node-ids](device/presets.md#node-ids)).
Never save on `presetError` or an unconfirmed edit; an unconfirmed edit is read back and re-sent once only when it visibly did not land ([device/presets.md#confirms](device/presets.md#confirms)).
Block-mutation code touches all three places a block lives and never normalizes ids ([device/presets.md#three-keyed-places](device/presets.md#three-keyed-places)).
Probe entry points: `probe --insert-active` and `--insert-map`; the probe's replace arms use `replace_many_held`.

## Read-back

After the last confirmed op and before the rename and save, `read_working_copy` re-prompts the working copy with `currentPresetDataRequest` (field 2) through `Session::live_preset_value`, never a buffer scrape, because the unit pushes no fresh `currentPresetDataChanged` (field 3) after a structural edit [HW+RE].
It compares the reply with `expected_roster(pre-edit roster, acked ops)` as per-group multisets: the projected order of chained same-model inserts is unreliable, and the adopted read carries the device's real order.
That oracle is both the completion predicate (a partial read keeps pumping until the roster matches, then the payload must hold still for two slices) and the adoption guard.
An edit the roster cannot see (a same-model re-stamp) is unverifiable, so no read is spent and `copy_apply` omits the graph; the backend alone decides adoptability.
The read goes through `current_audio_graph`, so a document cut before a complete `template` is refused rather than adopted with a default series shape.
The log line `[copy_apply] slot N: working-copy read …` names the verdict: the acked edit adopted, equal to the pre-edit roster (logged, never blocks the save), truncated (fewer nodes), another mismatch, or no reply.
Offline, `SimDevice` renders its field-2 reply from a working copy that confirmed structural edits mutate.
