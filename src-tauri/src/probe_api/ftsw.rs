//! Probe entry points: footswitch structure read + validation.

use crate::session::Session;
use crate::audiograph;
use crate::session;

/// The `ftsw` array of the SAVED preset at `device_slot` (1-based), via a field-8 slot
/// read on a fresh quiet session — the reliable post-save source.
pub(crate) fn read_slot_ftsw(device_slot: u32) -> Option<serde_json::Value> {
    let mut v = Session::connect().ok()?;
    v.drain_until_quiet(250, 20).ok()?;
    let raw = v.read_slot_preset_json(device_slot).ok()??;
    session::tolerant_parse_json(&String::from_utf8_lossy(&raw))?
        .get("ftsw")
        .cloned()
}

/// HW validation harness for the footswitch-assignment protocol (`probe --ftsw-validate
/// [switchIndex] [--commit]`). Answers, on the real unit, the open unknowns for the
/// block-acting-footswitch feature: (1) does `setFootswitchAssignment`(54) land on the
/// working copy and what (if anything) confirms it, (2) the `footswitchAddress`↔`ftsw`
/// array-index mapping, (3) the `valueType`/value round-trip, (4) what `swap` does, (5)
/// that `clearFootswitchAssignment`(55) removes it, and — with `--commit` — that
/// `saveCurrentPreset`(14) persists it (then RESTORES the preset to its original ftsw).
/// DRY by default: edits run on the working copy and are discarded by reloading the
/// preset; nothing is saved without `--commit`. Targets the ACTIVE preset.
pub fn probe_ftsw_validate(switch_override: Option<u32>, commit: bool) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --ftsw-validate] {}\n",
        if commit {
            "COMMIT (tests persistence, then restores original ftsw)"
        } else {
            "DRY RUN (working-copy only; reverted, nothing saved)"
        }
    ));

    // Scan an ftsw array for our sentinel (customLabel == "PROBE") → (switch, func index).
    let find_probe = |ftsw: &serde_json::Value| -> Option<(usize, usize)> {
        for (i, sw) in ftsw.as_array()?.iter().enumerate() {
            if let Some(fns) = sw.as_array() {
                for (j, f) in fns.iter().enumerate() {
                    if f.get("customLabel").and_then(|v| v.as_str()) == Some("PROBE") {
                        return Some((i, j));
                    }
                }
            }
        }
        None
    };
    let func_count = |ftsw: &serde_json::Value, i: usize| -> usize {
        ftsw.as_array()
            .and_then(|a| a.get(i))
            .and_then(|s| s.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    };
    let func_at = |ftsw: &serde_json::Value, i: usize, j: usize| -> Option<serde_json::Value> {
        ftsw.as_array()?.get(i)?.as_array()?.get(j).cloned()
    };

    // ── Phase 0: resolve the active preset, read & print its ftsw ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let active_name = s.active_preset_name();
    let loaded = s.loaded_slot();
    let presets = s.list_my_presets().unwrap_or_default();
    let list_index = loaded
        .or_else(|| {
            active_name.as_ref().and_then(|nm| {
                let m: Vec<u32> = presets
                    .iter()
                    .filter(|p| &p.name == nm)
                    .map(|p| p.slot)
                    .collect();
                if m.len() == 1 {
                    Some(m[0])
                } else {
                    None
                }
            })
        })
        .ok_or("could not determine the active preset — load one on the unit first")?;
    let name = active_name
        .clone()
        .or_else(|| {
            presets
                .iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default();
    let graph = s.current_preset_value().ok();
    let ftsw0 = graph
        .as_ref()
        .and_then(|v| v.get("ftsw").cloned())
        .ok_or("no ftsw in the active preset's field-3 push")?;
    report.push_str(&format!(
        "  active {name:?} list_index={list_index} (device slot {})\n  current ftsw ({} switches):\n{}\n",
        list_index + 1,
        ftsw0.as_array().map(|a| a.len()).unwrap_or(0),
        serde_json::to_string_pretty(&ftsw0).unwrap_or_default()
    ));

    // Pick the target switch: explicit, else the first on-off switch.
    let target = switch_override
        .map(|x| x as usize)
        .or_else(|| {
            ftsw0.as_array()?.iter().position(|sw| {
                sw.as_array()
                    .and_then(|a| a.first())
                    .and_then(|f| f.get("func"))
                    .and_then(|v| v.as_str())
                    == Some("on-off")
            })
        })
        .ok_or("no on-off footswitch found — pass an explicit switch index")?;

    // The block that switch toggles, and a REAL param of it (first non-bypass), so the
    // device can't reject the functionJson on an unknown parameterId.
    let f0 = func_at(&ftsw0, target, 0).ok_or("target switch is empty")?;
    let n0 = f0
        .get("nodes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .ok_or("target on-off has no nodes[]")?;
    let grp = n0
        .get("groupId")
        .and_then(|v| v.as_str())
        .unwrap_or("G1")
        .to_string();
    let node = n0
        .get("nodeId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut param = "volume".to_string();
    if let Some(g) = &graph {
        audiograph::for_each_node(g, |obj| {
            if obj.get("nodeId").and_then(|v| v.as_str()) == Some(node.as_str()) {
                if let Some(ps) = obj.get("dspUnitParameters").and_then(|v| v.as_object()) {
                    if let Some(k) = ps
                        .keys()
                        .find(|k| !matches!(k.as_str(), "bypass" | "bypassType"))
                    {
                        param = k.clone();
                    }
                }
            }
        });
    }
    let target_index = func_count(&ftsw0, target) as u32; // next free slot (stack on the on-off)
    report.push_str(&format!(
        "  target switch [{target}] (on-off on {grp}/{node}); adding a 'param' fn on '{param}' at functionIndex {target_index}\n"
    ));
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Sentinel param function (customLabel "PROBE" = find marker; isActive but harmless —
    // it only changes audio when the footswitch is physically engaged, and we never save
    // it except under --commit, which restores).
    let func_json = serde_json::to_string(&serde_json::json!({
        "func": "param", "groupId": grp, "nodeId": node, "parameterId": param,
        "valueA": 0.123, "valueB": 0.456, "valueType": 2, "colorA": 3, "colorB": 0,
        "customLabel": "PROBE", "switchType": 0, "isActive": true
    }))
    .unwrap();

    // ── Phase 1: held session — set / swap / clear on the working copy ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(list_index)?;
    if !s.await_active_preset(&name, 20) {
        return Err(format!(
            "after load, active preset != {name:?} — aborting before any edit (safety)"
        ));
    }

    // UNKNOWN 1 — write + confirm gate. Try setter framing (no batchStatus) first.
    s.set_footswitch_assignment(target as u32, target_index, &func_json, false, None)?;
    report.push_str(&format!(
        "  [set no-batch] PresetMessage fields the device replied with: {:?}\n",
        s.seen_preset_fields()
    ));
    let mut after = s.live_ftsw();
    let mut framing = "no-batch";
    if after.as_ref().and_then(&find_probe).is_none() {
        s.set_footswitch_assignment(target as u32, target_index, &func_json, false, Some(11))?;
        report.push_str(&format!(
            "  [set batch=11] PresetMessage fields the device replied with: {:?}\n",
            s.seen_preset_fields()
        ));
        after = s.live_ftsw();
        framing = "batch=11";
    }
    let landed = after.as_ref().and_then(&find_probe);
    match (landed, &after) {
        (Some((si, fi)), Some(f)) => {
            report.push_str(&format!(
                "  ✓ LANDED via {framing}: PROBE at ftsw[{si}][{fi}] (sent footswitchAddress={target} functionIndex={target_index})\n"
            ));
            if si == target {
                report.push_str("    → footswitchAddress maps 1:1 to the ftsw array index\n");
            } else {
                report.push_str(&format!(
                    "    ⚠ footswitchAddress {target} landed at ftsw index {si} (offset {})\n",
                    si as i64 - target as i64
                ));
            }
            report.push_str(&format!(
                "    stored functionJson (valueType/value round-trip): {}\n",
                func_at(f, si, fi).map(|v| v.to_string()).unwrap_or_default()
            ));
        }
        _ => report
            .push_str("  ✗ did NOT land via either framing — setFootswitchAssignment had no working-copy effect\n"),
    }

    // UNKNOWN 2 — swap semantics (re-send same fn with swap=true, diff the stored object).
    if let Some((si, fi)) = landed {
        let before = func_at(&after.clone().unwrap(), si, fi);
        s.set_footswitch_assignment(target as u32, target_index, &func_json, true, None)?;
        let after_swap = s.live_ftsw();
        let after_fn = after_swap
            .as_ref()
            .and_then(&find_probe)
            .and_then(|(a, b)| func_at(after_swap.as_ref().unwrap(), a, b));
        report.push_str(&format!(
            "  [swap=true] before: {}\n              after:  {}\n",
            before.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
            after_fn
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into())
        ));
    }

    // UNKNOWN 3 — clear.
    s.clear_footswitch_assignment(target as u32, target_index)?;
    let after_clear = s.live_ftsw();
    let gone = after_clear.as_ref().and_then(&find_probe).is_none();
    report.push_str(&format!(
        "  [clear] {} — switch [{target}] func count: was {} now {}\n",
        if gone {
            "✓ PROBE removed"
        } else {
            "✗ PROBE still present"
        },
        func_count(&ftsw0, target),
        after_clear
            .as_ref()
            .map(|f| func_count(f, target))
            .unwrap_or(0)
    ));

    // ── Phase 2: persistence (commit only), then RESTORE ──
    if commit && landed.is_some() {
        s.set_footswitch_assignment(target as u32, target_index, &func_json, false, None)?;
        s.save_current_preset(list_index)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let persisted = read_slot_ftsw(list_index + 1);
        report.push_str(&format!(
            "  [commit] field-8 readback slot {:03}: PROBE persisted = {}\n",
            list_index + 1,
            persisted.as_ref().and_then(&find_probe).is_some()
        ));
        // RESTORE: clear the probe fn and re-save.
        let mut r = Session::connect()?;
        r.begin_live_edit()?;
        r.load_preset(list_index)?;
        if r.await_active_preset(&name, 20) {
            r.clear_footswitch_assignment(target as u32, target_index)?;
            r.save_current_preset(list_index)?;
        }
        drop(r);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let restored = read_slot_ftsw(list_index + 1);
        report.push_str(&format!(
            "  [restore] field-8 readback: PROBE gone = {}, switch [{target}] func count = {} (original {})\n",
            restored.as_ref().and_then(&find_probe).is_none(),
            restored.as_ref().map(|f| func_count(f, target)).unwrap_or(99),
            func_count(&ftsw0, target)
        ));
    } else {
        // DRY: discard the working copy by reloading the preset.
        let _ = s.load_preset(list_index);
        drop(s);
    }

    Ok(report)
}
