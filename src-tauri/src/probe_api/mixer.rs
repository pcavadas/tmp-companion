//! Probe entry point: read the device's **output mixer** channel strip state.
//!
//! Why this exists: every loudness number this app produces is captured from
//! **USB 1/2**, which the Owner's Manual (p.36) documents as a full mixer channel
//! with its own fader, mute, solo and AUX/Bluetooth injection. None of that lives
//! in preset data, and a non-unity fader biases every solved `presetLevel` by a
//! constant the solve-then-verify round trip **cannot** see (both captures ride
//! the same attenuation). Reading the strip is what turns that from a disclaimer
//! into a pre-flight.
//!
//! Read-only: sweeps all five `MixerMessage` read requests, no writes, no re-amp.

use crate::proto;
use crate::proto::Val;
use crate::session::Session;

/// TMS oneof slot for `MixerMessage` (see `proto::mixer_state_request`).
const TMS_MIXER: u32 = 5;
/// `MixerMessage.channelDisplayStateChanged` — the reply to a state request.
const CHANNEL_DISPLAY_STATE_CHANGED: u32 = 7;
/// `ChannelDisplayStateChanged.record` — repeated `ChannelDisplayState`.
const RECORD: u32 = 2;

/// One decoded `ChannelDisplayState` row.
#[derive(Debug, Default)]
struct ChannelState {
    id_enum: u64,
    is_stereo: bool,
    fader_level: f32,
    mute_active: bool,
    solo_active: bool,
    link_to_master_lvl: bool,
    mono_out_active: bool,
    global_eq_active: bool,
    cut_active: bool,
    pre_enabled: bool,
    /// `sourceActive` map entries (key → active) — AUX / Bluetooth / chain feeds.
    sources: Vec<(u64, bool)>,
}

/// Decode one `ChannelDisplayState` submessage.
///
/// Proto3 omits false/0 defaults, so every field is read as "absent ⇒ default"
/// rather than required — a channel with the fader at unity and nothing engaged
/// legitimately serializes to just its `idEnum`.
fn decode_channel(buf: &[u8]) -> ChannelState {
    let mut c = ChannelState::default();
    for (f, v) in proto::parse(buf) {
        match f {
            1 => c.id_enum = v.as_u64().unwrap_or(0),
            2 => c.is_stereo = v.as_u64().unwrap_or(0) != 0,
            3 => c.fader_level = v.as_f32().unwrap_or(0.0),
            4 => c.mute_active = v.as_u64().unwrap_or(0) != 0,
            5 => c.solo_active = v.as_u64().unwrap_or(0) != 0,
            6 => c.link_to_master_lvl = v.as_u64().unwrap_or(0) != 0,
            7 => c.mono_out_active = v.as_u64().unwrap_or(0) != 0,
            8 => c.global_eq_active = v.as_u64().unwrap_or(0) != 0,
            9 => {
                // SourceActiveEntry { key = 1, value = 2 } — a proto3 map entry.
                if let Some(b) = v.as_bytes() {
                    let e = proto::parse(b);
                    let key = e
                        .iter()
                        .find(|(g, _)| *g == 1)
                        .and_then(|(_, w)| w.as_u64())
                        .unwrap_or(0);
                    let val = e
                        .iter()
                        .find(|(g, _)| *g == 2)
                        .and_then(|(_, w)| w.as_u64())
                        .unwrap_or(0);
                    c.sources.push((key, val != 0));
                }
            }
            10 => c.cut_active = v.as_u64().unwrap_or(0) != 0,
            11 => c.pre_enabled = v.as_u64().unwrap_or(0) != 0,
            _ => {}
        }
    }
    c
}

/// Pull every `ChannelDisplayState` out of a reply stream body, if it carries one.
fn channels_in(body: &[u8]) -> Vec<ChannelState> {
    let top = proto::parse(body);
    let Some(mixer) = proto::first_bytes(&top, TMS_MIXER) else {
        return Vec::new();
    };
    let mixer_fields = proto::parse(mixer);
    let Some(changed) = proto::first_bytes(&mixer_fields, CHANNEL_DISPLAY_STATE_CHANGED) else {
        return Vec::new();
    };
    proto::parse(changed)
        .iter()
        .filter(|(f, _)| *f == RECORD)
        .filter_map(|(_, v)| match v {
            Val::Bytes(b) => Some(decode_channel(b)),
            _ => None,
        })
        .collect()
}

/// HW PROBE (WRITE): set the global master level — `MixerMessage.SetMasterLevel`,
/// TMS 5 field 16.
///
/// Exists to settle A1 — is the master volume control in the USB 1/2 path? —
/// WITHOUT a hand on the physical knob. Set the level over USB, re-capture USB
/// 1/2, compare against a null control.
///
/// **This cannot confirm its own write.** TMS-5 reads are unserved on fw 1.8.45
/// (`probe_mixer_state` got silence from all five requests), so
/// there is no reply and no notification to wait for. Verify OUT OF BAND: take a
/// `--device-backup` and read `settingsBackup.mixerSaveData.masterVolume`.
/// Treating "no error" as "it landed" is exactly the false positive this
/// experiment exists to avoid.
///
/// `masterVolume` is a GLOBAL setting: the caller MUST restore the original value,
/// or the unit is left quieter than its owner set it.
///
/// Calls [`Session::begin_live_edit`] before writing, closing the confound a past
/// attempt on an unwarmed (merely-`drain_until_quiet`d) line left open. Retested
/// 2026-07-26 on a warmed session: still no observable change (see open-questions.md
/// A1) — narrows but does not close whether TMS 5 is write-dead.
pub fn probe_set_master_level(level: f32) -> Result<String, String> {
    // Guard a fat-fingered magnitude (`5` for `0.5`). The field is a 0..1
    // normalised float; >0.75 is REFUSED rather than clamped, because silently
    // substituting a level the caller did not ask for would corrupt the very
    // measurement this exists to take. 0.75 still admits any plausible restore.
    if !level.is_finite() || !(0.0..=0.75).contains(&level) {
        return Err(format!(
            "master level {level} outside accepted 0.0..=0.75 \
             (normalised float; >0.75 refused so a typo cannot blast the outputs)"
        ));
    }
    let mut s = Session::connect()?;
    s.drain_until_quiet(300, 20)?;
    // Warm the live-controller heartbeat before writing — closes the confound
    // documented above (this write used to ride a merely-drained line).
    s.begin_live_edit()?;
    // Dump whatever comes back purely for the record — a reply is NOT expected and
    // its absence is NOT a failure, so the result is never derived from it.
    let dump = s.send_and_dump(&proto::set_master_level(level), 600)?;
    Ok(format!(
        "SetMasterLevel({level}) sent — NOT confirmed (TMS 5 emits no reply).\n\
         Verify: probe --device-backup → settingsBackup.mixerSaveData.masterVolume\n\
         reply stream (informational only):\n{dump}"
    ))
}

/// HW PROBE (read-only): ask the unit for its output-mixer channel strips.
///
/// Prints the raw reply-stream summary FIRST (so a silent ignore is
/// distinguishable from an error reply from a real answer) and then the decoded
/// rows if any arrived. Both halves are printed in one device visit deliberately
/// — every HID open risks the `0xe00002c5` lockout, and a retry re-arms it.
///
/// A reply with no TMS-5 stream is a genuine negative for the fw 1.7.75 schema
/// this was built from; it is NOT proof the mixer is unreadable on the unit's
/// firmware. Record the firmware version alongside the result.
pub fn probe_mixer_state() -> Result<String, String> {
    let mut s = Session::connect()?;
    // MANDATORY: fire reads only on a QUIET line. The device DROPS a request that
    // rides behind the handshake's ~480-frame preset-list/ProductProfile flood
    // (same rule as `read_slot_preset_json`), so an undrained probe produces a
    // false negative — it did on the first run of this experiment.
    s.drain_until_quiet(300, 20)?;

    let mut out = String::new();
    // Sweep every read request in the schema. `reAmpModeRequest` (4) is itself a
    // TMS-5 request like the other four, NOT an independent control on a
    // different, known-working branch — a silent answer here is one more
    // untested instance of this branch, not proof the branch is unserved.
    let requests: [(u32, &str); 5] = [
        (1, "allChannelsDisplayDataRequest"),
        (2, "allChannelsDisplayStateRequest"),
        (3, "globalEqDisplayInfoRequest"),
        (4, "reAmpModeRequest"),
        (5, "masterLevelInfoRequest"),
    ];
    let mut any_mixer_reply = false;
    for (field, name) in requests {
        let dump = s.send_and_dump(&proto::mixer_request(field), 600)?;
        let saw_mixer = s
            .push_bodies()
            .iter()
            .any(|b| proto::first_bytes(&proto::parse(b), TMS_MIXER).is_some());
        any_mixer_reply |= saw_mixer;
        out += &format!(
            "TMS 5 / {name} (field {field}) → {}\n{dump}",
            if saw_mixer {
                "MIXER REPLY"
            } else {
                "no TMS-5 stream"
            }
        );
        s.drain_until_quiet(250, 8)?;
    }

    let channels: Vec<ChannelState> = s
        .push_bodies()
        .iter()
        .flat_map(|b| channels_in(b))
        .collect();

    if channels.is_empty() {
        out += &format!(
            "\n  NO ChannelDisplayState decoded. Any TMS-5 reply at all: {any_mixer_reply}.\n  \
             => {}\n",
            if any_mixer_reply {
                "TMS 5 IS served but the state reply has a different shape — decode it before \
                 concluding anything"
            } else {
                "TMS 5 read as NOT served on this attempt — mixer state is unreadable live over \
                 this protocol, so a leveling pre-flight must read `settingsBackup.mixerSaveData` \
                 from a device backup instead (see open-questions.md A1)"
            }
        );
        return Ok(out);
    }

    out += &format!("\n  decoded {} channel strip(s):\n", channels.len());
    out += "    idEnum  stereo  fader     mute   solo   mstLink  mono   gEQ    cut    pre    sources\n";
    for c in &channels {
        out += &format!(
            "    {:<7} {:<7} {:<9.4} {:<6} {:<6} {:<8} {:<6} {:<6} {:<6} {:<6} {:?}\n",
            c.id_enum,
            c.is_stereo,
            c.fader_level,
            c.mute_active,
            c.solo_active,
            c.link_to_master_lvl,
            c.mono_out_active,
            c.global_eq_active,
            c.cut_active,
            c.pre_enabled,
            c.sources,
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a reply the way the device would frame one and assert the decoder
    /// walks TMS 5 → 7 → repeated 2 and reads every scalar at the right field.
    #[test]
    fn decodes_channel_rows_from_a_framed_reply() {
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut v = Vec::new();
            v.push(((field << 3) | 2) as u8);
            v.push(inner.len() as u8);
            v.extend_from_slice(inner);
            v
        }
        // ChannelDisplayState{ idEnum=3, faderLevel=0.5, muteActive=true,
        //                      linkToMasterLvl=true, sourceActive{7:true} }
        let mut ch = Vec::new();
        ch.extend_from_slice(&[0x08, 0x03]); // 1 varint 3
        ch.push(0x1d); // 3 fixed32
        ch.extend_from_slice(&0.5f32.to_le_bytes());
        ch.extend_from_slice(&[0x20, 0x01]); // 4 varint 1
        ch.extend_from_slice(&[0x30, 0x01]); // 6 varint 1
        let entry = [0x08, 0x07, 0x10, 0x01]; // key=7, value=true
        ch.extend_from_slice(&ld(9, &entry));

        let body = ld(
            TMS_MIXER,
            &ld(CHANNEL_DISPLAY_STATE_CHANGED, &ld(RECORD, &ch)),
        );

        let got = channels_in(&body);
        assert_eq!(got.len(), 1);
        let c = &got[0];
        assert_eq!(c.id_enum, 3);
        assert_eq!(c.fader_level, 0.5);
        assert!(c.mute_active);
        assert!(c.link_to_master_lvl);
        assert!(!c.solo_active, "omitted proto3 default must read false");
        assert_eq!(c.sources, vec![(7, true)]);
    }

    /// A reply that carries no mixer branch must decode to nothing rather than
    /// inventing a default row — that distinction IS the experiment's result.
    #[test]
    fn unrelated_reply_yields_no_channels() {
        assert!(channels_in(&[0x12, 0x02, 0x08, 0x01]).is_empty()); // TMS 2, not 5
    }
}
