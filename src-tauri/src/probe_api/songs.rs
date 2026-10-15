//! Probe entry points: Songs CRUD. The shared song reads live in [`crate::song_io`].

use crate::session;
use crate::session::Session;
use crate::slot::ListIndex;
use crate::slot::SCRATCH_SLOTS;
use crate::song_io::{converge_song_bpm, find_song_slot, read_song_list, read_song_presets};

/// Probe (AC7 verification): read a Song's preset rows over USB. Prints each
/// row's `userPresetSlot` (the device slot the Song points at — the positional
/// binding an in-place edit must preserve) and `presetSceneSlot`. Used as the
/// baseline before an edit and the check after.
pub fn probe_song_presets(song_slot: u32) -> Result<String, String> {
    let recs = read_song_presets(song_slot)?;
    if recs.is_empty() {
        return Ok(format!(
            "[probe --songpresets] song {song_slot}: no rows \
             (empty song, or songPresetListResponse(13) unsupported on this firmware)\n"
        ));
    }
    let mut out = format!(
        "[probe --songpresets] song {song_slot}: {} row(s)\n",
        recs.len()
    );
    for (i, r) in recs.iter().enumerate() {
        if r.is_empty {
            out += &format!("  row {i}: (empty)\n");
        } else {
            out += &format!(
                "  row {i}: userPresetSlot={} presetSceneSlot={} scene={:?}\n",
                r.user_preset_slot, r.preset_scene_slot, r.preset_scene_name
            );
        }
    }
    Ok(out)
}

/// Probe (`--songs`): list every Song on the device with its notes and BPM.
/// Read-only; prints one line per song.
pub fn probe_list_songs() -> Result<String, String> {
    let songs = read_song_list()?;
    if songs.is_empty() {
        return Ok("[probe --songs] no songs on the device \
                   (none defined, or songListResponse(3) unsupported on this firmware)\n"
            .to_string());
    }
    let mut out = format!("[probe --songs] {} song(s):\n", songs.len());
    for s in &songs {
        let bpm = if s.bpm_active {
            format!("{} BPM", s.bpm)
        } else {
            format!("{} BPM (off)", s.bpm)
        };
        let notes = if s.notes.is_empty() {
            "—".to_string()
        } else {
            format!("{:?}", s.notes)
        };
        out += &format!(
            "  {:>2}. {:<28}  {:<14}  notes: {}\n",
            s.slot, s.name, bpm, notes
        );
    }
    Ok(out)
}

fn song_line(r: &session::SongRecord) -> String {
    let bpm = if r.bpm_active {
        format!("{} BPM", r.bpm)
    } else {
        format!("{} BPM (off)", r.bpm)
    };
    let notes = if r.notes.is_empty() {
        "—".to_string()
    } else {
        format!("{:?}", r.notes)
    };
    format!(
        "  slot {:>2}  {:<14}  [{}]  notes: {}",
        r.slot, r.name, bpm, notes
    )
}

/// `--add-song <name>` — create a song, verify it appears.
pub fn probe_add_song(name: &str) -> Result<String, String> {
    {
        let mut s = Session::connect()?;
        s.add_song(name)?;
    }
    let songs = read_song_list()?;
    let found = songs.iter().find(|x| x.name == name);
    let mut out = format!(
        "[probe --add-song] add {name:?} → {} ({} songs total)\n",
        if found.is_some() {
            "FOUND ✓"
        } else {
            "NOT FOUND ✗"
        },
        songs.len()
    );
    if let Some(r) = found {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// `--rename-song <old> <new>` — rename (resolved by old name), verify.
pub fn probe_rename_song(old: &str, new: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, old)?;
    {
        let mut s = Session::connect()?;
        s.rename_song(slot, new)?;
    }
    let after = read_song_list()?;
    let ok = after.iter().any(|x| x.slot == slot && x.name == new);
    let old_gone = !after.iter().any(|x| x.name == old);
    let mut out = format!(
        "[probe --rename-song] slot {slot} {old:?} → {new:?}: {} (old-name-gone={})\n",
        if ok { "OK ✓" } else { "FAILED ✗" },
        old_gone
    );
    if let Some(r) = after.iter().find(|x| x.slot == slot) {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// `--song-notes <name> <notes>` — set a song's notes, verify.
pub fn probe_set_song_notes(name: &str, notes: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?;
    {
        let mut s = Session::connect()?;
        s.set_song_notes(slot, notes)?;
    }
    let after = read_song_list()?;
    let r = after.iter().find(|x| x.slot == slot);
    let ok = r.map(|x| x.notes == notes).unwrap_or(false);
    let mut out = format!(
        "[probe --song-notes] slot {slot} {name:?} notes → {notes:?}: {}\n",
        if ok { "OK ✓" } else { "MISMATCH ✗" }
    );
    if let Some(r) = r {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// `--active-slot` — read-only: what a plain handshake shows about the active preset
/// (the `PresetLoaded` echo, the current-preset name, the strict My Presets list) and
/// the restore target [`Session::active_slot_live`] derives from it.
pub fn probe_active_slot() -> Result<String, String> {
    let mut s = Session::connect()?;
    let started = std::time::Instant::now();
    let snapshot = |s: &Session| {
        let name = s.active_preset_name();
        let list = s.harvest_preset_list_strict();
        let hits = match (&name, &list) {
            (Some(n), Some(l)) => l.iter().filter(|x| *x == n).count().to_string(),
            _ => "-".into(),
        };
        format!(
            "loaded_slot={:?} name={name:?} strict_list_len={:?} name_hits={hits}",
            s.loaded_slot(),
            list.as_ref().map(Vec::len)
        )
    };
    let mut out = format!(
        "[probe --active-slot] after handshake: {}
",
        snapshot(&s)
    );
    let live = s.active_slot_live();
    out += &format!(
        "[probe --active-slot] active_slot_live={live:?} after {} ms: {}\n",
        started.elapsed().as_millis(),
        snapshot(&s)
    );
    Ok(out)
}

/// `--song-bpm <name> <bpm>` — set a song's numeric BPM via [`converge_song_bpm`]
/// (resolve slot by name, then run the shared ritual). Verify by re-read.
pub fn probe_set_song_bpm(name: &str, bpm: f32) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?;
    let (after, converged) = converge_song_bpm(slot, bpm, |_| Ok(()))?;
    let r = after.iter().find(|x| x.slot == slot);
    let got = r.map(|x| x.bpm).unwrap_or(0);
    if converged {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: OK ✓ (read-back={got})\n  {}\n",
            r.map(song_line).unwrap_or_default()
        ))
    } else {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: read-back={got} — did NOT land\n"
        ))
    }
}

/// `--remove-song <name>` — DELETE a song resolved by exact name (guard: refuses on
/// no-match / ambiguity, so the slot used for deletion is the one that reads as `name`).
/// HW PROBE (DEVICE WRITE): bind a user preset to a Song row (`assignSongPreset`, which
/// confirms the row by reading it back), then print the Song's rows either way.
///
/// `list_index` is the 0-based preset list index — **scratch zone only**, because
/// the point of this probe is to then reorder that preset and see whether the
/// binding follows it.
pub fn probe_assign_song_preset(
    song_name: &str,
    row: u32,
    list_index: ListIndex,
) -> Result<String, String> {
    if !SCRATCH_SLOTS.contains(&list_index) {
        return Err(format!(
            "refusing to bind list index {list_index}: scratch zone {SCRATCH_SLOTS:?} only"
        ));
    }
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, song_name)?;
    let mut s = Session::connect()?;
    // Label/colour are cosmetic; base scene, since 0 is the FIRST scene and the device
    // refuses one the preset doesn't have.
    let assigned = s.assign_song_preset(
        slot,
        row,
        list_index,
        "PROBE",
        1,
        session::BASE_SCENE_SLOT,
        1,
    );
    let rows = s.song_presets(slot)?;
    let mut out = format!(
        "[probe --assign-song-preset] {song_name:?} (songSlot {slot}) row {row} \
         → list idx {list_index} (device userPresetSlot {}): {}\n",
        list_index.device_slot(),
        match &assigned {
            Ok(()) => "landed ✓".to_string(),
            Err(e) => format!("REFUSED ✗ — {e}"),
        }
    );
    for (i, r) in rows.iter().enumerate() {
        out += &format!(
            "  row {i}: empty={} userPresetSlot={} presetSceneSlot={} scene={:?}\n",
            r.is_empty, r.user_preset_slot, r.preset_scene_slot, r.preset_scene_name
        );
    }
    Ok(out)
}

pub fn probe_remove_song(name: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?; // guard in the same (read) space we will delete
    {
        let mut s = Session::connect()?;
        s.remove_song(slot)?;
    }
    let after = read_song_list()?;
    let gone = !after.iter().any(|x| x.name == name);
    Ok(format!(
        "[probe --remove-song] delete slot {slot} {name:?}: {} ({} songs remain)\n",
        if gone {
            "GONE ✓"
        } else {
            "STILL PRESENT ✗"
        },
        after.len()
    ))
}
