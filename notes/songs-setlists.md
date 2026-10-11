# Songs & setlists (Songs tab)

The Songs tab is device-backed: the unit is the source of truth. It reads songs and setlists from the connected device, and every create/edit action is a read-back-after-write USB command.

## Reads

- `songMessage` list reads reply **in-burst with a top-level `batchStatus`** (preset/list reads are batch-bearing; setters omit `batchStatus`).
- Song/setlist list rows are positional (1-based index in the read list).

## Writes (the SongMessage family)

- Songs: add / rename / remove / set notes; per-song BPM.
- Setlists: add / rename / remove; add / remove / move a song within a setlist.
- `addSetlistSong` takes the **GLOBAL song slot** of the song to add; `removeSetlistSong` / `moveSetlistSong` address by **1-based position within the setlist** (`moveSetlistSong` is an array splice). See `commands/setlists.rs`.
- Per-song BPM has no dedicated setter — it is `SettingsMessage.tapTempoBpm{value, originatorId=1}` applied to the **active** song, which requires the song to have a footswitch (`assignSongPreset`) and then be activated via `loadPreset{tabEnum=5, songSlot, songPresetSlot, presetSlot}`. The tap lands only when that song's `bpmActive` is **already** on, so the order is load → `setSongBpmActive(true)` → `tapTempoBpm` → one read-back, on one connection (`probe_api::songs::set_song_bpm_on`). The batched saves (`create_song_full` / `update_song_full`) run every write, the BPM step and the read-backs on that same connection; the preset active at connect (`Session::active_slot_live`: the `PresetLoaded` echo, else a name unique in the My Presets list) is reloaded afterwards.
- `assignSongPreset`'s `presetSceneSlot` must be a scene the preset has, or 8 (`BASE_SCENE_SLOT`, the whole preset): the device refuses any other with SongError 5 and writes no row. `Session::assign_song_preset` reads the row back, so a refusal is an error.

## Positional slot behaviour (load-bearing)

A new song **inserts at protocol list slot 1 and shifts every other song +1** (insert, not append), and shows second-to-last on the device screen (protocol order ≠ display order). So resolve a just-created song **by name**, and treat any cached song-slot membership as invalidated by any song add/remove. A new setlist appends at the end.

## Song-link safety

Song rows bind to a preset by slot + identity; overwriting a bound slot with a different-identity preset empties the row. See `write-safety.md`.
