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
/// (`songSlot==0`) entries dropped so the list is dense — position `i` here is the
/// `setlistSongSlot` the membership ops address (the index base is HW-pinned).
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

/// Rename a setlist by slot; returns the fresh setlist list.
#[tauri::command]
pub(crate) async fn rename_setlist(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_setlist(slot, &name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Delete a setlist by slot — DESTRUCTIVE (the songs themselves are kept). Guarded
/// by `expect_name` in the same read space. Returns the fresh setlist list.
#[tauri::command]
pub(crate) async fn remove_setlist(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_setlist_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("setlist slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: setlist slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        read_setlist_list()
    })
    .await
}

/// Add a song (by GLOBAL song slot) to a setlist; returns the setlist's fresh
/// ordered member song slots (dense — trailing 0s dropped).
#[tauri::command]
pub(crate) async fn add_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Remove a song from a setlist by its POSITION within the setlist
/// (`setlist_song_slot`, NOT the global song slot). Returns fresh member slots.
#[tauri::command]
pub(crate) async fn remove_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    setlist_song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.remove_setlist_song(setlist_slot, setlist_song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Reorder a song within a setlist by POSITION (both indices are positions within
/// the setlist, NOT global song slots). Returns fresh member slots.
#[tauri::command]
pub(crate) async fn move_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    old_pos: u32,
    new_pos: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}
/// Add several songs (by GLOBAL song slot) to a setlist under ONE bookend with ONE
/// final membership read — replaces the UI's per-song `add_setlist_song` loop
/// (which paid a bookend + membership read per song). Same proven per-write fresh
/// connection per `addSetlistSong`.
#[tauri::command]
pub(crate) async fn add_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slots: Vec<u32>,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        for ss in &song_slots {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, *ss)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}
