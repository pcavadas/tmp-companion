//! Device trace recorder (TCT v1): a lossless record of every HID call, settle, re-amp
//! capture and seed field-8 read a process makes, for later replay and analysis.
//!
//! OFF BY DEFAULT AND INERT. Nothing here runs unless `TMP_TRACE_DIR` is set: no
//! thread, no allocation, no file. [`recorder`] reads the environment once; every entry
//! point checks it first. It is compiled in every build (like `validate_log`), so a
//! plain-build probe can record a calibration session too.
//!
//! WHAT ARMS IT. `scripts/e2e.sh` exports these to the seed probe and the e2e server of
//! an online or soak run; any script or probe arms it the same way:
//!
//! | Variable | Meaning |
//! | --- | --- |
//! | `TMP_TRACE_DIR` | the run's `trace/` directory; `wav/` and `field8/` go next to it |
//! | `TMP_TRACE_PROCESS` | the process name in the file name (default: the executable's) |
//! | `TMP_TRACE_RUN_ID`, `TMP_TRACE_GIT_SHA`, `TMP_TRACE_TREE_KEY` | copied into the header |
//! | `TMP_E2E_UNIT_CONFIG` | `stock` or `non-stock`, copied into the header |
//! | `TMP_TRACE_WAV=pair` | write each capture's processed pair as a WAV |
//! | `TMP_TRACE_FW_FILE` | where a device session's firmware version is written |
//!
//! THE SEAMS. [`open`] is the one hook in `session::open_transport`, so every session
//! (command lane, monitor lane, probe) is recorded below `TrackedHid` and its
//! auto-reopens are on the wire record too. [`settle`] is called from
//! `device_gate::settle_ms`; [`capture`] wraps `audio::reamp_capture`; [`record_field8`]
//! follows the seed's field-8 reads; [`record_cmd`] is for the e2e server's `/invoke`
//! route. The event table and file layout are in `notes/e2e-test-plan.md` ("Recording").
//!
//! PRIVACY. A trace holds the owner's preset names and bodies, and captures are the
//! device's DSP output. Traces, WAVs and field-8 files stay on the machine that made
//! them: never commit them, attach them to an issue or PR, or upload them as a CI
//! artifact.

mod capture;
mod format;
mod record;
mod state;

use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub(crate) use capture::capture_ctx;
#[allow(unused_imports)] // the replay normalizer's shared entry points
pub(crate) use format::read_trace;
pub(crate) use record::Backend;
#[allow(unused_imports)]
pub(crate) use state::canonical_json;

use crate::hid::HidTransport;
use record::{Config, Recorder};

static RECORDER: OnceLock<Option<Arc<Recorder>>> = OnceLock::new();

/// This process's recorder, started on first use when `TMP_TRACE_DIR` is set.
fn recorder() -> Option<&'static Arc<Recorder>> {
    RECORDER
        .get_or_init(|| {
            let cfg = Config::from_vars(|k| std::env::var(k).ok())?;
            match Recorder::start(cfg) {
                Ok(r) => Some(r),
                Err(e) => {
                    log::warn!("trace recorder not started: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// Open a device transport through `f`, recording the open and, when it succeeds, every
/// call on the transport. `backend` names the branch that ran. Returns `f`'s transport
/// unwrapped when the recorder is off.
pub(crate) fn open(
    backend: Backend,
    f: impl FnOnce() -> Result<Box<dyn HidTransport>, String>,
) -> Result<Box<dyn HidTransport>, String> {
    match recorder() {
        Some(r) => r.open(backend, f),
        None => f(),
    }
}

/// A settle: what the caller asked for and what was applied.
pub(crate) fn settle(nominal_ms: u64, applied_ms: u64) {
    if let Some(r) = recorder() {
        r.settle(nominal_ms, applied_ms);
    }
}

/// Run a re-amp capture through `f` and record it. The writer meters it; this thread
/// only clones the stimulus and the samples.
pub(crate) fn capture(
    backend: Backend,
    stimulus: &[f32],
    rate: u32,
    tail_ms: u64,
    f: impl FnOnce() -> Result<crate::audio::Capture, String>,
) -> Result<crate::audio::Capture, String> {
    let Some(r) = recorder() else {
        return f();
    };
    let t_start = Instant::now();
    let result = f();
    let t_end = Instant::now();
    let copy = match &result {
        Ok(c) => Ok(crate::audio::Capture {
            interleaved: c.interleaved.clone(),
            channels: c.channels,
            sample_rate: c.sample_rate,
        }),
        Err(e) => Err(e.clone()),
    };
    r.capture(capture::CaptureJob {
        backend: backend.as_str(),
        t_start,
        t_end,
        stimulus: stimulus.to_vec(),
        rate,
        tail_ms,
        result: copy,
        ctx: capture::current_ctx(),
    });
    result
}

/// A field-8 body the seed read for `list_index`, with its pristine verdict (`ok`, the
/// reason it is not, or `None` when not checked). Record only fixture bodies.
pub(crate) fn record_field8(list_index: u32, body: &[u8], pristine: Option<&str>) {
    if let Some(r) = recorder() {
        r.field8(list_index, body, pristine.map(str::to_string));
    }
}

/// Whether this process is recording, for a caller that must build something (a
/// serialized argument list) only to record it.
#[allow(dead_code)] // only the e2e server's /invoke route asks
pub(crate) fn enabled() -> bool {
    recorder().is_some()
}

/// A command the e2e server ran: its name, a hash of its arguments, and the outcome.
#[allow(dead_code)] // only the e2e server's /invoke route records commands
pub(crate) fn record_cmd(name: &str, args: &[u8], ok: bool, ms: u64) {
    if let Some(r) = recorder() {
        r.cmd(name, args, ok, ms);
    }
}

#[cfg(test)]
mod tests;
