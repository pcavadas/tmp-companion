# Copy blocks (Copy tab)

The Copy tab copies signal-chain blocks from one reference preset into one or more target presets, with per-target placement: replace a block, insert before/after a position, or remove a block.

## Flow

1. **Choose presets** — pick a reference preset + the targets.
2. **Place blocks** — per target, edit the chain (the interactive signal-path renderer shows the result). `copyModel.ts` diffs each edited target against its current graph into an ordered op list (`diffToOps`).
3. **Save** — `copy_apply` runs the op list per target over a held USB session.

Non-active target presets are rendered from `BackupPresetRow.graph` (the device backup the app already pulls on connect), so no extra device read is needed to draw them.

## Live edit protocol

Saving runs live, link-safe, in-place edits on a held session, re-armed per preset:

- `replaceNode` — swap a block for a stock model.
- `replaceNodeWithBlock` — swap for a user saved block / dual cab (by library index).
- `insertNode` — add a block (sent bare; `groupId` is the group key; field-2 = the same-group FenderId to insert BEFORE, omitted = append).
- `removeNode` — remove a block.
- A user IR is applied as `replaceNode` → `ACD_UserIRTMS` plus a string `changeParameter` on the node's `file` param.

Confirm each edit on its acknowledgement (`nodeReplaced` / `nodeInserted` / `nodeRemoved`); **never save on `presetError` or an unconfirmed edit** (a wrong-content save corrupts the slot). The first edit after a fresh load can be dropped — retry it once. `cancel_copy_apply` stops a run.

## Block-mutation invariants

The three-keyed-places rule, the exact-FenderId matching rule, and the firmware-aware-palette /
offline-impossible notes are common to ALL block-mutation code (bulk-replace and Copy alike) —
see `notes/gotchas.md#live-per-node-structural-edits-the-protocol-behind-the-block-edit-features`,
not restated here.
