//! Active-graph and level-block discovery: the retrying reads the preset and leveling
//! commands and the probe use to learn what the loaded preset holds.

use crate::session;
use crate::session::Session;
use crate::slot::ListIndex;

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
                    s.pump_collect_alive(250)?;
                }
            }
            Err(e) => errors.push(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err(errors.join("\n--- retry ---\n"))
}

/// Discover a preset's level-type block controls (the leveling-knob candidates).
///
/// Primary path is the RICH LEAN SESSION (the bench intel-session / `prepass_scene_docs`
/// pattern): heartbeat warmup → `send_and_collect(LoadPreset)` → pump past the 125 hit →
/// read `current_preset_blocks` from the accumulated field-3 push bodies. Field-78
/// (`connect_for_discovery`) is the fallback: it failed on 1.8.45 while its window was
/// silent, and delivers kept-alive on 1.8.58. Without this, FS-scene leveling found zero
/// amp candidates and silently skipped every scene (the device never switched scenes).
/// `slot` is the 0-based list index; every path opens its own session.
pub(crate) fn load_then_discover_blocks(
    slot: ListIndex,
) -> Result<Vec<session::LevelBlock>, String> {
    match discover_blocks_rich(slot) {
        Ok(blocks) if !blocks.is_empty() => return Ok(blocks),
        Ok(_) => log::info!("rich block discovery for slot={slot}: loaded but no level blocks"),
        Err(e) => log::warn!("rich block discovery for slot={slot}: {e}"),
    }
    // Fallback: the discovery handshake's own field-3 push.
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
            // Same picker gate as `current_preset_blocks` above — this is the last fallback
            // of the SAME enumeration, so it must not offer a different control set.
            let blocks = session::extract_level_candidates(&value);
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
/// bench intel session + `prepass_scene_docs`. Private: the primary path of
/// `load_then_discover_blocks`, which owns the fallbacks.
fn discover_blocks_rich(slot: ListIndex) -> Result<Vec<session::LevelBlock>, String> {
    crate::leveller::make_current(slot)?;
    let mut s = Session::connect()?;
    s.rich_warmup()?;
    s.rich_load_collect(slot)?;
    s.current_preset_blocks()
}
