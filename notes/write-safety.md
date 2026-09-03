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

On the real unit a block's `nodeId` **equals** its FenderId (model id) — there is no per-instance handle distinct from the model. Consequences for the Copy/edit op-list (`copyModel.diffToOps`):

- A device group CAN hold **two blocks of the same model** (ONLINE `copy.spec.ts` 2026-09-03, fw 1.8.45: four `ACD_TubeScreamer` in G1 after chained inserts, confirmed by the working-copy re-prompt), but they are indistinguishable on the wire — so any op addressing one of them (a FenderId anchor, an IR/saved follow-up) hits whichever the device picks. Companion treats the state as unaddressable: `copy_apply` refuses an IR/saved insert into a group already holding that model, and the read-back roster compares per-group MULTISETS because the projected ORDER of a duplicate-anchored insert is not reliable.
- Anchoring an insert by FenderId (`insertNode` field-2 = "before this node") is therefore **unambiguous and sufficient**; there's no need for a per-instance anchor.
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
