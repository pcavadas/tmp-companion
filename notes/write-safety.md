# Preset read/write & safety

## Reading a preset

A USB read is complete when it ends in its final `0x35` frame (HW + static RE, fw 1.8.58); a lapse cuts it. Write paths still build from the OFFLINE `.preset` file or the device backup; USB reads serve live state (active preset, scene names, footswitch tags).

- The active preset's JSON arrives as `currentPresetDataChanged` (field 3), the reply to `currentPresetDataRequest` (PresetMessage **field 2**). `currentPresetInfoRequest` (**field 1**) is answered with `presetLoaded` (field 11) then `currentPresetInfoChanged` (field 22, carrying `isDirty`); it does NOT trigger field 3.
- A slot-addressed read (`presetDataRequest` (field 8) → `presetDataChanged` (field 9), plaintext) returns the stored row, not the working copy.

**Post-edit reads.** After a LIVE structural edit (`insertNode` / `replaceNode` / `removeNode`) the device pushes no fresh field 3 (static RE + HW, fw 1.8.58), so a session's BUFFERED document does not reflect the edit. A field-2 re-prompt on that same session (`Session::live_preset_value`: cleared buffer + field 2) answers with the post-edit working copy (HW, `probe --reprompt-map`). Confirm a live edit landed via its acknowledgement (`nodeInserted` / `nodeReplaced` / `nodeRemoved`); verify placement via the re-prompt or the post-save field-8 read.

## Writing a preset

- LIVE setters are single-packet and carry **no `batchStatus`** (`notes/protocol.md`); the import framing (`importPresetRequest`, `LZ4(raw .preset bytes)`) is in `notes/gotchas.md`'s live-setters entry.
- An import always lands in the lowest empty user slot, and `importPresetResponse` (118) did not arrive within 5 s (HW, fw 1.8.58) — never wait on it; prove the landing from the stored row (`notes/gotchas.md`'s save-durability entry).

## Slot addressing

The device userSlot is **list index + 1**, owned by `session/preset.rs`; confirm a slot mapping with a non-destructive read before any destructive op, with the guard in the same address space as the mutation (`danger.md`).

## Identity safety (song links)

Song rows follow the SLOT. Overwriting a song-bound slot with a **different-identity** preset empties the song row. Link-safe editing must preserve `info.preset_id` and the scene structure — either a LIVE in-place edit (`changeParameter` / live node edit, then `saveCurrentPreset`) or an identity-preserving OFFLINE re-import. An in-place save keeps the song link, though every save re-mints `preset_id` (static RE + HW, fw 1.8.58).

## Block identity (per-instance node ids)

- A group keeps its nodes in a map keyed by node id, so ids are unique. Each insert or replace mints the FenderId, then the first free `<FenderId>_1`, `_2` … in the group (a replace frees the old id first, so replacing X with X keeps "X"); two blocks of one model never share an id (static RE + HW, fw 1.8.58). A LOAD keeps the stored ids as they are (`cab1` for an `ACD_CabSimTMS` in the scenario fixtures), and a stored duplicate id is not renamed: the second node overwrites the first's map entry, the first stays in the processing list unaddressed, and a field-3 read lists the id once (static RE, fw 1.8.58). The `insertNode` anchor (field 2, `nodeIdInsertLocation`) is looked up in that map: a miss gets no reply and aborts the server.
- Copy follows this because a miss is fatal: `diffToOps` names each anchor by its original `nodeId` or an earlier insert's key, and `copy_apply` resolves it to the device's current id from the `nodeJson` of the `nodeInserted` / `nodeReplaced` confirms (field 3 of both; `nodeReplaced.replacedNodeId` echoes the OLD id), or from a working-copy read when a confirm carries none. An anchor it cannot resolve is refused before anything is sent.
- An IR/saved insert into a group already holding that model is still refused (its follow-up addresses the new node by FenderId when no confirm id is known), and the read-back roster compares per-group MULTISETS.
- The op-list is emitted `removes → replaces → inserts`, inserts **right-to-left**, so each insert's anchor is still present when it lands. This is what makes "insert A before B, insert C after B, then remove B → exactly `[A, C]`" correct: the inserts anchor on the surviving siblings in the FINAL graph, never on the removed B. (Locked by `copyModel.test.ts` "INV-A".)

## Backup / restore

Pre-edit backups capture the original preset; restore re-imports it in place. Saving permanently alters a preset and cannot be undone from the app, so a new leveling run and a Copy save each sit behind the "I've backed up with Pro Control" acknowledgment (`ui/BackupAckLabel` — the leveling Set-up footer and the Copy Place blocks footer; Doctor never writes), and that backup is the only way back. The one exception is the leveling Summary's "Try again", which re-runs sounds of the same flow straight away on the first run's settings — no Set up, so no second acknowledgment (`levelRunStore.retryLeveling`) — and still saves. A leveling run saves each sound automatically as soon as it matches; there is no separate save opt-in (`danger.md`).

## Preset-schema fidelity (the "newer firmware revision" banner)

The unit shows **"this preset was created using a newer firmware revision"** for a preset whose JSON is MISSING top-level keys every real preset carries, even when `info.version` and every block's `since` match the connected firmware (HW). Any write/import path that composes preset JSON (rather than round-tripping a real preset) must carry the FULL top-level key set of a real export: `ampControl`, `audioGraph`, `bpm`, `exp`, `expAutoOffTime`, `ftsw`, `fxLoop`, `info`, `inputImpedanceMode`, `lastLoadedScene`, `onLoadMidiMsgs`, `outputMixerSettings`, `presetFootswitchColorActive`, `presetFootswitchColorInactive`, `scenes` (a real preset has no top-level `ftswStates`). The e2e scenario fixtures carry the full key set. Separately, a `version` ahead of the firmware makes the load substitute the EMPTY preset with `presetError` 6 (static RE, fw 1.8.58).
