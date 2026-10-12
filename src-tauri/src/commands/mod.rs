//! Tauri command layer — one module per feature area, glob-re-exported from `lib.rs`.
//!
//! Commands are registered by path from the one list in `app_commands.rs`
//! (`crate::commands::<module>::<command>`); `#[tauri::command]` re-exports each
//! command's generated `__cmd__*` / `__tauri_command_name_*` macros next to the fn, so
//! no `#[macro_use]` cascade is needed.

pub(crate) mod bulk_replace;
pub(crate) mod copy_apply;
pub(crate) mod device;
pub(crate) mod doctor;
pub(crate) mod edit_tools;
pub(crate) mod held_edit;
pub(crate) mod level_footswitch;
pub(crate) mod level_preset;
pub(crate) mod level_scenes;
pub(crate) mod library;
pub(crate) mod media;
pub(crate) mod migration;
pub(crate) mod presets;
pub(crate) mod setlists;
pub(crate) mod settings;
pub(crate) mod songs;
pub(crate) mod support;
