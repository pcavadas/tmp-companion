//! The transport boundary: only the transport set (`mod`, `link`, `timing`, `handshake`,
//! `backup_stream`, `diag`) may touch `self.hid`, the clock or a literal pump window.
//! Every other session file calls the link primitives with a named `timing` const, so a
//! timing change is always a change to a transport file, and the gate map can tell the
//! two apart.

/// Session files outside the transport set, by name and source.
const COMMAND_FILES: &[(&str, &str)] = &[
    ("decode.rs", include_str!("decode.rs")),
    ("ftsw.rs", include_str!("ftsw.rs")),
    ("graph.rs", include_str!("graph.rs")),
    ("lists.rs", include_str!("lists.rs")),
    ("preset.rs", include_str!("preset.rs")),
    ("reads.rs", include_str!("reads.rs")),
    ("reamp.rs", include_str!("reamp.rs")),
    ("scene.rs", include_str!("scene.rs")),
    ("songs.rs", include_str!("songs.rs")),
    ("structural.rs", include_str!("structural.rs")),
    ("types.rs", include_str!("types.rs")),
];

/// The I/O primitives whose arguments carry a pump window or a round cap.
const PRIMITIVES: &[&str] = &[
    "pump_silent",
    "pump_collect_alive",
    "pump_until_stable",
    "keepalive_pump",
    "send_and_collect_alive",
    "send_and_collect",
    "transact_eager",
    "transact_chunked",
    "transact",
    "send_chunked_collect",
    "drain_until_quiet",
];

/// `src` without `//` comments (doc comments included) and with all whitespace removed,
/// so a call split over lines reads as one token run.
fn squeeze(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The top-level arguments of the call whose `(` ends at byte `open` in `s`.
fn call_args(s: &str, open: usize) -> Vec<&str> {
    let (mut depth, mut start, mut args) = (1, open, Vec::new());
    for (i, c) in s[open..].char_indices() {
        let at = open + i;
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    args.push(&s[start..at]);
                    break;
                }
            }
            ',' if depth == 1 => {
                args.push(&s[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    args
}

/// Every boundary breach in one file's source, as readable strings.
fn breaches(src: &str) -> Vec<String> {
    let s = squeeze(src);
    let mut out = Vec::new();
    for banned in ["self.hid.", "Instant::now", "thread::sleep", "settle("] {
        if s.contains(banned) {
            out.push(format!("uses `{banned}`"));
        }
    }
    for prim in PRIMITIVES {
        let call = format!("{prim}(");
        let mut from = 0;
        while let Some(i) = s[from..].find(&call) {
            let at = from + i;
            from = at + call.len();
            let before = s[..at].chars().last();
            let is_name_start = !before.is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !is_name_start || s[..at].ends_with("fn") {
                continue;
            }
            for arg in call_args(&s, from) {
                if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_digit() || c == '_') {
                    out.push(format!("calls `{prim}` with the literal window `{arg}`"));
                }
            }
        }
    }
    out
}

#[test]
fn command_files_reach_the_device_only_through_link_with_named_windows() {
    let found: Vec<String> = COMMAND_FILES
        .iter()
        .flat_map(|(name, src)| {
            breaches(src)
                .into_iter()
                .map(move |b| format!("{name} {b}"))
        })
        .collect();
    assert!(
        found.is_empty(),
        "move the wait into the transport set or name it in timing.rs:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_check_catches_a_direct_hid_call_and_a_literal_window() {
    let src = "fn f(&mut self) {\n    self.hid\n        .transact(&b, 250)?;\n    self.pump_silent(\n        200,\n    )?;\n}";
    let found = breaches(src);
    assert!(found.iter().any(|b| b.contains("self.hid.")), "{found:?}");
    assert!(
        found
            .iter()
            .any(|b| b.contains("`pump_silent`") && b.contains("200")),
        "{found:?}"
    );
    assert!(breaches("fn f(&mut self) { self.pump_silent(LIVE_BEAT_MS)?; }").is_empty());
    assert!(breaches("/// pump_silent(200) in a doc\nfn g() {}").is_empty());
}

/// A new session file is either in the transport set or held to the boundary: it can't
/// land unchecked.
#[test]
fn every_session_file_is_in_the_transport_set_or_checked() {
    const TRANSPORT: &[&str] = &[
        "mod.rs",
        "link.rs",
        "timing.rs",
        "handshake.rs",
        "backup_stream.rs",
        "diag.rs",
    ];
    const TESTS: &[&str] = &["boundary_tests.rs", "tests.rs", "wire_golden.rs"];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session");
    for entry in std::fs::read_dir(dir).expect("session dir") {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !name.ends_with(".rs")
            || TRANSPORT.contains(&name.as_str())
            || TESTS.contains(&name.as_str())
        {
            continue;
        }
        assert!(
            COMMAND_FILES.iter().any(|(n, _)| *n == name),
            "{name}: add it to COMMAND_FILES, or to the transport set here and in the gate map"
        );
    }
}
