//! Song reads and the song BPM write, shared by the song and setlist commands, the
//! in-place replace path and the probe. Every read rides the handshake burst with
//! [`proto::BATCH_DRAIN`] and fails closed on a truncated multi-packet reply.

use crate::proto;
use crate::session;
use crate::session::Session;
use crate::slot::ListIndex;

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

/// Whether a song list shows `slot` at `bpm` (±1.5).
pub(crate) fn bpm_landed(songs: &[session::SongRecord], slot: u32, bpm: f32) -> bool {
    songs
        .iter()
        .any(|x| x.slot == slot && (x.bpm as f32 - bpm).abs() < 1.5)
}

/// Set song `slot`'s BPM on ONE open session, then load `restore` (0-based list index).
/// Order matters: `tapTempoBpm` writes the ACTIVE song's BPM only once that song's
/// `bpmActive` is already on (fw 1.8.58), so load the song (tab 5), enable,
/// THEN tap, and read back once; no wait or retry. The flag stays on (the Songs tab shows
/// a BPM only while it is). An existing footswitch binding is used untouched; a song with
/// none gets row 1 → preset 001's base scene. Returns the read-back list (`None` if it
/// didn't arrive) and whether the BPM landed.
pub(crate) fn set_song_bpm_on(
    s: &mut Session,
    slot: u32,
    bpm: f32,
    restore: Option<ListIndex>,
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
            s.assign_song_preset(
                slot,
                1,
                crate::slot::ListIndex(0),
                "",
                0,
                session::BASE_SCENE_SLOT,
                0,
            )
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
/// refused footswitch, or a refusal from `guard`, which runs on the same connection
/// before any write. Shared by the `set_song_bpm` command and `probe_set_song_bpm`.
pub(crate) fn converge_song_bpm(
    slot: u32,
    bpm: f32,
    guard: impl FnOnce(&mut Session) -> Result<(), String>,
) -> Result<(Vec<session::SongRecord>, bool), String> {
    let read = {
        let mut s = Session::connect()?;
        // Before any read clears the handshake replies it comes from.
        let prior = s.active_slot_live();
        guard(&mut s)?;
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

    // fw 1.8.58: a tap reaches the active song only once its flag is on,
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
        s.load_preset(ListIndex(0)).unwrap();
        s.set_tap_tempo_bpm(90.0).unwrap();
        assert_eq!(sim.song(1).unwrap().bpm, 141);
    }

    // `assignSongPreset` with a scene index the preset doesn't have is
    // refused with SongError 5 and writes no row (the read-back makes it an Err); base
    // (8) is always accepted.
    #[test]
    fn a_song_scene_the_preset_does_not_have_is_refused_and_reported() {
        let sim = SimDevice::new().with_preset_scenes(ListIndex(1), 2);
        let mut s = sim_session(&sim);
        let err = s
            .assign_song_preset(1, 1, ListIndex(0), "", 0, 1, 0)
            .unwrap_err(); // 001: no scenes
        assert!(err.contains("did not bind"), "{err}");
        assert_eq!(sim.song(1).unwrap().rows[0], None);
        assert!(sim.events().contains(&SimEvent::SongError(5)));
        s.assign_song_preset(1, 2, ListIndex(1), "", 0, 1, 0)
            .unwrap(); // 002: 2 scenes
        s.assign_song_preset(1, 3, ListIndex(0), "", 0, session::BASE_SCENE_SLOT, 0)
            .unwrap();
        let rows = sim.song(1).unwrap().rows;
        assert_eq!(rows[1].as_ref().map(|r| r.scene_slot), Some(1));
        assert_eq!(
            rows[2].as_ref().map(|r| r.scene_slot),
            Some(session::BASE_SCENE_SLOT)
        );
    }

    // The two song bugs together: a new song (no footswitch, flag off, 120 BPM) on a preset
    // with no scenes. One pass on one session must land the BPM — no retry.
    #[test]
    fn a_new_song_bpm_lands_on_the_first_pass_on_one_session() {
        let sim = SimDevice::new();
        let mut s = sim_session(&sim);
        let (songs, landed) = set_song_bpm_on(&mut s, 1, 97.0, Some(ListIndex(5))).unwrap();
        assert!(landed);
        let rec = songs.unwrap().into_iter().find(|r| r.slot == 1).unwrap();
        assert_eq!((rec.bpm, rec.bpm_active), (97, true));
        assert_eq!(
            song_events(&sim),
            vec![
                SimEvent::SongAssign {
                    song: 1,
                    row: 1,
                    list_index: ListIndex(0),
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
