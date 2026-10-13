//! Preset commands: load, level, parameter writes, live-edit, identity confirms, save,
//! clear, rename, import, move and the live working-copy reads. Calls link primitives
//! and `timing` consts only.

use super::*;

/// The save choke point's refusal while the working copy is the unit's empty template.
pub(crate) const TEMPLATE_SAVE_REFUSED: &str =
    "The unit replaced this preset with an empty one (presetError 6). Nothing was saved.";

impl Session {
    /// The active preset's 0-based My Presets list index, from the most recent
    /// `PresetLoaded` echo in this session's accumulated pushes (handshake +
    /// heartbeat). `None` if no echo has arrived yet.
    pub fn loaded_slot(&self) -> Option<ListIndex> {
        self.push_bodies()
            .iter()
            .rev()
            .find_map(|body| extract_loaded_user_slot(body))
            .or_else(|| {
                self.streams()
                    .iter()
                    .rev()
                    .find_map(|st| extract_loaded_user_slot(&st.body))
            })
    }

    /// Re-arm the open connection and ask for a fresh `currentPresetInfoChanged`
    /// (field 22) after a load — the held-edit preamble. Batch 3 so the request is
    /// answered on this session (a 1/2 would only queue, and a later session's
    /// handshake would replay it). Callers then [`Self::await_active_preset`].
    pub(crate) fn rearm_active_info(&mut self) -> Result<(), String> {
        self.send_and_collect(&proto::connection_request(), REARM_CONNECT_MS)?;
        self.send_and_collect(
            &proto::current_preset_info_request(proto::BATCH_DRAIN),
            REARM_INFO_REQUEST_MS,
        )
    }

    /// LoadPreset on the My Presets tab (tabEnum = 1) at **list index** `slot`
    /// (`list_my_presets` position); the device addresses presets by a 1-based userSlot,
    /// which [`DeviceSlot::from`] derives. HW-confirmed on 1.7.75: the Song read reports
    /// `userPresetSlot=2` for the preset at list index 1, and a device `loadPreset(28)`
    /// loads list index 27 — i.e. device userSlot = list + 1.
    pub fn load_preset(&mut self, slot: ListIndex) -> Result<(), String> {
        // Eager pump: the reports are discarded (fire-and-forget), so exit as
        // soon as the echo burst completes instead of burning the full window.
        self.landing_load(|s| s.transact_eager(&proto::load_my_preset(slot.into()), LOAD_ECHO_MS))?;
        Ok(())
    }

    /// Raw loadPreset with BOTH `preset_slot` and `tab_enum` passed VERBATIM (no
    /// +1, no hardcoded tab) — the `probe --load-probe` experiment for the factory
    /// bank. Same fire-and-forget `transact_eager` path as `load_preset` (which is
    /// HW-proven to change the active preset).
    // `pub(crate)`, not `pub`: raw device-slot addressing bypasses the
    // session-owned 0-based→1-based translation, so only the deliberate verbatim
    // experiment arm (`probe_api::slot_read::probe_load_probe`) may reach it —
    // everything else goes through `load_preset`/`load_factory_preset`.
    pub(crate) fn load_preset_raw(
        &mut self,
        preset_slot: u64,
        tab_enum: u64,
    ) -> Result<(), String> {
        self.landing_load(|s| {
            s.transact_eager(&proto::load_preset(preset_slot, tab_enum), LOAD_ECHO_MS)
        })?;
        Ok(())
    }

    /// Load a FACTORY preset (tab enum 4) by **0-based list index** — the factory bank
    /// takes the same 1-based slot shape as My Presets, so callers never apply `+ 1`.
    pub fn load_factory_preset(&mut self, list_index: ListIndex) -> Result<(), String> {
        self.load_preset_raw(DeviceSlot::from(list_index).wire(), 4)
    }

    /// Set the whole-preset level (0.0..=1.0). Returns the echoed
    /// `PresetLevelChanged.presetLevel` if reported (the device's confirmation).
    pub fn set_preset_level(&mut self, level: f32) -> Result<Option<f32>, String> {
        self.raw.clear();
        self.send_and_collect(&proto::set_preset_level(level), SET_LEVEL_ECHO_MS)?;
        Ok(self.streams().iter().find_map(|s| {
            dig(&s.body, TMS_PRESET, PRESET_LEVEL_CHANGED)
                .and_then(|f| field1(&f).and_then(|v| v.as_f32()))
        }))
    }

    /// Set one block control via `ChangeParameter` (fire-and-forget setter, no
    /// reply — like `load_preset`). `group_id`/`node_id`/`parameter_id` name the
    /// `dspUnitParameters` entry (see `current_preset_blocks`); `value` is in the
    /// parameter's own units (often 0..1, but e.g. an IR `outputlevel` is in dB).
    /// HW-proven to move loudness; needs no SetNodeSceneEdit and is latched by
    /// re-amp engage, so set it BEFORE engaging (same rule as `set_preset_level`).
    pub fn change_parameter(
        &mut self,
        group_id: &str,
        node_id: &str,
        parameter_id: &str,
        value: f32,
    ) -> Result<(), String> {
        self.transact(
            &proto::change_parameter(group_id, node_id, parameter_id, value),
            CHANGE_PARAMETER_MS,
        )?;
        Ok(())
    }

    /// Set one block BOOL control (e.g. `bypass`) via `ChangeParameter.boolVal`
    /// (field 7, always emitted). Same fire-and-forget setter rule as the float
    /// [`change_parameter`]. Used to force a block active/bypassed while measuring
    /// an off-in-base footswitch block for the bake path.
    pub fn change_parameter_bool(
        &mut self,
        group_id: &str,
        node_id: &str,
        parameter_id: &str,
        value: bool,
    ) -> Result<(), String> {
        self.transact(
            &proto::change_parameter_bool(group_id, node_id, parameter_id, value),
            CHANGE_PARAMETER_MS,
        )?;
        Ok(())
    }

    /// Establish/refresh **live-controller status** with a dense ~200 ms heartbeat.
    /// fw 1.8.45 only accepts structural edits (`replaceNode`/`replaceNodeWithBlock`)
    /// from a live controller; a lapsed session (e.g. after a silent
    /// `drain_until_quiet`) gets EVERY structural request answered with an empty
    /// `connectionError` and silently dropped. Call this AFTER the load and BEFORE
    /// the first edit — and keep editing without long quiet gaps. (HW-confirmed via
    /// a Pro Control USB capture; same reason `device_backup` heartbeats.)
    pub fn begin_live_edit(&mut self) -> Result<(), String> {
        // Warmup count is env-tunable for the perf experiments (E2); 8×200 ms = 1.6 s
        // is the validated default. The held-session architecture pays this ONCE, so
        // it is amortized across a whole bulk run.
        let n = std::env::var("TMP_WARMUP_N")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(8)
            .clamp(1, 32);
        for _ in 0..n {
            self.heartbeat()?;
            self.pump_silent(LIVE_BEAT_MS)?;
        }
        Ok(())
    }

    /// Echo-gated wait (E3): pump (heartbeat) until `active_preset_name()` equals
    /// `name`, or the iteration budget runs out — exits the moment the device's
    /// `currentPresetInfoChanged` (field 22) for the just-loaded preset lands instead
    /// of burning a fixed window. Returns whether it matched. Used by the held-session
    /// edit to confirm the load re-attached the edit context.
    pub fn await_active_preset(&mut self, name: &str, max_iters: u32) -> bool {
        for _ in 0..max_iters {
            if self.active_preset_name().as_deref() == Some(name) {
                return true;
            }
            let _ = self.heartbeat();
            let _ = self.pump_silent(ACTIVE_PRESET_BEAT_MS);
        }
        self.active_preset_name().as_deref() == Some(name)
    }

    /// The active preset's display name, from the handshake's
    /// `currentPresetInfoChanged` (field 22) — used to CONFIRM the edit session is
    /// attached to the intended preset before mutating it (a load that didn't take
    /// leaves a different preset active; editing+saving it corrupts the target slot).
    pub fn active_preset_name(&self) -> Option<String> {
        self.push_bodies()
            .iter()
            .rev()
            .find_map(|b| extract_current_preset_display_name(b))
    }

    /// The working copy's dirty flag, from the latest `currentPresetInfoChanged` (field
    /// 22) `isDirty`. fw 1.8.58 fills it from the same flag that decides whether a
    /// same-slot `loadPreset` really reloads (fw 1.8.58; `getPresetInfo` copies `+0x686`).
    pub(crate) fn active_preset_dirty(&self) -> Option<bool> {
        self.push_bodies()
            .iter()
            .rev()
            .find_map(|b| decode_info_changed(b).map(|(_, dirty, _)| dirty))
    }

    /// Pure read-only check: is the preset at 0-based `list_index` the ACTIVE one?
    /// Prefer the `PresetLoaded` slot echo (identity, immune to duplicate display
    /// names); fall back to the active-preset NAME ONLY when no slot echo has arrived
    /// — a slot echo that names a DIFFERENT slot must win over a possibly-duplicate
    /// name. This is the shared shape of every save-over guard site.
    ///
    /// The name fallback FAILS CLOSED unless the accumulated My Presets list proves
    /// `expected_name` maps to EXACTLY ONE slot and that slot is `list_index`: two
    /// presets can share a display name (or the name can be an empty-slot label), so a
    /// bare name match on a dropped load could otherwise confirm — and then save over —
    /// the WRONG preset. No proven-unique list ⇒ no confirmation.
    pub(crate) fn active_matches(
        &self,
        list_index: ListIndex,
        expected_name: Option<&str>,
    ) -> bool {
        let loaded = self.loaded_slot();
        if loaded == Some(list_index) {
            return true;
        }
        if loaded.is_some() {
            return false;
        }
        let Some(n) = expected_name else {
            return false;
        };
        if n.is_empty() || is_empty_slot_name(n) || self.active_preset_name().as_deref() != Some(n)
        {
            return false;
        }
        self.best_preset_list()
            .is_some_and(|names| name_maps_uniquely(&names, n, list_index))
    }

    /// Confirm the preset at 0-based `list_index` is the ACTIVE one BEFORE a save-over
    /// write. Checks [`Self::active_matches`]; if inconclusive, re-arms the device's
    /// reply state ONCE (`connection_request` → `preset_list_request` →
    /// `current_preset_info_request`) to force a fresh `currentPresetInfoChanged`,
    /// then re-checks. `Err` when NEITHER the slot echo nor the name confirms — the
    /// caller MUST NOT save: a load that didn't take leaves a DIFFERENT preset active,
    /// and saving it overwrites the target slot with the wrong content (HW-demonstrated
    /// data loss). Only call on a fresh/quiet connection — the re-arm draws a
    /// `connectionError` on a dense-heartbeat session (the live-line re-arm gotcha).
    pub fn confirm_active(
        &mut self,
        list_index: ListIndex,
        expected_name: Option<&str>,
    ) -> Result<(), String> {
        if self.active_matches(list_index, expected_name) {
            return Ok(());
        }
        self.rearm_active_info()?;
        if let Some(name) = expected_name {
            self.await_active_preset(name, 8);
        }
        if self.active_matches(list_index, expected_name) {
            return Ok(());
        }
        Err(format!(
            "could not confirm the preset at list index {list_index} is active before \
             saving (slot echo {:?}, active name {:?}, expected {:?}) — refusing the save \
             to avoid overwriting the wrong preset",
            self.loaded_slot(),
            self.active_preset_name(),
            expected_name,
        ))
    }

    /// Prove the loaded working copy is the import before a save writes it over another
    /// slot: a body the firmware rejects at LOAD becomes an EMPTY preset that keeps the
    /// stored name (fw 1.8.58, — its `presetError` 6 is dropped by
    /// `load_preset`'s eager transact), which [`Self::confirm_active`] passes. Re-prompts the working copy (the HW-proven [`Self::live_audio_graph`] seam)
    /// and hands `check` its block roster and the accepted document.
    pub(crate) fn confirm_loaded_body(
        &mut self,
        check: impl FnOnce(&crate::blockcaps::Roster, &serde_json::Value) -> Result<(), String>,
    ) -> Result<(), String> {
        let live = self.live_audio_graph(|v| v.get("audioGraph").is_some())?;
        check(
            &crate::blockcaps::group_roster_of(&live.nodes),
            &self.current_preset_value()?,
        )
    }

    /// Persist the current edit buffer into the slot at list index `list_index` (sent as
    /// its 1-based device slot; see [`Self::load_preset`] for the HW evidence).
    /// The device sends no save ACK; persistence is verified by reload.
    ///
    /// Refuses, sending nothing, while [`Self::template_substituted`]: the working copy is
    /// then the unit's EMPTY template, and a save would overwrite the stored preset with it.
    pub fn save_current_preset(&mut self, list_index: ListIndex) -> Result<(), String> {
        if self.template_substituted() {
            return Err(TEMPLATE_SAVE_REFUSED.to_string());
        }
        self.transact(
            &proto::save_current_preset(list_index.into()),
            PRESET_WRITE_ECHO_MS,
        )?;
        // Stored-preset mutation choke point: EVERY save routes through here, so this
        // one call keeps the Doctor's cached BEFORE clip correct-by-construction
        // (per-command clears were forgotten on ~7 mutating paths). Over-clearing is
        // harmless (the cache is a pure optimization). Same-crate call up into
        // commands/ — accepted for a static reset with no real coupling.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// Clear (delete) the user preset at list index `list_index` — `clearUserPreset`
    /// (field 15), sent as its 1-based device slot. Fire-and-forget; verify by re-listing.
    pub fn clear_user_preset(&mut self, list_index: ListIndex) -> Result<(), String> {
        self.transact(
            &proto::clear_user_preset(list_index.into()),
            PRESET_WRITE_ECHO_MS,
        )?;
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// Rename the CURRENT preset — `renameCurrentPreset` (field 13). Per the Pro
    /// Control capture a rename is "save under a new name": the caller must follow
    /// this with `save_current_preset(slot)` to persist.
    /// Fire-and-forget; verify by reload.
    pub fn rename_current_preset(&mut self, name: &str) -> Result<(), String> {
        self.transact(&proto::rename_current_preset(name), PRESET_WRITE_ECHO_MS)?;
        Ok(())
    }

    /// Re-prompt the device for its live WORKING COPY (`currentPresetDataRequest`, field 2
    /// → a fresh field-3 push) and return the first parse `done` accepts, pumping the reply
    /// in ≤8 × 200 ms slices under the live-controller heartbeat. This is the ONE re-prompt
    /// seam: the buffer is cleared first, so nothing buffered before the request can pass
    /// as the reply — the difference between a read and the buffer scrape the 2026-09-02
    /// Copy stale-cache incident came from. `Err` names the carriers that landed when no
    /// accepted parse arrived within the budget. Keep the wire shape as is: it is the
    /// HW-proven footswitch confirm channel ([`Self::live_ftsw`], fw 1.8.45).
    pub(crate) fn live_preset_value(
        &mut self,
        done: impl Fn(&serde_json::Value) -> bool,
    ) -> Result<serde_json::Value, String> {
        self.clear_raw();
        let _ = self.send_and_collect_alive(
            &proto::current_preset_data_request(3),
            LIVE_VALUE_REQUEST_MS,
        );
        let mut last = String::new();
        for _ in 0..8 {
            let _ = self.heartbeat();
            let _ = self.pump_collect_alive(LIVE_VALUE_POLL_MS);
            match self.current_preset_value() {
                Ok(v) if done(&v) => return Ok(v),
                Ok(_) => last = "a reply parsed but was not accepted".to_string(),
                Err(e) => last = e,
            }
        }
        Err(format!(
            "no accepted working-copy reply ({last}; carriers {:?})",
            self.json_payload_carriers()
        ))
    }

    /// The working-copy GRAPH behind a re-prompt: [`Self::live_preset_value`] until `done`
    /// accepts a parse, then hold until no new report lands for two 200 ms slices (bounded
    /// at 6, a heartbeat per slice — `read_slot_preset_json_inner`'s growth-stability
    /// pattern on the raw report count, which needs no reassembly), re-check `done` on
    /// what landed, then [`Self::current_audio_graph`] with its truncation guard. The Copy read and the
    /// `probe --reprompt-map` COMMIT arm share this exact seam, so the probe's
    /// "save persists after a re-prompt" sample is taken with the product's own timing.
    pub(crate) fn live_audio_graph(
        &mut self,
        done: impl Fn(&serde_json::Value) -> bool,
    ) -> Result<ActiveGraph, String> {
        self.live_preset_value(&done)?;
        let (mut last, mut stable) = (self.raw.len(), 0u32);
        for _ in 0..6 {
            let _ = self.heartbeat();
            let _ = self.pump_silent(LIVE_BEAT_MS);
            let len = self.raw.len();
            if len == last {
                stable += 1;
                if stable >= 2 {
                    break;
                }
            } else {
                stable = 0;
            }
            last = len;
        }
        // The seam's own guarantee: the document handed back is one `done` accepts, even if
        // frames kept landing after the first accepted parse.
        match self.current_preset_value() {
            Ok(v) if done(&v) => self.current_audio_graph(),
            Ok(_) => Err("the working-copy reply changed after it was accepted".to_string()),
            Err(e) => Err(e),
        }
    }

    /// Re-import a full preset to the device. `preset_bytes` is the
    /// raw `.preset` file content (XOR'd compact JSON, the OFFLINE codec's output).
    /// It is LZ4-block compressed and wrapped in `importPresetRequest` (field 117),
    /// then sent as a chunked `0x33/0x34*/0x35` stream — the framing + encoding
    /// reverse-engineered from a Pro Control import capture. The device chooses the slot
    /// (the lowest empty one) and usually sends no `importPresetResponse` (118), so this
    /// returns nothing about where the import landed: callers prove the landing from the
    /// stored row (`replace_inplace::StoredProof`).
    pub fn import_preset(&mut self, preset_bytes: &[u8]) -> Result<(), String> {
        self.import_preset_echo(preset_bytes).map(|_| ())
    }

    /// [`Self::import_preset`], also returning the `(listEnum, presetSlot)` of an
    /// `importPresetResponse` echo when one lands in the ingest wait. Only
    /// `probe --import-reload-check` reads it, to log whether the echo is ever sent.
    pub(crate) fn import_preset_echo(
        &mut self,
        preset_bytes: &[u8],
    ) -> Result<Option<(u32, u32)>, String> {
        // The ONE import choke point: refuse a body the firmware would silently replace.
        // An undecodable body passes through — the firmware's own LZ4/JSON check handles it.
        if let Some(doc) = crate::library::decode_preset_bytes(preset_bytes)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            crate::footswitch::validate_import_body(&doc)?;
        }
        self.raw.clear();
        let payload = proto::lz4_block_compress_stored(preset_bytes);
        let body = proto::import_preset_request(&payload);
        // Longer wait than setters (300 ms): a ~100-frame import takes longer to
        // ingest, and any importPresetResponse echo trails the whole burst — kept alive,
        // and over as soon as the echo lands.
        self.send_chunked_collect(&body, KEEPALIVE_WINDOW_MS)?;
        let mut echo = self.import_echo();
        for _ in 1..IMPORT_ECHO_WAIT_MS.div_ceil(KEEPALIVE_WINDOW_MS) {
            if echo.is_some() {
                break;
            }
            self.keepalive_pump()?;
            echo = self.import_echo();
        }
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(echo)
    }

    /// Keep the session alive up to `max_ms` longer, until an `importPresetResponse`
    /// echo lands. For `probe --import-reload-check`'s echo log only.
    pub(crate) fn await_import_echo(&mut self, max_ms: u64) -> Result<Option<(u32, u32)>, String> {
        let mut echo = self.import_echo();
        for _ in 0..max_ms.div_ceil(KEEPALIVE_WINDOW_MS) {
            if echo.is_some() {
                break;
            }
            self.keepalive_pump()?;
            echo = self.import_echo();
        }
        Ok(echo)
    }

    /// The `(listEnum, presetSlot)` of an `importPresetResponse`(118) echo, if one landed.
    pub(super) fn import_echo(&self) -> Option<(u32, u32)> {
        // ImportPresetResponse{ presetJson=1, listEnum=2, presetSlot=3 }
        self.streams().iter().find_map(|s| {
            let fields = dig(&s.body, TMS_PRESET, 118)?;
            let slot = field_n(&fields, 3)?.as_u64()? as u32;
            let list_enum = field_n(&fields, 2).and_then(Val::as_u64).unwrap_or(0) as u32;
            Some((list_enum, slot))
        })
    }

    /// Relocate a user preset between list indices `old`→`new` — `moveUserPreset`
    /// (field 16), sent as their 1-based device slots. Fire-and-forget; verify by re-listing.
    pub fn move_user_preset(&mut self, old: ListIndex, new: ListIndex) -> Result<(), String> {
        self.transact(
            &proto::move_user_preset(old.into(), new.into()),
            PRESET_WRITE_ECHO_MS,
        )?;
        // Stored-preset mutation — see save_current_preset's choke-point note.
        crate::commands::doctor::clear_doctor_before_cache();
        Ok(())
    }

    /// The active preset's 0-based list index from this session's handshake replies,
    /// waiting up to 2 s (kept alive) for the drain to deliver them — HW (fw 1.8.58):
    /// they land ~1 s after a plain handshake returns. The `PresetLoaded` echo, else the
    /// current preset's name when it maps to exactly ONE slot of the strict My Presets
    /// list. `None` = not provable (duplicate or empty-slot name, no reply). Read it
    /// before anything that clears the accumulator.
    pub(crate) fn active_slot_live(&mut self) -> Option<ListIndex> {
        const WINDOW_MS: u64 = 250;
        const MAX_WAIT_MS: u64 = 2000;
        let mut waited = 0;
        loop {
            if let Some(i) = self.loaded_slot() {
                return Some(i);
            }
            // An inconclusive name (a duplicate) keeps waiting: the echo may still land.
            if let (Some(name), Some(names)) =
                (self.active_preset_name(), self.harvest_preset_list_strict())
            {
                if let Some(i) = unique_list_index(&names, &name) {
                    return Some(i);
                }
            }
            if waited >= MAX_WAIT_MS {
                return None;
            }
            self.pump_collect_alive(WINDOW_MS).ok()?;
            waited += WINDOW_MS;
        }
    }
}
