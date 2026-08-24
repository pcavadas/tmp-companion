//! IOKit HID transport for the Tone Master Pro.
//!
//! Why raw IOKit (not the `hidapi` crate): the TMP's HID is exclusive-access,
//! and we *want* exclusive access to drive it. `hidapi` on macOS opens with
//! `kIOHIDOptionsTypeNone` (shared) and exposes no seize flag, so we go through
//! `IOHIDManager` directly with `kIOHIDOptionsTypeSeizeDevice`. Opening fails if
//! Pro Control already holds the device — surfaced as a friendly error.
//!
//! Threading: every IOKit call (create, schedule, open, `SetReport`, and the
//! input-report callback) must run on the thread whose `CFRunLoop` the device is
//! scheduled on. So one dedicated thread owns all device state for its whole
//! life; other threads issue `Send`/`Transact`/`Pump` commands over a channel.
//! Input reports only arrive while `CFRunLoopRunInMode` is running (during a
//! pump), all on that same thread — so the receive buffer needs no locking.

use crossbeam_channel::{bounded, Sender};
use std::thread::JoinHandle;

use crate::proto;

/// Fender vendor ID / Tone Master Pro product ID.
pub const VID: i32 = 0x1ED8;
pub const PID: i32 = 0x44;

/// Commands marshaled to the HID thread. Each carries a reply channel so the
/// caller blocks until the IOKit work completes on the owning thread.
enum Cmd {
    /// Fire-and-forget send (e.g. heartbeat). Replies with the send result.
    Send(Vec<u8>, Sender<Result<(), String>>),
    /// Send a body, pump the run loop `ms`, return the raw input reports that
    /// arrived during the pump. Reassembly happens caller-side (cumulatively),
    /// since a multi-packet stream can span several pumps.
    Transact(Vec<u8>, u64, Sender<Result<Vec<Vec<u8>>, String>>),
    /// Pump only (collect late reports), no send.
    Pump(u64, Sender<Result<Vec<Vec<u8>>, String>>),
    /// Send a body as one or more chunked output reports (`0x33/0x34*/0x35`),
    /// then pump `ms` and return the raw input reports. For bodies over
    /// `MAX_BODY` (e.g. `importPresetRequest`).
    TransactChunked(Vec<u8>, u64, Sender<Result<Vec<Vec<u8>>, String>>),
    /// Like `Transact`, but pumps in short slices and returns EARLY once the
    /// response looks complete (see [`EAGER_SLICE_MS`] / [`EAGER_QUIET_MS`]).
    /// `u64` is the max pump budget — never exceeded, so the worst case equals
    /// a plain `Transact` with the same window.
    TransactEager(Vec<u8>, u64, Sender<Result<Vec<Vec<u8>>, String>>),
    Close,
}

/// Slice granularity for the eager pump (ms). `CFRunLoopRunInMode` with
/// `returnAfterSourceHandled = false` always burns its full window, so early
/// exit requires pumping in small slices and checking between them.
const EAGER_SLICE_MS: u64 = 20;
/// Quiet window (ms) the eager pump requires AFTER the framing looks complete
/// before returning — catches a second stream that follows the first
/// back-to-back (inter-report gaps on the interrupt endpoint are ~ms, so 60 ms
/// of silence reliably marks the end of a push burst).
const EAGER_QUIET_MS: u64 = 60;

/// Fold a batch of raw input reports into the stream-open state: `0x33` opens a
/// multi-packet stream, `0x35` closes it (the 0x35-is-final rule of
/// `proto::reassemble_streams_final`); `0x34` continues. Returns the new state.
/// Pure so the eager-exit condition is unit-testable without IOKit.
fn fold_frame_open(reports: &[Vec<u8>], mut open: bool) -> bool {
    for r in reports {
        if r.len() < 4 || r[0] != 0x00 {
            continue;
        }
        match r[1] {
            proto::MAGIC_IN_START => open = true,
            0x35 => open = false,
            _ => {}
        }
    }
    open
}

/// Extract `(vendor_id, product_id)` from a sysfs `uevent` body.
///
/// The kernel writes `HID_ID=<bus>:<vendor>:<product>` with each field zero-padded
/// uppercase hex (e.g. `HID_ID=0003:00001ED8:00000044` for the TMP). Pure so the
/// hidraw lookup is testable without sysfs.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_hid_id(uevent: &str) -> Option<(i32, i32)> {
    let line = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = line.trim().split(':');
    let _bus = parts.next()?;
    let vid = i32::from_str_radix(parts.next()?, 16).ok()?;
    let pid = i32::from_str_radix(parts.next()?, 16).ok()?;
    Some((vid, pid))
}

/// Handle to the HID worker thread. Dropping it closes the device.
pub struct Hid {
    cmd_tx: Sender<Cmd>,
    join: Option<JoinHandle<()>>,
}

impl Hid {
    /// Open the TMP (seizing it). Errors if no device is present or Pro Control
    /// holds it. Blocks until the worker thread has the device open.
    pub fn open() -> Result<Hid, String> {
        imp::open()
    }
}

impl Drop for Hid {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Close);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// The device-transport seam: the send/pump primitives [`Session`] issues against the
/// open device. [`Hid`] is the production implementation (real IOKit seize); a test can
/// substitute an in-memory fake (`sim_device::SimDevice`) so the held-session edit/level
/// orchestration runs end-to-end with no hardware. Methods take `&self` (the real `Hid`
/// marshals over a channel to its worker thread), so the trait is object-safe and
/// `Session` stores a `Box<dyn HidTransport>`.
///
/// [`Session`]: crate::session::Session
pub trait HidTransport: Send {
    /// Fire-and-forget send (e.g. heartbeat).
    fn send(&self, body: &[u8]) -> Result<(), String>;
    /// Send `body`, pump for `pump_ms`, and return the raw input reports received during
    /// the pump. The caller reassembles cumulatively (a multi-packet stream can span
    /// several pump windows).
    fn transact(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String>;
    /// Send `body` as chunked output reports (`0x33/0x34*/0x35`) for bodies over
    /// `MAX_BODY`, then pump `pump_ms` and return the raw input reports.
    fn transact_chunked(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String>;
    /// Pump without sending — collects reports that land after a prior send. An implementation
    /// MAY return before `pump_ms` elapses (the sim returns instantly) — this is not a promise
    /// of wall-clock duration, only of "however long this pump took, here's what arrived". A
    /// caller that needs actual wall-clock pacing across a wait loop (spacing retries, not just
    /// collecting) must measure elapsed time itself rather than counting nominal pump slices;
    /// see `leveller::ensure_fresh_load_paced`'s wait loop for the pattern.
    fn pump(&self, pump_ms: u64) -> Result<Vec<Vec<u8>>, String>;
    /// Like [`Self::transact`], but returns EARLY once the response framing is complete
    /// and the line has gone quiet (`max_ms` stays the hard cap, so the worst case equals
    /// a plain transact). ONLY for sends whose callers don't parse the returned reports
    /// (e.g. fire-and-forget `loadPreset`/`loadScene`) — an early exit could otherwise
    /// drop a late second echo a caller expects.
    fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String>;
}

impl HidTransport for Hid {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        let (tx, rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::Send(body.to_vec(), tx))
            .map_err(|_| "HID thread gone".to_string())?;
        rx.recv()
            .map_err(|_| "HID thread dropped reply".to_string())?
    }

    fn transact(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let (tx, rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::Transact(body.to_vec(), pump_ms, tx))
            .map_err(|_| "HID thread gone".to_string())?;
        rx.recv()
            .map_err(|_| "HID thread dropped reply".to_string())?
    }

    fn transact_chunked(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let (tx, rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::TransactChunked(body.to_vec(), pump_ms, tx))
            .map_err(|_| "HID thread gone".to_string())?;
        rx.recv()
            .map_err(|_| "HID thread dropped reply".to_string())?
    }

    fn pump(&self, pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let (tx, rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::Pump(pump_ms, tx))
            .map_err(|_| "HID thread gone".to_string())?;
        rx.recv()
            .map_err(|_| "HID thread dropped reply".to_string())?
    }

    fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let (tx, rx) = bounded(1);
        self.cmd_tx
            .send(Cmd::TransactEager(body.to_vec(), max_ms, tx))
            .map_err(|_| "HID thread gone".to_string())?;
        rx.recv()
            .map_err(|_| "HID thread dropped reply".to_string())?
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation_sys::base::{kCFAllocatorDefault, CFIndex, CFRelease, CFRetain, CFTypeRef};
    use core_foundation_sys::dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreateMutable,
        CFDictionaryRef, CFDictionarySetValue, CFMutableDictionaryRef,
    };
    use core_foundation_sys::number::{kCFNumberSInt32Type, CFNumberCreate};
    use core_foundation_sys::runloop::{
        kCFRunLoopDefaultMode, CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRunInMode,
    };
    use core_foundation_sys::set::{CFSetGetCount, CFSetGetValues, CFSetRef};
    use core_foundation_sys::string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringRef,
    };
    use crossbeam_channel::unbounded;
    use std::os::raw::{c_char, c_void};

    type IOHIDManagerRef = *mut c_void;
    type IOHIDDeviceRef = *mut c_void;
    type IOReturn = i32;

    type IOHIDReportCallback = extern "C" fn(
        context: *mut c_void,
        result: IOReturn,
        sender: *mut c_void,
        report_type: u32,
        report_id: u32,
        report: *mut u8,
        report_length: CFIndex,
    );

    const K_IOHID_OPTIONS_TYPE_SEIZE_DEVICE: u32 = 0x01;
    const K_IOHID_REPORT_TYPE_OUTPUT: u32 = 1;
    // Common IOReturn codes when the device can't be seized.
    const K_IORETURN_EXCLUSIVE_ACCESS: IOReturn = 0xe000_02c5u32 as i32;
    const K_IORETURN_NOT_PERMITTED: IOReturn = 0xe000_02e2u32 as i32;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDManagerCreate(allocator: CFTypeRef, options: u32) -> IOHIDManagerRef;
        fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
        fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
        fn IOHIDDeviceOpen(device: IOHIDDeviceRef, options: u32) -> IOReturn;
        fn IOHIDDeviceClose(device: IOHIDDeviceRef, options: u32) -> IOReturn;
        fn IOHIDDeviceSetReport(
            device: IOHIDDeviceRef,
            report_type: u32,
            report_id: CFIndex,
            report: *const u8,
            report_length: CFIndex,
        ) -> IOReturn;
        fn IOHIDDeviceScheduleWithRunLoop(
            device: IOHIDDeviceRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
        fn IOHIDDeviceUnscheduleFromRunLoop(
            device: IOHIDDeviceRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
        fn IOHIDDeviceRegisterInputReportCallback(
            device: IOHIDDeviceRef,
            report: *mut u8,
            report_length: CFIndex,
            callback: IOHIDReportCallback,
            context: *mut c_void,
        );
    }

    /// Input-report callback. Runs on the HID thread during a pump; pushes a
    /// copy of each report into the `Vec<Vec<u8>>` pointed to by `context`.
    extern "C" fn input_cb(
        context: *mut c_void,
        _result: IOReturn,
        _sender: *mut c_void,
        _report_type: u32,
        _report_id: u32,
        report: *mut u8,
        report_length: CFIndex,
    ) {
        if context.is_null() || report.is_null() || report_length <= 0 {
            return;
        }
        let recv = unsafe { &mut *(context as *mut Vec<Vec<u8>>) };
        let n = (report_length as usize).min(64);
        let slice = unsafe { std::slice::from_raw_parts(report, n) };
        recv.push(slice.to_vec());
    }

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

    /// Locate + open the TMP (seized). Returns `(manager, device)` — BOTH must
    /// stay alive for the device's whole life: the device is owned by the
    /// manager's internal device set, so releasing the manager (or the copied
    /// device set) before use frees the device. We additionally `CFRetain` the
    /// device and reclaim both at thread exit. (The earlier crash was exactly
    /// this use-after-free.) All calls here run on the HID worker thread.
    ///
    /// On persistent `kIOReturnExclusiveAccess` this re-ENUMERATES (fresh
    /// manager + device copy) between attempts instead of only re-opening:
    /// after some session closes the device object can go stale/re-register,
    /// and re-opening the stale ref fails 0xe00002c5 FOREVER while a fresh
    /// enumeration succeeds immediately (HW-observed: the second of
    /// two back-to-back probe sessions failed through 5 s of same-ref retries,
    /// yet a fresh process connected instantly). Genuine Pro Control contention
    /// persists across every attempt and still surfaces the error (~5 s).
    unsafe fn open_device() -> Result<(IOHIDManagerRef, IOHIDDeviceRef), String> {
        // LONG quiet backoffs, few attempts: hammering open every ~700 ms NEVER
        // recovered a locked-out device across hundreds of HW retries
        // — each failed seize attempt appears to RESET the device's lockout
        // window, so rapid retries are self-defeating. ~8 s of true quiet lets
        // the lockout expire.
        const ENUM_RETRIES: u32 = 3;
        const ENUM_RETRY_DELAY_MS: u64 = 8000;
        let mut last_err = String::new();
        for attempt in 0..=ENUM_RETRIES {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ENUM_RETRY_DELAY_MS));
            }
            match open_device_once() {
                Ok(pair) => {
                    if attempt > 0 {
                        log::warn!("HID open succeeded on re-enumeration attempt {attempt}");
                    }
                    return Ok(pair);
                }
                Err(e) => {
                    let transient = e.contains("0xe00002c5");
                    last_err = e;
                    if !transient {
                        break;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// One enumeration + open attempt (with the short same-ref retry for the
    /// common few-ms seize-recycle lag). See `open_device` for the outer
    /// re-enumeration loop.
    unsafe fn open_device_once() -> Result<(IOHIDManagerRef, IOHIDDeviceRef), String> {
        let manager = IOHIDManagerCreate(kCFAllocatorDefault as CFTypeRef, 0);
        if manager.is_null() {
            return Err("IOHIDManagerCreate returned null".into());
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

        let devices = IOHIDManagerCopyDevices(manager);
        // dict/keys/values no longer needed.
        CFRelease(vid_key as CFTypeRef);
        CFRelease(pid_key as CFTypeRef);
        CFRelease(vid_val as CFTypeRef);
        CFRelease(pid_val as CFTypeRef);
        CFRelease(dict as CFTypeRef);

        if devices.is_null() {
            CFRelease(manager as CFTypeRef);
            return Err("no TMP found (VID 0x1ED8 / PID 0x44) — is it plugged in?".into());
        }
        let count = CFSetGetCount(devices);
        if count == 0 {
            CFRelease(devices as CFTypeRef);
            CFRelease(manager as CFTypeRef);
            return Err("no TMP found (VID 0x1ED8 / PID 0x44) — is it plugged in?".into());
        }
        let mut values: Vec<*const c_void> = vec![std::ptr::null(); count as usize];
        CFSetGetValues(devices, values.as_mut_ptr());
        let device = values[0] as IOHIDDeviceRef;
        // Retain the device so it outlives the copied set we're about to drop.
        CFRetain(device as CFTypeRef);
        CFRelease(devices as CFTypeRef);

        // Open + seize, retrying briefly on kIOReturnExclusiveAccess (0xe00002c5).
        // That code fires both when Pro Control genuinely holds the device AND on a
        // transient SELF-collision WITHIN one operation: `with_released_seize` drops
        // the app's old seize and immediately reconnects, but the kernel can lag a
        // few ms recycling the exclusive lock after the prior handle closed (the Hid
        // Drop blocks on the worker-thread join + IOHIDDeviceClose, but the
        // kernel-side release isn't instantaneous). The device-op gate
        // (`DEVICE_OP_LOCK`) serializes whole operations so two of them can't
        // overlap; this short bounded retry covers the remaining intra-operation
        // drop→reconnect lag (≤7 opens / ≤6 sleeps ≈ 0.48 s). The stale-ref case
        // needs re-enumeration, not more same-ref retries — see `open_device`.
        // A failed open holds nothing, so re-opening needs no cleanup between.
        const OPEN_RETRY_RETRIES: u32 = 6;
        const OPEN_RETRY_DELAY_MS: u64 = 80;
        let mut rc = IOHIDDeviceOpen(device, K_IOHID_OPTIONS_TYPE_SEIZE_DEVICE);
        for _ in 0..OPEN_RETRY_RETRIES {
            if rc != K_IORETURN_EXCLUSIVE_ACCESS {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(OPEN_RETRY_DELAY_MS));
            rc = IOHIDDeviceOpen(device, K_IOHID_OPTIONS_TYPE_SEIZE_DEVICE);
        }
        if rc != 0 {
            CFRelease(device as CFTypeRef);
            CFRelease(manager as CFTypeRef);
            let hint = if rc == K_IORETURN_EXCLUSIVE_ACCESS || rc == K_IORETURN_NOT_PERMITTED {
                " — close Fender Pro Control (it holds the device) and retry"
            } else {
                ""
            };
            return Err(format!("IOHIDDeviceOpen failed: 0x{:08x}{hint}", rc as u32));
        }
        // Keep `manager` alive (returned to caller); it backs the device.
        Ok((manager, device))
    }

    pub fn open() -> Result<Hid, String> {
        // Worker reports the open result back through this oneshot.
        let (ready_tx, ready_rx) = bounded::<Result<(), String>>(1);
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();

        let join = std::thread::Builder::new()
            .name("tmp-hid".into())
            .spawn(move || unsafe {
                // Receive buffer lives for the whole thread; raw ptr handed to
                // the C callback as context. Reclaimed before the thread exits.
                let recv_ptr: *mut Vec<Vec<u8>> = Box::into_raw(Box::new(Vec::new()));
                // Persistent 64-byte report buffer the callback writes into.
                let mut report_buf = vec![0u8; 64];

                let (manager, device) = match open_device() {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        drop(Box::from_raw(recv_ptr));
                        return;
                    }
                };

                let run_loop: CFRunLoopRef = CFRunLoopGetCurrent();
                IOHIDDeviceScheduleWithRunLoop(device, run_loop, kCFRunLoopDefaultMode);
                IOHIDDeviceRegisterInputReportCallback(
                    device,
                    report_buf.as_mut_ptr(),
                    report_buf.len() as CFIndex,
                    input_cb,
                    recv_ptr as *mut c_void,
                );
                let _ = ready_tx.send(Ok(()));

                // Run the run loop for `seconds`, letting input callbacks fire.
                // (The enclosing `unsafe` block extends into these closures.)
                let pump = |seconds: f64| {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, false as u8);
                };
                // Take + clear the raw reports received so far (no reassembly —
                // the caller accumulates and reassembles across pumps).
                let drain = || -> Vec<Vec<u8>> {
                    let recv = &mut *recv_ptr;
                    std::mem::take(recv)
                };
                let set_report_raw = |pkt: &[u8]| -> Result<(), String> {
                    let rc = IOHIDDeviceSetReport(
                        device,
                        K_IOHID_REPORT_TYPE_OUTPUT,
                        0,
                        pkt.as_ptr(),
                        pkt.len() as CFIndex,
                    );
                    if rc != 0 {
                        Err(format!("IOHIDDeviceSetReport failed: 0x{:08x}", rc as u32))
                    } else {
                        Ok(())
                    }
                };
                // ALL outbound sends chunk when needed: a body ≤ MAX_BODY is one 0x35 frame
                // (identical to the old `make_envelope`), a larger one splits into the
                // 0x33/0x34/0x35 multi-packet frames the device accepts (symmetric with
                // inbound). Without this a long-FenderId `changeParameter` (e.g. a 40-char
                // reverb+cab amp id + "outputLevel") overflowed a single report and PANICKED.
                let set_report = |body: &[u8]| -> Result<(), String> {
                    for pkt in proto::make_chunked_envelopes(body) {
                        set_report_raw(&pkt)?;
                    }
                    Ok(())
                };

                for cmd in cmd_rx.iter() {
                    match cmd {
                        Cmd::Send(body, reply) => {
                            let _ = reply.send(set_report(&body));
                        }
                        Cmd::Transact(body, ms, reply) => match set_report(&body) {
                            Ok(()) => {
                                pump(ms as f64 / 1000.0);
                                let _ = reply.send(Ok(drain()));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        },
                        Cmd::Pump(ms, reply) => {
                            pump(ms as f64 / 1000.0);
                            let _ = reply.send(Ok(drain()));
                        }
                        Cmd::TransactChunked(body, ms, reply) => match set_report(&body) {
                            Ok(()) => {
                                pump(ms as f64 / 1000.0);
                                let _ = reply.send(Ok(drain()));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        },
                        Cmd::TransactEager(body, max_ms, reply) => match set_report(&body) {
                            Ok(()) => {
                                // Pump in short slices; exit early once at least one
                                // report arrived, no multi-packet stream is open, and
                                // the line stayed quiet for EAGER_QUIET_MS. `max_ms`
                                // is the hard cap (the old fixed-window behavior).
                                let mut elapsed = 0u64;
                                let mut open = false;
                                let mut scanned = 0usize;
                                let mut quiet = 0u64;
                                while elapsed < max_ms {
                                    pump(EAGER_SLICE_MS as f64 / 1000.0);
                                    elapsed += EAGER_SLICE_MS;
                                    let recv = &*recv_ptr;
                                    if recv.len() > scanned {
                                        open = fold_frame_open(&recv[scanned..], open);
                                        scanned = recv.len();
                                        quiet = 0;
                                    } else {
                                        quiet += EAGER_SLICE_MS;
                                    }
                                    if scanned > 0 && !open && quiet >= EAGER_QUIET_MS {
                                        break;
                                    }
                                }
                                let _ = reply.send(Ok(drain()));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        },
                        Cmd::Close => break,
                    }
                }

                // Tear down in spec order: unschedule, one short run-loop turn,
                // close. NOTE — do NOT add a "drain until quiet" pump here: on a
                // live-cadence (heartbeat) session, pumping ~1.5 s without
                // heartbeats before close reliably WEDGED the device's next
                // exclusive open (HW A/B: identical sequences opened
                // clean 4/4 without the drain, 0xe00002c5 with it).
                IOHIDDeviceUnscheduleFromRunLoop(device, run_loop, kCFRunLoopDefaultMode);
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, false as u8);
                let close_rc = IOHIDDeviceClose(device, K_IOHID_OPTIONS_TYPE_SEIZE_DEVICE);
                if close_rc != 0 {
                    log::warn!("IOHIDDeviceClose failed: 0x{:08x}", close_rc as u32);
                }
                CFRelease(device as CFTypeRef);
                CFRelease(manager as CFTypeRef);
                drop(Box::from_raw(recv_ptr));
            })
            .map_err(|e| format!("spawn HID thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Hid {
                cmd_tx,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => Err("HID thread exited before reporting status".into()),
        }
    }
}

/// Linux hidraw transport.
///
/// Why hidraw and not the `hidapi` crate: hidraw needs no C dependency and no udev
/// crate — the device node is found by walking sysfs (see [`parse_hid_id`]) — which
/// keeps this a zero-new-dependency port.
///
/// There is no seize here, and none is needed. `kIOHIDOptionsTypeSeizeDevice` exists
/// on macOS to lock Pro Control out of the device; Fender ships no Pro Control for
/// Linux, so nothing competes for the unit. The macOS open-retry tuning (the
/// `0xe00002c5` lockout, the 8 s quiet backoff) describes IOKit exclusive-open
/// behaviour and is deliberately NOT copied: there is no evidence the same lockout
/// exists here, and inventing retries for an unobserved failure mode would just make
/// a real error slow to surface.
///
/// Threading mirrors the macOS side so [`Hid`] behaves identically: one worker thread
/// owns the fd for its whole life and other threads issue commands over a channel.
/// The blocking `poll`/`read` loop replaces `CFRunLoopRunInMode` and is a good deal
/// simpler — no run loop, no C callback, no shared receive buffer.
///
/// # Inbound framing needs no fixup (measured, fw 1.8.58)
///
/// Inbound frames are parsed as `[0x00, magic, _, body_len, …]` while OUTBOUND frames
/// put the magic at index 0 (`proto::make_envelope`). That asymmetry looks like a
/// report-ID artifact of the reading API, which would differ per HID backend and force
/// this module to re-add or strip a byte.
///
/// It is not. Every inbound report reads as **64 bytes beginning with a literal
/// `0x00`** — the leading byte is part of the device's report, not something IOKit
/// prepends. Confirmed on hardware over a full preset-list harvest: 668/668 reports
/// `n=64`, first bytes `00 35 …` / `00 33 …` / `00 34 …`, and zero in the bare
/// `[magic, …]` shape.
///
/// So reports go up RAW, exactly as the macOS input callback pushes them, and
/// `proto.rs` plus its golden vectors stay byte-identical across platforms. Reports
/// that are not frames need no filtering here either — `proto::reassemble_streams`
/// already skips anything failing `len >= 4 && data[0] == 0x00`.
#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::ffi::CString;
    use std::io::Error as IoError;
    use std::os::raw::c_void;
    use std::time::{Duration, Instant};

    /// Locate the TMP's `/dev/hidraw*` node via sysfs.
    fn find_hidraw() -> Result<CString, String> {
        let dir = std::fs::read_dir("/sys/class/hidraw")
            .map_err(|e| format!("cannot list /sys/class/hidraw ({e}) — is hidraw available?"))?;
        let mut seen = 0usize;
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("hidraw") {
                continue;
            }
            seen += 1;
            let Ok(body) = std::fs::read_to_string(entry.path().join("device/uevent")) else {
                continue;
            };
            if parse_hid_id(&body) == Some((VID, PID)) {
                return CString::new(format!("/dev/{name}")).map_err(|e| e.to_string());
            }
        }
        Err(format!(
            "no TMP found (VID 0x{VID:04X} / PID 0x{PID:02X}) among {seen} HID device(s) — is it plugged in?"
        ))
    }

    fn open_device() -> Result<i32, String> {
        let path = find_hidraw()?;
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            let err = IoError::last_os_error();
            // EACCES is the overwhelmingly common first-run failure: the node defaults
            // to root-only. Name the fix rather than surfacing a bare errno.
            let hint = if err.raw_os_error() == Some(libc::EACCES) {
                " — no permission for the hidraw node; install the udev rule (see CONTRIBUTING.md), then unplug and replug the unit"
            } else {
                ""
            };
            return Err(format!("open {}: {err}{hint}", path.to_string_lossy()));
        }

        // hidraw has no equivalent of IOKit's exclusive seize: two processes can open
        // the same node and interleave reads and writes on it. Verified — two `probe`
        // runs opened this device concurrently and both proceeded, where the second
        // would have failed 0xe00002c5 on macOS. The rest of the codebase assumes that
        // cannot happen (the write-safety rules in particular), so restore the
        // invariant with an advisory whole-file lock: every path into the device goes
        // through here, so our own processes mutually exclude. It is advisory, which is
        // enough — Fender ships no Pro Control for Linux, so nothing else contends.
        // The lock releases on close, including on abnormal exit.
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            let err = IoError::last_os_error();
            unsafe { libc::close(fd) };
            if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Err(
                    "the Tone Master Pro is already in use by another TMP Companion process (app or probe) — close it and retry"
                        .into(),
                );
            }
            return Err(format!("lock {}: {err}", path.to_string_lossy()));
        }
        Ok(fd)
    }

    /// hidraw takes the report ID as the first byte written. The TMP uses unnumbered
    /// reports, so that byte is `0x00` and the device receives exactly the 63-byte
    /// frame — the same bytes IOKit puts on the wire via `SetReport(reportID = 0)`.
    fn write_frame(fd: i32, frame: &[u8]) -> Result<(), String> {
        let mut pkt = Vec::with_capacity(frame.len() + 1);
        pkt.push(0x00);
        pkt.extend_from_slice(frame);
        let n = unsafe { libc::write(fd, pkt.as_ptr() as *const c_void, pkt.len()) };
        if n < 0 {
            return Err(format!("hidraw write: {}", IoError::last_os_error()));
        }
        if n as usize != pkt.len() {
            return Err(format!("hidraw short write: {n} of {}", pkt.len()));
        }
        Ok(())
    }

    fn send_body(fd: i32, body: &[u8]) -> Result<(), String> {
        for pkt in proto::make_chunked_envelopes(body) {
            write_frame(fd, &pkt)?;
        }
        Ok(())
    }

    /// Collect reports for up to `ms`, then return — the same "burn the budget" shape
    /// as `CFRunLoopRunInMode`, so callers reassemble cumulatively across pumps.
    fn pump_into(fd: i32, ms: u64, out: &mut Vec<Vec<u8>>) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let rc =
                unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(i32::MAX as u128) as i32) };
            if rc < 0 {
                // A signal is not a failure; anything else ends the pump.
                if IoError::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            if rc == 0 || pfd.revents & libc::POLLIN == 0 {
                return; // budget expired, or POLLERR/POLLHUP — the device is gone
            }
            let mut buf = [0u8; 64];
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
            if n <= 0 {
                return;
            }
            // Pushed RAW, exactly as the macOS input callback does — see the module
            // note on inbound framing for why no report-id fixup is needed here.
            #[allow(clippy::cast_sign_loss)] // n > 0 checked immediately above
            out.push(buf[..n as usize].to_vec());
        }
    }

    fn pump(fd: i32, ms: u64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        pump_into(fd, ms, &mut out);
        out
    }

    /// [`Cmd::TransactEager`]: pump in slices and return once the framing is complete
    /// and the line has gone quiet. Identical policy to the macOS path — it reuses the
    /// same pure [`fold_frame_open`] — with `max_ms` as the hard cap.
    fn pump_eager(fd: i32, max_ms: u64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut elapsed = 0u64;
        let mut open = false;
        let mut scanned = 0usize;
        let mut quiet = 0u64;
        while elapsed < max_ms {
            let slice = EAGER_SLICE_MS.min(max_ms - elapsed);
            pump_into(fd, slice, &mut out);
            elapsed += slice;
            if out.len() > scanned {
                open = fold_frame_open(&out[scanned..], open);
                scanned = out.len();
                quiet = 0;
            } else {
                quiet += slice;
            }
            if scanned > 0 && !open && quiet >= EAGER_QUIET_MS {
                break;
            }
        }
        out
    }

    pub fn open() -> Result<Hid, String> {
        let (ready_tx, ready_rx) = bounded::<Result<(), String>>(1);
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();

        let join = std::thread::Builder::new()
            .name("tmp-hid".into())
            .spawn(move || {
                let fd = match open_device() {
                    Ok(fd) => fd,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));

                for cmd in cmd_rx.iter() {
                    match cmd {
                        Cmd::Send(body, reply) => {
                            let _ = reply.send(send_body(fd, &body));
                        }
                        // Chunking is inside send_body, so plain and chunked transacts
                        // are the same operation here (as on macOS).
                        Cmd::Transact(body, ms, reply) | Cmd::TransactChunked(body, ms, reply) => {
                            match send_body(fd, &body) {
                                Ok(()) => {
                                    let _ = reply.send(Ok(pump(fd, ms)));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e));
                                }
                            }
                        }
                        Cmd::Pump(ms, reply) => {
                            let _ = reply.send(Ok(pump(fd, ms)));
                        }
                        Cmd::TransactEager(body, max_ms, reply) => match send_body(fd, &body) {
                            Ok(()) => {
                                let _ = reply.send(Ok(pump_eager(fd, max_ms)));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        },
                        Cmd::Close => break,
                    }
                }

                if unsafe { libc::close(fd) } < 0 {
                    log::warn!("hidraw close failed: {}", IoError::last_os_error());
                }
            })
            .map_err(|e| format!("spawn HID thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Hid {
                cmd_tx,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => Err("HID thread exited before reporting status".into()),
        }
    }

    #[cfg(test)]
    mod tests {
        // A child module, not a sibling of the outer `tests`: `pump_into` is private to
        // `imp`, and only a child can reach it. That access is the whole point — the
        // property under test belongs to the transport, not to a helper beside it.
        use super::*;

        /// Pins the measured hidraw wire shape (see the Linux `imp` module note): a real
        /// inbound report is 64 bytes led by a literal `0x00`, and reaches the parsers
        /// RAW. If a future change ever re-adds a report-id fixup, this is the assertion
        /// that should have stopped it.
        ///
        /// Driven through the real [`pump_into`] over a pipe rather than over a buffer
        /// built in the test: a fixup would live inside `pump_into`, so a test that
        /// constructs its own report and inspects it can never observe one.
        #[test]
        fn a_real_inbound_report_needs_no_report_id_fixup() {
            // Byte-for-byte head of a report captured from a TMP on fw 1.8.58 (the 0x35
            // single-frame reply that opens a preset-list harvest), zero-padded to 64.
            let mut raw = vec![0u8; 64];
            raw[..4].copy_from_slice(&[0x00, 0x35, 0x00, 0x06]);

            // A pipe is fd-compatible with the poll/read loop and needs no hardware.
            let mut fds = [0i32; 2];
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe()");
            let (rd, wr) = (fds[0], fds[1]);
            let written = unsafe { libc::write(wr, raw.as_ptr() as *const c_void, raw.len()) };
            assert_eq!(written, 64, "write()");
            // Close the write end so the pump sees EOF instead of burning its budget.
            assert_eq!(unsafe { libc::close(wr) }, 0, "close(wr)");

            let mut out = Vec::new();
            pump_into(rd, 250, &mut out);
            assert_eq!(unsafe { libc::close(rd) }, 0, "close(rd)");

            // Exactly one report, byte-identical to the wire: no byte stripped, none
            // inserted, no re-framing.
            assert_eq!(out, vec![raw.clone()]);
            // And what came out folds as a CLOSED single frame, so the parsers' own
            // admission test (`len >= 4 && data[0] == 0x00`) passes on it as-is.
            assert!(!fold_frame_open(&out, false));
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::*;
    pub fn open() -> Result<Hid, String> {
        Err("TMP Companion supports macOS (IOKit HID) and Linux (hidraw)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic 64-byte input report: `0x00` report id, the magic, a
    /// dummy byte, a body length, then padding — the shape `fold_frame_open`
    /// (and `proto::reassemble_streams*`) sees from IOKit.
    fn report(magic: u8) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[1] = magic;
        r[3] = 4; // body_len (irrelevant to the fold)
        r
    }

    #[test]
    fn hid_id_parses_the_kernels_zero_padded_hex() {
        // Real shape from /sys/class/hidraw/hidrawN/device/uevent — the TMP.
        let tmp = "DRIVER=hid-generic\nHID_ID=0003:00001ED8:00000044\nHID_NAME=Tone Master Pro\n";
        assert_eq!(parse_hid_id(tmp), Some((VID, PID)));
        // A different device must not match (this is the whole point of the lookup).
        let mouse = "HID_ID=0003:0000046D:0000C077\nHID_NAME=Logitech USB Optical Mouse\n";
        assert_eq!(parse_hid_id(mouse), Some((0x046D, 0xC077)));
        assert_ne!(parse_hid_id(mouse), Some((VID, PID)));
        // Absent / malformed lines yield None rather than a bogus id.
        assert_eq!(parse_hid_id("DRIVER=hid-generic\n"), None);
        assert_eq!(parse_hid_id("HID_ID=0003:notahex:00000044"), None);
        assert_eq!(parse_hid_id("HID_ID=0003:00001ED8"), None);
    }

    #[test]
    fn fold_tracks_the_0x35_is_final_rule() {
        // Single-frame response (0x35 only): complete immediately.
        assert!(!fold_frame_open(&[report(0x35)], false));
        // Multi-packet stream: 0x33 opens, 0x34 continues (still open), 0x35 closes.
        assert!(fold_frame_open(&[report(0x33)], false));
        assert!(fold_frame_open(&[report(0x33), report(0x34)], false));
        assert!(!fold_frame_open(
            &[report(0x33), report(0x34), report(0x35)],
            false
        ));
        // State carries across batches (a stream can span pump slices).
        let open = fold_frame_open(&[report(0x33)], false);
        assert!(!fold_frame_open(&[report(0x35)], open));
    }

    #[test]
    fn fold_ignores_malformed_and_foreign_reports() {
        // Short report / wrong report id: skipped, state unchanged.
        assert!(fold_frame_open(&[vec![0x00, 0x35]], true)); // len < 4
        let mut bad_id = report(0x35);
        bad_id[0] = 0x01;
        assert!(fold_frame_open(&[bad_id], true));
        // Unknown magic: skipped.
        assert!(fold_frame_open(&[report(0x99)], true));
    }
}
