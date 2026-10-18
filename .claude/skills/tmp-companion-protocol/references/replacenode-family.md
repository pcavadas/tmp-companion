# ReplaceNode family — per-slot signal-chain mutation

TMS exposes a small family of messages for replacing a single block in the signal chain without reloading the whole preset.
These map directly onto the `audioGraph.guitarNodes.G{1,2,…}[]` arrays in the decoded `presetJson` and are the cleanest known protocol-level lever for mutating the chain one slot at a time.

This reference documents the block-mutation wire API.
The `replaceNode` / `insertNode` / `removeNode` family is the SHIPPED mechanism behind the companion's Copy feature (`copyModel.ts` → `copy_apply` → `audiograph.rs`).

## The five messages

Message schemas from the extracted client-app protobuf (the companion mirrors the codec in `src-tauri/src/proto.rs`, golden-tested).

### `ReplaceNode.proto`

```proto
message ReplaceNode {
  string destGroup = 1;          // group ID, e.g. "G1" (matches presetJson.audioGraph.guitarNodes keys)
  string nodeIdToReplace = 2;    // current node ID in that group
  string fenderId = 3;           // new block model ID (e.g. "ACD_TwinReverbAmp")
}
```

Surfaced as `PresetMessage.replaceNode = 39` (`PresetMessage.proto:178`).

**Semantics:** swap the node at `destGroup[nodeIdToReplace]` for a fresh instance of model `fenderId`.
The slot index is implicit — the array slot of the node currently bearing `nodeIdToReplace`.
There is no explicit `index` field; identity comes from `nodeIdToReplace`.
`replaceNode` carries ONLY the model id — it installs a fresh default-param instance and does NOT transfer the source block's parameter values; a same-model replace is therefore a model re-stamp, not a settings copy (HW).

### `ReplaceNodeWithBlock.proto`

```proto
message ReplaceNodeWithBlock {
  string destGroup = 1;          // group ID
  string nodeIdToReplace = 2;    // current node ID
  string fenderId = 3;           // *block-preset* ID (saved block snapshot, not a raw model)
  uint32 index = 4;              // index within the block-preset library
}
```

Surfaced as `PresetMessage.replaceNodeWithBlock = 100` (`PresetMessage.proto:239`).

**Semantics:** like `ReplaceNode` but installs a saved **block preset** (a block with all parameters preserved) rather than a default-parameter model.
`index` is the index in the block-preset library (see `RequestAllBlockPresets`/`AllBlockPresetsResponse`, fields 135-136, for the library contents).

### `NodeReplaced.proto`

```proto
message NodeReplaced {
  string destGroup = 1;          // echoes the request's destGroup
  string replacedNodeId = 2;     // the OLD node's ID, echoed from the request
  string nodeJson = 3;           // full JSON of the new node; its "nodeId" is the NEW id
}
```

Surfaced as `PresetMessage.nodeReplaced = 40` (`PresetMessage.proto:179`).

**Semantics:** server → every-client broadcast after a successful `ReplaceNode` or `ReplaceNodeWithBlock`.
Carries the **full JSON** of the new node — everything needed to render the new block (model name, parameter values, defaults, display strings) without a follow-up read.
Re-key the node by `nodeJson["nodeId"]`, never by field 2 (static RE, fw 1.8.58).

### `UndoReplaceNode` / `UndoReplaceNodeResponse`

`PresetMessage.undoReplaceNode = 121` `{originalNodeJson=1, groupId=2, currentNodeIdToReplace=3}` and its ACK `undoReplaceNodeResponse = 122` `{replacedNodeId=1, originalNodeId=2, groupId=3}` (`PresetMessage.proto:260-261`): a **stateless undo** — the caller carries the pre-replace node JSON and the device re-installs it; there is no device-side undo stack.
The app does not send them.

## Identifier semantics

- Addressing is **by string ID**, not by numeric slot index.
  `destGroup` is the group key ("G1", "G2", …); `nodeIdToReplace` is the per-node ID.
- This matches the `presetJson` shape: `audioGraph.guitarNodes.G1[].nodeId`.
  The host can find a node by walking the JSON array; it does not need to track wire-level slot indices.
- One consequence: replacing a node **does not change other nodes' IDs**.
  A host can hold a stable reference to a downstream node across a `ReplaceNode` on an upstream one.

## Comparison to Mustang LT

LT's [`ReplaceNode.proto` + `ReplaceNodeStatus.proto`](https://github.com/brentmaxwell/LtAmp/tree/main/Schema/protobuf) pair is the LT-side analogue.
TMS extends it in three ways:

1. **Adds `ReplaceNodeWithBlock`** for installing saved block presets (LT has no block-preset library exposed at this level).
2. **Adds `UndoReplaceNode` + `UndoReplaceNodeResponse`** for stateless undo (LT has no undo at the protocol layer).
3. **Returns `NodeReplaced` (with full `nodeJson`)** rather than LT's `ReplaceNodeStatus` (which carries only ack flags).
   TMS gives the host enough to update its model without a follow-up `RetrievePreset`/`PresetJSONMessage` round-trip.

## InsertNode / NodeInserted — ADD a block to the chain (field 34)

The structural-edit counterpart that ADDS a block (the messages above only replace/remove).
Captured byte-exact from a Pro Control add-block session (append / insert-before / the UserIR `changeParameter`).

### `InsertNode.proto`

```proto
message InsertNode {
  string groupId = 1;                 // group KEY, e.g. "G1" (NOT the graph name "guitarNodes")
  string nodeIdInsertLocation = 2;    // OPTIONAL: omit = APPEND to end of group; set = a SAME-group NODE ID to insert BEFORE (fw 1.8.58: a miss aborts the server)
  string fenderId = 3;                // new block model ID
}
```

Surfaced as `PresetMessage.insertNode = 34`; the device confirms with `PresetMessage.nodeInserted = 33`, broadcast to every client: field 2 echoes the anchor, and field 3 is the new node's JSON, whose `"nodeId"` is the minted id (static RE + HW, fw 1.8.58).
There is also `InsertNodeAtBlockIndex` (field 99, adds `index = 4`), but **Pro Control sends the bare field 34 for add-block**; field 99 is only the `probe --insert-map --at-index` dev tool, and it aborts the server over budget exactly like field 34.
`groupId` is the group KEY, exactly like `replaceNode.destGroup`.

**Semantics:** insert a fresh `fenderId` instance into `groupId`; the device fills the new model's default parameters.
Omit field 2 to append at the end of the group, or set it to the **LIVE node id** of a node in the same group to insert immediately **BEFORE** that node (HW).
The anchor is matched against the group's CURRENT node ids.
A replace or insert mints the new node's id from its FenderId: the FenderId itself if no node in the group holds it, else the first free `<FenderId>_1`, `_2`… (static RE + HW, fw 1.8.58) — so take the id from the confirm, and never anchor on a pre-edit id.
A node id often equals its model's FenderId, but a second instance does not.
`copy_apply` resolves each anchor through its `DeviceIds` and checks every insert with `blockcaps::check_op` first: on fw 1.8.58 an anchor that names no node in the group gets no reply and ABORTS the device server (static RE; [insert abort](../../../../notes/device/structural-edits.md#insert-abort)).
The field-2 anchor MUST name a node in the **SAME group** as the insert — a visual signal-chain series can span device groups (e.g. amp in G1, pedals in G4), and an anchor in another group is a miss that aborts the server.
So `diffToOps` anchors each insert before the next SAME-group block, else appends.

## Live edit sequence

This family is the SHIPPED mechanism behind the companion's live block editing.
**Two consumers:** **`copy_apply`** (the Copy tab — an ordered replace/insert/remove op list PER target preset) and the `probe` replace arms' `replace_many_held`, both on the same held-session + confirm-gate machinery.
The load-bearing wire facts:

- **`batchStatus`:** structural mutations (`replaceNode`=39, `removeNode`=35, `replaceNodeWithBlock`=100) are sent without one ([request batching](../../../../notes/device/session.md#request-batching)); an empty `connectionError` means the session had lapsed and the message was dropped.
- **Pro Control's persist sequence (per node):** `nodeJsonRequest`(119) edit-context preamble → the no-batch mutation → `renameCurrentPreset`(13) → `saveCurrentPreset`(14); the code mirrors it.
  `nodeJsonRequest` is a stateless read whose `nodeJsonResponse` (120) goes to every client, and `saveCurrentPreset` stores the whole working copy, structural edits included, so neither the preamble nor the rename is needed to persist (static RE, fw 1.8.58).
- **Confirm/reject gate:** save ONLY after `nodeReplaced`(40) / `nodeRemoved`(36); a `presetError`(53) means the device REJECTED the edit → ABORT, never save (you'd persist the wrong active preset's graph).
  A `presetError` can also mean an **invalid model id** — a `fenderId` the firmware doesn't ship rejects — or an unknown `nodeIdToReplace` (`presetError` 14, like an over-budget replace); use a FenderId read from a live node, not a guessed catalog/codename id.
  The session must be ATTACHED to the target preset — verify by the `PresetLoaded` slot echo (identity), NOT display-name equality (duplicate names + a load-that-didn't-take → wrong-preset edit).
- **60-byte single-report limit:** a `replaceNode` body with a long id (e.g. a `…CabIR` node ≈ 61 B) overflows the single HID report — MUST use the chunked `0x33/0x34/0x35` framing (`transact_chunked`); the single-report path panics on long ids.
- **User IR replacement = two edits:** `replaceNode`→`ACD_UserIRTMS`, then a STRING `changeParameter` (the `value` oneof's **`stringVal`, field 6**, not floatVal/5) setting the new node's `file` param to the chosen IR filename; verify by re-reading `nodeJsonResponse`(120).`nodeJsonString` before saving.
- **`replaceNodeWithBlock`(100)** installs a user SAVED block / dual cab by its **library index** within `blockPresetsMap` (params are device-internal-by-index — there is no offline payload; faithful insert is live-only).
  NOTE: `blockPresetsMap` is DOMINATED by device-default entries (HW: 464 total → 366 defaults, named `"<Model> - N"` / `"Factory Default"`, carrying only `name`+`favorite` — no structural `isDefault` flag), so a "user saved blocks" picker must filter by name heuristic or it shows hundreds of factory defaults; but the `index` still spans the WHOLE unfiltered map, so compute it against the full map, not the filtered view.
- **Discovery RPCs:** `RequestAllBlockPresets`(135)→`AllBlockPresetsResponse`(136) and `UserIRListRequest` follow the `batchStatus` rule like any request; the code sends them as a standalone post-handshake transact.

- **InsertNode (add a block) is sent BARE:** `insertNode`(34) takes **NO `nodeJsonRequest`(119) preamble** and NO `batchStatus`; confirm on **`nodeInserted`(33)**.
  Add-then-save = `insertNode`×N [+ a string `changeParameter`(12) for a UserIR `file` param] → `renameCurrentPreset`(13) → `saveCurrentPreset`(14).
  The device does NOT auto-push a fresh field-3 (`currentPresetDataChanged`) after an in-session insert, so VERIFY via `nodeInserted`(33) + a post-save field-8 slot read, not an in-session graph re-read.
  (If you do want to re-prompt the field-3 working-copy push, the trigger is `current_preset_data_request` — `PresetMessage` field **2**; `current_preset_info_request`, field **1**, is INERT for refresh.)
  No handler drops a correctly framed structural edit: each confirms or fails loudly, and a missing confirm is one still queued behind the load's pushes, a session lapse, a backup freeze or an insert abort (static RE, fw 1.8.58).
  So an unconfirmed insert is read back (`Session::insert_landed`) and re-sent only if it visibly did not land — a blind retry duplicates it.
  Companion entry point: `probe --insert-active <id> [--group][--after][--slot][--commit]` (no `--commit` = dry, inserts then reloads to discard).

Schema-level notes above still hold: addressing is by string id (`destGroup`+`nodeIdToReplace`), and `NodeReplaced.nodeJson` carries the full new-node JSON.
