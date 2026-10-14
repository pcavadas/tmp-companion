//! The device-and-capture seam under every leveling orchestrator (#277): the [`Rig`] is
//! what a leveling or Doctor run actually touches — the device transport every `Session`
//! opens, the re-amp capture, the hardware settles, and the Stop flag. Everything above it
//! (the connection shapes, the re-amp ON/OFF choreography, cancel semantics) is the same
//! code on every rig.
//!
//! WHO GETS WHICH RIG. Device work runs on one blocking thread per command
//! (`device_gate::with_released_seize_blocking`), so the rig is PER THREAD, like
//! `session::reamp`'s `REAMP_UNSETTLED`: [`scoped`] installs one for a closure and every
//! `Session::connect*`, `audio::reamp_capture`, settle and abort check inside it — however
//! deep — goes to that rig. A thread with nothing installed gets the build's default:
//! - production and the ONLINE e2e tier (`TMP_E2E_ONLINE=1`): [`DeviceRig`], the real
//!   seized HID and Core Audio / ALSA / WASAPI streams;
//! - an e2e build with `TMP_E2E_REMOTE`: [`RemoteRig`], the remote device over
//!   `hid::remote::RemoteHid` (captures over `remote_audio` when `TMP_E2E_REMOTE_AUDIO` is
//!   set, refused otherwise; settles at full length). The remote device counts as online
//!   everywhere else (`e2e_online`), since its state is device state, not SimDevice's;
//! - an e2e build OFFLINE: `sim_device::LiveSimRig`, the e2e server's shared SimDevice
//!   (the transport factory, the physics capture model, collapsed settles).
//!
//! A unit test installs its own `sim_device::SimRig` — a private SimDevice, its own capture
//! model and its own Stop flag, no process globals — so leveling orchestration tests run in
//! parallel under plain `cargo test --lib` (`rig_tests.rs`).
//!
//! WHY NOT A PARAMETER. The orchestrators reach the device through ~50 nested helpers
//! (`measure_c` → `measure_knob_at` → `arm_measurement` → …), each opening its own fresh
//! connection by design (`danger.md`'s connection shapes). Threading a `&dyn Rig` through
//! all of them would touch every leveling signature for no behavioural gain; the scoped
//! rig keeps every production call site byte-for-byte as it was.

use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::Capture;
use crate::error::{DeviceError, LevelError};
use crate::hid::HidTransport;

/// What a leveling run touches: the device and its re-amp audio. See the module doc.
pub(crate) trait Rig {
    /// Open the device transport a `Session` runs over (`session::open_transport`).
    fn open_transport(&self) -> Result<Box<dyn HidTransport>, DeviceError>;

    /// Play `stimulus` into the re-amp input and return the processed USB-Out return, for
    /// the stimulus plus `tail_ms`. Re-amp is already ON (the caller's job).
    fn capture(&self, stimulus: &[f32], rate: u32, tail_ms: u64) -> Result<Capture, LevelError>;

    /// The wait a hardware settle of `ms` actually takes on this rig.
    fn settle_ms(&self, ms: u64) -> u64;

    /// Has a Stop been requested for the work running on this rig?
    fn aborted(&self) -> bool;
}

/// The real unit: the seized HID transport and the host's re-amp audio streams.
pub(crate) struct DeviceRig;

impl Rig for DeviceRig {
    fn open_transport(&self) -> Result<Box<dyn HidTransport>, DeviceError> {
        crate::trace::open(crate::trace::Backend::Device, || {
            Ok(Box::new(crate::hid::Hid::open()?))
        })
    }

    fn capture(&self, stimulus: &[f32], rate: u32, tail_ms: u64) -> Result<Capture, LevelError> {
        crate::trace::capture(
            crate::trace::Backend::Device,
            stimulus,
            rate,
            tail_ms,
            || crate::audio::reamp_capture_real(stimulus, rate, tail_ms),
        )
    }

    fn settle_ms(&self, ms: u64) -> u64 {
        ms
    }

    fn aborted(&self) -> bool {
        crate::device_gate::op_abort_requested()
    }
}

thread_local! {
    /// The rig [`scoped`] installed on this thread, if any. See the module doc.
    static SCOPED: RefCell<Option<Rc<dyn Rig>>> = const { RefCell::new(None) };
}

/// Run `f` with `rig` as this thread's rig, restoring the previous one afterwards (a panic
/// in `f` included). Test-only: production never installs one — the device rig IS its
/// default.
#[cfg(test)]
pub(crate) fn scoped<T>(rig: Rc<dyn Rig>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Rc<dyn Rig>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            SCOPED.with(|s| *s.borrow_mut() = prev);
        }
    }
    let _restore = Restore(SCOPED.with(|s| s.borrow_mut().replace(rig)));
    f()
}

/// The `remote` e2e tier (`TMP_E2E_REMOTE`): a remote device reached over
/// `hid::remote::RemoteHid`. Its captures are the remote device's own audio over `remote_audio`
/// (`TMP_E2E_REMOTE_AUDIO`); with no audio address a capture is refused rather than faked.
/// Settles run at the unit's real length.
#[cfg(feature = "e2e")]
pub(crate) struct RemoteRig;

#[cfg(feature = "e2e")]
impl Rig for RemoteRig {
    fn open_transport(&self) -> Result<Box<dyn HidTransport>, DeviceError> {
        let addr = crate::e2e_remote()
            .ok_or_else(|| DeviceError::Other("TMP_E2E_REMOTE is not a remote address".into()))?;
        crate::trace::open(crate::trace::Backend::Remote, || {
            Ok(Box::new(crate::hid::remote::RemoteHid::open(&addr)?))
        })
    }

    fn capture(&self, stimulus: &[f32], rate: u32, tail_ms: u64) -> Result<Capture, LevelError> {
        crate::trace::capture(
            crate::trace::Backend::Remote,
            stimulus,
            rate,
            tail_ms,
            || {
                let addr = crate::e2e_remote_audio().ok_or_else(|| {
                    LevelError::Device(
                    "the remote device has no re-amp audio here: set TMP_E2E_REMOTE_AUDIO to its \
                         audio address"
                        .into(),
                )
                })?;
                crate::remote_audio::capture(&addr, stimulus, rate, tail_ms, &|| self.aborted())
            },
        )
    }

    fn settle_ms(&self, ms: u64) -> u64 {
        ms
    }

    fn aborted(&self) -> bool {
        crate::device_gate::op_abort_requested()
    }
}

/// The build's rig for a thread with none installed. See the module doc.
fn default_rig() -> &'static dyn Rig {
    #[cfg(feature = "e2e")]
    if crate::e2e_remote().is_some() {
        return &RemoteRig;
    }
    #[cfg(feature = "e2e")]
    if !crate::e2e_online() {
        return &crate::sim_device::LiveSimRig;
    }
    &DeviceRig
}

/// Run `f` against this thread's rig. The installed `Rc` is cloned out first, so `f` may
/// re-enter (a capture that checks the Stop flag, a settle inside an open).
fn with_current<T>(f: impl FnOnce(&dyn Rig) -> T) -> T {
    match SCOPED.with(|s| s.borrow().clone()) {
        Some(rig) => f(&*rig),
        None => f(default_rig()),
    }
}

/// [`Rig::open_transport`] on this thread's rig.
pub(crate) fn open_transport() -> Result<Box<dyn HidTransport>, DeviceError> {
    with_current(|r| r.open_transport())
}

/// [`Rig::capture`] on this thread's rig. Its wall time joins [`WAITS`].
pub(crate) fn capture(stimulus: &[f32], rate: u32, tail_ms: u64) -> Result<Capture, LevelError> {
    let started = std::time::Instant::now();
    let result = with_current(|r| r.capture(stimulus, rate, tail_ms));
    WAITS.note_capture(started.elapsed());
    result
}

/// The process's running total of HARDWARE WAITS (#198): every settle the code asked for
/// and what this rig applied ([`crate::device_gate::settle_ms`]), and the wall time spent
/// inside re-amp captures ([`capture`]). On the device a capture is the stimulus playing
/// through the unit (~6.8 s), so these two are the time a run spends waiting on hardware
/// rather than computing; offline the settles collapse to zero and a capture is the
/// model's arithmetic plus any armed hold. Process-wide and monotonic: a reader takes a
/// snapshot before and after the work it times and subtracts (`scripts/e2e.sh`'s online
/// lane does exactly that per spec, through the e2e server's `GET /timing`).
///
/// An abortable settle that a Stop cut short still counts its full applied length, so the
/// applied total is an upper bound on time actually slept — the case is a cancelled run,
/// whose timing nobody budgets.
pub(crate) static WAITS: Waits = Waits::new();

/// See [`WAITS`].
pub(crate) struct Waits {
    settle_requested_ms: std::sync::atomic::AtomicU64,
    settle_applied_ms: std::sync::atomic::AtomicU64,
    settles: std::sync::atomic::AtomicU64,
    capture_ms: std::sync::atomic::AtomicU64,
    captures: std::sync::atomic::AtomicU64,
}

/// One reading of [`WAITS`]. `allow`, not `expect`: rustc 1.97 reports this struct dead
/// when [`Waits::totals`] is, rustc 1.96 does not, and an unfulfilled `expect` fails the build.
#[cfg_attr(
    not(any(test, feature = "e2e")),
    allow(
        dead_code,
        reason = "only the e2e server's /timing route reads the totals"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct WaitTotals {
    pub settle_requested_ms: u64,
    pub settle_applied_ms: u64,
    pub settles: u64,
    pub capture_ms: u64,
    pub captures: u64,
}

impl Waits {
    const fn new() -> Self {
        use std::sync::atomic::AtomicU64;
        Waits {
            settle_requested_ms: AtomicU64::new(0),
            settle_applied_ms: AtomicU64::new(0),
            settles: AtomicU64::new(0),
            capture_ms: AtomicU64::new(0),
            captures: AtomicU64::new(0),
        }
    }

    /// One settle: `requested` ms asked for, `applied` ms this rig waits.
    pub(crate) fn note_settle(&self, requested: u64, applied: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.settle_requested_ms.fetch_add(requested, Relaxed);
        self.settle_applied_ms.fetch_add(applied, Relaxed);
        self.settles.fetch_add(1, Relaxed);
    }

    /// One capture that took `took` of wall time (an error included).
    fn note_capture(&self, took: std::time::Duration) {
        use std::sync::atomic::Ordering::Relaxed;
        let ms = u64::try_from(took.as_millis()).unwrap_or(u64::MAX);
        self.capture_ms.fetch_add(ms, Relaxed);
        self.captures.fetch_add(1, Relaxed);
    }

    /// The totals so far.
    #[cfg_attr(
        not(any(test, feature = "e2e")),
        expect(
            dead_code,
            reason = "only the e2e server's /timing route reads the totals"
        )
    )]
    pub(crate) fn totals(&self) -> WaitTotals {
        use std::sync::atomic::Ordering::Relaxed;
        WaitTotals {
            settle_requested_ms: self.settle_requested_ms.load(Relaxed),
            settle_applied_ms: self.settle_applied_ms.load(Relaxed),
            settles: self.settles.load(Relaxed),
            capture_ms: self.capture_ms.load(Relaxed),
            captures: self.captures.load(Relaxed),
        }
    }
}

/// [`Rig::settle_ms`] on this thread's rig.
pub(crate) fn settle_ms(ms: u64) -> u64 {
    with_current(|r| r.settle_ms(ms))
}

/// [`Rig::aborted`] on this thread's rig.
pub(crate) fn aborted() -> bool {
    with_current(|r| r.aborted())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);
    impl Rig for Fixed {
        fn open_transport(&self) -> Result<Box<dyn HidTransport>, DeviceError> {
            Err(DeviceError::Other("no device in this test".into()))
        }
        fn capture(&self, _: &[f32], _: u32, _: u64) -> Result<Capture, LevelError> {
            Err(LevelError::NoSignal)
        }
        fn settle_ms(&self, _: u64) -> u64 {
            self.0
        }
        fn aborted(&self) -> bool {
            true
        }
    }

    /// A scoped rig answers only inside its closure, nests, and is restored after a panic.
    #[test]
    fn a_scoped_rig_is_this_threads_rig_only_inside_its_closure() {
        // Both sides read the e2e settle regime, which the IPC tests switch under this lock.
        #[cfg(feature = "e2e")]
        let _regime = crate::lock_ok(&crate::e2e_server::TEST_SERIAL);
        assert_eq!(settle_ms(120), default_rig().settle_ms(120));
        scoped(Rc::new(Fixed(7)), || {
            assert_eq!(settle_ms(120), 7);
            assert!(aborted());
            scoped(Rc::new(Fixed(9)), || assert_eq!(settle_ms(120), 9));
            assert_eq!(settle_ms(120), 7, "the inner rig is gone after its closure");
            // Another thread never sees this thread's rig.
            let other = std::thread::spawn(|| SCOPED.with(|s| s.borrow().is_none()));
            assert!(other.join().expect("join"));
        });
        let panicked = std::panic::catch_unwind(|| {
            scoped(Rc::new(Fixed(3)), || panic!("boom"));
        });
        assert!(panicked.is_err());
        assert!(
            SCOPED.with(|s| s.borrow().is_none()),
            "a panic inside the closure still restores the rig"
        );
    }

    /// The wait totals only ever grow, and a settle and a capture each land in their own
    /// counters: requested and applied apart, so the collapsed offline tier still shows what
    /// the device would have waited. A private `Waits`, so parallel tests cannot interfere.
    #[test]
    fn wait_totals_separate_settles_from_captures() {
        let w = Waits::new();
        w.note_settle(400, 0);
        w.note_settle(150, 150);
        w.note_capture(std::time::Duration::from_millis(6_800));
        assert_eq!(
            w.totals(),
            WaitTotals {
                settle_requested_ms: 550,
                settle_applied_ms: 150,
                settles: 2,
                capture_ms: 6_800,
                captures: 1,
            }
        );
    }
}
