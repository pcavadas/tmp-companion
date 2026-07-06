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

/// Bounded wait for the monitor to ack a pause (≈ `PAUSE_WAIT_TRIES × 25 ms`). The
/// monitor pumps in ~120 ms windows, so it checks the flag ~8×/sec; 40 × 25 ms = 1 s
/// is generous. If the budget is exceeded (monitor mid-connect on a congested
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
