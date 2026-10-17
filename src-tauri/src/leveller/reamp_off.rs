//! The guaranteed re-amp OFF (`danger.md`: every run ends with re-amp OFF on a fresh
//! connection): the OFF ladder, its outcome and warning, and [`ReampRunGuard`], the app's
//! run-end backstop around every device command.

use super::*;

/// The run-result text when [`reamp_off_guaranteed`] cannot read the Mixer store back OFF —
/// for the log and the bug report; the frontend reads the typed flag
/// ([`ReampOffOutcome::still_on`], `reamp_still_on` on the wire), never this text. Re-amp
/// replaces Inputs 1 and 2, so the guitar goes unheard until it is off. A USB unplug/replug
/// does NOT clear it (nothing resets re-amp when a connection closes); a restart does (the
/// firmware forces it off at boot) — static evidence, fw 1.8.58: `notes/gotchas.md`.
pub(crate) const REAMP_STILL_ON_WARNING: &str =
    "Re-amp may still be on. Restart your Tone Master Pro.";

/// How a [`reamp_off_guaranteed`] run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReampOffOutcome {
    /// The Mixer store (the audio path, [`Session::read_reamp_state`]) read back OFF.
    /// `false` covers both a store still ON and a store that never answered.
    pub(crate) verified: bool,
}

impl ReampOffOutcome {
    /// Re-amp may still be on: the OFF never read back OFF. The typed signal every leveling
    /// result carries (`reamp_still_on`), beside the [`Self::warning`] text.
    pub(crate) fn still_on(self) -> bool {
        !self.verified
    }

    /// The run-result warning: [`REAMP_STILL_ON_WARNING`] unless the OFF was verified.
    pub(crate) fn warning(self) -> Option<String> {
        self.still_on().then(|| REAMP_STILL_ON_WARNING.to_string())
    }

    /// `e` with the warning appended, for a run that failed AND left re-amp unverified. A
    /// cancel is returned untouched: it stays a cancel, with its own text.
    pub(crate) fn annotate_err(self, e: LevelError) -> LevelError {
        match self.warning() {
            Some(w) if !e.is_cancelled() => LevelError::Device(format!("{e} — {w}")),
            _ => e,
        }
    }

    /// The run-end step of a leveling command, with a TYPED error: `rows` gets this outcome
    /// (each row records [`Self::still_on`] and the warning), and a failure becomes an
    /// [`AppError::Leveling`] carrying the failure's own [`SkipKind`] — or
    /// [`SkipKind::ConnectionLost`] when the unit was unplugged since `watch` started — the
    /// text with the warning appended, and the typed `reamp_still_on`.
    pub(crate) fn stamp_leveling<T, E: Into<LevelError>>(
        self,
        result: Result<T, E>,
        watch: crate::watcher::DetachWatch,
        rows: impl FnOnce(&mut T, ReampOffOutcome),
    ) -> Result<T, AppError> {
        match result {
            Ok(mut r) => {
                rows(&mut r, self);
                Ok(r)
            }
            Err(e) => {
                let e = e.into();
                let kind = watch.refine(e.skip_kind());
                Err(AppError::Leveling {
                    kind,
                    message: self.annotate_err(e).to_string(),
                    reamp_still_on: self.still_on(),
                })
            }
        }
    }

    /// [`Self::stamp_leveling`] for a BATCHED command: the rows come back inside a
    /// [`BatchLevelRun`] that carries this outcome whatever the row count — none at all after
    /// a Stop in the ceiling prepass or when every row failed — and each row is stamped too
    /// (`stamp_row`).
    pub(crate) fn stamp_batch<R, E: Into<LevelError>>(
        self,
        result: Result<Vec<R>, E>,
        watch: crate::watcher::DetachWatch,
        stamp_row: impl Fn(&mut R, ReampOffOutcome),
    ) -> Result<BatchLevelRun<R>, AppError> {
        self.stamp_leveling(result.map(BatchLevelRun::new), watch, |run, out| {
            run.rows.iter_mut().for_each(|r| stamp_row(r, out));
            run.reamp_still_on = out.still_on();
            run.reamp_warning = out.warning();
        })
    }
}

/// What a batched leveling command returns (`level_scenes_apply_batched`,
/// `level_footswitches_apply`): the rows it leveled, and the run-end re-amp verdict BESIDE
/// them. The verdict is a fact about the run, not about a row, and a batch can end with no
/// rows at all — a Stop during the ceiling prepass (which engages re-amp once per sound), or a
/// batch whose every row failed — so it must not ride the rows alone: an OFF that never read
/// back OFF reaches the Summary on every path (`danger.md`). Each row carries it as well.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchLevelRun<T> {
    /// The leveled rows; a row that failed or never ran is not here (its streamed item said
    /// why).
    pub(crate) rows: Vec<T>,
    /// Re-amp may still be on ([`ReampOffOutcome::still_on`]): what the Summary reads.
    pub(crate) reamp_still_on: bool,
    /// Its text twin ([`ReampOffOutcome::warning`]), for the log. Omitted when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reamp_warning: Option<String>,
}

impl<T> BatchLevelRun<T> {
    /// `rows`, with no verdict stamped yet ([`ReampOffOutcome::stamp_batch`] stamps it).
    pub(crate) fn new(rows: Vec<T>) -> Self {
        Self {
            rows,
            reamp_still_on: false,
            reamp_warning: None,
        }
    }
}

/// GUARANTEED re-amp OFF on a fresh connection — the run-end backstop every
/// command that engages re-amp must call, success or failure. The in-session
/// disengage after each capture cannot be trusted: on fw 1.8.45 an OFF sent after more
/// than ~1 s of idle was silently dropped (HW-bisected: 300 ms lands, 1 s+ drops), and on
/// fw 1.8.58 an OFF to a lapsed session draws ConnectionError(0) and is discarded
/// (device-confirmed). `TrackedHid` reopens a client after 600 ms of silence, which
/// covers the known lapse shapes. A unit stranded re-amp-ON is input-muted; a developer
/// recovers it with `probe --reamp-off`, a user by restarting the unit — a USB
/// unplug/replug does not clear it (`danger.md`, `notes/gotchas.md`). `tag` names the
/// calling lane in the log lines.
///
/// The OFF is read back: after it, the Mixer store is read on the same session
/// ([`Session::read_reamp_state`]), and a store still ON (or silent) climbs a ladder —
/// one more OFF on a fresh connection, then a Settings ON → OFF on a fresh connection,
/// which re-syncs both stores when a stale Settings flag made the OFF a no-op that still
/// echoes (device-confirmed) — reading back after each step. If none reads OFF, the
/// outcome is unverified: it is logged as an error and [`ReampOffOutcome::warning`] goes
/// on the run result. The ladder only ever ADDS work after the first OFF; it never skips
/// or delays it, and it never sends Mixer `setReAmpMode`(15), which desyncs the two
/// stores (`danger.md`). Callers keep calling it on every exit path.
pub(crate) fn reamp_off_guaranteed(tag: &str) -> ReampOffOutcome {
    let out = reamp_off_ladder(tag, Session::connect_lean);
    // Every rung has been tried, verified or not: nothing is left for a guard to send.
    crate::session::settle_reamp_engage();
    out
}

/// One OFF on a fresh connection, unverified — the MID-run disengage a solve sends after a
/// failed capture, before it re-engages on another fresh connection, or before a write
/// session. Not a run-end backstop: that is [`reamp_off_guaranteed`], owned by
/// [`ReampRunGuard`].
pub(crate) fn reamp_off_fresh(tag: &str) {
    if let Err(e) = Session::connect_lean().and_then(|mut s| s.set_reamp_mode(false)) {
        log::warn!("{tag}: fresh-connection re-amp OFF failed ({e})");
    }
}

/// The run-end re-amp OFF as a scope guard: the panic-safe form of [`reamp_off_guaranteed`].
///
/// Create it before the run's first engage, and declare it BEFORE every `Session` the run
/// opens, so it drops after them (locals drop in reverse order): the OFF goes out on a fresh
/// connection with no other seize held in this process, never on a held one (`danger.md`).
///
/// - [`Self::finish`] is the normal end: it always runs the ladder and returns its verdict,
///   which the caller stamps on the run's result.
/// - `Drop` without `finish` (a panic unwinding, or an early return) runs the ladder only if
///   this thread engaged re-amp since the guard was created or the last ladder ran
///   ([`crate::session::reamp_engage_unsettled`]), so an early refusal before any engage
///   opens no connection. It never panics: a panic
///   while unwinding aborts the process, so a failure is logged and swallowed.
///
/// [`crate::device_gate::with_released_seize_blocking`] holds one around every device
/// command, so a command that engaged and returned (or panicked) without its own backstop
/// still ends OFF.
#[must_use = "the guard sends the run-end re-amp OFF when it drops"]
pub(crate) struct ReampRunGuard {
    pub(super) tag: &'static str,
    pub(super) connect: Box<dyn FnMut() -> Result<Session, String>>,
    /// Cleared by [`Self::finish`], which has already sent the ladder.
    pub(super) armed: bool,
}

impl ReampRunGuard {
    pub(crate) fn new(tag: &'static str) -> Self {
        Self::with_connect(tag, Session::connect_lean)
    }

    /// [`Self::new`] over any `connect` (a test hands it a fake's sessions).
    pub(crate) fn with_connect(
        tag: &'static str,
        connect: impl FnMut() -> Result<Session, String> + 'static,
    ) -> Self {
        // The guard answers for engages made in ITS scope only: a pooled blocking thread can
        // carry an unsettled flag from earlier work that had no guard of its own.
        crate::session::settle_reamp_engage();
        Self {
            tag,
            connect: Box::new(connect),
            armed: true,
        }
    }

    /// The normal run end: the full ladder, always, with its verdict.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no run reads the verdict yet; every run ends through Drop"
        )
    )]
    pub(crate) fn finish(mut self) -> ReampOffOutcome {
        self.armed = false;
        let out = reamp_off_ladder(self.tag, &mut self.connect);
        crate::session::settle_reamp_engage();
        out
    }
}

impl Drop for ReampRunGuard {
    fn drop(&mut self) {
        if !self.armed || !crate::session::reamp_engage_unsettled() {
            return;
        }
        let tag = self.tag;
        let connect = &mut self.connect;
        if std::thread::panicking() {
            log::error!("{tag}: unwinding after a re-amp engage; sending the run-end OFF");
        } else {
            log::info!("{tag}: re-amp engaged with no run-end OFF yet; sending it now");
        }
        let sent = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reamp_off_ladder(tag, connect)
        }));
        crate::session::settle_reamp_engage();
        match sent {
            Ok(out) if !out.verified => log::error!("{tag}: {REAMP_STILL_ON_WARNING}"),
            Ok(_) => {}
            Err(_) => {
                log::error!("{tag}: the run-end re-amp OFF panicked; {REAMP_STILL_ON_WARNING}")
            }
        }
    }
}

/// One rung of the [`reamp_off_ladder`] past the first OFF: its log name and its sends.
pub(super) type ReampRung<'a> = (&'a str, &'a dyn Fn(&mut Session) -> Result<(), String>);

/// [`reamp_off_guaranteed`] over any `connect` (a test hands it a fake's sessions).
pub(super) fn reamp_off_ladder(
    tag: &str,
    mut connect: impl FnMut() -> Result<Session, String>,
) -> ReampOffOutcome {
    // The Mixer store, read on `s` with kept-alive waits; `None` = no reply (never OFF).
    let read = |s: &mut Session, step: &str| match s.read_reamp_state() {
        Ok(state) => {
            if state.mixer.is_some() && state.settings != state.mixer {
                log::warn!(
                    "{tag}: re-amp stores disagree after {step} (mixer {:?}, settings {:?})",
                    state.mixer,
                    state.settings
                );
            }
            state.mixer
        }
        Err(e) => {
            log::warn!("{tag}: re-amp read-back after {step} failed ({e})");
            None
        }
    };
    // Step 1: the OFF itself, first and unconditional, then the read on the same session.
    let mixer = match connect() {
        Ok(mut s) => match s.set_reamp_mode(false) {
            Ok(_) => {
                log::info!("{tag}: final re-amp OFF sent");
                read(&mut s, "the final OFF")
            }
            Err(e) => {
                log::warn!("{tag}: final re-amp OFF failed ({e})");
                None
            }
        },
        Err(e) => {
            log::warn!("{tag}: final re-amp OFF failed ({e})");
            None
        }
    };
    if mixer == Some(false) {
        return ReampOffOutcome { verified: true };
    }
    log::warn!("{tag}: re-amp read back {mixer:?} after the final OFF; climbing the ladder");
    // Step 2: one more OFF on a fresh connection.
    let off_again = |s: &mut Session| s.set_reamp_mode(false).map(|_| ());
    // Step 3: Settings ON then OFF, which re-syncs both stores.
    let resync = |s: &mut Session| {
        s.set_reamp_mode(true)?;
        s.set_reamp_mode(false).map(|_| ())
    };
    let rungs: [ReampRung; 2] = [
        ("a fresh-connection OFF", &off_again),
        ("a Settings ON then OFF", &resync),
    ];
    let mut last = mixer;
    for (step, send) in rungs {
        // Each rung's session drops before the next `connect`: never two seizes at once.
        last = match connect() {
            Ok(mut s) => match send(&mut s) {
                Ok(()) => read(&mut s, step),
                Err(e) => {
                    log::warn!("{tag}: {step} failed ({e})");
                    None
                }
            },
            Err(e) => {
                log::warn!("{tag}: {step} could not connect ({e})");
                None
            }
        };
        if last == Some(false) {
            log::info!("{tag}: re-amp read back OFF after {step}");
            return ReampOffOutcome { verified: true };
        }
    }
    log::error!(
        "{tag}: {REAMP_STILL_ON_WARNING} The Mixer store read {last:?} after every OFF the \
         run-end ladder sent (recovery: `probe --reamp-off`)."
    );
    ReampOffOutcome { verified: false }
}

/// The run-end re-amp OFF ladder (`reamp_off_ladder`) over the fake: every rung, its
/// verdict, and the one write it must never send.
#[cfg(test)]
mod reamp_off_ladder_tests {
    use super::*;
    use crate::hid::HidTransport;
    use crate::proto;
    use crate::sim_device::{Injection, SimDevice, SimEvent};
    use std::sync::{Arc, Mutex};

    fn session(sim: &SimDevice) -> Session {
        Session::from_transport(Box::new(sim.clone()))
    }

    /// An engaged fake with `injection` armed.
    fn engaged(injection: Injection) -> SimDevice {
        let sim = SimDevice::new();
        session(&sim).set_reamp_mode(true).expect("engage");
        sim.arm(injection);
        sim
    }

    fn reamp_writes(sim: &SimDevice) -> Vec<bool> {
        sim.events()
            .into_iter()
            .filter_map(|e| match e {
                SimEvent::ReAmp(on) => Some(on),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_stale_flag_off_is_caught_by_the_read_and_the_fresh_off_lands() {
        let sim = engaged(Injection::DropNextReampOff);
        let out = reamp_off_ladder("test", || Ok(session(&sim)));
        assert!(out.verified, "the second OFF lands and reads back OFF");
        assert_eq!(out.warning(), None);
        assert_eq!(
            session(&sim).read_reamp_state().expect("read").mixer,
            Some(false),
            "the fake's latch ends OFF"
        );
        // Engage, the dropped OFF, the fresh-connection OFF; no Settings re-sync needed.
        assert_eq!(reamp_writes(&sim), [true, false, false]);
    }

    #[test]
    fn a_clean_off_is_verified_with_no_extra_write() {
        let sim = SimDevice::new();
        session(&sim).set_reamp_mode(true).expect("engage");
        let out = reamp_off_ladder("test", || Ok(session(&sim)));
        assert!(out.verified);
        assert_eq!(reamp_writes(&sim), [true, false]);
    }

    #[test]
    fn an_off_that_never_lands_climbs_every_rung_and_warns() {
        let sim = engaged(Injection::ReampOffNeverLands);
        let out = reamp_off_ladder("test", || Ok(session(&sim)));
        assert!(!out.verified);
        assert_eq!(out.warning().as_deref(), Some(REAMP_STILL_ON_WARNING));
        // Engage; the final OFF; one more OFF; the Settings ON → OFF re-sync.
        assert_eq!(reamp_writes(&sim), [true, false, false, true, false]);
        assert_eq!(
            out.annotate_err("boom".into()).to_string(),
            format!("boom — {REAMP_STILL_ON_WARNING}")
        );
        // A cancel stays a cancel — also one whose restore failed (#269).
        assert_eq!(
            out.annotate_err(LevelError::Cancelled),
            LevelError::Cancelled
        );
        let restore_failed = LevelError::Cancelled.with_restore(Err("HID gone".into()));
        assert_eq!(out.annotate_err(restore_failed.clone()), restore_failed);
        assert!(out.still_on(), "the typed signal the frontend reads");
    }

    /// The typed half of the run-end verdict (the screen reads the flag, never the warning
    /// text): every returned row records `reamp_still_on`, and a failed leveling command's
    /// typed error carries it beside its skip kind — the kind of the failure itself, decided
    /// before the warning text is appended, so the frontend never reads the text for either.
    #[test]
    fn a_leveling_command_carries_the_reamp_verdict_typed_on_rows_and_errors() {
        let stuck = ReampOffOutcome { verified: false };
        let clean = ReampOffOutcome { verified: true };
        let watch = crate::watcher::DetachWatch::start();

        let mut seen = None;
        let rows = stuck.stamp_leveling(Ok::<_, LevelError>(vec![1, 2]), watch, |_, out| {
            seen = Some(out)
        });
        assert_eq!(rows.expect("rows"), [1, 2]);
        assert_eq!(seen, Some(stuck), "the rows get the outcome itself");

        let err = stuck
            .stamp_leveling(
                Err::<(), _>(LevelError::refused(SkipKind::PresetUnreadable, "no read")),
                watch,
                |_, _| {},
            )
            .expect_err("a failed run");
        assert_eq!(
            err,
            AppError::Leveling {
                kind: SkipKind::PresetUnreadable,
                message: format!("no read — {REAMP_STILL_ON_WARNING}"),
                reamp_still_on: true,
            }
        );
        let err = clean
            .stamp_leveling(Err::<(), _>(LevelError::NoSignal), watch, |_, _| {})
            .expect_err("a failed run");
        assert_eq!(
            err,
            AppError::Leveling {
                kind: SkipKind::NoSignal,
                message: "no signal captured".into(),
                reamp_still_on: false,
            }
        );
        // A Stop stays a Stop, with its own text, and still reports the flag.
        let err = stuck
            .stamp_leveling(Err::<(), _>(LevelError::Cancelled), watch, |_, _| {})
            .expect_err("a stopped run");
        assert_eq!(err.kind(), "cancelled");
        assert_eq!(err.message(), "cancelled");
        assert!(matches!(
            err,
            AppError::Leveling {
                reamp_still_on: true,
                ..
            }
        ));
    }

    /// A batched command's verdict rides BESIDE its rows, so a batch that returns no rows (a
    /// Stop in the ceiling prepass, or every row failed) still says re-amp may be on.
    #[test]
    fn a_batch_carries_the_reamp_verdict_whatever_its_row_count() {
        let stuck = ReampOffOutcome { verified: false };
        let clean = ReampOffOutcome { verified: true };
        let watch = crate::watcher::DetachWatch::start();

        let none = stuck
            .stamp_batch(Ok::<Vec<u32>, LevelError>(Vec::new()), watch, |_, _| {
                unreachable!("there is no row to stamp")
            })
            .expect("an empty batch is still Ok");
        assert_eq!(
            none,
            BatchLevelRun {
                rows: Vec::new(),
                reamp_still_on: true,
                reamp_warning: Some(REAMP_STILL_ON_WARNING.to_string()),
            }
        );
        let wire = serde_json::to_value(&none).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({
                "rows": [],
                "reampStillOn": true,
                "reampWarning": REAMP_STILL_ON_WARNING,
            })
        );

        let stamped = std::cell::Cell::new(0);
        let some = stuck
            .stamp_batch(Ok::<_, LevelError>(vec![1u32, 2]), watch, |_, out| {
                assert_eq!(out, stuck);
                stamped.set(stamped.get() + 1);
            })
            .expect("rows");
        assert_eq!(
            (some.rows.len(), stamped.get()),
            (2, 2),
            "every row stamped"
        );
        assert!(some.reamp_still_on);

        let ok = clean
            .stamp_batch(Ok::<Vec<u32>, LevelError>(Vec::new()), watch, |_, _| {})
            .expect("rows");
        assert!(!ok.reamp_still_on);
        assert_eq!(
            serde_json::to_value(&ok).expect("serialize"),
            serde_json::json!({ "rows": [], "reampStillOn": false }),
            "no warning key when the OFF read back"
        );
    }

    /// The user's recovery is a restart of the unit: a USB unplug/replug does not clear a
    /// stuck re-amp (static evidence, fw 1.8.58 — `notes/gotchas.md`).
    #[test]
    fn the_stuck_reamp_warning_says_restart_never_replug() {
        assert!(REAMP_STILL_ON_WARNING.contains("Restart your Tone Master Pro"));
        assert!(!REAMP_STILL_ON_WARNING.to_lowercase().contains("plug"));
    }

    /// #261: a panic after an engage unwinds through the guard, which sends the OFF ladder
    /// on a FRESH connection (the run's own session dropped first) — the shipped app's
    /// equivalent of `probe`'s `ReampOffGuard`.
    #[test]
    fn a_panic_after_an_engage_still_sends_the_off_on_a_fresh_connection() {
        let sim = SimDevice::new();
        let connects = Arc::new(Mutex::new(0u32));
        let (fake, n) = (sim.clone(), connects.clone());
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = ReampRunGuard::with_connect("test", move || {
                *n.lock().expect("count") += 1;
                Ok(session(&fake))
            });
            let mut run = session(&sim);
            run.set_reamp_mode(true).expect("engage");
            panic!("a reply parser blew up mid-capture");
        }));
        assert!(unwound.is_err(), "the panic still propagates");
        assert_eq!(*connects.lock().expect("count"), 1, "one fresh connection");
        assert_eq!(reamp_writes(&sim), [true, false]);
        assert_eq!(
            session(&sim).read_reamp_state().expect("read").mixer,
            Some(false)
        );
        assert!(!crate::session::reamp_engage_unsettled());
    }

    /// A connect that PANICS inside the guard's `Drop`, while the run is already unwinding,
    /// is caught there: the run's own panic propagates, and the process does not abort.
    #[test]
    fn a_panicking_off_inside_drop_is_swallowed() {
        let sim = SimDevice::new();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard =
                ReampRunGuard::with_connect("test", || panic!("the OFF's connect panicked"));
            session(&sim).set_reamp_mode(true).expect("engage");
            panic!("the run panicked");
        }));
        let msg = unwound.expect_err("the run's panic propagates");
        assert_eq!(msg.downcast_ref::<&str>(), Some(&"the run panicked"));
        assert!(!crate::session::reamp_engage_unsettled());
    }

    /// An early return BEFORE any engage (a refused read, a bad stimulus) opens nothing.
    #[test]
    fn a_guard_with_no_engage_opens_no_connection() {
        crate::session::settle_reamp_engage();
        let connects = Arc::new(Mutex::new(0u32));
        let n = connects.clone();
        drop(ReampRunGuard::with_connect("test", move || {
            *n.lock().expect("count") += 1;
            Err("no device".to_string())
        }));
        assert_eq!(*connects.lock().expect("count"), 0);
    }

    /// `finish` is the normal run end: the ladder runs once, its verdict comes back, and the
    /// guard's own `Drop` sends nothing more.
    #[test]
    fn finish_runs_the_ladder_once() {
        let sim = SimDevice::new();
        session(&sim).set_reamp_mode(true).expect("engage");
        let fake = sim.clone();
        let out = ReampRunGuard::with_connect("test", move || Ok(session(&fake))).finish();
        assert!(out.verified);
        assert_eq!(reamp_writes(&sim), [true, false]);
    }

    #[test]
    fn a_failed_connect_never_verifies() {
        let out = reamp_off_ladder("test", || Err("no device".to_string()));
        assert!(!out.verified);
    }

    /// Records every body a session sends, then hands it to the fake.
    struct Recording {
        sim: SimDevice,
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Recording {
        fn log(&self, body: &[u8]) {
            self.sent.lock().expect("log").push(body.to_vec());
        }
    }

    impl HidTransport for Recording {
        fn send(&self, body: &[u8]) -> Result<(), String> {
            self.log(body);
            self.sim.send(body)
        }
        fn transact(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.log(body);
            self.sim.transact(body, ms)
        }
        fn transact_chunked(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.log(body);
            self.sim.transact_chunked(body, ms)
        }
        fn pump(&self, ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.sim.pump(ms)
        }
        fn transact_eager(&self, body: &[u8], ms: u64) -> Result<Vec<Vec<u8>>, String> {
            self.log(body);
            self.sim.transact_eager(body, ms)
        }
        fn template_mark(&self) -> std::sync::Arc<crate::hid::TemplateMark> {
            crate::hid::HidTransport::template_mark(&self.sim)
        }
    }

    /// The MixerMessage(5) fields `body` carries (empty when it is not a Mixer message).
    fn mixer_fields(body: &[u8]) -> Vec<u32> {
        proto::first_bytes(&proto::parse(body), 5)
            .map(|m| proto::parse(m).iter().map(|(f, _)| *f).collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_ladder_never_sends_mixer_set_reamp_mode() {
        // Every rung runs: the OFF never lands.
        let sim = engaged(Injection::ReampOffNeverLands);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let out = reamp_off_ladder("test", || {
            Ok(Session::from_transport(Box::new(Recording {
                sim: sim.clone(),
                sent: sent.clone(),
            })))
        });
        assert!(!out.verified);
        let sent = sent.lock().expect("log");
        let mixer: Vec<u32> = sent.iter().flat_map(|b| mixer_fields(b)).collect();
        // Non-vacuity: the decoder sees the Mixer reads (reAmpModeRequest, field 4), one per
        // rung.
        assert_eq!(mixer.iter().filter(|f| **f == 4).count(), 3, "{mixer:?}");
        assert!(
            !mixer.contains(&15),
            "the ladder sent Mixer setReAmpMode(15), which desyncs the two stores: {mixer:?}"
        );
    }
}
