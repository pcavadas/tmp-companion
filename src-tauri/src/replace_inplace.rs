//! In-place preset re-import — overwrite a slot preserving its Song link.

use crate::session::Session;
use crate::{backup, read_song_presets, session};

/// The tolerant list read, retried on a fresh reconnect until `pred` is satisfied (or a
/// reconnect stops helping). A single short read is the common case; the retry only
/// engages for the high-index tail-truncation class documented on `replace_inplace_with`.
fn read_list_until(
    pred: impl Fn(&[session::PresetEntry]) -> bool,
) -> Result<Vec<session::PresetEntry>, String> {
    const ATTEMPTS: u32 = 4;
    let mut list = Vec::new();
    for attempt in 1..=ATTEMPTS {
        list = Session::connect()?.list_my_presets()?;
        if pred(&list) || attempt == ATTEMPTS {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1_500));
    }
    Ok(list)
}

/// Retried until the list actually reaches `orig_list_index` — tests the SAME condition
/// the caller looks the entry up by (`find(|p| p.slot == orig_list_index)`), not merely
/// `list.len()`: a dropped INTERIOR entry can make the list longer than the index while
/// the target itself is still missing, which a length-only check would treat as reached.
fn read_list_reaching(orig_list_index: u32) -> Result<Vec<session::PresetEntry>, String> {
    read_list_until(|list| list.iter().any(|p| p.slot == orig_list_index))
}

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
    replace_inplace_with(orig_list_index, bytes, true)
}

/// `verify` = read the Song-1 bindings (before/after) + the post-save settle/re-read
/// that fill the outcome's report fields. The e2e seed passes `false`: scratch slots
/// carry no Song rows, and each verification read costs 1–4 fresh connections —
/// every open is one more chance to land in the device's post-close open LOCKOUT
/// (`0xe00002c5`, armed by aborted sessions and re-armed by each failed attempt),
/// so the seed keeps its open count minimal.
/// The write-safety chain (floored landing lists → `confirm_active` → guarded clear)
/// is identical in both modes.
pub(crate) fn replace_inplace_with(
    orig_list_index: u32,
    bytes: &[u8],
    verify: bool,
) -> Result<ReplaceOutcome, String> {
    // TOLERANT reads for both landing-detection lists — strict decodes only
    // terminal-frame streams and fails/garbles on the interleaved responses that
    // back-to-back lean sessions produce (HW-observed: tolerant 504/504 on a healthy
    // device while strict truncated or returned nothing). Tail-truncation is SAFE
    // here in both directions: a slot past the cut is ABSENT from the list (so it
    // never enters `empty_before` — landing detection misses it and aborts, it can
    // never mis-clear), and the fail-closed `confirm_active` below still gates the
    // save before any damage.
    //
    // A high scratch-zone target (the e2e seed's slots 400+) makes tail-truncation the
    // COMMON case rather than a rare tail: several fresh connects in quick succession
    // (this function's own multi-step sequence, on top of the caller's own reads) can
    // reliably chop the tolerant read well short of a high index (HW-observed 2026-07-27:
    // ~310-350/504 across many back-to-back attempts, independent of rest time — this
    // is the documented interleave/pump-window chop, not the open lockout, so waiting
    // doesn't help but a bounded reconnect-retry does). Retry a few times before giving
    // up, rather than erroring on the first short read.
    let before = read_list_reaching(orig_list_index)?;
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
    let songs_before = if verify {
        read_song_presets(1).unwrap_or_default()
    } else {
        Vec::new()
    };

    // 1) Import — appends a scratch copy of the edited preset into an empty slot.
    Session::connect()?.import_preset(bytes)?;

    // 2) Observe where it landed: a slot that was EMPTY before and is now occupied.
    // Keying on the previously-empty set (not a name diff) means a flaky/partial
    // baseline list can't misidentify a *real* pre-existing preset as the scratch
    // and get it cleared in step 3. Same retry as the baseline read above — the scratch
    // itself lands at a high, previously-empty index, so it's exposed to the identical
    // tail-truncation chop; a single-attempt read here would burn the run's import on
    // the first short response instead of giving the reconnect-retry a chance to land it.
    let after_import = read_list_until(|list| {
        list.iter()
            .any(|p| empty_before.contains(&p.slot) && !session::is_empty_slot_name(&p.name))
    })?;
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
    drop(save_conn); // end the save's connection before guarded_clear opens the next one
    guarded_clear(scratch_slot, &scratch_name)?; // remove the scratch copy (guarded)

    if !verify {
        // Lean mode: the write is done (confirm_active gated the save; the clear was
        // guarded) — skip the report reads. Fields the reads would fill stay empty.
        return Ok(ReplaceOutcome {
            orig_list_index,
            scratch_slot,
            scratch_name,
            orig_name_before,
            orig_name_after: None,
            scratch_name_after: None,
            edit_landed: true,
            had_binding: false,
            binding_preserved: false,
            songs_before,
            songs_after: Vec::new(),
        });
    }

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
    // Tolerant read: a tail-truncated list leaves the slot ABSENT → cur = None →
    // the guard refuses (fail-closed). Strict fails outright on interleaved
    // responses from back-to-back sessions (see replace_inplace_with).
    let list = Session::connect()?.list_my_presets()?;
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
