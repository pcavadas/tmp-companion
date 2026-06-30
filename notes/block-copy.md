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

## Block-mutation invariants (`audiograph.rs`)

A block lives in three differently-keyed places — touch all or leave dangling state:

1. `audioGraph.{guitarNodes,micNodes}` — the roster, by FenderId.
2. `scenes[].<group>.<FenderId>` — per-scene overrides, by FenderId.
3. `ftsw[i][].nodes[].nodeId` — footswitch assignments, by exact nodeId.

`drop_scene_overrides` + `retarget_ftsw` keep 2 and 3 consistent on replace/remove. The backend matches by exact FenderId — frontend suffix-normalization (CabIR/ConvRvb) must not leak into a backend op or it silently fail-matches.

The firmware-aware palette filters stock models to those available on the connected firmware; user IRs and saved blocks are read live, so they are inherently firmware-correct. Saved blocks are metadata-only, so a faithful saved-block insert is live-only (there is no offline payload to reconstruct).
