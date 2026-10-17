//! High-level TMP control session: handshake, preset enumeration, and the
//! leveling primitives (load preset, toggle re-amp, set preset level, save).
//!
//! The load-bearing USB-protocol invariants (batchStatus grouping, re-amp latch
//! rules, the HID seize, slot +1 addressing, the capture window) are written up
//! in `notes/protocol.md`.
//!
//! Builds on `hid` (transport) + `proto` (wire codec). Replicates the request
//! sequence that is known-good against a real device. The
//! handshake REUSES `batchStatus` in the device's exact groups (1; 2×7; 3; 4) —
//! NOT a per-request counter: incrementing per request makes the device go silent
//! after the preset lists. Setters and heartbeats omit `batchStatus` entirely.
//!
//! Layout. The TRANSPORT SET owns the wire and the clock: `link` (the transport seam,
//! `TrackedHid`, the send/pump/collect primitives), `timing` (every window, by name),
//! `handshake` (connect + burst), `backup_stream` and `diag`. The command and read
//! modules (`lists`, `reads`, `preset`, `reamp`, `scene`, `structural`, `ftsw`, `songs`)
//! call link primitives with `timing` consts only, and `decode`, `graph` and `types` are
//! pure. `boundary_tests` enforces the split and `wire_golden` pins every command's
//! requests and windows. This file keeps the `Session` struct and the shared field
//! numbers; every item keeps its `crate::session::X` path through the re-exports below.

use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

use crate::error::DeviceError;
use crate::hid::HidTransport;
use crate::proto::{self, Val};
use crate::slot::{DeviceSlot, ListIndex};

// The transport set: only these files touch `self.hid`, the clock or a literal pump
// window (`boundary_tests` holds the rest to it).
mod backup_stream;
mod diag;
mod handshake;
mod link;
mod timing;

// Commands, reads and pure decoding, on the link primitives and `timing` consts.
mod decode;
mod ftsw;
mod graph;
mod lists;
mod preset;
mod reads;
mod reamp;
mod scene;
mod songs;
mod structural;
mod types;

// Every item keeps its `crate::session::X` path; each glob re-exports at the item's own
// visibility.
pub(crate) use decode::*;
pub use graph::*;
pub use handshake::*;
#[cfg(feature = "e2e")]
pub use link::e2e_transport;
use link::*;
pub(crate) use reads::*;
#[allow(unused_imports)] // the counters and the state type are read by tests and the e2e build
pub use reamp::*;
pub use songs::*;
pub(crate) use timing::*;
pub use types::*;

pub struct Session {
    hid: TrackedHid,
    /// Set once the handshake's batch-3 drain is sent; from then on every request must
    /// be answered on this connection (see [`proto::answered_at_once`]).
    handshake_done: bool,
    /// Raw input reports accumulated during the current high-level operation.
    /// Reassembled cumulatively (a multi-packet stream can span pump windows).
    /// Cleared at the start of each operation that reads a reply.
    pub(crate) raw: Vec<Vec<u8>>,
    /// Firmware version the device reported during the handshake (in reply to
    /// the in-burst `currentFwRequest` — a standalone request afterwards is
    /// rejected with ConnectionError). Captured at connect time because `raw`
    /// is cleared by later operations.
    fw_version: Option<String>,
}

/// FenderMessageTMS top-level oneof field numbers.
const TMS_PRESET: u32 = 2;
const TMS_SETTINGS: u32 = 3;
/// PresetMessage oneof field numbers.
const PRESET_LIST_RESPONSE: u32 = 5;
const PRESET_LOADED: u32 = 11;
const CURRENT_PRESET_INFO_CHANGED: u32 = 22;
const NODE_REPLACED: u32 = 40; // PresetMessage.nodeReplaced (device confirms a structural replace)
const NODE_REMOVED: u32 = 36; // PresetMessage.nodeRemoved (device confirms a structural remove)
const NODE_INSERTED: u32 = 33; // PresetMessage.nodeInserted (device confirms a structural insert) — SPEC field; the add-block inbound capture caught no reply, so this is unverified on HW (corroborate with a content read-back on the first run)
const NODE_JSON_RESPONSE: u32 = 120; // PresetMessage.nodeJsonResponse (a node's current JSON)
const PRESET_ERROR: u32 = 53; // PresetMessage.presetError (device REJECTS an edit — never save after this)
const PRESET_LEVEL_CHANGED: u32 = 77;
/// SettingsMessage oneof field numbers.
const SETTINGS_CURRENT_FW: u32 = 2; // SettingsMessage.currentFwResponse (reply to in-burst currentFwRequest)
const REAMP_SETTING: u32 = 30; // SettingsMessage.reampModeActive (echo of the setter)
const USB_SETTINGS_RESPONSE: u32 = 56; // SettingsMessage.usbSettingsResponse (UsbSettings)
const USB_SETTINGS_REQUEST: u32 = 55; // SettingsMessage.usbSettingsRequest (EmptyRequestMessage)
const USB_REAMP_ACTIVE: u32 = 1; // UsbSettings.reampModeActive (ReampModeActive{ value = 1 })
/// FenderMessageTMS.mixerMessage and its re-amp read pair.
const TMS_MIXER: u32 = 5;
const MIXER_REAMP_REQUEST: u32 = 4; // MixerMessage.reAmpModeRequest
const MIXER_REAMP_RESPONSE: u32 = 9; // MixerMessage.reAmpModeResponse{ isActive = 1 }

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod wire_golden;

#[cfg(test)]
mod tests;
