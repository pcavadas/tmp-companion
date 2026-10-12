//! Setlist CRUD + membership Tauri commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ─── Song & Setlist CRUD for the device-backed Songs page ─────────────────────
// Read-back-after-write: each write fires through its own fresh connection, then
// re-reads via the strict fail-closed helpers and returns the fresh authoritative
// list, so the UI never predicts the device's positional slots (which shift on
// every add/remove). All run with the app's seize released (`with_released_seize`).
// These are the proven `probe_*` flows (probe_api::{songs,setlists})
// reshaped as slot-addressed, frontend-callable commands. HW writes are gated by
// the read-only-on-hardware policy (lifted per-session by explicit authorization).

/// Read every Setlist's name for the Songs page — strict fail-closed live read.
#[tauri::command]
pub(crate) async fn read_setlists(
    state: State<'_, AppState>,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), read_setlist_list).await
}

/// Read a setlist's songs in device order as GLOBAL song slots, trailing empty
/// (`songSlot==0`) entries dropped — index `i` here is position `i + 1`, the
/// `setlistSongSlot` the membership ops address (the index base is HW-pinned). A `0`
/// between members is kept so the positions after it stay true.
#[tauri::command]
pub(crate) async fn list_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        read_setlist_songs(setlist_slot)
    })
    .await
}
/// Create a setlist; returns the fresh setlist list.
#[tauri::command]
pub(crate) async fn add_setlist(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist(&name)?;
        }
        read_setlist_list()
    })
    .await
}

// ─── Slot guards ──────────────────────────────────────────────────────────────
// Setlist slots are list index + 1 and shift down after every delete, so a slot the
// UI read earlier can name ANOTHER setlist now. Every slot-keyed write therefore
// re-reads the list in the same slot space and refuses unless `slot` still holds the
// name the user acted on (danger.md: the guard lives in the mutation's address space).
// The list must also still hold the count the UI saw: when two setlists share a name, a
// delete can shift one into the other's slot, and only the count tells them apart.

/// Refuse unless `list` still holds `expect_count` setlists and setlist `slot` still
/// reads `expect_name`.
pub(crate) fn check_setlist_slot(
    list: &[session::SetlistRecord],
    slot: u32,
    expect_name: &str,
    expect_count: u32,
) -> Result<(), String> {
    if list.len() != expect_count as usize {
        return Err(format!(
            "refused: the unit has {} setlists, expected {expect_count} (list changed)",
            list.len()
        ));
    }
    let rec = list
        .iter()
        .find(|r| r.slot == slot)
        .ok_or_else(|| format!("setlist slot {slot} no longer exists (list changed) — refused"))?;
    if rec.name != expect_name {
        return Err(format!(
            "refused: setlist slot {slot} reads {:?}, expected {:?} (list changed)",
            rec.name, expect_name
        ));
    }
    Ok(())
}

/// Read the setlist list on `s` and refuse unless it passes [`check_setlist_slot`].
pub(crate) fn guard_setlist_on(
    s: &mut Session,
    slot: u32,
    expect_name: &str,
    expect_count: u32,
) -> Result<(), String> {
    let list = s
        .setlist_list_strict()?
        .ok_or("the setlist list didn't read back, so the setlist can't be confirmed — refused")?;
    check_setlist_slot(&list, slot, expect_name, expect_count)
}

/// Refuse unless position `position` (1-based) of `members` still holds `expect_song`
/// — the song the user acted on. Positions shift on every membership edit.
pub(crate) fn check_member_position(
    members: &[u32],
    position: u32,
    expect_song: u32,
) -> Result<(), String> {
    let at = (position as usize)
        .checked_sub(1)
        .and_then(|i| members.get(i))
        .copied();
    match at {
        Some(got) if got == expect_song => Ok(()),
        Some(got) => Err(format!(
            "refused: setlist position {position} holds song slot {got}, expected {expect_song} \
             (setlist changed)"
        )),
        None => Err(format!(
            "refused: setlist has no position {position} (setlist changed)"
        )),
    }
}

/// Guard a membership edit on `s`: the setlist still reads `expect_name` among
/// `expect_count` setlists and `position` still holds `expect_song`.
fn guard_member_on(
    s: &mut Session,
    setlist_slot: u32,
    expect_name: &str,
    expect_count: u32,
    position: u32,
    expect_song: u32,
) -> Result<(), String> {
    guard_setlist_on(s, setlist_slot, expect_name, expect_count)?;
    let members = s.setlist_songs_strict(setlist_slot)?.ok_or(
        "the setlist's songs didn't read back, so the position can't be confirmed — refused",
    )?;
    check_member_position(&members, position, expect_song)
}

/// Rename a setlist by slot, guarded by `expect_name` and `expect_count`; returns the
/// fresh setlist list.
#[tauri::command]
pub(crate) async fn rename_setlist(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
    expect_count: u32,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            guard_setlist_on(&mut s, slot, &expect_name, expect_count)?;
            s.rename_setlist(slot, &name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Delete a setlist by slot — DESTRUCTIVE (the songs themselves are kept). Guarded
/// by `expect_name` and `expect_count` in the same read space. Returns the fresh
/// setlist list.
#[tauri::command]
pub(crate) async fn remove_setlist(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
    expect_count: u32,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        check_setlist_slot(&read_setlist_list()?, slot, &expect_name, expect_count)?;
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        read_setlist_list()
    })
    .await
}

/// Add a song (by GLOBAL song slot) to a setlist; returns the setlist's fresh
/// member song slots (trailing 0s dropped). Guarded: the setlist still reads
/// `expect_name` among `expect_count` setlists, and the song slot still reads
/// `expect_song_name` among `expect_song_count` songs.
#[tauri::command]
pub(crate) async fn add_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    expect_name: String,
    expect_count: u32,
    song_slot: u32,
    expect_song_name: String,
    expect_song_count: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            guard_setlist_on(&mut s, setlist_slot, &expect_name, expect_count)?;
            crate::commands::songs::guard_song_on(
                &mut s,
                song_slot,
                &expect_song_name,
                expect_song_count,
            )?;
            s.add_setlist_song(setlist_slot, song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Remove a song from a setlist by its POSITION within the setlist
/// (`setlist_song_slot`, NOT the global song slot). Guarded: the setlist still reads
/// `expect_name` among `expect_count` setlists and the position still holds
/// `expect_song_slot`. Returns fresh member slots.
#[tauri::command]
pub(crate) async fn remove_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    expect_name: String,
    expect_count: u32,
    setlist_song_slot: u32,
    expect_song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            guard_member_on(
                &mut s,
                setlist_slot,
                &expect_name,
                expect_count,
                setlist_song_slot,
                expect_song_slot,
            )?;
            s.remove_setlist_song(setlist_slot, setlist_song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Reorder a song within a setlist by POSITION (both indices are positions within
/// the setlist, NOT global song slots). Guarded: the setlist still reads
/// `expect_name` among `expect_count` setlists and `old_pos` still holds
/// `expect_song_slot`. Returns fresh member slots.
#[tauri::command]
pub(crate) async fn move_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    expect_name: String,
    expect_count: u32,
    old_pos: u32,
    new_pos: u32,
    expect_song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            guard_member_on(
                &mut s,
                setlist_slot,
                &expect_name,
                expect_count,
                old_pos,
                expect_song_slot,
            )?;
            s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}
/// Add several songs (by GLOBAL song slot) to a setlist under ONE bookend with ONE
/// final membership read — replaces the UI's per-song `add_setlist_song` loop
/// (which paid a bookend + membership read per song). Same proven per-write fresh
/// connection per `addSetlistSong`. Guarded before any write: the setlist still reads
/// `expect_name` among `expect_count` setlists, and each song slot still reads its
/// `expect_song_names` entry among `expect_song_count` songs (song slots shift on every
/// song add/remove).
#[tauri::command]
pub(crate) async fn add_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
    expect_name: String,
    expect_count: u32,
    song_slots: Vec<u32>,
    expect_song_names: Vec<String>,
    expect_song_count: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        if song_slots.len() != expect_song_names.len() {
            return Err(format!(
                "{} song slots but {} expected names — refused",
                song_slots.len(),
                expect_song_names.len()
            ));
        }
        // The guard reads on its own connection and each write opens a fresh one (the
        // proven per-write shape), so an edit made ON THE UNIT in between could still
        // shift a song slot. The app's own ops are serialized, so that window is only
        // the unit's front panel or Pro Control acting mid-command.
        {
            let mut s = Session::connect()?;
            guard_setlist_on(&mut s, setlist_slot, &expect_name, expect_count)?;
            let songs = s.song_list_strict()?.ok_or(
                "the song list didn't read back, so the songs can't be confirmed — refused",
            )?;
            for (slot, name) in song_slots.iter().zip(&expect_song_names) {
                crate::commands::songs::check_song_slot(&songs, *slot, name, expect_song_count)?;
            }
        }
        for ss in &song_slots {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, *ss)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<session::SetlistRecord> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| session::SetlistRecord {
                slot: i as u32 + 1,
                name: (*n).to_string(),
            })
            .collect()
    }

    #[test]
    fn a_setlist_slot_must_still_read_the_expected_name() {
        // [A, B, C] with B deleted → [A, C]: the UI's stale "B" at slot 2 and "C" at
        // slot 3 both refuse; only the current pairing passes.
        let now = list(&["A", "C"]);
        assert!(check_setlist_slot(&now, 2, "B", 2).is_err());
        assert!(check_setlist_slot(&now, 3, "C", 2).is_err());
        assert!(check_setlist_slot(&now, 2, "C", 2).is_ok());
    }

    #[test]
    fn a_shift_onto_a_same_named_setlist_is_refused_by_the_count() {
        // [X, X, C] with the first X deleted → [X, C]: the UI's stale (1, "X") still
        // names an "X", but the other one; the count the UI saw (3) refuses it.
        let now = list(&["X", "C"]);
        assert!(check_setlist_slot(&now, 1, "X", 3).is_err());
        assert!(check_setlist_slot(&now, 1, "X", 2).is_ok());
    }

    #[test]
    fn a_member_position_must_still_hold_the_expected_song() {
        let members = [4, 0, 6];
        assert!(check_member_position(&members, 3, 6).is_ok());
        assert!(check_member_position(&members, 2, 6).is_err());
        assert!(check_member_position(&members, 0, 4).is_err());
        assert!(check_member_position(&members, 4, 6).is_err());
    }

    #[test]
    fn only_trailing_padding_is_dropped_from_a_setlist_reply() {
        // A middle 0 keeps its place, so position 3 still addresses song 6.
        assert_eq!(
            session::trim_setlist_padding(vec![4, 0, 6, 0, 0]),
            [4, 0, 6]
        );
        assert_eq!(session::trim_setlist_padding(vec![0, 0]), Vec::<u32>::new());
    }
}
