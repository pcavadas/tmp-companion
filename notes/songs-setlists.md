# Songs & setlists (Songs tab)

The Songs tab is device-backed: the unit is the source of truth. It reads songs and setlists from the connected device, and every create/edit action is a read-back-after-write USB command.

## Reads

- `songMessage` list reads reply **in-burst with a top-level `batchStatus`** (preset/list reads are batch-bearing; setters omit `batchStatus`).
- Song/setlist list rows are positional (1-based index in the read list).

## Writes (the SongMessage family)

- Songs: add / rename / remove / set notes; per-song BPM.
- Setlists: add / rename / remove; add / remove / move a song within a setlist.
- `addSetlistSong` / `removeSetlistSong` / `moveSetlistSong` address a song by its **1-based position within the setlist** (not the global song slot); `moveSetlistSong` is an array splice.
- Per-song BPM has no dedicated setter — it is `SettingsMessage.tapTempoBpm{value, originatorId=1}` applied to the **active** song, which requires the song to have a footswitch (`assignSongPreset`) and then be activated via `loadPreset{tabEnum=5, songSlot, songPresetSlot, presetSlot}`.

## Positional slot behaviour (load-bearing)

A new song **inserts at protocol list slot 1 and shifts every other song +1** (insert, not append), and shows second-to-last on the device screen (protocol order ≠ display order). So resolve a just-created song **by name**, and treat any cached song-slot membership as invalidated by any song add/remove. A new setlist appends at the end.

## Song-link safety

Song rows bind to a preset by slot + identity; overwriting a bound slot with a different-identity preset empties the row. See `write-safety.md`.
