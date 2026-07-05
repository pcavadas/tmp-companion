//! Probe entry points: Setlists CRUD + shared setlist-read helpers (used by the setlist commands).

use crate::session::Session;
use crate::proto;
use crate::session;
use super::songs::find_song_slot;
use super::songs::read_song_list;

/// Read every Setlist's name — the net-new live `setlistListResponse` read. Same
/// in-burst + `batchStatus`-sweep contract as [`read_song_list`] (setlist reads ride
/// the handshake burst). A `SetlistListRecord` is name-only; per-setlist song
/// membership is a separate read. Empty Vec = no setlists defined.
pub(crate) fn read_setlist_list() -> Result<Vec<session::SetlistRecord>, String> {
    // Fail-closed, like read_song_list: accept only a strictly-complete
    // setlistListResponse, retrying until one lands (the multi-packet response
    // truncates non-deterministically, worse right after a write).
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::setlist_list_request(Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlists_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err("could not read a complete setlist list (multi-packet response kept truncating)".into())
}

/// Probe (`--setlists`): list every Setlist on the device (name only — per-setlist
/// song membership is a separate read). Read-only; prints one line per setlist.
pub fn probe_list_setlists() -> Result<String, String> {
    let setlists = read_setlist_list()?;
    if setlists.is_empty() {
        return Ok("[probe --setlists] no setlists on the device \
                   (none defined, or setlistListResponse(3) unsupported on this firmware)\n"
            .to_string());
    }
    let mut out = format!("[probe --setlists] {} setlist(s):\n", setlists.len());
    for l in &setlists {
        out += &format!("  {:>2}. {}\n", l.slot, l.name);
    }
    Ok(out)
}

/// Read the RAW song slots a Setlist contains — `setlistSongListResponse` records,
/// each a `songSlot`, INCLUDING trailing `songSlot==0` padding (unassigned slots).
/// Same in-burst + `batchStatus`-sweep contract as the other song/setlist reads.
/// Most callers want [`read_setlist_songs`] (dense); only the HW-characterization
/// probe needs the raw padding to count empties.
pub(crate) fn read_setlist_songs_raw(setlist_slot: u32) -> Result<Vec<u32>, String> {
    // Fail-closed strict read; a complete response for an empty setlist is Some(vec![]).
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::setlist_song_list_request(setlist_slot as u64, Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlist_songs_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err(format!(
        "could not read a complete song list for setlist slot {setlist_slot}"
    ))
}

/// A setlist's member song slots in device order, DENSE — trailing `songSlot==0`
/// padding (unassigned slots) dropped. This is the membership every command + the UI
/// consume, so the "songSlot 0 = not a member" invariant lives HERE, once, rather
/// than re-applied at each call site. Empty Vec = an empty setlist.
pub(crate) fn read_setlist_songs(setlist_slot: u32) -> Result<Vec<u32>, String> {
    Ok(read_setlist_songs_raw(setlist_slot)?
        .into_iter()
        .filter(|&s| s != 0)
        .collect())
}

/// Probe (`--setlist-songs`): for every Setlist on the device, list the songs it
/// contains, resolving each referenced `songSlot` against the song list. Read-only.
/// Each line shows the raw `songSlot` next to the resolved name so any slot-base
/// mismatch is self-evident rather than silently mislabeled.
pub fn probe_setlist_songs() -> Result<String, String> {
    let songs = read_song_list()?;
    let setlists = read_setlist_list()?;
    if setlists.is_empty() {
        return Ok("[probe --setlist-songs] no setlists on the device\n".to_string());
    }
    let name_for = |slot: u32| -> String {
        songs
            .iter()
            .find(|s| s.slot == slot)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("(unknown song slot {slot})"))
    };
    let mut out = format!("[probe --setlist-songs] {} setlist(s):\n", setlists.len());
    for l in &setlists {
        // songSlot is 1-based (matches the song list); a returned songSlot==0 is a
        // trailing empty/unassigned slot, not a song — count + label it separately.
        // This diagnostic wants the RAW list to report the padding count; everyone
        // else uses the dense `read_setlist_songs`.
        let raw = read_setlist_songs_raw(l.slot)?;
        let real: Vec<u32> = raw.iter().copied().filter(|&s| s != 0).collect();
        let empties = raw.len() - real.len();
        out += &format!("\n  {}. {}  ({} song(s))\n", l.slot, l.name, real.len());
        if real.is_empty() {
            out += "     (empty)\n";
        } else {
            for (i, slot) in real.iter().enumerate() {
                out += &format!(
                    "     {:>2}. [songSlot {}] {}\n",
                    i + 1,
                    slot,
                    name_for(*slot)
                );
            }
        }
        if empties > 0 {
            out += &format!("     (+ {empties} empty/unassigned slot(s) returned as songSlot 0)\n");
        }
    }
    Ok(out)
}

/// Resolve a setlist's 1-based slot by exact name. Errors on no-match or ambiguity.
fn find_setlist_slot(setlists: &[session::SetlistRecord], name: &str) -> Result<u32, String> {
    let hits: Vec<u32> = setlists
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.slot)
        .collect();
    match hits.len() {
        0 => Err(format!("no setlist named {name:?} on the device")),
        1 => Ok(hits[0]),
        n => Err(format!(
            "{n} setlists named {name:?} — ambiguous, refusing to mutate"
        )),
    }
}

/// `--add-setlist <name>` — create a setlist, verify it appears.
///
/// `addSetlist` works from a bare connection (the earlier "failures" were a
/// truncating multi-packet READ, not a write problem — see [`read_setlist_list`]).
pub fn probe_add_setlist(name: &str) -> Result<String, String> {
    {
        let mut s = Session::connect()?;
        s.add_setlist(name)?;
    }
    let setlists = read_setlist_list()?;
    let found = setlists.iter().find(|x| x.name == name);
    Ok(format!(
        "[probe --add-setlist] add {name:?} → {} ({} setlists total)\n",
        if found.is_some() {
            "FOUND ✓"
        } else {
            "NOT FOUND ✗"
        },
        setlists.len()
    ))
}

/// `--add-setlist-song <setlist-name> <song-name>` — add the song to the setlist
/// (by resolved slots), verify membership.
pub fn probe_add_setlist_song(setlist_name: &str, song_name: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let setlists = read_setlist_list()?;
    let song_slot = find_song_slot(&songs, song_name)?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    {
        let mut s = Session::connect()?;
        s.add_setlist_song(setlist_slot, song_slot)?;
    }
    let members = read_setlist_songs(setlist_slot)?;
    let present = members.contains(&song_slot);
    Ok(format!(
        "[probe --add-setlist-song] {song_name:?} (songSlot {song_slot}) → {setlist_name:?} (setlistSlot {setlist_slot}) → {}\n  members now: {:?}\n",
        if present { "PRESENT ✓" } else { "NOT PRESENT ✗" }, members
    ))
}

/// `--rename-setlist <old> <new>` — rename (resolved by old name), verify.
pub fn probe_rename_setlist(old: &str, new: &str) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let slot = find_setlist_slot(&setlists, old)?;
    {
        let mut s = Session::connect()?;
        s.rename_setlist(slot, new)?;
    }
    let after = read_setlist_list()?;
    let ok = after.iter().any(|x| x.slot == slot && x.name == new);
    Ok(format!(
        "[probe --rename-setlist] slot {slot} {old:?} → {new:?}: {} (now: {:?})\n",
        if ok { "OK ✓" } else { "FAILED ✗" },
        after.iter().map(|x| x.name.clone()).collect::<Vec<_>>()
    ))
}

/// `--remove-setlists-named <name>` — DELETE every setlist with this exact name
/// (handles duplicates: find first match → remove its slot → re-read → repeat).
pub fn probe_remove_setlists_named(name: &str) -> Result<String, String> {
    let mut removed = 0;
    loop {
        let setlists = read_setlist_list()?;
        let Some(rec) = setlists.iter().find(|x| x.name == name) else {
            break;
        };
        let slot = rec.slot;
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        removed += 1;
        if removed > 30 {
            break; // safety bound
        }
    }
    let after = read_setlist_list()?;
    Ok(format!(
        "[probe --remove-setlists-named] removed {removed} setlist(s) named {name:?}; {} setlists remain\n",
        after.len()
    ))
}

/// `--remove-setlist <name>` — DELETE a setlist resolved by exact name (guarded).
pub fn probe_remove_setlist(name: &str) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let slot = find_setlist_slot(&setlists, name)?;
    {
        let mut s = Session::connect()?;
        s.remove_setlist(slot)?;
    }
    let after = read_setlist_list()?;
    let gone = !after.iter().any(|x| x.name == name);
    Ok(format!(
        "[probe --remove-setlist] delete slot {slot} {name:?}: {} ({} setlists remain)\n",
        if gone {
            "GONE ✓"
        } else {
            "STILL PRESENT ✗"
        },
        after.len()
    ))
}

/// `--remove-setlist-song <setlist-name> <position>` — remove the song at a given
/// POSITION within the setlist (setlist resolved by name). Prints before/after
/// membership so the 0-vs-1-based `setlistSongSlot` semantics are self-evident.
/// Used to HW-pin the index base before the UI relies on it.
pub fn probe_remove_setlist_song(setlist_name: &str, position: u32) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    let before = read_setlist_songs(setlist_slot)?;
    {
        let mut s = Session::connect()?;
        s.remove_setlist_song(setlist_slot, position)?;
    }
    let after = read_setlist_songs(setlist_slot)?;
    Ok(format!(
        "[probe --remove-setlist-song] {setlist_name:?} (slot {setlist_slot}) remove position {position}\n  \
         before: {before:?}\n  after:  {after:?}\n"
    ))
}

/// `--move-setlist-song <setlist-name> <old-pos> <new-pos>` — reorder a song within
/// a setlist by POSITION. Prints before/after membership (pins the move semantics).
pub fn probe_move_setlist_song(
    setlist_name: &str,
    old_pos: u32,
    new_pos: u32,
) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    let before = read_setlist_songs(setlist_slot)?;
    {
        let mut s = Session::connect()?;
        s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
    }
    let after = read_setlist_songs(setlist_slot)?;
    Ok(format!(
        "[probe --move-setlist-song] {setlist_name:?} (slot {setlist_slot}) move {old_pos}→{new_pos}\n  \
         before: {before:?}\n  after:  {after:?}\n"
    ))
}
