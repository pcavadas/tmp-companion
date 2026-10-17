//! Tauri command layer — one module per feature area, each addressed by its path
//! (`commands::<area>::<item>`); `lib.rs` re-exports only `copy_apply::CopyRepl` (see there why).
//!
//! Commands are registered by path from the one list in `app_commands.rs`
//! (`crate::commands::<module>::<command>`); `#[tauri::command]` re-exports each
//! command's generated `__cmd__*` / `__tauri_command_name_*` macros next to the fn, so
//! no `#[macro_use]` cascade is needed.

/// A streamed progress row's state, shared by the Level (scene and footswitch) and Doctor
/// channels. Serializes to the same lowercase words the rows always carried, so the wire
/// shape (`"active" | "done" | "error" | "cancelled"`) is unchanged; the Doctor channel
/// never sends `cancelled` (its TS mirror leaves it out).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressStatus {
    Active,
    Done,
    Error,
    Cancelled,
}

pub(crate) mod bulk_replace;
pub(crate) mod copy_apply;
pub(crate) mod device;
pub(crate) mod doctor;
pub(crate) mod held_edit;
pub(crate) mod level_footswitch;
pub(crate) mod level_preset;
pub(crate) mod level_scenes;
pub(crate) mod library;
pub(crate) mod presets;
pub(crate) mod setlists;
pub(crate) mod settings;
pub(crate) mod songs;
pub(crate) mod support;
