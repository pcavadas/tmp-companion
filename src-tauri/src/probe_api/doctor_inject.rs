//! `probe --doctor-inject` — DEFECT-INJECTION validation arm (R5).
//!
//! Injects a KNOWN spectral defect into a clean preset's LIVE edit buffer (an
//! `ACD_TenBandEQStereo` insert with explicit band gains — the same op vehicle
//! the Doctor's own prescriptions use) and diagnoses the sound before/after:
//! a band-rule threshold earns its value only if the rule FIRES on its injected
//! defect and stays SILENT on the un-defected control. Never saves — the edit
//! buffer is discarded by a stored-preset reload at the end (and on any error).
//!
//! Usage:
//! ```text
//! probe --doctor-inject <slot> <gains_csv|none>
//! # e.g. probe --doctor-inject 16 gain250hz=8            (muddy positive)
//! #      probe --doctor-inject 16 none                   (clean insert control)
//! ```
//! `gains_csv` = comma-separated `controlId=dB` EQ-10 band gains; `none` inserts
//! the EQ with all bands at 0 (proves the insert itself doesn't shift verdicts).
//! An UNVERIFIED band controlId silently no-ops on the device — visible here as
//! the defect not appearing in the after-capture (this arm doubles as the
//! band-id verification). Attended; loads the probed slot; ends re-amp OFF.

use crate::doctor;
use crate::leveller;

use super::analyze::{analyze_capture, DoctorRead};
use super::stimulus::{probe_stimulus_path, read_stimulus_48k};

/// One capture's Doctor read (verdicts + the metric internals per band) AND
/// its standard debug line — `pub(crate)` so `--doctor-defects`
/// (`doctor_defects.rs`) reuses the exact same capture→line pipeline instead
/// of forking it; a metric-pipeline change can't drift the two arms' report
/// format apart.
pub(crate) fn measure(
    stim: &[f32],
    label: &str,
    capture: Result<(Vec<f32>, u32), String>,
) -> Result<(DoctorRead, String), String> {
    let (samples, rate) = capture?;
    // Padded production stim (`leveller::doctor_stim_slice`) → the body PSD's
    // onset is pad-adjusted via `doctor_signal_start` (`pad_aware: true`) —
    // see `analyze_capture`'s doc for why this differs from
    // `doctor_window_ab`'s raw-stim variant.
    let read = analyze_capture(stim, &samples, rate, doctor::Family::Guitar, true)?;
    let mut line = format!(
        "  {label:<7} tilt={} dev={} locals={} tail={:.1} verdicts={:?}\n",
        read.tilt_slope.map_or("n/a".into(), |s| format!("{s:+.2}")),
        read.deviations
            .iter()
            .map(|v| format!("{v:+.1}"))
            .collect::<Vec<_>>()
            .join(","),
        read.locals
            .iter()
            .map(|v| format!("{v:+.1}"))
            .collect::<Vec<_>>()
            .join(","),
        read.tail_ratio_db,
        read.verdicts
    );
    if !read.peaks.is_empty() {
        let tops: Vec<String> = read
            .peaks
            .iter()
            .take(10)
            .map(|p| format!("{:.0}Hz h={:.1}dB q={:.1}", p.freq_hz, p.height_db, p.q))
            .collect();
        line += &format!("          peaks: {}\n", tops.join(" · "));
    }
    Ok((read, line))
}

/// The `dspUnitParameters` of the first node whose `FenderId` matches, from a live
/// field-3 preset doc (searches both node groups).
fn node_params<'a>(doc: &'a serde_json::Value, fender_id: &str) -> Option<&'a serde_json::Value> {
    // guitarNodes/micNodes are OBJECTS keyed by group id (G1..G7 / M1..M4),
    // each holding the node array.
    ["guitarNodes", "micNodes"].iter().find_map(|grp| {
        doc.pointer(&format!("/audioGraph/{grp}"))?
            .as_object()?
            .values()
            .filter_map(|nodes| nodes.as_array())
            .flatten()
            .find(|n| n.pointer("/FenderId").and_then(|v| v.as_str()) == Some(fender_id))?
            .pointer("/dspUnitParameters")
    })
}

/// `doctor::doctor_tail_ms` off a live/parsed preset doc's active graph — the
/// SAME production tail policy `commands/doctor.rs` uses (300 ms known-dry /
/// 1500 ms wet-or-unknown), so a probe capture is taken in the same capture
/// space production ships, not a pinned literal from a different one. An
/// unparseable/empty graph conservatively falls through `doctor_tail_ms`'s own
/// `nodes.is_empty()` guard to the wet tail.
pub(crate) fn tail_ms_for_doc(doc: &serde_json::Value) -> u32 {
    let nodes: Vec<doctor::DoctorNode> = crate::session::extract_active_graph(doc, None)
        .nodes
        .iter()
        .map(doctor::DoctorNode::from_graph_node)
        .collect();
    doctor::doctor_tail_ms(&nodes)
}

/// The insert anchor for both defect-injection probe arms: the LAST guitar
/// node's group (post-amp/cab, never pre-drive, never a mic group) — the
/// same anchor the Doctor's own Rx inserts use — plus the preset's display
/// name (needed by `ops_session`'s `confirm_active`) and the STORED preset's
/// own production tail ([`tail_ms_for_doc`], for the BEFORE capture — the
/// AFTER capture must re-resolve its own tail since an inserted block can
/// change the wet/dry classification). `pub(crate)` so `--doctor-defects`
/// (`doctor_defects.rs`) shares this exact resolution instead of forking it —
/// a graph shaped `[...guitar, ...mic]` (any gtrMicMix/Parallel/Series
/// template) would otherwise let a plain `.nodes.last()` pick a mic node.
pub(crate) fn last_guitar_group_anchor(slot: u32) -> Result<(String, String, u32), String> {
    let (preset, _, _) = crate::read_slot_preset_parsed(slot)?;
    let graph = crate::session::extract_active_graph(&preset, None);
    let doctor_nodes: Vec<doctor::DoctorNode> = graph
        .nodes
        .iter()
        .map(doctor::DoctorNode::from_graph_node)
        .collect();
    let tail_ms = doctor::doctor_tail_ms(&doctor_nodes);
    let group = graph
        .nodes
        .into_iter()
        .rev()
        .find(|n| n.group_id.starts_with('G'))
        .map(|n| n.group_id)
        .ok_or("empty graph — nowhere to anchor the insert")?;
    let name = preset
        .pointer("/info/displayName")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((group, name, tail_ms))
}

/// See the module doc. `gains` empty = the clean-insert control. `block`
/// overrides the EQ-10 insert vehicle (default) — e.g. `ACD_CryBabyGCB95`
/// with no gains injects a wah at its DEFAULT (cocked) position, the classic
/// high-Q resonance the `resonant` rule exists for, without needing any
/// documented controlIds.
pub fn probe_doctor_inject(
    slot: u32,
    gains: &[(String, f64)],
    block: Option<&str>,
) -> Result<String, String> {
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(&probe_stimulus_path(
        "guitar-humbucker",
    )?)?);

    let fender_id = block.unwrap_or("ACD_TenBandEQStereo");
    // Fresh-connection re-amp OFF on EVERY exit path from here down (early `?`,
    // normal return, even unwind) — the drop-guard form of the sweep-tail call.
    let _reamp_off = super::ReampOffGuard;
    let mut out = format!(
        "doctor-inject slot {slot} block {fender_id} gains {:?}\n",
        gains
            .iter()
            .map(|(p, v)| format!("{p}={v}"))
            .collect::<Vec<_>>()
    );
    // Resolve the insert anchor + the STORED preset's own production tail
    // (`doctor::doctor_tail_ms`, same policy `commands/doctor.rs` uses) before
    // touching the device — the before-capture must be taken in production's
    // capture space, not a pinned literal.
    let (last_group, name, before_tail) = last_guitar_group_anchor(slot)?;
    let before_tail = u64::from(before_tail);

    // BEFORE: the stored preset as-is (also loads it, so the live edit below
    // confirms the already-current preset — the doctor_apply shape).
    let (_, before_line) = measure(
        &stim,
        "before",
        leveller::doctor_capture(slot, None, &[], &stim, Some(0.5), before_tail, false),
    )?;
    out += &before_line;

    // Insert the EQ at the END of the chain (the LAST guitar node's group,
    // appended) — the same anchor the Doctor's own Rx inserts use (post-amp/
    // cab, never pre-drive).
    let ops = vec![doctor::DoctorOp::InsertNode {
        group_id: last_group,
        before_fender_id: None,
        fender_id: fender_id.to_string(),
        params: gains.to_vec(),
    }];
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let mut ops_s = crate::commands::doctor::ops_session(slot, &name, &ops, "inject")?;
    // Schema/write-verification readout: the edit provokes a live field-3 push whose
    // graph carries the freshly-inserted vehicle WITH its parameter values — print
    // them, so an unmapped controlId shows up as an unchanged default instead of a
    // silent no-op one has to infer from an unmoved after-capture.
    let _ = ops_s.pump_collect(700);
    // Re-resolve the tail off the EDITED graph, not the before-capture's: an
    // inserted time-effect (e.g. `--doctor-defects`'s `washed` recipe, a
    // reverb) can turn a known-dry chain wet, and reusing the before tail
    // would under-capture the new wash tail. Falls back to the before tail
    // (already conservative-safe) if the live doc didn't come back.
    let after_tail = match ops_s.current_preset_value() {
        Ok(doc) => {
            out += &format!(
                "  vehicle params: {}\n",
                node_params(&doc, fender_id)
                    .map_or("node not found in live field-3 doc".into(), |p| p
                        .to_string())
            );
            u64::from(tail_ms_for_doc(&doc))
        }
        Err(e) => {
            out += &format!("  vehicle params: (no live field-3 doc: {e})\n");
            before_tail
        }
    };
    drop(ops_s);
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    // AFTER: the edit buffer, no reload (a load would discard the insert).
    let after_res = measure(
        &stim,
        "after",
        leveller::doctor_capture_current(&stim, None, &[], Some(0.5), after_tail),
    );

    // Discard the injected defect ALSO when the after-capture failed (the module
    // doc's "and on any error" promise): the injected edit must never outlive the
    // command — and a restore failure is REPORTED, never dropped behind the
    // capture error.
    let restore_res = leveller::restore_saved_preset(slot);
    match (after_res, restore_res) {
        (Ok((_, after_line)), Ok(())) => {
            out += &after_line;
            out += "  (edit buffer discarded — stored preset reloaded)\n";
            Ok(out)
        }
        (Ok(_), Err(r)) => Err(format!("edit-buffer restore failed: {r}")),
        (Err(e), restore) => Err(leveller::append_restore_err(e, restore)),
    }
}
