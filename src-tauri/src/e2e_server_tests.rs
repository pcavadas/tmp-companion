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

/// A short non-silent stimulus for the offline capture model (0.5 s @ 440 Hz, 48 kHz).
fn test_stim() -> Vec<f32> {
    let rate = 48_000usize;
    (0..rate / 2)
        .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect()
}

/// Point the offline capture model + seed at the committed fixtures. Each entry is
/// (env var, path relative to `CARGO_MANIFEST_DIR`) — folds the scenario/sidecar/backup/
/// stimulus var setup that the physics gates share into one call (no style fork).
fn set_e2e_env(pairs: &[(&str, &str)]) {
    for (k, v) in pairs {
        std::env::set_var(k, format!("{}{v}", env!("CARGO_MANIFEST_DIR")));
    }
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
    // the 4 scenario presets at slots 400-403 (matching the backup fixture).
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
        crate::session::PresetEntry {
            slot: 403,
            name: "E2E Realistic".into(),
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

    // 2. list presets → the snapshot's 4 fixture entries.
    let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
    assert_eq!(list.as_array().map(|a| a.len()), Some(4), "presets: {list}");

    // 3. read the library via the fixture backup → 4 rows, decoded graphs.
    let lib = invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
    let rows = lib
        .get("presets")
        .and_then(|p| p.as_array())
        .expect("library presets array");
    assert_eq!(rows.len(), 4, "library rows: {lib}");
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
/// `--features e2e` physics-faithful capture model (`audio::reamp_capture` → the
/// SimDevice's `e2e_capture`), so the leveler measures the modeled loudness and solves a
/// finite `C` / final level with no hardware. Proves the audio seam AND the physics
/// wire-in: slot 0 is unlisted in the sidecar → the flat default `C = -18`, so the solved
/// `constant_c` lands there (a `set_live` regression that skipped the model would read the
/// passthrough stimulus's own loudness instead and fail this).
#[test]
fn offline_level_preset_runs_against_the_fake_audio() {
    let _serial = serial();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim); // drive the physics-faithful capture model
    let sim_for_factory = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim_for_factory.clone())));

    let stim = test_stim(); // 0.5 s @ 440 Hz — non-silent so the loudness meter is finite

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
    // The physics wire-in ran: an unlisted slot solves to the sidecar's flat default C.
    assert!(
        (r.constant_c - (-18.0)).abs() < 0.5,
        "the physics model produced the default C=-18 (not a passthrough loudness): {r:?}"
    );
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

/// The physics that drives `level-defaults.spec.ts`: slot 403 (E2E Realistic) at a
/// SHIPPED DEFAULT target (Crunch -24) produces the first-session outcome set — a Base that
/// CLAMPS at its ceiling (headroom, reason-less) and an off-branch footswitch (its block sits
/// on the muted parallel branch → silence → the "no signal on USB 1/2" routing clamp). Fast
/// backend gate for the sidecar authoring, independent of the Playwright UI flow: a sidecar
/// C perturbation or an `offbranch_switch_node` regression flips these here (mutation-check
/// #2/#4). Uses the committed fixture + sidecar via their env overrides.
#[test]
fn level_defaults_403_base_clamps_and_footswitch_is_offbranch() {
    let _serial = serial();
    set_e2e_env(&[
        (
            "TMP_E2E_SCENARIO_PRESETS",
            "/../e2e/fixtures/scenario-presets.json",
        ),
        (
            "TMP_E2E_LOUDNESS_SIDECAR",
            "/../e2e/fixtures/scenario-loudness.json",
        ),
    ]);
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let stim = test_stim();

    // The fixture must actually put the off-branch node on a SPLIT lane (not the trunk), else
    // the ONLINE footswitch off-branch would break while this flag-driven offline gate stayed
    // green (the drift-lock keeps JSON↔fixture in sync but doesn't assert node-on-muted-branch).
    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let realistic = spec
        .iter()
        .find(|p| p.list_index == 403)
        .expect("403 present");
    let pj: serde_json::Value = serde_json::from_str(&realistic.preset_json).expect("403 json");
    let g3 = pj
        .pointer("/audioGraph/guitarNodes/G3")
        .and_then(|v| v.as_array());
    assert!(
        g3.is_some_and(|arr| arr
            .iter()
            .any(|n| n.get("FenderId").and_then(|v| v.as_str()) == Some("ACD_TubeScreamer"))),
        "403's off-branch node ACD_TubeScreamer must sit on the split lane G3, not the trunk"
    );

    // Base at Crunch (-24) → CLAMP at the ceiling (~-28), headroom (reason-less).
    let opts = crate::leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let base = crate::leveller::level_preset(403, &stim, -24.0, opts, &[], None, || false)
        .expect("level 403 base");
    assert!(
        base.clamped,
        "403 Base must clamp at a shipped default target"
    );
    assert!(
        base.clamp_reason.is_none(),
        "403 Base is a headroom clamp (reason-less), not a routing clamp: {base:?}"
    );
    assert!(
        (base.predicted_lufs - (-28.0)).abs() < 0.5,
        "403 Base clamps at its ~-28 ceiling: {base:?}"
    );

    // FS1 (BRANCH B) toggles ACD_TubeScreamer on the muted parallel branch (G3): engaging it
    // (bypass=false) routes to a dead branch → off-branch silence → the routing clamp.
    let fs = crate::leveller::level_footswitch(
        403,
        0,
        ("G3", "ACD_TubeScreamer", "level"),
        &[("G3".into(), "ACD_TubeScreamer".into(), false)],
        &crate::leveller::FsWrite::Bake { clear_stale: None },
        &stim,
        -24.0,
        false,
        true,
    )
    .expect("level 403 fs");
    assert!(fs.clamped, "the off-branch footswitch clamps");
    assert_eq!(
        fs.clamp_reason.as_deref(),
        Some("no signal on USB 1/2"),
        "off-branch → the routing clamp reason (drives the UI offbranch verdict): {fs:?}"
    );
}

/// The SCENE-leveling physics for slot 403 through the REAL `level_scenes_apply_batched`
/// command over mock IPC — the same path the offline UI drives, minus the per-scene Channel
/// stream the HTTP bridge no-ops (so the UI can't render these outcomes offline; this gate
/// asserts them on the command's RETURN value instead). At the shipped default target (-26)
/// the 4 scenes produce the level-defaults outcome set: 3 SOLVABLE (amp `outputLevel`
/// converged to ~-26) + 1 OFF-BRANCH ("Clean", saved with the amp output at zero → no
/// authority over the USB capture → the routing clamp). Proves the graph-echo fix (the prepass
/// classifies gtrParallel1 and picks the trunk amp) AND the sidecar scene C authoring.
#[test]
fn level_defaults_403_scenes_solve_and_offbranch() {
    let _serial = serial();
    set_e2e_env(&[
        (
            "TMP_E2E_SCENARIO_PRESETS",
            "/../e2e/fixtures/scenario-presets.json",
        ),
        (
            "TMP_E2E_LOUDNESS_SIDECAR",
            "/../e2e/fixtures/scenario-loudness.json",
        ),
        (
            "TMP_E2E_BACKUP_FIXTURE",
            "/../e2e/fixtures/backup-fixture.bin",
        ),
        (
            "TMP_E2E_STIMULUS",
            "/resources/samples/guitar-humbucker.wav",
        ),
    ]);
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_scenes_apply_batched])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    // The trunk amp candidate (the backup scan / list_level_blocks resolves the same one).
    let amp = serde_json::json!([{
        "groupId": "G1", "nodeId": "ACD_TwinReverb65NoFx",
        "parameterId": "outputLevel", "value": 0.5
    }]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": 403,
            "jobs": (0..4).map(|s| serde_json::json!({"sceneSlot": s, "targetLufs": -26.0})).collect::<Vec<_>>(),
            "candidates": amp,
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    let rows = res.as_array().expect("results array");
    assert_eq!(rows.len(), 4, "one result per scene: {rows:?}");
    let offbranch: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|r| r["clamp_reason"].is_string())
        .collect();
    let solvable: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|r| r["clamp_reason"].is_null() && r["clamped"] == serde_json::Value::Bool(false))
        .collect();
    assert_eq!(
        offbranch.len(),
        1,
        "the amp-at-zero 'Clean' scene is off-branch: {rows:?}"
    );
    assert_eq!(solvable.len(), 3, "the other 3 scenes solve: {rows:?}");
    assert!(
        offbranch[0]["clamp_reason"]
            .as_str()
            .is_some_and(|s| s.contains("route it to USB 1/2")),
        "off-branch carries the routing clamp reason: {rows:?}"
    );
    for r in solvable {
        let lufs = r["measured_lufs"].as_f64().expect("lufs");
        assert!(
            (lufs + 26.0).abs() < 1.0,
            "solvable scene lands near -26: {r:?}"
        );
    }
}

/// Instrument-profile stimulus resolution + re-level smoke: a profile-driven level run resolves
/// its stimulus (the `profile_id` with no stored DI capture must fall back to the topology
/// stimulus, not crash) and a repeated run re-levels without a stale-candidate crash or a panic.
/// SCOPE HONESTY: this is the drivable SUBSET of journey #22 (calibrate→re-level), NOT the
/// staleness reproduction itself — the real #22 is a device-write-by-feature-A → feature-B's
/// stale FRONTEND scan cache, which is a UI-cache class no backend command-level test can
/// reproduce; and the Tier-2 DI CAPTURE (`calibrate_profile` → `capture_input`, the dry tap) is
/// not modeled offline. So this gate protects the profile-stimulus-resolution + re-level
/// stability, and the label makes no journey-#22 coverage claim it can't back.
#[test]
fn cross_feature_profile_relevel_resolves_and_no_crash() {
    let _serial = serial();
    set_e2e_env(&[
        (
            "TMP_E2E_SCENARIO_PRESETS",
            "/../e2e/fixtures/scenario-presets.json",
        ),
        (
            "TMP_E2E_LOUDNESS_SIDECAR",
            "/../e2e/fixtures/scenario-loudness.json",
        ),
        (
            "TMP_E2E_STIMULUS",
            "/resources/samples/guitar-humbucker.wav",
        ),
    ]);
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_preset])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    // A profile-driven job (an instrument + its topology). profile_id with no stored DI capture
    // falls back to the topology stimulus — the resolution must not crash on the missing capture.
    let job = serde_json::json!({
        "slot": 401, "target_lufs": -26.0, "save": false,
        "topology_id": "guitar-humbucker", "calibration_lufs": null, "stimulus_path": null,
        "profile_id": "tele-1",
        "block_group_id": null, "block_node_id": null, "block_parameter_id": null, "block_value": null
    });
    let run = |label: &str| {
        let r = invoke(&webview, "level_preset", serde_json::json!({ "job": job }))
            .unwrap_or_else(|e| panic!("{label} failed: {e}"));
        assert!(
            r["measured_lufs"].as_f64().is_some_and(f64::is_finite),
            "{label}: the profile-driven run resolved its stimulus and measured a finite loudness: {r}"
        );
    };
    run("run 1 (profile-driven)");
    run("run 2 (re-level — no stale-candidate crash)");
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
