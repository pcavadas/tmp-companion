//! Probe entry points: read-only device inspection (roster, slot JSON, discover, saved blocks, backup).

use crate::audiograph;
use crate::proto;
use crate::saved_blocks::{find_block_presets_blob, parse_block_presets_map, SavedBlock};
use crate::session;
use crate::session::Session;
use crate::{discover_replace_plans, read_backup_archive};

/// READ-ONLY RE spike (`probe --re-blocks`): fire `RequestAllBlockPresets` (135) +
/// the user-IR list request inside the handshake burst and dump every saved-block /
/// IR response the device streams back, so the opaque `blockPresetsMap` blob and the
/// IR list shape can be decoded. No device writes; nothing persists (optional raw
/// dumps only when `TMP_RE_OUT=<dir>` is set). Used once to derive the wire schema;
/// the production readers (`list_saved_blocks` / `list_user_irs`) build on what it finds.
pub fn probe_re_blocks() -> Result<String, String> {
    let hexn = |b: &[u8], n: usize| -> String {
        b.iter()
            .take(n)
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Attempt 1: ride the request inside the handshake burst (batch-2 group).
    let mut s = Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
    for _ in 0..4 {
        s.pump_collect(250)?;
    }
    // Attempt 2: send it post-handshake as a request/response message (no batchStatus),
    // keeping the session alive with heartbeats — the framing the ReplaceNode family uses.
    s.heartbeat()?;
    s.pump_collect(80)?;
    s.send_and_collect(&proto::request_all_block_presets(None), 600)?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(250)?;
    }
    let bodies = s.push_bodies();
    drop(s); // release the HID seize before host-side work

    let out_dir = std::env::var("TMP_RE_OUT").ok();
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --re-blocks] scanned {} reassembled bodies\n",
        bodies.len()
    ));

    // Top-level TMS field histogram, so we can see what arrived.
    let mut top_hist: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for b in &bodies {
        for (f, _) in proto::parse(b) {
            *top_hist.entry(f).or_default() += 1;
        }
    }
    report.push_str(&format!("top TMS fields seen: {top_hist:?}\n"));

    // Per-body inner-field map (so we can see what the device actually streamed and
    // whether 135/136 ever appears under PresetMessage).
    for (bi, b) in bodies.iter().enumerate() {
        let top = proto::parse(b);
        let tf: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
        let mut inner_desc = String::new();
        for (f, _) in &top {
            if let Some(inner_bytes) = proto::first_bytes(&top, *f) {
                let inner = proto::parse(inner_bytes);
                let ifields: Vec<u32> = inner.iter().map(|(x, _)| *x).collect();
                inner_desc.push_str(&format!(" top{f}{ifields:?}"));
            }
        }
        report.push_str(&format!("  body {bi}: top {tf:?} ·{inner_desc}\n"));
    }

    let mut found_blocks = 0usize;
    let mut found_updated = 0usize;
    let mut found_ir = 0usize;
    for (bi, b) in bodies.iter().enumerate() {
        let top = proto::parse(b);
        // PresetMessage (top field 2): inner 136 = AllBlockPresetsResponse, 96 = BlockPresetUpdated.
        if let Some(pm) = proto::first_bytes(&top, 2) {
            let inner = proto::parse(pm);
            let inner_fields: Vec<u32> = inner.iter().map(|(f, _)| *f).collect();
            if let Some(resp) = proto::first_bytes(&inner, 136) {
                // AllBlockPresetsResponse { bytes blockPresetsMap = 1 }.
                let map_bytes = proto::parse(resp);
                let blob = proto::first_bytes(&map_bytes, 1).unwrap_or(resp);
                found_blocks += 1;
                report.push_str(&format!(
                    "\n=== AllBlockPresetsResponse (body {bi}) ===\n  inner fields {inner_fields:?}\n  blockPresetsMap: {} bytes\n  head: {}\n",
                    blob.len(),
                    hexn(blob, 64)
                ));
                if let Some(dir) = &out_dir {
                    let p = format!("{dir}/blockPresetsMap.bin");
                    std::fs::write(&p, blob).map_err(|e| format!("write {p}: {e}"))?;
                    report.push_str(&format!("  wrote {p}\n"));
                }
            }
            if let Some(upd) = proto::first_bytes(&inner, 96) {
                // BlockPresetUpdated { fenderId=2, blockNames=3, dualCabsEnabled=4, cab1Names=5, cab2Names=6 }.
                let u = proto::parse(upd);
                let fid = proto::first_bytes(&u, 2)
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .unwrap_or_default();
                let names: Vec<String> = proto::all_bytes(&u, 3)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                let cab1: Vec<String> = proto::all_bytes(&u, 5)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                let cab2: Vec<String> = proto::all_bytes(&u, 6)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                found_updated += 1;
                report.push_str(&format!(
                    "\n=== BlockPresetUpdated (body {bi}) ===\n  fenderId={fid:?} names={names:?} cab1={cab1:?} cab2={cab2:?}\n"
                ));
            }
        }
        // UserMessage (top field 13): the UserIRListResponse.
        if let Some(um) = proto::first_bytes(&top, 13) {
            let inner = proto::parse(um);
            let inner_fields: Vec<u32> = inner.iter().map(|(f, _)| *f).collect();
            found_ir += 1;
            report.push_str(&format!(
                "\n=== UserMessage (body {bi}) ===\n  inner fields {inner_fields:?}\n"
            ));
            // Dump each inner length-delimited field as candidate records.
            for (f, v) in &inner {
                if let proto::Val::Bytes(bytes) = v {
                    let rec = proto::parse(bytes);
                    let rfields: Vec<u32> = rec.iter().map(|(rf, _)| *rf).collect();
                    let strs: Vec<String> = rec
                        .iter()
                        .filter_map(|(_, rv)| {
                            if let proto::Val::Bytes(s) = rv {
                                Some(String::from_utf8_lossy(s).into_owned())
                            } else {
                                None
                            }
                        })
                        .collect();
                    report.push_str(&format!(
                        "  field {f}: subfields {rfields:?} strings {strs:?} head {}\n",
                        hexn(bytes, 32)
                    ));
                }
            }
            if let Some(dir) = &out_dir {
                let p = format!("{dir}/userMessage_body{bi}.bin");
                std::fs::write(&p, um).map_err(|e| format!("write {p}: {e}"))?;
            }
        }
    }
    report.push_str(&format!(
        "\nsummary: AllBlockPresetsResponse×{found_blocks}, BlockPresetUpdated×{found_updated}, UserMessage×{found_ir}\n\
         NOTE: read-only; no device writes; raw dumps only when TMP_RE_OUT is set.\n"
    ));
    Ok(report)
}

/// Read-only full-roster dump for the perf experiments (`probe --roster <slots_csv>`):
/// every `(group, node_id, fenderId)` per slot via the proven field-8 read, so an
/// experiment can pick a FROM/TO that actually exists in the target presets.
pub fn probe_roster(device_slots: &[u32]) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!("[probe --roster] slots {device_slots:?}\n"));
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    for &dev_slot in device_slots {
        let Some(raw) = s.read_slot_preset_json(dev_slot)? else {
            report.push_str(&format!("  slot {dev_slot:03}: (no JSON)\n"));
            continue;
        };
        let Some(value) = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)) else {
            report.push_str(&format!("  slot {dev_slot:03}: (parse failed)\n"));
            continue;
        };
        let name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        report.push_str(&format!("  slot {dev_slot:03} {name:?}:\n"));
        for (group, node_id, fid) in audiograph::roster(&value) {
            report.push_str(&format!("    {group:<10} {node_id:<30} {fid}\n"));
        }
    }
    Ok(report)
}

/// Read-only raw field-8 preset-JSON dump for a single slot (`probe --slot-json <slot>`),
/// pretty-printed. Used to inspect node params the roster summary drops (e.g. a saved
/// dual-cab's `dualCabsEnabled`/`cab1`/`cab2` after a `replaceNodeWithBlock`).
pub fn probe_slot_json(device_slot: u32) -> Result<String, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let raw = s
        .read_slot_preset_json(device_slot)?
        .ok_or_else(|| format!("slot {device_slot}: field-8 read returned no JSON"))?;
    let text = String::from_utf8_lossy(&raw);
    match session::tolerant_parse_json(&text) {
        Some(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())),
        None => Ok(text.to_string()),
    }
}

/// E5 — discovery-source diff (`probe --discover-diff <FROM> <slots_csv>`). Builds the
/// replace-target set for each slot from BOTH the per-slot field-8 read
/// (`discover_replace_plans`) and the whole-library device backup (complete presetJson
/// roster), then diffs the `(group, node_id)` sets. For large N the backup (~22 s flat,
/// COMPLETE JSON) beats per-slot field-8 (~0.9 s × N) AND can't miss a block past the
/// field-8 truncation point — this proves the two sources agree (or surfaces a miss).
/// Read-only.
pub fn probe_discover_diff(from_id: &str, device_slots: &[u32]) -> Result<String, String> {
    use std::collections::BTreeSet;
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --discover-diff] {from_id} on slots {device_slots:?}\n"
    ));

    // Source A — per-slot field-8 reads (the current production discovery).
    let t_f8 = std::time::Instant::now();
    let plans = discover_replace_plans(device_slots, from_id)?;
    let f8_secs = t_f8.elapsed().as_secs_f64();
    let f8: std::collections::HashMap<u32, BTreeSet<(String, String)>> = plans
        .iter()
        .map(|p| ((p.list_index + 1), p.targets.iter().cloned().collect()))
        .collect();

    std::thread::sleep(std::time::Duration::from_millis(600));

    // Source B — whole-library device backup (one stream, complete JSON).
    let t_bk = std::time::Instant::now();
    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_p| {})?;
    drop(s);
    let result = read_backup_archive(&blob)?;
    let bk_secs = t_bk.elapsed().as_secs_f64();
    let want: BTreeSet<u32> = device_slots.iter().copied().collect();
    let bk: std::collections::HashMap<u32, BTreeSet<(String, String)>> = result
        .presets
        .iter()
        .filter(|p| p.slot > 0 && want.contains(&(p.slot as u32)))
        .map(|p| {
            let set: BTreeSet<(String, String)> = p
                .blocks
                .iter()
                .filter(|b| b.fender_id == from_id)
                .map(|b| (b.group_id.clone(), b.node_id.clone()))
                .collect();
            (p.slot as u32, set)
        })
        .collect();

    report.push_str(&format!(
        "  field-8 discovery: {f8_secs:.2}s for {} slots\n  backup discovery:  {bk_secs:.2}s (whole library, {} presets)\n\n",
        device_slots.len(),
        result.presets.len()
    ));
    let mut agree = 0usize;
    let mut disagree = 0usize;
    for &slot in device_slots {
        let a = f8.get(&slot).cloned().unwrap_or_default();
        let b = bk.get(&slot).cloned().unwrap_or_default();
        if a == b {
            agree += 1;
            report.push_str(&format!(
                "    slot {slot:03}: AGREE ({} target(s))\n",
                a.len()
            ));
        } else {
            disagree += 1;
            let only_f8: Vec<_> = a.difference(&b).collect();
            let only_bk: Vec<_> = b.difference(&a).collect();
            report.push_str(&format!(
                "    slot {slot:03}: DIFFER  field-8-only={only_f8:?}  backup-only={only_bk:?}\n"
            ));
        }
    }
    report.push_str(&format!("\n  {agree} agree, {disagree} differ.\n"));
    Ok(report)
}

/// Validate the PRODUCTION saved-block decode path live (`probe --saved-blocks`):
/// the exact `list_saved_blocks` flow (RequestAllBlockPresets → decode →
/// `parse_block_presets_map`), printed as a summary. Read-only.
pub fn probe_saved_blocks() -> Result<String, String> {
    let mut s = Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
    for _ in 0..4 {
        s.pump_collect(250)?;
    }
    let bodies = s.push_bodies();
    drop(s);
    let blob = find_block_presets_blob(&bodies)
        .ok_or_else(|| "device sent no allBlockPresetsResponse".to_string())?;
    let blocks = parse_block_presets_map(&blob)?;
    let named: Vec<&SavedBlock> = blocks
        .iter()
        .filter(|b| !b.name.is_empty() && !b.name.to_lowercase().contains("autogen default"))
        .collect();
    let dual: Vec<&SavedBlock> = blocks.iter().filter(|b| b.dual_cabs_enabled).collect();
    let sample: String = named
        .iter()
        .take(12)
        .map(|b| {
            format!(
                "    {} [{}] {:?}{}",
                b.fender_id,
                b.index,
                b.name,
                if b.dual_cabs_enabled {
                    " (dual-cab)"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "[probe --saved-blocks] production decode of allBlockPresetsResponse.blockPresetsMap\n\
         total entries:   {}\n\
         user-named:      {}\n\
         dual-cabs:       {}\n\
         sample (named):\n{sample}\n\
         NOTE: read-only; this is the exact path list_saved_blocks serves the UI.\n",
        blocks.len(),
        named.len(),
        dual.len(),
    ))
}

/// Fast full-library read via the device backup (`probe --device-backup`): one
/// `BackupRequest` streams a GNU-tar + LZ4-frame archive of `/data`, decoded in
/// memory (see [`read_backup_archive`]). Replaces ~500 per-preset round-trips with
/// one stream + one local SQLite read, and yields COMPLETE presets (not the
/// USB-partial slot-read). Prints live transfer progress (the same `BackupProgress`
/// that drives the UI's `tmp://backup-progress` bar).
pub fn probe_device_backup() -> Result<String, String> {
    let mut s = Session::connect()?;
    let mut last_bucket = -1i32;
    let (blob, stats) = s.device_backup(60, |p| {
        if p.phase == "building" {
            eprintln!(
                "[probe]   building… (build_size={}, build_ticks={})",
                p.build_size, p.build_ticks
            );
        } else {
            let bucket = (p.percent as i32 / 10) * 10;
            if bucket > last_bucket {
                last_bucket = bucket;
                eprintln!(
                    "[probe]   streaming {:>3.0}%  {}/{} chunks  {} KiB",
                    p.percent,
                    p.received,
                    p.total,
                    p.bytes / 1024
                );
            }
        }
    })?;
    drop(s); // release the HID seize before host-side work

    // Diagnostic escape hatch (OFF by default → nothing persists): dump the raw
    // streamed archive when TMP_BACKUP_RAW=<path> is set.
    if let Ok(path) = std::env::var("TMP_BACKUP_RAW") {
        std::fs::write(&path, &blob).map_err(|e| format!("dump raw: {e}"))?;
        eprintln!("[probe] wrote raw archive ({} B) to {path}", blob.len());
    }

    let crc_ok = stats.bytes_assembled == stats.num_bytes as usize;
    let magic: Vec<String> = blob.iter().take(16).map(|b| format!("{b:02x}")).collect();
    let result = read_backup_archive(&blob)?;

    let throughput = if stats.elapsed_secs > 0.0 {
        stats.bytes_assembled as f64 / 1024.0 / stats.elapsed_secs
    } else {
        0.0
    };
    let members: String = result
        .members
        .iter()
        .map(|(p, sz)| format!("    {p} ({sz} B)"))
        .collect::<Vec<_>>()
        .join("\n");
    let sample: String = result
        .presets
        .iter()
        .take(5)
        .map(|p| {
            let names: Vec<&str> = p.scenes.iter().map(|s| s.name.as_str()).collect();
            let count = if p.scene_count < 0 {
                "?".to_string()
            } else {
                p.scene_count.to_string()
            };
            format!(
                "    slot {}: {:?} ({count} scene(s): {})",
                p.slot,
                p.name,
                names.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let scenes_known = result.presets.iter().filter(|p| p.scene_count >= 0).count();

    Ok(format!(
        "[probe --device-backup] full library via BackupRequest (one stream, no per-preset round-trips)\n\
         transfer:\n\
         \telapsed        {:.2} s  (first chunk at {:.2} s = handshake + device build)\n\
         \tchunks         {}/{} received\n\
         \tarchive bytes  {} (declared numBytes={}, crc=0x{:08x}, integrity {})\n\
         \tthroughput     {throughput:.0} KiB/s   archive magic [{}]\n\
         \tbuild progress device-reported: size={} ticks={} ({})\n\
         \tstate log      {:?}\n\
         archive members ({}):\n{members}\n\
         normalDb.db3: {} bytes decompressed\n\
         UserPresets rows: {} total ({} non-empty named)\n\
         total scenes: {} across {scenes_known} presets (scene count via {})\n\
         sample:\n{sample}\n\
         NOTE: archive held in RAM only; temp DB deleted on exit (no backup persisted).\n",
        stats.elapsed_secs,
        stats.first_chunk_secs,
        stats.chunks_received,
        stats.num_chunks,
        stats.bytes_assembled,
        stats.num_bytes,
        stats.crc,
        if crc_ok { "ok" } else { "SIZE MISMATCH" },
        magic.join(" "),
        stats.build_size,
        stats.build_ticks,
        if stats.build_size > 0 { "determinate" } else { "not reported → use indeterminate spinner for build phase" },
        stats.state_log,
        result.members.len(),
        result.db_bytes,
        result.total_rows,
        result.presets.len(),
        result.total_scenes(),
        result.scene_mode,
    ))
}
