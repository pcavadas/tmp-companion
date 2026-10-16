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

The device accepts a duplicate song name, so only a UNIQUE match is the new song. When the name was already taken, `create_song_full` keeps the created song but skips its notes, BPM and setlist (they could land on the older song) and reports it in its own typed field, `duplicate_name`, apart from `bpm_warning`. The Songs tab picks its message from those fields, never from the warning text: "You already have a song called {name}." for the first, "Song saved, but its tempo didn't. Set it again." for the second (the warning text goes to the log).

A setlist delete shifts every later setlist down one slot. So every slot-keyed write carries the name the user acted on, and the backend re-reads the list in the same slot space and refuses on a mismatch, rather than writing to whatever now sits at that slot: `rename_setlist`, `remove_setlist`, `add_setlist_songs` (setlist and each song), `create_song_full`'s `add_to_setlist`, `update_song_full` and `remove_song`. Each also carries the whole list the UI saw (its names, in order), and refuses when the unit's list reads differently: names can repeat, so a delete plus an add can leave a stale slot holding another song or setlist with the same name in a list of the same size. The unit exposes no stable song or setlist id, so a change that restores the same names in the same order stays invisible. `remove_setlist_song` / `move_setlist_song` also carry the song slot at the position they address, and refuse if the position now holds another song. The guards read on the write's own connection (`commands/setlists.rs` "Slot guards"). A membership read has no guard, so a read queued behind a setlist delete re-addresses its setlist through that delete when it runs (`slotAfterDeletes`).

A setlist's membership read drops only the TRAILING `songSlot == 0` padding (`session::trim_setlist_padding`). A `0` between members stays at its position, and the UI shows it (and any slot that names no song) as an "Unknown song" row, because filtering it out would shift every later row onto the wrong device position. Whether the device ever sends a `0` between members is untested.

The Songs tab writes its post-edit lists back into the startup library-scan snapshot (`patchLibrarySongs`), because tabs unmount on switch and the next mount paints from that snapshot. A song add/remove marks the song↔preset join unknown (`invalidateSongPresetJoin`): the Presets axis shows "unknown" until a reconnect rescans, never a confident zero. Edits made before the scan settles are held and applied over its result, since its backup may predate them. Device ops run on a FIFO queue: an action taken while another op runs waits its turn instead of being dropped.

## Song-link safety

Song rows bind to a preset by slot + identity; overwriting a bound slot with a different-identity preset empties the row. See `write-safety.md`.
