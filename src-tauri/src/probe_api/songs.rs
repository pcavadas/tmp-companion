//! Probe entry points: Songs CRUD + shared song-read helpers (used by the song commands).

use super::slot_write::discover_active_graph;
use crate::proto;
use crate::session;
use crate::session::Session;

/// Read a Song's preset assignments. Song reads ride the handshake burst AND
/// need a top-level `batchStatus` (like preset *reads*; unlike setters, which omit
/// it) — a no-batch or standalone request gets no reply (HW-confirmed 1.7.75, via
/// a batchStatus sweep). The accepted batch tracks the burst's active group, so we
/// sweep a few and take the first that yields records. Empty Vec = the song is
/// genuinely empty.
pub(crate) fn read_song_presets(song_slot: u32) -> Result<Vec<session::SongPresetRecord>, String> {
    for batch in [2u64, 3, 1, 4] {
        let req = proto::song_preset_list_request(song_slot as u64, Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..6 {
            let r = s.harvest_song_presets();
            if !r.is_empty() {
                return Ok(r);
            }
            s.pump_more(400)?;
        }
    }
    Ok(Vec::new())
}

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

/// Read every Song's metadata (name / notes / BPM) — the net-new live
/// `songListResponse` read. Song reads ride the handshake burst AND need a
/// top-level `batchStatus` (same constraint as [`read_song_presets`]), so sweep a
/// few batch values and take the first that yields records. Empty Vec = no songs.
pub(crate) fn read_song_list() -> Result<Vec<session::SongRecord>, String> {
    // FAIL-CLOSED: a single multi-packet read can be tail-truncated by concurrent
    // device streams (reassemble_streams can't demux concurrent 0x33 streams), so
    // accept ONLY a strictly-complete response (`harvest_songs_strict`), retrying
    // independent reads until one lands. See the reassembly review.
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::song_list_request(Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_songs_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err("could not read a complete song list (multi-packet response kept truncating)".into())
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

/// Resolve a song's 1-based slot by exact name. Errors on no-match or ambiguity.
pub(crate) fn find_song_slot(songs: &[session::SongRecord], name: &str) -> Result<u32, String> {
    let hits: Vec<u32> = songs
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.slot)
        .collect();
    match hits.len() {
        0 => Err(format!("no song named {name:?} on the device")),
        1 => Ok(hits[0]),
        n => Err(format!(
            "{n} songs named {name:?} — ambiguous, refusing to mutate"
        )),
    }
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

/// The RE'd per-song BPM ritual (there is NO dedicated BPM setter — BPM is the global
/// `tapTempoBpm` applied to the ACTIVE song, so the song must be activated via a
/// footswitch). **Non-destructive:** if the song already has a footswitch binding we
/// activate THAT one and never overwrite it; we only `assignSongPreset` (purely
/// additively — nothing to clobber) when the song has no footswitch at all. (The
/// device does not echo a binding's footswitch label/color on read, so re-asserting
/// an existing binding could only blank them — hence we never re-write it.) Then
/// retry ≤5× { `load_song` + `tapTempoBpm` + enable BPM display + read-back-verify
/// within ±1.5 } — the first load often doesn't settle. Finally **restore the prior
/// active preset** so editing BPM doesn't leave the amp on the song's footswitch
/// preset. Returns `(fresh_song_list, converged)`; `converged == false` means the
/// retries were exhausted (the caller decides whether that's an error). `Err` only on
/// an actual connection/transact failure. Shared by the `set_song_bpm` command and
/// `probe_set_song_bpm` so this fragile, HW-derived flow lives once.
pub(crate) fn converge_song_bpm(
    slot: u32,
    bpm: f32,
) -> Result<(Vec<session::SongRecord>, bool), String> {
    // Read the currently-active preset (to restore afterward) + the song's existing
    // footswitch bindings (to avoid clobbering one). Both best-effort reads.
    let prior_active = discover_active_graph().ok().and_then(|(g, _)| g.slot);

    // FAIL-CLOSED read of the footswitch bindings. `read_song_presets` returns an
    // EMPTY Vec only when it got NO usable response (a genuinely footswitch-less song
    // still returns its all-empty row set), so an empty result = we couldn't read the
    // bindings authoritatively → refuse rather than additively assign over a binding
    // we just failed to see. (A truncated record defaults `is_empty=false`, so a
    // partial read can never FALSELY report a bound position as empty — it errs
    // toward "activate the existing binding", never toward clobbering.)
    let rows = read_song_presets(slot)?;
    if rows.is_empty() {
        return Err(
            "could not read this song's footswitch bindings — refusing to set \
                    BPM to avoid overwriting a footswitch preset (load the song on the \
                    unit and retry)"
                .into(),
        );
    }
    let existing = rows.iter().enumerate().find(|(_, r)| !r.is_empty);

    // 1) Resolve the footswitch to activate. Existing binding → use it untouched;
    //    authoritatively none → assign position 1 additively (the original behavior,
    //    only when there's nothing to overwrite). `user_preset_slot` is the device
    //    1-based slot already stored in the binding, which `load_song.presetSlot` wants.
    let (fs_pos, fs_preset_slot) = match existing {
        Some((i, r)) => ((i + 1) as u32, r.user_preset_slot),
        None => {
            let mut s = Session::connect()?;
            s.assign_song_preset(slot, 1, 0, "", 0, 1)?;
            (1u32, 1u32)
        }
    };

    // 2) Activate + set tempo, retry until the read-back lands within ±1.5.
    let mut last = Vec::new();
    let mut converged = false;
    for _ in 0..5 {
        {
            let mut s = Session::connect()?;
            s.load_song(slot, fs_pos, fs_preset_slot)?;
            s.set_tap_tempo_bpm(bpm)?;
            s.set_song_bpm_active(slot, true)?;
        }
        last = read_song_list()?;
        if let Some(r) = last.iter().find(|x| x.slot == slot) {
            if (r.bpm as f32 - bpm).abs() < 1.5 {
                converged = true;
                break;
            }
        }
    }

    // 3) Restore the prior active preset so the live tone returns to what it was
    //    (best-effort — only if we could read it).
    if let Some(prior) = prior_active {
        if let Ok(mut s) = Session::connect() {
            let _ = s.load_preset(prior);
        }
    }

    Ok((last, converged))
}

/// `--song-bpm <name> <bpm>` — set a song's numeric BPM via [`converge_song_bpm`]
/// (resolve slot by name, then run the shared ritual). Verify by re-read.
pub fn probe_set_song_bpm(name: &str, bpm: f32) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?;
    let (after, converged) = converge_song_bpm(slot, bpm)?;
    let r = after.iter().find(|x| x.slot == slot);
    let got = r.map(|x| x.bpm).unwrap_or(0);
    if converged {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: OK ✓ (read-back={got})\n  {}\n",
            r.map(song_line).unwrap_or_default()
        ))
    } else {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: read-back={got} — did NOT converge after retries \
             (active-song targeting may differ for this slot)\n"
        ))
    }
}

/// `--remove-song <name>` — DELETE a song resolved by exact name (guard: refuses on
/// no-match / ambiguity, so the slot used for deletion is the one that reads as `name`).
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
