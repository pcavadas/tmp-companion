# ReplaceNode family — per-slot signal-chain mutation

TMS exposes a small family of messages for replacing a single block in the signal chain without reloading the whole preset. These map directly onto the `audioGraph.guitarNodes.G{1,2,…}[]` arrays in the decoded `presetJson` and are the cleanest known protocol-level lever for mutating the chain one slot at a time.

This reference documents the block-mutation wire API. The `replaceNode` / `insertNode` / `removeNode` family is the SHIPPED mechanism behind the companion's Copy feature (`copyModel.ts` → `copy_apply` → `audiograph.rs`).

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

**Semantics:** swap the node at `destGroup[nodeIdToReplace]` for a fresh instance of model `fenderId`. The slot index is implicit — the array slot of the node currently bearing `nodeIdToReplace`. There is no explicit `index` field; identity comes from `nodeIdToReplace`. `replaceNode` carries ONLY the model id — it installs a fresh default-param instance and does NOT transfer the source block's parameter values; a same-model replace is therefore a model re-stamp, not a settings copy (HW fw 1.8.45).

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

**Semantics:** like `ReplaceNode` but installs a saved **block preset** (a block with all parameters preserved) rather than a default-parameter model. `index` is the index in the block-preset library (see `RequestAllBlockPresets`/`AllBlockPresetsResponse`, fields 135-136, for the library contents).

### `NodeReplaced.proto`

```proto
message NodeReplaced {
  string destGroup = 1;          // echoes the request's destGroup
  string replacedNodeId = 2;     // the NEW node's ID (not the one that got replaced)
  string nodeJson = 3;           // full JSON of the new node (model, params, defaults)
}
```

Surfaced as `PresetMessage.nodeReplaced = 40` (`PresetMessage.proto:179`).

**Semantics:** server → host event emitted after a successful `ReplaceNode` or `ReplaceNodeWithBlock`. Carries the **full JSON** of the new node — everything needed to render the new block (model name, parameter values, defaults, display strings) without a follow-up read.

### `UndoReplaceNode.proto`

```proto
message UndoReplaceNode {
  string originalNodeJson = 1;     // full JSON of the node that USED to be there
  string groupId = 2;
  string currentNodeIdToReplace = 3;  // the current (post-Replace) node ID
}
```

Surfaced as `PresetMessage.undoReplaceNode = 121` (`PresetMessage.proto:260`).

**Semantics:** **stateless undo.** The caller carries the pre-replacement node JSON; the server simply re-installs it. There is no server-side undo stack — every undo round-trips the full payload. This is markedly different from a transactional API and has implications:

- The host (Pro Control) maintains its own undo stack, not the device.
- `originalNodeJson` is the same shape as `NodeReplaced.nodeJson` — a host can simply cache the _pre_-replace `NodeReplaced` payload (from the prior load) and replay it.
- Undo is no different from a forward `ReplaceNode` from the device's perspective; it produces another `NodeReplaced` event.

### `UndoReplaceNodeResponse.proto`

```proto
message UndoReplaceNodeResponse {
  string replacedNodeId = 1;     // ID of the node now in place (re-restored)
  string originalNodeId = 2;     // ID of the node that was undone
  string groupId = 3;
}
```

Surfaced as `PresetMessage.undoReplaceNodeResponse = 122` (`PresetMessage.proto:261`).

**Semantics:** informational ACK of the undo. The host can pair this with its own undo-stack pop. Notably **does not** carry `nodeJson` — the restored node is fully described by the `originalNodeJson` the caller already sent. Server is just confirming "yes, applied."

## Identifier semantics

- Addressing is **by string ID**, not by numeric slot index. `destGroup` is the group key ("G1", "G2", …); `nodeIdToReplace` is the per-node ID.
- This matches the `presetJson` shape: `audioGraph.guitarNodes.G1[].nodeId`. The host can find a node by walking the JSON array; it does not need to track wire-level slot indices.
- One consequence: replacing a node **does not change other nodes' IDs**. A host can hold a stable reference to a downstream node across a `ReplaceNode` on an upstream one.

## Comparison to Mustang LT

LT's [`ReplaceNode.proto` + `ReplaceNodeStatus.proto`](https://github.com/brentmaxwell/LtAmp/tree/main/Schema/protobuf) pair is the LT-side analogue. TMS extends it in three ways:

1. **Adds `ReplaceNodeWithBlock`** for installing saved block presets (LT has no block-preset library exposed at this level).
2. **Adds `UndoReplaceNode` + `UndoReplaceNodeResponse`** for stateless undo (LT has no undo at the protocol layer).
3. **Returns `NodeReplaced` (with full `nodeJson`)** rather than LT's `ReplaceNodeStatus` (which carries only ack flags). TMS gives the host enough to update its model without a follow-up `RetrievePreset`/`PresetJSONMessage` round-trip.

## InsertNode / NodeInserted — ADD a block to the chain (field 34, HW-confirmed fw 1.8.45, 2026-06-15)

The structural-edit counterpart that ADDS a block (the messages above only replace/remove). Captured byte-exact from a Pro Control add-block session (append / insert-before / the UserIR `changeParameter`).

### `InsertNode.proto`

```proto
message InsertNode {
  string groupId = 1;                 // group KEY, e.g. "G1" (NOT the graph name "guitarNodes")
  string nodeIdInsertLocation = 2;    // OPTIONAL: omit = APPEND to end of group; set = a SAME-group FenderId to insert BEFORE
  string fenderId = 3;                // new block model ID
}
```

Surfaced as `PresetMessage.insertNode = 34`; the device confirms with `PresetMessage.nodeInserted = 33` (parallel to `nodeReplaced`/40). There is also `InsertNodeAtBlockIndex` (field 99, adds `index = 4`) but **Pro Control does NOT use it for add-block — it sends the bare field 34** (capture-confirmed; the earlier field-99 guess was wrong). On fw 1.8.45, field-99 `index = 0` does NOT prepend — it lands the block AFTER the group head — so it is excluded from the Copy save and kept only as the `probe --insert-map --at-index` dev tool. `groupId` is the group KEY, exactly like `replaceNode.destGroup`.

**Semantics:** insert a fresh `fenderId` instance into `groupId`; the device fills the new model's default parameters. Omit field 2 to append at the end of the group, or set it to an existing node's FenderId to insert immediately **BEFORE** that node (HW-verified fw 1.8.45 — the "insert after" reading of the capture was a MISREAD that mis-placed every Copy insert until corrected). The field-2 anchor MUST name a node in the **SAME group** as the insert — a visual signal-chain series can span device groups (e.g. amp in G1, pedals in G4), and an anchor in a different group makes the device silently DROP the insert. So `diffToOps` anchors each insert before the next SAME-group block, else appends.

## Live edit sequence — HW-validated on a real TMP (fw 1.8.45, 2026-06-12/13)

This family is the SHIPPED mechanism behind the companion's live block editing. **Two consumers** (`apps/tmp-companion`): `bulk_replace_live` (one replace across a selection — the Bulk Block Edit WIZARD UI was deleted on branch `worktree-copy-feature`, but the command stays registered) and **`copy_apply`** (the Copy tab — an ordered replace/insert/remove op list PER target preset, same held-session + confirm-gate machinery). The load-bearing wire facts (each cost a debugging round):

- **Two-opposite framing rules coexist:** structural MUTATIONS (`replaceNode`=39, `removeNode`=35, `replaceNodeWithBlock`=100) carry **NO `batchStatus`** (field 10); request/response messages **REQUIRE** it. Wrong choice EITHER way = the device SILENTLY DROPS the message (empty `connectionError` ack, no error). Mirror Pro Control.
- **Pro Control's persist sequence (per node):** `nodeJsonRequest`(119) edit-context preamble → the no-batch mutation → `renameCurrentPreset`(13) → `saveCurrentPreset`(14). The rename is structural (preserves identity); **save alone does not persist.** Without the 119 preamble fw 1.8.45 drops the mutation.
- **Confirm/reject gate (a wrong-content save corrupted a real preset):** save ONLY after `nodeReplaced`(40) / `nodeRemoved`(36); a `presetError`(53) means the device REJECTED the edit → ABORT, never save (you'd persist the wrong active preset's graph). A `presetError` can also mean an **invalid model id** — a `fenderId` the firmware doesn't ship rejects (e.g. `ACD_Klon` is NOT valid on fw 1.8.45); use a FenderId read from a live node, not a guessed catalog/codename id. The session must be ATTACHED to the target preset — verify by the `PresetLoaded` slot echo (identity), NOT display-name equality (duplicate names + a load-that-didn't-take → wrong-preset edit).
- **60-byte single-report limit:** a `replaceNode` body with a long id (e.g. a `…CabIR` node ≈ 61 B) overflows the single HID report — MUST use the chunked `0x33/0x34/0x35` framing (`transact_chunked`); the single-report path panics on long ids.
- **User IR replacement = two edits:** `replaceNode`→`ACD_UserIRTMS`, then a STRING `changeParameter` (the `value` oneof's **`stringVal`, field 6**, not floatVal/5) setting the new node's `file` param to the chosen IR filename; verify by re-reading `nodeJsonResponse`(120).`nodeJsonString` before saving.
- **`replaceNodeWithBlock`(100)** installs a user SAVED block / dual cab by its **library index** within `blockPresetsMap` (params are device-internal-by-index — there is no offline payload; faithful insert is live-only). NOTE: `blockPresetsMap` is DOMINATED by device-default entries (HW: 464 total → 366 defaults, named `"<Model> - N"` / `"Factory Default"`, carrying only `name`+`favorite` — no structural `isDefault` flag), so a "user saved blocks" picker must filter by name heuristic or it shows hundreds of factory defaults; but the `index` still spans the WHOLE unfiltered map, so compute it against the full map, not the filtered view.
- **Discovery RPCs are dropped in-burst:** `RequestAllBlockPresets`(135)→`AllBlockPresetsResponse`(136) and `UserIRListRequest` answer ONLY as a standalone post-handshake `send_and_collect` transact, NOT an in-burst poll.

- **InsertNode (add a block) is sent BARE — the OPPOSITE of replace's preamble:** `insertNode`(34) takes **NO `nodeJsonRequest`(119) preamble** (mirroring replace's preamble made the device REJECT the insert) and NO `batchStatus`; confirm on **`nodeInserted`(33)** — HW-verified 2026-06-15 (the prior inbound capture caught nothing, so field 33 was spec-only before). Add-then-save = `insertNode`×N [+ a string `changeParameter`(12) for a UserIR `file` param] → `renameCurrentPreset`(13) → `saveCurrentPreset`(14). The device does NOT auto-push a fresh field-3 (`currentPresetDataChanged`) after an in-session insert, so VERIFY via `nodeInserted`(33) + a post-save field-8 slot read, not an in-session graph re-read. (If you do want to re-prompt the field-3 working-copy push, the trigger is `current_preset_data_request` — `PresetMessage` field **2**; `current_preset_info_request`, field **1**, is INERT for refresh.) The "first structural edit after a fresh load can be silently dropped, an immediate retry lands it" rule applies — retry once on a drop (NOT on a `presetError`). The earlier "structural-edit reliability wall" was a STALE heavily-cycled-HID-session artifact, not the protocol/device: a fresh process + the held load→re-arm→insert path landed it first try. Companion entry point: `probe --insert-active <id> [--group][--after][--slot][--commit]` (no `--commit` = dry, inserts then reloads to discard).

Schema-level notes above still hold: addressing is by string id (`destGroup`+`nodeIdToReplace`), and `NodeReplaced.nodeJson` carries the full new-node JSON.
