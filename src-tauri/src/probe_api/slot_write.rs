//! Probe entry points: preset write ops (import / clear / map / diag) + block discovery + bulk apply + connect/firmware.

use super::songs::read_song_presets;
use crate::bulk_cmd;
use crate::bulkrun;
use crate::library;
use crate::proto;
use crate::session;
use crate::session::Session;
use crate::PresetEntry;
use crate::{format_dry_run, io_for_path, library_categories, targets_from_library};

/// Headless hardware probe used by the `probe` bin: connect (seizing the
/// device), run the handshake, and return the "My Presets" list. Lets us verify
/// the HID stack against a real TMP without launching the GUI.
pub fn probe_connect_and_list() -> Result<Vec<PresetEntry>, String> {
    let mut s = Session::connect()?;
    s.list_my_presets()
}

/// Headless firmware-version read (`probe --fw`): connect, request the version
/// in-burst (`currentFwRequest`), and return the `currentFwResponse` data.
pub fn probe_firmware_version() -> Result<String, String> {
    let s = Session::connect_with_firmware()?;
    s.firmware_version()
        .ok_or_else(|| "handshake carried no currentFwResponse".to_string())
}

/// Retry active-graph discovery because the TMP's field-3 stream length varies
/// slightly between handshakes. A graph is usable only after its routing
/// template arrives; otherwise a parallel path can be rendered as series.
pub(crate) fn discover_active_graph() -> Result<(session::ActiveGraph, String), String> {
    let mut errors = Vec::new();
    for _ in 0..3 {
        match Session::connect_for_discovery() {
            Ok(mut s) => {
                for _ in 0..4 {
                    let diagnostics = format!(
                        "{}\n{}",
                        s.slot_read_diagnostics(),
                        s.active_graph_diagnostics()
                    );
                    match s.current_audio_graph() {
                        Ok(mut graph) => {
                            if graph.slot.is_none() {
                                graph.slot = s.resolve_unique_my_preset_slot(graph.name.as_deref());
                            }
                            return Ok((graph, diagnostics));
                        }
                        Err(e) => errors.push(format!("{e}\n{diagnostics}")),
                    }
                    s.pump_more(250)?;
                }
            }
            Err(e) => errors.push(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err(errors.join("\n--- retry ---\n"))
}

/// Read-only diagnostic for the active-preset signal chain. Prints the
/// live discovery payload summary and the routing-aware graph sent to React.
pub fn probe_active_graph() -> Result<String, String> {
    let (graph, diagnostics) = discover_active_graph()?;
    let graph = serde_json::to_string_pretty(&graph)
        .map_err(|e| format!("serialize active graph diagnostic: {e}"))?;
    Ok(format!("{diagnostics}\n{graph}\n"))
}

/// Force re-amp mode OFF on a fresh connection — the recovery path for a unit left
/// input-muted ("no sound") by an interrupted leveling run (re-amp routes the input
/// to USB; a dropped fire-and-forget OFF strands it there). `probe --reamp-off`.
pub fn probe_reamp_off() -> Result<(), String> {
    Session::connect()?.set_reamp_mode(false).map(|_| ())
}

/// Discover a preset's level-type block controls (the leveling-knob candidates).
///
/// Primary path is the 1.8.45-SAFE RICH LEAN SESSION (the bench intel-session /
/// `prepass_scene_docs` pattern): heartbeat warmup → `send_and_collect(LoadPreset)`
/// → pump past the 125 hit → read `current_preset_blocks` from the accumulated
/// field-3 push bodies. `connect_for_discovery` (field-78) is effectively DEAD on
/// fw 1.8.45 — it never delivers `currentPresetDataChanged` — so it can no longer be
/// the primary; it stays only as a fallback for older firmware. Without this, FS-scene
/// leveling found zero amp candidates and silently skipped every scene (the device
/// never switched scenes).
pub(crate) fn load_then_discover_blocks(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    match discover_blocks_rich(slot) {
        Ok(blocks) if !blocks.is_empty() => return Ok(blocks),
        Ok(_) => log::warn!("rich block discovery for slot={slot}: loaded but no level blocks"),
        Err(e) => log::warn!("rich block discovery for slot={slot}: {e}"),
    }
    // Fallback for older firmware where the field-78 discovery handshake works.
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    match Session::connect_for_discovery()?.current_preset_blocks() {
        Ok(blocks) => Ok(blocks),
        Err(first_err) => {
            log::warn!("block discovery fallback for slot={slot}: {first_err}");
            let mut s = Session::connect()?;
            let raw = s.capture_full_preset_json(Some(slot), 2000)?;
            let text = String::from_utf8_lossy(&raw);
            let value = session::tolerant_parse_json(&text)
                .ok_or_else(|| format!("{first_err}; fallback field-3 JSON did not parse"))?;
            let blocks = session::extract_level_blocks(&value);
            if blocks.is_empty() {
                Err(format!(
                    "{first_err}; fallback field-3 JSON had no level blocks"
                ))
            } else {
                Ok(blocks)
            }
        }
    }
}

/// 1.8.45-safe block discovery: a single rich lean session loads the preset via
/// `send_and_collect` (NOT `load_preset`, which discards the reports the field-3 push
/// rides on) and reads the level blocks from the accumulated push bodies. Mirrors the
/// bench intel session + `prepass_scene_docs`.
pub(crate) fn discover_blocks_rich(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
    // Keep pumping past the 125 hit — the multi-packet field-3 push (block discovery)
    // needs the extra turns to finish arriving.
    for _ in 0..10 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    s.current_preset_blocks()
}

/// Enumerate a preset's level-type block controls (the leveling-knob candidates).
/// Used by `probe --blocks`.
pub fn probe_list_blocks(slot: u32) -> Result<String, String> {
    let blocks = load_then_discover_blocks(slot)?;
    if blocks.is_empty() {
        return Err(
            "no level-type block controls found (preset JSON may have truncated \
                    before audioGraph completed, or the preset has no level params)"
                .to_string(),
        );
    }
    let mut out = format!(
        "slot {slot}: {} level-type block control(s):\n",
        blocks.len()
    );
    for b in &blocks {
        out += &format!(
            "  {} / {} [{}] / {} = {:.4}\n",
            b.group_id, b.node_id, b.model_id, b.parameter_id, b.value
        );
    }
    Ok(out)
}

/// Probe (AC3): re-import a `.preset` file to the device over USB and
/// report where it landed. Reads the raw file bytes (the OFFLINE codec's XOR'd
/// output), sends the chunked `importPresetRequest`, and re-lists "My Presets" to
/// show the change. The device chooses the slot; this is additive (creates a new
/// user preset) — clear it afterward to restore state.
pub fn probe_import_preset(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    // Baseline list on its own session, then import on a second, then re-list on a
    // third fresh session — the post-import re-list must NOT reuse the importing
    // session's buffer (the chunked send pollutes its accumulated reports).
    let before = Session::connect()?.list_my_presets()?;
    let before_pairs: std::collections::HashSet<(u32, String)> =
        before.iter().map(|p| (p.slot, p.name.clone())).collect();

    let resp = Session::connect()?.import_preset(&bytes)?;

    let after = Session::connect()?.list_my_presets()?;
    let changed: Vec<String> = after
        .iter()
        .filter(|p| !before_pairs.contains(&(p.slot, p.name.clone())))
        .map(|p| format!("slot {} = {:?}", p.slot, p.name))
        .collect();
    let resp_str = match resp {
        Some((le, slot)) => format!("importPresetResponse: listEnum={le} presetSlot={slot}"),
        None => {
            "no importPresetResponse echo (device may not reply — verify via the slot diff)".into()
        }
    };
    Ok(format!(
        "[probe --import] file={path} raw_bytes={}\n{resp_str}\n\
         My Presets: before={} slots, after={} slots\n\
         slots whose name changed after import: {changed:?}\n",
        bytes.len(),
        before.len(),
        after.len(),
    ))
}

/// Probe: clear (delete) a user preset slot — `clearUserPreset` (AC4 setter).
/// Used to undo a `--import` test and to exercise the setter on hardware. Guarded:
/// only clears a slot that currently reads `expect_name` (pass the imported name,
/// e.g. "Guitar") so a mistyped slot can't nuke an unrelated preset.
/// `slot` is the 0-BASED list index — the same space as `PresetEntry.slot` (the
/// guard's read) and `clear_user_preset`'s argument (the mutation), so guard and
/// mutation act in ONE address space (the write-safety lesson). NB: `--export`
/// takes the 1-BASED device slot instead — this probe's list index + 1.
pub fn probe_clear_preset(slot: u32, expect_name: &str) -> Result<String, String> {
    let before = Session::connect()?.list_my_presets()?;
    let cur = before
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    if cur.as_deref() != Some(expect_name) {
        return Ok(format!(
            "[probe --clear] slot {slot} reads {cur:?}, not {expect_name:?} — refused (no change)\n"
        ));
    }
    Session::connect()?.clear_user_preset(slot)?;
    let after = Session::connect()?.list_my_presets()?;
    let now = after
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    Ok(format!(
        "[probe --clear] slot {slot} was {expect_name:?}; cleared → now reads {now:?}\n"
    ))
}

/// Diagnostic (`--diag-frames`): dump the raw inbound frame magic/len sequence for a
/// setlist-list read, so the multi-packet framing (0x33 start / 0x34 cont / 0x35 final)
/// and any interleaved foreign streams are visible, plus whether a strict decode lands.
pub fn probe_diag_frames() -> Result<String, String> {
    let req = proto::setlist_list_request(Some(2));
    let mut s = Session::connect_with_burst_request(&req)?;
    let mut out = String::from("[diag-frames] setlist read, per-pump frame magics:\n");
    for i in 0..6 {
        let strict = s.harvest_setlists_strict().map(|v| v.len());
        out += &format!(
            "  pump {i}: strict={strict:?}\n    frames: {}\n",
            s.raw_frame_summary()
        );
        s.pump_more(300)?;
    }
    Ok(out)
}

/// Diagnostic (`--diag-writes`): compare the device's REPLY to addSong (known-good)
/// vs addSetlist (silently failing) to distinguish an error reply from a silent
/// ignore, and whether setlist context changes the reply. Creates throwaway
/// "Diag*" entries (clean up after).
pub fn probe_diag_writes() -> Result<String, String> {
    let mut out = String::new();
    {
        let mut s = Session::connect()?;
        out += "[diag] addSong \"DiagSong\" (default context) reply:\n";
        out += &s.send_and_dump(&proto::add_song("DiagSong"), 800)?;
    }
    {
        let mut s = Session::connect()?;
        out += "[diag] addSetlist \"DiagBare\" reply:\n";
        out += &s.send_and_dump(&proto::add_setlist("DiagBare"), 800)?;
    }
    Ok(out)
}

/// Probe (AC7 prerequisite): report the data needed to understand the
/// list-index ↔ device-userSlot relationship.
///
/// `list_my_presets` returns 0-based list positions; the `userSlot` setters
/// (`saveCurrentPreset`/`clearUserPreset`/`moveUserPreset`) address a device
/// userSlot. `requestNextEmptyPresetSlot` (81→82) is **dead on 1.7.75**, so we
/// can't enumerate device empties; instead we report the list's `--` placeholders
/// and Song-1's `userPresetSlot`s. Correlating a Song row's `userPresetSlot` with
/// the list index of the preset assigned to it (you supply that preset) pins the
/// numbering: if they match, list index == device userSlot. The shipped leveller
/// already saves via list index, which is consistent with that.
pub fn probe_map_slots() -> Result<String, String> {
    let list = Session::connect()?.list_my_presets()?;
    let empty_list: Vec<u32> = list
        .iter()
        .filter(|p| session::is_empty_slot_name(&p.name))
        .map(|p| p.slot)
        .collect();
    let songs = read_song_presets(1).unwrap_or_default();
    let song_rows: Vec<(u32, u32)> = songs
        .iter()
        .filter(|r| !r.is_empty)
        .map(|r| (r.user_preset_slot, r.preset_scene_slot))
        .collect();

    let mut out = format!(
        "[probe --map-slots]\n\
         My Presets list entries: {}\n\
         empty ('--') list indices (first 12): {:?}\n\
         song-1 (userPresetSlot, presetSceneSlot) rows: {:?}\n",
        list.len(),
        empty_list.iter().take(12).collect::<Vec<_>>(),
        song_rows,
    );
    out += "note: requestNextEmptyPresetSlot (81→82) is dead on 1.7.75 — scratch slots are \
            found by observation (import then re-list), not prediction. To pin the \
            list↔userSlot numbering, assign a known preset to a song and compare its \
            list index with the userPresetSlot reported above.\n";
    Ok(out)
}

/// Build + reconcile a library from `folder` against the live device (Pro Control
/// closed). Shared by the dry-run / apply probes.
fn probe_load_reconciled_library(folder: &str) -> Result<library::Library, String> {
    let (mut records, errors) =
        library::load_library_from_dir(std::path::Path::new(folder), &library_categories())?;
    if !errors.is_empty() {
        eprintln!("[probe] {} file(s) skipped: {errors:?}", errors.len());
    }
    let device_list = Session::connect()?.list_my_presets()?;
    library::reconcile_with_device(&mut records, &device_list);
    Ok(library::Library {
        folder: folder.into(),
        records,
    })
}

/// `probe --bulk-dryrun <folder> <opspec.json>` — preview an op over all matched
/// presets; writes nothing.
pub fn probe_bulk_dryrun(folder: &str, opspec_json: &str) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection: Vec<u32> = lib.records.iter().filter_map(|r| r.list_index).collect();
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(format_dry_run(&bulkrun::dry_run(
        &targets,
        operation.as_ref(),
    )))
}

/// `probe --bulk-apply <folder> <opspec.json> [--slots a,b] [revert]` — apply on the
/// real device (backup-first), optionally reverting immediately for a full round-trip.
pub fn probe_bulk_apply(
    folder: &str,
    opspec_json: &str,
    slots: Option<Vec<u32>>,
    revert: bool,
) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection =
        slots.unwrap_or_else(|| lib.records.iter().filter_map(|r| r.list_index).collect());
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    let mut io = io_for_path(op.io_path());
    let backup_dir = std::env::temp_dir().join(format!(
        "tmp-companion-probe-backups-{}",
        std::process::id()
    ));

    let report = bulkrun::apply(&targets, operation.as_ref(), io.as_mut(), &backup_dir);
    let mut out = format!(
        "[probe bulk apply] {} target(s): {} changed, {} verified, {} error(s)\n",
        report.entries.len(),
        report.changed(),
        report.verified(),
        report.errors(),
    );
    for e in &report.entries {
        out += &format!(
            "  slot {:>3} {:<24} changed={} verified={}{}\n",
            e.list_index,
            e.display_name,
            e.changed,
            e.verified,
            e.error
                .as_ref()
                .map(|x| format!("  ERROR: {x}"))
                .unwrap_or_default(),
        );
    }
    if revert {
        let rev = bulkrun::revert(&report, io.as_mut());
        out += &format!(
            "[probe bulk revert] {} restored\n",
            rev.iter().filter(|r| r.restored).count()
        );
        for r in &rev {
            out += &format!(
                "  slot {:>3} restored={}{}\n",
                r.list_index,
                r.restored,
                r.error
                    .as_ref()
                    .map(|x| format!("  ERROR: {x}"))
                    .unwrap_or_default(),
            );
        }
    }
    Ok(out)
}
