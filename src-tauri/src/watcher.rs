//! TMP hotplug watcher: surfaces physical attach/detach of the device to the
//! frontend as Tauri events, so the UI can drop to "disconnected" on unplug
//! and reconnect immediately on replug (instead of waiting for the 3 s poll).
//!
//! On macOS, uses a NON-seizing `IOHIDManager` that only registers
//! matching/removal callbacks for the TMP's VID/PID — no `IOHIDManagerOpen`, no
//! device I/O. That means it never interferes with the session's exclusive
//! seize, generates zero protocol traffic, and cannot false-positive during
//! `with_released_seize` windows (the device's physical presence doesn't
//! change while the seize is merely released for a leveling pass).
//!
//! On Linux there is no netlink/udev-monitor equivalent reachable without a
//! `udev` crate dependency (`hid.rs`'s hidraw transport deliberately has none —
//! see its module note), so the Linux `imp` instead polls
//! `hid::device_present()` — the same sysfs walk `hid.rs` uses to open the
//! device, just without opening it — once a second. Same events, same
//! detach-cleanup ordering, coarser latency (up to ~1 s vs. IOKit's
//! near-instant callback).
//!
//! Windows also registers non-seizing Configuration Manager HID notifications.
//! Queued removal/arrival events survive a quick replug between the one-second
//! SetupAPI presence polls. Failed scans preserve the previous presence state.
//!
//! On removal (all supported platforms) it also clears the shared session slot: the
//! seized handle is dead once the device is gone, and dropping it keeps a
//! later reconnect from colliding with our own stale exclusive handle
//! (`0xe00002c5` on macOS; the `flock` on Linux).

use std::sync::{Arc, Mutex};

use crate::session::Session;

/// Event names the frontend listens for (`@tauri-apps/api/event`).
///
/// The macOS, Linux and Windows `imp`s emit these; elsewhere they are
/// genuinely dead until a platform watcher exists to fire them. The allow is
/// scoped to exactly that case rather than blanket-applied to the module, so a
/// constant going unused on a platform that DOES have a watcher still surfaces
/// as a warning.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", windows)),
    allow(dead_code)
)]
pub const EVT_ATTACHED: &str = "tmp://device-attached";
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", windows)),
    allow(dead_code)
)]
pub const EVT_DETACHED: &str = "tmp://device-detached";

/// Spawn the watcher thread. Lives for the whole process; never joined.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub fn spawn(app: tauri::AppHandle, session: Arc<Mutex<Option<Session>>>) {
    imp::spawn(app, session);
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn spawn(_app: tauri::AppHandle, _session: Arc<Mutex<Option<Session>>>) {}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease, CFTypeRef};
    use core_foundation_sys::dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreateMutable,
        CFDictionaryRef, CFDictionarySetValue, CFMutableDictionaryRef,
    };
    use core_foundation_sys::number::{kCFNumberSInt32Type, CFNumberCreate};
    use core_foundation_sys::runloop::{
        kCFRunLoopDefaultMode, CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRun,
    };
    use core_foundation_sys::string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringRef,
    };
    use std::os::raw::{c_char, c_void};
    use tauri::Emitter;

    use crate::hid::{PID, VID};

    type IOHIDManagerRef = *mut c_void;
    type IOHIDDeviceRef = *mut c_void;
    type IOReturn = i32;

    /// IOHIDDeviceCallback — shared shape for matching + removal callbacks.
    type IOHIDDeviceCallback = extern "C" fn(
        context: *mut c_void,
        result: IOReturn,
        sender: *mut c_void,
        device: IOHIDDeviceRef,
    );

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDManagerCreate(allocator: CFTypeRef, options: u32) -> IOHIDManagerRef;
        fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
        fn IOHIDManagerRegisterDeviceMatchingCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDDeviceCallback,
            context: *mut c_void,
        );
        fn IOHIDManagerRegisterDeviceRemovalCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDDeviceCallback,
            context: *mut c_void,
        );
        fn IOHIDManagerScheduleWithRunLoop(
            manager: IOHIDManagerRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
    }

    /// Callback context — intentionally leaked (the watcher lives for the
    /// process's whole life, so there is no teardown to balance it).
    struct Ctx {
        app: tauri::AppHandle,
        session: Arc<Mutex<Option<Session>>>,
    }

    // Small CF constructors, self-contained like hid.rs's (each module owns its
    // copy — same convention as the stub catalog's static-inline client_base.h).
    fn cf_number_i32(value: i32) -> CFTypeRef {
        unsafe {
            CFNumberCreate(
                kCFAllocatorDefault,
                kCFNumberSInt32Type,
                &value as *const i32 as *const c_void,
            ) as CFTypeRef
        }
    }

    fn cf_string(s: &str) -> CFStringRef {
        let c = std::ffi::CString::new(s).expect("no interior NUL");
        unsafe {
            CFStringCreateWithCString(
                kCFAllocatorDefault,
                c.as_ptr() as *const c_char,
                kCFStringEncodingUTF8,
            )
        }
    }

    /// Device matched (already present at schedule time, or just plugged in).
    extern "C" fn matched_cb(
        context: *mut c_void,
        _result: IOReturn,
        _sender: *mut c_void,
        _device: IOHIDDeviceRef,
    ) {
        if context.is_null() {
            return;
        }
        let ctx = unsafe { &*(context as *const Ctx) };
        log::info!("hotplug: TMP attached");
        let _ = ctx.app.emit(EVT_ATTACHED, ());
    }

    /// Device physically removed: drop the (now-dead) seized session so the
    /// next connect doesn't collide with our own stale handle, then notify.
    extern "C" fn removed_cb(
        context: *mut c_void,
        _result: IOReturn,
        _sender: *mut c_void,
        _device: IOHIDDeviceRef,
    ) {
        if context.is_null() {
            return;
        }
        let ctx = unsafe { &*(context as *const Ctx) };
        // May briefly block on an in-flight device command holding the lock;
        // that command fails on its own (the device is gone) and releases.
        // lock_ok (never .unwrap()): a poison-panic here would unwind across the
        // extern "C" boundary = abort.
        *crate::lock_ok(&ctx.session) = None;
        crate::monitor::reset_startup_state();
        // An offline unit can be edited elsewhere (Pro Control, the unit itself) --
        // the Doctor's cached BEFORE clip can no longer be trusted.
        crate::commands::doctor::clear_doctor_before_cache();
        log::info!("hotplug: TMP detached — session released");
        let _ = ctx.app.emit(EVT_DETACHED, ());
    }

    pub fn spawn(app: tauri::AppHandle, session: Arc<Mutex<Option<Session>>>) {
        std::thread::Builder::new()
            .name("tmp-hotplug-watcher".into())
            .spawn(move || unsafe {
                let ctx = Box::into_raw(Box::new(Ctx { app, session })) as *mut c_void;

                let manager = IOHIDManagerCreate(kCFAllocatorDefault as CFTypeRef, 0);
                if manager.is_null() {
                    log::warn!("hotplug: IOHIDManagerCreate returned null — watcher disabled");
                    return;
                }
                let dict: CFMutableDictionaryRef = CFDictionaryCreateMutable(
                    kCFAllocatorDefault,
                    0,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                );
                let vid_key = cf_string("VendorID");
                let pid_key = cf_string("ProductID");
                let vid_val = cf_number_i32(VID);
                let pid_val = cf_number_i32(PID);
                CFDictionarySetValue(dict, vid_key as *const c_void, vid_val as *const c_void);
                CFDictionarySetValue(dict, pid_key as *const c_void, pid_val as *const c_void);
                IOHIDManagerSetDeviceMatching(manager, dict as CFDictionaryRef);
                CFRelease(vid_key as CFTypeRef);
                CFRelease(pid_key as CFTypeRef);
                CFRelease(vid_val as CFTypeRef);
                CFRelease(pid_val as CFTypeRef);
                CFRelease(dict as CFTypeRef);

                IOHIDManagerRegisterDeviceMatchingCallback(manager, matched_cb, ctx);
                IOHIDManagerRegisterDeviceRemovalCallback(manager, removed_cb, ctx);
                // Matching/removal notifications need only a scheduled manager —
                // no IOHIDManagerOpen, so the session's exclusive seize is
                // untouched and no HID I/O happens on this thread.
                IOHIDManagerScheduleWithRunLoop(
                    manager,
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                );
                log::info!("hotplug: watcher armed (VID 0x{VID:04X} PID 0x{PID:02X})");
                CFRunLoopRun(); // parks this thread for the process's life
            })
            .expect("spawn tmp-hotplug-watcher");
    }
}

#[cfg(windows)]
mod windows_notifications;

#[cfg(any(target_os = "linux", windows))]
mod imp {
    use super::*;
    use std::time::Duration;
    use tauri::Emitter;

    /// Poll the non-seizing Linux/Windows presence backend once a second,
    /// faster than the UI's three-second connection-retry interval.
    const POLL_INTERVAL: Duration = Duration::from_secs(1);

    pub fn spawn(app: tauri::AppHandle, session: Arc<Mutex<Option<Session>>>) {
        std::thread::Builder::new()
            .name("tmp-hotplug-watcher".into())
            .spawn(move || {
                log::info!(
                    "hotplug: watcher armed (polling {} every {}s)",
                    if cfg!(windows) {
                        "Windows HID interfaces"
                    } else {
                        "/sys/class/hidraw"
                    },
                    POLL_INTERVAL.as_secs()
                );
                let mut last = false;
                #[cfg(windows)]
                let mut notifications = None;
                loop {
                    #[cfg(windows)]
                    {
                        // Register before the initial scan, then retain every
                        // removal witness even if the next scan is already true.
                        // Retry registration errors while polling remains active.
                        if notifications.is_none() {
                            match super::windows_notifications::Notifications::register() {
                                Ok(registered) => notifications = Some(registered),
                                Err(error) => {
                                    log::warn!("hotplug: notification registration failed: {error}")
                                }
                            }
                        }
                        if let Some(registered) = &notifications {
                            for evt in observe_notifications(&mut last, registered.observations()) {
                                emit_event(&app, &session, evt);
                            }
                        }
                    }
                    #[cfg(target_os = "linux")]
                    let scan = Ok(crate::hid::device_present());
                    #[cfg(windows)]
                    let scan = crate::hid::device_present();
                    let event = match observe_presence(&mut last, scan) {
                        Ok(event) => event,
                        Err(error) => {
                            log::warn!("hotplug: presence scan failed: {error}");
                            std::thread::sleep(POLL_INTERVAL);
                            continue;
                        }
                    };
                    if let Some(evt) = event {
                        emit_event(&app, &session, evt);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .expect("spawn tmp-hotplug-watcher");
    }

    fn emit_event(app: &tauri::AppHandle, session: &Arc<Mutex<Option<Session>>>, evt: &str) {
        if evt == EVT_DETACHED {
            // Same cleanup order as macOS: release the stale handle, reset the
            // monitor and Doctor caches, then notify the frontend.
            *crate::lock_ok(session) = None;
            crate::monitor::reset_startup_state();
            crate::commands::doctor::clear_doctor_before_cache();
            log::info!("hotplug: TMP detached — session released");
        } else {
            log::info!("hotplug: TMP attached");
        }
        let _ = app.emit(evt, ());
    }
}

/// A failed presence scan is uncertainty, not a physical detach. Keep the last
/// successful observation; startup is seeded false to emit an initial attach.
#[cfg(any(target_os = "linux", windows, test))]
fn observe_presence(
    last: &mut bool,
    scan: Result<bool, String>,
) -> Result<Option<&'static str>, String> {
    let now = scan?;
    let event = match (*last, now) {
        (false, true) => Some(EVT_ATTACHED),
        (true, false) => Some(EVT_DETACHED),
        _ => None,
    };
    *last = now;
    Ok(event)
}

/// Drain notification observations in order before reconciling the current scan.
/// A removal followed by arrival must still release the old session.
#[cfg(any(windows, test))]
fn observe_notifications(
    last: &mut bool,
    observations: impl Iterator<Item = bool>,
) -> Vec<&'static str> {
    observations
        .filter_map(|present| observe_presence(last, Ok(present)).ok().flatten())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_replug_between_present_polls_still_emits_detach_then_attach() {
        let mut last = true;
        assert_eq!(
            observe_notifications(&mut last, [false, true].into_iter()),
            vec![EVT_DETACHED, EVT_ATTACHED]
        );
        assert_eq!(observe_presence(&mut last, Ok(true)).unwrap(), None);
    }

    #[test]
    fn notification_cleanup_is_not_lost_when_the_following_scan_fails() {
        let mut last = true;
        assert_eq!(
            observe_notifications(&mut last, [false].into_iter()),
            vec![EVT_DETACHED]
        );
        assert!(observe_presence(&mut last, Err("scan unavailable".into())).is_err());
        assert!(!last);
        assert_eq!(
            observe_presence(&mut last, Ok(true)).unwrap(),
            Some(EVT_ATTACHED)
        );
    }

    #[test]
    fn present_at_startup_emits_attach_once() {
        let mut last = false;
        assert_eq!(
            observe_presence(&mut last, Ok(true)).unwrap(),
            Some(EVT_ATTACHED)
        );
        assert_eq!(observe_presence(&mut last, Ok(true)).unwrap(), None);
    }

    #[test]
    fn physical_removal_emits_detach_once() {
        let mut last = true;
        assert_eq!(
            observe_presence(&mut last, Ok(false)).unwrap(),
            Some(EVT_DETACHED)
        );
        assert_eq!(observe_presence(&mut last, Ok(false)).unwrap(), None);
    }

    #[test]
    fn failed_scan_preserves_presence_until_an_actual_removal() {
        let mut last = true;
        assert!(observe_presence(&mut last, Err("temporary SetupAPI failure".into())).is_err());
        assert!(last);
        assert_eq!(observe_presence(&mut last, Ok(true)).unwrap(), None);
        assert_eq!(
            observe_presence(&mut last, Ok(false)).unwrap(),
            Some(EVT_DETACHED)
        );
    }

    #[test]
    fn failed_startup_scan_does_not_prevent_a_later_attach() {
        let mut last = false;
        assert!(observe_presence(&mut last, Err("temporary SetupAPI failure".into())).is_err());
        assert!(!last);
        assert_eq!(observe_presence(&mut last, Ok(false)).unwrap(), None);
        assert_eq!(
            observe_presence(&mut last, Ok(true)).unwrap(),
            Some(EVT_ATTACHED)
        );
    }
}
