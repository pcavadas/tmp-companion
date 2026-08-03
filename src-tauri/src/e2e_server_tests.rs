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

/// How many presets `e2e/fixtures/scenario-presets.json` ships (= the offline snapshot list
/// + the backup-fixture row count). One constant so adding a scenario preset is one edit.
const SCENARIO_PRESETS: usize = 6;

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
    // the 6 scenario presets at slots 400-405 (matching the backup fixture).
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
        crate::session::PresetEntry {
            slot: 404,
            name: "E2E Hiwatt 3S".into(),
        },
        crate::session::PresetEntry {
            slot: 405,
            name: "E2E Preset24".into(),
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

    // 2. list presets → the snapshot's fixture entries (one per scenario preset).
    let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
    assert_eq!(
        list.as_array().map(|a| a.len()),
        Some(SCENARIO_PRESETS),
        "presets: {list}"
    );

    // 3. read the library via the fixture backup → the same rows, decoded graphs.
    let lib = invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
    let rows = lib
        .get("presets")
        .and_then(|p| p.as_array())
        .expect("library presets array");
    assert_eq!(rows.len(), SCENARIO_PRESETS, "library rows: {lib}");
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
/// wire-in: slot 0 is unlisted in the sidecar → the flat default `C = -15` (PR2
/// re-baseline: +3 from the mono-era -18), so the solved
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
    // The physics wire-in ran: an unlisted slot solves to the sidecar's flat default C
    // (PR2 re-baseline: +3 from the mono-era -18).
    assert!(
        (r.constant_c - (-15.0)).abs() < 0.5,
        "the physics model produced the default C=-15 (not a passthrough loudness): {r:?}"
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

/// `note_structural_save` flips `SCENARIO_VERIFIED` false for a `STRUCTURAL_SAVE_CMDS`
/// member and leaves it untouched for anything else — pins the set's intent in BOTH
/// directions. Root cause (2026-08-01, `notes/user-journeys.md` bug→gate registry):
/// online `copy.spec.ts` saved a structural edit (dropping a block) over the resident
/// `E2E Target 2` fixture with nothing clearing `SCENARIO_VERIFIED`, so the next spec's
/// `ensureScenario` hit the fast path, skipped the device re-verify, and asserted on the
/// mutilated fixture. Value-only leveling saves are deliberately excluded from the set
/// (within-run value drift is handled by spec ORDERING — doctor before level-strict —
/// not by paying a device re-verify per spec inside the HID open-lockout window).
#[test]
fn note_structural_save_flags_structural_saves_only() {
    use std::sync::atomic::Ordering::SeqCst;
    let _serial = serial();

    // Process-global flag: restore the default (false) on exit — INCLUDING a panicking
    // assert — so the leaked value can't steer a later serial test's fast path.
    struct FlagReset;
    impl Drop for FlagReset {
        fn drop(&mut self) {
            super::SCENARIO_VERIFIED.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _reset = FlagReset;

    super::SCENARIO_VERIFIED.store(true, SeqCst);
    super::note_structural_save("copy_apply");
    assert!(
        !super::SCENARIO_VERIFIED.load(SeqCst),
        "a structural save (copy_apply) must invalidate the verified flag"
    );

    super::SCENARIO_VERIFIED.store(true, SeqCst);
    super::note_structural_save("level_preset");
    assert!(
        super::SCENARIO_VERIFIED.load(SeqCst),
        "a value-only leveling save must NOT invalidate the verified flag"
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

    // Base at Crunch (-21, PR2 re-baseline: +3 from the mono-era -24) → CLAMP at
    // the ceiling (~-25, +3 from ~-28), headroom (reason-less).
    let opts = crate::leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let base = crate::leveller::level_preset(403, &stim, -21.0, opts, &[], None, || false)
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
        (base.predicted_lufs - (-25.0)).abs() < 0.5,
        "403 Base clamps at its ~-25 ceiling: {base:?}"
    );

    // FS1 (BRANCH B) toggles ACD_TubeScreamer on the muted parallel branch (G3): engaging it
    // (bypass=false) routes to a dead branch → off-branch silence → the routing clamp.
    let fs = crate::leveller::level_footswitch(
        403,
        0,
        ("G3", "ACD_TubeScreamer", "level"),
        &[("G3".into(), "ACD_TubeScreamer".into(), false)],
        &crate::leveller::FsWrite::Bake {
            clear_stale: None,
            mirror_scenes: vec![],
        },
        &stim,
        -21.0,
        false,
        true,
        None,
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
/// asserts them on the command's RETURN value instead). At the shipped default target (-23,
/// PR2 re-baseline: +3 from the mono-era -26) the 4 scenes produce the level-defaults outcome
/// set: 3 SOLVABLE (amp `outputLevel` converged to ~-23) + 1 OFF-BRANCH ("Clean", saved with
/// the amp output at zero → no
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
            "jobs": (0..4).map(|s| serde_json::json!({"sceneSlot": s, "targetLufs": -23.0})).collect::<Vec<_>>(),
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
            (lufs + 23.0).abs() < 1.0,
            "solvable scene lands near -23: {r:?}"
        );
    }
}

/// Gain-budget redistribution (PR5) end-to-end through the real command against the offline
/// physics: slot 400 (E2E Reference) is the loud class — Base C=-15 solves, scene 0 C=-27
/// clamps (PR2 re-baseline: +3 from the mono-era -18/-30). `redistribute_headroom` raises
/// presetLevel by the solved delta and re-levels the base amp + BOTH scenes back to −23, so the
/// previously-clamped scene 0 reaches target (done, not clamped) and every sound lands near −23
/// — AND it records the pre-values (presetLevel + touched knobs) for the Summary's Restore.
/// This is the offline half of "clamped run →
/// redistribute → all done"; the base-scene skip + save-persistence idempotency are online
/// (the sim models no field-8 read-back / saved-state reload, same limit as `level-rerun`).
#[test]
fn redistribute_400_gives_the_clamped_scene_headroom() {
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
        .invoke_handler(tauri::generate_handler![redistribute_headroom])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let amp = serde_json::json!([{
        "groupId": "G1", "nodeId": "ACD_DeluxeReverb65BlondeVibratoNoFxCabIR",
        "parameterId": "outputLevel", "value": 0.5
    }]);
    // Base (wire slot 8) + scene 0 (the clamped one) + scene 1, all to −23 (PR2 re-baseline: +3
    // from the mono-era −26). `worstClampedDeficitDb` = scene 0's deficit at presetLevel 0.32
    // (≈5.9); 6.0 is enough to fully rescue it.
    let jobs = serde_json::json!([
        {"sceneSlot": 8, "targetLufs": -23.0},
        {"sceneSlot": 0, "targetLufs": -23.0},
        {"sceneSlot": 1, "targetLufs": -23.0}
    ]);
    let res = invoke(
        &webview,
        "redistribute_headroom",
        serde_json::json!({
            "slot": 400, "jobs": jobs, "candidates": amp, "worstClampedDeficitDb": 6.0,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("redistribute_headroom");
    let results = res["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3, "one result per sound: {res}");
    for r in results {
        let lufs = r["measured_lufs"].as_f64().expect("lufs");
        assert!(
            (lufs + 23.0).abs() < 1.0,
            "every sound (incl. the once-clamped scene 0) reaches −23: {r}"
        );
        assert_eq!(
            r["clamped"],
            serde_json::json!(false),
            "no clamp remains: {r}"
        );
    }
    // presetLevel was raised by a positive delta, and the pre-values are recorded for Restore.
    let delta = res["deltaDb"].as_f64().expect("deltaDb");
    assert!(delta > 0.0, "a positive redistribution delta: {res}");
    assert!(
        res["newPresetLevel"].as_f64().unwrap() > res["previousPresetLevel"].as_f64().unwrap(),
        "presetLevel rose: {res}"
    );
    let prev = res["previousKnobs"].as_array().expect("previousKnobs");
    assert!(
        prev.iter().any(|k| k["sceneSlot"].is_null()),
        "the base amp knob (sceneSlot null) is recorded for Restore: {res}"
    );
    assert!(
        prev.iter().filter(|k| k["sceneSlot"].is_number()).count() >= 2,
        "both scene overlays are recorded for Restore: {res}"
    );
    // The other half of the atomicity contract: exactly ONE deferred save persisted it all.
    assert_eq!(
        sim.events()
            .iter()
            .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(_)))
            .count(),
        1,
        "exactly one deferred save persists the redistribution: {:?}",
        sim.events()
    );
}

/// Redistribution ATOMICITY: a capture fault (one sound reads silence) makes that
/// compensating solve fail, so the whole redistribution is a PARTIAL — it aborts PRE-save,
/// reloads to discard the raise, and persists NOTHING (no `saveCurrentPreset`). The command
/// returns an error; the fake records zero `Saved` events.
#[test]
fn redistribute_aborts_and_saves_nothing_on_a_dropped_capture() {
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
    // Fault the NEXT capture for slot 400 → the first compensating solve fails.
    crate::sim_device::arm_capture_fault(400);
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![redistribute_headroom])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let amp = serde_json::json!([{
        "groupId": "G1", "nodeId": "ACD_DeluxeReverb65BlondeVibratoNoFxCabIR",
        "parameterId": "outputLevel", "value": 0.5
    }]);
    let jobs = serde_json::json!([
        {"sceneSlot": 8, "targetLufs": -23.0},
        {"sceneSlot": 0, "targetLufs": -23.0}
    ]);
    let res = invoke(
        &webview,
        "redistribute_headroom",
        serde_json::json!({
            "slot": 400, "jobs": jobs, "candidates": amp, "worstClampedDeficitDb": 6.0,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    );
    assert!(
        res.is_err(),
        "a faulted redistribution must return an error: {res:?}"
    );
    assert!(
        !sim.events()
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
        "an aborted redistribution saves NOTHING: {:?}",
        sim.events()
    );
    // And the working copy is discarded: the abort reloads the stored preset AFTER the last
    // live write, so nothing unsaved can linger on the unit.
    let events = sim.events();
    let last_write = events
        .iter()
        .rposition(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_)));
    let last_reload = events
        .iter()
        .rposition(|e| matches!(e, crate::sim_device::SimEvent::Loaded(400)));
    assert!(
        matches!((last_write, last_reload), (Some(w), Some(l)) if l > w),
        "the abort's discard-reload follows the last live write: {events:?}"
    );
}

/// Reachable-common-target derivation (PR6) through the real command: given a finished run's
/// ALREADY-measured ceilings, `common_reachable_target` returns `min(C − offset) − headroom`
/// (guitar offset 0 → `min(C) − 1`). This is the quiet-preset clamp fallback's target, and it
/// re-captures NOTHING — the frontend re-levels every sound to this value via the run loop. The
/// offset-space + min math is unit-gated in `leveller`; this pins the command wiring.
#[test]
fn common_reachable_target_returns_min_ceiling_minus_headroom() {
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![common_reachable_target])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let res = invoke(
        &webview,
        "common_reachable_target",
        serde_json::json!({
            "ceilings": [
                { "cLufs": -25.0, "topologyId": null },
                { "cLufs": -20.0, "topologyId": "guitar-humbucker" }
            ]
        }),
    )
    .expect("common_reachable_target");
    let t = res.as_f64().expect("target f64");
    assert!(
        (t - -26.0).abs() < 1e-9,
        "min(-25,-20) − 1 headroom = -26 (PR2 re-baseline: +3 from the mono-era -28/-23 → -29): {t}"
    );

    // No finite ceiling → an error (an all-silent run has nothing to derive from).
    let err = invoke(
        &webview,
        "common_reachable_target",
        serde_json::json!({ "ceilings": [] }),
    );
    assert!(err.is_err(), "empty ceilings must error: {err:?}");
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
        "slot": 401, "target_lufs": -23.0, "save": false,
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

/// FIELD-8 GATE: the SimDevice answers `presetDataRequest`(8) for a scenario slot, so
/// `read_saved_preset` — THE saved document behind `set_knobs`' overlay classification and the
/// footswitch bake gate — resolves offline. Without it every scene/footswitch write is refused
/// ("no saved-preset read") and the whole offline scene tier goes dark (it did: 951d141 landed
/// with two offline gates red for exactly this reason).
///
/// Also pins the reassembly, which is the part that can silently regress: a ~20 KB presetJson
/// is ~340 HID frames, so a rule change in `streams_final`/`try_preset_data_json` would return
/// a TRUNCATED document — which parses, and then answers "overlay unknown" instead of failing
/// loudly. Hence the `scenes` + per-scene-overlay assertions, not just `is_some`.
#[test]
fn sim_answers_the_field8_saved_preset_read() {
    let _serial = serial();
    set_e2e_env(&[(
        "TMP_E2E_SCENARIO_PRESETS",
        "/../e2e/fixtures/scenario-presets.json",
    )]);
    let sim = crate::sim_device::SimDevice::new();
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));

    let saved = crate::read_saved_preset(403).expect("field-8 read answers for a scenario slot");
    let scenes = saved["scenes"].as_array().expect("scenes array survived");
    assert_eq!(scenes.len(), 4, "403 has 4 scenes: {scenes:?}");
    assert!(
        matches!(
            crate::scene_overlay(&saved, 1, "ACD_TwinReverb65NoFx"),
            crate::SceneOverlay::Present(_)
        ),
        "the per-node overlay accessor resolves against the read document"
    );
}

/// The scenario slot + node the corruption-class gates below drive: the reported preset
/// (a real 1.8.45 unit's 3-scene + "Base Scene" Hiwatt, saved `lastLoadedScene = 3`, 4
/// block-acting footswitches) and its trunk amp. See `notes/user-journeys.md`'s bug→gate rows.
const HIWATT: u32 = 404;
const HIWATT_AMP: &str = "ACD_HiwattDR103CanMod";

/// The scenario env every 404 gate needs (fixture presets + the authored C table + backup +
/// stimulus), plus a live SimDevice wired as the transport factory. Returns the fake so the
/// caller can read its event log.
fn hiwatt_sim() -> crate::sim_device::SimDevice {
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
    sim
}

/// BUG→GATE (2026-07-27 report, the SCENE-TONE WIPE — the corruption bug): leveling the
/// reported preset's scenes must NOT send `SetNodeSceneEdit(enable)` for a node that ALREADY
/// has an overlay in that scene. On hardware that enable RESEEDS the whole overlay from base,
/// so the scene's authored tone (this preset's per-scene Hiwatt bass/treble/middle/presence/
/// volumes) reverted to base while the leveled `outputLevel` survived — the user's "it changed
/// my sound" report.
///
/// WHY THE EVENT LOG, not a persisted read-back: the fake has no persisted preset store at all
/// (`param_writes` is edit-buffer state, cleared on every `loadPreset`), so "the stored overlay
/// still holds its tone params" is not expressible offline. The enable being SENT is the
/// device-visible cause, and `sim_device::scene_context_tests::
/// enabling_scene_edit_reseeds_the_node_from_base` pins that the enable does wipe. Together
/// they cover the class; the persisted survival itself is an ONLINE assertion.
///
/// The premise is asserted first (the amp really does have an overlay in every job scene),
/// so the gate cannot pass vacuously if a fixture edit flattens the overlays.
#[test]
fn hiwatt_scene_leveling_never_reseeds_an_existing_overlay() {
    let _serial = serial();
    let sim = hiwatt_sim();

    // Premise: scenes 0/1/2 each carry an overlay for the amp — the `Present` branch, the one
    // where the enable is pure corruption (an `Absent` overlay legitimately needs it).
    let saved = crate::read_saved_preset(HIWATT).expect("field-8 read");
    for scene in 0..3u32 {
        assert!(
            matches!(
                crate::scene_overlay(&saved, scene, HIWATT_AMP),
                crate::SceneOverlay::Present(_)
            ),
            "fixture premise: scene {scene} must already carry an overlay for {HIWATT_AMP}"
        );
    }

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_scenes_apply_batched])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let amp = serde_json::json!([{
        "groupId": "G1", "nodeId": HIWATT_AMP, "parameterId": "outputLevel", "value": 0.69
    }]);
    // Scenes 0/1/2 only: scene 3 ("Base Scene") is the measurement-context probe — its authored
    // C is 8 dB below the target on purpose (see the sidecar comment), so it would clamp here.
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": HIWATT,
            "jobs": (0..3).map(|s| serde_json::json!({"sceneSlot": s, "targetLufs": -23.0})).collect::<Vec<_>>(),
            "candidates": amp,
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    let rows = res.as_array().expect("results array");
    assert_eq!(rows.len(), 3, "one result per scene: {rows:?}");

    let events = sim.events();
    let reseeds: Vec<&crate::sim_device::SimEvent> = events
        .iter()
        .filter(|e| {
            matches!(e, crate::sim_device::SimEvent::SceneEdit { node, enable: true, .. }
                if node == HIWATT_AMP)
        })
        .collect();
    assert!(
        reseeds.is_empty(),
        "leveling a scene whose overlay already exists must NOT enable Scene Edit (it reseeds \
         the overlay from base — the reported tone corruption): {reseeds:?}"
    );

    // The other half of the same bug: with the enable dropped, the write must still land in the
    // SCENE's overlay, never leak to base (which would move every scene at once).
    for scene in 0..3i64 {
        assert!(
            events.iter().any(|e| matches!(e,
                crate::sim_device::SimEvent::ChangeParameter { scene: s, node, param, .. }
                    if *s == scene && node == HIWATT_AMP && param == "outputLevel")),
            "scene {scene}'s solved outputLevel must be written under that scene, not base: \
             {events:?}"
        );
    }
    assert!(
        !events.iter().any(|e| matches!(e,
            crate::sim_device::SimEvent::ChangeParameter { scene, node, param, .. }
                if *scene == crate::sim_device::SCENE_BASE && node == HIWATT_AMP
                    && param == "outputLevel")),
        "no scene-leveling write may land at base: {events:?}"
    );
}

/// BUG→GATE (2026-07-27 report, the MEASUREMENT CONTEXT): a preset loads into its SAVED
/// `lastLoadedScene`, so a base measurement that does not recall base first measures THAT
/// scene. The reported preset saves `lastLoadedScene = 3`, and the authored C table puts scene
/// 3 eight dB below base (-31 vs -17, PR2 re-baseline: +3 from the mono-era -34/-20) — so the
/// outcome itself discriminates: recalled to base the run SOLVES -23 (presetLevel ~0.5);
/// measuring scene 3 instead the target is above that ceiling and the run reports CLAMPED. No
/// event-order heuristic needed.
#[test]
fn hiwatt_base_leveling_measures_base_not_the_saved_scene() {
    let _serial = serial();
    let sim = hiwatt_sim();
    // Premise: the fake really does activate the fixture's saved scene on a load (the whole
    // reason the recall exists) — else this gate would pass with no recall at all.
    {
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(HIWATT).expect("load");
        assert!(
            sim.events()
                .iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Loaded(s) if *s == HIWATT)),
            "the fake saw the load"
        );
    }
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_preset])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let job = serde_json::json!({
        "slot": HIWATT, "target_lufs": -23.0, "save": false,
        "topology_id": null, "calibration_lufs": null, "stimulus_path": null, "profile_id": null,
        "block_group_id": null, "block_node_id": null, "block_parameter_id": null,
        "block_value": null
    });
    let r = invoke(&webview, "level_preset", serde_json::json!({ "job": job }))
        .expect("level_preset 404");
    assert_eq!(
        r["clamped"],
        serde_json::json!(false),
        "base solved from base's C=-17; a CLAMP means the capture measured the saved scene 3 \
         (C=-31) instead of base: {r}"
    );
    let measured = r["measured_lufs"].as_f64().expect("measured_lufs");
    assert!(
        (measured + 23.0).abs() < 1.0,
        "the base run lands on target: {r}"
    );
    // Belt-and-braces on the mechanism, not just the outcome: base (wire slot 8) was recalled.
    assert!(
        sim.events()
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(s)
                if *s == crate::session::BASE_SCENE_SLOT)),
        "every base measurement recalls base explicitly: {:?}",
        sim.events()
    );
}

/// BUG→GATE (2026-07-27 report, the FOOTSWITCH "MULTI" class): pin the bake-vs-assign PLAN the
/// reported preset produces, so a change to the discriminator has to face this fixture.
///
/// All four of its block-acting switches must take the BAKE path (no `ftsw` touch, no added
/// function, no MULTI — the user-reported expectation, issues 3/4), even though each scene
/// overlays the very param the leveler bakes with a value that differs from base (MythicDrive
/// `output` 0.55 → 0.78 in scene 3; TremoloBias `level` 0.5 → 0.0; UniVibe `volume` 0.49 →
/// 0.54; Lightspeed `loudness` 0.47 → 0.26 in scene 2 — scene 3 being this preset's own
/// `lastLoadedScene`). A diverging overlay can never make the bake unsafe: it MASKS base (HW,
/// Hiwatt slot 31), so the plan bakes, MIRRORS the solved value only into the scenes that
/// restated base, and leaves each authored per-scene mix untouched. This gate goes red if
/// anything sends these switches down Assign (the MULTI regression) or mirrors a diverging
/// scene.
///
/// The shipped discriminator is `scene_jobs::scene_overlays_change_param`, asked once for
/// `bypass` and once for the LEVELED param, by VALUE. Key presence was not enough: this
/// fixture's device-authored overlays carry the full param set for every node in every scene, so
/// a "does the `bypass` KEY appear" gate is true of every switch of every preset the unit itself
/// wrote — it collapsed to the whole-preset gate and the added-function / "MULTI" symptom
/// survived. Pure planner test (no device): `plan_footswitch_jobs` is the whole decision.
#[test]
fn hiwatt_footswitch_plan_bakes_and_mirrors_only_the_scenes_restating_base() {
    let _serial = serial(); // pure, but it writes the shared scenario-path env
    set_e2e_env(&[(
        "TMP_E2E_SCENARIO_PRESETS",
        "/../e2e/fixtures/scenario-presets.json",
    )]);
    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let preset: serde_json::Value = serde_json::from_str(
        &spec
            .iter()
            .find(|p| p.list_index == HIWATT)
            .expect("404 present")
            .preset_json,
    )
    .expect("404 json");
    let ftsw = preset["ftsw"].clone();

    // The switches + their DEFAULT leveled param, exactly as the UI derives them (the backup
    // scan's `level_params`, then `leveling.ts::defaultParamIndex` = first loudness-only param).
    let jobs: Vec<(u32, &str, &str)> = vec![
        (2, "ACD_MythicDrive", "output"),
        (3, "ACD_Lightspeed", "loudness"),
        (11, "ACD_TremoloBias", "level"),
        (12, "ACD_UniVibe", "volume"),
    ];
    let keys: Vec<crate::footswitch::FsJobKey> = jobs
        .iter()
        .map(|(switch, node, param)| crate::footswitch::FsJobKey {
            switch: *switch,
            lev_node: node,
            lev_param: param,
            target_bits: (-26.0f64).to_bits(),
        })
        .collect();
    let plans = crate::footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys);
    assert_eq!(plans.len(), jobs.len());
    for ((switch, node, param), plan) in jobs.iter().zip(&plans) {
        // Per-scene ground truth from the fixture: which overlays restate base's value and
        // which authored their own (this preset varies each pedal's level param in exactly
        // one scene — e.g. its trem is MUTED in one scene with `level: 0.0`).
        let base = crate::commands::level_footswitch::node_param_f64(&preset, node, param)
            .unwrap_or_else(|| panic!("{node}.{param} exists at base"));
        let scenes = preset["scenes"].as_array().expect("scenes");
        let overlay_value = |scene: u32| match crate::scene_overlay(&preset, scene, node) {
            crate::SceneOverlay::Present(p) => p.get(*param).and_then(serde_json::Value::as_f64),
            _ => None,
        };
        let restating: Vec<u32> = (0..scenes.len() as u32)
            .filter(|&sc| overlay_value(sc).is_some_and(|v| (v - base).abs() <= 1e-6))
            .collect();
        let diverging: Vec<u32> = (0..scenes.len() as u32)
            .filter(|&sc| overlay_value(sc).is_some_and(|v| (v - base).abs() > 1e-6))
            .collect();
        assert!(
            !diverging.is_empty(),
            "{node}.{param}: fixture precondition — at least one scene authored its own value"
        );
        // The user-reported expectation (issues 3/4): leveling this preset's switches must
        // NOT touch `ftsw` at all — no Assign, no added function, no MULTI. A scene that
        // overlays the leveled param can never make a bake unsafe (the overlay MASKS base,
        // HW slot 31), so the plan BAKES, mirroring the solved value only into the scenes
        // that restated base and leaving each authored per-scene mix untouched.
        match plan {
            crate::footswitch::FsLevelPlan::Bake { mirror_scenes, .. } => {
                assert_eq!(
                    mirror_scenes, &restating,
                    "switch {switch} ({node}.{param}): mirror exactly the restating scenes"
                );
                for sc in &diverging {
                    assert!(
                        !mirror_scenes.contains(sc),
                        "switch {switch} ({node}.{param}): scene {sc} authored its own value \
                         and must never be mirrored"
                    );
                }
            }
            other => panic!(
                "switch {switch} ({node}.{param}): expected Bake (ftsw untouched), got {other:?}"
            ),
        }
    }
}

// ─── ensure_fresh_load barrier, end-to-end against the sim lazy-commit model ─────────────
//
// These are the tests `leveller::fresh_load_registry_tests` points at: the barrier owns its
// own `Session::connect()`, so the only way to put a sim behind it is the process-global
// transport factory — which this module's serial harness already owns.

/// Clear the save registry when the test ends — INCLUDING on a panicking assert. A leaked
/// witness would send any later serial test that levels the same slot into the barrier's
/// full commit-window wait against a doc that can never match.
struct RegistryReset;
impl Drop for RegistryReset {
    fn drop(&mut self) {
        crate::leveller::clear_slot_save_registry();
    }
}

/// Route every `Session::connect()` at one shared sim with the given commit latency, and
/// start from a clean save registry.
fn install_barrier_sim(latency_ms: u64) -> crate::sim_device::SimDevice {
    let sim = crate::sim_device::SimDevice::new().with_commit_latency(latency_ms);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    crate::leveller::clear_slot_save_registry();
    sim
}

/// Save `level` to slot 401 through the real session wire path (the sim's F_SAVE handler
/// records it as the slot's pending lazy-commit doc).
fn save_level_401(sim: &crate::sim_device::SimDevice, level: f32) {
    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    s.load_preset(401).unwrap();
    s.set_preset_level(level).unwrap();
    s.save_current_preset(401).unwrap();
}

#[test]
fn fresh_load_barrier_passes_on_a_committed_witness_match() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(0); // 0 ms: the save commits immediately
    save_level_401(&sim, 0.81);
    crate::leveller::register_slot_save(401, crate::leveller::SaveWitness::PresetLevel(0.81));
    let start = std::time::Instant::now();
    let result = crate::leveller::ensure_fresh_load(401, &mut || false);
    assert!(result.is_ok(), "{result:?}");
    // First-harvest pass: the sim's pumps are instant, so anything near the production
    // retry cadence (10 s) means the loop waited against an already-fresh doc.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "a matching committed witness must pass on the FIRST harvest, took {:?}",
        start.elapsed()
    );
}

#[test]
fn fresh_load_barrier_waits_out_a_pending_commit_then_passes() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(1_500);
    save_level_401(&sim, 0.81);
    crate::leveller::register_slot_save(401, crate::leveller::SaveWitness::PresetLevel(0.81));
    let start = std::time::Instant::now();
    // The sim's pumps return instantly, so pace the loop from the cancel hook (50 ms per
    // probe) — otherwise the retry wait is a busy spin and the log drowns in warns.
    let result = crate::leveller::ensure_fresh_load_paced(
        401,
        &mut || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            false
        },
        200,
    );
    assert!(result.is_ok(), "{result:?}");
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(1_200),
        "the barrier must have genuinely waited for the sim's 1.5 s commit, took {:?}",
        start.elapsed()
    );
    // The barrier's final load materialized the COMMITTED doc — the caller's own load now
    // sees the saved value, which is the entire point of the wait.
    assert!(
        (sim.preset_level() - 0.81).abs() < 1e-3,
        "post-barrier the sim must hold the committed level, got {}",
        sim.preset_level()
    );
}

#[test]
fn fresh_load_barrier_cancel_mid_wait_returns_cancelled() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(600_000); // never commits during this test
    save_level_401(&sim, 0.9);
    crate::leveller::register_slot_save(401, crate::leveller::SaveWitness::PresetLevel(0.9));
    let mut calls = 0u32;
    let result = crate::leveller::ensure_fresh_load_paced(
        401,
        &mut || {
            calls += 1;
            calls > 1 // first probe (loop top) proceeds; the wait-loop probe cancels
        },
        600_000,
    );
    assert_eq!(
        result,
        Err(crate::leveller::CANCELLED.to_string()),
        "a Stop during the stale wait must surface as the CANCELLED sentinel"
    );
}

#[test]
fn fresh_load_barrier_time_gate_proceeds_on_an_unharvestable_witness() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(600_000); // the pending save never commits
    save_level_401(&sim, 0.9);
    // Backdate the registration to the commit-window edge: the barrier engages (elapsed is
    // not yet PAST the window) but the witness can never match — the time-gate must let the
    // run proceed within ~a second rather than hard-erroring or hanging.
    crate::leveller::register_slot_save_at(
        401,
        crate::leveller::SaveWitness::PresetLevel(0.9),
        std::time::Instant::now()
            - std::time::Duration::from_secs(crate::leveller::COMMIT_WINDOW_SECS),
    );
    let result = crate::leveller::ensure_fresh_load_paced(
        401,
        &mut || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            false
        },
        200,
    );
    assert!(
        result.is_ok(),
        "time-gate must proceed, not error: {result:?}"
    );
    // Proof it exited via the TIME-GATE, not a witness match: the sim still materializes
    // the pre-save committed level (the fixture's own 0.32).
    assert!(
        (sim.preset_level() - 0.32).abs() < 1e-3,
        "the pending save must still be uncommitted, got {}",
        sim.preset_level()
    );
}
