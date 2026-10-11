//! Diagnostics for probe and investigation paths: raw dumps, frame summaries and the
//! slot-read and active-graph reports. Has its own timing loops. Transport set.

use super::*;

impl Session {
    /// Diagnostic: send `body`, collect the reply, and return a dump of the reply
    /// streams (count + per-stream top-level field numbers, with songMessage[11] /
    /// setlistMessage[12] / presetMessage[2] expanded to their inner field numbers).
    /// Used to tell a silent-ignore (no reply) from an error reply.
    pub fn send_and_dump(&mut self, body: &[u8], ms: u64) -> Result<String, String> {
        self.raw.clear();
        self.send_and_collect_alive(body, ms)?;
        let streams = self.streams();
        let mut out = format!("  reply streams: {}\n", streams.len());
        for (i, s) in streams.iter().enumerate() {
            let top = proto::parse(&s.body);
            let mut desc = Vec::new();
            for (f, _) in &top {
                if matches!(*f, 2 | 11 | 12) {
                    if let Some(b) = proto::first_bytes(&top, *f) {
                        let inner: Vec<u32> = proto::parse(b).iter().map(|(g, _)| *g).collect();
                        desc.push(format!("{f}{inner:?}"));
                        continue;
                    }
                }
                desc.push(f.to_string());
            }
            out += &format!("    [{i}] {}B fields={}\n", s.body.len(), desc.join(","));
        }
        Ok(out)
    }

    /// Push-listener experiment (probe --listen): park on the open post-handshake
    /// connection and print every inbound message stream as it completes, with a
    /// first-seen timestamp — the discovery tool for the unit's unsolicited pushes
    /// (PresetLoaded 2[11], SceneLoaded 2[102], CurrentPresetInfoChanged 2[22], …)
    /// fired by footswitch taps / scene recalls / preset changes ON THE UNIT.
    /// `hb_ms` > 0 sends a ConnectionHeartbeat every `hb_ms` MILLISECONDS (set 0 to
    /// test whether pushes flow without one). Pro Control holds the live-controller
    /// session with a dense ~250 ms (4/sec) heartbeat; our earlier 10 000 ms cadence
    /// let the session lapse → the device answered everything with `connectionError`.
    /// This is the keepalive-cadence test: at ~250 ms do the errors stop and the
    /// unit's footswitch/scene pushes (SceneLoaded 102 / PresetLoaded 11 / …) start
    /// arriving? `poll_secs` > 0 also re-sends the current-preset requests each that
    /// many seconds. Read-only apart from the heartbeat + poll requests.
    pub fn listen_dump(&mut self, seconds: u64, hb_ms: u64, poll_secs: u64) -> Result<(), String> {
        let start = std::time::Instant::now();
        let mut last_hb = std::time::Instant::now();
        let mut last_poll = std::time::Instant::now();
        // Pump in short windows so a sub-second heartbeat can actually fire on time
        // (a 700 ms window would cap the cadence at ~1.4/sec). 150 ms lets ~250 ms
        // heartbeats land close to PC's 4/sec.
        let pump_ms = if hb_ms > 0 { hb_ms.clamp(40, 150) } else { 200 };
        let mut first_seen: Vec<f32> = Vec::new();
        let mut printed = 0usize;
        println!(
            "[listen] handshake done, parked for {seconds}s (heartbeat: {}; poll: {}). Drive the unit NOW: tap footswitches, recall scenes, change presets.",
            if hb_ms > 0 { format!("every {hb_ms}ms") } else { "OFF".to_string() },
            if poll_secs > 0 { format!("every {poll_secs}s") } else { "OFF".to_string() }
        );
        while start.elapsed().as_secs() < seconds {
            self.pump_silent(pump_ms)?;
            let streams = self.streams_final();
            for _ in first_seen.len()..streams.len() {
                first_seen.push(start.elapsed().as_secs_f32());
            }
            // Print every stream except the newest (it may still be growing).
            while printed + 1 < streams.len() {
                println!(
                    "[t+{:>7.1}s] {}",
                    first_seen[printed],
                    describe_push(&streams[printed].body)
                );
                printed += 1;
            }
            if hb_ms > 0 && last_hb.elapsed().as_millis() as u64 >= hb_ms {
                self.heartbeat()?;
                last_hb = std::time::Instant::now();
            }
            if poll_secs > 0 && last_poll.elapsed().as_secs() >= poll_secs {
                // Poll the current-preset state on the OPEN connection. If the device
                // answers these standalone (the in-handshake-only requests do NOT —
                // known gotcha), the reply streams reflect the live scene.
                self.hid
                    .send(&proto::current_preset_info_request(proto::BATCH_DRAIN))?;
                self.hid
                    .send(&proto::current_preset_data_request(proto::BATCH_DRAIN))?;
                println!(
                    "[t+{:>7.1}s] -> poll sent (currentPresetInfo + currentPresetData, batch=3)",
                    start.elapsed().as_secs_f32()
                );
                last_poll = std::time::Instant::now();
            }
        }
        // Flush the tail.
        let streams = self.streams_final();
        while printed < streams.len() {
            let t = first_seen
                .get(printed)
                .copied()
                .unwrap_or_else(|| start.elapsed().as_secs_f32());
            println!("[t+{:>7.1}s] {}", t, describe_push(&streams[printed].body));
            printed += 1;
        }
        println!("[listen] done: {printed} streams in {seconds}s");
        Ok(())
    }

    /// Diagnostic: the magic/len sequence of the raw inbound reports collected so far
    /// (`<magic>/<body_len>` per frame). Reveals whether a multi-packet response ends
    /// in a 0x35 frame and whether foreign 0x33 streams are interleaved.
    pub fn raw_frame_summary(&self) -> String {
        self.raw
            .iter()
            .filter(|d| d.len() >= 4 && d[0] == 0x00)
            .map(|d| format!("{:02x}/{}", d[1], d[3]))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Debug aid: summarize every reassembled stream as its TMS top-level field
    /// numbers (and, for the presetMessage carrier, its inner field numbers), so
    /// a missing/redirected reply can be diagnosed without a capture.
    #[allow(dead_code)]
    pub(super) fn stream_field_summary(&self) -> String {
        let streams = self.streams();
        if streams.is_empty() {
            return "(none)".to_string();
        }
        streams
            .iter()
            .map(|s| {
                let top = proto::parse(&s.body);
                let top_fields: Vec<String> = top
                    .iter()
                    .map(|(f, _)| {
                        // Expand presetMessage(2)/settingsMessage(3) into inner field numbers.
                        if *f == TMS_PRESET || *f == TMS_SETTINGS {
                            if let Some(b) = proto::first_bytes(&top, *f) {
                                let inner: Vec<u32> =
                                    proto::parse(b).iter().map(|(g, _)| *g).collect();
                                return format!("{f}{inner:?}");
                            }
                        }
                        f.to_string()
                    })
                    .collect();
                format!("[{}B:{}]", s.body.len(), top_fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn try_export_json(&self) -> Option<Vec<u8>> {
        // Largest wins: a complete multi-packet stream beats a truncated/early one.
        self.streams()
            .iter()
            .filter_map(|s| proto::export_response_preset_json(&s.body))
            .max_by_key(|b| b.len())
    }

    /// Diagnostic: sizes of every reassembled presetDataChanged(9) and
    /// exportPresetResponse(116) presetJson payload, plus the raw report count.
    /// Distinguishes a device-side truncation cap from a host reassembly split.
    pub fn slot_read_diagnostics(&self) -> String {
        let streams = self.streams();
        let mut f9 = Vec::new();
        let mut f116 = Vec::new();
        for s in &streams {
            if let Some(b) = proto::preset_data_changed_json(&s.body) {
                f9.push(b.len());
            }
            if let Some(b) = proto::export_response_preset_json(&s.body) {
                f116.push(b.len());
            }
        }
        format!(
            "raw_reports={} streams={} field9_payloads={f9:?} field116_payloads={f116:?}",
            self.raw.len(),
            streams.len(),
        )
    }

    /// Diagnostic for active-graph discovery: report every current-preset JSON
    /// carrier separately. A larger truncated compressed field-3 payload can be
    /// less useful than a smaller raw field-79 payload, so size alone is not
    /// enough to understand a missing active-preset signal chain graph.
    pub fn active_graph_diagnostics(&self) -> String {
        const CARRIERS: [(u32, u32); 3] = [(3, 1), (79, 1), (9, 3)];
        let streams = self.streams();
        let mut found = Vec::new();
        for s in &streams {
            for &(pm_field, json_field) in &CARRIERS {
                let Some(inner) = dig(&s.body, TMS_PRESET, pm_field) else {
                    continue;
                };
                let Some(payload) = inner
                    .iter()
                    .find(|(f, _)| *f == json_field)
                    .and_then(|(_, v)| v.as_bytes())
                else {
                    continue;
                };
                let decompressed = proto::lz4_block_decompress(payload).ok();
                let decoded = decompressed
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_else(|| String::from_utf8_lossy(payload));
                let prefix: String = decoded.chars().take(120).collect();
                let suffix: String = decoded
                    .chars()
                    .rev()
                    .take(500)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                found.push(format!(
                    "field{pm_field}->{json_field}: payload={}B lz4={} decoded={}B prefix={prefix:?} suffix={suffix:?}",
                    payload.len(),
                    decompressed.as_ref().map_or_else(|| "no".to_string(), |b| format!("{}B", b.len())),
                    decoded.len(),
                ));
            }
        }
        format!(
            "streams={} summary={} carriers=[{}]",
            streams.len(),
            self.stream_field_summary(),
            found.join("; "),
        )
    }
}
