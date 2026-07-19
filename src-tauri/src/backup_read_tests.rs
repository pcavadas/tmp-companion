use super::*;
use crate::session;

#[test]
fn backup_preset_scenes_parse_names_and_fs_tags() {
    // The DB presetJson is the same plaintext shape as the live field-3 doc:
    // scenes[].sceneName slot-ordered + an ftsw map assigning footswitches to
    // scene slots. decode_preset_scenes must yield names + 1-based FS tags.
    let json = br#"{"ftsw":[[{"func":"scene","sceneSlot":1,"isActive":true}],[{"func":"scene","sceneSlot":2,"isActive":true}]],"lastLoadedScene":0,"scenes":[{"sceneName":"Rhythm","uuid":"a"},{"sceneName":"Crunch","uuid":"b"},{"sceneName":"Lead","uuid":"c"}]}"#;
    let ps = decode_preset_scenes(json).expect("parse");
    assert_eq!(ps.scenes, vec!["Rhythm", "Crunch", "Lead"]);
    // ftsw assigns FS1→scene 1, FS2→scene 2 (1-based tag = switch index + 1);
    // scene 0 has no footswitch.
    assert_eq!(ps.fs, vec![None, Some(1), Some(2)]);
}

/// Build a real in-memory device-backup archive from one SQL script: run it through
/// `sqlite3` to a fresh temp `normalDb.db3`, tar it under the firmware's logical
/// `databaseBackup` entry name, then LZ4-frame compress — the exact shape
/// `read_backup_archive` decodes. Shared by the two backup-decode tests and the
/// showcase-fixture generator (a per-call counter keeps parallel temp dirs distinct).
fn build_backup_archive(sql: &str) -> Vec<u8> {
    build_backup_archive_with_settings(sql, None)
}

/// Same recipe as [`build_backup_archive`], with an optional extra `settingsBackup`
/// tar entry (the device's settings.json) so the settings-capture path can be
/// exercised without duplicating the whole sqlite→tar→lz4 pipeline.
fn build_backup_archive_with_settings(sql: &str, settings_json: Option<&[u8]>) -> Vec<u8> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let uniq = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tmp-companion-backup-{}-{uniq}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("normalDb.db3");
    let _ = std::fs::remove_file(&db_path);
    let status = std::process::Command::new("sqlite3")
        .arg(&db_path)
        .arg(sql)
        .status()
        .expect("spawn sqlite3");
    assert!(status.success(), "sqlite3 create failed");
    let db_bytes = std::fs::read(&db_path).expect("read db");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir(&dir);

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(db_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "databaseBackup",
                std::io::Cursor::new(&db_bytes),
            )
            .expect("tar append");
        if let Some(settings) = settings_json {
            let mut sheader = tar::Header::new_gnu();
            sheader.set_size(settings.len() as u64);
            sheader.set_mode(0o644);
            sheader.set_cksum();
            builder
                .append_data(
                    &mut sheader,
                    "settingsBackup",
                    std::io::Cursor::new(settings),
                )
                .expect("tar append settings");
        }
        builder.finish().expect("tar finish");
    }
    let mut archive = Vec::new();
    {
        let mut enc = lz4_flex::frame::FrameEncoder::new(&mut archive);
        enc.write_all(&tar_bytes).expect("lz4 write");
        enc.finish().expect("lz4 finish");
    }
    archive
}

/// A decoded backup row carries the routed signal-chain `graph` (lanes/stages)
/// for a NON-active preset — the "Copy blocks between presets" frontend reads each
/// preset's real topology from this without a device round-trip. Builds a real
/// in-memory backup archive (sqlite → tar `databaseBackup` → LZ4-frame) so the
/// FULL `read_backup_archive` path is exercised, then asserts the multi-block
/// `gtrSeries` preset's row has a non-empty `graph.stages` (and `graph.nodes`).
#[test]
fn backup_row_carries_routed_graph() {
    // A known multi-block series preset doc (the live field-3 shape): two guitar
    // blocks in G1 under a `gtrSeries` template → extract_active_graph yields one
    // Series stage with both nodes.
    let preset_json = r#"{"info":{"displayName":"Copy Test","userSlot":3},"audioGraph":{"template":"gtrSeries","guitarNodes":{"G1":[{"nodeId":"ACD_Comp","FenderId":"ACD_Comp","dspUnitParameters":{"bypass":false}},{"nodeId":"ACD_TwinReverb","FenderId":"ACD_TwinReverb","dspUnitParameters":{"bypass":false}}]}},"scenes":[{"sceneName":"Rhythm","uuid":"a"}]}"#;

    let archive = build_backup_archive(&format!(
        "CREATE TABLE UserPresets(slot INTEGER, displayName TEXT, presetJson TEXT); \
             INSERT INTO UserPresets VALUES (3, 'Copy Test', '{}');",
        preset_json.replace('\'', "''")
    ));

    let result = read_backup_archive(&archive).expect("decode archive");
    let row = result
        .presets
        .iter()
        .find(|p| p.name == "Copy Test")
        .expect("the row is present");
    // Block roster survives (the pre-existing field) …
    assert_eq!(row.blocks.len(), 2, "two-block roster");
    // … AND the new routed graph carries the topology the frontend renders.
    assert_eq!(row.graph.template.as_deref(), Some("gtrSeries"));
    assert!(
        !row.graph.stages.is_empty(),
        "graph.stages must be non-empty"
    );
    assert!(!row.graph.nodes.is_empty(), "graph.nodes must be non-empty");
    let session::Stage::Series { blocks } = &row.graph.stages[0] else {
        panic!("gtrSeries → a Series stage");
    };
    assert_eq!(blocks.len(), 2, "both blocks in the series stage");
}

/// A backup archive carries the song→preset bindings out of the `SongPresets`
/// table (Songs.slot → UserPresets.slot), so the Songs tab's Presets axis can show
/// "which songs use this preset" with ZERO new device reads. Builds the full
/// archive (sqlite → tar → LZ4) with `Songs` + `SongPresets` rows and asserts the
/// decoded `song_presets` carry the expected `{song_slot, preset_slot}` pairs.
#[test]
fn backup_carries_song_preset_bindings() {
    // UserPresets (slot 8, 58) + two Songs + three SongPresets bindings:
    //   Song slot 1 → presets 8, 58 ; Song slot 2 → preset 58.
    let archive = build_backup_archive(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             INSERT INTO UserPresets VALUES (10, 8, 'Plexi Crunch', '{}'); \
             INSERT INTO UserPresets VALUES (11, 58, 'Stadium Lead', '{}'); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             INSERT INTO Songs VALUES (1, 1, 'Song A'); \
             INSERT INTO Songs VALUES (2, 2, 'Song B'); \
             CREATE TABLE SongPresets(id INTEGER PRIMARY KEY, Songs_id INTEGER, UserPresets_id INTEGER, slot INTEGER); \
             INSERT INTO SongPresets VALUES (1, 1, 10, 1); \
             INSERT INTO SongPresets VALUES (2, 1, 11, 2); \
             INSERT INTO SongPresets VALUES (3, 2, 11, 1);",
        );

    let result = read_backup_archive(&archive).expect("decode archive");
    assert_eq!(
        result.song_presets,
        vec![
            SongPresetBinding {
                song_slot: 1,
                preset_slot: 8
            },
            SongPresetBinding {
                song_slot: 1,
                preset_slot: 58
            },
            SongPresetBinding {
                song_slot: 2,
                preset_slot: 58
            },
        ],
    );
}

/// The Songs tab is sourced from the startup backup: the archive carries the full
/// `Songs` (name/notes/bpm) + `Setlists` (name) + `SetlistSongs` (membership) tables,
/// so the tab can paint with ZERO live device reads. Builds the full archive (sqlite →
/// tar → LZ4) and asserts the decoded `songs`/`setlists`/`setlist_songs`.
#[test]
fn backup_carries_songs_setlists_membership() {
    let archive = build_backup_archive(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT, notes TEXT, bpmActive INTEGER, bpm INTEGER); \
             INSERT INTO Songs VALUES (1, 1, 'Opener', 'capo 2', 1, 128); \
             INSERT INTO Songs VALUES (2, 2, 'Ballad', '', 0, 72); \
             CREATE TABLE Setlists(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             INSERT INTO Setlists VALUES (1, 1, 'Main Set'); \
             CREATE TABLE SetlistSongs(id INTEGER PRIMARY KEY, Setlists_id INTEGER, Songs_id INTEGER, slot INTEGER); \
             INSERT INTO SetlistSongs VALUES (1, 1, 2, 1); \
             INSERT INTO SetlistSongs VALUES (2, 1, 1, 2);",
        );
    let result = read_backup_archive(&archive).expect("decode archive");
    assert_eq!(
        result.songs,
        vec![
            session::SongRecord {
                slot: 1,
                name: "Opener".into(),
                notes: "capo 2".into(),
                bpm: 128,
                bpm_active: true
            },
            session::SongRecord {
                slot: 2,
                name: "Ballad".into(),
                notes: String::new(),
                bpm: 72,
                bpm_active: false
            },
        ],
    );
    assert_eq!(
        result.setlists,
        vec![session::SetlistRecord {
            slot: 1,
            name: "Main Set".into()
        }],
    );
    // Setlist 1 holds song slot 2 at position 1, song slot 1 at position 2.
    assert_eq!(
        result.setlist_songs,
        vec![
            BackupSetlistSong {
                setlist_slot: 1,
                song_slot: 2,
                position: 1
            },
            BackupSetlistSong {
                setlist_slot: 1,
                song_slot: 1,
                position: 2
            },
        ],
    );
}

/// The archive's `settingsBackup` entry (the device's settings.json) round-trips
/// into `BackupReadResult::settings_bytes` — the capture the command layer persists
/// to `<app_config_dir>/support/device-settings.json` for a later support bundle.
#[test]
fn backup_carries_settings_bytes_when_present() {
    let settings_json = br#"{"foo":"bar"}"#;
    let archive = build_backup_archive_with_settings(
        "CREATE TABLE UserPresets(slot INTEGER, displayName TEXT, presetJson TEXT);",
        Some(settings_json),
    );
    let result = read_backup_archive(&archive).expect("decode archive");
    assert_eq!(result.settings_bytes.as_deref(), Some(&settings_json[..]));
    assert!(
        result.members.iter().any(|(p, _)| p == "settingsBackup"),
        "settingsBackup should be listed among the archive members"
    );
}

/// An archive WITHOUT a `settingsBackup` entry (e.g. the e2e fixture) decodes fine
/// and leaves `settings_bytes` `None` — the existing behavior this change must not
/// disturb.
#[test]
fn backup_settings_bytes_none_without_settings_entry() {
    let archive = build_backup_archive(
        "CREATE TABLE UserPresets(slot INTEGER, displayName TEXT, presetJson TEXT);",
    );
    let result = read_backup_archive(&archive).expect("decode archive");
    assert_eq!(result.settings_bytes, None);
}

/// GENERATOR (not a gate — `#[ignore]`): expand the curated, non-personal
/// `e2e/fixtures/showcase/showcase.json` into a real device-backup archive
/// (`showcase-fixture.bin`) the marketing-screenshot tour decodes through the SAME
/// `read_backup_archive` path the app uses. Reuses the archive-building recipe of
/// `backup_row_carries_routed_graph` / `backup_carries_song_preset_bindings`
/// (sqlite → tar `databaseBackup` → LZ4-frame). Run with:
///   `cargo test --features e2e build_showcase_fixture -- --ignored`
/// (also chained by `bun run screenshots`). Committed output, regenerated only when
/// `showcase.json` changes.
#[test]
#[ignore = "generator: writes showcase-fixture.bin from showcase.json"]
fn build_showcase_fixture() {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase.json"
    );
    let spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(src).expect("read showcase.json"))
            .expect("parse showcase.json");

    // Expand one compact preset spec → a full `.preset` doc (the live field-3 shape).
    let build_preset_json = |p: &serde_json::Value| -> String {
        let slot = p["slot"].as_u64().expect("slot");
        let name = p["name"].as_str().expect("name");
        let template = p["template"].as_str().unwrap_or("gtrSeries");
        let mk_node = |fender: &str| {
            serde_json::json!({
                "FenderId": fender,
                "nodeId": fender,
                "nodeType": "dspUnit",
                "dspUnitParameters": { "bypass": false }
            })
        };
        // G1..G7 (+ M1..M4 empty) — empty for any group the spec omits.
        let mut guitar = serde_json::Map::new();
        for g in ["G1", "G2", "G3", "G4", "G5", "G6", "G7"] {
            let arr: Vec<_> = p["groups"][g]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(mk_node).collect())
                .unwrap_or_default();
            guitar.insert(g.to_string(), serde_json::Value::Array(arr));
        }
        let mut mic = serde_json::Map::new();
        for m in ["M1", "M2", "M3", "M4"] {
            mic.insert(m.to_string(), serde_json::json!([]));
        }
        // Named scenes (count + names drive the Level list's "N scenes" breakdown).
        let scenes: Vec<_> = p["scenes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .enumerate()
                        .map(|(i, s)| {
                            serde_json::json!({
                                "sceneName": s,
                                "uuid": format!("5cffe000-0000-0000-0000-{:012}", slot * 100 + i as u64),
                                "guitarNodes": {}
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
        let doc = serde_json::json!({
            "audioGraph": {
                "guitarNodes": guitar,
                "micNodes": mic,
                "presetLevel": 0.5,
                "template": template
            },
            "scenes": scenes,
            "info": { "displayName": name, "userSlot": slot, "version": "5.0" }
        });
        doc.to_string()
    };

    // Build the SQL: UserPresets (id = slot) + Songs (id = ordinal) + SongPresets.
    let mut sql = String::from(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             CREATE TABLE SongPresets(id INTEGER PRIMARY KEY, Songs_id INTEGER, UserPresets_id INTEGER, slot INTEGER);",
        );
    for p in spec["presets"].as_array().expect("presets") {
        let slot = p["slot"].as_u64().expect("slot");
        let name = p["name"].as_str().expect("name").replace('\'', "''");
        let json = build_preset_json(p).replace('\'', "''");
        sql.push_str(&format!(
            " INSERT INTO UserPresets VALUES ({slot}, {slot}, '{name}', '{json}');"
        ));
    }
    let mut sp_id = 0u64;
    for (si, song) in spec["songs"].as_array().expect("songs").iter().enumerate() {
        let song_id = si as u64 + 1; // Songs.id; slot = positional (1-based)
        let name = song["name"]
            .as_str()
            .expect("song name")
            .replace('\'', "''");
        sql.push_str(&format!(
            " INSERT INTO Songs VALUES ({song_id}, {song_id}, '{name}');"
        ));
        for (pi, ps) in song["presets"].as_array().into_iter().flatten().enumerate() {
            sp_id += 1;
            let preset_slot = ps.as_u64().expect("preset slot"); // = UserPresets.id
            let ord = pi as u64 + 1;
            sql.push_str(&format!(
                " INSERT INTO SongPresets VALUES ({sp_id}, {song_id}, {preset_slot}, {ord});"
            ));
        }
    }

    // sqlite → tar `databaseBackup` → LZ4-frame, the exact device-backup shape.
    let archive = build_backup_archive(&sql);

    // Self-check: the archive round-trips through the real decoder before we commit it.
    let decoded = read_backup_archive(&archive).expect("decode generated archive");
    let n_presets = spec["presets"].as_array().unwrap().len();
    assert_eq!(decoded.presets.len(), n_presets, "all presets decode");
    let active = spec["activeSlot"].as_u64().unwrap();
    let hero = decoded
        .presets
        .iter()
        .find(|r| r.slot as u64 == active)
        .expect("active preset present");
    assert!(
        !hero.graph.stages.is_empty(),
        "hero graph has routed stages"
    );

    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase-fixture.bin"
    );
    std::fs::write(out, &archive).expect("write showcase-fixture.bin");
    eprintln!(
        "build_showcase_fixture: wrote {} bytes ({} presets, {} songs) → {out}",
        archive.len(),
        n_presets,
        spec["songs"].as_array().unwrap().len(),
    );
}

/// GENERATOR (run explicitly, ignored otherwise): derive
/// `e2e/fixtures/backup-fixture.bin` from `e2e/fixtures/scenario-presets.json`
/// — the ONE source of truth for the scenario presets (the online seed imports
/// the same JSONs). Keeps the two fixtures in sync mechanically instead of by
/// discipline. Schema mirrors the shipped fixture (UserPresets with `isEmpty`;
/// device userSlot = listIndex + 1). Run with:
///   `cargo test build_scenario_fixture -- --ignored`
/// Committed output, regenerated only when `scenario-presets.json` changes.
/// `scenario-presets.json` → the UserPresets SQL script the scenario fixture
/// archive is built from. Shared by the (ignored) generator and the non-ignored
/// drift lock below, so the two can never diverge on construction.
fn scenario_fixture_sql() -> (String, usize) {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/scenario-presets.json"
    );
    let spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(src).expect("read scenario-presets.json"))
            .expect("parse scenario-presets.json");
    let presets = spec.as_array().expect("array of scenario presets");

    let mut sql = String::from(
        "CREATE TABLE UserPresets (id INTEGER PRIMARY KEY, slot INTEGER, isEmpty INTEGER, \
         displayName TEXT, presetJson BLOB);",
    );
    for p in presets {
        let slot = p["listIndex"].as_u64().expect("listIndex") + 1; // device userSlot
        let name = p["name"].as_str().expect("name").replace('\'', "''");
        let json = p["presetJson"]
            .as_str()
            .expect("presetJson")
            .replace('\'', "''");
        sql.push_str(&format!(
            " INSERT INTO UserPresets VALUES ({slot}, {slot}, 0, '{name}', '{json}');"
        ));
    }
    (sql, presets.len())
}

/// DRIFT LOCK (non-ignored, runs in CI): the committed `backup-fixture.bin` must
/// stay in sync with `scenario-presets.json` — the offline specs read the former,
/// the online seed imports the latter, and they must describe the SAME presets.
/// Regenerates the archive in memory through the exact generator construction and
/// compares the DECODED rows (content, not bytes — sqlite/tar/LZ4 output is not
/// byte-stable across environments). On failure: rerun
/// `cargo test build_scenario_fixture -- --ignored` and commit the result.
#[test]
fn scenario_fixture_matches_scenario_presets_json() {
    let (sql, n) = scenario_fixture_sql();
    let regenerated =
        read_backup_archive(&build_backup_archive(&sql)).expect("decode regenerated archive");
    let committed_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/backup-fixture.bin"
    ))
    .expect("read committed backup-fixture.bin");
    let committed = read_backup_archive(&committed_bytes).expect("decode committed fixture");
    assert_eq!(regenerated.presets.len(), n, "all scenario presets decode");
    assert_eq!(
        serde_json::to_value(&regenerated.presets).expect("serialize regenerated"),
        serde_json::to_value(&committed.presets).expect("serialize committed"),
        "backup-fixture.bin is out of sync with scenario-presets.json — rerun \
         `cargo test build_scenario_fixture -- --ignored` and commit the regenerated fixture"
    );
}

#[test]
#[ignore = "generator: writes backup-fixture.bin from scenario-presets.json"]
fn build_scenario_fixture() {
    let (sql, n_presets) = scenario_fixture_sql();
    let archive = build_backup_archive(&sql);

    // Round-trip through the real decoder before committing the bytes.
    let decoded = read_backup_archive(&archive).expect("decode generated archive");
    assert_eq!(decoded.presets.len(), n_presets, "all presets decode");
    let reference = decoded
        .presets
        .iter()
        .find(|r| r.name == "E2E Reference")
        .expect("Reference present");
    assert!(
        !reference.graph.stages.is_empty(),
        "Reference graph has routed stages"
    );
    assert!(
        !reference.footswitches.is_empty(),
        "Reference keeps its block-acting footswitches"
    );

    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/backup-fixture.bin"
    );
    std::fs::write(out, &archive).expect("write backup-fixture.bin");
    eprintln!(
        "build_scenario_fixture: wrote {} bytes ({} presets) → {out}",
        archive.len(),
        n_presets,
    );
}

// --- silence_hint: the Layer-1 static pre-flight for the "not on USB 1/2" clamp ---
// A silent leveling capture has two JSON-visible causes the generic routing verdict
// hides: the amp's outputLevel saved at 0 (definite silence), and an expression-pedal
// binding whose zero end mutes the amp when a physical pedal sits there (conditional).

#[test]
fn silence_hint_flags_zeroed_active_amp() {
    let v = serde_json::json!({"audioGraph":{"guitarNodes":{"G1":[
        {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
         "dspUnitParameters":{"bypass":false,"outputLevel":0.0}}]}}});
    assert_eq!(silence_hint(&v), Some("amp_zero"));
}

#[test]
fn silence_hint_ignores_bypassed_amp_at_zero() {
    // A bypassed amp passes signal flat — outputLevel 0 on it is inert.
    let v = serde_json::json!({"audioGraph":{"guitarNodes":{"G1":[
        {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
         "dspUnitParameters":{"bypass":true,"outputLevel":0.0}}]}}});
    assert_eq!(silence_hint(&v), None);
}

#[test]
fn silence_hint_ignores_zero_when_another_active_amp_is_live() {
    // Parallel-lane case: one amp muted, the other still feeds USB.
    let v = serde_json::json!({"audioGraph":{"guitarNodes":{
        "G1":[{"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
               "dspUnitParameters":{"bypass":false,"outputLevel":0.0}}],
        "G2":[{"FenderId":"ACD_TwinReverb65","nodeId":"ACD_TwinReverb65",
               "dspUnitParameters":{"bypass":false,"outputLevel":0.5}}]}}});
    assert_eq!(silence_hint(&v), None);
}

#[test]
fn silence_hint_flags_exp_binding_with_zero_end() {
    // The HW-confirmed field case (user preset "Rhythm"): exp2 → amp outputLevel,
    // heel 0.0 / liveMode — a pedal parked at heel measures deep digital silence.
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
             "dspUnitParameters":{"bypass":false,"outputLevel":0.68}}]}},
        "exp":{"exp1":[],"exp2":[
            {"func":"param","groupId":"G1","nodeId":"ACD_TweedDeluxe",
             "paramId":"outputLevel","heel":0.0,"toe":0.68,"liveMode":true}]}});
    assert_eq!(silence_hint(&v), Some("exp_mute"));
}

#[test]
fn silence_hint_none_on_healthy_preset() {
    // Live amp + an exp binding whose BOTH ends are non-zero (no mutable position).
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
             "dspUnitParameters":{"bypass":false,"outputLevel":0.68}}]}},
        "exp":{"exp1":[],"exp2":[
            {"func":"param","groupId":"G1","nodeId":"ACD_TweedDeluxe",
             "paramId":"outputLevel","heel":0.3,"toe":1.0}]}});
    assert_eq!(silence_hint(&v), None);
}

#[test]
fn silence_hint_amp_zero_wins_over_exp_binding() {
    // Both present → the definite cause (saved zero) outranks the conditional one.
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
             "dspUnitParameters":{"bypass":false,"outputLevel":0.0}}]}},
        "exp":{"exp2":[
            {"func":"param","paramId":"outputLevel","heel":0.0,"toe":0.68}]}});
    assert_eq!(silence_hint(&v), Some("amp_zero"));
}

#[test]
fn silence_hint_ignores_exp_binding_on_non_amp_node() {
    // The bound groupId/nodeId resolves, but the node isn't an amp (e.g. an EQ block
    // that happens to expose a same-named "outputLevel" param) — must not flag.
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_FiveBandParamEQ","nodeId":"ACD_FiveBandParamEQ",
             "dspUnitParameters":{"bypass":false,"outputLevel":0.68}}]}},
        "exp":{"exp1":[],"exp2":[
            {"func":"param","groupId":"G1","nodeId":"ACD_FiveBandParamEQ",
             "paramId":"outputLevel","heel":0.0,"toe":0.68}]}});
    assert_eq!(silence_hint(&v), None);
}

#[test]
fn silence_hint_ignores_exp_binding_on_missing_node() {
    // A stale binding pointing at a groupId/nodeId no longer present in the graph.
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
             "dspUnitParameters":{"bypass":false,"outputLevel":0.68}}]}},
        "exp":{"exp1":[],"exp2":[
            {"func":"param","groupId":"G1","nodeId":"ACD_GoneNow",
             "paramId":"outputLevel","heel":0.0,"toe":0.68}]}});
    assert_eq!(silence_hint(&v), None);
}

#[test]
fn silence_hint_ignores_exp_binding_on_bypassed_amp() {
    // The bound amp is bypassed — its outputLevel is inert, so a zero-end binding
    // can't mute anything live.
    let v = serde_json::json!({
        "audioGraph":{"guitarNodes":{"G1":[
            {"FenderId":"ACD_TweedDeluxe","nodeId":"ACD_TweedDeluxe",
             "dspUnitParameters":{"bypass":true,"outputLevel":0.68}}]}},
        "exp":{"exp1":[],"exp2":[
            {"func":"param","groupId":"G1","nodeId":"ACD_TweedDeluxe",
             "paramId":"outputLevel","heel":0.0,"toe":0.68}]}});
    assert_eq!(silence_hint(&v), None);
}
