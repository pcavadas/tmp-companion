//! Songs and setlists: their record types, edit commands and strict harvests. Calls
//! link primitives and `timing` consts only.

use super::*;

/// One row of a Song's preset assignment list (`SongPresetListRecord`). The
/// load-bearing fields are `user_preset_slot` (the **device** slot the
/// Song row points at — the positional binding an in-place edit must preserve)
/// and `preset_scene_slot` (the bound scene's position). `is_empty` marks an
/// unassigned row.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SongPresetRecord {
    pub is_empty: bool,
    pub user_preset_slot: u32,
    pub preset_scene_slot: u32,
    pub preset_scene_name: String,
    pub footswitch_label: String,
    pub footswitch_color: u32,
    pub footswitch_color_inactive: u32,
}

/// One Song's metadata, decoded from `songListResponse` (the net-new live song
/// read). `bpm_active` mirrors the device's per-song "BPM on/off" flag. Per-song
/// preset assignments are a separate read (`songPresetListResponse`); this carries
/// metadata only.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SongRecord {
    pub slot: u32,
    pub name: String,
    pub notes: String,
    pub bpm: u32,
    pub bpm_active: bool,
}

/// One Setlist's metadata, decoded from `setlistListResponse` (the net-new live
/// setlist read). A `SetlistListRecord` carries only `setlistName`; the device
/// orders the list, so `slot` is the 1-based list position. A setlist's song
/// membership is a separate read (`setlistSongListResponse`); this is names only.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SetlistRecord {
    pub slot: u32,
    pub name: String,
}

/// Drop a setlist reply's TRAILING `songSlot == 0` padding (the unassigned slots after
/// the last member). A `0` between members is kept: the membership setters address a
/// song by its position in this reply, so dropping a middle entry would shift every
/// later position onto the wrong song. Callers render a kept `0` as an unknown entry.
pub(crate) fn trim_setlist_padding(mut members: Vec<u32>) -> Vec<u32> {
    while members.last() == Some(&0) {
        members.pop();
    }
    members
}

impl Session {
    /// Assign a user preset (+ its scene) to a Song row — `assignSongPreset`.
    /// `user_list_index` is a **0-based list index**; the device `userPresetSlot` is
    /// 1-based, so `+1` is applied (consistent with every other slot setter). The
    /// `song_slot` / `song_preset_slot` / `preset_scene_slot` are song-internal
    /// positions passed through. The label and both colours are written as given
    /// (an empty/zero value stores ""/OFF), so pass the row's current ones.
    ///
    /// The row is read back: the device refuses a `preset_scene_slot` the preset doesn't
    /// have (not [`BASE_SCENE_SLOT`] and ≥ its scene count) with SongError 5 and writes
    /// nothing (fw 1.8.58), so a row that didn't land is an `Err`.
    #[allow(clippy::too_many_arguments)]
    pub fn assign_song_preset(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
        user_list_index: ListIndex,
        footswitch_label: &str,
        footswitch_color: u32,
        preset_scene_slot: u32,
        footswitch_color_inactive: u32,
    ) -> Result<(), String> {
        self.transact(
            &proto::assign_song_preset(
                song_slot as u64,
                song_preset_slot as u64,
                DeviceSlot::from(user_list_index).wire(),
                footswitch_label,
                footswitch_color as u64,
                preset_scene_slot as u64,
                footswitch_color_inactive as u64,
            ),
            SONG_SETTER_ECHO_MS,
        )?;
        let rows = self.song_presets(song_slot)?;
        if rows.is_empty() {
            return Err(format!(
                "assigned song {song_slot} footswitch {song_preset_slot}, but its rows \
                 couldn't be read back to confirm it"
            ));
        }
        let landed = (song_preset_slot as usize)
            .checked_sub(1)
            .and_then(|i| rows.get(i))
            .is_some_and(|r| {
                !r.is_empty
                    && r.user_preset_slot == DeviceSlot::from(user_list_index).get()
                    && r.preset_scene_slot == preset_scene_slot
            });
        if landed {
            return Ok(());
        }
        Err(format!(
            "the unit did not bind song {song_slot} footswitch {song_preset_slot} to preset \
             {:03} scene slot {preset_scene_slot} (it refuses a scene the preset doesn't \
             have; {BASE_SCENE_SLOT} = the whole preset)",
            DeviceSlot::from(user_list_index).get()
        ))
    }

    /// Reorder a Song row — `moveSongPreset`. Song-internal positions.
    pub fn move_song_preset(&mut self, song_slot: u32, old: u32, new: u32) -> Result<(), String> {
        self.transact(
            &proto::move_song_preset(song_slot as u64, old as u64, new as u64),
            SONG_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Swap two Song rows — `swapSongPreset`.
    pub fn swap_song_preset(&mut self, song_slot: u32, a: u32, b: u32) -> Result<(), String> {
        self.transact(
            &proto::swap_song_preset(song_slot as u64, a as u64, b as u64),
            SONG_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Empty a Song row — `clearSongPreset`.
    pub fn clear_song_preset(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
    ) -> Result<(), String> {
        self.transact(
            &proto::clear_song_preset(song_slot as u64, song_preset_slot as u64),
            SONG_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Make a song the ACTIVE song by loading one of its footswitch presets
    /// (`LoadPreset{ tabEnum=5, songSlot, songPresetSlot, presetSlot }`). Required
    /// before `set_tap_tempo_bpm` targets that song. Fire-and-forget.
    pub fn load_song(
        &mut self,
        song_slot: u32,
        song_preset_slot: u32,
        preset_slot: u32,
    ) -> Result<(), String> {
        self.transact(
            &proto::load_song(
                song_slot as u64,
                song_preset_slot as u64,
                preset_slot as u64,
            ),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Set the global tap-tempo BPM (`SettingsMessage.tapTempoBpm`, originatorId=1),
    /// which the device stores as the **active song's** BPM. Load the target song
    /// first (`load_song`). Fire-and-forget.
    pub fn set_tap_tempo_bpm(&mut self, bpm: f32) -> Result<(), String> {
        self.transact(&proto::set_tap_tempo_bpm(bpm, 1), SONG_SETTER_ECHO_MS)?;
        Ok(())
    }

    // ─── Song CRUD (fire-and-forget setters; verify by re-reading the list) ──
    pub fn add_song(&mut self, name: &str) -> Result<(), String> {
        self.transact(&proto::add_song(name), SONG_LIST_SETTER_ECHO_MS)?;
        Ok(())
    }

    pub fn rename_song(&mut self, song_slot: u32, name: &str) -> Result<(), String> {
        self.transact(
            &proto::rename_song(song_slot as u64, name),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    pub fn remove_song(&mut self, song_slot: u32) -> Result<(), String> {
        self.transact(
            &proto::remove_song(song_slot as u64),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    pub fn set_song_notes(&mut self, song_slot: u32, notes: &str) -> Result<(), String> {
        self.transact(
            &proto::set_song_notes(song_slot as u64, notes),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    pub fn set_song_bpm_active(&mut self, song_slot: u32, active: bool) -> Result<(), String> {
        self.transact(
            &proto::set_song_bpm_active(song_slot as u64, active),
            SONG_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    // ─── Setlist CRUD (fire-and-forget setters) ─────────────────────────────
    pub fn add_setlist(&mut self, name: &str) -> Result<(), String> {
        self.transact(&proto::add_setlist(name), SONG_LIST_SETTER_ECHO_MS)?;
        Ok(())
    }

    pub fn rename_setlist(&mut self, setlist_slot: u32, name: &str) -> Result<(), String> {
        self.transact(
            &proto::rename_setlist(setlist_slot as u64, name),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    pub fn remove_setlist(&mut self, setlist_slot: u32) -> Result<(), String> {
        self.transact(
            &proto::remove_setlist(setlist_slot as u64),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    pub fn add_setlist_song(&mut self, setlist_slot: u32, song_slot: u32) -> Result<(), String> {
        self.transact(
            &proto::add_setlist_song(setlist_slot as u64, song_slot as u64),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Remove a song from a setlist by its POSITION within the setlist
    /// (`setlist_song_slot`, NOT the global song slot). Fire-and-forget.
    pub fn remove_setlist_song(
        &mut self,
        setlist_slot: u32,
        setlist_song_slot: u32,
    ) -> Result<(), String> {
        self.transact(
            &proto::remove_setlist_song(setlist_slot as u64, setlist_song_slot as u64),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Reorder a song within a setlist by POSITION (both indices are positions
    /// within the setlist, NOT global song slots). Fire-and-forget.
    pub fn move_setlist_song(
        &mut self,
        setlist_slot: u32,
        old_pos: u32,
        new_pos: u32,
    ) -> Result<(), String> {
        self.transact(
            &proto::move_setlist_song(setlist_slot as u64, old_pos as u64, new_pos as u64),
            SONG_LIST_SETTER_ECHO_MS,
        )?;
        Ok(())
    }

    /// Request on this open session (batch 3: answered at once after the handshake) and
    /// harvest, re-sending up to 3 times. `None` = no usable reply.
    pub(super) fn request_harvest<T>(
        &mut self,
        req: &[u8],
        pumps: u32,
        harvest: impl Fn(&Self) -> Option<T>,
    ) -> Result<Option<T>, String> {
        for _ in 0..3 {
            self.clear_raw();
            self.send_and_collect_alive(req, HARVEST_REQUEST_MS)?;
            for _ in 0..pumps {
                if let Some(r) = harvest(self) {
                    return Ok(Some(r));
                }
                self.pump_collect_alive(HARVEST_POLL_MS)?;
            }
        }
        Ok(None)
    }

    /// Read song `song_slot`'s rows on this session. Empty Vec = no usable reply (an
    /// empty song still returns its all-empty rows).
    pub(crate) fn song_presets(&mut self, song_slot: u32) -> Result<Vec<SongPresetRecord>, String> {
        let req = proto::song_preset_list_request(song_slot as u64, Some(proto::BATCH_DRAIN));
        let rows = self.request_harvest(&req, 6, |s| {
            Some(s.harvest_song_presets(song_slot)).filter(|r| !r.is_empty())
        })?;
        Ok(rows.unwrap_or_default())
    }

    /// Read the whole song list on this session, strict (complete replies only).
    pub(crate) fn song_list_strict(&mut self) -> Result<Option<Vec<SongRecord>>, String> {
        let req = proto::song_list_request(Some(proto::BATCH_DRAIN));
        self.request_harvest(&req, 8, Self::harvest_songs_strict)
    }

    /// Read the whole setlist list on this session, strict (complete replies only). A
    /// batch-3 request is answered mid-session, the same shape as [`Self::song_list_strict`].
    pub(crate) fn setlist_list_strict(&mut self) -> Result<Option<Vec<SetlistRecord>>, String> {
        let req = proto::setlist_list_request(Some(proto::BATCH_DRAIN));
        self.request_harvest(&req, 8, Self::harvest_setlists_strict)
    }

    /// Read setlist `setlist_slot`'s member song slots on this session, strict (complete
    /// replies only), with the trailing `songSlot == 0` padding dropped
    /// ([`trim_setlist_padding`]): index `i` is device position `i + 1`.
    pub(crate) fn setlist_songs_strict(
        &mut self,
        setlist_slot: u32,
    ) -> Result<Option<Vec<u32>>, String> {
        let req = proto::setlist_song_list_request(setlist_slot as u64, Some(proto::BATCH_DRAIN));
        let members =
            self.request_harvest(&req, 8, |s| s.harvest_setlist_songs_strict(setlist_slot))?;
        Ok(members.map(trim_setlist_padding))
    }

    /// Decode `song_slot`'s Song-preset reply from the accumulated streams. Largest
    /// record set wins (a complete multi-packet response beats a stray/partial
    /// frame); a reply for another song is ignored.
    pub fn harvest_song_presets(&self, song_slot: u32) -> Vec<SongPresetRecord> {
        let text = |r: &[(u32, proto::Val)], f| {
            proto::first_bytes(r, f)
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default()
        };
        self.streams()
            .iter()
            .filter_map(|s| proto::song_preset_list_records(&s.body, song_slot as u64))
            .map(|recs| {
                recs.iter()
                    .map(|r| SongPresetRecord {
                        is_empty: proto::first_varint(r, 1).unwrap_or(0) != 0,
                        user_preset_slot: proto::first_varint(r, 2).unwrap_or(0) as u32,
                        preset_scene_slot: proto::first_varint(r, 5).unwrap_or(0) as u32,
                        preset_scene_name: text(r, 6),
                        footswitch_label: text(r, 3),
                        footswitch_color: proto::first_varint(r, 4).unwrap_or(0) as u32,
                        footswitch_color_inactive: proto::first_varint(r, 7).unwrap_or(0) as u32,
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
            .unwrap_or_default()
    }

    // ─── Strict, completeness-validated harvests ────────────────────────────
    // (The non-strict `harvest_songs`/`harvest_setlists`/`harvest_setlist_songs`
    // were removed once every read path moved to the fail-closed strict variants —
    // a tolerant decode can return a tail-truncated list, which these reads must
    // never do. The strict harvests + `*_records_strict` decoders are the only tier.)
    // Return `Some` only if a reassembled stream decodes as a COMPLETE list
    // response (no tail truncation); `None` if only truncated/absent responses
    // are present. The caller retries until `Some` (fail-closed) — this is what
    // makes the multi-packet reads reliable despite concurrent-stream truncation.

    pub fn harvest_songs_strict(&self) -> Option<Vec<SongRecord>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::song_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .enumerate()
                    .map(|(i, r)| SongRecord {
                        slot: (i + 1) as u32,
                        name: proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                        notes: proto::first_bytes(r, 2)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                        bpm_active: proto::first_varint(r, 3).unwrap_or(0) != 0,
                        bpm: proto::first_varint(r, 4).unwrap_or(0) as u32,
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }

    pub fn harvest_setlists_strict(&self) -> Option<Vec<SetlistRecord>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::setlist_list_records_strict(&s.body))
            .map(|records| {
                records
                    .iter()
                    .enumerate()
                    .map(|(i, r)| SetlistRecord {
                        slot: (i + 1) as u32,
                        name: proto::first_bytes(r, 1)
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }

    pub fn harvest_setlist_songs_strict(&self, setlist_slot: u32) -> Option<Vec<u32>> {
        self.streams_final()
            .iter()
            .filter_map(|s| proto::setlist_song_list_records_strict(&s.body, setlist_slot as u64))
            .map(|records| {
                records
                    .iter()
                    .map(|r| proto::first_varint(r, 1).unwrap_or(0) as u32)
                    .collect::<Vec<_>>()
            })
            .max_by_key(|v| v.len())
    }
}
