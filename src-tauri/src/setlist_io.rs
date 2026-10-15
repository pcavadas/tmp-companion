//! Setlist reads, shared by the setlist and song commands and the probe. Same in-burst
//! [`proto::BATCH_DRAIN`] contract and fail-closed strict reads as [`crate::song_io`].

use crate::proto;
use crate::session;
use crate::session::Session;

/// Read every Setlist's name — the net-new live `setlistListResponse` read. Same
/// in-burst batch-3 contract as [`crate::song_io::read_song_list`] (setlist reads ride
/// the handshake burst). A `SetlistListRecord` is name-only; per-setlist song
/// membership is a separate read. Empty Vec = no setlists defined.
pub(crate) fn read_setlist_list() -> Result<Vec<session::SetlistRecord>, String> {
    // Fail-closed, like read_song_list: accept only a strictly-complete
    // setlistListResponse, retrying until one lands (the multi-packet response
    // truncates non-deterministically, worse right after a write).
    let req = proto::setlist_list_request(Some(proto::BATCH_DRAIN));
    for _ in 0..10 {
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlists_strict() {
                return Ok(r);
            }
            s.pump_collect_alive(250)?;
        }
    }
    Err("could not read a complete setlist list (multi-packet response kept truncating)".into())
}

/// Read the RAW song slots a Setlist contains — `setlistSongListResponse` records,
/// each a `songSlot`, INCLUDING trailing `songSlot==0` padding (unassigned slots).
/// Same in-burst batch-3 contract as the other song/setlist reads; only a reply
/// carrying this `setlistSlot` counts.
/// Most callers want [`read_setlist_songs`] (dense); only the HW-characterization
/// probe needs the raw padding to count empties.
pub(crate) fn read_setlist_songs_raw(setlist_slot: u32) -> Result<Vec<u32>, String> {
    // Fail-closed strict read; a complete response for an empty setlist is Some(vec![]).
    let req = proto::setlist_song_list_request(setlist_slot as u64, Some(proto::BATCH_DRAIN));
    for _ in 0..10 {
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlist_songs_strict(setlist_slot) {
                return Ok(r);
            }
            s.pump_collect_alive(250)?;
        }
    }
    Err(format!(
        "could not read a complete song list for setlist slot {setlist_slot}"
    ))
}

/// A setlist's member song slots in device order, with the trailing `songSlot==0`
/// padding (unassigned slots) dropped by [`session::trim_setlist_padding`], which also
/// says why a `0` between members is kept. Index `i` is device position `i + 1` — the
/// position every membership command and the UI address. Empty Vec = an empty setlist.
pub(crate) fn read_setlist_songs(setlist_slot: u32) -> Result<Vec<u32>, String> {
    Ok(session::trim_setlist_padding(read_setlist_songs_raw(
        setlist_slot,
    )?))
}
