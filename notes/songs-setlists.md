# Songs and setlists

The Songs tab is device-backed: the unit is the source of truth.
It reads songs and setlists from the connected unit, and every create or edit is a USB write followed by a read-back.
Facts are for firmware 1.8.58 unless a line says otherwise.

## Reads

- Song and setlist list reads carry `batchStatus` 3 (`proto::BATCH_DRAIN`, `song_io.rs`) and are answered at once; setters carry none, and replies carry none ([device/session.md#request-batching](device/session.md#request-batching)).
- Song and setlist list rows are positional: the 1-based index in the read list.

## Writes

- Songs: add, rename, remove, set notes, and per-song BPM.
- Setlists: add, rename, remove, and add / remove / move a song within a setlist.
- `addSetlistSong` takes the global song slot of the song to add; `removeSetlistSong` and `moveSetlistSong` address a 1-based position within the setlist, and `moveSetlistSong` is an array splice (`commands/setlists.rs`).
- `assignSongPreset`'s `presetSceneSlot` must be a scene the preset has, or 8 (`BASE_SCENE_SLOT`, the whole preset); the unit refuses any other with SongError 5 and writes no row [RE].
  `Session::assign_song_preset` reads the row back, so a refusal is an error.

**Per-song BPM.**
There is no dedicated setter: the BPM is `SettingsMessage.tapTempoBpm{value, originatorId=1}` applied to the active song.
The song must have a footswitch (`assignSongPreset`) and be activated with `loadPreset{tabEnum=5, songSlot, songPresetSlot, presetSlot}`.
The tap lands only when that song's `bpmActive` is already on [HW+RE], so the order is load → `setSongBpmActive(true)` → `tapTempoBpm` → one read-back, on one connection (`song_io::set_song_bpm_on`).
The batched saves (`create_song_full`, `update_song_full`) run every write, the BPM step and the read-backs on that one connection.
The preset active at connect (`Session::active_slot_live`: the `PresetLoaded` echo, else a name unique in the My Presets list) is reloaded afterwards.

## Slot shifts

A new song inserts at protocol list slot 1 and shifts every other song +1, and it shows second-to-last on the unit's screen (protocol order is not display order) [HW].
So a just-created song is resolved by name, and any cached song-slot membership is invalid after a song add or remove.
A new setlist appends at the end [HW].

The unit accepts a duplicate song name, so only a unique match is the new song [HW].
When the name was already taken, `create_song_full` keeps the created song but skips its notes, BPM and setlist (they could land on the older song), and reports it in its typed `duplicate_name` field, apart from `bpm_warning`.
The Songs tab picks its message from those fields, never from the warning text: "You already have a song called {name}." for the first, "Song saved, but its tempo didn't. Set it again." for the second; the warning text goes to the log.

A setlist delete shifts every later setlist down one slot [HW].
So every slot-keyed write carries the name the user acted on, and the backend re-reads the list in the same slot space and refuses on a mismatch rather than writing to whatever now sits at that slot: `rename_setlist`, `remove_setlist`, `add_setlist_songs` (setlist and each song), `create_song_full`'s `add_to_setlist`, `update_song_full` and `remove_song`.
Each also carries the whole list the UI saw (its names, in order), and refuses when the unit's list reads differently, because names can repeat and a delete plus an add can leave a same-named row at a stale slot in a list of the same size.
The unit exposes no stable song or setlist id, so a change that restores the same names in the same order stays invisible.
`remove_setlist_song` and `move_setlist_song` also carry the song slot at the position they address, and refuse if the position now holds another song.
The guards read on the write's own connection (`commands/setlists.rs`, "Slot guards").
A membership read has no guard, so a read queued behind a setlist delete re-addresses its setlist through that delete when it runs (`slotAfterDeletes`, `views/songs/songUtil.ts`).

## Song delete cascade

Removing a song removes its song-preset rows and its entry in every setlist, closes each setlist's gap, and shifts every later song down one slot [RE].
The unit then pushes `songRemoved` (Song 11), `setlistSongRemoved` (Setlist 20) for the dropped entries and `setlistSongChanged` (Setlist 18) for the songs that moved [RE].
Never follow a song delete with `removeSetlistSong` for the same entries: it would remove the entry that moved into that position.

A setlist's membership read drops only the trailing `songSlot == 0` padding (`session::trim_setlist_padding`).
The unit closes every gap, so a `0` between members is not expected; the UI still shows one (and any slot that names no song) as an "Unknown song" row at its position, because filtering it out would shift every later row onto the wrong device position.

## Frontend state

The Songs tab writes its post-edit lists back into the startup library-scan snapshot (`patchLibrarySongs`, `device/libraryScan.ts`), because tabs unmount on switch and the next mount paints from that snapshot.
A song add or remove marks the song ↔ preset join unknown (`invalidateSongPresetJoin`): the Presets axis shows "unknown" until a reconnect rescans, never a confident zero.
Edits made before the scan settles are held and applied over its result, since its backup may predate them.
Device ops run on a FIFO queue: an action taken while another op runs waits its turn instead of being dropped.

## Song links

What a song row binds to, why a preset move carries it, and which edits keep it are in [device/presets.md#song-links](device/presets.md#song-links).
