//! In-place preset re-import — overwrite a slot preserving its Song link.

use crate::session::Session;
use crate::{backup, read_song_presets, session};

/// Probe (AC7 positive case): edit a preset IN PLACE on its original slot and
/// report whether the slot, its Song assignment, and scene binding survive.
/// Compare against `--import` (bare append) as the negative control — that one
/// lands the edit at a new slot and the Song row then points at the stale copy.
pub fn probe_replace_inplace(orig_list_index: u32, path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    run_replace_inplace(orig_list_index, &bytes, &format!("file={path}"))
}

/// Shared in-place-edit core (used by `--replace-inplace` and AC5 `--restore`).
///
/// Sequence: import (appends a scratch copy) → locate the scratch by re-listing
/// (`requestNextEmptyPresetSlot` is dead, so we OBSERVE where it landed) →
/// `load_preset(scratch)` → `save_current_preset(orig)` to overwrite the original
/// slot in place → **guarded** `clear_user_preset(scratch)`. All addresses are
/// 0-based list indices; `session.rs` translates each to the 1-based device
/// userSlot. The Song-1 link is read before/after to confirm the binding survives.
/// Structured result of the in-place edit core — what landed where, whether the
/// edit took, and whether the Song binding survived. Consumed by the `--replace-inplace`
/// / `--restore` probe formatter AND by `preset_io::OfflineIo::write`.
pub(crate) struct ReplaceOutcome {
    pub orig_list_index: u32,
    pub scratch_slot: u32,
    pub scratch_name: String,
    pub orig_name_before: String,
    pub orig_name_after: Option<String>,
    pub scratch_name_after: Option<String>,
    pub edit_landed: bool,
    pub had_binding: bool,
    pub binding_preserved: bool,
    pub songs_before: Vec<session::SongPresetRecord>,
    pub songs_after: Vec<session::SongPresetRecord>,
}

/// Reusable in-place-edit core (AC7): import a scratch copy → locate it by observing
/// which previously-empty slot filled → `load(scratch)` → `save_current_preset(orig)`
/// to overwrite the original slot → **guarded** `clear(scratch)` → re-read to confirm
/// the edit landed and the Song-1 binding survived. All addresses are 0-based list
/// indices; `session.rs` translates each to the 1-based device userSlot.
pub(crate) fn replace_inplace_core(
    orig_list_index: u32,
    bytes: &[u8],
) -> Result<ReplaceOutcome, String> {
    // Strict (completeness-validated) reads for both landing-detection lists: a
    // tolerant tail-truncated read makes every slot past the cut look empty, so the
    // post-import diff would "find" the landing at a garbage slot (the fail-closed
    // confirm_active below catches it before damage, but the run still aborts).
    let before = Session::connect()?.list_my_presets_strict()?;
    let orig_name_before = before
        .iter()
        .find(|p| p.slot == orig_list_index)
        .map(|p| p.name.clone())
        .ok_or_else(|| {
            format!(
                "orig list index {orig_list_index} out of range ({} entries)",
                before.len()
            )
        })?;
    // Slots that were EMPTY before — import will fill exactly one of these.
    let empty_before: std::collections::HashSet<u32> = before
        .iter()
        .filter(|p| session::is_empty_slot_name(&p.name))
        .map(|p| p.slot)
        .collect();
    let songs_before = read_song_presets(1).unwrap_or_default();

    // 1) Import — appends a scratch copy of the edited preset into an empty slot.
    Session::connect()?.import_preset(bytes)?;

    // 2) Observe where it landed: a slot that was EMPTY before and is now occupied.
    // Keying on the previously-empty set (not a name diff) means a flaky/partial
    // baseline list can't misidentify a *real* pre-existing preset as the scratch
    // and get it cleared in step 3.
    let after_import = Session::connect()?.list_my_presets_strict()?;
    let (scratch_slot, scratch_name) = after_import
        .iter()
        .find(|p| empty_before.contains(&p.slot) && !session::is_empty_slot_name(&p.name))
        .map(|p| (p.slot, p.name.clone()))
        .ok_or_else(|| "could not locate the imported scratch preset (no previously-empty slot became occupied)".to_string())?;

    // 3) Land it on the original slot. The session layer translates these 0-based
    // list indices to the device's 1-based userSlot (HW-confirmed 1.7.75).
    Session::connect()?.load_preset(scratch_slot)?; // scratch becomes current (persists across reconnect)
                                                    // Fresh connection re-attaches to the now-current preset; CONFIRM it is the scratch
                                                    // copy BEFORE saving it over the (real, irreplaceable) original slot. A dropped load
                                                    // would leave a DIFFERENT preset current, and saving that over orig_list_index is
                                                    // silent data loss — so the guard lives in the SAME connection as the mutation. On
                                                    // failure ABORT before the save (and before the clear), leaving the scratch import on
                                                    // the device for manual recovery.
    let mut save_conn = Session::connect()?;
    save_conn
        .confirm_active(scratch_slot, Some(&scratch_name))
        .map_err(|e| {
            format!(
                "{e}. Left the scratch import at list index {scratch_slot} ({scratch_name:?}); \
                 the original slot {orig_list_index} was NOT modified."
            )
        })?;
    save_conn.save_current_preset(orig_list_index)?; // overwrite the original slot in place
    guarded_clear(scratch_slot, &scratch_name)?; // remove the scratch copy (guarded)

    // 4) Re-read and confirm slot / Song-link survival. Settle first: clear/save are
    // fire-and-forget (no ACK); give the device a moment or the read returns pre-clear state.
    std::thread::sleep(std::time::Duration::from_millis(800));
    let after = Session::connect()?.list_my_presets()?;
    let orig_name_after = after
        .iter()
        .find(|p| p.slot == orig_list_index)
        .map(|p| p.name.clone());
    let scratch_name_after = after
        .iter()
        .find(|p| p.slot == scratch_slot)
        .map(|p| p.name.clone());
    let songs_after = read_song_presets(1).unwrap_or_default();
    // A meaningful binding check needs a binding to begin with — equal-but-empty
    // (both reads returned no rows) is NOT evidence the link survived.
    let had_binding = songs_before.iter().any(|r| !r.is_empty);
    let binding_preserved = had_binding && songs_before == songs_after;
    let edit_landed = orig_name_after.as_deref() != Some(orig_name_before.as_str());

    Ok(ReplaceOutcome {
        orig_list_index,
        scratch_slot,
        scratch_name,
        orig_name_before,
        orig_name_after,
        scratch_name_after,
        edit_landed,
        had_binding,
        binding_preserved,
        songs_before,
        songs_after,
    })
}

/// String-reporting wrapper over [`replace_inplace_core`] for the probe subcommands.
fn run_replace_inplace(orig_list_index: u32, bytes: &[u8], src: &str) -> Result<String, String> {
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

/// Clear the user preset at LIST index `list_index`, but only if that list entry
/// reads `expect_name`. The guard checks the slot in **list-index space** and then
/// clears the matching **device userSlot = list_index + 1** — so the verification
/// and the mutation address the *same* preset (the earlier guard bug checked the
/// list index but cleared a same-numbered device slot = a different preset).
pub(crate) fn guarded_clear(list_index: u32, expect_name: &str) -> Result<(), String> {
    let list = Session::connect()?.list_my_presets_strict()?;
    let cur = list
        .iter()
        .find(|p| p.slot == list_index)
        .map(|p| p.name.as_str());
    if cur != Some(expect_name) {
        return Err(format!(
            "guarded clear refused: list index {list_index} reads {cur:?}, expected {expect_name:?}"
        ));
    }
    Session::connect()?.clear_user_preset(list_index) // session translates list → device slot
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
