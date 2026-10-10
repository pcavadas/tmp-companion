//! Probe entry points: Songs CRUD + shared song-read helpers (used by the song commands).

use super::SCRATCH_SLOTS;
use crate::proto;
use crate::session;
use crate::session::Session;

/// Read a Song's preset assignments. Song reads ride the handshake burst with
/// [`proto::BATCH_DRAIN`] (fw 1.8.58: 1/2 only queue, 3 queues and drains, ≥4 is
/// dropped), and only a reply carrying this `songSlot` counts — a stale queued
/// request for another song can be answered on this session. Empty Vec = the song
/// is genuinely empty.
pub(crate) fn read_song_presets(song_slot: u32) -> Result<Vec<session::SongPresetRecord>, String> {
    let req = proto::song_preset_list_request(song_slot as u64, Some(proto::BATCH_DRAIN));
    for _ in 0..4 {
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..6 {
            let r = s.harvest_song_presets(song_slot);
            if !r.is_empty() {
                return Ok(r);
            }
            s.pump_collect_alive(400)?;
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
/// `songListResponse` read. Same in-burst batch-3 contract as
/// [`read_song_presets`]. Empty Vec = no songs.
pub(crate) fn read_song_list() -> Result<Vec<session::SongRecord>, String> {
    // FAIL-CLOSED: a single multi-packet read can be tail-truncated by concurrent
    // device streams (reassemble_streams can't demux concurrent 0x33 streams), so
    // accept ONLY a strictly-complete response (`harvest_songs_strict`), retrying
    // independent reads until one lands. See the reassembly review.
    let req = proto::song_list_request(Some(proto::BATCH_DRAIN));
    for _ in 0..10 {
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_songs_strict() {
                return Ok(r);
            }
            s.pump_collect_alive(250)?;
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

/// Whether a song list shows `slot` at `bpm` (±1.5).
pub(crate) fn bpm_landed(songs: &[session::SongRecord], slot: u32, bpm: f32) -> bool {
    songs
        .iter()
        .any(|x| x.slot == slot && (x.bpm as f32 - bpm).abs() < 1.5)
}

/// Set song `slot`'s BPM on ONE open session, then load `restore` (0-based list index).
/// Order matters: `tapTempoBpm` writes the ACTIVE song's BPM only once that song's
/// `bpmActive` is already on (fw 1.8.58, tmp-audit Q40), so load the song (tab 5), enable,
/// THEN tap, and read back once; no wait or retry. The flag stays on (the Songs tab shows
/// a BPM only while it is). An existing footswitch binding is used untouched; a song with
/// none gets row 1 → preset 001's base scene. Returns the read-back list (`None` if it
/// didn't arrive) and whether the BPM landed.
pub(crate) fn set_song_bpm_on(
    s: &mut Session,
    slot: u32,
    bpm: f32,
    restore: Option<u32>,
) -> Result<(Option<Vec<session::SongRecord>>, bool), String> {
    // FAIL-CLOSED: an empty read means no usable reply (a footswitch-less song still
    // returns its all-empty rows), so refuse rather than assign over a binding we
    // couldn't see. A truncated record defaults `is_empty=false`, so a partial read errs
    // toward activating a binding, never toward clobbering one.
    let rows = s.song_presets(slot)?;
    if rows.is_empty() {
        return Err(
            "could not read this song's footswitch bindings — refusing to set \
             BPM to avoid overwriting a footswitch preset (load the song on the \
             unit and retry)"
                .into(),
        );
    }
    // `user_preset_slot` is the 1-based device slot `load_song.presetSlot` wants.
    let (fs_pos, fs_preset_slot) = match rows.iter().position(|r| !r.is_empty) {
        Some(i) => ((i + 1) as u32, rows[i].user_preset_slot),
        None => {
            // An all-empty song: no label or colours to keep.
            s.assign_song_preset(slot, 1, 0, "", 0, session::BASE_SCENE_SLOT, 0)
                .map_err(|e| {
                    format!(
                        "could not give this song a footswitch, which setting its BPM \
                         needs — assign a preset to it on the unit and retry ({e})"
                    )
                })?;
            (1, 1)
        }
    };

    let songs = (|| {
        s.load_song(slot, fs_pos, fs_preset_slot)?;
        s.set_song_bpm_active(slot, true)?;
        s.set_tap_tempo_bpm(bpm)?;
        s.song_list_strict()
    })();

    // Put the amp back on the preset that was active (best-effort), even after a failed
    // write: the song load above already left song mode active.
    if let Some(prior) = restore {
        let _ = s.load_preset(prior);
    }
    let songs = songs?;
    let landed = songs.as_deref().is_some_and(|l| bpm_landed(l, slot, bpm));
    Ok((songs, landed))
}

/// Set song `slot`'s BPM over ONE connection ([`set_song_bpm_on`], restoring the preset
/// the handshake showed active). Returns the fresh song list and whether the BPM landed;
/// `false` is the caller's to report. `Err` only on a connection/transact failure or a
/// refused footswitch. Shared by the `set_song_bpm` command and `probe_set_song_bpm`.
pub(crate) fn converge_song_bpm(
    slot: u32,
    bpm: f32,
) -> Result<(Vec<session::SongRecord>, bool), String> {
    let read = {
        let mut s = Session::connect()?;
        let prior = s.active_slot_live();
        set_song_bpm_on(&mut s, slot, bpm, prior)?
    };
    match read {
        (Some(songs), landed) => Ok((songs, landed)),
        // The read-back never arrived on the session: one fresh strict read decides.
        (None, _) => {
            let songs = read_song_list()?;
            let landed = bpm_landed(&songs, slot, bpm);
            Ok((songs, landed))
        }
    }
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
    list_index: u32,
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
        list_index + 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_device::{SimDevice, SimEvent, SimSongRow};

    /// A bound song row on 1-based device slot `dev_slot`, base scene.
    fn row(dev_slot: u32, label: &str) -> SimSongRow {
        SimSongRow {
            user_preset_slot: dev_slot,
            scene_slot: session::BASE_SCENE_SLOT,
            label: label.into(),
            color: 3,
            color_inactive: 19,
        }
    }

    fn sim_session(sim: &SimDevice) -> Session {
        Session::from_transport(Box::new(sim.clone()))
    }

    /// The device events a BPM write produced, minus keep-alives.
    fn song_events(sim: &SimDevice) -> Vec<SimEvent> {
        sim.events()
            .into_iter()
            .filter(|e| !matches!(e, SimEvent::Heartbeat))
            .collect()
    }

    // tmp-audit Q40 (fw 1.8.58): a tap reaches the active song only once its flag is on,
    // and enabling the flag re-applies the STORED BPM rather than the one just tapped.
    #[test]
    fn sim_tap_lands_only_when_the_song_bpm_flag_is_already_on() {
        let sim = SimDevice::new().with_song(1, |song| song.rows[0] = Some(row(1, "")));
        let mut s = sim_session(&sim);
        s.load_song(1, 1, 1).unwrap();
        // The companion's old order: tap, then enable → the song keeps 120.
        s.set_tap_tempo_bpm(140.0).unwrap();
        s.set_song_bpm_active(1, true).unwrap();
        assert_eq!(sim.song(1).unwrap().bpm, 120);
        // Flag on: the next tap lands.
        s.set_tap_tempo_bpm(141.0).unwrap();
        assert_eq!(sim.song(1).unwrap().bpm, 141);
        // A tab-1 load leaves song mode: a tap no longer reaches any song.
        s.load_preset(0).unwrap();
        s.set_tap_tempo_bpm(90.0).unwrap();
        assert_eq!(sim.song(1).unwrap().bpm, 141);
    }

    // tmp-audit Q40: `assignSongPreset` with a scene index the preset doesn't have is
    // refused with SongError 5 and writes no row (the read-back makes it an Err); base
    // (8) is always accepted.
    #[test]
    fn a_song_scene_the_preset_does_not_have_is_refused_and_reported() {
        let sim = SimDevice::new().with_preset_scenes(1, 2);
        let mut s = sim_session(&sim);
        let err = s.assign_song_preset(1, 1, 0, "", 0, 1, 0).unwrap_err(); // 001: no scenes
        assert!(err.contains("did not bind"), "{err}");
        assert_eq!(sim.song(1).unwrap().rows[0], None);
        assert!(sim.events().contains(&SimEvent::SongError(5)));
        s.assign_song_preset(1, 2, 1, "", 0, 1, 0).unwrap(); // 002: 2 scenes
        s.assign_song_preset(1, 3, 0, "", 0, session::BASE_SCENE_SLOT, 0)
            .unwrap();
        let rows = sim.song(1).unwrap().rows;
        assert_eq!(rows[1].as_ref().map(|r| r.scene_slot), Some(1));
        assert_eq!(
            rows[2].as_ref().map(|r| r.scene_slot),
            Some(session::BASE_SCENE_SLOT)
        );
    }

    // The two Q40 bugs together: a new song (no footswitch, flag off, 120 BPM) on a preset
    // with no scenes. One pass on one session must land the BPM — no retry.
    #[test]
    fn a_new_song_bpm_lands_on_the_first_pass_on_one_session() {
        let sim = SimDevice::new();
        let mut s = sim_session(&sim);
        let (songs, landed) = set_song_bpm_on(&mut s, 1, 97.0, Some(5)).unwrap();
        assert!(landed);
        let rec = songs.unwrap().into_iter().find(|r| r.slot == 1).unwrap();
        assert_eq!((rec.bpm, rec.bpm_active), (97, true));
        assert_eq!(
            song_events(&sim),
            vec![
                SimEvent::SongAssign {
                    song: 1,
                    row: 1,
                    list_index: 0,
                    scene: session::BASE_SCENE_SLOT,
                },
                SimEvent::Loaded(0),
                SimEvent::SongLoaded { song: 1, row: 1 },
                SimEvent::SongBpmActive { song: 1, on: true },
                SimEvent::TapTempo(97.0),
                SimEvent::Loaded(5), // the prior preset, restored
            ]
        );
    }

    #[test]
    fn an_existing_footswitch_is_activated_untouched() {
        let sim = SimDevice::new().with_song(2, |song| {
            song.bpm = 88;
            song.bpm_active = true;
            song.rows[2] = Some(row(5, "VERSE"));
        });
        let mut s = sim_session(&sim);
        let (_, landed) = set_song_bpm_on(&mut s, 2, 132.0, None).unwrap();
        assert!(landed);
        assert_eq!(sim.song(2).unwrap().bpm, 132);
        assert_eq!(
            sim.song(2).unwrap().rows[2]
                .as_ref()
                .map(|r| r.label.as_str()),
            Some("VERSE")
        );
        assert_eq!(
            song_events(&sim),
            vec![
                SimEvent::Loaded(4),
                SimEvent::SongLoaded { song: 2, row: 3 },
                SimEvent::SongBpmActive { song: 2, on: true },
                SimEvent::TapTempo(132.0),
            ]
        );
    }
}
