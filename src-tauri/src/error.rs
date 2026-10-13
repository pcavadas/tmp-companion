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
//!
//! The migration is incremental, boundary first: only `connect_device`, whose failure kinds
//! the frontend tells apart, returns [`AppError`]. Every other command keeps
//! `Result<_, String>` until a caller needs its kinds. `From` impls into `String` let a
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
    /// `primary` failed, and the stored-preset restore that followed failed too. Keeps the
    /// primary's kind: a cancel whose restore failed is still a cancel.
    Restore {
        primary: Box<LevelError>,
        restore: String,
    },
}

impl LevelError {
    /// A Stop, directly or as the primary of a failed restore.
    pub fn is_cancelled(&self) -> bool {
        match self {
            LevelError::Cancelled => true,
            LevelError::Restore { primary, .. } => primary.is_cancelled(),
            LevelError::NoSignal
            | LevelError::Clipped
            | LevelError::FloorRead
            | LevelError::Device(_) => false,
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
            | LevelError::Device(_) => false,
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
    /// The macOS open retries on this kind alone (`kIOReturnExclusiveAccess`).
    SeizeHeld(String),
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
            | DeviceError::NotReady(m)
            | DeviceError::Other(m) => m,
        }
    }

    /// The same kind with its message rewritten by `f` (context added around it).
    pub fn map_message(self, f: impl FnOnce(String) -> String) -> DeviceError {
        match self {
            DeviceError::NotFound(m) => DeviceError::NotFound(f(m)),
            DeviceError::SeizeHeld(m) => DeviceError::SeizeHeld(f(m)),
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
    /// The device is held by someone else (Pro Control, another session or process).
    Busy(String),
    /// The device is present but not ready yet (booting, or just went away).
    NotReady(String),
    /// Anything else, its text verbatim.
    Other(String),
}

impl AppError {
    /// The wire `kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::NotConnected(_) => "not_connected",
            AppError::Busy(_) => "busy",
            AppError::NotReady(_) => "not_ready",
            AppError::Other(_) => "other",
        }
    }

    /// The human message.
    pub fn message(&self) -> &str {
        match self {
            AppError::NotConnected(m)
            | AppError::Busy(m)
            | AppError::NotReady(m)
            | AppError::Other(m) => m,
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
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", self.message())?;
        s.end()
    }
}

impl From<String> for AppError {
    fn from(e: String) -> Self {
        AppError::Other(e)
    }
}

impl From<DeviceError> for AppError {
    fn from(e: DeviceError) -> Self {
        match e {
            DeviceError::NotFound(m) => AppError::NotConnected(m),
            DeviceError::SeizeHeld(m) => AppError::Busy(m),
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

    #[test]
    fn device_error_map_message_keeps_the_kind() {
        let e = DeviceError::SeizeHeld("held".into()).map_message(|m| format!("1 other ({m})"));
        assert_eq!(e, DeviceError::SeizeHeld("1 other (held)".into()));
    }
}
