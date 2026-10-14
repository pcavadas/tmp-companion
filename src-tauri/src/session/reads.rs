//! Preset-JSON reads: slot reads, the rich warm-up, payload reassembly and the
//! current-preset accessors built on them. Calls link primitives and `timing` consts only.

use super::*;

/// [`Session::latest_json_payload`]'s body, split into a free function over raw reports so it
/// is unit-testable without a live `Session` (mirrors `best_preset_list_from_reports`). See
/// that method's doc for the three carriers and the latest-wins rule.
///
/// LATEST, never longest: a session opened within the device's inactivity drop of the
/// previous one inherits the rest of that session's queued replies, AHEAD of its own
/// (HW 2026-10-14, fw 1.8.58: the unit keeps sending its queue while no host is open, and
/// what it has not sent yet reaches the next session first). The previous handshake's own
/// field-3 reply ends that queue, so it is the working copy as it was BEFORE the previous
/// session's loads — and an unsaved insert makes it the longer document. Longest-wins read
/// that stale copy after a Doctor discard (3/3 at a 100 ms settle; carriers 2871 stale →
/// 2451 → 2691 → 2691). A latest copy cut short is the caller's error to retry, never a
/// reason to fall back to an older one.
///
/// DANGER: carrier `(9, 3)` is the field-8 slot-read reply's own payload. If a session's
/// accumulated reports carry a field-3 live push AND a later stray field-9 reply, the
/// field-9 content wins — a caller harvesting the LOADED document must clear `raw` first and
/// issue no field-8 read on that session ([`Session::rich_load_collect`] does the former);
/// `session/tests.rs::latest_json_payload_from_reports_takes_a_later_field9_reply` proves the
/// hazard exists at this layer.
pub(super) fn latest_json_payload_from_reports(reports: &[Vec<u8>]) -> Vec<u8> {
    json_payloads_from_reports(reports)
        .into_iter()
        .map(|(_, payload)| payload)
        .next_back()
        .unwrap_or_default()
}

/// Every `presetJson` payload in `reports`, tagged with the PresetMessage carrier field it
/// rode in on (3 / 79 / 9 — see [`latest_json_payload_from_reports`]), in stream order.
pub(super) fn json_payloads_from_reports(reports: &[Vec<u8>]) -> Vec<(u32, Vec<u8>)> {
    pub(super) const CARRIERS: [(u32, u32); 3] = [(3, 1), (79, 1), (9, 3)];
    proto::reassemble_streams(reports)
        .iter()
        .filter_map(|s| {
            CARRIERS.iter().find_map(|&(pm_field, json_field)| {
                let inner = dig(&s.body, TMS_PRESET, pm_field)?;
                inner
                    .iter()
                    .find(|(f, _)| *f == json_field)
                    .and_then(|(_, v)| v.as_bytes())
                    .map(|b| (pm_field, b.to_vec()))
            })
        })
        .collect()
}

/// Extract the firmware version from a `currentFwResponse` frame:
/// settingsMessage(3) → currentFwResponse(2) → data(1), a plain UTF-8 string
/// (e.g. "1.7.75"). The device sends this in reply to a `currentFwRequest`
/// issued in-burst with no batchStatus (see [`Session::connect_with_firmware`]);
/// a standalone request after the burst is rejected with ConnectionError.
pub(crate) fn extract_fw_version(body: &[u8]) -> Option<String> {
    dig(body, TMS_SETTINGS, SETTINGS_CURRENT_FW).and_then(|fields| {
        field1(&fields)
            .and_then(Val::as_bytes)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
    })
}

/// Extract the current preset's display name from the handshake's
/// `CurrentPresetInfoChanged` event. Unlike the truncated preset JSON, this
/// event is small and reliably survives the live discovery burst.
pub(super) fn extract_current_preset_display_name(body: &[u8]) -> Option<String> {
    dig(body, TMS_PRESET, CURRENT_PRESET_INFO_CHANGED).and_then(|fields| {
        field1(&fields)
            .and_then(Val::as_bytes)
            .map(|s| String::from_utf8_lossy(s).into_owned())
    })
}

/// Extract a My Presets list index from `PresetLoaded`. The wire event carries
/// the 1-based device user slot; the Companion uses 0-based list indices.
pub(super) fn extract_loaded_user_slot(body: &[u8]) -> Option<ListIndex> {
    let fields = dig(body, TMS_PRESET, PRESET_LOADED)?;
    let tab_enum = field1(&fields)?.as_u64()?;
    if tab_enum != 1 {
        return None;
    }
    let device_slot = u32::try_from(field_n(&fields, 6)?.as_u64()?).ok()?;
    DeviceSlot::try_from(device_slot).ok().map(ListIndex::from)
}

impl Session {
    /// NON-DESTRUCTIVE slot-addressed preset-JSON read: `presetDataRequest`
    /// (field 8) → `presetDataChanged` (field 9, PLAINTEXT partial). Sends NO
    /// LoadPreset — the unit's selected preset (and any unsaved edits on it)
    /// are never touched. Addressed by 1-based [`DeviceSlot`].
    ///
    /// HW-proven on fw 1.8.45 (`probe --slotread-x` / `--scenes-passive`):
    /// the device answers exactly ONE data request per burst state, and a
    /// re-sent `connection_request` re-arms that state on the OPEN connection
    /// — so a whole-library sweep rides one connection (25/25, ~0.9 s/slot).
    /// The classic full-handshake placement was 0/25 on 1.8.45: the reply queued
    /// behind the ~480-frame flood and was discarded by the inactivity drop during the
    /// silent window ([`HID_INACTIVITY_DROP_MS`]).
    /// `Ok(None)` = the device didn't answer this read (caller counts a miss).
    ///
    /// For a DEDICATED/QUIET session (every `probe_*` sweep + `scan_preset_scenes`):
    /// a session silent past the device's inactivity drop has LAPSED, and the leading
    /// `connection_request` re-arm reopens it before the data request. The read itself
    /// heartbeats, so the next read on the same session finds it live and skips the
    /// re-arm — on a live session the re-arm only draws a `connectionError` (HW: 1
    /// error/read + ~140 ms slower). On the monitor's LIVE-CONTROLLER session use
    /// [`Self::read_slot_preset_json_live`], which never re-arms.
    pub fn read_slot_preset_json(
        &mut self,
        device_slot: DeviceSlot,
    ) -> Result<Option<Vec<u8>>, String> {
        self.read_slot_preset_json_inner(device_slot, false)
    }

    /// Variant for the LIVE monitor session (startup graph fallback + metadata lane):
    /// the session is already armed by its dense heartbeat, so this SKIPS the
    /// `connection_request` re-arm (which would draw a per-read `connectionError`)
    /// and instead keeps that heartbeat alive through the harvest. HW-confirmed
    /// (`probe --slotread-live`): 0 `connectionError`/read vs 1 with the re-arm,
    /// and ~140 ms/read faster, reliable across repeated reads.
    pub fn read_slot_preset_json_live(
        &mut self,
        device_slot: DeviceSlot,
    ) -> Result<Option<Vec<u8>>, String> {
        self.read_slot_preset_json_inner(device_slot, true)
    }

    pub(super) fn read_slot_preset_json_inner(
        &mut self,
        device_slot: DeviceSlot,
        on_live_session: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        self.read_list_preset_json(1, device_slot, on_live_session)
    }

    /// [`Self::read_slot_preset_json`] on any preset list (`list_enum`: 1 = My Presets,
    /// 4 = Factory, 3 = Cloud).
    pub(crate) fn read_list_preset_json(
        &mut self,
        list_enum: u64,
        device_slot: DeviceSlot,
        on_live_session: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        self.raw.clear();
        // Re-arm a lapsed quiet session (see the doc): the write reopens the client
        // first (`TrackedHid`). A small batch-3 request, so nothing is left queued.
        if !on_live_session && self.maybe_lapsed() {
            self.send_and_collect(
                &proto::current_preset_info_request(proto::BATCH_DRAIN),
                SLOT_READ_REARM_MS,
            )?;
        }
        self.send_and_collect(
            &proto::preset_data_request(list_enum, device_slot.wire(), None),
            SLOT_READ_REQUEST_MS,
        )?;
        // Growth-stability harvest: a 17 KB reply is ~290 frames, ~1 s end-to-end; small
        // presets land in the first slice.
        self.pump_until_stable(SLOT_READ_MAX_WINDOWS, SLOT_READ_WINDOW_MS, |s| {
            s.try_preset_data_json().map_or(0, |b| b.len())
        })?;
        Ok(self.try_preset_data_json())
    }

    /// Rich-harvest warmup: the heartbeat/pump turns that establish this session as a live
    /// controller before a [`Session::rich_load_collect`] (the bench intel-session /
    /// `prepass_scene_docs` shape). Used by `discover_blocks_rich`.
    pub(crate) fn rich_warmup(&mut self) -> Result<(), String> {
        for _ in 0..8 {
            self.heartbeat()?;
            self.pump_silent(RICH_WARMUP_BEAT_MS)?;
        }
        Ok(())
    }

    /// Load list-index `slot` for a field-3 harvest: clear `raw` first (this session must
    /// never carry an older stream into the harvest — `latest_json_payload` takes the
    /// LATEST of its carriers, so a leftover arriving late can shadow the fresh push), send `LoadPreset`
    /// via [`Session::send_and_collect`] (NOT [`Session::load_preset`], whose
    /// `transact_eager` discards the reports the field-3 push rides on), then keep pumping
    /// past the 125 hit so the multi-packet push finishes arriving.
    pub(crate) fn rich_load_collect(&mut self, slot: ListIndex) -> Result<(), String> {
        self.raw.clear();
        self.send_and_collect(&proto::load_my_preset(slot.into()), LOAD_ECHO_MS)?;
        for _ in 0..10 {
            self.heartbeat()?;
            self.pump_silent(LIVE_BEAT_MS)?;
        }
        Ok(())
    }

    /// Byte length of the presetJson carrier [`Self::current_preset_value`] would parse —
    /// how much of a field-3 reply has landed so far (the growth-stability gate).
    pub(crate) fn json_payload_len(&self) -> usize {
        self.latest_json_payload().len()
    }

    /// The LATEST reassembled `presetJson` payload in the accumulated streams, across
    /// all three carriers (presetMessage submessage field → presetJson inner field):
    /// currentPresetDataChanged 3→1, currentPresetDataJsonResponse 79→1, and
    /// presetDataChanged 9→3 (the slot-addressed reply). Latest wins, not longest: an
    /// earlier stream can be the previous session's queued reply (see
    /// [`latest_json_payload_from_reports`]).
    pub(super) fn latest_json_payload(&self) -> Vec<u8> {
        latest_json_payload_from_reports(&self.raw)
    }

    /// Diagnostic: every `presetJson` carrier in the accumulated reports as
    /// `(PresetMessage field, payload bytes)` — `3` = the `currentPresetDataChanged` live
    /// push, `79` = `currentPresetDataJsonResponse`, `9` = `presetDataChanged` (the field-8
    /// slot read's reply, the stored document). Names WHICH document a buffer-scraped
    /// "read-back" ([`Self::current_preset_value`]) actually came from — the Copy post-save
    /// verdict line logs it so an online run can tell a late field-3 push from a stored-slot
    /// field-9 reply.
    pub(crate) fn json_payload_carriers(&self) -> Vec<(u32, usize)> {
        json_payloads_from_reports(&self.raw)
            .into_iter()
            .map(|(field, payload)| (field, payload.len()))
            .collect()
    }

    /// HW / Tier-4 diagnostic: capture the FULL `currentPresetDataChanged` (field
    /// 3) preset JSON, decompressed byte-exact (not the lossy-UTF-8 prefix). Holds
    /// a dense ~250 ms ConnectionHeartbeat while pumping so the device treats us as
    /// a live controller and pushes the COMPLETE field-3 (the lean path truncates
    /// at `"scenes"`; a healthy dense-heartbeat session was observed to deliver the
    /// full ~16 KB preset). `slot` (Some) loads that preset first to trigger a fresh
    /// push (non-destructive). Returns the decompressed bytes — which may still be a
    /// device-truncated partial; the caller checks completeness.
    pub fn capture_full_preset_json(
        &mut self,
        slot: Option<ListIndex>,
        settle_ms: u64,
    ) -> Result<Vec<u8>, String> {
        if let Some(idx) = slot {
            self.load_preset(idx)?;
        }
        let steps = (settle_ms / CAPTURE_SETTLE_STEP_MS).max(1);
        for _ in 0..steps {
            self.heartbeat()?;
            self.pump_silent(CAPTURE_SETTLE_STEP_MS)?;
        }
        let payload = self.latest_json_payload();
        if payload.is_empty() {
            return Err(
                "no currentPresetDataChanged (field 3) payload captured — is a preset loaded?"
                    .to_string(),
            );
        }
        proto::lz4_block_decompress(&payload).map_err(|e| format!("LZ4 decompress failed: {e}"))
    }

    /// The current preset's tolerant-parsed JSON document, from the data the
    /// handshake fetched (`currentPresetDataChanged`, field 3 — LZ4, routinely
    /// truncated mid-object; `audioGraph` and usually `scenes` survive). The shared
    /// read under [`current_preset_blocks`] / the per-scene amp pick.
    pub fn current_preset_value(&self) -> Result<serde_json::Value, String> {
        parse_preset_text(&self.buffered_preset_text()?)
    }

    /// The buffered `currentPresetDataChanged` (field 3) document, decoded but unparsed.
    pub(super) fn buffered_preset_text(&self) -> Result<String, String> {
        decode_preset_json(&self.latest_json_payload()).ok_or_else(|| {
            "no preset JSON in handshake — currentPresetDataChanged absent (is a preset loaded?)"
                .to_string()
        })
    }

    /// [`Self::current_preset_value`], refused unless the buffered document's `audioGraph`
    /// arrived whole ([`json_section_complete`]; the scenes tail may still be cut, as it
    /// routinely is on HW). A tolerant parse of a read cut inside the nodes reads as a
    /// shorter roster, so every insert-safety decision (budget, anchor, did it land) reads
    /// through this.
    pub(crate) fn complete_graph_value(&self) -> Result<serde_json::Value, String> {
        let text = self.buffered_preset_text()?;
        if !json_section_complete(&text, "audioGraph") {
            return Err("the working-copy read was cut inside audioGraph".to_string());
        }
        parse_preset_text(&text)
    }

    /// A fresh working-copy read ([`Self::live_audio_graph`]) through
    /// [`Self::complete_graph_value`].
    pub(crate) fn live_complete_value(&mut self) -> Result<serde_json::Value, String> {
        self.live_audio_graph(|v| v.get("audioGraph").is_some())?;
        self.complete_graph_value()
    }

    /// The current preset's leveling-candidate block controls, parsed from the data
    /// the handshake fetched. To enumerate a SPECIFIC slot, load it then open a fresh
    /// `Session` (the handshake fetches whatever preset is current, and a loaded
    /// preset stays current across reconnects). Gated by
    /// [`extract_level_candidates`] — the classifier table, the same one the
    /// footswitch and scene-handle pickers use.
    pub fn current_preset_blocks(&self) -> Result<Vec<LevelBlock>, String> {
        Ok(extract_level_candidates(&self.current_preset_value()?))
    }

    /// The active preset's full signal-chain graph (active-preset signal chain block strip) — every
    /// block per group plus routing template and split/mix controls, parsed from the same
    /// live field-3 partial as [`current_preset_blocks`]. Unlike that level-only
    /// reader, this surfaces every node + its model id + bypass + the routing
    /// fields the strip needs to draw series vs parallel. `name`/`slot` come from
    /// `info` when present (usually absent in the truncated partial → `None`).
    pub fn current_audio_graph(&self) -> Result<ActiveGraph, String> {
        let payload = self.latest_json_payload();
        let text = decode_preset_json(&payload).ok_or_else(|| {
            "no preset JSON in handshake — currentPresetDataChanged absent (is a preset loaded?)"
                .to_string()
        })?;
        let v = tolerant_parse_json(&text)
            .ok_or_else(|| "could not parse preset JSON, even tolerantly".to_string())?;
        let template_hint = extract_partial_json_string(&text, "template");
        let mut graph = extract_active_graph(&v, template_hint.as_deref());
        if !graph.nodes.is_empty() && !is_known_routing_template(graph.template.as_deref()) {
            return Err(format!(
                "active graph truncated before a complete audioGraph.template: {:?}",
                graph.template
            ));
        }
        if graph.name.is_none() {
            graph.name = self
                .streams()
                .iter()
                .rev()
                .find_map(|s| extract_current_preset_display_name(&s.body));
        }
        if graph.slot.is_none() {
            graph.slot = self
                .streams()
                .iter()
                .rev()
                .find_map(|s| extract_loaded_user_slot(&s.body));
        }
        Ok(graph)
    }

    pub(crate) fn try_preset_data_json(&self) -> Option<Vec<u8>> {
        // Largest wins: a complete multi-packet stream beats a truncated/early one.
        // BOTH reassembly rules are tried: `streams()` keeps an interleaved 0x35
        // separate (safe mid-flood) but DROPS the final frame of a complete
        // stream; `streams_final()` folds the trailing 0x35 in. On the passive
        // scene scan the field-9 stream ends in a 0x35 final frame — the
        // `streams()`-only harvest systematically lost the document tail
        // (`"scenes"` lives at the end; HW: 22/25 presets read "scenes unknown"
        // until the final-rule recovery).
        self.streams()
            .iter()
            .map(|s| s.body.clone())
            .chain(self.push_bodies())
            .filter_map(|b| proto::preset_data_changed_json(&b))
            .max_by_key(|b| b.len())
    }

    /// Pump until a slot-read reply stops growing, then return the largest
    /// presetDataChanged (field 9 → presetJson 3) else exportPresetResponse
    /// (field 116 → presetJson 1) payload. For the in-burst AC1 spike
    /// (`connect_with_burst_request`). Keeps pumping while the payload grows so a
    /// large multi-packet preset is fully reassembled before we read it.
    #[cfg(test)] // the in-burst slot-read spike; only the wire golden runs it
    pub fn harvest_slot_read(&mut self) -> Result<Vec<u8>, String> {
        let best = |s: &Self| s.try_preset_data_json().or_else(|| s.try_export_json());
        // Kept alive: the old "device stops at the same byte count" baseline was measured on
        // SILENT windows, whose stop the inactivity drop also produces.
        self.pump_until_stable(SLOT_HARVEST_MAX_WINDOWS, KEEPALIVE_WINDOW_MS, |s| {
            best(s).map_or(0, |b| b.len())
        })?;
        best(self).ok_or_else(|| {
            format!(
                "no slot-read reply (field 9/116). Streams seen: {}",
                self.stream_field_summary()
            )
        })
    }
}
