//! Tauri app bootstrap — builder, logger, menu, setup, `run()` entry point.
// Commands are registered by path through `app_invoke_handler!` (`lib.rs`), from the
// one list in `app_commands.rs`.
use crate::{monitor, watcher, AppState};

/// Mirrors `scripts/tauri-dev-env.sh`'s Wayland workaround (see its header for the
/// hardware evidence: WebKitGTK's native-Wayland window sometimes maps but never
/// paints on some KDE/GNOME + GPU driver combos). That script only wraps `tauri
/// dev`; a packaged bundle gets no wrapper, so the shipped binary needs its own
/// copy or a released user hits a blank window on first launch. `-z
/// "${GDK_BACKEND:-}"`: an EMPTY `GDK_BACKEND` counts as unset, and an explicit
/// value (e.g. `wayland`) is left alone. Pure so the four cases are unit-testable
/// without a real session.
#[cfg(target_os = "linux")]
fn should_force_x11(session_type: Option<&str>, gdk_backend: Option<&str>) -> bool {
    session_type == Some("wayland") && gdk_backend.is_none_or(str::is_empty)
}

/// Item id of the Help entry that opens the in-app bug-report dialog.
const REPORT_BUG_ID: &str = "report_bug";
/// Item id of the Linux Quit entry. A CUSTOM text item, not `SubmenuBuilder::quit`
/// — see `renders_on_gtk` for why the predefined one cannot be used here.
const QUIT_ID: &str = "quit";

/// One entry of the native menu. The predefined variants map 1:1 onto muda's
/// `PredefinedMenuItem`s; `Custom(id, label)` is a plain text item whose id
/// `on_menu_event` dispatches on.
///
/// The menu is described as DATA first, then interpreted by `add_entry`, so the
/// macOS/GTK divergence is unit-testable: `MenuBuilder` needs a live `AppHandle`
/// and cannot be exercised from a unit test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    About,
    Separator,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Maximize,
    Fullscreen,
    CloseWindow,
    Quit,
    Custom(&'static str, &'static str),
}

/// Whether muda's GTK backend actually RENDERS `entry` — the test oracle behind
/// `menu_plan`'s split, mirroring muda 0.19 `platform_impl/gtk/mod.rs`'s
/// `is_item_supported!`. Test-only: nothing in the shipped build needs to ask,
/// because each platform's plan already names exactly what that platform draws.
#[cfg(test)]
const fn renders_on_gtk(entry: Entry) -> bool {
    matches!(
        entry,
        Entry::Separator
            | Entry::Cut
            | Entry::Copy
            | Entry::Paste
            | Entry::SelectAll
            | Entry::About
            | Entry::Custom(..)
    )
}

/// The native menu as data: `(submenu title, entries)`, in order.
///
/// **Why the menu is platform-split at all:** muda 0.19's GTK backend gates every
/// append behind `is_item_supported!`, which admits only `Separator | Copy | Cut |
/// Paste | SelectAll | About`. Every other predefined item is **silently dropped**
/// — not disabled, not an error. (Tauri documents the casualties one at a time,
/// e.g. `SubmenuBuilder::quit`: "Platform-specific: **Linux:** Unsupported.")
///
/// Shipping one AppKit-shaped menu therefore gave Linux an app submenu of About
/// plus two orphaned separators and NO QUIT, an Edit submenu that lost Undo/Redo
/// behind a leading separator, and a Window submenu that rendered entirely empty.
/// Custom text items are always drawn, which is why Linux's Quit is one.
///
/// `gtk` is a parameter rather than a `cfg!` so both plans are constructible —
/// and therefore assertable — from either platform's test run.
fn menu_plan(gtk: bool) -> &'static [(&'static str, &'static [Entry])] {
    if gtk {
        // No Window submenu: every one of its items is a GTK casualty, so it
        // would render as an empty menu. Window management is the WM's job on
        // Linux anyway. Edit keeps only what GTK draws, with no leading
        // separator left behind by the dropped Undo/Redo.
        &[
            (
                "TMP Companion",
                &[
                    Entry::About,
                    Entry::Separator,
                    Entry::Custom(QUIT_ID, "Quit TMP Companion"),
                ],
            ),
            (
                "Edit",
                &[Entry::Cut, Entry::Copy, Entry::Paste, Entry::SelectAll],
            ),
            ("Help", &[Entry::Custom(REPORT_BUG_ID, "Report a Bug…")]),
        ]
    } else {
        &[
            (
                "TMP Companion",
                &[
                    Entry::About,
                    Entry::Separator,
                    Entry::Hide,
                    Entry::HideOthers,
                    Entry::ShowAll,
                    Entry::Separator,
                    Entry::Quit,
                ],
            ),
            (
                "Edit",
                &[
                    Entry::Undo,
                    Entry::Redo,
                    Entry::Separator,
                    Entry::Cut,
                    Entry::Copy,
                    Entry::Paste,
                    Entry::SelectAll,
                ],
            ),
            (
                "Window",
                &[
                    Entry::Minimize,
                    Entry::Maximize,
                    Entry::Separator,
                    Entry::Fullscreen,
                    Entry::CloseWindow,
                ],
            ),
            ("Help", &[Entry::Custom(REPORT_BUG_ID, "Report a Bug…")]),
        ]
    }
}

/// Interpret one `Entry` onto a `SubmenuBuilder`. The single place that maps the
/// plan onto muda, so the data above stays the one source of truth.
fn add_entry<'m, R: tauri::Runtime, M: tauri::Manager<R>>(
    b: tauri::menu::SubmenuBuilder<'m, R, M>,
    entry: Entry,
    about: &tauri::menu::AboutMetadata<'m>,
) -> tauri::menu::SubmenuBuilder<'m, R, M> {
    match entry {
        Entry::About => b.about_with_text("About TMP Companion", Some(about.clone())),
        Entry::Separator => b.separator(),
        Entry::Undo => b.undo(),
        Entry::Redo => b.redo(),
        Entry::Cut => b.cut(),
        Entry::Copy => b.copy(),
        Entry::Paste => b.paste(),
        Entry::SelectAll => b.select_all(),
        Entry::Hide => b.hide(),
        Entry::HideOthers => b.hide_others(),
        Entry::ShowAll => b.show_all(),
        Entry::Minimize => b.minimize(),
        Entry::Maximize => b.maximize(),
        Entry::Fullscreen => b.fullscreen(),
        Entry::CloseWindow => b.close_window(),
        Entry::Quit => b.quit(),
        Entry::Custom(id, label) => b.text(id, label),
    }
}

pub fn run() {
    #[cfg(target_os = "linux")]
    if should_force_x11(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("GDK_BACKEND").ok().as_deref(),
    ) {
        // SAFETY: called before any thread is spawned (the very top of `run()`,
        // ahead of `tauri::Builder`), so there is no concurrent env access.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }

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
                // Cap the on-disk log file so it can't grow unbounded across a long
                // uptime; keep a few rotated backups. Rotation is checked at init, so
                // this also retroactively caps a previous version's already-uncapped
                // log the next time the app starts.
                .max_file_size(2 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .build(),
        )
        // In-app auto-update (checks the GitHub latest.json endpoint) + the process
        // plugin it relaunches through to apply a downloaded update.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Every app command, from the one list in `app_commands.rs` (#270). Which of them the
        // webview may invoke is `capabilities/default.json`'s call (#268).
        .invoke_handler(app_invoke_handler!())
        // Native menu, per platform (`menu_plan`). Setting a menu replaces the
        // default, so the standard submenus are rebuilt explicitly (Edit is
        // load-bearing — copy/paste in the rename fields ride its predefined
        // items, and those ARE among the few GTK renders). The non-affiliation
        // notice lives in the standard "About TMP Companion" panel via
        // AboutMetadata; the leveling explainer is in-app (Level tab). Help
        // carries one item, "Report a Bug…", which just emits tmp://open-bug-report
        // for the frontend to open its own dialog (see on_menu_event below).
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
            // Build from `menu_plan` rather than inline chains: GTK silently
            // DROPS most predefined items (see `renders_on_gtk`), so the two
            // platforms need different content, and the plan keeps that
            // divergence in one testable place.
            let mut submenus = Vec::new();
            // `target_os = "linux"`, not `not(macos)`: muda's Windows backend
            // has no `is_item_supported` gate and draws Quit/Undo/Redo/Window
            // fine, so the in-flight Windows port must NOT inherit the stripped
            // plan. GTK is also muda's BSD backend, but this crate's hidraw and
            // ALSA transports are Linux-only, so a BSD build cannot reach a menu.
            for (title, entries) in menu_plan(cfg!(target_os = "linux")) {
                let mut sub = SubmenuBuilder::new(handle, *title);
                for entry in *entries {
                    sub = add_entry(sub, *entry, &about);
                }
                submenus.push(sub.build()?);
            }
            let mut menu = MenuBuilder::new(handle);
            for sub in &submenus {
                menu = menu.item(sub);
            }
            menu.build()
        })
        .on_menu_event(|app, event| {
            if event.id() == REPORT_BUG_ID {
                use tauri::Emitter;
                let _ = app.emit("tmp://open-bug-report", ());
            }
            // Only ever fires on GTK: macOS uses the predefined Quit, which the
            // system handles and which never reaches this handler.
            if event.id() == QUIT_ID {
                app.exit(0);
            }
        })
        .setup(|app| {
            // Confirms the logger is live (and gives the log file a deterministic
            // first line). Subsequent warn/error from the device + frontend paths
            // append here too.
            log::info!("TMP Companion {} started", env!("CARGO_PKG_VERSION"));
            // Dock icon for `tauri dev` (the raw binary has no bundle .icns).
            #[cfg(target_os = "macos")]
            crate::dock::set_dock_icon();
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
            // The persisted playback-channels choice → the meter every output-side
            // measurement hub reads (Settings updates it on change).
            crate::profiles::sync_meter_channels(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::should_force_x11;

    #[test]
    fn wayland_with_no_gdk_backend_forces_x11() {
        assert!(should_force_x11(Some("wayland"), None));
    }

    #[test]
    fn wayland_with_empty_gdk_backend_forces_x11() {
        // An empty string is what `std::env::var(...).ok()` yields for a var set
        // to "" — must be treated the same as unset, matching the shell script's
        // `-z "${GDK_BACKEND:-}"`.
        assert!(should_force_x11(Some("wayland"), Some("")));
    }

    #[test]
    fn wayland_with_explicit_gdk_backend_is_left_alone() {
        assert!(!should_force_x11(Some("wayland"), Some("wayland")));
    }

    #[test]
    fn non_wayland_session_is_never_forced() {
        assert!(!should_force_x11(Some("x11"), None));
        assert!(!should_force_x11(None, None));
    }
}

/// Menu-plan tests. Deliberately NOT gated on `target_os`: the point is that both
/// plans are checkable from either platform's CI leg, so a macOS-only run still
/// catches a regression in the GTK plan (and vice versa).
#[cfg(test)]
mod menu_tests {
    use super::{menu_plan, renders_on_gtk, Entry, QUIT_ID, REPORT_BUG_ID};

    const GTK: bool = true;
    const APPKIT: bool = false;

    /// The regression this whole split exists for: on GTK, every entry we emit
    /// must be one muda actually draws. A predefined item that GTK drops leaves
    /// no error behind — only a hole in the menu.
    #[test]
    fn gtk_plan_emits_only_entries_gtk_renders() {
        for (title, entries) in menu_plan(GTK) {
            for entry in *entries {
                assert!(
                    renders_on_gtk(*entry),
                    "{title:?} carries {entry:?}, which muda's GTK backend drops silently",
                );
            }
        }
    }

    /// The macOS plan is what the GTK one must NOT be. If this ever stops holding,
    /// the two plans have converged and the split is pointless — or someone has
    /// quietly removed macOS's Quit.
    #[test]
    fn the_appkit_plan_would_lose_items_on_gtk() {
        let dropped: Vec<Entry> = menu_plan(APPKIT)
            .iter()
            .flat_map(|(_, entries)| entries.iter().copied())
            .filter(|e| !renders_on_gtk(*e))
            .collect();
        assert!(
            dropped.contains(&Entry::Quit),
            "the macOS plan must still carry a predefined Quit; got {dropped:?}",
        );
    }

    /// Quit must stay reachable on Linux. It is a CUSTOM item precisely because
    /// the predefined one is dropped, so this also pins that choice.
    #[test]
    fn gtk_plan_keeps_a_reachable_quit() {
        let has_quit = menu_plan(GTK)
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .any(|e| matches!(e, Entry::Custom(id, _) if *id == QUIT_ID));
        assert!(has_quit, "Linux menu has no way to quit the app");
    }

    /// "Report a Bug…" is the ONLY entry point to `BugReportDialog`, on every
    /// platform — losing it strands the whole bug-report flow.
    #[test]
    fn every_plan_keeps_the_bug_report_entry() {
        for gtk in [GTK, APPKIT] {
            let has_report = menu_plan(gtk)
                .iter()
                .flat_map(|(_, entries)| entries.iter())
                .any(|e| matches!(e, Entry::Custom(id, _) if *id == REPORT_BUG_ID));
            assert!(has_report, "gtk={gtk}: no Report a Bug… item");
        }
    }

    /// An empty submenu is exactly what shipped: Window's five items were all GTK
    /// casualties, leaving a title that opens onto nothing.
    #[test]
    fn no_plan_has_a_submenu_that_renders_empty() {
        for gtk in [GTK, APPKIT] {
            for (title, entries) in menu_plan(gtk) {
                let visible = entries
                    .iter()
                    .filter(|e| !gtk || renders_on_gtk(**e))
                    .filter(|e| !matches!(e, Entry::Separator))
                    .count();
                assert!(visible > 0, "gtk={gtk}: {title:?} renders empty");
            }
        }
    }

    /// Separators must separate something. The shipped GTK app menu showed two in
    /// a row with nothing between them, because Hide/HideOthers/ShowAll/Quit had
    /// all been dropped out from between them.
    #[test]
    fn no_plan_has_a_dangling_or_doubled_separator() {
        for gtk in [GTK, APPKIT] {
            for (title, entries) in menu_plan(gtk) {
                let rendered: Vec<Entry> = entries
                    .iter()
                    .copied()
                    .filter(|e| !gtk || renders_on_gtk(*e))
                    .collect();
                assert_ne!(
                    rendered.first(),
                    Some(&Entry::Separator),
                    "gtk={gtk}: {title:?} opens with a separator",
                );
                assert_ne!(
                    rendered.last(),
                    Some(&Entry::Separator),
                    "gtk={gtk}: {title:?} ends with a separator",
                );
                for pair in rendered.windows(2) {
                    assert!(
                        pair != [Entry::Separator, Entry::Separator],
                        "gtk={gtk}: {title:?} has two adjacent separators",
                    );
                }
            }
        }
    }

    /// The macOS menu must come through this refactor byte-identical — it is the
    /// shipping platform and nothing here was meant to change it.
    #[test]
    fn the_appkit_plan_is_unchanged() {
        let titles: Vec<&str> = menu_plan(APPKIT).iter().map(|(t, _)| *t).collect();
        assert_eq!(titles, ["TMP Companion", "Edit", "Window", "Help"]);
        assert_eq!(
            menu_plan(APPKIT)[0].1,
            [
                Entry::About,
                Entry::Separator,
                Entry::Hide,
                Entry::HideOthers,
                Entry::ShowAll,
                Entry::Separator,
                Entry::Quit,
            ],
        );
        assert_eq!(
            menu_plan(APPKIT)[1].1,
            [
                Entry::Undo,
                Entry::Redo,
                Entry::Separator,
                Entry::Cut,
                Entry::Copy,
                Entry::Paste,
                Entry::SelectAll,
            ],
        );
        assert_eq!(
            menu_plan(APPKIT)[2].1,
            [
                Entry::Minimize,
                Entry::Maximize,
                Entry::Separator,
                Entry::Fullscreen,
                Entry::CloseWindow,
            ],
        );
        assert_eq!(
            menu_plan(APPKIT)[3].1,
            [Entry::Custom(REPORT_BUG_ID, "Report a Bug…")],
        );
    }
}
