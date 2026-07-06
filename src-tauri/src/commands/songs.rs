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

/// Set a song's numeric BPM by slot via the RE'd mechanism (there is NO dedicated
/// BPM setter): ensure the song has a footswitch (`assignSongPreset`), activate it
/// (`loadPreset tabEnum=5`), send the global `tapTempoBpm` (which the device stores
/// as the ACTIVE song's BPM — so this mutates active-song state as a side effect),
/// enable BPM display, verify by re-read. Retries the activate+tempo (the first load
/// after a fresh assign often doesn't settle). Non-convergence → Err. Returns the
/// fresh song list on success.
#[tauri::command]
pub(crate) async fn set_song_bpm(
    state: State<'_, AppState>,
    slot: u32,
    bpm: f32,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let (after, converged) = converge_song_bpm(slot, bpm)?;
        if converged {
            return Ok(after);
        }
        let got = after
            .iter()
            .find(|x| x.slot == slot)
            .map(|x| x.bpm)
            .unwrap_or(0);
        Err(format!(
            "BPM for song slot {slot} did not converge to {bpm} after retries (read-back={got}); \
             tempo applies to the active song and this slot may not have activated"
        ))
    })
    .await
}
// ─── Batched song/setlist transactions ────────────────────────────────────────
// The granular commands above pay one full `with_released_seize` bookend + one
// strict fail-closed read PER FIELD (a song create with notes + BPM was 3 bookends
// + 3 reads ≈ 10 s+). These transactions run the same proven per-write fresh
// connections (the wire behavior is untouched) but under ONE bookend, skipping the
// intermediate read-backs: only the final authoritative read(s) remain. Slot
// stability inside a transaction: notes/BPM/membership writes don't shift slots —
// only song add/remove do, and a transaction does at most one add (first).

/// Result of a batched song save: the fresh authoritative song list, the fresh
/// membership of `add_to_setlist` (when requested), and the best-effort BPM
/// warning (BPM is the active-song tap tempo on the unit and can fail to settle —
/// the song itself is kept, mirroring the UI's previous per-call behavior).
#[derive(serde::Serialize)]
pub(crate) struct SongSaveOutcome {
    songs: Vec<session::SongRecord>,
    /// `Some` only when `add_to_setlist` was requested: that setlist's fresh
    /// ordered member song slots.
    members: Option<Vec<u32>>,
    bpm_warning: Option<String>,
}

/// Best-effort BPM step shared by the batched song transactions: returns the
/// fresh song list when the converge ran (success OR non-convergence), plus the
/// warning when it didn't stick. Never fails the transaction — mirrors the UI's
/// previous "Saved, but BPM didn't stick" toast semantics.
fn apply_song_bpm_best_effort(
    slot: u32,
    bpm: f32,
) -> (Option<Vec<session::SongRecord>>, Option<String>) {
    match converge_song_bpm(slot, bpm) {
        Ok((after, true)) => (Some(after), None),
        Ok((after, false)) => {
            let got = after
                .iter()
                .find(|x| x.slot == slot)
                .map(|x| x.bpm)
                .unwrap_or(0);
            (
                Some(after),
                Some(format!("BPM didn't converge to {bpm} (read-back={got})")),
            )
        }
        Err(e) => (None, Some(e)),
    }
}

/// Create a song with optional notes / BPM / setlist membership as ONE device
/// transaction (one bookend, one final read) — replaces the UI's add → read →
/// notes → read → bpm → read → addToSetlist → read chain. The created song is
/// resolved BY NAME from the post-add read (a new song inserts at protocol slot 1
/// and shifts every other song +1 — the device assigns the slot).
#[tauri::command]
pub(crate) async fn create_song_full(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
    bpm: Option<f32>,
    add_to_setlist: Option<u32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        let mut songs = read_song_list()?;
        let Some(slot) = songs.iter().find(|s| s.name == name).map(|s| s.slot) else {
            // Created but not resolvable by name (duplicate-name edge) — return the
            // fresh list; the optional fields are skipped, surfaced as a warning.
            return Ok(SongSaveOutcome {
                songs,
                members: None,
                bpm_warning: Some(format!(
                    "song {name:?} created, but not resolvable by name — notes/BPM skipped"
                )),
            });
        };
        let notes = notes.filter(|n| !n.trim().is_empty());
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n.trim())?;
        }
        let mut bpm_warning = None;
        match bpm {
            Some(b) => {
                let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
                if let Some(fresh) = fresh {
                    songs = fresh; // the converge already re-read — reuse it
                }
                bpm_warning = warn;
            }
            None if notes.is_some() => songs = read_song_list()?,
            None => {}
        }
        let members = match add_to_setlist {
            Some(setlist_slot) => {
                {
                    let mut s = Session::connect()?;
                    s.add_setlist_song(setlist_slot, slot)?;
                }
                Some(read_setlist_songs(setlist_slot)?)
            }
            None => None,
        };
        Ok(SongSaveOutcome {
            songs,
            members,
            bpm_warning,
        })
    })
    .await
}

/// Update a song's changed fields (rename / notes / BPM) as ONE device
/// transaction (one bookend, one final read) — replaces the UI's per-field
/// command chain. `None` = field unchanged. The caller skips the call entirely
/// when nothing changed.
#[tauri::command]
pub(crate) async fn update_song_full(
    state: State<'_, AppState>,
    slot: u32,
    name: Option<String>,
    notes: Option<String>,
    bpm: Option<f32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        if let Some(n) = &name {
            let mut s = Session::connect()?;
            s.rename_song(slot, n)?;
        }
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n)?;
        }
        let mut songs = None;
        let mut bpm_warning = None;
        if let Some(b) = bpm {
            let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
            songs = fresh;
            bpm_warning = warn;
        }
        let songs = match songs {
            Some(s) => s,
            None => read_song_list()?, // one final authoritative read
        };
        Ok(SongSaveOutcome {
            songs,
            members: None,
            bpm_warning,
        })
    })
    .await
}
// ─── Song-assignment device WRITE (SongMessage 14–17) ────────────────────────────

/// Bind a user preset (+ scene) to a Song row on the device — `assignSongPreset`.
/// `user_list_index` is 0-based (session applies the device +1). DEVICE WRITE.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn song_assign(
    song_slot: u32,
    song_preset_slot: u32,
    user_list_index: u32,
    footswitch_label: String,
    footswitch_color: u32,
    preset_scene_slot: u32,
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
