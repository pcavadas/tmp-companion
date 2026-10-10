//! Song CRUD + song-assignment device-write Tauri commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Read every Song's metadata (name / notes / BPM) for the Songs overview — the
/// net-new live `songListResponse` read (rides the handshake burst).
#[tauri::command]
pub(crate) async fn list_songs(
    state: State<'_, AppState>,
) -> Result<Vec<session::SongRecord>, String> {
    // Strict, fail-closed read (retry-until-complete): a read immediately after a
    // write is the worst case for the multi-packet truncation, and the Songs page
    // re-reads after every mutation, so accept only a strictly-complete response.
    with_released_seize(state.session.clone(), read_song_list).await
}
/// Create a song; returns the fresh song list (the device assigns the slot).
#[tauri::command]
pub(crate) async fn add_song(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        read_song_list()
    })
    .await
}

/// Rename a song by slot; returns the fresh song list.
#[tauri::command]
pub(crate) async fn rename_song(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_song(slot, &name)?;
        }
        read_song_list()
    })
    .await
}

/// Delete a song by slot — DESTRUCTIVE. Guarded: a fresh read in the SAME slot space
/// must still show `expect_name` at `slot` (tolerant of duplicate names elsewhere),
/// else refuse. Returns the fresh song list.
#[tauri::command]
pub(crate) async fn remove_song(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_song_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("song slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: song slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_song(slot)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's notes by slot; returns the fresh song list.
#[tauri::command]
pub(crate) async fn set_song_notes(
    state: State<'_, AppState>,
    slot: u32,
    notes: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, &notes)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's numeric BPM by slot over one connection ([`converge_song_bpm`]: activate
/// the song through a footswitch, enable its BPM, then `tapTempoBpm`, read back once).
/// A BPM that didn't land → Err. Returns the fresh song list on success.
#[tauri::command]
pub(crate) async fn set_song_bpm(
    state: State<'_, AppState>,
    slot: u32,
    bpm: f32,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let (after, _) = converge_song_bpm(slot, bpm)?;
        match bpm_warning(&after, slot, bpm) {
            None => Ok(after),
            Some(w) => Err(format!("song slot {slot}: {w}")),
        }
    })
    .await
}
// ─── Batched song/setlist transactions ────────────────────────────────────────
// The granular commands above pay one full `with_released_seize` bookend + one
// strict fail-closed read PER FIELD (a song create with notes + BPM was 3 bookends
// + 3 reads ≈ 10 s+). These transactions run every write and read on ONE
// connection, skipping the intermediate read-backs: only the final authoritative
// read(s) remain. Slot
// stability inside a transaction: notes/BPM/membership writes don't shift slots —
// only song add/remove do, and a transaction does at most one add (first).

/// Result of a batched song save: the fresh authoritative song list, the fresh
/// membership of `add_to_setlist` (when requested), and the best-effort BPM
/// warning (BPM is the active-song tap tempo on the unit; when it doesn't land the
/// song itself is kept, mirroring the UI's previous per-call behavior).
#[derive(serde::Serialize)]
pub(crate) struct SongSaveOutcome {
    songs: Vec<session::SongRecord>,
    /// `Some` only when `add_to_setlist` was requested: that setlist's fresh
    /// ordered member song slots.
    members: Option<Vec<u32>>,
    bpm_warning: Option<String>,
}

/// The BPM step of a batched song transaction, on its open session. Never fails the
/// transaction: a refused footswitch or a failed write becomes the warning (the song
/// is kept), and whether the BPM landed is judged on the final list ([`bpm_warning`]).
/// Returns the read-back list when it arrived.
fn song_bpm_step(
    s: &mut Session,
    slot: u32,
    bpm: f32,
    prior: Option<u32>,
) -> (Option<Vec<session::SongRecord>>, Option<String>) {
    match set_song_bpm_on(s, slot, bpm, prior) {
        Ok((songs, _)) => (songs, None),
        Err(e) => (None, Some(e)),
    }
}

/// "BPM didn't land" when the final list doesn't show `slot` at `bpm`.
fn bpm_warning(songs: &[session::SongRecord], slot: u32, bpm: f32) -> Option<String> {
    (!bpm_landed(songs, slot, bpm)).then(|| {
        let got = songs
            .iter()
            .find(|x| x.slot == slot)
            .map(|x| x.bpm)
            .unwrap_or(0);
        format!("BPM didn't land at {bpm} (read-back={got})")
    })
}

/// Create a song with optional notes / BPM / setlist membership on ONE connection —
/// replaces the UI's add → read → notes → read → bpm → read → addToSetlist → read
/// chain. The created song is resolved BY NAME from the post-add read (the device
/// assigns the slot).
#[tauri::command]
pub(crate) async fn create_song_full(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
    bpm: Option<f32>,
    add_to_setlist: Option<u32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        let notes = notes.filter(|n| !n.trim().is_empty());
        let (slot, songs, members, warning) = {
            let mut s = Session::connect()?;
            // Before any read clears the handshake replies it comes from.
            let prior = bpm.and_then(|_| s.active_slot_live());
            s.add_song(&name)?;
            let listed = s.song_list_strict()?.ok_or_else(|| {
                format!("song {name:?} created, but the song list didn't read back")
            })?;
            let Some(slot) = listed.iter().find(|x| x.name == name).map(|x| x.slot) else {
                // Created but not resolvable by name (duplicate-name edge) — return the
                // fresh list; the optional fields are skipped, surfaced as a warning.
                return Ok(SongSaveOutcome {
                    songs: listed,
                    members: None,
                    bpm_warning: Some(format!(
                        "song {name:?} created, but not resolvable by name — notes/BPM skipped"
                    )),
                });
            };
            let mut songs = Some(listed);
            if let Some(n) = &notes {
                s.set_song_notes(slot, n.trim())?;
                songs = None;
            }
            let mut warning = None;
            if let Some(b) = bpm {
                (songs, warning) = song_bpm_step(&mut s, slot, b, prior);
            }
            if songs.is_none() {
                songs = s.song_list_strict()?;
            }
            let mut members = None;
            if let Some(setlist_slot) = add_to_setlist {
                s.add_setlist_song(setlist_slot, slot)?;
                members = s.setlist_songs_strict(setlist_slot)?;
            }
            (slot, songs, members, warning)
        };
        finish_song_save(slot, bpm, songs, add_to_setlist, members, warning)
    })
    .await
}

/// Update a song's changed fields (rename / notes / BPM) on ONE connection — replaces
/// the UI's per-field command chain. `None` = field unchanged. The caller skips the
/// call entirely when nothing changed.
#[tauri::command]
pub(crate) async fn update_song_full(
    state: State<'_, AppState>,
    slot: u32,
    name: Option<String>,
    notes: Option<String>,
    bpm: Option<f32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        let (songs, warning) = {
            let mut s = Session::connect()?;
            // Before any read clears the handshake replies it comes from.
            let prior = bpm.and_then(|_| s.active_slot_live());
            if let Some(n) = &name {
                s.rename_song(slot, n)?;
            }
            if let Some(n) = &notes {
                s.set_song_notes(slot, n)?;
            }
            let (mut songs, mut warning) = (None, None);
            if let Some(b) = bpm {
                (songs, warning) = song_bpm_step(&mut s, slot, b, prior);
            }
            if songs.is_none() {
                songs = s.song_list_strict()?;
            }
            (songs, warning)
        };
        finish_song_save(slot, bpm, songs, None, None, warning)
    })
    .await
}

/// Close a batched song save once its session is dropped: a list or membership read
/// that didn't arrive on the session falls back to a fresh-connection read, and the BPM
/// is judged on the final list.
fn finish_song_save(
    slot: u32,
    bpm: Option<f32>,
    songs: Option<Vec<session::SongRecord>>,
    add_to_setlist: Option<u32>,
    members: Option<Vec<u32>>,
    warning: Option<String>,
) -> Result<SongSaveOutcome, String> {
    let songs = match songs {
        Some(l) => l,
        None => read_song_list()?,
    };
    let members = match (add_to_setlist, members) {
        (Some(setlist_slot), None) => Some(read_setlist_songs(setlist_slot)?),
        (_, m) => m,
    };
    let bpm_warning = warning.or_else(|| bpm.and_then(|b| bpm_warning(&songs, slot, b)));
    Ok(SongSaveOutcome {
        songs,
        members,
        bpm_warning,
    })
}
// ─── Song-assignment device WRITE (SongMessage 14–17) ────────────────────────────

/// Bind a user preset (+ scene) to a Song row on the device — `assignSongPreset`.
/// `user_list_index` is 0-based (session applies the device +1). The label and both
/// colours overwrite the row's, so pass its current ones. The row is read back, so a
/// refused scene is an Err (see `Session::assign_song_preset`). DEVICE WRITE.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn song_assign(
    song_slot: u32,
    song_preset_slot: u32,
    user_list_index: u32,
    footswitch_label: String,
    footswitch_color: u32,
    preset_scene_slot: u32,
    footswitch_color_inactive: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.assign_song_preset(
            song_slot,
            song_preset_slot,
            user_list_index,
            &footswitch_label,
            footswitch_color,
            preset_scene_slot,
            footswitch_color_inactive,
        )
    })
    .await
}

/// Empty a Song row on the device — `clearSongPreset`. DEVICE WRITE.
#[tauri::command]
pub(crate) async fn song_clear(
    song_slot: u32,
    song_preset_slot: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.clear_song_preset(song_slot, song_preset_slot)
    })
    .await
}

/// Reorder a Song row on the device — `moveSongPreset`. DEVICE WRITE.
#[tauri::command]
pub(crate) async fn song_move(
    song_slot: u32,
    old: u32,
    new: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.move_song_preset(song_slot, old, new)
    })
    .await
}

/// Swap two Song rows on the device — `swapSongPreset`. DEVICE WRITE.
#[tauri::command]
pub(crate) async fn song_swap(
    song_slot: u32,
    a: u32,
    b: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.swap_song_preset(song_slot, a, b)
    })
    .await
}
