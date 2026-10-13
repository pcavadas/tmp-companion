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
//! - production and the ONLINE e2e tier: [`DeviceRig`], the real seized HID and Core Audio
//!   / ALSA / WASAPI streams;
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

/// The build's rig for a thread with none installed. See the module doc.
fn default_rig() -> &'static dyn Rig {
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

/// [`Rig::capture`] on this thread's rig.
pub(crate) fn capture(stimulus: &[f32], rate: u32, tail_ms: u64) -> Result<Capture, LevelError> {
    with_current(|r| r.capture(stimulus, rate, tail_ms))
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
}
