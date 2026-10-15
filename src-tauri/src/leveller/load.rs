//! Preset loads and restores: the latch-rule isolated load ([`load_preset_isolated`]), the
//! stored-preset restore that discards unsaved edits ([`restore_saved_preset`]), and the
//! restore-after-error wrappers every unsaved run ends through.

use super::*;

/// Reload the stored preset to discard temporary level edits made while
/// measuring. `save=false` is a preview/read-only contract for callers: the TMP
/// edit buffer may be mutated during capture, but it must not remain dirty.
///
/// fw 1.8.58 skips a same-slot `loadPreset` while the working copy's dirty flag is clear,
/// keeping the working copy. Every edit the callers make sets the flag
/// (HW 2026-10-10, `probe --restore-check`), but a clear flag can still hide a stale
/// working copy, and reading `isDirty` costs ~1.1 s. So the restore always loads a
/// neighbour slot first, which forces the reload (+0.34 s on HW: 1.67 s vs 1.33 s).
///
/// It ends with [`Session::lapse`] for the reason [`load_preset_isolated`] does, and here
/// the queued copy is the one the restore exists to discard: without it a Doctor discard
/// read back the applied block (online e2e, 3/3 at a 100 ms settle, HW 2026-10-14).
pub(crate) fn restore_saved_preset(slot: ListIndex) -> Result<(), String> {
    // NOT `sleep_or_cancel`: this runs AFTER a cancel to clean up. Bailing here would leave
    // the edit buffer dirty at the measurement level — the whole point of the restore.
    let mut s = Session::connect_lean()?;
    let via = reload_stored_on(&mut s, slot)?;
    crate::device_gate::settle(Duration::from_millis(settle_after_load_ms()));
    // The handshake's own field-3 reply (the DIRTY working copy this restore discards) is
    // still queued behind its ~1.2 s drain, and the next session would read it first.
    s.lapse()?;
    log::info!("restored stored preset slot={slot} after unsaved measurement (via {via})");
    Ok(())
}

/// [`restore_saved_preset`]'s loads on `s`: a neighbour slot, then the target. Returns the
/// neighbour, which stays inside the e2e scratch zone when the target is in it.
pub(super) fn reload_stored_on(s: &mut Session, slot: ListIndex) -> Result<ListIndex, String> {
    let other = if slot == crate::slot::SCRATCH_SLOTS[0] {
        ListIndex(slot.get() + 1)
    } else {
        ListIndex(slot.get().checked_sub(1).unwrap_or(1))
    };
    s.load_preset(other)?;
    s.load_preset(slot)?;
    Ok(other)
}

/// Load `slot` on its own lean connection so it is the device's current preset. A rich
/// harvest (`discover_blocks_rich`, `prepass_scene_docs`) keeps the LATEST preset push it
/// collects, and a push of the previously-current preset can still land after its buffer
/// clear: online e2e (2026-10-10, fw 1.8.58) slot 410's block discovery, run right after a
/// save on another slot, returned that slot's blocks. With the target already current,
/// every push the harvest can see is the target's. A bare load on purpose: a same-slot
/// no-op still leaves the target current, and a neighbour load would add its own pushes.
pub(crate) fn make_current(slot: ListIndex) -> Result<(), String> {
    load_preset_isolated(slot, CancelMode::Cleanup).map_err(String::from)
}

/// How [`load_preset_isolated`]'s post-load settle treats a Stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelMode {
    /// The settle wakes on a Stop and the load returns [`LevelError::Cancelled`]: the
    /// step ahead of a capture, where nothing is engaged or written yet.
    Abortable,
    /// The settle always runs to its end: cleanup, reloads, and the legacy seams that
    /// never polled for a Stop here.
    Cleanup,
}

/// THE latch-rule load: open a lean connection, load `slot`, settle
/// [`settle_after_load_ms`], let the session lapse, and drop the connection. `danger.md`: a `set_preset_level`
/// after a load on the same connection is overridden by the load's own level-apply, and a
/// load + engage on one connection captured silence (fw 1.8.45), so every leveling path
/// loads in its own connection and measures or writes on a fresh one; the device keeps the
/// loaded preset current across reconnects. One home so the settle length and the
/// connection shape change in one place.
///
/// Every session that ends here ends with [`Session::lapse`]: a lean session is shorter
/// than the device's ~1.2 s handshake drain, and the unit keeps sending a queue across a
/// reconnect inside its inactivity drop, so the next session would receive the rest of
/// this one's replies ahead of its own — last of all, this handshake's field-3 copy of the
/// PRE-load working copy (HW 2026-10-14, fw 1.8.58: a reopen 0–500 ms after the close got
/// it every time; after 0.75 s of host silence the unit had discarded the queue). Host
/// silence past the drop is what makes the unit discard it, so the lapse IS the fix; it
/// costs the part of [`HID_INACTIVITY_DROP_MS`] + 100 ms the settle has not already spent.
///
/// [`HID_INACTIVITY_DROP_MS`]: crate::session::HID_INACTIVITY_DROP_MS
pub(crate) fn load_preset_isolated(slot: ListIndex, mode: CancelMode) -> Result<(), LevelError> {
    // The connection is held through the settle and dropped after it, as every inline
    // copy of this sequence did.
    let mut s = Session::connect_lean()?;
    s.load_preset(slot)?;
    match mode {
        CancelMode::Abortable => settle_or_cancel(settle_after_load_ms())?,
        CancelMode::Cleanup => {
            crate::device_gate::settle(Duration::from_millis(settle_after_load_ms()))
        }
    }
    s.lapse()?;
    drop(s);
    Ok(())
}

/// If an unsaved operation fails after touching a level control, try to discard
/// the temporary edit before returning the original error. A restore that fails too
/// comes back as [`LevelError::Restore`], which keeps the original's kind: a cancel
/// whose restore failed still reads as a cancel to every caller.
pub(crate) fn restore_after_unsaved_error<T>(
    slot: ListIndex,
    save: bool,
    result: Result<T, LevelError>,
) -> Result<T, LevelError> {
    restore_after_unsaved_error_with(slot, save, result, restore_saved_preset)
}

/// [`restore_after_unsaved_error`] over any `restore` — the seam a unit test hands a
/// failing restore, with no device.
pub(super) fn restore_after_unsaved_error_with<T>(
    slot: ListIndex,
    save: bool,
    result: Result<T, LevelError>,
    restore: impl FnOnce(ListIndex) -> Result<(), String>,
) -> Result<T, LevelError> {
    let err = match result {
        Ok(value) => return Ok(value),
        Err(err) => err,
    };
    // Restore the stored preset to discard a dirty edit buffer when nothing was
    // persisted: an unsaved op (save=false), OR a cancel — even with save=true a cancel
    // bails before `apply_level`, leaving `presetLevel` at the measurement reference
    // (`measure_knob_at` sets it and never restores). A non-cancel save error keeps the
    // prior pass-through behavior (no reload).
    if save && !err.is_cancelled() {
        return Err(err);
    }
    Err(err.with_restore(restore(slot)))
}

/// Fold a restore failure into a primary error — the ONE wording for the
/// "primary error + edit-buffer restore failure" merge every restore-after-
/// failure path shares (here, `probe_api::doctor_inject`/`doctor_defects`). The
/// String form of [`LevelError::with_restore`], for callers whose primary is text.
pub(crate) fn append_restore_err(primary: String, restore: Result<(), String>) -> String {
    LevelError::Device(primary)
        .with_restore(restore)
        .to_string()
}
