# Copy blocks (Copy tab)

The Copy tab copies signal-chain blocks from one reference preset into one or more target presets, with per-target placement: replace a block, insert before/after a position, or remove a block.

## Flow

1. **Choose presets** — pick the preset to copy from + the targets ("Select all N shown" ticks the rows the search shows).
2. **Place blocks** — choose a block on the source strip; it stays chosen. Then click a "+" gap in a target's strip to insert it there (before the block to the gap's right, or after the last block of a row), or a target block's menu to Replace it with the chosen block or Remove it. A gap or a Replace that would break a firmware limit (`validateBlockEdit.ts::checkOp`, the same predicates as `blockcaps.rs`) is greyed or left out, with the reason under the strip. Undo/Redo span every target and never touch the unit. `copyModel.ts` diffs each edited target against its current graph into an ordered op list (`diffToOps`).
3. **Save** — behind the "I've backed up with Pro Control" tick, only the targets that changed become jobs; `copy_apply` runs each job's op list over a held USB session. The dialog names the ticked presets it skips as unchanged.

The "+" gaps are the one approved hook into the frozen signal-path code: `StripGraph.gap` / `StripBlock.uid` (`views/signalChain/stripTypes.ts`), drawn by `views/copy/GapPlus.tsx` through `CopyPath`. A strip with gaps is drawn exactly as one without them, so a dual cab keeps its two parallel cab tiles while a block is being placed. The upper cab tile carries the cab's `uid`, so its "+" before and after place the block before or after the whole cab ("at the end" after a trailing one); the lower tile has no `uid` and draws no "+". Those upper-lane "+" repeat the series "+" either side of the cab, which place the block at the same spot, so no insert position is lost.

Non-active target presets are rendered from `BackupPresetRow.graph` (the device backup the app already pulls on connect), so no extra device read is needed to draw them.

## Live edit protocol

Saving runs live, link-safe, in-place edits on a held session, re-armed per preset:

- `replaceNode` — swap a block for a stock model.
- `replaceNodeWithBlock` — swap for a user saved block / dual cab (by library index).
- `insertNode` — add a block (sent bare; `groupId` is the group key; field-2 = the LIVE node id, in the same group, to insert BEFORE, omitted = append). The anchor is matched against the group's CURRENT node ids, and a replace or insert mints a new one, so it is never a FenderId and never a pre-edit id: `copy_apply` resolves each op's anchor (an original block's `nodeId`, or an earlier insert's `key`) through its `DeviceIds`, and checks every insert with `blockcaps::check_op` first — a missing anchor or an over-budget insert aborts the device server (`danger.md`).
- `removeNode` — remove a block.
- A user IR is applied as `replaceNode` → `ACD_UserIRTMS` plus a string `changeParameter` on the node's `file` param.

Confirm each edit on its acknowledgement (`nodeReplaced` / `nodeInserted` / `nodeRemoved`); **never save on `presetError` or an unconfirmed edit** (a wrong-content save corrupts the slot). A missing confirm with no `presetError` is recovered by READ-BACK, never by blind re-send: `copy_apply` reads the working copy and re-sends once only when the edit visibly did not land (a landed replace has minted a new node id, so re-sending would name a node that is gone). The old "the first edit after a fresh load can be dropped" reading is a fw 1.8.45 belief with no cited HW evidence; fw 1.8.58 has no silent-drop path for a correctly framed edit (INFERRED). A missing confirm is a confirm queued behind the FIFO, a lapse (which draws ConnectionError), a backup freeze, or an insert abort. `cancel_copy_apply` stops a run.

## Block-mutation invariants

The three-keyed-places rule, the exact-FenderId matching rule, and the firmware-aware-palette /
offline-impossible notes are common to ALL block-mutation code (bulk-replace and Copy alike) —
see `notes/gotchas.md#live-per-node-structural-edits-the-protocol-behind-the-block-edit-features`,
not restated here.
