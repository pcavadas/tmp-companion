//! Tauri app bootstrap — builder, logger, menu, setup, `run()` entry point.
// The command fns arrive via the crate-root re-export glob; their generated
// `__cmd__*` macros ride in via `#[macro_use] mod commands` in `lib.rs` (declared
// before `mod bootstrap`, so its macros are in textual scope here).
use crate::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        // Frontend (console/error) + backend `log::*` records → OS log dir
        // (~/Library/Logs/dev.tmpcompanion.app/) and stdout. Gives render
        // crashes and device errors an on-disk trace.
        .plugin(
            tauri_plugin_log::Builder::new()
                // `Builder::new()` already ships DEFAULT_LOG_TARGETS = [Stdout, LogDir]
                // and `.target()` APPENDS — so re-adding the same two duplicated every
                // record (each line written 2× to BOTH the log file and stdout). Clear
                // the defaults first, then set exactly the two sinks we want.
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir { file_name: None },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            list_samples,
            list_pickup_topologies,
            get_store,
            save_profiles,
            save_targets,
            set_playback_level,
            calibrate_profile,
            level_preset,
            level_setlist,
            list_level_blocks,
            import_library,
            library_records,
            library_filter,
            bulk_dry_run,
            bulk_apply,
            bulk_revert,
            migration_scan,
            bulk_rename,
            create_variant,
            list_block_templates,
            save_block_template,
            spectrum_scan,
            audition_render,
            eq_match,
            rank_candidates,
            migration_plan,
            migration_apply,
            audit_loudness,
            list_snapshots,
            song_assign,
            song_clear,
            song_move,
            song_swap,
            level_scenes,
            level_scenes_apply,
            level_scenes_apply_batched,
            cancel_scene_leveling,
            doctor_check,
            cancel_doctor_check,
            doctor_apply,
            doctor_save,
            doctor_discard,
            cancel_preset_leveling,
            level_footswitches_apply,
            cancel_footswitch_leveling,
            read_active_preset,
            current_graph,
            request_scene_list,
            stop_live_sync,
            list_songs,
            load_preset_on_amp,
            delete_preset,
            move_preset,
            rename_save_preset,
            load_scene_on_amp,
            read_setlists,
            list_setlist_songs,
            add_song,
            rename_song,
            remove_song,
            set_song_notes,
            set_song_bpm,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_song,
            remove_setlist_song,
            move_setlist_song,
            create_song_full,
            update_song_full,
            add_setlist_songs,
            read_preset_scenes,
            scan_preset_scenes,
            cancel_scene_scan,
            read_library_via_backup,
            list_saved_blocks,
            list_user_irs,
            bulk_replace_live,
            cancel_bulk_replace,
            copy_apply,
            cancel_copy_apply
        ])
        // Native macOS menu. Setting a menu replaces the default, so the standard
        // App / Edit / Window submenus are rebuilt explicitly (Edit is load-bearing
        // — copy/paste in the rename fields ride its predefined items). The
        // non-affiliation notice lives in the standard "About TMP Companion" panel
        // via AboutMetadata; the leveling explainer is in-app (Level tab), so there
        // is no custom Help submenu.
        .menu(|handle| {
            use tauri::menu::{AboutMetadataBuilder, MenuBuilder, SubmenuBuilder};
            let about = AboutMetadataBuilder::new()
                .name(Some("TMP Companion"))
                // ponytail: omit `version` (NSAboutPanelOptionApplicationVersion, the
                // parenthetical) — macOS already shows the bundle's short version, so
                // setting it too renders the redundant `Version 0.1.0 (0.1.0)`.
                .short_version(Some(env!("CARGO_PKG_VERSION")))
                // The dev binary has no bundle icon, so the panel would show a
                // generic folder — set it explicitly (same art as the Dock icon).
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/dock.png")).ok())
                // macOS draws `copyright` as the small line and `credits` as the
                // body. Copyright = the real © line; the affiliation + trademark
                // notice is the body.
                .copyright(Some("© 2026 Pedro Cavadas"))
                .credits(Some(
                    "Fender, Tone Master Pro, and other amp, cabinet, and effect \
                     names are trademarks of their respective owners, used \
                     nominatively to describe compatibility and lineage. \
                     Independent project — not affiliated with Fender Musical \
                     Instruments Corporation.",
                ))
                .build();
            let app_menu = SubmenuBuilder::new(handle, "TMP Companion")
                .about_with_text("About TMP Companion", Some(about))
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .quit()
                .build()?;
            let edit = SubmenuBuilder::new(handle, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let window = SubmenuBuilder::new(handle, "Window")
                .minimize()
                .maximize()
                .separator()
                .fullscreen()
                .close_window()
                .build()?;
            MenuBuilder::new(handle)
                .items(&[&app_menu, &edit, &window])
                .build()
        })
        .setup(|app| {
            // Confirms the logger is live (and gives the log file a deterministic
            // first line). Subsequent warn/error from the device + frontend paths
            // append here too.
            log::info!("TMP Companion {} started", env!("CARGO_PKG_VERSION"));
            // Dock icon for `tauri dev` (the raw binary has no bundle .icns).
            #[cfg(target_os = "macos")]
            dock::set_dock_icon();
            // Hotplug watcher: attach/detach events + dead-seize cleanup.
            use tauri::Manager;
            let session = app.state::<AppState>().session.clone();
            watcher::spawn(app.handle().clone(), session.clone());
            // Device monitor: app-level `connect_device` enables it, then the monitor
            // owns the idle seize with a dense ~250 ms heartbeat, publishes the startup
            // snapshot, and mirrors unsolicited unit pushes as tmp://live-preset /
            // live-scene / scene-list / signal-chain / sync. It coexists with commands
            // via the pause-then-ack protocol inside `lock_device_op`, and only opens
            // HID while `AppState.session` is None.
            monitor::spawn(app.handle().clone(), session);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
