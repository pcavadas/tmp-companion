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

/// Scale a hardware settle for the current tier: unchanged in production and on the ONLINE
/// e2e tier, ZERO when the offline SimDevice fake is installed.
///
/// Every settle in this codebase exists to let a REAL Tone Master Pro's IOKit seize or DSP
/// state catch up — an in-process fake has neither. They are also conditional on WHICH
/// commands were sent, never on elapsed time, so zeroing them cannot change the emitted
/// wire sequence (the `/sim/events` golden is the gate for exactly that claim).
///
/// `TMP_E2E_KEEP_SETTLES=1` restores full production timing in the offline tier. It is the
/// bisect handle for the one risk this collapse carries: zeroing a settle also removes a
/// thread yield, so if an offline spec ever turns flaky, re-run it with this set — flaky
/// with it, a real product race; green with it, the yield mattered and this function is the
/// single place to reintroduce a small non-zero floor.
pub(crate) fn settle_ms(ms: u64) -> u64 {
    #[cfg(feature = "e2e")]
    if crate::e2e_offline_fake() && std::env::var_os("TMP_E2E_KEEP_SETTLES").is_none() {
        return 0;
    }
    ms
}

/// [`settle_ms`] as a blocking sleep. Takes a `Duration` rather than millis so the ~80
/// raw `std::thread::sleep(..)` settle sites swap by NAME only, keeping each call site's
/// units and constants exactly as written.
///
/// NOT a drop-in for a HID retry backoff or a poll cadence — only for settles, which are
/// dead time against a fake. `hid.rs`'s open-retry and `monitor.rs`'s poll loops keep
/// `std::thread::sleep`.
pub(crate) fn settle(d: std::time::Duration) {
    std::thread::sleep(std::time::Duration::from_millis(settle_ms(
        d.as_millis() as u64
    )));
}

/// Sleep up to `ms`, waking early if an abort is requested. Returns `true` if it aborted
/// (the caller bails), `false` if the full duration elapsed.
///
/// Sleeps the FULL duration in every tier. Scaling is opt-in via [`settle_abortable`] /
/// [`settle_or_cancel`], deliberately: some callers are a POLL CADENCE, not a settle, and
/// for them the sleep is the only thing bounding a loop. `audio.rs`'s capture-hop loop is
/// the example — `while Instant::now() < deadline { sleep_or_cancel(hop)? … }`, where a
/// zero hop would busy-spin for the whole capture hammering the sample-buffer mutex. Making
/// the collapse opt-in means a NEW caller is correct by default and only settles opt in.
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

/// [`sleep_abortable`] for a HARDWARE SETTLE: [`settle_ms`]-scaled, so it collapses to a
/// single abort check offline. Use for a wait that exists to let the device catch up; use
/// the unscaled `sleep_abortable` for a poll cadence.
pub(crate) fn settle_abortable(ms: u64) -> bool {
    sleep_abortable(settle_ms(ms))
}

/// [`sleep_or_cancel`] for a HARDWARE SETTLE — the `?`-able form of [`settle_abortable`].
/// The abort check runs before the (possibly zero) wait, so Stop still wins in both tiers.
pub(crate) fn settle_or_cancel(ms: u64) -> Result<(), String> {
    sleep_or_cancel(settle_ms(ms))
}

/// Bounded wait for the monitor to ack a pause (≈ `PAUSE_WAIT_TRIES × 25 ms`). The
/// monitor pumps in ~120 ms windows, so it checks the flag ~8×/sec, and an idle
/// monitor acks well inside 1 s. The budget is 4 s because the monitor can't ack
/// while it is mid-HANDSHAKE: a `graph=none` re-snapshot cycle (drop → 3 s backoff
/// → re-handshake, `monitor.rs`) held the ack ~1 s past the old 1 s budget on a
/// real run, and the calibration's own `Session::connect` then raced the still-
/// seized device — its re-amp OFF never reached the unit and the take read as
/// "no instrument signal". Waiting costs nothing when the monitor is idle (the
/// loop exits on the ack) and is exactly right when it is busy. If the budget is
/// still exceeded (wedged monitor), the command proceeds anyway — `hid.rs`'s
/// bounded `IOHIDDeviceOpen` retry (≤0.48 s on `0xe00002c5`) absorbs the residual
/// race, the same safety net that already covers `with_released_seize`'s own
/// drop→reconnect lag.
const PAUSE_WAIT_TRIES: u32 = 160;
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

    // Wait for the ack only when someone can actually SEND one. Two conditions, and both
    // are load-bearing:
    //
    // - `MONITOR_ENABLED` — a disabled monitor idles in its disabled branch and never acks,
    //   so waiting would burn the full `PAUSE_WAIT_TRIES × PAUSE_WAIT_STEP_MS` budget on
    //   every command whenever live-sync is off. The one transition where the flag is
    //   already false while the monitor still holds its seize for ≤1 pump (`stop_live_sync`
    //   clears it before locking) is absorbed by hid.rs's bounded open-retry.
    // - `MONITOR_SPAWNED` — a monitor THREAD exists at all. `e2e_server` sets
    //   `MONITOR_ENABLED` in BOTH tiers (it wants the reconnect skip in
    //   `with_released_seize_blocking`) but never calls `monitor::spawn`, so the ack could
    //   never arrive and EVERY bridged command paid the full budget: measured 1.14 s for a
    //   trivial `e2e_load_preset`. This is the precise condition — "is there a thread to
    //   answer?" — so it fixes the ONLINE e2e tier too, not just the offline fake, and it
    //   cannot mask a wedged monitor in production (there the thread exists, so the wait
    //   and its `log::warn!` still happen).
    if MONITOR_ENABLED.load(SeqCst) && crate::MONITOR_SPAWNED.load(SeqCst) {
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

    /// NON-REGRESSION GATE: the offline settle collapse must never reach hardware.
    ///
    /// `settle_ms` returns 0 only while the offline SimDevice fake is installed in THIS
    /// process. Every other build and tier — the shipped app, and the ONLINE e2e tier
    /// driving a real Tone Master Pro — must get the settle back unchanged, because those
    /// sleeps are what let the unit's IOKit seize and DSP state catch up. A regression
    /// here is silent and expensive: writes drop with no `presetError` (the ~400-450 ms
    /// idle-gap cliff), so it surfaces as corrupt levels on a real unit, not a red test.
    ///
    /// Asserted for the DEFAULT process state, which is what makes it meaningful in both
    /// build modes. Under `--features e2e` nothing has called `e2e_install_offline_fake`
    /// (the unit tests install their fake via raw `set_factory`/`set_live`), so the flag is
    /// false and this is a live check on the guard; without the feature the branch does not
    /// compile at all and this pins the identity path. It is deliberately OUTSIDE any
    /// `#[cfg(feature = "e2e")]` gating, mirroring `lib.rs`'s `fixture_gates`: a gate that
    /// only compiles under a feature CI does not build is not a gate.
    #[test]
    fn settles_are_full_length_unless_the_offline_fake_is_installed() {
        #[cfg(feature = "e2e")]
        assert!(
            !crate::e2e_offline_fake(),
            "the offline-fake flag must be armed ONLY by e2e_install_offline_fake / \
             e2e_install_showcase — never merely by building with --features e2e"
        );
        for ms in [1_u64, 150, 400, 600, 800, 5_000] {
            assert_eq!(
                settle_ms(ms),
                ms,
                "settle_ms must be identity off the offline tier — a real device needs \
                 the full {ms}ms settle"
            );
        }
        // And the abortable SETTLE really sleeps. It must be `settle_abortable`, not
        // `sleep_abortable`: scaling is opt-in, so only the former routes through
        // `settle_ms` and only the former can regress into collapsing on hardware.
        OP_ABORT.store(false, SeqCst);
        let t = std::time::Instant::now();
        assert!(!settle_abortable(120));
        assert!(
            t.elapsed() >= std::time::Duration::from_millis(110),
            "settle_abortable must not collapse off the offline tier"
        );
    }
}
