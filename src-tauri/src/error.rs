//! Typed errors in place of string matching (#269).
//!
//! Most of the backend still returns `Result<_, String>`, and the frontend shows that text
//! verbatim. Where code had to tell one failure KIND from another, it used to compare or
//! search the text (`e == CANCELLED`, `e.contains("no signal captured")`,
//! `e.contains("0xe00002c5")`, `e.contains("no TMP found")`). Rewording a message broke
//! that silently, and a cancel whose restore also failed stopped reading as a cancel. The
//! types here carry the kind instead; each one's `Display` is the exact text the old string
//! carried, so what the user reads (and what tests assert on) is unchanged.
//!
//! - [`LevelError`]: the leveling and measurement paths (`leveller`, `audio::reamp_capture`,
//!   `device_gate::sleep_or_cancel`). Cancel and silent-capture are variants, and
//!   [`LevelError::Restore`] keeps its primary's kind.
//! - [`DeviceError`]: why a device open or connect failed (`hid::Hid::open`,
//!   `Session::connect_with_firmware`, the monitor's stored connect error).
//! - [`AppError`]: the serializable COMMAND error, `{ "kind": …, "message": … }` on the wire.
//! - [`SkipKind`] and [`Refusal`]: WHY a sound could not be leveled, as a kind beside the
//!   free-text reason. The leveling commands report it on each failed row and on their own
//!   error, so the Summary picks its message (and whether Try again can help) by kind; the
//!   text stays for the log and the bug report.
//!
//! The migration is incremental, boundary first: `connect_device` and the three leveling
//! commands (`level_preset`, `level_scenes_apply_batched`, `level_footswitches_apply`),
//! whose failure kinds the frontend tells apart, return [`AppError`]. Every other command
//! keeps `Result<_, String>` until a caller needs its kinds. `From` impls into `String` let a
//! String-returning caller keep using `?` on the typed errors.

use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

/// The per-item error when a floor read persists through the floor guard's retry
/// ([`LevelError::FloorRead`]'s text).
pub(crate) const FLOOR_READ_ERR: &str = "no stimulus reached the device (captured only \
    the output floor) — check the USB audio connection and try again";

/// The per-item error when a capture flat-tops at full scale on the processed pair
/// ([`crate::lufs::Loudness::clipped`]): its integrated loudness reads LOW, so a level solved
/// from it lands too LOUD. Raised by the shared measurement hub, so every lane refuses
/// to solve from a clipped capture; only `measure_c` has a rescue (half the reference).
/// [`LevelError::Clipped`]'s text.
pub(crate) const CLIPPED_CAPTURE_ERR: &str = "the sound clipped at full scale while it was \
    measured, so its loudness can't be read — turn this sound's level down and run it again";

/// WHY a sound could not be leveled — the typed kind a failed row and a failed leveling
/// command carry beside their free-text reason. The frontend picks the Summary's message by
/// kind and keeps the sounds that fail the same way every time (no amp, level it manually, a
/// refused write) out of Try again; the text is for the log and the bug report only.
///
/// Serialized `snake_case`, mirrored in `src/lib/types.ts` (`SkipKind`). Add a kind with the
/// code that produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    /// The preset itself could not be read from the unit (a failed or cut field-8 read, a
    /// signal chain the read could not answer for), so nothing was written.
    PresetUnreadable,
    /// The preset's footswitch assignments could not be read — its `ftsw` section came back
    /// cut or missing, or does not carry the switch asked for — so nothing was written. A
    /// read that failed outright is [`Self::PresetUnreadable`]
    /// (`slot_read::SlotReadError::skip_kind`).
    FootswitchesUnreadable,
    /// No guitar amp is on in this scene (or the preset has none), so the default control —
    /// the amp's output level — does not exist here.
    NoActiveAmp,
    /// The routing is one the leveler does not drive on its own (amps across several
    /// splits, an amp before a split mixed with lane amps, an amp with no output level, a
    /// routing template it does not know). A chosen block and knob still level it.
    LevelManually,
    /// The scene write was refused: this scene shares the knob with the main sound (its
    /// Scene Edit is off), or the saved preset cannot say where the write would land.
    SceneWriteRefused,
    /// The chosen knob is not a volume or mix control, so sweeping it would change the sound
    /// instead of its loudness.
    UnsupportedKnob,
    /// The chosen knob is no longer in the preset (its block or parameter is gone).
    KnobMissing,
    /// The chosen knob's block is off in this sound (bypassed in the main sound and not
    /// turned on by this footswitch), so turning it cannot change the loudness.
    BlockOff,
    /// The unit's Scene Change Behavior is set to discard changes, which would throw away
    /// every scene write before the run's one save.
    SceneChangeDiscards,
    /// The test tone never reached the unit (only its output floor was captured).
    NoStimulus,
    /// The capture clipped at full scale, so its loudness could not be read.
    ClippedCapture,
    /// Nothing came back from the unit at all.
    NoSignal,
    /// The unit was unplugged during the run.
    ConnectionLost,
    /// The run was stopped before this sound finished. Not a failure: a stopped sound is one
    /// the run never reached.
    Cancelled,
    /// Anything else.
    Other,
}

impl SkipKind {
    /// The wire token (the serde name).
    pub fn as_str(self) -> &'static str {
        match self {
            SkipKind::PresetUnreadable => "preset_unreadable",
            SkipKind::FootswitchesUnreadable => "footswitches_unreadable",
            SkipKind::NoActiveAmp => "no_active_amp",
            SkipKind::LevelManually => "level_manually",
            SkipKind::SceneWriteRefused => "scene_write_refused",
            SkipKind::UnsupportedKnob => "unsupported_knob",
            SkipKind::KnobMissing => "knob_missing",
            SkipKind::BlockOff => "block_off",
            SkipKind::SceneChangeDiscards => "scene_change_discards",
            SkipKind::NoStimulus => "no_stimulus",
            SkipKind::ClippedCapture => "clipped_capture",
            SkipKind::NoSignal => "no_signal",
            SkipKind::ConnectionLost => "connection_lost",
            SkipKind::Cancelled => "cancelled",
            SkipKind::Other => "other",
        }
    }

    /// Whether a lost connection can be the real cause of a failure of this kind: a read
    /// or a capture that failed, or anything untyped. Not the kinds decided from the saved
    /// preset or the unit's settings alone, and not a Stop — a detach during the run never
    /// re-labels those (see `crate::watcher::DetachWatch`).
    pub fn can_be_a_lost_connection(self) -> bool {
        matches!(
            self,
            SkipKind::PresetUnreadable
                | SkipKind::FootswitchesUnreadable
                | SkipKind::NoStimulus
                | SkipKind::ClippedCapture
                | SkipKind::NoSignal
                | SkipKind::Other
        )
    }
}

/// A refusal to level one sound (or a whole batch): its [`SkipKind`] and the free-text reason
/// (`Display`), which is the exact text the untyped `String` carried before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub kind: SkipKind,
    pub reason: String,
}

impl Refusal {
    pub fn new(kind: SkipKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for Refusal {}

impl From<Refusal> for String {
    fn from(r: Refusal) -> Self {
        r.reason
    }
}

/// A leveling/measurement failure. `Display` is the historical message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelError {
    /// A cooperative cancel (Stop) was observed at a checkpoint. Displays `cancelled`.
    Cancelled,
    /// The capture held no measurable signal (the preset's output is not on USB 1/2, or
    /// re-amp injected nothing). Displays `no signal captured`.
    NoSignal,
    /// The capture flat-topped at full scale. Displays [`CLIPPED_CAPTURE_ERR`].
    Clipped,
    /// The capture still read as the device's stationary output floor after the floor
    /// guard's retry (a silent re-amp inject). Displays [`FLOOR_READ_ERR`].
    FloorRead,
    /// Any other failure: a device, audio or validation error, its text verbatim.
    Device(String),
    /// A refusal of a known kind ([`SkipKind`]), decided before or instead of device work.
    /// Displays its reason verbatim.
    Refused(Refusal),
    /// `primary` failed, and the stored-preset restore that followed failed too. Keeps the
    /// primary's kind: a cancel whose restore failed is still a cancel.
    Restore {
        primary: Box<LevelError>,
        restore: String,
    },
}

impl LevelError {
    /// A [`LevelError::Refused`] of `kind`.
    pub fn refused(kind: SkipKind, reason: impl Into<String>) -> Self {
        LevelError::Refused(Refusal::new(kind, reason))
    }

    /// A Stop, directly or as the primary of a failed restore.
    pub fn is_cancelled(&self) -> bool {
        match self {
            LevelError::Cancelled => true,
            LevelError::Restore { primary, .. } => primary.is_cancelled(),
            LevelError::NoSignal
            | LevelError::Clipped
            | LevelError::FloorRead
            | LevelError::Device(_)
            | LevelError::Refused(_) => false,
        }
    }

    /// A silent capture, directly or as the primary of a failed restore.
    pub fn is_no_signal(&self) -> bool {
        match self {
            LevelError::NoSignal => true,
            LevelError::Restore { primary, .. } => primary.is_no_signal(),
            LevelError::Cancelled
            | LevelError::Clipped
            | LevelError::FloorRead
            | LevelError::Device(_)
            | LevelError::Refused(_) => false,
        }
    }

    /// This failure as a [`Refusal`]: its [`Self::skip_kind`] and its text — what a failed
    /// row records.
    pub fn refusal(&self) -> Refusal {
        Refusal::new(self.skip_kind(), self.to_string())
    }

    /// The [`SkipKind`] a row that failed this way reports. A failed restore keeps its
    /// primary's kind, so a Stop whose restore failed still reads as a Stop.
    pub fn skip_kind(&self) -> SkipKind {
        match self {
            LevelError::Refused(r) => r.kind,
            LevelError::NoSignal => SkipKind::NoSignal,
            LevelError::Clipped => SkipKind::ClippedCapture,
            LevelError::FloorRead => SkipKind::NoStimulus,
            LevelError::Cancelled => SkipKind::Cancelled,
            LevelError::Restore { primary, .. } => primary.skip_kind(),
            LevelError::Device(_) => SkipKind::Other,
        }
    }

    /// Fold a restore result into this error: unchanged when the restore succeeded, else
    /// the [`LevelError::Restore`] that keeps this error's kind.
    pub fn with_restore(self, restore: Result<(), String>) -> LevelError {
        match restore {
            Ok(()) => self,
            Err(restore) => LevelError::Restore {
                primary: Box::new(self),
                restore,
            },
        }
    }
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LevelError::Cancelled => f.write_str("cancelled"),
            LevelError::NoSignal => f.write_str("no signal captured"),
            LevelError::Clipped => f.write_str(CLIPPED_CAPTURE_ERR),
            LevelError::FloorRead => f.write_str(FLOOR_READ_ERR),
            LevelError::Device(e) => f.write_str(e),
            LevelError::Refused(r) => f.write_str(&r.reason),
            LevelError::Restore { primary, restore } => {
                write!(
                    f,
                    "{primary}; also failed to restore stored preset: {restore}"
                )
            }
        }
    }
}

impl std::error::Error for LevelError {}

impl From<String> for LevelError {
    fn from(e: String) -> Self {
        LevelError::Device(e)
    }
}

impl From<&str> for LevelError {
    fn from(e: &str) -> Self {
        LevelError::Device(e.to_string())
    }
}

impl From<LevelError> for String {
    fn from(e: LevelError) -> Self {
        e.to_string()
    }
}

impl From<Refusal> for LevelError {
    fn from(r: Refusal) -> Self {
        LevelError::Refused(r)
    }
}

impl From<DeviceError> for LevelError {
    fn from(e: DeviceError) -> Self {
        LevelError::Device(e.to_string())
    }
}

/// Why opening or connecting to the device failed. `Display` is the message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceError {
    /// No TMP is enumerated: nothing plugged in.
    NotFound(String),
    /// Another handle holds the exclusive open (Pro Control, or another session or process).
    /// The macOS open retries on this kind alone (`kIOReturnExclusiveAccess`); see
    /// [`DeviceError::is_held_seize`].
    SeizeHeld(String),
    /// The OS refused the open in the way another app holding the unit makes it refuse
    /// (macOS `kIOReturnNotPermitted`, Windows `ERROR_ACCESS_DENIED`). The user is told the
    /// same thing as for [`DeviceError::SeizeHeld`] (both are the `busy` kind), but the open
    /// is not retried on it. Only the macOS and Windows transports raise it.
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
    OpenDenied(String),
    /// The unit opened, but the OS refused a report write during the handshake: its USB
    /// stack is not accepting reports yet (the cold-boot window), or it went away.
    NotReady(String),
    /// Anything else.
    Other(String),
}

impl DeviceError {
    /// The message text, whatever the kind.
    pub fn message(&self) -> &str {
        match self {
            DeviceError::NotFound(m)
            | DeviceError::SeizeHeld(m)
            | DeviceError::OpenDenied(m)
            | DeviceError::NotReady(m)
            | DeviceError::Other(m) => m,
        }
    }

    /// A held exclusive open: the one kind worth re-trying an open on (macOS re-enumerates
    /// on it). An [`DeviceError::OpenDenied`] reads as busy to the user but is not retried.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn is_held_seize(&self) -> bool {
        matches!(self, DeviceError::SeizeHeld(_))
    }

    /// The same kind with its message rewritten by `f` (context added around it).
    pub fn map_message(self, f: impl FnOnce(String) -> String) -> DeviceError {
        match self {
            DeviceError::NotFound(m) => DeviceError::NotFound(f(m)),
            DeviceError::SeizeHeld(m) => DeviceError::SeizeHeld(f(m)),
            DeviceError::OpenDenied(m) => DeviceError::OpenDenied(f(m)),
            DeviceError::NotReady(m) => DeviceError::NotReady(f(m)),
            DeviceError::Other(m) => DeviceError::Other(f(m)),
        }
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for DeviceError {}

impl From<String> for DeviceError {
    fn from(e: String) -> Self {
        DeviceError::Other(e)
    }
}

impl From<DeviceError> for String {
    fn from(e: DeviceError) -> Self {
        e.to_string()
    }
}

/// The serializable command error: `{ "kind": "<snake_case>", "message": "<text>" }`. The
/// frontend's `CommandError` reads `kind` to decide how to present a failure, and shows
/// `message` as before. Only the kinds a command actually produces exist; add one with the
/// command that needs it, and mirror it in `src/lib/types.ts`'s `CommandErrorKind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// No device to talk to (nothing plugged in).
    NotConnected(String),
    /// The device is held by someone else (Pro Control, another session or process), or
    /// the OS refused the open the way it does when another app holds it.
    Busy(String),
    /// The device is present but not ready yet (booting, or just went away).
    NotReady(String),
    /// Anything else, its text verbatim.
    Other(String),
    /// A leveling command that failed as a whole: WHY ([`SkipKind`], the wire `kind`), its
    /// text, and whether the run-end re-amp OFF failed to read back OFF (`reampStillOn` on
    /// the wire — the typed twin of the warning appended to the text, so the frontend never
    /// reads the text to know the unit may still be input-muted).
    Leveling {
        kind: SkipKind,
        message: String,
        reamp_still_on: bool,
    },
}

impl AppError {
    /// The wire `kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::NotConnected(_) => "not_connected",
            AppError::Busy(_) => "busy",
            AppError::NotReady(_) => "not_ready",
            AppError::Other(_) => "other",
            AppError::Leveling { kind, .. } => kind.as_str(),
        }
    }

    /// The human message.
    pub fn message(&self) -> &str {
        match self {
            AppError::NotConnected(m)
            | AppError::Busy(m)
            | AppError::NotReady(m)
            | AppError::Other(m)
            | AppError::Leveling { message: m, .. } => m,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let reamp_still_on = match self {
            AppError::Leveling { reamp_still_on, .. } => Some(*reamp_still_on),
            _ => None,
        };
        let mut s =
            serializer.serialize_struct("AppError", 2 + usize::from(reamp_still_on.is_some()))?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", self.message())?;
        if let Some(on) = reamp_still_on {
            s.serialize_field("reampStillOn", &on)?;
        }
        s.end()
    }
}

impl From<String> for AppError {
    fn from(e: String) -> Self {
        AppError::Other(e)
    }
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.message().to_string()
    }
}

impl From<Refusal> for AppError {
    fn from(r: Refusal) -> Self {
        AppError::Leveling {
            kind: r.kind,
            message: r.reason,
            reamp_still_on: false,
        }
    }
}

/// A leveling command's error from a typed leveling failure that returned before anything
/// engaged re-amp, so `reamp_still_on` is `false` (the run-end backstop's own error is
/// built by `ReampOffOutcome::stamp_leveling`).
impl From<LevelError> for AppError {
    fn from(e: LevelError) -> Self {
        AppError::Leveling {
            kind: e.skip_kind(),
            message: e.to_string(),
            reamp_still_on: false,
        }
    }
}

impl From<DeviceError> for AppError {
    fn from(e: DeviceError) -> Self {
        match e {
            DeviceError::NotFound(m) => AppError::NotConnected(m),
            DeviceError::SeizeHeld(m) | DeviceError::OpenDenied(m) => AppError::Busy(m),
            DeviceError::NotReady(m) => AppError::NotReady(m),
            DeviceError::Other(m) => AppError::Other(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_error_display_keeps_the_historical_text() {
        assert_eq!(LevelError::Cancelled.to_string(), "cancelled");
        assert_eq!(LevelError::NoSignal.to_string(), "no signal captured");
        assert_eq!(LevelError::Device("boom".into()).to_string(), "boom");
        assert_eq!(LevelError::Clipped.to_string(), CLIPPED_CAPTURE_ERR);
        assert_eq!(LevelError::FloorRead.to_string(), FLOOR_READ_ERR);
        assert_eq!(
            LevelError::Cancelled
                .with_restore(Err("HID gone".into()))
                .to_string(),
            "cancelled; also failed to restore stored preset: HID gone"
        );
        assert_eq!(String::from(LevelError::NoSignal), "no signal captured");
    }

    #[test]
    fn a_cancel_whose_restore_failed_is_still_a_cancel() {
        let e = LevelError::Cancelled.with_restore(Err("HID gone".into()));
        assert!(e.is_cancelled());
        assert!(!e.is_no_signal());
        // Nested restores keep the kind too.
        let e = e.with_restore(Err("again".into()));
        assert!(e.is_cancelled());
        // A restore that succeeded leaves the error as it was.
        assert_eq!(
            LevelError::NoSignal.with_restore(Ok(())),
            LevelError::NoSignal
        );
        assert!(!LevelError::Device("cancelled".into()).is_cancelled());
    }

    #[test]
    fn app_error_serializes_as_kind_and_message() {
        let v = serde_json::to_value(AppError::from(DeviceError::NotFound("no TMP found".into())))
            .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "kind": "not_connected", "message": "no TMP found" })
        );
        for (e, kind) in [
            (DeviceError::SeizeHeld("x".into()), "busy"),
            (DeviceError::OpenDenied("x".into()), "busy"),
            (DeviceError::NotReady("x".into()), "not_ready"),
            (DeviceError::Other("x".into()), "other"),
        ] {
            let v = serde_json::to_value(AppError::from(e)).unwrap();
            assert_eq!(v["kind"], kind);
            assert_eq!(v["message"], "x");
        }
        assert_eq!(
            serde_json::to_value(AppError::from("t".to_string())).unwrap()["kind"],
            "other"
        );
    }

    /// macOS `kIOReturnNotPermitted` and Windows `ERROR_ACCESS_DENIED` are how the OS
    /// refuses an open while another app holds the unit: they reach the user as `busy`
    /// ("Another app is using your Tone Master Pro"), never the generic failure, yet the
    /// open's retry still keys on a held seize alone, so their retry behaviour is unchanged.
    #[test]
    fn an_open_denial_is_busy_but_never_retried_as_a_held_seize() {
        let denied = DeviceError::OpenDenied("IOHIDDeviceOpen failed: 0xe00002e2".into());
        assert!(!denied.is_held_seize());
        assert!(DeviceError::SeizeHeld("x".into()).is_held_seize());
        for other in [
            DeviceError::NotFound("x".into()),
            DeviceError::NotReady("x".into()),
            DeviceError::Other("x".into()),
        ] {
            assert!(!other.is_held_seize());
        }
        let app = AppError::from(denied);
        assert_eq!(app.kind(), "busy");
        assert_eq!(app.message(), "IOHIDDeviceOpen failed: 0xe00002e2");
        assert_eq!(
            DeviceError::OpenDenied("d".into()).map_message(|m| format!("open ({m})")),
            DeviceError::OpenDenied("open (d)".into())
        );
    }

    /// Every kind, through a match with no wildcard: a new variant fails to compile here
    /// until it is listed, so the wire-token test below covers it too.
    fn every_skip_kind() -> Vec<SkipKind> {
        let all = vec![
            SkipKind::PresetUnreadable,
            SkipKind::FootswitchesUnreadable,
            SkipKind::NoActiveAmp,
            SkipKind::LevelManually,
            SkipKind::SceneWriteRefused,
            SkipKind::UnsupportedKnob,
            SkipKind::KnobMissing,
            SkipKind::BlockOff,
            SkipKind::SceneChangeDiscards,
            SkipKind::NoStimulus,
            SkipKind::ClippedCapture,
            SkipKind::NoSignal,
            SkipKind::ConnectionLost,
            SkipKind::Cancelled,
            SkipKind::Other,
        ];
        for k in &all {
            match k {
                SkipKind::PresetUnreadable
                | SkipKind::FootswitchesUnreadable
                | SkipKind::NoActiveAmp
                | SkipKind::LevelManually
                | SkipKind::SceneWriteRefused
                | SkipKind::UnsupportedKnob
                | SkipKind::KnobMissing
                | SkipKind::BlockOff
                | SkipKind::SceneChangeDiscards
                | SkipKind::NoStimulus
                | SkipKind::ClippedCapture
                | SkipKind::NoSignal
                | SkipKind::ConnectionLost
                | SkipKind::Cancelled
                | SkipKind::Other => {}
            }
        }
        all
    }

    /// The wire token is the serde name, `snake_case`, and `as_str` (what `AppError::kind`
    /// sends) never drifts from it — the frontend's `SkipKind` union mirrors these strings.
    #[test]
    fn a_skip_kind_serializes_as_its_wire_token() {
        let tokens: Vec<&str> = every_skip_kind().iter().map(|k| k.as_str()).collect();
        for k in every_skip_kind() {
            assert_eq!(serde_json::to_value(k).unwrap(), k.as_str(), "{k:?}");
        }
        assert_eq!(
            tokens,
            [
                "preset_unreadable",
                "footswitches_unreadable",
                "no_active_amp",
                "level_manually",
                "scene_write_refused",
                "unsupported_knob",
                "knob_missing",
                "block_off",
                "scene_change_discards",
                "no_stimulus",
                "clipped_capture",
                "no_signal",
                "connection_lost",
                "cancelled",
                "other",
            ]
        );
    }

    /// A failed row's kind comes from the failure's own type, never its text: the capture
    /// failures keep their meaning, a refusal keeps the kind it was raised with, and a Stop
    /// whose restore failed is still a Stop.
    #[test]
    fn a_level_error_names_its_skip_kind_by_type() {
        assert_eq!(LevelError::NoSignal.skip_kind(), SkipKind::NoSignal);
        assert_eq!(LevelError::Clipped.skip_kind(), SkipKind::ClippedCapture);
        assert_eq!(LevelError::FloorRead.skip_kind(), SkipKind::NoStimulus);
        assert_eq!(LevelError::Cancelled.skip_kind(), SkipKind::Cancelled);
        assert_eq!(
            LevelError::Device("no active amp".into()).skip_kind(),
            SkipKind::Other,
            "untyped text never picks a kind, whatever it says"
        );
        let refused = LevelError::refused(SkipKind::BlockOff, "block is bypassed");
        assert_eq!(refused.skip_kind(), SkipKind::BlockOff);
        assert_eq!(refused.to_string(), "block is bypassed", "the text is kept");
        assert!(!refused.is_cancelled());
        assert_eq!(
            refused.refusal(),
            Refusal::new(SkipKind::BlockOff, "block is bypassed")
        );
        assert_eq!(
            LevelError::Cancelled
                .with_restore(Err("HID gone".into()))
                .skip_kind(),
            SkipKind::Cancelled
        );
        assert_eq!(
            LevelError::from(Refusal::new(SkipKind::KnobMissing, "gone")).skip_kind(),
            SkipKind::KnobMissing
        );
        assert_eq!(
            String::from(Refusal::new(SkipKind::Other, "why")),
            "why",
            "a String caller keeps the historical text"
        );
    }

    /// A leveling command's error: the skip kind on the wire `kind`, its text, and the typed
    /// re-amp flag — the only `AppError` that carries `reampStillOn`.
    #[test]
    fn a_leveling_error_serializes_its_kind_text_and_reamp_flag() {
        let e = AppError::Leveling {
            kind: SkipKind::PresetUnreadable,
            message: "could not read preset 4".into(),
            reamp_still_on: true,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({
                "kind": "preset_unreadable",
                "message": "could not read preset 4",
                "reampStillOn": true
            })
        );
        assert_eq!(String::from(e), "could not read preset 4");
        let from_level = serde_json::to_value(AppError::from(LevelError::NoSignal)).unwrap();
        assert_eq!(from_level["kind"], "no_signal");
        assert_eq!(from_level["reampStillOn"], false);
        let from_refusal =
            serde_json::to_value(AppError::from(Refusal::new(SkipKind::NoActiveAmp, "x"))).unwrap();
        assert_eq!(from_refusal["kind"], "no_active_amp");
        // The connect kinds never carry the flag.
        assert!(serde_json::to_value(AppError::Other("x".into()))
            .unwrap()
            .get("reampStillOn")
            .is_none());
    }

    /// Only a kind a lost connection can explain (a failed read or capture, or anything
    /// untyped) is re-labelled after a detach; facts about the saved preset, the unit's
    /// settings and a Stop keep their own kind.
    #[test]
    fn only_a_read_or_capture_failure_can_be_a_lost_connection() {
        let can: Vec<SkipKind> = every_skip_kind()
            .into_iter()
            .filter(|k| k.can_be_a_lost_connection())
            .collect();
        assert_eq!(
            can,
            [
                SkipKind::PresetUnreadable,
                SkipKind::FootswitchesUnreadable,
                SkipKind::NoStimulus,
                SkipKind::ClippedCapture,
                SkipKind::NoSignal,
                SkipKind::Other,
            ]
        );
    }

    #[test]
    fn device_error_map_message_keeps_the_kind() {
        let e = DeviceError::SeizeHeld("held".into()).map_message(|m| format!("1 other ({m})"));
        assert_eq!(e, DeviceError::SeizeHeld("1 other (held)".into()));
    }
}
