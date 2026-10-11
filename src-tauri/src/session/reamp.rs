//! Re-amp: the engage/disengage setter with its counters, and the read-only state read
//! of both device stores (Mixer and Settings). Calls link primitives and `timing` consts
//! only.

use super::*;

/// Re-amp engage/disengage counters (ALWAYS compiled — the e2e verification harness
/// reads them over the bridge, offline AND online, to assert every leveling/doctor run
/// disengages at least as often as it engages). Incremented at the top of
/// [`Session::set_reamp_mode`] by flag, so `OFF >= ON` proves the app-side
/// `reamp_off_guaranteed` backstop ran and isn't being masked by a spec teardown rescue.
pub static REAMP_ON_COUNT: AtomicU32 = AtomicU32::new(0);

pub static REAMP_OFF_COUNT: AtomicU32 = AtomicU32::new(0);

/// The device's re-amp state as [`Session::read_reamp_state`] read it: `None` = that store
/// gave no reply, `Some(on)` = it answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReampState {
    /// The Mixer store — the audio path's real state.
    pub mixer: Option<bool>,
    /// The Settings flag a Settings engage checks.
    pub settings: Option<bool>,
}

/// A `MixerMessage.reAmpModeResponse` body → its flag (field 9 without field 1 = OFF).
pub(super) fn decode_mixer_reamp(body: &[u8]) -> Option<bool> {
    let resp = dig(body, TMS_MIXER, MIXER_REAMP_RESPONSE)?;
    Some(proto::first_varint(&resp, 1).unwrap_or(0) != 0)
}

/// A `SettingsMessage.usbSettingsResponse` body → its re-amp flag (absent = OFF).
pub(super) fn decode_usb_reamp(body: &[u8]) -> Option<bool> {
    let usb = dig(body, TMS_SETTINGS, USB_SETTINGS_RESPONSE)?;
    let flag = proto::first_bytes(&usb, USB_REAMP_ACTIVE)
        .and_then(|b| proto::first_varint(&proto::parse(b), 1));
    Some(flag.unwrap_or(0) != 0)
}

impl Session {
    /// Set re-amp mode via the Global Settings path. Returns the echoed
    /// `SettingsMessage.reampModeActive` value if the device reports it.
    ///
    /// That return is NOT a confirmation. The OFF echo is the empty `ReampModeActive{}`
    /// (proto3 omits a false field), so OFF always returns `None`, and a late ON echo
    /// queued behind the ~1.2 s handshake drain can land in the OFF window as
    /// `Some(true)` (fw 1.8.58, device trace). Read the real state with
    /// [`Self::read_reamp_state`].
    pub fn set_reamp_mode(&mut self, active: bool) -> Result<Option<bool>, String> {
        if active {
            REAMP_ON_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            REAMP_OFF_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        self.raw.clear();
        self.send_and_collect(&proto::set_reamp_mode(active), REAMP_ECHO_MS)?;
        Ok(self.streams().iter().find_map(|s| {
            dig(&s.body, TMS_SETTINGS, REAMP_SETTING)
                .and_then(|f| field1(&f).and_then(|v| v.as_u64()))
                .map(|v| v != 0)
        }))
    }

    /// Read the re-amp state back from BOTH device stores — read-only, it never sends a
    /// re-amp write. The Mixer store (`MixerMessage.reAmpModeRequest`(4) → field 9) is the
    /// audio path; the Settings flag (`usbSettingsRequest`(55) → field 56 → field 1) is what
    /// a Settings engage checks. They can disagree (a stale Settings flag): a Settings engage
    /// is then acknowledged and does nothing (fw 1.8.58, device-confirmed). A store that never
    /// answered within [`REAMP_STATE_WAIT_MS`] reads `None` — never treat a missing reply as
    /// OFF. Both requests go out first and one buffer collects both replies, so a late Mixer
    /// reply that arrives after the Settings request is still read. A present reply with its flag omitted is OFF (proto3 elides a false field).
    pub fn read_reamp_state(&mut self) -> Result<ReampState, String> {
        self.raw.clear();
        self.send_and_collect(
            &proto::mixer_request(MIXER_REAMP_REQUEST),
            KEEPALIVE_WINDOW_MS,
        )?;
        self.send_and_collect(
            &proto::settings_request(USB_SETTINGS_REQUEST),
            KEEPALIVE_WINDOW_MS,
        )?;
        let mut waited = 2 * KEEPALIVE_WINDOW_MS;
        loop {
            let streams = self.streams();
            let mixer = streams.iter().find_map(|s| decode_mixer_reamp(&s.body));
            let settings = streams.iter().find_map(|s| decode_usb_reamp(&s.body));
            if (mixer.is_some() && settings.is_some()) || waited >= REAMP_STATE_WAIT_MS {
                return Ok(ReampState { mixer, settings });
            }
            self.keepalive_pump()?;
            waited += KEEPALIVE_WINDOW_MS;
        }
    }
}
