//! Footswitch assignment commands and the live footswitch read. Calls link primitives
//! and `timing` consts only.

use super::*;

impl Session {
    /// Send `setFootswitchAssignment` (PresetMessage field 54): set ONE function
    /// (`index`, 0-based) on footswitch `addr` to `function_json` (the same JSON shape as
    /// a preset's `ftsw[switch][func]` object). The schema has NO dedicated confirm echo
    /// (unlike `nodeInserted`), so the caller confirms by re-reading the working-copy
    /// `ftsw` ([`Self::live_ftsw`]) or inspecting [`Self::seen_preset_fields`]. Sent
    /// chunked (a `param` functionJson exceeds the 60 B single-report limit), on the live
    /// heartbeat established by [`Self::begin_live_edit`]. `batch=None` = setter framing.
    pub fn set_footswitch_assignment(
        &mut self,
        addr: u32,
        index: u32,
        function_json: &str,
        swap: bool,
        batch: Option<u64>,
    ) -> Result<(), String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::set_footswitch_assignment(
                addr as u64,
                index as u64,
                function_json,
                swap,
                batch,
            ),
            FTSW_EDIT_ECHO_MS,
        )?;
        self.heartbeat()?;
        self.pump_silent(LIVE_BEAT_MS)?;
        Ok(())
    }

    /// Send `clearFootswitchAssignment` (PresetMessage field 55): remove function `index`
    /// from footswitch `addr`. Same confirm model as [`Self::set_footswitch_assignment`].
    pub fn clear_footswitch_assignment(&mut self, addr: u32, index: u32) -> Result<(), String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::clear_footswitch_assignment(addr as u64, index as u64),
            FTSW_EDIT_ECHO_MS,
        )?;
        self.heartbeat()?;
        self.pump_silent(LIVE_BEAT_MS)?;
        Ok(())
    }

    /// Re-prompt and read the live WORKING-COPY `ftsw` array (`currentPresetDataRequest`
    /// → fresh field-3 push). Reflects UNSAVED edits, so it's how a footswitch set/clear is
    /// confirmed (no dedicated echo). `ftsw` sits at byte ~4330 of field-3, before the
    /// scene-tail truncation, so it survives the partial. `None` if no field-3 lands.
    pub fn live_ftsw(&mut self) -> Option<serde_json::Value> {
        self.live_preset_value(|v| v.get("ftsw").is_some())
            .ok()
            .and_then(|v| v.get("ftsw").cloned())
    }
}
