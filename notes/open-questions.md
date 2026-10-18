# Open questions

Device questions whose answer would change what TMP Companion does or says.
Each entry gives the question, why it matters, what would answer it, and the code that depends on it.
When one closes, delete its entry and write the answer as a plain fact in the note that owns the subject ([`README.md`](README.md)).
Link an entry as `notes/open-questions.md#<id>`; the headings are stable ids.
Facts are for firmware 1.8.58 unless a line says otherwise.

## Stale load echo

Why can the first connection after a load echo the slot loaded before it?
A fresh connection opened ~0.3–1 s after `load_and_prove`'s load got a `presetLoaded` (field 11) echo naming the previous slot, although the load had landed; a connection 16 s later named the new slot [HW] ([device/presets.md#echo-of-the-previous-slot](device/presets.md#echo-of-the-previous-slot)).
Two causes fit: the load is still running when the next connection handshakes, or the handshake echo reports state the unit has not updated yet.

- **Why it matters:** it sizes the retry in `Session::connect_confirmed` and decides whether a wait after the load could replace the lapse-and-retry.
- **What would answer it (HW, scratch slot, no save):** a timing sweep of load → lapse → reconnect at several gaps, logging the echo and the field-3 `info.displayName` each time.
- **Code:** `Session::connect_confirmed`, `Session::active_matches`, `replace_inplace::load_and_prove`.

## Cabinet cap weights

Which blocks does fw 1.8.58's cabinet cap count as 2?
The unit's UI counts combos, half-stacks, bass amps, cabinets and IR blocks, exempts heads and `ACD_ExternalCab`, and adds 1 for a node with a per-node flag set [RE], likely its second cab or second mic [RE, likely].
`blockcaps.rs` counts `ACD_ExternalCab` and weights 2 only a standalone `ACD_CabSimTMS` with `cabsim2enabled` ([device/presets.md#block-caps](device/presets.md#block-caps)).

- **Why it matters:** `blockcaps.rs` is the only cap enforcement, so a wrong weight lets a third cab through or refuses a legal edit.
- **What would answer it (HW, Pro Control closed, no save):** on the unit's own screen, try to add a second cab block to a preset holding an amp with its second mic on, and to one holding `ACD_ExternalCab` and one cab block; note which add the UI refuses.
- **Code:** `blockcaps.rs` (`Counts`, `roster_from_preset`), `src/models/block-classification.json`, `validateBlockEdit.ts`.
