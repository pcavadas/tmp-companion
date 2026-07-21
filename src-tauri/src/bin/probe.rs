//! Headless re-validation kit for the TMP Companion (run with the device
//! plugged in and Pro Control closed — exclusive HID seize). Three commands:
//!
//!   probe                          connect (seize) + handshake → list My Presets
//!   probe --fw                     print the firmware version the device reports
//!   probe --scenes                  scene names for every preset (LoadPreset loop, ~0.5s each —
//!                                  DESTRUCTIVE: steps the unit through every preset; benchmark only)
//!   probe --scenes-full         full details (scenes+ftsw+graph) via LoadPreset → field-3 (DESTRUCTIVE)
//!   probe --scenes-passive      scene names for every preset via field-8 slot reads
//!                                  (retained validation kit — NO LoadPreset)
//!   probe --overlay-ab <slot|all> [--contexts N]
//!                                  EVIDENCE: SAVED per-scene overlay {bypass,outputLevel} vs
//!                                  the LIVE prepass state, per scene-amp, plus saved-vs-saved
//!                                  across N read contexts (before/after the prepass). slot is
//!                                  1-based (as --slot-json); 'all' = every non-empty preset.
//!                                  NON-DESTRUCTIVE (no writes/saves, no re-amp)
//!   probe --device-backup       FAST full-library read: BackupRequest streams a
//!                                  tar.lz4 of /data → extract normalDb.db3 → every
//!                                  preset + scene count via sqlite3 (one stream, no
//!                                  per-preset round-trips; archive RAM-only, temp DB
//!                                  deleted — no backup persisted)
//!   probe --slotread-x [slot…]  field-8 slot-read experiment matrix (NO LoadPreset —
//!                                  never changes the unit's selected preset); slots are
//!                                  1-based device slots, default first 3 non-empty
//!   probe --songs                  list every Song on the device (name / notes / BPM)
//!   probe --setlists               list every Setlist on the device (names)
//!   probe --setlist-songs          list the songs each Setlist contains (slots → names)
//!   probe --activegraph            print the current preset's live signal-chain graph
//!   probe --insert-active <NEW_MODEL_ID> [--group G1] [--after <FENDER_ID>] [--slot N] [--commit]
//!                                  ADD a block to the device's CURRENT ACTIVE preset via
//!                                  live insertNode (field 34). No --commit = DRY RUN
//!                                  (insert + verify, then revert); --commit = save in-place
//!   probe --lufs <wav>             measure a WAV's loudness (validate lufs.rs vs an oracle)
//!   probe --capture-reference <slot> <topology> <out.wav>
//!                                  OFFLINE-HARNESS: capture one full ~6.8s re-amp clip
//!                                  to build the adaptive-tuning corpus (DEVICE OP)
//!   probe --measure-adaptive <slot> <topology>
//!                                  DEVICE A/B: full-capture vs adaptive-capture LUFS +
//!                                  timings on one preset (the RE-BASELINE decision aid)
//!   probe --doctor <slots_csv> <topology>
//!                                  Doctor calibration sweep: capture each 0-based list
//!                                  index's BASE sound (Doctor tail) — or one scene via a
//!                                  `slot:scene` entry (0-based wire index) — print band
//!                                  profiles + metrics + fired diagnoses (JSON + table).
//!                                  READ-ONLY: loads + captures, never saves
//!   probe --doctor-calib <slots_csv> --stim <wav> --family <guitar|bass|bass-vi> [--labels <rules.json>] --out <report.json>
//!                                  CAPTURE-space Doctor recal: sweep each BASE sound through a
//!                                  REAL DI stimulus, measure profile + pre-onset noise floor +
//!                                  band coverage, and (with --labels {"rule":[slots]}) DERIVE
//!                                  proposed *_CAPTURE thresholds → a DETERMINISTIC JSON report.
//!                                  READ-ONLY: loads + captures, never saves
//!   probe --doctor-inject <slot> <gains_csv|none>   R5 defect-injection A/B (live EQ-10 insert,
//!                                   never saves; loads the slot)
//!   probe --doctor-defects <slot> [--out <report.json>]
//!                                  Versioned KNOWN-DEFECT fixture sweep: injects a committed
//!                                  table of named recipes (control/muddy/lost/washed/
//!                                  resonant_wah/resonant_peq/boxy_peq)
//!                                  one at a time into a clean preset's live edit buffer, checks
//!                                  each after-capture's fired verdicts against the recipe's
//!                                  must_fire/must_not_fire, prints a HIT/MISS/VIOLATION
//!                                  table. Never saves; loads the slot; ends re-amp OFF.
//!   probe --doctor-window-ab <slots_csv> --stim <wav> [--family <guitar|bass|bass-vi>] [--out <report.json>]
//!                                  CAPTURE-WINDOW A/B evidence arm: per slot, captures the
//!                                  oracle (full 6s stim + the pinned 2.5s oracle tail —
//!                                  ORACLE_TAIL_MS, deliberately NOT the production
//!                                  DOCTOR_TAIL_MS) vs a 3s-stim/1.5s-tail
//!                                  and a 4s-stim/1.5s-tail variant, reports band-dB/tilt/tail
//!                                  deltas + whether the fired-verdict set changed (the
//!                                  re-baseline decision aid — never self-consistent).
//!                                  LOADS the probed slots (LoadPreset via doctor_capture);
//!                                  never saves; ends re-amp OFF
//!   probe --stim-ab <slots_csv> <wavA> <wavB> [ref_level=0.5]
//!                                  DEVICE A/B: measure_c per preset with two stimuli →
//!                                  C/spread/ΔC table (playing-style sensitivity; capture
//!                                  a real DI clip for one side via --capture-input)
//!   probe --capture-wav <out.wav> [secs=12]
//!                                  DEVICE: save the dry instrument (USB-Out 3) to a
//!                                  48 kHz WAV while you play (the --stim-ab DI side)
//!   probe --scale-wav <in.wav> <out.wav> <target_lufs>
//!                                  NO-DEVICE: LUFS-match a WAV via the Tier-2
//!                                  calibration transform (0.99 peak cap)
//!   probe --measure-prefix-sweep <wav>
//!                                  NO-DEVICE: integrated LUFS over 0.5..6s prefixes vs full
//!   probe --measure-converge-replay <wav> <eps_lu> <stable_k> <preroll_ms>
//!                                  NO-DEVICE: replay a clip through reamp_measure's
//!                                  convergence state machine → exit time + Δ vs full
//!   probe --levelpreset <slot> <target_lufs> [save] [noverify]
//!                                  full one-shot leveling on the real device
//!                                  (stimulus via TMP_LEVELLER_STIMULUS)
//!   probe --measure-current <topology> [sceneSlot] [calibrationLUFS]
//!                                  measure current live state without changing levels
//!   probe --measure-scene <slot> <sceneSlot> <topology> [calibrationLUFS]
//!                                  load preset+scene, then measure without changing levels
//!                                  (slot = 0-BASED list index, same convention as
//!                                  --levelpreset — NOT the 1-based device userSlot)
//!   probe --capture-input [secs]   GATE 1: report USB-Out per-channel levels while
//!                                  you play (identifies the dry-instrument channel)
//!   probe --agc-test <slot>        GATE 2: full vs half re-amp inject on a CLEAN
//!                                  preset → is the inject AGC'd? (TMP_LEVELLER_STIMULUS)
//!
//! Exits non-zero on failure so it can gate dev scripts.

/// Look up a `--name value` CLI flag in `args`, by adjacent position (the
/// value is the arg right after the flag). Shared by the doctor-calib-style
/// subcommands, each of which parses several such flags.
///
/// A PRESENT flag with a missing value (flag is last, or immediately followed
/// by another `--flag`) is a usage error, exit 2 — distinguishable from an
/// absent flag (`None`), so a bare optional `--out` can never silently fall
/// back to a default instead of what the user asked for.
fn flag_arg(args: &[String], name: &str) -> Option<String> {
    let j = args.iter().position(|a| a == name)?;
    match args.get(j + 1) {
        Some(v) if !v.starts_with("--") => Some(v.clone()),
        _ => {
            eprintln!("[probe] {name} requires a value");
            std::process::exit(2);
        }
    }
}

/// Resolve an opspec argument: if it names an existing file, read its contents;
/// otherwise treat it as a literal inline JSON string. Empty when absent.
fn opspec_arg(arg: Option<&String>) -> String {
    match arg {
        Some(a) if std::path::Path::new(a).is_file() => {
            std::fs::read_to_string(a).unwrap_or_default()
        }
        Some(a) => a.clone(),
        None => String::new(),
    }
}

/// Minimal stderr logger: the shared library modules (leveller floor guards,
/// `audio::estimate_onset`, session retries) diagnose through `log::*`, which is
/// silently DROPPED without an installed logger — in the app tauri-plugin-log
/// owns it; in this CLI the diagnostics belong on stderr next to the eprintln
/// status lines. No timestamps/levels-config — probe is an attended tool.
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

/// `Name=Target` tail-args parser for the scene-leveling arms. Out-of-range `from`
/// (every optional arg omitted) is an empty list, NOT a panic — `args[from..]` sliced
/// past the end aborted `--level-preset-scenes <idx> <target> <topology>` (no save arg).
fn parse_target_overrides(args: &[String], from: usize) -> Vec<(String, f64)> {
    args.get(from..)
        .unwrap_or(&[])
        .iter()
        .filter_map(|a| {
            let (n, t) = a.split_once('=')?;
            Some((n.to_string(), t.parse::<f64>().ok()?))
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if log::set_logger(&StderrLog).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }

    if args.iter().any(|a| a == "--activegraph") {
        match tmp_companion_lib::probe_active_graph() {
            Ok(graph) => {
                print!("{graph}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--dump-currentpresetdata") {
        // --dump-currentpresetdata [slot]  — Tier-4: capture the live field-3
        // currentPresetDataChanged on a dense-heartbeat session, dump the full
        // decompressed JSON, and report whether scenes[] + ftsw are complete.
        // Optional `slot` (0-based list index) loads that preset first to trigger a
        // fresh push (non-destructive). Default output: /tmp/tmp_currentpresetdata.json.
        let slot: Option<u32> = args.get(i + 1).and_then(|s| s.parse().ok());
        let out = "/tmp/tmp_currentpresetdata.json";
        eprintln!("[probe] capturing currentPresetDataChanged (slot={slot:?})…");
        match tmp_companion_lib::probe_dump_preset_data(slot, out) {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--fw") {
        // Read-only: the firmware version the device pushed in the handshake.
        match tmp_companion_lib::probe_firmware_version() {
            Ok(v) => {
                println!("{v}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--device-backup") {
        eprintln!("[probe] device backup (BackupRequest → tar.lz4 stream → extract DB)…");
        match tmp_companion_lib::probe_device_backup() {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor-inject") {
        // --doctor-inject <slot> <gains_csv|none> [--block <fender_id>]
        // (R5 defect-injection A/B; --block overrides the EQ-10 insert vehicle —
        // e.g. a wah at its default cocked position for a resonant positive.)
        // Strict parsing: a typo'd slot or gain pair must not silently run a
        // DIFFERENT experiment (slot 0 / an empty control injection) that still
        // reports success — the same silent-wrong-arm class the calib matrix hit.
        let usage = || -> ! {
            eprintln!("usage: probe --doctor-inject <slot> <gains_csv|none> [--block <fender_id>]");
            std::process::exit(2);
        };
        let slot: u32 = match args.get(i + 1).and_then(|s| s.parse().ok()) {
            Some(s) => s,
            None => usage(),
        };
        let gains: Vec<(String, f64)> = match args.get(i + 2).map(String::as_str) {
            None | Some("none") => Vec::new(),
            Some(csv) if !csv.starts_with("--") => csv
                .split(',')
                .map(|kv| {
                    let Some((k, v)) = kv.split_once('=') else {
                        eprintln!("[probe] malformed gain pair '{kv}' (expected controlId=dB)");
                        usage();
                    };
                    match v.parse::<f64>() {
                        Ok(val) if val.is_finite() => (k.to_string(), val),
                        _ => {
                            eprintln!("[probe] malformed gain value in '{kv}' (expected a finite dB number)");
                            usage();
                        }
                    }
                })
                .collect(),
            Some(_) => usage(),
        };
        let block = flag_arg(&args, "--block");
        match tmp_companion_lib::probe_doctor_inject(slot, &gains, block.as_deref()) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor-defects") {
        // --doctor-defects <slot> [--out <report.json>]  (versioned defect-fixture sweep)
        let slot: u32 = match args.get(i + 1).and_then(|s| s.parse().ok()) {
            Some(s) => s,
            None => {
                eprintln!("usage: probe --doctor-defects <slot> [--out <report.json>]");
                std::process::exit(2);
            }
        };
        let out = flag_arg(&args, "--out");
        eprintln!("[probe] doctor-defects: slot {slot}…");
        match tmp_companion_lib::probe_doctor_defects(slot, out.as_deref()) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--replace-debug") {
        let slot: u32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let from = args.get(i + 2).cloned().unwrap_or_default();
        let to = args.get(i + 3).cloned().unwrap_or_default();
        match tmp_companion_lib::probe_replace_debug(slot, &from, &to) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--bulk-replace") {
        // --bulk-replace FROM TO SLOTS [--commit]   (SLOTS = comma list, 1-based)
        let from = args.get(i + 1).cloned().unwrap_or_default();
        let to = args.get(i + 2).cloned().unwrap_or_default();
        let slots: Vec<u32> = args
            .get(i + 3)
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        let commit = args.iter().any(|a| a == "--commit");
        if from.is_empty() || to.is_empty() || slots.is_empty() {
            eprintln!("usage: probe --bulk-replace <FROM_ID> <TO_ID> <slot,slot,…> [--commit]");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_bulk_replace(&from, &to, &slots, commit) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--roster") {
        // --roster <slot,slot,…>   (1-based device slots; read-only roster dump)
        let slots: Vec<u32> = args
            .get(i + 1)
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        if slots.is_empty() {
            eprintln!("usage: probe --roster <slot,slot,…>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_roster(&slots) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--discover-diff") {
        // E5: --discover-diff FROM SLOTS   (field-8 vs backup discovery; read-only)
        let from = args.get(i + 1).cloned().unwrap_or_default();
        let slots: Vec<u32> = args
            .get(i + 2)
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        if from.is_empty() || slots.is_empty() {
            eprintln!("usage: probe --discover-diff <FROM_ID> <slot,slot,…>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_discover_diff(&from, &slots) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--slot-json") {
        let slot: u32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        if slot == 0 {
            eprintln!("usage: probe --slot-json <slot>   (1-based)");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_slot_json(slot) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--replace-held") {
        // E1: --replace-held FROM TO SLOTS [--commit]   (held-session decider)
        let from = args.get(i + 1).cloned().unwrap_or_default();
        let to = args.get(i + 2).cloned().unwrap_or_default();
        let slots: Vec<u32> = args
            .get(i + 3)
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        let commit = args.iter().any(|a| a == "--commit");
        if from.is_empty() || to.is_empty() || slots.is_empty() {
            eprintln!("usage: probe --replace-held <FROM_ID> <TO_ID> <slot,slot,…> [--commit]");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_replace_held(&from, &to, &slots, commit) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--insert-active") {
        // --insert-active <NEW_MODEL_ID> [--group G1] [--after <FENDER_ID>] [--slot N] [--commit]
        // ADD a block to the device's CURRENT ACTIVE preset (live insertNode, field 34).
        // No --commit = DRY RUN (insert + verify, then revert). --commit = save in-place.
        let fender_id = args.get(i + 1).cloned().unwrap_or_default();
        let group = args
            .iter()
            .position(|a| a == "--group")
            .and_then(|j| args.get(j + 1))
            .cloned();
        let after = args
            .iter()
            .position(|a| a == "--after")
            .and_then(|j| args.get(j + 1))
            .cloned();
        let slot: Option<u32> = args
            .iter()
            .position(|a| a == "--slot")
            .and_then(|j| args.get(j + 1))
            .and_then(|s| s.parse().ok());
        let commit = args.iter().any(|a| a == "--commit");
        if fender_id.is_empty() || fender_id.starts_with("--") {
            eprintln!("usage: probe --insert-active <NEW_MODEL_ID> [--group G1] [--after <FENDER_ID>] [--slot <deviceSlot>] [--commit]");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_insert_active(
            &fender_id,
            group.as_deref(),
            after.as_deref(),
            slot,
            commit,
        ) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--level-footswitch") {
        // --level-footswitch <slot> <switch> <levGroup> <levNode> <levParam> <target> [--commit]
        // Level a block-acting footswitch's engaged state (stimulus via TMP_LEVELLER_STIMULUS).
        // DRY by default (measure + solve, no write); --commit writes valueA + saves.
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let switch: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let lev_group = args.get(i + 3).cloned().unwrap_or_default();
        let lev_node = args.get(i + 4).cloned().unwrap_or_default();
        let lev_param = args.get(i + 5).cloned().unwrap_or_default();
        let target: f64 = args
            .get(i + 6)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let commit = args.iter().any(|a| a == "--commit");
        if slot == u32::MAX || switch == u32::MAX || lev_node.is_empty() || target.is_nan() {
            eprintln!("usage: probe --level-footswitch <slot> <switch> <levGroup> <levNode> <levParam> <target> [--commit]  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_level_footswitch(
            slot, switch, &lev_group, &lev_node, &lev_param, target, commit,
        ) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--bake-validate") {
        // --bake-validate <slot> <switch> <group> <node> <param> <target>  (commit + restore)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let switch: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let group = args.get(i + 3).cloned().unwrap_or_default();
        let node = args.get(i + 4).cloned().unwrap_or_default();
        let param = args.get(i + 5).cloned().unwrap_or_default();
        let target: f64 = args
            .get(i + 6)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        if slot == u32::MAX || switch == u32::MAX || node.is_empty() || target.is_nan() {
            eprintln!("usage: probe --bake-validate <slot> <switch> <group> <node> <param> <target>  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_bake_validate(slot, switch, &group, &node, &param, target) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--fs-list") {
        // --fs-list <slot>   (read-only: footswitch blocks + bake/assign classification)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if slot == u32::MAX {
            eprintln!("usage: probe --fs-list <slot>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_fs_list(slot) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-forced") {
        // --measure-forced <slot> <group> <node>   (GO/NO-GO: does live bypass=false work?)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let group = args.get(i + 2).cloned().unwrap_or_default();
        let node = args.get(i + 3).cloned().unwrap_or_default();
        if slot == u32::MAX || group.is_empty() || node.is_empty() {
            eprintln!("usage: probe --measure-forced <slot> <group> <node>  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_measure_forced(slot, &group, &node) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--repro-chunked") {
        match tmp_companion_lib::probe_repro_chunked() {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--clear-ftsw") {
        // --clear-ftsw <slot> <switch> <index>   (restore/cleanup)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let switch: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let index: u32 = args
            .get(i + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if slot == u32::MAX || switch == u32::MAX || index == u32::MAX {
            eprintln!("usage: probe --clear-ftsw <slot> <switch> <index>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_clear_footswitch(slot, switch, index) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--ftsw-validate") {
        // --ftsw-validate [switchIndex] [--commit]
        // Validate the footswitch-assignment protocol (set/clear/swap/persist) on the
        // ACTIVE preset. DRY by default (working-copy only, reverted); --commit also tests
        // saveCurrentPreset persistence and then restores the original ftsw.
        let switch_override: Option<u32> = args
            .get(i + 1)
            .filter(|s| !s.starts_with("--"))
            .and_then(|s| s.parse().ok());
        let commit = args.iter().any(|a| a == "--commit");
        match tmp_companion_lib::probe_ftsw_validate(switch_override, commit) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--insert-map") {
        // --insert-map <slot> <group> <fenderId> [--before <id>] [--at-index <n>]
        // EMPIRICAL: load slot, print ordered group roster, insert (field-34 --before OR
        // field-99 --at-index OR bare append), print roster again, then commit/revert.
        let slot: u32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let group = args.get(i + 2).cloned().unwrap_or_default();
        let fender_id = args.get(i + 3).cloned().unwrap_or_default();
        let before = args
            .iter()
            .position(|a| a == "--before")
            .and_then(|j| args.get(j + 1))
            .cloned();
        let at_index: Option<u32> = args
            .iter()
            .position(|a| a == "--at-index")
            .and_then(|j| args.get(j + 1))
            .and_then(|s| s.parse().ok());
        let commit = args.iter().any(|a| a == "--commit");
        if slot == 0 || group.is_empty() || fender_id.is_empty() {
            eprintln!("usage: probe --insert-map <slot> <group> <fenderId> [--before <id>] [--at-index <n>] [--commit]");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_insert_map(
            slot,
            &group,
            &fender_id,
            before.as_deref(),
            at_index,
            commit,
        ) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--bulk-replace-saved") {
        // E6: --bulk-replace-saved FROM SLOTS [--commit]   (auto-picks first dual-cab)
        let from = args.get(i + 1).cloned().unwrap_or_default();
        let slots: Vec<u32> = args
            .get(i + 2)
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        let commit = args.iter().any(|a| a == "--commit");
        if from.is_empty() || slots.is_empty() {
            eprintln!("usage: probe --bulk-replace-saved <FROM_CAB_ID> <slot,slot,…> [--commit]");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_bulk_replace_saved(&from, &slots, commit) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--saved-blocks") {
        eprintln!("[probe] list saved blocks (production decode path)…");
        match tmp_companion_lib::probe_saved_blocks() {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--re-blocks") {
        eprintln!("[probe] RE spike: RequestAllBlockPresets + user-IR list (READ-ONLY)…");
        match tmp_companion_lib::probe_re_blocks() {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--overlay-ab") {
        // --overlay-ab <slot|all> [--contexts N]  — SAVED overlay vs LIVE prepass A/B.
        // NON-DESTRUCTIVE: no writes/saves, no re-amp. slot is 1-based (as --slot-json).
        let target = args.get(i + 1).cloned().unwrap_or_default();
        let contexts: u32 = args
            .iter()
            .position(|a| a == "--contexts")
            .and_then(|j| args.get(j + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        if target.is_empty() || target.starts_with("--") {
            eprintln!(
                "usage: probe --overlay-ab <slot|all> [--contexts N]   (slot 1-based, N default 2)"
            );
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_overlay_ab(&target, contexts) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--scenes-passive") {
        match tmp_companion_lib::probe_scan_scenes_passive() {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--slotread-x") {
        let slots: Vec<u32> = args[i + 1..].iter().map_while(|a| a.parse().ok()).collect();
        match tmp_companion_lib::probe_slotread_experiments(slots) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--slotread-live") {
        let slot: u32 = args.get(i + 1).and_then(|a| a.parse().ok()).unwrap_or(0);
        let rounds: u32 = args.get(i + 2).and_then(|a| a.parse().ok()).unwrap_or(4);
        if slot == 0 {
            eprintln!("usage: probe --slotread-live <slot> [rounds]   (1-based device slot)");
            std::process::exit(1);
        }
        match tmp_companion_lib::probe_slotread_live(slot, rounds) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--scenes-full") {
        eprintln!("[probe] full scene scan via LoadPreset → field-3 (changes active preset)…");
        match tmp_companion_lib::probe_scan_scenes_full_live() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--scenes") {
        eprintln!("[probe] scene scan (LoadPreset → sceneList 125)…");
        match tmp_companion_lib::probe_scan_scenes_load() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--seed-scenario") {
        // Online-e2e seeding in a FRESH process (device work from a long-lived
        // process degrades — truncated list harvests + capricious opens); sweeps
        // stray scenario imports first. The e2e runner calls this per spec.
        eprintln!("[probe] seeding the e2e scenario presets (sweep + import)…");
        match tmp_companion_lib::probe_seed_scenario() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--clear-strays") {
        // Attended cleanup: sweep stray scenario imports (exact-name, wrong-slot
        // matches only, off a completeness-floored list) without seeding.
        eprintln!("[probe] sweeping stray e2e scenario imports…");
        match tmp_companion_lib::probe_clear_strays() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--listall") {
        // Read-only: print every My Presets slot + name (full list, not just the
        // first 20). For auditing device state after a run.
        match tmp_companion_lib::probe_connect_and_list() {
            Ok(presets) => {
                println!("[probe --listall] {} entries:", presets.len());
                for p in &presets {
                    println!("  idx {:>3} · slot {:>3}  {}", p.slot, p.slot + 1, p.name);
                }
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--capture-input") {
        let secs: f32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5.0);
        eprintln!("[probe] GATE 1 — play your guitar continuously for {secs:.0}s NOW…");
        match tmp_companion_lib::probe_capture_input(secs) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--agc-test") {
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if slot == u32::MAX {
            eprintln!("usage: probe --agc-test <clean-preset-slot>  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        eprintln!("[probe] GATE 2 — re-amp inject AGC test on slot {slot} (no playing needed)…");
        match tmp_companion_lib::probe_reamp_agc_test(slot) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--channels") {
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if slot == u32::MAX {
            eprintln!(
                "usage: probe --channels <slot>   (N1: per-channel re-amp LUFS; mono-vs-stereo check)"
            );
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_channels(slot) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--levelblock") {
        // --levelblock <slot> <target_lufs> <groupId> <nodeId> <parameterId>  (stimulus via env)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let group = args.get(i + 3).cloned().unwrap_or_default();
        let node = args.get(i + 4).cloned().unwrap_or_default();
        let param = args.get(i + 5).cloned().unwrap_or_default();
        if slot == u32::MAX
            || target.is_nan()
            || group.is_empty()
            || node.is_empty()
            || param.is_empty()
        {
            eprintln!("usage: probe --levelblock <slot> <target_lufs> <groupId> <nodeId> <parameterId>  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        eprintln!("[probe] closed-loop block leveling: slot {slot} {group}/{node}/{param} → {target} LUFS…");
        match tmp_companion_lib::probe_level_block(slot, target, group, node, param) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--bench-scene-leveling") {
        // --bench-scene-leveling <slots_csv> <target_lufs> <topology_id> <out.json>
        // Example: --bench-scene-leveling 0,1,5 -22 guitar-humbucker /tmp/level.json
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        let target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        let out = args.get(i + 4).cloned().unwrap_or_default();
        let slots: Vec<u32> = slots_csv
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        if slots.is_empty() || target.is_nan() || topology.is_empty() || out.is_empty() {
            eprintln!("usage: probe --bench-scene-leveling <slots_csv> <target_lufs> <topology_id> <out.json>");
            std::process::exit(2);
        }
        eprintln!(
            "[probe] benchmarking scene leveling slots={slots:?} target={target} topology={topology} → {out}…"
        );
        match tmp_companion_lib::probe_bench_scene_leveling(slots, target, topology, out) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--blocks") {
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if slot == u32::MAX {
            eprintln!("usage: probe --blocks <slot>");
            std::process::exit(2);
        }
        eprintln!("[probe] enumerating level-type block controls for slot {slot}…");
        match tmp_companion_lib::probe_list_blocks(slot) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--listen") {
        // --listen [seconds] [heartbeatMs] [pollSecs]  — push-listener discovery:
        // park on the post-handshake connection and print every inbound push (drive
        // the unit by hand meanwhile). heartbeatMs 0 = no heartbeat; 250 ≈ Pro
        // Control's 4/sec keepalive that holds the live-controller session (our old
        // 10 000 ms let it lapse → connectionError). pollSecs > 0 also re-requests
        // current-preset state each tick. Default: 120s, hb 250ms, poll OFF.
        let seconds: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(120);
        let hb: u64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(250);
        let poll: u64 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(0);
        match tmp_companion_lib::probe_listen(seconds, hb, poll) {
            Ok(()) => return,
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--classify") {
        // --classify <listIdx> [scene…] — NON-DESTRUCTIVE: print how build_scene_jobs
        // classifies each scene's amp-knob set (routing-aware). No re-amp / writes.
        let Some(list_index) = args.get(i + 1).and_then(|s| s.parse::<u32>().ok()) else {
            eprintln!("usage: probe --classify <listIdx> [scene…]");
            std::process::exit(2);
        };
        let scenes: Vec<u32> = args[i + 2..]
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        match tmp_companion_lib::probe_classify_scenes(list_index, scenes) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args
        .iter()
        .position(|a| a == "--level-scenes" || a == "--rebalance-scenes")
    {
        // --level-scenes <listIdx> <target> <topology> [scene…]      NO-SAVE joint-k run.
        // --rebalance-scenes <listIdx> <target> <topology> [scene…]  NO-SAVE rebalance run.
        let rebalance = args[i] == "--rebalance-scenes";
        let list_index = args.get(i + 1).and_then(|s| s.parse::<u32>().ok());
        let target = args.get(i + 2).and_then(|s| s.parse::<f64>().ok());
        let topology = args.get(i + 3).cloned();
        let (Some(list_index), Some(target), Some(topology)) = (list_index, target, topology)
        else {
            eprintln!("usage: probe --level-scenes|--rebalance-scenes <listIdx> <targetLUFS> <topology> [scene…]");
            std::process::exit(2);
        };
        let scenes: Vec<u32> = args[i + 4..]
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        match tmp_companion_lib::probe_level_scenes_oneshot(
            list_index, target, topology, scenes, rebalance,
        ) {
            Ok(report) => {
                print!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--reamp-state") {
        // DIAGNOSTIC (reamp-stuck investigation): passive re-amp state read — no
        // HID commands; audio-only tell (see probe_api::level::probe_reamp_state).
        match tmp_companion_lib::probe_reamp_state("guitar-humbucker") {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--reamp-toggle-test") {
        // DIAGNOSTIC (reamp-stuck investigation): --reamp-toggle-test <idle_ms> [hb]
        let idle_ms: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let hb = args.iter().any(|a| a == "hb");
        match tmp_companion_lib::probe_reamp_toggle_test(idle_ms, hb) {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--reamp-off") {
        // Recovery: force re-amp OFF (a stranded re-amp mutes the guitar input).
        match tmp_companion_lib::probe_reamp_off() {
            Ok(()) => {
                println!("re-amp OFF sent OK");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--loadscene") {
        // --loadscene <sceneSlot>  — recall a scene on the CURRENT preset. The wire
        // slot is the 0-based scenes[] index (0..=7); 8 = the base scene (constant).
        // Slot 0 is valid (the encoder emits the field explicitly even for 0 — the
        // device ignores an empty LoadScene{}, HW-found).
        // Non-destructive live state change; verify via an --activegraph diff.
        let Some(scene_slot) = args.get(i + 1).and_then(|s| s.parse::<u32>().ok()) else {
            eprintln!("usage: probe --loadscene <sceneSlot>   (0-based scenes[] index; 8 = base; current preset)");
            std::process::exit(2);
        };
        eprintln!("[probe] recalling scene {scene_slot} on the current preset…");
        match tmp_companion_lib::probe_load_scene(scene_slot) {
            Ok(()) => {
                println!("loadScene({scene_slot}) sent OK");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-current") {
        // --measure-current <topology_id> [sceneSlot] [calibrationLUFS]
        // Read-only loudness check for the current preset/scene. Optional sceneSlot
        // recalls that scene before capture; no presetLevel/block writes, no save.
        let topology = args.get(i + 1).cloned().unwrap_or_default();
        if topology.is_empty() {
            eprintln!("usage: probe --measure-current <topology_id> [sceneSlot] [calibrationLUFS]");
            std::process::exit(2);
        }
        let scene_slot = args.get(i + 2).and_then(|s| s.parse::<u32>().ok());
        let calibration_lufs = args.get(i + 3).and_then(|s| s.parse::<f32>().ok());
        eprintln!(
            "[probe] measuring current live LUFS topology={topology} scene={scene_slot:?} calibration={calibration_lufs:?}…"
        );
        match tmp_companion_lib::probe_measure_current_lufs(
            &topology,
            None,
            scene_slot,
            calibration_lufs,
        ) {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-scene") {
        // --measure-scene <slot> <sceneSlot> <topology_id> [calibrationLUFS]
        // slot = 0-based list index (same convention as --levelpreset; NOT the
        // 1-based device userSlot). Loads the preset in its own connection, then
        // scene+reamp in a fresh one.
        let slot = args.get(i + 1).and_then(|s| s.parse::<u32>().ok());
        let scene_slot = args.get(i + 2).and_then(|s| s.parse::<u32>().ok());
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        if slot.is_none() || scene_slot.is_none() || topology.is_empty() {
            eprintln!(
                "usage: probe --measure-scene <slot(0-based list index)> <sceneSlot> <topology_id> [calibrationLUFS]"
            );
            std::process::exit(2);
        }
        let calibration_lufs = args.get(i + 4).and_then(|s| s.parse::<f32>().ok());
        eprintln!(
            "[probe] measuring slot={} scene={} topology={topology} calibration={calibration_lufs:?}…",
            slot.unwrap(),
            scene_slot.unwrap(),
        );
        match tmp_companion_lib::probe_measure_current_lufs(
            &topology,
            slot,
            scene_slot,
            calibration_lufs,
        ) {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--held-reengage") {
        // --held-reengage <slot> <topology_id> <sceneA,sceneB,...> [calibrationLUFS]
        // HW probe: can re-amp disengage→re-engage on ONE held connection? Compares
        // held-session per-scene loudness vs the proven fresh-connection control.
        let slot = args.get(i + 1).and_then(|s| s.parse::<u32>().ok());
        let topology = args.get(i + 2).cloned().unwrap_or_default();
        let scenes: Vec<u32> = args
            .get(i + 3)
            .map(|s| {
                s.split(',')
                    .filter_map(|t| t.trim().parse::<u32>().ok())
                    .collect()
            })
            .unwrap_or_default();
        if slot.is_none() || topology.is_empty() || scenes.is_empty() {
            eprintln!(
                "usage: probe --held-reengage <slot> <topology_id> <sceneA,sceneB,...> [calibrationLUFS]"
            );
            std::process::exit(2);
        }
        let calibration_lufs = args.get(i + 4).and_then(|s| s.parse::<f32>().ok());
        eprintln!(
            "[probe] held-reengage slot={} topology={topology} scenes={scenes:?}…",
            slot.unwrap()
        );
        match tmp_companion_lib::probe_held_reengage(
            &topology,
            slot.unwrap(),
            &scenes,
            calibration_lufs,
        ) {
            Ok(report) => {
                println!("{report}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--export") {
        // --export <listEnum> <slot>  — AC1 spike. Always no-batch (the
        // device gives no reply to a batched slot-read — the resolved AC1 finding).
        let list_enum: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let slot: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if list_enum == u32::MAX || slot == u32::MAX {
            eprintln!("usage: probe --export <listEnum> <slot>   (listEnum 1 = My Presets)");
            std::process::exit(2);
        }
        eprintln!("[probe] exporting preset listEnum={list_enum} slot={slot}…");
        match tmp_companion_lib::probe_export_preset(list_enum, slot) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--factory-list") {
        // --factory-list — print every Factory preset as `slot<TAB>name`.
        eprintln!("[probe] listing Factory presets…");
        match tmp_companion_lib::probe_factory_list() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--load-probe") {
        // --load-probe <slot> <tabEnum> — raw loadPreset, SEND-ONLY (the TMP
        // screen is the verification oracle). Both values pass through verbatim
        // (experiment with 0-/1-based slots + unknown Factory tabEnums).
        // Non-destructive (load only, no save).
        let slot: u64 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX);
        let tab_enum: u64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX);
        if slot == u64::MAX || tab_enum == u64::MAX {
            eprintln!(
                "usage: probe --load-probe <slot> <tabEnum>   (tabEnum 1 = UserPresets; Factory unknown)"
            );
            std::process::exit(2);
        }
        eprintln!("[probe] loadPreset slot={slot} tabEnum={tab_enum}…");
        match tmp_companion_lib::probe_load_probe(slot, tab_enum) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--import") {
        // --import <file.preset>  — AC3: re-import a preset over USB (additive).
        let path = match args.get(i + 1) {
            Some(p) => p.clone(),
            None => {
                eprintln!("usage: probe --import <file.preset>");
                std::process::exit(2);
            }
        };
        eprintln!("[probe] importing {path}…");
        match tmp_companion_lib::probe_import_preset(&path) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--save-load-test") {
        // --save-load-test <slotA> <slotB> <level> --commit <expectedNameOfSlotA>
        // (HW experiment: save + next-load on ONE connection; DESTRUCTIVE — overwrites
        // slotA's stored presetLevel). slotA/slotB are 0-based list indices (same
        // space as `list_my_presets`). --commit's expected name is checked against a
        // non-destructive read of slotA BEFORE the mutation — required, not optional,
        // since this command has no non-destructive form.
        let a: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let b: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let level: f32 = args
            .get(i + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f32::NAN);
        let commit_i = args.iter().position(|arg| arg == "--commit");
        let expected_name = commit_i.and_then(|ci| args.get(ci + 1)).cloned();
        let Some(expected_name) =
            expected_name.filter(|_| a != u32::MAX && b != u32::MAX && level.is_finite())
        else {
            eprintln!(
                "usage: probe --save-load-test <slotA> <slotB> <level> --commit <expectedNameOfSlotA>  (slotA/slotB are 0-based list indices; DESTRUCTIVE)"
            );
            std::process::exit(2);
        };
        match tmp_companion_lib::probe_save_load_test(a, b, level, &expected_name) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args
        .iter()
        .position(|a| a == "--redistribute-persist-check")
    {
        // --redistribute-persist-check <scratchSlot> <expectedName>
        // PR5 go/no-go: do presetLevel + base amp outputLevel + scene overlay all
        // persist through ONE save? Point at a prepared scratch preset with an amp +
        // ≥1 scene (e.g. E2E Reference at 400 after --seed-scenario). 0-based list index;
        // name-guarded; restores the slot afterward.
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let expected_name = args.get(i + 2).cloned();
        let Some(expected_name) =
            expected_name.filter(|n| !n.starts_with("--") && slot != u32::MAX)
        else {
            eprintln!(
                "usage: probe --redistribute-persist-check <scratchSlot> <expectedName>  (0-based list index; the scratch preset needs an amp + ≥1 scene)"
            );
            std::process::exit(2);
        };
        match tmp_companion_lib::probe_redistribute_persist_check(slot, &expected_name) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--clear") {
        // --clear <listIndex> <expect-name>  — clears only if the slot reads
        // expect-name. 0-BASED list index (what `--import`'s diff prints), NOT the
        // 1-based device slot `--export` takes.
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let expect = args.get(i + 2).cloned().unwrap_or_default();
        if slot == u32::MAX || expect.is_empty() {
            eprintln!("usage: probe --clear <listIndex(0-based)> <expect-name>");
            std::process::exit(2);
        }
        eprintln!("[probe] clearing user preset slot {slot} (if it reads {expect:?})…");
        match tmp_companion_lib::probe_clear_preset(slot, &expect) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--map-slots") {
        // Resolve the list-index ↔ device-userSlot offset (AC7 prerequisite).
        eprintln!("[probe] resolving list↔device slot mapping…");
        match tmp_companion_lib::probe_map_slots() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--songs") {
        // Read-only: list every Song on the device with its notes + BPM.
        match tmp_companion_lib::probe_list_songs() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--setlists") {
        // Read-only: list every Setlist on the device (names only).
        match tmp_companion_lib::probe_list_setlists() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--setlist-songs") {
        // Read-only: for every Setlist, list the songs it contains (slots → names).
        match tmp_companion_lib::probe_setlist_songs() {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // ─── Song/Setlist WRITE primitives (DEVICE WRITES; name-addressed) ────────
    // Helper macro-free dispatch: each maps to a tmp_companion_lib::probe_* fn.
    macro_rules! run {
        ($e:expr) => {
            match $e {
                Ok(r) => {
                    print!("{r}");
                    return;
                }
                Err(e) => {
                    eprintln!("[probe] FAILED: {e}");
                    std::process::exit(1);
                }
            }
        };
    }

    if args.iter().any(|a| a == "--diag-frames") {
        eprintln!("[probe] DIAG: dumping raw inbound frame magics for a setlist read…");
        run!(tmp_companion_lib::probe_diag_frames());
    }

    if args.iter().any(|a| a == "--diag-writes") {
        eprintln!("[probe] DIAG: comparing addSong vs addSetlist device replies…");
        run!(tmp_companion_lib::probe_diag_writes());
    }

    if let Some(i) = args.iter().position(|a| a == "--add-song") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --add-song <name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE add-song {name:?}…");
        run!(tmp_companion_lib::probe_add_song(&name));
    }
    if let Some(i) = args.iter().position(|a| a == "--add-setlist") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --add-setlist <name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE add-setlist {name:?}…");
        run!(tmp_companion_lib::probe_add_setlist(&name));
    }
    if let Some(i) = args.iter().position(|a| a == "--add-setlist-song") {
        let setlist = args.get(i + 1).cloned().unwrap_or_default();
        let song = args.get(i + 2).cloned().unwrap_or_default();
        if setlist.is_empty() || song.is_empty() {
            eprintln!("usage: probe --add-setlist-song <setlist-name> <song-name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE add {song:?} → setlist {setlist:?}…");
        run!(tmp_companion_lib::probe_add_setlist_song(&setlist, &song));
    }
    if let Some(i) = args.iter().position(|a| a == "--rename-song") {
        let old = args.get(i + 1).cloned().unwrap_or_default();
        let new = args.get(i + 2).cloned().unwrap_or_default();
        if old.is_empty() || new.is_empty() {
            eprintln!("usage: probe --rename-song <old-name> <new-name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE rename-song {old:?} → {new:?}…");
        run!(tmp_companion_lib::probe_rename_song(&old, &new));
    }
    if let Some(i) = args.iter().position(|a| a == "--rename-setlist") {
        let old = args.get(i + 1).cloned().unwrap_or_default();
        let new = args.get(i + 2).cloned().unwrap_or_default();
        if old.is_empty() || new.is_empty() {
            eprintln!("usage: probe --rename-setlist <old-name> <new-name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE rename-setlist {old:?} → {new:?}…");
        run!(tmp_companion_lib::probe_rename_setlist(&old, &new));
    }
    if let Some(i) = args.iter().position(|a| a == "--song-notes") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        let notes = args.get(i + 2).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --song-notes <song-name> <notes>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE song-notes {name:?} → {notes:?}…");
        run!(tmp_companion_lib::probe_set_song_notes(&name, &notes));
    }
    if let Some(i) = args.iter().position(|a| a == "--song-bpm") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        let bpm: f32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f32::NAN);
        if name.is_empty() || bpm.is_nan() {
            eprintln!("usage: probe --song-bpm <song-name> <bpm>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE song-bpm {name:?} → {bpm}…");
        run!(tmp_companion_lib::probe_set_song_bpm(&name, bpm));
    }
    if let Some(i) = args.iter().position(|a| a == "--remove-song") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --remove-song <name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE remove-song {name:?}…");
        run!(tmp_companion_lib::probe_remove_song(&name));
    }
    if let Some(i) = args.iter().position(|a| a == "--remove-setlists-named") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --remove-setlists-named <name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE remove-setlists-named {name:?}…");
        run!(tmp_companion_lib::probe_remove_setlists_named(&name));
    }

    if let Some(i) = args.iter().position(|a| a == "--remove-setlist") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        if name.is_empty() {
            eprintln!("usage: probe --remove-setlist <name>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE remove-setlist {name:?}…");
        run!(tmp_companion_lib::probe_remove_setlist(&name));
    }
    if let Some(i) = args.iter().position(|a| a == "--remove-setlist-song") {
        let setlist = args.get(i + 1).cloned().unwrap_or_default();
        let pos: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if setlist.is_empty() || pos == u32::MAX {
            eprintln!("usage: probe --remove-setlist-song <setlist-name> <position>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE remove-setlist-song {setlist:?} position {pos}…");
        run!(tmp_companion_lib::probe_remove_setlist_song(&setlist, pos));
    }
    if let Some(i) = args.iter().position(|a| a == "--move-setlist-song") {
        let setlist = args.get(i + 1).cloned().unwrap_or_default();
        let old: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let new: u32 = args
            .get(i + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if setlist.is_empty() || old == u32::MAX || new == u32::MAX {
            eprintln!("usage: probe --move-setlist-song <setlist-name> <old-pos> <new-pos>");
            std::process::exit(2);
        }
        eprintln!("[probe] WRITE move-setlist-song {setlist:?} {old}→{new}…");
        run!(tmp_companion_lib::probe_move_setlist_song(
            &setlist, old, new
        ));
    }

    if let Some(i) = args.iter().position(|a| a == "--songpresets") {
        // --songpresets <songSlot>  — read a Song's preset rows (userPresetSlot/scene).
        let song: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if song == u32::MAX {
            eprintln!("usage: probe --songpresets <songSlot>");
            std::process::exit(2);
        }
        eprintln!("[probe] reading song {song} preset rows…");
        match tmp_companion_lib::probe_song_presets(song) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--replace-inplace") {
        // --replace-inplace <orig-list-index> <file.preset>
        //   Edit a preset IN PLACE (preserve slot + Song link): import → load scratch
        //   → saveCurrentPreset over the original → guarded clear of the scratch.
        let idx: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let path = args.get(i + 2).cloned().unwrap_or_default();
        if idx == u32::MAX || path.is_empty() {
            eprintln!("usage: probe --replace-inplace <orig-list-index> <file.preset>");
            std::process::exit(2);
        }
        eprintln!("[probe] in-place edit of list idx {idx} from {path}…");
        match tmp_companion_lib::probe_replace_inplace(idx, &path) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--restore") {
        // --restore <snapshot.json>  — AC5: restore a backup in place (orig slot + Song link).
        let path = args.get(i + 1).cloned().unwrap_or_default();
        if path.is_empty() {
            eprintln!("usage: probe --restore <snapshot.json>");
            std::process::exit(2);
        }
        eprintln!("[probe] restoring snapshot {path} in place…");
        match tmp_companion_lib::probe_restore(&path) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--capture-reference") {
        // --capture-reference <slot> <topology> <out.wav>  (offline-harness corpus)
        // DEVICE OP: load slot + engage + full ~6.8s capture → write the mono clip.
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let topology = args.get(i + 2).cloned().unwrap_or_default();
        let out = args.get(i + 3).cloned().unwrap_or_default();
        if slot == u32::MAX || topology.is_empty() || out.is_empty() {
            eprintln!("usage: probe --capture-reference <slot> <topology> <out.wav>");
            std::process::exit(2);
        }
        eprintln!("[probe] capturing reference clip slot {slot} topology={topology} → {out}…");
        match tmp_companion_lib::probe_capture_reference(slot, &topology, &out) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-adaptive") {
        // --measure-adaptive <slot> <topology>  (DEVICE A/B: full vs adaptive capture)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let topology = args.get(i + 2).cloned().unwrap_or_default();
        if slot == u32::MAX || topology.is_empty() {
            eprintln!("usage: probe --measure-adaptive <slot> <topology>");
            std::process::exit(2);
        }
        eprintln!("[probe] A/B full vs adaptive capture slot {slot} topology={topology}…");
        match tmp_companion_lib::probe_measure_adaptive(slot, &topology) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor") {
        // --doctor <slots_csv> <topology>  (Doctor calibration sweep, read-only)
        // A CSV entry is `N` (the slot's BASE sound) or `N:S` (its 0-based wire
        // scene S — e.g. `0:1` = list index 0, scene 1).
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        let topology = args.get(i + 2).cloned().unwrap_or_default();
        let slots: Vec<(u32, Option<u32>)> = match slots_csv
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| match x.split_once(':') {
                Some((slot, scene)) => Ok((slot.parse::<u32>()?, Some(scene.parse::<u32>()?))),
                None => x.parse::<u32>().map(|s| (s, None)),
            })
            .collect::<Result<Vec<(u32, Option<u32>)>, std::num::ParseIntError>>()
        {
            Ok(s) if !s.is_empty() && !topology.is_empty() => s,
            _ => {
                eprintln!("usage: probe --doctor <slots_csv[:scene]> <topology>");
                std::process::exit(2);
            }
        };
        eprintln!(
            "[probe] doctor sweep over {} slot(s), topology={topology}…",
            slots.len()
        );
        match tmp_companion_lib::probe_doctor(&slots, &topology) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor-calib") {
        // --doctor-calib <slots_csv> --stim <wav> --family <guitar|bass|bass-vi>
        //                [--labels <rules.json>] --out <report.json>  (read-only sweep)
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        let stim = flag_arg(&args, "--stim").unwrap_or_default();
        let family = flag_arg(&args, "--family").unwrap_or_default();
        let out = flag_arg(&args, "--out").unwrap_or_default();
        let labels = flag_arg(&args, "--labels");
        let slots: Vec<u32> = match slots_csv
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<u32>, _>>()
        {
            Ok(s) if !s.is_empty() && !stim.is_empty() && !family.is_empty() && !out.is_empty() => {
                s
            }
            _ => {
                eprintln!(
                    "usage: probe --doctor-calib <slots_csv> --stim <wav> --family <guitar|bass|bass-vi> [--labels <rules.json>] --out <report.json>"
                );
                std::process::exit(2);
            }
        };
        eprintln!(
            "[probe] doctor-calib sweep over {} slot(s), family={family}, stim={stim}…",
            slots.len()
        );
        match tmp_companion_lib::probe_doctor_calib(&slots, &stim, &family, labels.as_deref(), &out)
        {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor-calib-factory") {
        // --doctor-calib-factory <factory_slots_csv> --stim <wav> --family <..> --out <report.json>
        // Loads each FACTORY preset (tabEnum=4) + captures AS-LOADED for reference derivation.
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        let stim = flag_arg(&args, "--stim").unwrap_or_default();
        let family = flag_arg(&args, "--family").unwrap_or_default();
        let out = flag_arg(&args, "--out").unwrap_or_default();
        let slots: Vec<u32> = match slots_csv
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<u32>, _>>()
        {
            Ok(s) if !s.is_empty() && !stim.is_empty() && !family.is_empty() && !out.is_empty() => {
                s
            }
            _ => {
                eprintln!(
                    "usage: probe --doctor-calib-factory <factory_slots_csv> --stim <wav> --family <guitar|bass|bass-vi> --out <report.json>"
                );
                std::process::exit(2);
            }
        };
        eprintln!(
            "[probe] doctor-calib-factory sweep over {} factory slot(s), family={family}, stim={stim}…",
            slots.len()
        );
        match tmp_companion_lib::probe_doctor_calib_factory(&slots, &stim, &family, &out) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--doctor-window-ab") {
        // --doctor-window-ab <slots_csv> --stim <wav> [--family <guitar|bass|bass-vi>]
        //                    [--out <report.json>]  (LOADS the probed slots, read-only otherwise)
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        let stim = flag_arg(&args, "--stim").unwrap_or_default();
        let family = flag_arg(&args, "--family").unwrap_or_else(|| "guitar".to_string());
        let out = flag_arg(&args, "--out");
        let slots: Vec<u32> = match slots_csv
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<u32>, _>>()
        {
            Ok(s) if !s.is_empty() && !stim.is_empty() => s,
            _ => {
                eprintln!(
                    "usage: probe --doctor-window-ab <slots_csv> --stim <wav> [--family <guitar|bass|bass-vi>] [--out <report.json>]"
                );
                std::process::exit(2);
            }
        };
        eprintln!(
            "[probe] doctor-window-ab sweep over {} slot(s), family={family}, stim={stim}…",
            slots.len()
        );
        match tmp_companion_lib::probe_doctor_window_ab(&slots, &stim, &family, out.as_deref()) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--capture-wav") {
        // --capture-wav <out.wav> [secs=12]  (DEVICE: save the dry DI to a WAV)
        let path = args.get(i + 1).cloned().unwrap_or_default();
        let secs: f32 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(12.0);
        if path.is_empty() {
            eprintln!("usage: probe --capture-wav <out.wav> [secs=12]");
            std::process::exit(2);
        }
        eprintln!("[probe] capturing dry instrument for {secs:.0}s — play continuously NOW…");
        match tmp_companion_lib::probe_capture_wav(&path, secs) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--scale-wav") {
        // --scale-wav <in.wav> <out.wav> <target_lufs>  (NO-DEVICE: Tier-2 LUFS match)
        let src = args.get(i + 1).cloned().unwrap_or_default();
        let dst = args.get(i + 2).cloned().unwrap_or_default();
        let target: Option<f32> = args.get(i + 3).and_then(|s| s.parse().ok());
        let (Some(target), false) = (target, src.is_empty() || dst.is_empty()) else {
            eprintln!("usage: probe --scale-wav <in.wav> <out.wav> <target_lufs>");
            std::process::exit(2);
        };
        match tmp_companion_lib::probe_scale_wav(&src, &dst, target) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--stim-ab") {
        // --stim-ab <slots_csv> <wavA> <wavB> [ref=0.5]  (DEVICE A/B: two stimuli per preset)
        let slots_csv = args.get(i + 1).cloned().unwrap_or_default();
        // Reject a malformed slot token instead of silently dropping it (which would
        // produce a valid-looking A/B table for a different set of presets).
        let slots: Vec<u32> = match slots_csv
            .split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<u32>, _>>()
        {
            Ok(s) => s,
            Err(_) => {
                eprintln!(
                    "[probe] --stim-ab: invalid slot in '{slots_csv}' (expected comma-separated integers)"
                );
                std::process::exit(2);
            }
        };
        let wav_a = args.get(i + 2).cloned().unwrap_or_default();
        let wav_b = args.get(i + 3).cloned().unwrap_or_default();
        // Reject a malformed ref_level rather than silently reverting to 0.5.
        let ref_level: f32 = match args.get(i + 4) {
            Some(s) => match s.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("[probe] --stim-ab: invalid ref_level '{s}' (expected a float)");
                    std::process::exit(2);
                }
            },
            None => 0.5,
        };
        if slots.is_empty() || wav_a.is_empty() || wav_b.is_empty() {
            eprintln!("usage: probe --stim-ab <slots_csv> <wavA> <wavB> [ref_level=0.5]");
            std::process::exit(2);
        }
        eprintln!("[probe] stimulus A/B on {} preset(s)…", slots.len());
        match tmp_companion_lib::probe_stim_ab(&slots, &wav_a, &wav_b, ref_level) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-prefix-sweep") {
        // --measure-prefix-sweep <wav>  (NO device — analysis of a reference clip)
        let wav = args.get(i + 1).cloned().unwrap_or_default();
        if wav.is_empty() {
            eprintln!("usage: probe --measure-prefix-sweep <wav>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_measure_prefix_sweep(&wav) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--measure-converge-replay") {
        // --measure-converge-replay <wav> <eps_lu> <stable_k> <preroll_ms>  (NO device)
        let wav = args.get(i + 1).cloned().unwrap_or_default();
        let eps: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let k: u32 = args
            .get(i + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let preroll: u64 = args
            .get(i + 4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX);
        if wav.is_empty() || eps.is_nan() || k == u32::MAX || preroll == u64::MAX {
            eprintln!(
                "usage: probe --measure-converge-replay <wav> <eps_lu> <stable_k> <preroll_ms>"
            );
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_measure_converge_replay(&wav, eps, k, preroll) {
            Ok(r) => {
                print!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--lufs") {
        let path = args.get(i + 1).cloned().unwrap_or_default();
        if path.is_empty() {
            eprintln!("usage: probe --lufs <wav>");
            std::process::exit(2);
        }
        match tmp_companion_lib::measure_wav_file(&path) {
            Ok(l) => {
                println!(
                    "integrated_lufs={:.3} short_term_max_lufs={:.3}",
                    l.integrated_lufs, l.short_term_max_lufs
                );
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--levelpreset") {
        // --levelpreset <slot> <target_lufs> [save] [noverify]  (stimulus via env)
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        if slot == u32::MAX || target.is_nan() {
            eprintln!("usage: probe --levelpreset <slot> <target_lufs> [save] [noverify]  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        let save = args.iter().any(|a| a == "save");
        let verify = !args.iter().any(|a| a == "noverify");
        eprintln!(
            "[probe] one-shot level slot {slot} → {target} LUFS (save={save}, verify={verify})…"
        );
        match tmp_companion_lib::probe_level_preset(slot, target, save, verify) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --live-lufs <slot> <target_lufs> [save] [noverify]  (stimulus via env)
    // Same path as --levelpreset, but installs an advisory live-LUFS sink that PRINTS each
    // streamed reading. The final summary must match a plain --levelpreset run — A/B on a
    // REVERB/DELAY preset to catch any capture-length re-baseline.
    if let Some(i) = args.iter().position(|a| a == "--live-lufs") {
        let slot: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        if slot == u32::MAX || target.is_nan() {
            eprintln!("usage: probe --live-lufs <slot> <target_lufs> [save] [noverify]  (TMP_LEVELLER_STIMULUS=<wav>)");
            std::process::exit(2);
        }
        let save = args.iter().any(|a| a == "save");
        let verify = !args.iter().any(|a| a == "noverify");
        eprintln!(
            "[probe] live-lufs level slot {slot} → {target} LUFS (save={save}, verify={verify}); streaming readings…"
        );
        match tmp_companion_lib::probe_live_lufs(slot, target, save, verify) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --level-preset-scenes <listIndex> <defaultTarget> <topology> <save:0|1> [Name=Target ...]
    // Levels a preset's Base + every FS scene; per-scene-name overrides hit a different
    // target. Example: --level-preset-scenes 0 -23 guitar-humbucker 1 Dist=-22 Swell=-22
    if let Some(i) = args.iter().position(|a| a == "--level-preset-scenes") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let default_target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        let save = matches!(
            args.get(i + 4).map(|s| s.as_str()),
            Some("1" | "true" | "save" | "yes")
        );
        if list_index == u32::MAX || default_target.is_nan() || topology.is_empty() {
            eprintln!("usage: probe --level-preset-scenes <listIndex> <defaultTarget> <topology> <save:0|1> [Name=Target ...]");
            std::process::exit(2);
        }
        let overrides = parse_target_overrides(&args, i + 5);
        eprintln!(
            "[probe] level-preset-scenes list_index={list_index} default={default_target} topology={topology} save={save} overrides={overrides:?}…"
        );
        match tmp_companion_lib::probe_level_preset_scenes(
            list_index,
            default_target,
            topology,
            save,
            overrides,
        ) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --bisect-scene <listIndex> <sceneSlot> <groupId> <nodeId> <value> [asis] [save]
    // HW bisection: potent isolated write-measure, plus optional jointk elements.
    if let Some(i) = args.iter().position(|a| a == "--bisect-scene") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let scene_slot: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let group = args.get(i + 3).cloned().unwrap_or_default();
        let node = args.get(i + 4).cloned().unwrap_or_default();
        let value: f32 = args
            .get(i + 5)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f32::NAN);
        if list_index == u32::MAX
            || scene_slot == u32::MAX
            || group.is_empty()
            || node.is_empty()
            || value.is_nan()
        {
            eprintln!("usage: probe --bisect-scene <listIndex> <sceneSlot> <groupId> <nodeId> <value> [asis] [save]");
            std::process::exit(2);
        }
        let with_asis = args.iter().any(|a| a == "asis");
        let with_save = args.iter().any(|a| a == "save");
        match tmp_companion_lib::probe_bisect_scene(
            list_index,
            scene_slot,
            group,
            node,
            value,
            with_asis,
            with_save,
            "guitar-singlecoil".to_string(),
        ) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--fs-batch") {
        // --fs-batch <listIndex> [v1 v2 …]  — batched footswitch WRITE validation:
        // plan bake/assign per block-acting switch, write ALL on one session, ONE save.
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if list_index == u32::MAX {
            eprintln!("usage: probe --fs-batch <listIndex> [v1 v2 …]");
            std::process::exit(2);
        }
        let values: Vec<f32> = args[i + 2..].iter().map_while(|s| s.parse().ok()).collect();
        match tmp_companion_lib::probe_fs_batch(list_index, values) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--defer-scenes") {
        // --defer-scenes <listIndex> <groupId> <nodeId> <scene:value,…>
        // TMP_DEFER_FINAL=asis|return|base picks the final-save shape.
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let group = args.get(i + 2).cloned().unwrap_or_default();
        let node = args.get(i + 3).cloned().unwrap_or_default();
        // Reject a malformed scene:value pair instead of silently dropping it (which
        // would produce a valid-looking deferred-scene run for a different scene set).
        let writes: Vec<(u32, f32)> = match args.get(i + 4) {
            Some(spec) => match spec
                .split(',')
                .map(|pair| {
                    let (sc, v) = pair.split_once(':').ok_or(())?;
                    Ok((sc.parse().map_err(|_| ())?, v.parse().map_err(|_| ())?))
                })
                .collect::<Result<Vec<(u32, f32)>, ()>>()
            {
                Ok(w) => w,
                Err(()) => {
                    eprintln!("[probe] --defer-scenes: invalid scene:value in '{spec}'");
                    std::process::exit(2);
                }
            },
            None => Vec::new(),
        };
        if list_index == u32::MAX || group.is_empty() || node.is_empty() || writes.is_empty() {
            eprintln!("usage: probe --defer-scenes <listIndex> <groupId> <nodeId> <scene:value,…>");
            std::process::exit(2);
        }
        match tmp_companion_lib::probe_defer_scenes(list_index, group, node, writes) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --jointk-scenes <listIndex> <defaultTarget> <topology> <save:0|1> [Name=Target ...]
    // Faithful UI-path repro: the REAL level_scenes_oneshot per scene (one call per
    // scene, like the wizard), with per-name target overrides. save=1 persists —
    // point it at a scratch preset.
    if let Some(i) = args.iter().position(|a| a == "--jointk-scenes") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let default_target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        let save = matches!(
            args.get(i + 4).map(|s| s.as_str()),
            Some("1" | "true" | "save" | "yes")
        );
        if list_index == u32::MAX || default_target.is_nan() || topology.is_empty() {
            eprintln!("usage: probe --jointk-scenes <listIndex> <defaultTarget> <topology> <save:0|1> [Name=Target ...]");
            std::process::exit(2);
        }
        let overrides = parse_target_overrides(&args, i + 5);
        eprintln!(
            "[probe] jointk-scenes list_index={list_index} default={default_target} topology={topology} save={save} overrides={overrides:?}…"
        );
        match tmp_companion_lib::probe_jointk_scenes(
            list_index,
            default_target,
            topology,
            save,
            overrides,
        ) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(i) = args.iter().position(|a| a == "--redistribute") {
        // --redistribute <listIndex> <target> <topology> <worstDeficitDb>  (PR5; SAVES — scratch)
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let target: f64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        let worst: f64 = args
            .get(i + 4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN);
        if list_index == u32::MAX || target.is_nan() || topology.is_empty() || worst.is_nan() {
            eprintln!(
                "usage: probe --redistribute <listIndex> <target> <topology> <worstDeficitDb>  (SAVES — point at a scratch preset)"
            );
            std::process::exit(2);
        }
        eprintln!("[probe] redistribute idx={list_index} target={target} topology={topology} worstDeficit={worst}…");
        match tmp_companion_lib::probe_redistribute(list_index, target, topology, worst) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --scene-knob-authority <listIndex> <sceneSlot> <topology> : measure whether the
    // active amp outputLevel moves the scene's loudness (global vs scene-edit). No save.
    if let Some(i) = args.iter().position(|a| a == "--scene-knob-authority") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let scene_slot: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        if list_index == u32::MAX || scene_slot == u32::MAX || topology.is_empty() {
            eprintln!("usage: probe --scene-knob-authority <listIndex> <sceneSlot> <topology>");
            std::process::exit(2);
        }
        eprintln!("[probe] scene-knob-authority list_index={list_index} scene={scene_slot} topology={topology}…");
        match tmp_companion_lib::probe_scene_knob_authority(list_index, scene_slot, topology) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --mute-floor <listIndex> <sceneSlot> <topology> : for a 2-amp merged scene, measure the
    // combined output, the both-lanes-muted floor, and each lane solo + margin (rebalance
    // mute-isolation check). No save.
    if let Some(i) = args.iter().position(|a| a == "--mute-floor") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let scene_slot: u32 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let topology = args.get(i + 3).cloned().unwrap_or_default();
        if list_index == u32::MAX || scene_slot == u32::MAX || topology.is_empty() {
            eprintln!("usage: probe --mute-floor <listIndex> <sceneSlot> <topology>");
            std::process::exit(2);
        }
        eprintln!(
            "[probe] mute-floor list_index={list_index} scene={scene_slot} topology={topology}…"
        );
        match tmp_companion_lib::probe_mute_floor(list_index, scene_slot, topology) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --measure-scene-levels <listIndex> <topology> : measure each saved scene's loudness.
    if let Some(i) = args.iter().position(|a| a == "--measure-scene-levels") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let topology = args.get(i + 2).cloned().unwrap_or_default();
        if list_index == u32::MAX || topology.is_empty() {
            eprintln!("usage: probe --measure-scene-levels <listIndex> <topology>");
            std::process::exit(2);
        }
        eprintln!(
            "[probe] measuring saved scene levels list_index={list_index} topology={topology}…"
        );
        match tmp_companion_lib::probe_measure_scene_levels(list_index, topology) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --scene-amp-diag <listIndex> : per-scene amp bypass + level-control dump (read-only).
    if let Some(i) = args.iter().position(|a| a == "--scene-amp-diag") {
        let list_index: u32 = args
            .get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        if list_index == u32::MAX {
            eprintln!("usage: probe --scene-amp-diag <listIndex>");
            std::process::exit(2);
        }
        eprintln!("[probe] scene amp diagnostic for list_index={list_index}…");
        match tmp_companion_lib::probe_scene_amp_diag(list_index) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --bulk-dryrun <folder> <opspec.json|inline-json> : preview a bulk op, no writes.
    if let Some(i) = args.iter().position(|a| a == "--bulk-dryrun") {
        let folder = args.get(i + 1).cloned().unwrap_or_default();
        let opspec = opspec_arg(args.get(i + 2));
        if folder.is_empty() || opspec.is_empty() {
            eprintln!("usage: probe --bulk-dryrun <preset-folder> <opspec.json|inline-json>");
            std::process::exit(2);
        }
        eprintln!("[probe] bulk dry-run over {folder}…");
        match tmp_companion_lib::probe_bulk_dryrun(&folder, &opspec) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // --bulk-apply <folder> <opspec> [--slots a,b,c] [revert] : apply on the device.
    if let Some(i) = args.iter().position(|a| a == "--bulk-apply") {
        let folder = args.get(i + 1).cloned().unwrap_or_default();
        let opspec = opspec_arg(args.get(i + 2));
        if folder.is_empty() || opspec.is_empty() {
            eprintln!("usage: probe --bulk-apply <preset-folder> <opspec.json|inline-json> [--slots a,b,c] [revert]");
            std::process::exit(2);
        }
        let slots = args
            .iter()
            .position(|a| a == "--slots")
            .and_then(|j| args.get(j + 1))
            .map(|csv| {
                csv.split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect::<Vec<_>>()
            });
        let revert = args.iter().any(|a| a == "revert");
        eprintln!("[probe] bulk apply over {folder} (slots={slots:?}, revert={revert})…");
        match tmp_companion_lib::probe_bulk_apply(&folder, &opspec, slots, revert) {
            Ok(r) => {
                println!("{r}");
                return;
            }
            Err(e) => {
                eprintln!("[probe] FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // Default: connect + list My Presets (HID stack sanity check).
    eprintln!("[probe] connecting to TMP (seizing device)…");
    match tmp_companion_lib::probe_connect_and_list() {
        Ok(presets) => {
            println!("[probe] OK — {} presets in My Presets:", presets.len());
            for p in presets.iter().take(20) {
                println!("  idx {:>3} · slot {:>3}  {}", p.slot, p.slot + 1, p.name);
            }
            if presets.len() > 20 {
                println!("  … and {} more", presets.len() - 20);
            }
            if presets.is_empty() {
                eprintln!("[probe] WARNING: connected but the list was empty");
                std::process::exit(2);
            }
        }
        Err(e) => {
            eprintln!("[probe] FAILED: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_target_overrides;

    // The regression that aborted the probe arm: `from` past the end of `args` must be
    // an empty override list, not a slice panic.
    #[test]
    fn overrides_from_past_end_is_empty() {
        let args: Vec<String> = vec!["--level-preset-scenes".into(), "26".into()];
        assert!(parse_target_overrides(&args, 7).is_empty());
    }

    #[test]
    fn overrides_parse_name_target_pairs_and_skip_junk() {
        let args: Vec<String> = ["a", "Clean=-23.5", "junk", "Base=-20"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got = parse_target_overrides(&args, 1);
        assert_eq!(
            got,
            vec![("Clean".to_string(), -23.5), ("Base".to_string(), -20.0)]
        );
    }
}
