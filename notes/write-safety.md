# Preset read/write & safety

## Reading a preset

A complete preset cannot be read back reliably over USB — every USB read is a device-truncated partial:

- The active preset's JSON arrives as `currentPresetDataChanged` (a partial; on a healthy dense-heartbeat session it includes the `ftsw` map and scene names, truncating only at the final scene). The field-3 push is triggered by `currentPresetDataRequest` (PresetMessage **field 2**); `currentPresetInfoRequest` (**field 1**) is a no-op dummy that does NOT trigger it.
- A slot-addressed read (`presetDataRequest` → `presetDataChanged`, plaintext) is also a per-slot partial.

So the **canonical full-preset source is the OFFLINE `.preset` file**. The USB partials are used for live state (active preset, scene names, footswitch tags).

**Post-edit reads (HW fw 1.8.45):** after a LIVE structural edit (`insertNode` / `replaceNode` / `removeNode`) on a held session the device does NOT auto-push a fresh `currentPresetDataChanged`, so the session's BUFFERED document does not reflect the edit — but a field-2 `currentPresetDataRequest` re-prompt on that same session DOES answer with the post-edit working copy (2026-09-03, `probe --reprompt-map` on Copy's exact held-session shape: 6/6 re-prompts showed the insert/remove in ~520 ms as a ~2.8 KB partial that still parses to a full `ActiveGraph` with its `template`, and both saves issued after a re-prompt persisted on a field-8 read-back). The earlier reading that the re-prompt returns the pre-edit graph came from the `--insert-map` DRY arm, whose re-prompt re-sent `connection_request` on the live session — the field-2 then goes unanswered (`carriers=[]` is what `TMP_PROBE_LEGACY_REPROMPT=1` reproduces today) — and read its buffer without clearing it; a buffer scrape of that shape is consistent with the old pre-edit reading, though the reading itself was not reproduced. A separate 2026-09-03 observation with an unresolved cause: a `list_my_presets` on the live session BEFORE the load left the whole load + re-arm unanswered (`fields=[]` for 10 s); `--insert-map` still carries that list call, Copy never sends it (the name comes from the job). Confirm a live edit landed via its acknowledgement (`nodeInserted` / `nodeReplaced` / `nodeRemoved`); verify placement via the re-prompt (`Session::live_preset_value`, cleared buffer + field 2) or the post-save field-8 read.

## Writing a preset

- LIVE setters are single-packet and carry **no `batchStatus`** (only requests do): `setPresetLevel`, `setReAmpMode`, `loadPreset`, `loadScene`, `renameCurrentPreset`, `saveCurrentPreset`, `moveUserPreset`, `clearUserPreset`, the song/setlist writes, and the live block edits. A setter sent with a `batchStatus` is silently ignored.
- A full-preset re-import is `importPresetRequest` where the payload is `LZ4(raw .preset bytes)`; multi-packet framing is `0x33` start / `0x34` continue / `0x35` final.

## Slot addressing

`list_my_presets` is 0-based; the device userSlot is **list index + 1**. `session.rs` owns this translation — callers pass a 0-based list index and the slot-addressed setters (`loadPreset`, `saveCurrentPreset`, `clearUserPreset`, `moveUserPreset`) send `+1`. Before any destructive op keyed on a slot mapping, confirm the mapping with a non-destructive read first, and put the guard in the same address space as the mutation.

## Identity safety (song links)

Overwriting a song-bound slot with a **different-identity** preset empties the song row. Link-safe editing must preserve `info.preset_id` and the scene structure — either a LIVE in-place edit (`changeParameter` / live node edit, then `saveCurrentPreset`) or an identity-preserving OFFLINE re-import. An in-place save keeps the song link even if it re-stamps `preset_id`.

## Block identity (no per-instance node id)

Node ids, as two sources describe them:

- **fw 1.8.45 HW:** a device group CAN hold **two blocks of the same model** (ONLINE `copy.spec.ts` 2026-09-03: four `ACD_TubeScreamer` in G1 after chained inserts, confirmed by the working-copy re-prompt), and they read back with `nodeId` == FenderId.
- **fw 1.8.58 static RE:** a group keeps its nodes in a map keyed by node id, so ids are unique. An insert or replace mints the new node's id from its FenderId, or the first free `<FenderId>_1`, `_2`… (a replace frees the old id first, so replacing X with X keeps "X"). A LOAD keeps the stored ids as they are (`cab1` for an `ACD_CabSimTMS` in the scenario fixtures), and a stored duplicate id is not renamed: the second node overwrites the first's map entry, the first stays in the processing list unaddressed, and a field-3 read lists the id once. The `insertNode` anchor (field 2, `nodeIdInsertLocation`) is looked up in that map: a miss gets no reply and aborts the server.
- The two disagree on duplicates: on 1.8.58 four inserts mint `ACD_TubeScreamer`, `_1`, `_2`, `_3`. 1.8.45 is not in the RE corpus, so the difference is a firmware change or a misread; a HW re-check on 1.8.58 is pending. Copy follows the RE because a miss is fatal: `diffToOps` names each anchor by its original `nodeId` or an earlier insert's key, and `copy_apply` resolves it to the device's current id from the `nodeJson` of the `nodeInserted` / `nodeReplaced` confirms (field 3 of both; `nodeReplaced.replacedNodeId` echoes the OLD id), or from a working-copy read when a confirm carries none. An anchor it cannot resolve is refused before anything is sent.
- An IR/saved insert into a group already holding that model is still refused (its follow-up addresses the new node by FenderId when no confirm id is known), and the read-back roster compares per-group MULTISETS.
- The op-list is emitted `removes → replaces → inserts`, inserts **right-to-left**, so each insert's anchor is still present when it lands. This is what makes "insert A before B, insert C after B, then remove B → exactly `[A, C]`" correct: the inserts anchor on the surviving siblings in the FINAL graph, never on the removed B. (Locked by `copyModel.test.ts` "INV-A".)

## Backup / restore

Pre-edit backups capture the original preset; restore re-imports it in place. Saving permanently alters a preset, so every write path that persists is opt-in.

## Preset-schema fidelity (the "newer firmware revision" banner)

The unit shows **"this preset was created using a newer firmware revision"** for a preset whose
JSON is MISSING top-level keys every real preset carries — reproduced 2026-07-19 on the e2e
scenario fixtures with `info.version` (`5.0`) and every block's `since` matching the connected
firmware, yet the banner still fired because the hand-built fixtures lack `ftswStates`,
`lastLoadedScene`, and `presetFootswitchColorActive`/`Inactive` (the plain Targets even lack
`scenes`). The firmware evidently treats an incomplete key set as "foreign/newer" in this
reproduced case; this doesn't establish that a version-field mismatch can never also trigger it —
that's untested here.
Any write/import path that composes preset JSON (rather than round-tripping a real preset) must
carry the FULL top-level key set of a real export; the reference set is any real `.preset` decode.
The 4 e2e fixtures still have this gap (cosmetic offline — SimDevice doesn't check).
