# Structural edits on the device

Read this before any code that replaces, inserts or removes a block on the unit, or checks the block caps; an insert can abort the device server.

Facts are for firmware 1.8.58 unless a line says otherwise.

## Edit ops

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
A confirm sent right after a load waits behind the load's field-3 push in the [HID reply queue](session.md#hid-reply-queue): a `nodeReplaced` took about 0.17 s on a 3-block preset, and more on a larger one [HW+RE].
Sent after `presetLoaded` (11), `nodeReplaced` arrived in about 30 ms, `nodeInserted` in about 20 ms and the save's `presetChanged` (25) in 40–65 ms [HW].
Read an unconfirmed edit back (`Session::insert_landed`, `read_working_copy`) and re-send it only when it visibly did not land; a blind re-send duplicates an insert or names a replaced node that is gone.
The unit pushes no field 3 after a structural edit; a field-2 re-prompt on the same session (`Session::live_preset_value`) answers with the post-edit working copy [HW+RE], and [Copy](../copy.md) builds its read-back on it.

### Node ids

A group keeps its nodes in a map keyed by node id [RE].
An insert or replace mints the model's FenderId, then the first free `<FenderId>_1`, `_2`… in the group (`audiograph::mint_node_id`); a replace frees the old id first, so replacing X with X keeps "X" [HW+RE].
`nodeInserted` and `nodeReplaced` carry the new node, with its minted id, in field 3; their field 2 echoes the anchor as sent or the old id [HW+RE].
A load keeps stored ids as they are (`cab1` in the scenario fixtures); a stored duplicate id overwrites the first node's map entry and reads back once [RE].
So never assume a FenderId is a node id: `copy_apply` resolves each anchor through `DeviceIds` (the confirms' `nodeJson` or a working-copy read) and refuses one it cannot resolve.

### Saves with a scene current

A save made while a non-base scene is current can store a different block chain than the live one, with no error [HW].
The firmware expects every node to have an entry in every scene; an insert seeds one and a replace re-keys it (static RE, fw 1.8.58).
A replace of a node that some non-current scene has no entry for still gets `nodeReplaced` and changes the live graph [HW], but leaves the scene state on the old node (static RE, fw 1.8.58), so a save made with a scene current stores the pre-edit block.
The unit itself makes such presets: one of 8 scene presets checked on a user's unit lacks a scene entry for one block [HW].
On a 3-scene preset loaded into scene 1, with the cab stored under the non-minted id `cab1` and no scene entry for it, these were lost [HW]:

- a saved-block replace (`replaceNodeWithBlock`), then save;
- a stock `replaceNode` of the same cab, then save;
- a stock `replaceNode` to re-mint the id, then the saved-block replace, then save.

In each case the stored copy kept the pre-edit node (a first save stored it as a bare `{bypass}` node), while the confirms and the working copy (fields 2/3) showed the edit.
After `loadScene(8)` (base), the same saved-block replace and save persisted the block with every parameter [HW].
So every structural save is proven against the stored copy: `save_proof` reads the slot back with field 8 and compares it with the working copy read just before the save, and a mismatch is an error, never "updated".

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
So an `insertNode` past the cabinet cap over USB is accepted with a `nodeInserted` [HW+RE], and the host check is the only guard.

The unit's block picker allows a row only while the chain's cab weight plus the candidate's stays at 2 or less, and the same check gates the "+ Add Mic" button [HW+RE]:

- A combo, half-stack, bass amp, cabinet (`ACD_CabSimTMS` or a standalone cab model such as `ACD_Diezel412FV`) or IR block weighs 1, plus 1 when its second cab or mic (`cabsim2enabled`) is on [HW+RE].
- Heads, effects and `ACD_ExternalCab` weigh 0 [HW+RE].
- A saved block installed by `replaceNodeWithBlock` (100) brings its own `cabsim2enabled` [HW].
  The saved-block listing's `dualCabsEnabled` is that flag for every block except `ACD_CabSimTMS`, which the listing always reports as dual (static RE, fw 1.8.58).

A chain already over the cap greys every picker row, effects included, except External Cabinet, until cab blocks are removed [HW].
`blockcaps.rs` reads the four caps and their membership sets from `src/models/block-classification.json`, copied from the unit's client.
Its `cabinetSet` also holds 7 amp+cab ids the client cannot classify, counted as the fail-closed choice ([provenance](../../src/models/block-classification.md#why-literal-extraction-not-acdcategory--regex)).
A `cabinetSet` member with `cabsim2enabled` on weighs 2, and every other member 1; a saved candidate weighs by its listing `dualCabsEnabled`, so a saved `ACD_CabSimTMS` always weighs 2 and a single-cab one next to another cab is refused although it fits.
