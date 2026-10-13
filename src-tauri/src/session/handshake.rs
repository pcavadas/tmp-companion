//! Opening a session: the connect variants, the first-connect handshake burst with its
//! `batchStatus` groups, the slot-read burst shapes and the in-burst firmware read.
//! Transport set.

use super::*;

/// Burst shapes for [`Session::connect_slotread`] (field-8 investigation).
#[derive(Clone, Copy, Debug)]
pub enum SlotReadBurst {
    /// Full Pro Control burst, read appended last (the classic AC1 spike — 0/25 on
    /// 1.8.45 while its window was silent).
    Classic,
    /// Read injected right after `connection_request` + the My Presets list
    /// request, BEFORE the flood; the standard burst remainder follows.
    Early,
    /// `connection_request` + My Presets request + the read, nothing else —
    /// tests whether the device answers without the full load-bearing burst.
    Minimal,
}

impl Session {
    /// Open the device (seizing it) and run the full first-connect handshake
    /// with the default pump windows (no preset-data JSON fetch). For
    /// measure/capture-only connections, prefer `connect_lean()` below.
    pub fn connect() -> Result<Session, String> {
        Ok(Self::connect_inner(false, None, false, false)?)
    }

    /// `connect` with the handshake pump windows trimmed ×0.25 (~660 → ~165 ms).
    /// ONLY for pure measure/capture sessions: connections that (a) never read
    /// handshake- or push-accumulated data (`list_my_presets` / `active_preset_name` /
    /// `confirm_active` / slot reads) and (b) never write-then-save. The trimmed
    /// windows are HW-validated capture-byte-identical on those paths (fw 1.8.45,
    /// ~10 sessions incl. ×0.1); write/save sessions keep the full windows because
    /// their device-side timing cliffs (the ~700 ms scene-write window, chunked
    /// ftsw edits) were NOT part of that A/B — do not widen the lean set without a
    /// dedicated write-lands HW bisect.
    pub fn connect_lean() -> Result<Session, String> {
        Ok(Self::connect_inner(false, None, false, true)?)
    }

    /// Like `connect`, but the handshake also waits out the drain's
    /// `currentPresetDataChanged` (field 3) JSON (adds ~2 s). Use for block enumeration (`current_preset_blocks`), not leveling.
    pub fn connect_for_discovery() -> Result<Session, String> {
        Ok(Self::connect_inner(true, None, false, false)?)
    }

    /// AC1 spike: run the handshake with an extra slot-addressed read
    /// request injected INSIDE the burst (the only window the device answers
    /// data requests). The reply (presetDataChanged 9 or exportPresetResponse 116) is
    /// then harvested from the accumulated streams by the caller.
    pub fn connect_with_burst_request(extra: &[u8]) -> Result<Session, String> {
        Ok(Self::connect_inner(
            false,
            Some(extra.to_vec()),
            false,
            false,
        )?)
    }

    /// Slot-read investigation (`probe --slotread-x`): connect with the
    /// slot-addressed read injected at a chosen position in the burst. The
    /// classic AC1 spike appends it LAST, behind the three preset lists + the ~17 KB
    /// ProductProfile. (The suspected "ProductProfile collision" was the inactivity
    /// drop: the device never interleaves replies.) `Early` fires the read before the
    /// flood requests, `Minimal` never sends the flood at all.
    /// Read-only — sends NO LoadPreset.
    pub fn connect_slotread(variant: SlotReadBurst, extra: &[u8]) -> Result<Session, String> {
        if matches!(variant, SlotReadBurst::Classic) {
            return Self::connect_with_burst_request(extra);
        }
        let hid = open_transport()?;
        let mut s = Session {
            hid: TrackedHid::new(hid),
            raw: Vec::new(),
            fw_version: None,
            handshake_done: false,
        };
        s.send_and_collect(&proto::connection_request(), SLOTREAD_CONNECT_MS)?;
        s.send_and_collect(&proto::preset_list_request(1, 1), SLOTREAD_BURST_STEP_MS)?; // My Presets
                                                                                        // The read fires here — before favorites/factory/cloud/ProductProfile
                                                                                        // can flood the unkeyed framing.
        s.send_and_collect_alive(extra, SLOTREAD_READ_MS)?;
        if matches!(variant, SlotReadBurst::Early) {
            // Complete the standard burst so the device sees the full
            // load-bearing sequence (tests placement, not burst trimming).
            s.send_and_collect(&proto::favorite_list_request(2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(&proto::preset_list_request(4, 2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(&proto::preset_list_request(3, 2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(&proto::product_profile_request(2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(
                &proto::current_preset_info_request(2),
                SLOTREAD_BURST_STEP_MS,
            )?;
            s.send_and_collect(&proto::settings_field66(2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(&proto::userir_field2(2), SLOTREAD_BURST_STEP_MS)?;
            s.send_and_collect(&proto::current_preset_data_request(3), SLOTREAD_DRAIN_MS)?;
        }
        Ok(s)
    }

    /// Connect and request the firmware version inside the handshake burst
    /// (`currentFwRequest`, no batch — see [`proto::current_fw_request`]). The
    /// reply (`currentFwResponse`) is harvested into `fw_version`. Used by
    /// `probe --fw`; leveling uses the lean [`Session::connect`] (no fw
    /// request) since it doesn't need it.
    ///
    /// The request rides the batch-2 group, BEFORE `current_preset_data_request`
    /// — sent after that batch-3 request, its reply queues behind the ~1.2 s
    /// handshake drain and a short window misses it, so it can't reuse the generic
    /// `extra_burst` slot (which fires last). INFERRED from the same queueing
    /// measured on a delayed `currentPresetInfoRequest`; `currentFwRequest` itself
    /// was not measured.
    ///
    /// Typed, because its caller tells the failures apart: the monitor's stored connect error
    /// is what `connect_device` reports, and the frontend shows "please connect" for a
    /// [`DeviceError::NotFound`] or [`DeviceError::NotReady`] instead of an error.
    pub fn connect_with_firmware() -> Result<Session, DeviceError> {
        let mut s = Self::connect_inner(false, None, true, false)?;
        s.fw_version = s
            .streams()
            .iter()
            .find_map(|st| extract_fw_version(&st.body));
        Ok(s)
    }

    pub(super) fn connect_inner(
        fetch_preset_json: bool,
        extra_burst: Option<Vec<u8>>,
        request_firmware: bool,
        lean: bool,
    ) -> Result<Session, DeviceError> {
        let hid = open_transport()?;
        let mut s = Session {
            hid: TrackedHid::new(hid),
            raw: Vec::new(),
            fw_version: None,
            handshake_done: false,
        };
        // A handshake whose report write the OS refused is the not-ready unit (see
        // `hid::open_device_failure`), told apart by a counter rather than the message.
        let refusals = crate::hid::report_write_refusals();
        s.handshake(fetch_preset_json, extra_burst, request_firmware, lean)
            .map_err(|e| crate::hid::open_device_failure(e, refusals))?;
        Ok(s)
    }

    /// Construct a `Session` over an arbitrary [`HidTransport`] WITHOUT the device
    /// handshake — the test seam that lets `sim_device::SimDevice` (an in-memory fake)
    /// stand in for real hardware, so the held-session edit/level orchestration is
    /// exercised end-to-end in `cargo test`. `batch` starts at 4 (the post-handshake
    /// value) to mirror a connected session; structural edits and setters carry no
    /// `batchStatus`, so the exact value is immaterial to the paths under test.
    #[cfg(test)]
    pub(crate) fn from_transport(hid: Box<dyn HidTransport>) -> Session {
        Session {
            hid: TrackedHid::new(hid),
            raw: Vec::new(),
            fw_version: None,
            handshake_done: false,
        }
    }

    /// Firmware version the device reported during the handshake (e.g.
    /// "1.7.75"); only populated by [`Session::connect_with_firmware`]. None if
    /// the burst didn't carry a `currentFwResponse`.
    pub fn firmware_version(&self) -> Option<String> {
        self.fw_version.clone()
    }

    /// Full first-connect handshake, replicating the device's captured 1.7.2
    /// sequence. The device only answers once it
    /// has seen this sequence. Crucially we ACCUMULATE every report into
    /// `self.raw` (never discard): the `PresetListResponse` for My Presets
    /// arrives early here, and `list_my_presets` reads it from the accumulator.
    /// Accumulating from the first send also keeps multi-packet framing intact
    /// (every `0x33` stream-start is captured).
    pub(super) fn handshake(
        &mut self,
        fetch_preset_json: bool,
        extra_burst: Option<Vec<u8>>,
        request_firmware: bool,
        lean: bool,
    ) -> Result<(), String> {
        // Replicate Pro Control's first-connect sequence, INCLUDING its exact
        // batchStatus values. fw 1.8.58 (static RE, HW-confirmed): batchStatus 1/2
        // only QUEUE a request, 3 queues it and then answers the whole queue in
        // order, 0/absent answers at once, and ≥4 is DROPPED with no reply. The
        // queue survives a lapse and a reconnect, and a later drain answers
        // whoever is on HID then — so every queued request here is closed by the
        // batch-3 `currentPresetDataRequest`, and nothing after the handshake may
        // leave a 1/2 behind (see [`proto::BATCH_DRAIN`]).
        //
        // The request SEQUENCE is identical in both window sets — `lean` only trims
        // how long the host pumps for replies it will never read (see
        // `connect_lean`). TMP_HANDSHAKE_SCALE is a diagnostic env override for
        // probe bisects on top of either set.
        let base = handshake_base_windows(lean);
        let scale = std::env::var("TMP_HANDSHAKE_SCALE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok());
        let hs = |ms: u64| handshake_window(ms, scale);
        self.send_and_collect(&proto::connection_request(), hs(base[0]))?;
        self.send_and_collect(&proto::preset_list_request(1, 1), hs(base[1]))?; // My Presets
        self.send_and_collect(&proto::favorite_list_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::preset_list_request(4, 2), hs(base[1]))?; // Factory
        self.send_and_collect(&proto::preset_list_request(3, 2), hs(base[1]))?; // Cloud
        self.send_and_collect(&proto::product_profile_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::current_preset_info_request(2), hs(base[1]))?;
        self.send_and_collect(&proto::settings_field66(2), hs(base[1]))?;
        self.send_and_collect(&proto::userir_field2(2), hs(base[1]))?;
        // Firmware read rides the batch-2 group (no batchStatus), BEFORE the
        // batch-3 current_preset_data_request — sent after that, the
        // `currentFwResponse` reply queues behind the ~1.2 s handshake drain and
        // this short pump misses it (INFERRED: only a delayed
        // `currentPresetInfoRequest` was measured, not `currentFwRequest`).
        // The reply is a tiny single 0x35 frame, so a short pump suffices.
        if request_firmware {
            self.send_and_collect(&proto::current_fw_request(), FW_READ_MS)?;
        }
        self.send_and_collect(&proto::current_preset_data_request(3), hs(base[2]))?;
        self.handshake_done = true;
        // The drain's last reply is `currentPresetDataChanged` (field 3), the current
        // preset's JSON: a multi-packet blob (~2 s). Only a discovery connect waits
        // it out.
        if fetch_preset_json {
            self.pump_collect_alive(HANDSHAKE_PRESET_JSON_MS)?;
        }
        // AC1 spike: inject a slot-addressed read inside the burst window. The
        // device only answers data requests while actively streaming this burst
        // (a drain drops it to the silent standalone state), so we send WITHOUT
        // draining and tolerate concurrent handshake streams in reassembly.
        if let Some(extra) = extra_burst {
            self.send_and_collect_alive(&extra, EXTRA_BURST_MS)?;
        }
        Ok(())
    }
}
