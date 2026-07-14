//! Probe entry points: slot-addressed preset read (export / forensics / live / dump / listen).

use crate::proto;
use crate::session;
use crate::session::Session;

/// Tier-4 diagnostic: capture the live `currentPresetDataChanged` (field 3) preset
/// JSON on a dense-heartbeat session, write the full decompressed body to
/// `out_path`, and report whether it is COMPLETE (the `scenes` array + `ftsw` map
/// present and the JSON parses to the end) — the go/no-go for live FS-tags and the
/// "truncates at scenes" gotcha. `slot` (Some) loads that preset first to trigger a
/// fresh push. Non-destructive (load + reads only).
pub fn probe_dump_preset_data(slot: Option<u32>, out_path: &str) -> Result<String, String> {
    let raw = Session::connect()?.capture_full_preset_json(slot, 2000)?;
    std::fs::write(out_path, &raw).map_err(|e| format!("write {out_path}: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    // A healthy dense-heartbeat field-3 is ~17 KB and routinely truncates only at
    // the LAST scene's uuid — so a full serde parse fails even though `ftsw` and
    // most of `scenes` ARE present. Detect presence by raw key search (reliable);
    // report the full-parse result separately. `ftsw` (footswitch→scene map) sorts
    // well before `scenes`, so its presence is the FS-tag go/no-go.
    let complete = serde_json::from_str::<serde_json::Value>(&text).is_ok();
    let has_ftsw = text.contains("\"ftsw\"");
    let has_scenes = text.contains("\"scenes\"");
    let scene_names = text.matches("\"sceneName\"").count();
    Ok(format!(
        "wrote {} bytes to {out_path}\n  full-parse-complete: {complete}\n  ftsw present: {has_ftsw}\n  scenes present: {has_scenes} ({scene_names} sceneName entries seen)\n  -> live FS-tags feasible: {}",
        raw.len(),
        if has_ftsw { "YES (ftsw survives in the live field-3 partial)" } else { "NO (ftsw truncated away -> degrade to em-dash)" },
    ))
}

/// Push-listener discovery experiment: full handshake, then park `seconds`
/// printing every inbound stream (the unit's unsolicited pushes) as it lands.
/// Read-only apart from the ConnectionHeartbeat every `hb_ms` MILLISECONDS (≈250 =
/// Pro Control's 4/sec keepalive) and the optional current-preset poll every
/// `poll_secs`.
pub fn probe_listen(seconds: u64, hb_ms: u64, poll_secs: u64) -> Result<(), String> {
    Session::connect()?.listen_dump(seconds, hb_ms, poll_secs)
}

/// AC1: read a library slot's preset JSON over USB and report whether
/// it is a complete preset or a partial. **RESOLVED on 1.7.75 HW:** USB does NOT
/// yield a complete preset — `presetDataRequest` (field 8 → `presetDataChanged`
/// 9, plaintext) returns a per-slot-DETERMINISTIC partial (e.g. slot 0 = 1669 B
/// empty nodes; slot 1 = 17264 B with scenes but cut mid-`uuid`); the device
/// truncates the stream at the source. `exportPresetRequest` (115) is unimplemented
/// (no response). So the canonical full-preset source is OFFLINE `.preset` files;
/// this path serves USB partials (search/inventory/quick reads), not backup.
///
/// The request MUST ride inside the handshake burst with NO batchStatus — a
/// standalone post-handshake request, or one carrying a batch, gets no reply.
pub fn probe_export_preset(list_enum: u32, slot: u32) -> Result<String, String> {
    // Slot-addressed full read on a MINIMAL re-armed burst (connect() +
    // read_slot_preset_json), NOT the full-handshake Classic burst. The field-9
    // reply's unkeyed 0x33/0x34/0x35 framing collides with the ~17 KB ProductProfile
    // flood when the read is appended to the full handshake (HW `probe --slotread-x`:
    // Classic burst = NO REPLY, every minimal/re-arm variant = clean reply). This is
    // the same reliable path scan_preset_scenes / probe_slot_json use. (`read_slot_preset_json`
    // addresses My Presets, so `list_enum` is effectively 1 here — the only validated case.)
    let _ = list_enum;
    let raw = {
        let mut s = Session::connect()?;
        s.drain_until_quiet(250, 20)?;
        s.read_slot_preset_json(slot)?
            .ok_or_else(|| format!("slot {slot}: field-8 slot read returned no JSON"))?
    };

    // presetDataChanged.presetJson is plaintext; currentPresetDataChanged is LZ4.
    // Try LZ4 first so this reporter also handles an LZ4 carrier if one appears.
    let (decoded, encoding) = match proto::lz4_block_decompress(&raw) {
        Ok(d) if !d.is_empty() => (d, "lz4-block"),
        _ => (raw.clone(), "plaintext"),
    };
    let text = String::from_utf8_lossy(&decoded);

    let parses_complete = serde_json::from_str::<serde_json::Value>(&text).is_ok();
    let has = |needle: &str| text.contains(needle);
    let verdict = if parses_complete && has("\"scenes\"") {
        "FULL — complete JSON with scenes"
    } else if has("\"scenes\"") {
        "PARTIAL — 'scenes' present but JSON truncated (device-side cut; OFFLINE needed for full)"
    } else {
        "PARTIAL — no 'scenes' (truncated early; OFFLINE needed for full)"
    };

    // TMP_EXPORT_RAW=<path>: dump the full decoded JSON for offline diffing (the
    // bisect readback classifies scene-overlay vs base-leak vs dropped writes).
    if let Ok(path) = std::env::var("TMP_EXPORT_RAW") {
        std::fs::write(&path, text.as_bytes()).map_err(|e| format!("TMP_EXPORT_RAW: {e}"))?;
    }

    let preview: String = text.chars().take(200).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(120)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Ok(format!(
        "[probe --export] listEnum={list_enum} slot={slot} via presetDataRequest(field 8, no-batch, in-burst)\n\
         raw_bytes={} encoding={encoding} decoded_bytes={}\n\
         parses_complete_json={parses_complete}\n\
         has_scenes={} has_ftsw={} has_exp={} has_audioGraph={}\n\
         VERDICT: {verdict}\n\
         --- head(200) ---\n{preview}\n--- tail(120) ---\n{tail}\n",
        raw.len(),
        decoded.len(),
        has("\"scenes\""),
        has("\"ftsw\""),
        has("\"exp\""),
        has("\"audioGraph\""),
    ))
}

/// `probe --factory-list`: connect (the full handshake already requests the
/// My/Factory/Cloud lists) and print every FACTORY preset as `slot<TAB>name`,
/// one per line. Empty slots print as their device-supplied label (usually `--`)
/// — not filtered, so the slot numbering stays 1:1 with the list index.
/// Read-only: list harvest only, no LoadPreset.
pub fn probe_factory_list() -> Result<String, String> {
    let entries = Session::connect()?.list_factory_presets()?;
    let mut out = String::new();
    for e in &entries {
        out.push_str(&format!("{}\t{}\n", e.slot, e.name));
    }
    out.push_str(&format!("({} factory presets)\n", entries.len()));
    Ok(out)
}

/// `probe --load-probe <slot> <tabEnum>`: send a RAW `loadPreset(presetSlot=slot,
/// tabEnum=tabEnum)` (both values passed VERBATIM — the human experiments with
/// 0-/1-based slots and unknown Factory tabEnums), then read back the ACTIVE
/// preset's identity (name + first block model) from the `currentPresetDataChanged`
/// (field 3) / `currentPresetInfoChanged` (field 22) pushes.
///
/// Non-destructive: only LoadPreset (changes the active preset) — never saves.
/// The load is fired on a QUIET line (a mid-flood request is dropped device-side)
/// via `send_and_collect` (NOT the fire-and-forget `Session::load_preset`, which
/// discards the reports the field-3 push rides on); the field-3 push arrives only
/// on a CHANGE, coaxed here with a few dense heartbeats (the monitor's cadence).
pub fn probe_load_probe(slot: u64, tab_enum: u64) -> Result<String, String> {
    let mut s = Session::connect()?;
    // Fire-and-forget via the SAME transact_eager path `session::load_preset` uses
    // (HW-proven to change the active preset); both slot + tabEnum verbatim. The
    // TMP's own screen is the oracle for which preset went active — the field-3
    // name readback proved unreliable on a lean one-shot session.
    s.load_preset_raw(slot, tab_enum)?;
    Ok(format!(
        "sent loadPreset slot={slot} tabEnum={tab_enum} — check the TMP screen for the active preset\n"
    ))
}

/// Frame/marker forensics over a session's raw accumulated reports: frame
/// counts by magic, plus plaintext-JSON marker counts in the concatenated
/// frame bodies — the field-9 `presetJson` is PLAINTEXT (not LZ4), so its
/// markers are visible in the raw bytes even when the unkeyed `0x33` stream
/// reassembly mangles it. `expected_name` adds a slot-specific needle
/// (`"displayName":"<name>"`) that the protobuf preset lists can't fake.
/// Distinguishes "device never sent it" from "host reassembly lost it".
fn slotread_forensics(raw: &[Vec<u8>], expected_name: &str) -> String {
    let (mut n33, mut n34, mut n35) = (0u32, 0u32, 0u32);
    let mut all = Vec::new();
    for r in raw {
        if r.len() < 4 || r[0] != 0 {
            continue;
        }
        match r[1] {
            0x33 => n33 += 1,
            0x34 => n34 += 1,
            0x35 => n35 += 1,
            _ => {}
        }
        let l = r[3] as usize;
        all.extend_from_slice(&r[4..(4 + l).min(r.len())]);
    }
    let hay = String::from_utf8_lossy(&all);
    let count = |n: &str| hay.matches(n).count();
    let name_kv = count(&format!("\"displayName\":\"{expected_name}\""))
        + count(&format!("\"displayName\": \"{expected_name}\""));
    format!(
        "frames 33/34/35={n33}/{n34}/{n35} rawmarkers: displayName(slot)={name_kv} sceneName={} audioGraph={}",
        count("\"sceneName\""),
        count("\"audioGraph\""),
    )
}

/// One result line for a slot-read experiment attempt: reply size + identity
/// check (does the JSON's displayName match the slot's list name — the
/// non-destructive mapping confirmation) + raw-frame forensics.
fn slotread_report(
    tag: &str,
    slot: u32,
    expected_name: &str,
    reply: Option<&[u8]>,
    s: &Session,
) -> String {
    let forensics = slotread_forensics(&s.raw, expected_name);
    match reply {
        Some(b) => {
            let text = String::from_utf8_lossy(b);
            let name_ok = text.contains(&format!("\"displayName\":\"{expected_name}\""))
                || text.contains(&format!("\"displayName\": \"{expected_name}\""));
            format!(
                "  [{tag}] slot {slot} ({expected_name}): REPLY {}B nameMatch={name_ok} sceneNames={} | {forensics}\n",
                b.len(),
                text.matches("\"sceneName\"").count(),
            )
        }
        None => format!(
            "  [{tag}] slot {slot} ({expected_name}): NO REPLY | {forensics} | diag {}\n",
            s.slot_read_diagnostics()
        ),
    }
}

/// Pump until the field-9 reply stops growing (2 stable windows), bounded.
/// A lighter `harvest_slot_read` for the experiment matrix (12×400 ms instead
/// of 20×500 ms — 9 connections back-to-back must not take minutes).
fn slotread_harvest(s: &mut Session) -> Option<Vec<u8>> {
    let mut last = 0usize;
    let mut stable = 0u32;
    for _ in 0..12 {
        if s.pump_more(400).is_err() {
            break;
        }
        let len = s.try_preset_data_json().map(|b| b.len()).unwrap_or(0);
        if len > 0 && len == last {
            stable += 1;
            if stable >= 2 {
                break;
            }
        } else {
            stable = 0;
        }
        last = len;
    }
    s.try_preset_data_json()
}

/// Investigation (`probe --slotread-x [deviceSlot…]`): can the slot-addressed
/// `presetDataRequest` (field 8 → `presetDataChanged` 9) serve a
/// NON-DESTRUCTIVE per-slot scene read — no LoadPreset, the unit's selected
/// preset never changes? The connect-fast benchmark scored the
/// classic in-burst read 0/25 on fw 1.8.45 ("ProductProfile collision"); this
/// matrix separates a device-side drop from a host-side reassembly loss:
///   B          post-handshake read on a warmed dense-heartbeat LIVE session
///   C-early    in-burst, read fired BEFORE the flood requests
///   C-minimal  trimmed burst: connection_request + My Presets + read only
///   A          classic full-burst baseline (the 0/25 configuration)
/// Slots are 1-based DEVICE slots (list index + 1); default = first 3
/// non-empty presets. Sends ZERO LoadPreset.
pub fn probe_slotread_experiments(device_slots: Vec<u32>) -> Result<String, String> {
    use session::SlotReadBurst;

    // ── Exp B: warmed live session (also sources the preset list + slots). ──
    let mut s = Session::connect()?;
    let presets = s.list_my_presets()?;
    let name_of = |dev_slot: u32| {
        presets
            .get((dev_slot - 1) as usize)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "?".to_string())
    };
    let slots: Vec<u32> = if device_slots.is_empty() {
        presets
            .iter()
            .filter(|p| !session::is_empty_slot_name(&p.name))
            .take(3)
            .map(|p| p.slot + 1)
            .collect()
    } else {
        device_slots
    };
    if slots.is_empty() {
        return Err("no non-empty presets to read".to_string());
    }
    let mut out = format!(
        "[slotread-x] device slots {slots:?} — field-8 presetDataRequest, NO LoadPreset\n\
         \n── Exp B: post-handshake on a warmed LIVE session (16×120ms heartbeats) ──\n"
    );
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    for &slot in &slots {
        s.raw.clear();
        s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 400)?;
        let reply = slotread_harvest(&mut s);
        out += &slotread_report("B", slot, &name_of(slot), reply.as_deref(), &s);
        let _ = s.heartbeat();
    }
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── Exps C-early / C-minimal / A-classic: one fresh connection per slot. ──
    // ── Exp D: MULTIPLE field-8 reads on ONE minimal-burst connection — the
    // production-scan shape (one connection for the whole launch scan). The
    // first read rides the burst window; each later read re-tests whether the
    // device keeps answering data requests on the same session.
    out += "\n── Exp D: sequential reads on ONE minimal-burst connection ──\n";
    {
        let first_req = proto::preset_data_request(1, slots[0] as u64, None);
        match Session::connect_slotread(SlotReadBurst::Minimal, &first_req) {
            Ok(mut s) => {
                let reply = slotread_harvest(&mut s);
                out += &slotread_report("D", slots[0], &name_of(slots[0]), reply.as_deref(), &s);
                for &slot in &slots[1..] {
                    s.raw.clear();
                    s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 400)?;
                    let reply = slotread_harvest(&mut s);
                    out += &slotread_report("D", slot, &name_of(slot), reply.as_deref(), &s);
                }
            }
            Err(e) => out += &format!("  [D] connect FAILED: {e}\n"),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── Exp E: re-arm the burst window ON THE SAME CONNECTION by re-sending
    // connection_request (+ My Presets) before each read. If the device treats
    // connection_request as a session reset, a whole-library scan needs only
    // ONE connection — no open/close churn (the congestion gotcha).
    out += "\n── Exp E: connection_request re-arm per read, ONE connection ──\n";
    {
        let t0 = std::time::Instant::now();
        let first_req = proto::preset_data_request(1, slots[0] as u64, None);
        match Session::connect_slotread(SlotReadBurst::Minimal, &first_req) {
            Ok(mut s) => {
                let reply = slotread_harvest(&mut s);
                out += &format!(
                    "  ({:.2}s){}",
                    t0.elapsed().as_secs_f64(),
                    slotread_report("E", slots[0], &name_of(slots[0]), reply.as_deref(), &s)
                );
                for &slot in &slots[1..] {
                    let t = std::time::Instant::now();
                    s.raw.clear();
                    s.send_and_collect(&proto::connection_request(), 100)?;
                    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
                    s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 200)?;
                    let reply = slotread_harvest(&mut s);
                    out += &format!(
                        "  ({:.2}s){}",
                        t.elapsed().as_secs_f64(),
                        slotread_report("E", slot, &name_of(slot), reply.as_deref(), &s)
                    );
                }
            }
            Err(e) => out += &format!("  [E] connect FAILED: {e}\n"),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    for (tag, variant) in [
        ("C-early", SlotReadBurst::Early),
        ("C-minimal", SlotReadBurst::Minimal),
        ("A-classic", SlotReadBurst::Classic),
    ] {
        out += &format!("\n── Exp {tag}: in-burst read, {variant:?} burst ──\n");
        for &slot in &slots {
            let req = proto::preset_data_request(1, slot as u64, None);
            match Session::connect_slotread(variant, &req) {
                Ok(mut s) => {
                    let reply = slotread_harvest(&mut s);
                    out += &slotread_report(tag, slot, &name_of(slot), reply.as_deref(), &s);
                }
                Err(e) => out += &format!("  [{tag}] slot {slot}: connect FAILED: {e}\n"),
            }
            // Seize-recycle settle between fresh connections.
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    }
    Ok(out)
}

/// HW validation for the live-session field-8 read (`probe --slotread-live <slot> [rounds]`).
/// Warms a dense heartbeat → live-controller status, then compares the two shipped
/// reads on that session: `read_slot_preset_json` (sends the `connection_request`
/// re-arm) vs `read_slot_preset_json_live` (skips it — the monitor's path), counting
/// inbound `connectionError` frames + field-9 successes + wall-time per read.
/// HW result: the re-arm path draws 1 `connectionError`/read and runs ~140 ms slower;
/// the live path is 0 errors, both return the same field-9. NON-DESTRUCTIVE — zero
/// LoadPreset, no re-amp.
pub fn probe_slotread_live(device_slot: u32, rounds: u32) -> Result<String, String> {
    // connectionError = ConnectionMessage (TMS field 4) → inner field 3.
    fn is_connection_error(body: &[u8]) -> bool {
        proto::first_bytes(&proto::parse(body), 4)
            .map(|cm| proto::parse(cm).first().map(|(g, _)| *g) == Some(3))
            .unwrap_or(false)
    }
    let run = |label: &str, live: bool| -> Result<String, String> {
        let mut s = Session::connect()?;
        // Warm a Pro-Control-style dense heartbeat → live-controller status.
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        let mut out = format!("── {label} on a warmed dense-heartbeat live session ──\n");
        for r in 0..rounds {
            let t0 = std::time::Instant::now();
            let res = if live {
                s.read_slot_preset_json_live(device_slot)?
            } else {
                s.read_slot_preset_json(device_slot)?
            };
            // connectionError frames that arrived DURING this read (raw cleared on entry).
            let errs = s
                .push_bodies()
                .iter()
                .filter(|b| is_connection_error(b))
                .count();
            out += &format!(
                "  read {r}: {} field-9 ({}B), {errs} connectionError, {:?}\n",
                if res.is_some() { "GOT " } else { "MISS" },
                res.as_ref().map(|b| b.len()).unwrap_or(0),
                t0.elapsed(),
            );
        }
        Ok(out)
    };

    let mut out = format!(
        "[slotread-live] device slot {device_slot}, {rounds} rounds — field-8 on a LIVE session\n\n"
    );
    out += &run(
        "WITH connection_request re-arm (read_slot_preset_json)",
        false,
    )?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    out += &run("LIVE, no re-arm (read_slot_preset_json_live)", true)?;
    Ok(out)
}
