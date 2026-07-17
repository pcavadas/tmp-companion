use super::super::*;
use tauri::test::MockRuntime;
use tauri::WebviewWindow;

/// The transport factory + startup snapshot are process-GLOBAL; cargo runs tests in
/// parallel, so the factory-installing tests must hold this for their whole body or
/// they stomp each other's fake (a hard-to-spot cross-contamination).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    lock_ok(&SERIAL)
}

/// Invoke a command through the SAME IPC path the HTTP bridge uses: a JSON body in,
/// the command's JSON response out (or its error value).
fn invoke(
    webview: &WebviewWindow<MockRuntime>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    tauri::test::get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|b| b.deserialize::<serde_json::Value>().expect("json body"))
}

/// The full OFFLINE Copy journey driven through the real backend exactly as the UI
/// drives it — connect → list presets → read the library → copy_apply — with the
/// device replaced by a `SimDevice` (via the transport factory) and the bulk backup
/// replaced by the built fixture blob. This is "UI to unit" minus the browser: every
/// command runs for real over the mock IPC; only the USB transport + the snapshot are
/// faked. The HTTP bridge + Playwright layer reuses this exact wiring.
#[test]
fn offline_copy_journey_through_real_backend() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = serial();

    // One shared fake: every Session::connect* (command lane) clones it.
    let sim = crate::sim_device::SimDevice::new();
    let sim_for_factory = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim_for_factory.clone())));
    // The library read decodes the fixture blob through the real backup path.
    std::env::set_var(
        "TMP_E2E_BACKUP_FIXTURE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/backup-fixture.bin"
        ),
    );
    // Pre-fill the startup snapshot so connect/list serve it with no monitor thread —
    // the 3 scenario presets at slots 400/401/402 (matching the backup fixture).
    let presets = vec![
        crate::session::PresetEntry {
            slot: 400,
            name: "E2E Reference".into(),
        },
        crate::session::PresetEntry {
            slot: 401,
            name: "E2E Target 1".into(),
        },
        crate::session::PresetEntry {
            slot: 402,
            name: "E2E Target 2".into(),
        },
    ];
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            connect_device,
            list_presets,
            read_library_via_backup,
            copy_apply
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build webview");

    // 1. connect → the pre-filled snapshot (firmware).
    let conn = invoke(&webview, "connect_device", serde_json::json!({})).expect("connect");
    assert_eq!(
        conn.get("firmware").and_then(|v| v.as_str()),
        Some("1.8.45")
    );

    // 2. list presets → the snapshot's 3 fixture entries.
    let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
    assert_eq!(list.as_array().map(|a| a.len()), Some(3), "presets: {list}");

    // 3. read the library via the fixture backup → 3 rows, decoded graphs.
    let lib = invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
    let rows = lib
        .get("presets")
        .and_then(|p| p.as_array())
        .expect("library presets array");
    assert_eq!(rows.len(), 3, "library rows: {lib}");
    assert!(
        rows.iter()
            .any(|r| r.get("graph").is_some_and(|g| !g.is_null())),
        "at least one library row carries a decoded signal graph: {lib}"
    );

    // 4. copy_apply a dry-run replace on the target → outcome "updated", NOTHING saved.
    // The job is the exact camelCase wire shape `CopyJob`/`CopyOp`/`CopyRepl` accept
    // (the input-only structs the frontend's `diffToOps` produces). The fake confirms
    // any structural edit, so the nodeId need not match a fixture node.
    let jobs = serde_json::json!([{
        "listIndex": 401,
        "name": "E2E Target 1",
        "ops": [{
            "kind": "replace",
            "group": "G1",
            "nodeId": "ACD_PhaserP90",
            "repl": { "kind": "model", "fenderId": "ACD_KingOfTone" }
        }]
    }]);
    let items = invoke(
        &webview,
        "copy_apply",
        serde_json::json!({ "jobs": jobs, "save": false, "onResult": "__CHANNEL__:0" }),
    )
    .expect("copy_apply");
    let items = items.as_array().expect("copy items");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("outcome").and_then(|v| v.as_str()),
        Some("updated"),
        "copy outcome: {items:?}"
    );
    let ev = sim.events();
    assert!(
        ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Replace { .. })),
        "the replace reached the fake: {ev:?}"
    );
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
        "dry run must not save: {ev:?}"
    );
}

/// The Level journey's measure→solve→apply path runs end-to-end OFFLINE: the device
/// goes through the `SimDevice` factory and the re-amp capture through the
/// `--features e2e` audio fake (`audio::reamp_capture` returns the stimulus), so the
/// leveler produces a finite `C` / final level with no hardware. Proves the audio
/// seam the UI Level run depends on; loudness fidelity stays the online tier's job.
#[test]
fn offline_level_preset_runs_against_the_fake_audio() {
    let _serial = serial();
    let sim = crate::sim_device::SimDevice::new();
    let sim_for_factory = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim_for_factory.clone())));

    // 0.5 s of a 440 Hz tone at 48 kHz — non-silent so the loudness meter is finite.
    let rate = 48_000usize;
    let stim: Vec<f32> = (0..rate / 2)
        .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect();

    let opts = crate::leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let r = crate::leveller::level_preset(0, &stim, -30.0, opts, &[], None, || false)
        .expect("level_preset");
    assert!(
        r.final_level.is_finite() && r.final_level > 0.0,
        "solved a finite level: {r:?}"
    );
    assert!(
        r.measured_lufs.is_finite(),
        "measured a finite loudness: {r:?}"
    );
    // Dry run — the fake recorded a level set but no save.
    let ev = sim.events();
    assert!(
        ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
        "the level setter reached the fake: {ev:?}"
    );
    assert!(
        !ev.iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
        "save:false must not save: {ev:?}"
    );
}

/// Songs CRUD through the real backend over the mock IPC: the SimDevice models the
/// song wire protocol (list / add / rename / remove), so `list_songs` reads the seed,
/// a write mutates it, and the read-back reflects the change — the Songs tab's
/// read-after-write contract, with no hardware.
#[test]
fn offline_songs_crud_through_real_backend() {
    let _serial = serial();
    let sim = crate::sim_device::SimDevice::new();
    let sim_for_factory = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim_for_factory.clone())));

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_songs,
            read_setlists,
            add_song,
            rename_song,
            remove_song
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build webview");

    // Seed: 2 songs, 1 setlist.
    let songs = invoke(&webview, "list_songs", serde_json::json!({})).expect("list_songs");
    assert_eq!(
        songs.as_array().map(|a| a.len()),
        Some(2),
        "seed songs: {songs}"
    );
    let setlists = invoke(&webview, "read_setlists", serde_json::json!({})).expect("read_setlists");
    assert_eq!(
        setlists.as_array().map(|a| a.len()),
        Some(1),
        "seed setlists: {setlists}"
    );

    // Add → read-back reflects it.
    let after_add = invoke(
        &webview,
        "add_song",
        serde_json::json!({ "name": "Soundcheck" }),
    )
    .expect("add_song");
    assert_eq!(
        after_add.as_array().map(|a| a.len()),
        Some(3),
        "after add: {after_add}"
    );
    assert!(sim.song_names().iter().any(|n| n == "Soundcheck"));

    // Remove the first → back to 2.
    let after_rm = invoke(
        &webview,
        "remove_song",
        serde_json::json!({ "slot": 1, "expectName": "Opening Set" }),
    )
    .expect("remove_song");
    assert_eq!(
        after_rm.as_array().map(|a| a.len()),
        Some(2),
        "after remove: {after_rm}"
    );
    assert!(!sim.song_names().iter().any(|n| n == "Opening Set"));
}

/// The stray classifier flags scenario names at the WRONG slot only — the legitimate
/// scenario slots and real user presets are never candidates (the HW incident: 13
/// stray "E2E Reference" copies stranded at 27–39 by aborted seeds).
#[test]
fn scenario_strays_flags_wrong_slot_copies_only() {
    let spec: Vec<super::ScenarioPreset> = serde_json::from_str(
        r#"[
            {"listIndex": 400, "name": "E2E Reference", "presetJson": ""},
            {"listIndex": 401, "name": "E2E Target 1", "presetJson": ""}
        ]"#,
    )
    .expect("spec json");
    let entry = |slot: u32, name: &str| session::PresetEntry {
        slot,
        name: name.into(),
    };
    let list = vec![
        entry(27, "E2E Reference"),  // stray (aborted-seed leftover)
        entry(39, "E2E Reference"),  // stray
        entry(40, "Guitar Boost"),   // real preset — untouched
        entry(400, "E2E Reference"), // legitimate scenario slot
        entry(401, "E2E Reference"), // scenario NAME at another scenario's slot → stray
        entry(402, "--"),            // empty
    ];
    let strays = super::scenario_strays(&list, &spec);
    assert_eq!(
        strays,
        vec![
            (27, "E2E Reference".to_string()),
            (39, "E2E Reference".to_string()),
            (401, "E2E Reference".to_string()),
        ]
    );
    // No spec → nothing is ever a stray.
    assert!(super::scenario_strays(&list, &[]).is_empty());
}
