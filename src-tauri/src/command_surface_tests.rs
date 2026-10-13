//! The webview's command surface, pinned (#267, #268, #270).
//!
//! Three files must agree, and nothing else would notice when they drift:
//! - `app_commands.rs`: every command the app registers;
//! - `capabilities/default.json`: the `allow-<command>` grants, i.e. what the webview may
//!   actually invoke (everything else is denied since `build.rs` declares the app manifest);
//! - `src/lib/invoke.ts`: the frontend's typed wrappers, one `invokeCommand("<command>")` each.
//!
//! The rule: a command is granted exactly when the frontend calls it. A registered command
//! the frontend never calls is denied and must be listed in [`UNWIRED`], so adding one is a
//! deliberate choice rather than silent IPC surface.

use std::collections::BTreeSet;

macro_rules! names {
    (() [$($m:ident :: $f:ident),* $(,)?]) => {
        &[$(stringify!($f)),*]
    };
}

/// Every registered app command, by name.
const ALL: &[&str] = with_app_commands!(names!());

/// Registered but called from no screen, so DENIED to the webview: kept for the `probe`
/// CLI, the Rust e2e tests, or a feature that is not wired into a tab yet. Wiring one into
/// the UI means adding its `invoke.ts` wrapper and its `allow-…` grant, and removing it here.
const UNWIRED: &[&str] = &[
    // Song and setlist editors superseded by `create_song_full` / `update_song_full` /
    // `add_setlist_songs`, plus the slot-addressed song moves.
    "add_setlist_song",
    "add_song",
    "rename_song",
    "set_song_bpm",
    "set_song_notes",
    "song_assign",
    "song_clear",
    "song_move",
    "song_swap",
    // Offline-library bulk editing and migration (no tab).
    "bulk_apply",
    "bulk_dry_run",
    "bulk_rename",
    "bulk_revert",
    "create_variant",
    "import_library",
    "library_filter",
    "library_records",
    "list_block_templates",
    "list_snapshots",
    "migration_apply",
    "migration_plan",
    "migration_scan",
    "save_block_template",
    // Live block replace and its cancel; Copy's own cancel (Copy has no Stop button).
    "bulk_replace_live",
    "cancel_bulk_replace",
    "cancel_copy_apply",
    // Re-amp drivers with no screen.
    "audit_loudness",
    "audition_render",
    "eq_match",
    "rank_candidates",
    "spectrum_scan",
    // Legacy leveling entry points (the Level tab uses the batched apply).
    "level_scenes",
    "level_scenes_apply",
    "level_setlist",
    // Preset management with no screen. `delete_preset` and `move_preset` carry the
    // expected-name guard; `rename_save_preset` confirms the loaded name itself.
    "delete_preset",
    "move_preset",
    "rename_save_preset",
    // Device reads and loads with no screen.
    "cancel_scene_scan",
    "list_samples",
    "list_saved_blocks",
    "list_user_irs",
    "load_preset_on_amp",
    "load_scene_on_amp",
    "read_preset_scenes",
    "request_scene_list",
    "scan_preset_scenes",
    "stop_live_sync",
];

/// The command names `invoke.ts` calls: every `invokeCommand("<name>"` (with or without a
/// type argument). Every wrapper goes through `invokeCommand`, so this is the full set.
fn frontend_commands() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/invoke.ts");
    let src = std::fs::read_to_string(path).expect("read src/lib/invoke.ts");
    let mut out = BTreeSet::new();
    let mut rest = src.as_str();
    const CALL: &str = "invokeCommand";
    while let Some(i) = rest.find(CALL) {
        rest = &rest[i + CALL.len()..];
        let mut tail = rest;
        if let Some(t) = tail.strip_prefix('<') {
            // Skip a type argument, nested `<…>` included.
            let mut depth = 1;
            let end = t.char_indices().find(|&(_, c)| {
                depth += match c {
                    '<' => 1,
                    '>' => -1,
                    _ => 0,
                };
                depth == 0
            });
            let Some((end, _)) = end else { continue };
            tail = &t[end + 1..];
        }
        let Some(args) = tail.strip_prefix('(') else {
            continue;
        };
        let Some(lit) = args.trim_start().strip_prefix('"') else {
            continue;
        };
        let name: String = lit
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        if lit[name.len()..].starts_with('"') && !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

/// The app commands `capabilities/default.json` grants, back from `allow-<slug>` to the
/// command name (`tauri_build` slugs `_` to `-`). Plugin permissions (`core:…`) skipped.
fn granted_commands() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
    let cap: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read capability"))
            .expect("capability is JSON");
    cap["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .filter_map(|p| p.as_str())
        .filter(|p| !p.contains(':'))
        .map(|p| {
            p.strip_prefix("allow-")
                .unwrap_or_else(|| panic!("unexpected app permission {p:?}"))
                .replace('-', "_")
        })
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_command_list_has_no_duplicates() {
    assert_eq!(
        set(ALL).len(),
        ALL.len(),
        "a command is listed twice in app_commands.rs"
    );
}

#[test]
fn the_parser_finds_the_frontend_commands() {
    // Guards the scan itself: a parser that found nothing would pass the checks below.
    let fe = frontend_commands();
    assert!(
        fe.contains("connect_device") && fe.contains("level_preset"),
        "{fe:?}"
    );
    assert!(
        fe.len() > 30,
        "only {} invokeCommand() calls found: {fe:?}",
        fe.len()
    );
}

#[test]
fn every_frontend_command_is_registered() {
    let missing: Vec<_> = frontend_commands().difference(&set(ALL)).cloned().collect();
    assert!(
        missing.is_empty(),
        "invoke.ts calls unregistered commands: {missing:?}"
    );
}

#[test]
fn the_capability_grants_exactly_the_frontend_commands() {
    let (granted, fe) = (granted_commands(), frontend_commands());
    let extra: Vec<_> = granted.difference(&fe).collect();
    let missing: Vec<_> = fe.difference(&granted).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "capabilities/default.json grants {extra:?} that invoke.ts never calls, \
         and misses {missing:?} that it does",
    );
}

#[test]
fn every_registered_command_is_granted_or_listed_as_unwired() {
    let unwired = set(UNWIRED);
    assert_eq!(
        unwired.len(),
        UNWIRED.len(),
        "UNWIRED lists a command twice"
    );
    let expected: BTreeSet<String> = set(ALL).difference(&frontend_commands()).cloned().collect();
    let new: Vec<_> = expected.difference(&unwired).collect();
    let stale: Vec<_> = unwired.difference(&expected).collect();
    assert!(
        new.is_empty() && stale.is_empty(),
        "registered with no frontend caller and not in UNWIRED: {new:?}; \
         in UNWIRED but now called or unregistered: {stale:?}",
    );
}
