//! The webview's command surface, pinned (#267, #268, #270).
//!
//! Three files must agree, and nothing else would notice when they drift:
//! - `app_commands.rs`: every command the app registers;
//! - `capabilities/default.json`: the `allow-<command>` grants, i.e. what the webview may
//!   actually invoke (everything else is denied since `build.rs` declares the app manifest);
//! - `src/lib/invoke.ts`: the frontend's typed wrappers, one `invokeCommand("<command>")` each.
//!
//! The rule: a command is registered exactly when the frontend calls it, and granted
//! exactly then. A command no screen calls is deleted rather than kept registered and
//! denied: the unwired surface was dropped, so a new one fails here instead of quietly
//! growing IPC surface.

use std::collections::BTreeSet;

macro_rules! names {
    (() [$($m:ident :: $f:ident),* $(,)?]) => {
        &[$(stringify!($f)),*]
    };
}

/// Every registered app command, by name.
const ALL: &[&str] = with_app_commands!(names!());

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
fn every_registered_command_has_a_frontend_caller() {
    let unwired: Vec<_> = set(ALL).difference(&frontend_commands()).cloned().collect();
    assert!(
        unwired.is_empty(),
        "registered with no frontend caller (wire it into a screen or delete it): {unwired:?}",
    );
}
