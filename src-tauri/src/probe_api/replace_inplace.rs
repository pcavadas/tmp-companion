//! Probe entry points: in-place re-import (`--replace-inplace`) and snapshot restore
//! (`--restore`), both reporting on [`crate::replace_inplace::replace_inplace_core`].

use crate::backup;
use crate::replace_inplace::replace_inplace_core;
use crate::slot::ListIndex;

/// Probe (AC7 positive case): edit a preset IN PLACE on its original slot and
/// report whether the slot, its Song assignment, and scene binding survive.
/// Compare against `--import` (bare append) as the negative control — that one
/// lands the edit at a new slot and the Song row then points at the stale copy.
pub fn probe_replace_inplace(orig_list_index: ListIndex, path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    run_replace_inplace(orig_list_index, &bytes, &format!("file={path}"))
}

/// String-reporting wrapper over [`replace_inplace_core`] for the probe subcommands.
fn run_replace_inplace(
    orig_list_index: ListIndex,
    bytes: &[u8],
    src: &str,
) -> Result<String, String> {
    let o = replace_inplace_core(orig_list_index, bytes)?;
    let ac7 = if o.had_binding {
        format!(
            "AC7 PASS = edit_landed && binding_preserved = {}",
            o.edit_landed && o.binding_preserved
        )
    } else {
        format!(
            "AC7 = edit_landed={}; binding NOT CHECKED (song 1 has no rows to preserve)",
            o.edit_landed
        )
    };
    Ok(format!(
        "[probe in-place] {src}\n\
         orig list index = {}; scratch landed at list index {} ({:?})\n\
         orig name:    before={:?}  after={:?}  (edit_landed={})\n\
         scratch slot name after (expect cleared/'--'): {:?}\n\
         song-1 rows: before={} after={}  (had_binding={}, binding_preserved={})\n\
         {ac7}\n\
         songs_before={:?}\n\
         songs_after={:?}\n",
        o.orig_list_index,
        o.scratch_slot,
        o.scratch_name,
        o.orig_name_before,
        o.orig_name_after,
        o.edit_landed,
        o.scratch_name_after,
        o.songs_before.len(),
        o.songs_after.len(),
        o.had_binding,
        o.binding_preserved,
        o.songs_before,
        o.songs_after,
    ))
}

/// Probe (AC5): restore a backup snapshot to the device IN PLACE — onto the
/// snapshot's original slot, preserving its Song link. Reads the snapshot JSON,
/// validates it is a faithful offline backup (refuses `usb-partial` — re-importing
/// a partial would overwrite the slot with truncated data), re-XORs it to `.preset`
/// bytes, and routes through the AC7 in-place path. `snapshot.slot` is the list
/// index the backup was captured at.
pub fn probe_restore(snapshot_path: &str) -> Result<String, String> {
    let snap = backup::load_snapshot_from_path(std::path::Path::new(snapshot_path))?;
    let bytes = backup::restore_bytes(&snap)?;
    run_replace_inplace(
        snap.slot,
        &bytes,
        &format!("restore={snapshot_path} (slot {})", snap.slot),
    )
}
