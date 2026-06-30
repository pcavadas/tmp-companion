# Preset read/write & safety

## Reading a preset

A complete preset cannot be read back reliably over USB — every USB read is a device-truncated partial:

- The active preset's JSON arrives as `currentPresetDataChanged` (a partial; on a healthy dense-heartbeat session it includes the `ftsw` map and scene names, truncating only at the final scene). The field-3 push is triggered by `currentPresetDataRequest` (PresetMessage **field 2**); `currentPresetInfoRequest` (**field 1**) is a no-op dummy that does NOT trigger it.
- A slot-addressed read (`presetDataRequest` → `presetDataChanged`, plaintext) is also a per-slot partial.

So the **canonical full-preset source is the OFFLINE `.preset` file**. The USB partials are used for live state (active preset, scene names, footswitch tags).

**Post-edit reads (HW fw 1.8.45):** after a LIVE structural edit (`insertNode` / `replaceNode` / `removeNode`) on a lean held session, the device does NOT auto-push a fresh `currentPresetDataChanged`, so the held session's working copy does NOT reflect the edit — re-prompting field-3 (even via the correct field-2 request) returns the pre-edit graph. To read the post-edit block ORDER, save and read it back via a field-8 slot read (or read off the dense-heartbeat monitor session), not an in-session graph re-read. Confirm a live edit landed via its acknowledgement (`nodeInserted` / `nodeReplaced` / `nodeRemoved`), and verify placement via the post-save field-8 read.

## Writing a preset

- LIVE setters are single-packet and carry **no `batchStatus`** (only requests do): `setPresetLevel`, `setReAmpMode`, `loadPreset`, `loadScene`, `renameCurrentPreset`, `saveCurrentPreset`, `moveUserPreset`, `clearUserPreset`, the song/setlist writes, and the live block edits. A setter sent with a `batchStatus` is silently ignored.
- A full-preset re-import is `importPresetRequest` where the payload is `LZ4(raw .preset bytes)`; multi-packet framing is `0x33` start / `0x34` continue / `0x35` final.

## Slot addressing

`list_my_presets` is 0-based; the device userSlot is **list index + 1**. `session.rs` owns this translation — callers pass a 0-based list index and the slot-addressed setters (`loadPreset`, `saveCurrentPreset`, `clearUserPreset`, `moveUserPreset`) send `+1`. Before any destructive op keyed on a slot mapping, confirm the mapping with a non-destructive read first, and put the guard in the same address space as the mutation.

## Identity safety (song links)

Overwriting a song-bound slot with a **different-identity** preset empties the song row. Link-safe editing must preserve `info.preset_id` and the scene structure — either a LIVE in-place edit (`changeParameter` / live node edit, then `saveCurrentPreset`) or an identity-preserving OFFLINE re-import. An in-place save keeps the song link even if it re-stamps `preset_id`.

## Block identity (no per-instance node id)

On the real unit a block's `nodeId` **equals** its FenderId (model id) — there is no per-instance handle distinct from the model. Consequences for the Copy/edit op-list (`copyModel.diffToOps`):

- A single device group can never hold **two blocks of the same model** — they'd be indistinguishable on the wire — so that state is unrepresentable, not merely unsupported.
- Anchoring an insert by FenderId (`insertNode` field-2 = "before this node") is therefore **unambiguous and sufficient**; there's no need for a per-instance anchor.
- The op-list is emitted `removes → replaces → inserts`, inserts **right-to-left**, so each insert's anchor is still present when it lands. This is what makes "insert A before B, insert C after B, then remove B → exactly `[A, C]`" correct: the inserts anchor on the surviving siblings in the FINAL graph, never on the removed B. (Locked by `copyModel.test.ts` "INV-A".)

## Backup / restore

Pre-edit backups capture the original preset; restore re-imports it in place. Saving permanently alters a preset, so every write path that persists is opt-in.
