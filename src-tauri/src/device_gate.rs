//! Process-global device-op serialization gate + monitor-pause guard.

use crate::session::Session;
use crate::{lock_ok, MONITOR_ENABLED, MONITOR_PAUSED_ACK, MONITOR_PAUSE_REQ};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};

/// Process-global device-operation gate (1 permit). The TMP is single-connection
/// exclusive-HID, and `AppState.session`'s `Mutex<Option<Session>>` only guards the
/// held-session SLOT — not the whole open→work→close→reconnect lifecycle of an
/// operation. So two operations can overlap: e.g. the Presets tab's
/// `read_active_preset` is still in its trailing reconnect (`with_released_seize`
/// re-acquire) when the Songs tab's `list_songs` starts, and the two
/// `IOHIDDeviceOpen`s collide with `0xe00002c5` (mis-reported as "close Pro
/// Control"). Every device operation holds this gate for its FULL duration.
/// Acquired INSIDE the `spawn_blocking` closure so the guard's lifetime is the
/// blocking work itself — it survives even if the async command future is dropped
/// (spawn_blocking work is not cancelled), and a panic only poisons it transiently
/// (recovered via `into_inner`, never permanently bricking device IO).
static DEVICE_OP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Cooperative abort for the in-flight device op's long WAITS. The per-lane cancel flags
/// (`PRESET_LEVEL_CANCEL`, `SCENE_LEVEL_CANCEL`, `FOOTSWITCH_LEVEL_CANCEL`, `DOCTOR_CANCEL`)
/// decide which *step* bails; they are only read at step seams, so a Stop pressed during
/// the ~6 s re-amp capture (or the settles around it) used to sit out the whole thing —
/// 10 s of dead time, ~22 s when the floor guard's 5 s retry gap fired. This flag makes
/// those waits interruptible: every sleep on a leveling/Doctor path polls it and bails
/// with [`crate::leveller::CANCELLED`].
///
/// ponytail: ONE process-global flag, not one per lane — [`DEVICE_OP_LOCK`] serializes
/// device work, so exactly one operation can ever observe it. Per-lane abort flags if a
/// second concurrent device op ever becomes a thing.
static OP_ABORT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Poll cadence for [`sleep_abortable`] — the worst-case overshoot after a Stop.
const ABORT_POLL_MS: u64 = 50;

/// Request the in-flight device op abandon its waits — called by every `cancel_*` command.
pub(crate) fn request_op_abort() {
    OP_ABORT.store(true, SeqCst);
}

/// Has an abort been requested? Polled inside the re-amp capture window.
pub(crate) fn op_aborted() -> bool {
    OP_ABORT.load(SeqCst)
}

/// Sleep up to `ms`, waking early if an abort is requested. Returns `true` if it aborted
/// (the caller bails), `false` if the full duration elapsed. Drop-in for the settle
/// `thread::sleep`s on the leveling/Doctor paths — the settle semantics are unchanged for
/// a run nobody stopped.
pub(crate) fn sleep_abortable(ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        if op_aborted() {
            return true;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(ABORT_POLL_MS)));
    }
}

/// [`sleep_abortable`] as a `?`-able step: `Ok(())` if the wait completed, the
/// [`crate::leveller::CANCELLED`] sentinel if a Stop landed. The shape of nearly every
/// settle on a leveling/Doctor path — `sleep_or_cancel(SETTLE_X)?;`.
pub(crate) fn sleep_or_cancel(ms: u64) -> Result<(), String> {
    if sleep_abortable(ms) {
        Err(crate::leveller::CANCELLED.to_string())
    } else {
        Ok(())
    }
}

/// Bounded wait for the monitor to ack a pause (≈ `PAUSE_WAIT_TRIES × 25 ms`). The
/// monitor pumps in ~120 ms windows, so it checks the flag ~8×/sec; 40 × 25 ms = 1 s
/// is generous. If the budget is exceeded (monitor mid-connect on a flooded
/// device), the command proceeds anyway — `hid.rs`'s bounded `IOHIDDeviceOpen` retry
/// (≤0.48 s on `0xe00002c5`) absorbs the residual race, the same safety net that
/// already covers `with_released_seize`'s own drop→reconnect lag.
const PAUSE_WAIT_TRIES: u32 = 40;
const PAUSE_WAIT_STEP_MS: u64 = 25;

/// RAII guard returned by [`lock_device_op`]: holds [`DEVICE_OP_LOCK`] AND keeps the
/// monitor paused (`MONITOR_PAUSE_REQ` true) for the guard's whole lifetime. On Drop
/// it clears the pause request (the monitor resumes + re-reads fresh state) and
/// releases the device-op lock. So the monitor stays parked for exactly the command's
/// release→work→reconnect window — it cannot interleave a seize between the command's
/// own fresh connections (which would break the leveller's latch model). Runs on
/// unwind too, so a command panic still resumes the monitor.
pub(crate) struct MonitorPauseGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for MonitorPauseGuard {
    fn drop(&mut self) {
        MONITOR_PAUSE_REQ.store(false, SeqCst);
    }
}

/// Acquire the device-operation gate (poison-tolerant) AND pause the persistent
/// monitor so this command owns the device exclusively. Serializes against other
/// commands first (the existing behavior), THEN asks the monitor to drop its seize
/// and waits (bounded) for the ack. Hold the returned guard for the whole device
/// operation; its Drop resumes the monitor. See [`DEVICE_OP_LOCK`] / [`MonitorPauseGuard`].
///
/// Deadlock-free by construction: the monitor acquires NO lock, so the command's
/// bounded *sleep* on `MONITOR_PAUSED_ACK` is never a lock-acquire cycle. The monitor
/// owns only the device, which the pause protocol forces it to release.
pub(crate) fn lock_device_op() -> MonitorPauseGuard {
    let g = lock_ok(&DEVICE_OP_LOCK);
    // Every device op starts with a clean abort flag, by construction — arming here rather
    // than at each run command means a new command can never inherit a stale Stop (and a
    // command QUEUED behind this lock can't clear the flag of the op currently running).
    OP_ABORT.store(false, SeqCst);
    MONITOR_PAUSE_REQ.store(true, SeqCst); // ask the monitor to yield its seize
                                           // Only wait for the ack while the monitor is actually enabled — a disabled
                                           // monitor never acks (it idles in its disabled branch), so waiting would burn
                                           // the full `PAUSE_WAIT_TRIES × 25 ms = 1 s` budget on EVERY command whenever
                                           // live-sync is off. The one transition where the flag is already false while
                                           // the monitor still holds its seize for ≤1 pump (`stop_live_sync` clears it
                                           // before locking) is absorbed by hid.rs's bounded open-retry, as documented
                                           // on PAUSE_WAIT_TRIES.
    if MONITOR_ENABLED.load(SeqCst) {
        let mut acked = false;
        for _ in 0..PAUSE_WAIT_TRIES {
            if MONITOR_PAUSED_ACK.load(SeqCst) {
                acked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(PAUSE_WAIT_STEP_MS));
        }
        if !acked {
            // Proceeding anyway (hid.rs's open-retry covers the seize-recycle race), but a
            // persistent no-ack means the monitor is wedged — every device op then pays the
            // full wait. Surface it instead of silently eating the latency.
            log::warn!(
                "device op proceeding without a monitor pause-ack ({PAUSE_WAIT_TRIES} tries × \
                 {PAUSE_WAIT_STEP_MS}ms) — the monitor may be wedged"
            );
        }
    }
    // Proceed even if not acked within budget (see PAUSE_WAIT_TRIES) — hid.rs's
    // open-retry covers the residual seize-recycle race.
    MonitorPauseGuard(g)
}

/// Settle gap before re-establishing the UI session, so the IOKit seize the
/// device work just released has time to free up before we re-open it.
pub(crate) const RECONNECT_AFTER_MS: u64 = 400;

/// Run blocking device work with the app's HID seize released — the leveller and
/// calibration open their own fresh connections, so the app must NOT hold a
/// competing seize while they run. Re-establishes a live session for the UI
/// afterward regardless of outcome, so the connection/preset list survive. This
/// release→work→reconnect bookend is shared by every command that drives the
/// device through its own connections.
pub(crate) async fn with_released_seize<T, F>(
    arc: Arc<Mutex<Option<Session>>>,
    work: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || with_released_seize_blocking(arc, work))
        .await
        .map_err(|e| format!("device task failed: {e}"))?
}

/// Blocking core of [`with_released_seize`] — split out so commands that try the
/// monitor's live command lane first (`monitor::try_live_op`) can fall back to the
/// release→work→reconnect bookend inside their own `spawn_blocking`.
pub(crate) fn with_released_seize_blocking<T, F>(
    arc: Arc<Mutex<Option<Session>>>,
    work: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let _op = lock_device_op(); // serialize the whole release→work→reconnect
    *lock_ok(&arc) = None;
    let result = work();
    // Re-establish the UI session so the connection / preset list survive the
    // command — UNLESS live-sync is active, in which case the MONITOR owns the
    // device: re-grabbing the UI seize here would leave `session = Some` and
    // permanently block the monitor on its `is_none()` opportunism check (the
    // hero would stay stuck "Reading active preset…"). When live-sync owns the
    // device, leave the seize RELEASED and let the monitor re-take it on its
    // next poll (the `_op` guard's Drop clears the pause that paused it) — and
    // skip the settle sleep too: it only exists to protect OUR immediate re-open
    // below, and the monitor's own connect path already absorbs the kernel's
    // seize-recycle lag (hid.rs bounded open-retry + its reconnect backoff).
    if !MONITOR_ENABLED.load(SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
        if let Ok(s) = Session::connect() {
            *lock_ok(&arc) = Some(s);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stop-latency contract: an armed wait runs its full duration, an aborted one
    /// returns within a poll interval no matter how long it was asked to sleep. This is
    /// what collapses a Stop pressed mid-capture from ~10 s to ~0.2 s.
    #[test]
    fn sleep_abortable_wakes_early_on_abort() {
        OP_ABORT.store(false, SeqCst);
        let t = std::time::Instant::now();
        assert!(!sleep_abortable(150), "no abort → sleeps the full duration");
        assert!(t.elapsed() >= std::time::Duration::from_millis(140));

        request_op_abort();
        let t = std::time::Instant::now();
        assert!(sleep_abortable(10_000), "abort → reports it bailed");
        assert!(
            t.elapsed() < std::time::Duration::from_millis(500),
            "a 10 s wait must not be sat out after a Stop"
        );
        OP_ABORT.store(false, SeqCst);
    }
}
