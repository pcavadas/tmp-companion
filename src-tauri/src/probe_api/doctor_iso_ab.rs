//! `probe --doctor-iso-ab` — evidence arm for deleting the Doctor's per-preset
//! ~1.9 s isolation read (`commands::doctor::doctor_force_bypass`'s live field-8
//! read) in favor of `derived_force_bypass`, which computes the SAME force-bypass
//! list OFFLINE from data the frontend already has (the startup backup scan's
//! `FootswitchInfo` + graph). This arm reads the WHOLE library via one device
//! backup (`--device-backup`'s recipe), then for every preset with block-acting
//! footswitches compares — base sound + each footswitch sound — the OFFLINE
//! DERIVED list against the LIVE field-8 list (`doctor_force_bypass`, one read per
//! preset, reused across its base + all its footswitch sounds like `doctor_check`
//! itself does). Both sides are SETS (order differences are not a defect, so both
//! are sorted before comparing). NON-DESTRUCTIVE: the backup + every per-preset
//! read are read-only field-8/field-115 style reads — no LoadPreset, no save.

use crate::doctor;
use crate::footswitch;
use crate::leveller;
use crate::read_slot_preset_parsed;
use crate::session::{ActiveGraph, Session};
use crate::{doctor_force_bypass, read_backup_archive};

/// `ActiveGraph.nodes` (backup-scan `GraphNode`s) → `doctor::DoctorNode`s, via
/// the shared [`doctor::DoctorNode::from_graph_node`] mapper.
fn doctor_nodes_from_graph(graph: &ActiveGraph) -> Vec<doctor::DoctorNode> {
    graph
        .nodes
        .iter()
        .map(doctor::DoctorNode::from_graph_node)
        .collect()
}

/// `DoctorNode`s → a `node_id → saved bypass` map, first-occurrence-wins
/// (mirrors `commands/doctor.rs`'s `saved_bypass_map`) — the shape
/// `footswitch::derived_force_bypass` needs.
fn saved_bypass_map(nodes: &[doctor::DoctorNode]) -> std::collections::HashMap<String, bool> {
    let mut map = std::collections::HashMap::new();
    for n in nodes {
        map.entry(n.node_id.clone()).or_insert(n.bypassed);
    }
    map
}

/// Sorted-set equality compare — order is not semantic for a force-bypass list.
fn sorted(mut v: Vec<(String, String, bool)>) -> Vec<(String, String, bool)> {
    v.sort();
    v
}

pub fn probe_doctor_iso_ab() -> Result<String, String> {
    eprintln!("[probe] doctor-iso-ab: reading the whole library via device backup…");
    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_p| {})?;
    drop(s); // release the seize before the host-side decode + the per-preset reads below
    let result = read_backup_archive(&blob)?;

    let mut sounds = 0usize;
    let mut diffs = 0usize;
    let mut skips = 0usize;
    let mut report = String::new();

    for row in &result.presets {
        if row.footswitches.is_empty() {
            continue;
        }
        let list_index = (row.slot - 1).max(0) as u32;
        let nodes = doctor_nodes_from_graph(&row.graph);
        let saved_bypass = saved_bypass_map(&nodes);

        // One live field-8 read per preset, reused across its base + every
        // footswitch sound below — mirrors `doctor_check`'s `preset_cache`.
        let live_preset = match read_slot_preset_parsed(list_index) {
            Ok((p, _, _)) => p,
            Err(e) => {
                skips += 1;
                report += &format!(
                    "  slot {} ({:?}): SKIP — live preset read failed: {e}\n",
                    row.slot, row.name
                );
                std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
                continue;
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let live_ftsw = live_preset
            .get("ftsw")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // (sound label, footswitch index) — base first, then each block-acting switch.
        let mut cases: Vec<(String, Option<u32>)> = vec![("Base".to_string(), None)];
        for fi in &row.footswitches {
            cases.push((format!("FS{}", fi.switch + 1), Some(fi.switch)));
        }

        for (label, fs) in cases {
            sounds += 1;
            let derived = sorted(footswitch::derived_force_bypass(
                &row.footswitches,
                &saved_bypass,
                fs,
            ));
            let old = sorted(doctor_force_bypass(&live_ftsw, &live_preset, fs));
            if derived == old {
                report += &format!("  slot {} ({:?}) {label}: PASS\n", row.slot, row.name);
            } else {
                diffs += 1;
                report += &format!(
                    "  slot {} ({:?}) {label}: DIFF\n    derived: {derived:?}\n    old:     {old:?}\n",
                    row.slot, row.name
                );
            }
        }
    }

    report += &format!("iso-ab: {sounds} sounds, {diffs} diffs, {skips} skipped\n");
    // A skipped preset carries no evidence — "0 diffs" must never read as a
    // complete pass when part of the library couldn't be compared.
    if skips > 0 {
        report += "iso-ab: INCOMPLETE — some presets could not be read; rerun before trusting equivalence\n";
    }
    Ok(report)
}
