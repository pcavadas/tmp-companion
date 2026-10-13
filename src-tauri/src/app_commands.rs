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
            device::list_samples,
            device::stop_live_sync,
            settings::list_pickup_topologies,
            settings::get_store,
            settings::save_profiles,
            settings::save_targets,
            settings::set_playback_level,
            settings::set_playback_channels,
            settings::set_auto_install_updates,
            level_preset::calibrate_profile,
            level_preset::level_preset,
            level_preset::list_level_blocks,
            level_preset::cancel_preset_leveling,
            level_scenes::level_setlist,
            level_scenes::level_scenes,
            level_scenes::level_scenes_apply,
            level_scenes::level_scenes_apply_batched,
            level_scenes::list_scene_level_handles,
            level_scenes::cancel_scene_leveling,
            library::import_library,
            library::library_records,
            library::library_filter,
            library::bulk_dry_run,
            library::bulk_apply,
            library::bulk_revert,
            migration::migration_scan,
            migration::migration_plan,
            migration::migration_apply,
            migration::audit_loudness,
            migration::list_snapshots,
            edit_tools::bulk_rename,
            edit_tools::create_variant,
            edit_tools::list_block_templates,
            edit_tools::save_block_template,
            media::spectrum_scan,
            media::audition_render,
            media::eq_match,
            media::rank_candidates,
            songs::song_assign,
            songs::song_clear,
            songs::song_move,
            songs::song_swap,
            songs::list_songs,
            songs::add_song,
            songs::rename_song,
            songs::remove_song,
            songs::set_song_notes,
            songs::set_song_bpm,
            songs::create_song_full,
            songs::update_song_full,
            doctor::doctor_check,
            doctor::cancel_doctor_check,
            doctor::doctor_apply,
            doctor::doctor_save,
            doctor::doctor_discard,
            level_footswitch::level_footswitches_apply,
            level_footswitch::list_footswitch_scene_contexts,
            level_footswitch::cancel_footswitch_leveling,
            presets::read_active_preset,
            presets::current_graph,
            presets::request_scene_list,
            presets::load_preset_on_amp,
            presets::delete_preset,
            presets::move_preset,
            presets::rename_save_preset,
            presets::load_scene_on_amp,
            presets::read_preset_scenes,
            presets::scan_preset_scenes,
            presets::cancel_scene_scan,
            presets::read_library_via_backup,
            presets::list_saved_blocks,
            presets::list_user_irs,
            setlists::read_setlists,
            setlists::list_setlist_songs,
            setlists::add_setlist,
            setlists::rename_setlist,
            setlists::remove_setlist,
            setlists::add_setlist_song,
            setlists::remove_setlist_song,
            setlists::move_setlist_song,
            setlists::add_setlist_songs,
            bulk_replace::bulk_replace_live,
            bulk_replace::cancel_bulk_replace,
            copy_apply::copy_apply,
            copy_apply::cancel_copy_apply,
            support::save_support_bundle,
            support::build_support_bundle,
        ])
    };
}
