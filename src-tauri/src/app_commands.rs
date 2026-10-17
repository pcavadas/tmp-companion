// The ONE list of Tauri commands this app registers (#270).
//
// Plain `//` comments only: `build.rs` `include!`s this file to hand the same names to
// `tauri_build`'s app manifest (#268), and an inner `//!` doc is illegal there.
//
// Three readers, one list:
// - `bootstrap::run` registers it on the real (Wry) runtime;
// - `e2e_server` registers it on the mock runtime, plus its own `e2e_*` helpers, which is
//   why every command is generic over `R: tauri::Runtime` rather than taking a bare
//   `tauri::AppHandle`;
// - `build.rs` turns each name into an `allow-…`/`deny-…` permission. Only the ones
//   `capabilities/default.json` grants can be invoked from the webview; the rest are
//   denied (the `command_surface` test in `lib.rs` pins which and why).
//
// Entries are `module::command`, `module` under `crate::commands`.

/// Expands `$cb!(($($args)*) [module::command, …])` with every app command.
macro_rules! with_app_commands {
    ($cb:ident!($($args:tt)*)) => {
        $cb!(($($args)*) [
            device::app_info,
            device::connect_device,
            device::list_presets,
            settings::list_pickup_topologies,
            settings::get_store,
            settings::save_profiles,
            settings::save_targets,
            settings::set_playback_level,
            settings::set_playback_channels,
            settings::set_auto_install_updates,
            settings::set_preset_instrument,
            level_preset::calibrate_profile,
            level_preset::level_preset,
            level_preset::list_level_blocks,
            level_preset::cancel_preset_leveling,
            level_scenes::level_scenes_apply_batched,
            level_scenes::list_scene_level_handles,
            level_scenes::cancel_scene_leveling,
            songs::list_songs,
            songs::remove_song,
            songs::create_song_full,
            songs::update_song_full,
            doctor::doctor_check,
            doctor::cancel_doctor_check,
            level_footswitch::level_footswitches_apply,
            level_footswitch::list_footswitch_scene_contexts,
            level_footswitch::cancel_footswitch_leveling,
            presets::read_active_preset,
            presets::current_graph,
            presets::read_library_via_backup,
            setlists::read_setlists,
            setlists::list_setlist_songs,
            setlists::add_setlist,
            setlists::rename_setlist,
            setlists::remove_setlist,
            setlists::remove_setlist_song,
            setlists::move_setlist_song,
            setlists::add_setlist_songs,
            copy_apply::copy_apply,
            support::save_support_bundle,
            support::build_support_bundle,
        ])
    };
}
