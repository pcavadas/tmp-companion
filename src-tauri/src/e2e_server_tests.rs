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
const SCENARIO_PRESETS: usize = 10;

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
    // the 10 scenario presets at slots 400-409 (matching the backup fixture).
    let presets = vec![
        crate::session::PresetEntry {
            slot: 400,
            name: "E2E Rig".into(),
        },
        crate::session::PresetEntry {
            slot: 401,
            name: "E2E Pedalboard".into(),
        },
        crate::session::PresetEntry {
            slot: 402,
            name: "E2E Edge".into(),
        },
        crate::session::PresetEntry {
            slot: 403,
            name: "E2E Parallel".into(),
        },
        crate::session::PresetEntry {
            slot: 404,
            name: "E2E Hiwatt 3S".into(),
        },
        crate::session::PresetEntry {
            slot: 405,
            name: "E2E Preset24".into(),
        },
        crate::session::PresetEntry {
            slot: 406,
            name: "E2E Combined Level".into(),
        },
        crate::session::PresetEntry {
            slot: 407,
            name: "E2E Doctor Oracle".into(),
        },
        crate::session::PresetEntry {
            slot: 408,
            name: "E2E Preset24 Min".into(),
        },
        crate::session::PresetEntry {
            slot: 409,
            name: "E2E Hiwatt Min".into(),
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
        "name": "E2E Pedalboard",
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
/// `E2E Edge` fixture with nothing clearing `SCENARIO_VERIFIED`, so the next spec's
/// `ensureScenario` hit the fast path, skipped the device re-verify, and asserted on the
/// mutilated fixture. Value-only leveling saves are deliberately excluded from the set
/// (within-run value drift is handled by spec ORDERING — doctor.online before level.online —
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

/// The physics that drives `level-defaults.spec.ts`, split across the two fixtures that
/// now carry it: slot 403 (E2E Parallel) at a SHIPPED DEFAULT target produces the
/// first-session Base CLAMP (headroom, reason-less), and slot 402 (E2E Edge) carries the
/// OFF-BRANCH footswitch — its block sits on the `gtrSplit` OUT-2 lane, which this preset
/// routes away from USB 1/2, so an isolated engaged capture reads dead air → the
/// "no signal on USB 1/2" routing clamp. Fast backend gate for the sidecar authoring,
/// independent of the Playwright UI flow: a sidecar C perturbation or an
/// `offbranch_switch_node` regression flips these here (mutation-check #2/#4). Uses the
/// committed fixture + sidecar via their env overrides.
#[test]
fn level_defaults_base_clamps_and_the_split_lane_footswitch_is_offbranch() {
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

    // The fixture must actually put the off-branch node on the OUT-2 SPLIT LANE (G3), not
    // the trunk, else the ONLINE footswitch off-branch would break while this flag-driven
    // offline gate stayed green (the drift-lock keeps JSON↔fixture in sync but doesn't
    // assert node-on-a-lane-that-misses-USB).
    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let edge = spec
        .iter()
        .find(|p| p.list_index == 402)
        .expect("402 present");
    let pj: serde_json::Value = serde_json::from_str(&edge.preset_json).expect("402 json");
    assert_eq!(
        pj.pointer("/audioGraph/template").and_then(|v| v.as_str()),
        Some("gtrSplit"),
        "the off-branch case needs a SPLIT-OUTPUT preset"
    );
    let g3 = pj
        .pointer("/audioGraph/guitarNodes/G3")
        .and_then(|v| v.as_array());
    assert!(
        g3.is_some_and(|arr| arr
            .iter()
            .any(|n| n.get("FenderId").and_then(|v| v.as_str()) == Some("ACD_KingOfTone"))),
        "402's off-branch node ACD_KingOfTone must sit on the OUT-2 lane G3, not the trunk"
    );
    assert_eq!(
        pj.pointer("/outputMixerSettings/USB12Input/out2"),
        Some(&serde_json::Value::Bool(false)),
        "OUT 2 must be routed AWAY from USB 1/2 — that is what makes the lane dead air \
         for the capture"
    );

    // Base at Lead (-19) on 403 → CLAMP at its ceiling (-20), headroom (reason-less).
    let opts = crate::leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let base = crate::leveller::level_preset(403, &stim, -19.0, opts, &[], None, || false)
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
        (base.predicted_lufs - (-20.0)).abs() < 0.5,
        "403 Base clamps at its ~-20 ceiling: {base:?}"
    );

    // 402's OUT2 switch toggles ACD_KingOfTone on the off-USB lane: engaging it
    // (bypass=false) routes to dead air → off-branch silence → the routing clamp.
    let fs = crate::leveller::level_footswitch(
        402,
        2,
        ("G3", "ACD_KingOfTone", "volume"),
        &[("G3".into(), "ACD_KingOfTone".into(), false)],
        &crate::leveller::FsWrite::Bake {
            clear_stale: None,
            mirror_scenes: vec![],
        },
        &stim,
        -21.0,
        false,
        true,
        None,
        // `volume` on a (non-amp) pedal is a plain level_linear control over [0,1].
        &crate::leveller::FsParamTarget::new("ACD_KingOfTone", "volume", 0.5),
    )
    .expect("level 402 fs");
    assert!(fs.clamped, "the off-branch footswitch clamps");
    assert_eq!(
        fs.clamp_reason.as_deref(),
        Some("no signal on USB 1/2"),
        "off-branch → the routing clamp reason (drives the UI offbranch verdict): {fs:?}"
    );
}

/// GATE (user report, preset 30 "Plumes+BD2+OCD", 2026-08-20): THE PREPASS MUST ANNOUNCE ITSELF.
///
/// The ceiling prepass measures every selected footswitch — one engage, ~10 s — BEFORE the
/// first row is solved, and it used to stream nothing at all. The wizard's optimistic
/// `markGroupActive` therefore held row 0 highlighted for the whole sweep while the unit
/// engaged four different pedals in turn, and the run read as "the Pinions row is leveling
/// with the Sapphire OD footswitch on". Nothing was misleveled — every capture's isolation
/// was correct — but the display named the wrong sound for a minute, which is the same thing
/// to the person watching.
///
/// Two halves, both required, because either alone still lies:
/// 1. ORDERING — all four rows go `active` before ANY row finishes. Per-row events are what
///    make the highlight follow the device.
/// 2. CAPTION — each row's FIRST active carries `leveller::PREPASS_ACTIVE_MSG`, and its
///    SECOND (the solve, phase 2) carries none. A live capture streams throughout, so the
///    wizard renders an active row's message as the VERB before the live number: with the
///    caption the prepass reads `measuring · -18.9`, without it `leveling · -18.9` — the
///    same lie, better aimed. The absent second caption is what flips the verb back.
///
/// Observed through `channel_interceptor`, which sees the `Channel` items the offline HTTP
/// bridge deliberately no-ops (`.claude/rules/e2e.md`) — so this contract is NOT reachable
/// from a Playwright spec, and this is the seam that owns it. `level_footswitches_apply` ends
/// in `with_released_seize(...).await`, so `get_ipc_response` returns only after every send
/// has landed: no polling, no sleep.
///
/// Fixture 405 is the incident's own shape — four bare on-off drive pedals on switches 5-8,
/// four distinct handles, so four plain `Bake` plans with no `BakeShared` sibling (which pays
/// no capture and correctly gets no prepass tick) and no skips.
#[test]
fn the_fs_prepass_announces_every_row_before_any_row_finishes() {
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

    // Every channel item in arrival order. The interceptor consumes them (`true`) so nothing
    // reaches a webview `eval` that does not exist under the mock runtime.
    let items: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>> = Default::default();
    let sink = items.clone();
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .channel_interceptor(move |_wv, _cb, _id, body| {
            if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                    lock_ok(&sink).push(v);
                }
            }
            true
        })
        .invoke_handler(tauri::generate_handler![level_footswitches_apply])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");

    // The four drive pedals, in switch order — Rat's handle is `volume`, the others' `level`.
    let switches: [(u32, &str, &str); 4] = [
        (5, "ACD_Plumes", "level"),
        (6, "ACD_BluesDriver", "level"),
        (7, "ACD_ObsessiveDrive", "level"),
        (8, "ACD_Rat", "volume"),
    ];
    let jobs: Vec<serde_json::Value> = switches
        .iter()
        .map(|(sw, node, param)| {
            serde_json::json!({
                "switch": sw,
                "levGroupId": "G1",
                "levNodeId": node,
                "levParameterId": param,
                "targetLufs": -23.0,
            })
        })
        .collect();
    // `save: false` — this gate owns the run's REPORTING, not its writes, and a save would
    // drag in the lazy-commit barrier (whose own message is the other half of the caption
    // contract: a note, sent when nothing is streaming).
    invoke(
        &webview,
        "level_footswitches_apply",
        serde_json::json!({
            "slot": 405,
            "jobs": jobs,
            "save": false,
            "topologyId": serde_json::Value::Null,
            "calibrationLufs": null,
            "profileId": null,
            "onResult": "__CHANNEL__:7",
        }),
    )
    .expect("level_footswitches_apply");

    let seen = lock_ok(&items).clone();
    let first_done = seen
        .iter()
        .position(|v| v["status"] == "done")
        .unwrap_or(seen.len());
    // Only the PREPASS ticks — phase 2's own `active` for the first row also lands before that
    // row's `done`, and it is not what this half is about.
    let announced: Vec<u32> = seen[..first_done]
        .iter()
        .filter(|v| v["status"] == "active" && v["message"] == crate::leveller::PREPASS_ACTIVE_MSG)
        .filter_map(|v| v["switch"].as_u64().map(|n| n as u32))
        .collect();
    assert_eq!(
        announced,
        vec![5, 6, 7, 8],
        "every selected row must go active, IN DEVICE ORDER, before any row finishes — the \
         prepass engages each in turn and the wizard highlights whichever it last heard \
         about. Got {announced:?} from {seen:?}"
    );
    for (sw, _, _) in switches {
        let captions: Vec<Option<&str>> = seen
            .iter()
            .filter(|v| v["status"] == "active" && v["switch"].as_u64() == Some(u64::from(sw)))
            .map(|v| v["message"].as_str())
            .collect();
        assert_eq!(
            captions.first().copied().flatten(),
            Some(crate::leveller::PREPASS_ACTIVE_MSG),
            "switch {sw}'s prepass tick must caption its own phase, or the wizard renders it \
             with the solve's verb over a live number: {seen:?}"
        );
        assert_eq!(
            captions.get(1).copied().flatten(),
            None,
            "switch {sw}'s SOLVE tick must carry no caption — that absence is what flips the \
             row's verb back from the prepass's: {seen:?}"
        );
    }
}

/// The SCENE half of the caption contract above. This lane already ticked per scene during its
/// prepass — its highlight followed the device — but both phases built the payload through
/// `scene_progress_item(.., None)`, so the two ticks were byte-identical and the prepass was
/// indistinguishable from the solve: with a live capture streaming throughout, a scene being
/// MEASURED read exactly like a scene being LEVELED.
///
/// Same two clauses as the FS gate, minus the ordering one (which held here already): each
/// scene's FIRST tick carries `leveller::PREPASS_ACTIVE_MSG`, its later ticks carry none.
/// Fixture 403 is the 4-scene preset the sibling solve/off-branch gate uses.
#[test]
fn the_scene_prepass_captions_its_own_phase_and_the_solve_does_not() {
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

    let items: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>> = Default::default();
    let sink = items.clone();
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .channel_interceptor(move |_wv, _cb, _id, body| {
            if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                    lock_ok(&sink).push(v);
                }
            }
            true
        })
        .invoke_handler(tauri::generate_handler![level_scenes_apply_batched])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let amp = serde_json::json!([
        {"groupId": "G2", "nodeId": "ampA", "parameterId": "outputLevel", "value": 1.0},
        {"groupId": "G3", "nodeId": "ampB", "parameterId": "outputLevel", "value": 1.0}
    ]);
    invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": 403,
            "jobs": (0..4).map(|s| serde_json::json!({"sceneSlot": s, "targetLufs": -23.0})).collect::<Vec<_>>(),
            "candidates": amp,
            "save": false, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:7"
        }),
    )
    .expect("level_scenes_apply_batched");

    let seen = lock_ok(&items).clone();
    for scene in 0..4u64 {
        let captions: Vec<Option<&str>> = seen
            .iter()
            // `SceneLevelProgressItem` is `rename_all = "camelCase"` — the wire key is
            // `sceneSlot`. (The FS item's `switch` is one word, so it is unaffected.)
            .filter(|v| v["status"] == "active" && v["sceneSlot"].as_u64() == Some(scene))
            .map(|v| v["message"].as_str())
            .collect();
        assert_eq!(
            captions.first().copied().flatten(),
            Some(crate::leveller::PREPASS_ACTIVE_MSG),
            "scene {scene}'s prepass tick must caption its own phase: {seen:?}"
        );
        assert!(
            captions[1..].iter().all(|c| c.is_none()),
            "scene {scene}'s solve ticks must carry no caption — that absence is what flips the \
             row's verb back: {seen:?}"
        );
    }
}

/// THE FS PREPASS CEILING (the reordered run's footswitch half): one engage with the leveling
/// handle PINNED AT THE TOP of its range reads how loud this footswitch sound can possibly be
/// — a MEASUREMENT, never an extrapolation, because an arbitrary block param has no
/// algebraically predictable response.
///
/// Slot 405's Plumes switch (5) is the fixture: its block is OFF in base, so engaging it puts
/// the saturated-amp curve in charge, and the ceiling must equal that curve AT THE HANDLE'S
/// TOP BOUND (`saturated_pedal_lufs(1.0)`), not at its authored 0.5. A prepass that forgot to
/// pin the handle, or that measured the disengaged sound, both fail here.
///
/// It must also WRITE NOTHING PERSISTENT: every write lands on the throwaway capture
/// connection's working copy, which is what makes a ceiling read safe to take before the run
/// has decided anything.
#[test]
fn the_fs_prepass_reads_the_ceiling_at_the_handles_top_bound() {
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

    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let p24 = spec
        .iter()
        .find(|p| p.list_index == 405)
        .expect("405 present");
    let preset: serde_json::Value = serde_json::from_str(&p24.preset_json).expect("405 json");
    let ftsw = preset["ftsw"].clone();

    // Caller contract (same as `measure_footswitch`): the preset is already current.
    {
        let mut s = crate::session::Session::connect_lean().expect("connect");
        s.load_preset(405).expect("load 405");
    }
    let states = crate::footswitch::switch_states(&ftsw, &preset, 5);
    let handle = crate::leveller::FsParamTarget::new("ACD_Plumes", "level", 0.5);
    let probe = crate::leveller::FsCeilingProbe {
        scene: None,
        states: &states,
        handle: ("G1".to_string(), "ACD_Plumes".to_string(), handle.clone()),
    };
    // THROUGH THE SIM — the only half this seam test owns. (The probe's PURE composition is
    // pinned without a device by `leveller::reordered_run_tests::
    // the_ceiling_probe_pins_the_handle_at_its_top_bound_and_writes_it_last`.)
    let (_, hi) = handle.bounds();
    let ceiling = crate::leveller::measure_fs_ceiling(&probe, &stim, None).expect("ceiling read");
    let expected = crate::sim_device::saturated_pedal_lufs(hi).unwrap();
    assert!(
        (ceiling.integrated_lufs - expected).abs() < 0.3,
        "the ceiling is the engaged curve at the handle's TOP ({expected:.2}), not at its \
         authored value — got {:.2}",
        ceiling.integrated_lufs
    );
}

/// The BAKE write path end-to-end offline — the DOMINANT footswitch shape under the assign
/// gate (user directive, 2026-08-19): a switch only plans `Assign` when it ALREADY carries a
/// `param` function for the user-selected control, and slot 400's Boost switch (2) does not —
/// it ships a bare on-off, so leveling `ACD_Boost.gain` writes the block directly
/// (`FsLevelPlan::Bake`) rather than adding a function to the switch (the shape `danger.md`
/// forbids: a two-entry row is HW-proven to make the firmware silently discard the whole
/// imported preset). `gain` is the raw-dB `[0, 12]` param the write must carry unclamped.
///
/// Four things are pinned, each of which was silently unprovable offline before:
/// 1. the run COMPLETES with the write routed through `FsWrite::Bake`, not the now-refused
///    append (`resolve_footswitch_job` would `Err` on this exact switch — see
///    `resolve_footswitch_job_refuses_rather_than_append_a_new_function`),
/// 2. the wire op sequence carries a `ChangeParameter` fire-and-forget setter for
///    `ACD_Boost.gain` — and NO `setFootswitchAssignment`(54) at all, because a Bake never
///    touches the switch's own row,
/// 3. the switch keeps its bare single on-off entry: the working-copy `ftsw` a
///    `Session::live_ftsw` reads is byte-identical before and after,
/// 4. the baked value SURVIVES the save — `leveller::verify_fs_persisted_writes` re-reads
///    field 8 and finds the solved value in `ACD_Boost.gain`'s own `dspUnitParameters`, so an
///    offline Bake row can report an honest `persist_mismatch: Some(false)`.
///
/// NOT pinned here (and deliberately): the loudness RESPONSE. Slot 400 declares no
/// `leveledParams`, so the sim's capture model is flat in `gain` — the target below is the
/// fixture's own base C so the solve converges on its first seed instead of chasing a curve
/// that the offline model doesn't have. See this test's sibling note in the report on the
/// wet-floor gap.
#[test]
fn bake_path_footswitch_writes_the_block_directly_and_persists_its_value() {
    let _serial = serial();
    let _reset = RegistryReset;
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
    crate::leveller::clear_slot_save_registry();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let stim = test_stim();

    let spec_json = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec_json
        .iter()
        .find(|p| p.list_index == 400)
        .expect("400 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("400 json");
    let ftsw = preset["ftsw"].clone();

    const SWITCH: u32 = 2; // the Boost switch — a bare on-off, no `param` fn on it
    const NODE: &str = "ACD_Boost";
    const PARAM: &str = "gain";
    // Base C for slot 400 (`scenario-loudness.json`) at the fixture's own presetLevel — see
    // the note above on why the target sits exactly there.
    const TARGET: f64 = -15.0;

    // The plan really is Bake — switch 2 carries no `param` fn on (ACD_Boost, gain), which is
    // exactly the assign gate's discriminator. If the fixture ever GAINS such a fn this test
    // would quietly become a second Assign gate.
    assert!(
        crate::footswitch::existing_param_fn_index(&ftsw, SWITCH, NODE, PARAM).is_none(),
        "this fixture's premise is a switch with NO existing param fn for the control"
    );
    let plans = crate::footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[crate::footswitch::FsJobKey {
            switch: SWITCH,
            lev_node: NODE,
            lev_param: PARAM,
            target_bits: TARGET.to_bits(),
        }],
    );
    let (engaged, clear_stale, mirror_scenes) = match &plans[0] {
        crate::footswitch::FsLevelPlan::Bake {
            engaged,
            clear_stale,
            mirror_scenes,
        } => (engaged.clone(), *clear_stale, mirror_scenes.clone()),
        other => panic!("400 switch {SWITCH} must plan as Bake, got {other:?}"),
    };
    assert!(
        clear_stale.is_none(),
        "nothing stale to clear — this switch never had a param fn to begin with: \
         {clear_stale:?}"
    );

    // The authored base gain — the `FsWrite::Bake` command path never calls
    // `resolve_footswitch_job` (that seam is Assign-only); it reads the block directly.
    let value_b = crate::commands::level_footswitch::node_param_f64(&preset, NODE, PARAM)
        .expect("ACD_Boost.gain must be a numeric dspUnitParameter");
    assert!(
        (value_b - 2.5).abs() < 1e-6,
        "valueB is ACD_Boost's authored base gain: {value_b}"
    );

    let r = crate::leveller::level_footswitch(
        400,
        SWITCH,
        ("G1", NODE, PARAM),
        &engaged,
        &crate::leveller::FsWrite::Bake {
            clear_stale,
            mirror_scenes: mirror_scenes.clone(),
        },
        &stim,
        TARGET,
        true,  // save
        false, // no re-measure verify
        crate::last_loaded_scene(&preset),
        &crate::leveller::FsParamTarget::new(NODE, PARAM, value_b as f32),
    )
    .expect("the Bake run must COMPLETE");
    assert_eq!(r.method, "baked");
    assert!(r.saved, "save: true must persist the bake: {r:?}");
    assert!(
        r.clamp_reason.is_none(),
        "ACD_Boost is on the trunk — no routing clamp: {r:?}"
    );

    // A raw-dB param must reach the wire unclamped (the `[0, 12]` range's own 2.5 seed).
    assert!(
        r.final_value > 1.0,
        "a raw-dB gain solve must not be pinned inside [0,1]: {r:?}"
    );

    // (2) the wire op: a `ChangeParameter` fire-and-forget setter for ACD_Boost.gain with the
    // SOLVED value, and NO `setFootswitchAssignment`(54) at all — a Bake never touches the
    // switch's own row.
    let events = sim.events();
    let baked = events
        .iter()
        .find_map(|e| match e {
            crate::sim_device::SimEvent::ChangeParameter {
                group,
                node,
                param,
                value,
                ..
            } if node == NODE && param == PARAM => Some((group.clone(), *value)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ChangeParameter write for {NODE}.{PARAM}: {events:?}"));
    assert_eq!(baked.0, "G1");
    assert!(
        (baked.1 - r.final_value).abs() < 1e-6,
        "the wire value must be the SOLVED value {}: {baked:?}",
        r.final_value
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::SetFootswitchAssignment { .. }
        )),
        "a Bake must never touch the switch's own ftsw row: {events:?}"
    );

    // (3) the CONFIRM channel: `Session::change_parameter` is a fire-and-forget setter with
    // no reply (its own doc comment) — unlike an Assign there is no read-back to check here.
    // The switch's own `ftsw` row is untouched: confirm that directly, so a future Bake that
    // accidentally starts writing `ftsw` (re-introducing the forbidden two-entry shape) fails
    // this test rather than sailing through silently.
    {
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(400).expect("load 400");
        let live = s.live_ftsw().expect("the field-2 re-prompt must answer");
        assert!(
            crate::footswitch::existing_param_fn_index(&live, SWITCH, NODE, PARAM).is_none(),
            "a Bake must never add a param fn to the switch: {live}"
        );
        let sw = live
            .as_array()
            .and_then(|a| a.get(SWITCH as usize))
            .and_then(|s| s.as_array())
            .expect("switch 2's row");
        assert_eq!(
            sw.len(),
            1,
            "switch 2 must keep its single bare on-off entry, never gain a second: {sw:?}"
        );
    }

    // (4) the save round trip, through the production persist verify (field-8 → the block's
    // own dspUnitParameters). `is_assign: false` reads the block, not `ftsw.valueA`.
    let mut results = vec![Some(r.clone())];
    crate::leveller::verify_fs_persisted_writes(
        400,
        &[(0, NODE.to_string(), PARAM.to_string(), r.final_value, false)],
        None,
        &mut results,
    );
    assert_eq!(
        results[0].as_ref().and_then(|x| x.persist_mismatch),
        Some(false),
        "the saved preset must hold the bake's value on the block: {:?}",
        results[0]
    );

    // (5) the STALE-LOAD barrier can see it. The write session registered a
    // `SaveWitness::Param` carrying the solved value, and for a Bake
    // `leveller::witness_value_in_doc` matches it against the block's own `dspUnitParameters`
    // (the `ftsw` row never changed and never could witness this write). A bake that doesn't
    // survive the save would leave the witness unharvestable, and the barrier would silently
    // fall out through its ~2-minute time gate on every offline Bake — so the elapsed bound,
    // not just the `Ok`, is the assertion.
    assert!(
        crate::leveller::slot_save_pending_commit(400),
        "the write session must have REGISTERED a witness — without an entry the barrier \
         below is a no-op and asserts nothing"
    );
    let start = std::time::Instant::now();
    assert!(
        crate::leveller::ensure_fresh_load(400, &mut || false).is_ok(),
        "the barrier must clear on the bake's own witness"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "the witness must harvest on the FIRST load, not via the time gate: {:?}",
        start.elapsed()
    );
}

/// THE BAKE-ARM IDEMPOTENCY GATE (this PR): before the fix, `level_footswitches_apply`'s Bake
/// arm called `measure_footswitch(..., None, ...)` unconditionally — no anchor for the re-run
/// probe in `solve_param_secant` — so a Bake row re-solved and RE-SAVED an identical value on
/// EVERY re-run (~6 min of device time, HW-confirmed tonight). This is the Bake-arm sibling of
/// `assign_path_footswitch_edits_its_existing_function_at_its_own_index_and_persists_its_value_a`'s
/// gate, but drives the actual BATCHED COMMAND (`level_footswitches_apply`) rather than the
/// single-switch `leveller::level_footswitch` probe seam — that seam's own doc says it "always
/// solve[s] fresh (no idempotency probe)" BY DESIGN (the batched command owns the re-run skip),
/// so it cannot exercise the regressed path at all.
///
/// Fixture 405's Plumes switch (5) is the shape: `ACD_Plumes` is bypassed in base and switch 5
/// carries a bare on-off with no `param` fn, so `plan_footswitch_jobs` bakes it (same premise as
/// `bake_path_footswitch_writes_the_block_directly_and_persists_its_value`, a different
/// fixture/switch). Its `level` knob rides the sim's `saturated_pedal_lufs` curve, so a target
/// off the AUTHORED 0.5 (→ -16.14 LUFS at that curve) forces run 1 to actually solve and write —
/// a fixture that already sat on target would make run 2's skip prove nothing.
///
/// `Saved` (not `ChangeParameter`) is the discriminator because the ceiling prepass engages once
/// per row on EVERY run regardless of the fix (a known, accepted cost — see `CLAUDE.md`'s
/// trade-offs) and writes its own throwaway `ChangeParameter` at the handle's top bound; only the
/// actual write session (`write_footswitch_values`, gated on `pending` being non-empty) ever
/// emits `Saved`.
///
/// WITHOUT the fix this is RED on run 2's `saved: false` / no-new-`Saved` assertions: the old
/// unconditional `None` anchor never lets the probe fire, and the old `save &&
/// r.clamp_reason.is_none()` guard (no comparison to `current`) pushes the identical value to
/// `pending` regardless, so `write_footswitch_values` runs again and a second `Saved` event
/// lands. The same two assertions also catch a HALF-applied fix: reading `current` correctly but
/// leaving the unconditional push still saves every time; guarding the push but leaving
/// `current: None` makes `Some(final_value) != None` always true, which also always saves.
#[test]
fn bake_path_footswitch_rerun_skips_the_persist_when_already_at_target() {
    let _serial = serial();
    let _reset = RegistryReset;
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
    crate::leveller::clear_slot_save_registry();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));

    const SWITCH: u32 = 5;
    const NODE: &str = "ACD_Plumes";
    const PARAM: &str = "level";
    // Reachable (the handle's top-bound ceiling reads ~ -14 LUFS on this curve) and >1 LU off
    // the authored 0.5's -16.14, so run 1 truly solves rather than trivially matching already.
    const TARGET: f64 = -15.0;

    let spec_json = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let p24 = spec_json
        .iter()
        .find(|p| p.list_index == 405)
        .expect("405 present");
    let preset: serde_json::Value = serde_json::from_str(&p24.preset_json).expect("405 json");
    let ftsw = preset["ftsw"].clone();
    assert!(
        crate::footswitch::existing_param_fn_index(&ftsw, SWITCH, NODE, PARAM).is_none(),
        "premise: switch 5 must carry no existing param fn on (ACD_Plumes, level) — a bare \
         on-off, so the row bakes"
    );
    let plans = crate::footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[crate::footswitch::FsJobKey {
            switch: SWITCH,
            lev_node: NODE,
            lev_param: PARAM,
            target_bits: TARGET.to_bits(),
        }],
    );
    assert!(
        matches!(plans[0], crate::footswitch::FsLevelPlan::Bake { .. }),
        "premise: switch 5 must plan Bake, got {:?}",
        plans[0]
    );
    let authored = crate::commands::level_footswitch::node_param_f64(&preset, NODE, PARAM)
        .expect("ACD_Plumes.level must be numeric");
    assert!(
        (authored - 0.5).abs() < 1e-6,
        "fixture premise: the authored level is 0.5: {authored}"
    );

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_footswitches_apply])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");

    let job = serde_json::json!({
        "switch": SWITCH,
        "levGroupId": "G1",
        "levNodeId": NODE,
        "levParameterId": PARAM,
        "targetLufs": TARGET,
    });
    let apply = |webview: &WebviewWindow<MockRuntime>| {
        invoke(
            webview,
            "level_footswitches_apply",
            serde_json::json!({
                "slot": 405,
                "jobs": [job],
                "save": true,
                "topologyId": serde_json::Value::Null,
                "calibrationLufs": null,
                "profileId": null,
                "onResult": "__CHANNEL__:0",
            }),
        )
        .expect("level_footswitches_apply")
    };

    // ── RUN 1 ── must actually solve and persist (the non-vacuous premise run 2's skip needs).
    let r1 = apply(&webview);
    assert_eq!(
        r1[0]["clamped"], false,
        "run 1 must reach target unclamped: {r1:?}"
    );
    assert_eq!(r1[0]["saved"], true, "run 1 must solve and save: {r1:?}");
    // Pin the premise: run 1 must actually CONVERGE (not fall back to a best-effort
    // best-point-found re-solve). Run 1 currently converges by accepting seed 0.75 with only
    // 29% of FS_TOL_LU headroom — if a future tolerance/curve shift makes it unconverged, run
    // 2 would legitimately re-solve too, and the no-new-`Saved` gate below would false-red on
    // a correct run. Fail here, at the premise, instead of misattributing that to a
    // regression in the fix.
    assert_eq!(
        r1[0]["unconverged"], false,
        "run 1 must fully converge (see the seed-headroom note above): {r1:?}"
    );
    let final_value_1 = r1[0]["final_value"].as_f64().expect("final_value");
    assert!(
        (final_value_1 - authored).abs() > 0.05,
        "run 1 must move the value off the authored 0.5, or run 2's skip proves nothing: {r1:?}"
    );
    let events1 = sim.events();
    let saved_events_after_run1 = events1
        .iter()
        .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(_)))
        .count();
    assert_eq!(
        saved_events_after_run1,
        1,
        "run 1 must emit exactly one Saved event: {:?}",
        sim.events()
    );
    assert!(
        !events1.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::SetFootswitchAssignment { .. }
        )),
        "a Bake must never touch the switch's own ftsw row: {:?}",
        sim.events()
    );

    // ── RUN 2 ── the re-run: the block's stored value is now run 1's `final_value`, already on
    // target — the idempotency probe must find it and skip the write entirely.
    let r2 = apply(&webview);
    assert_eq!(
        r2[0]["clamped"], false,
        "run 2 must still read unclamped: {r2:?}"
    );
    assert_eq!(
        r2[0]["saved"], false,
        "run 2 solved the same value already saved → must skip the write: {r2:?}"
    );
    let final_value_2 = r2[0]["final_value"].as_f64().expect("final_value");
    assert!(
        (final_value_2 - final_value_1).abs() < 1e-6,
        "the skip must return the CURRENT value verbatim, not re-solve/re-randomize it: \
         run1={final_value_1} run2={final_value_2}"
    );
    let saved_events_after_run2 = sim
        .events()
        .iter()
        .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(_)))
        .count();
    assert_eq!(
        saved_events_after_run2,
        saved_events_after_run1,
        "run 2 must emit NO new Saved event — a re-solve+re-save of an in-tolerance Bake row is \
         exactly the ~6-minute HW-confirmed regression this gate exists to catch: {:?}",
        sim.events()
    );
}

/// The ASSIGN write path end-to-end offline — the shape a switch takes ONLY when it already
/// carries a `param` function for the user-selected control (the assign gate, user directive
/// 2026-08-19). Slot 400's param-only switches (VERB KILL, WAH SWEEP) don't fit this test: both
/// target a node some OTHER switch also toggles on-off, so `siblings_off_excluding`'s isolation
/// — which only ever excludes THIS switch's OWN on-off nodes, and a param-only switch has none —
/// force-bypasses the very block being leveled (the "ponytail" edge case `plan_footswitch_jobs`
/// already flags as acknowledged-not-handled). That's a real, separate gap, not this gate's
/// subject, so it stays unexercised here rather than being smuggled in as if it were clean.
///
/// Slot 407 (the Doctor-oracle fixture) has a clean case instead: switch 13's WASHED is a
/// `param` fn on `plate1.wetdrymix` (a `wet_mix`-classified control, unlike Slot 400's
/// raw-dB `ACD_Boost.gain`), and NO other switch in 407 references that node at all — so the
/// isolation list can never touch it. `wetdrymix` is `bypass: false` in base, so the switch
/// plans `Assign`.
///
/// Four things are pinned, each of which was silently unprovable offline before:
/// 1. the run COMPLETES via `FsWrite::Assign`,
/// 2. the wire op sequence carries the field-54 write with the solved `valueA` and the
///    resolved `valueB` at the function's OWN index (0) — never appended at `sw.len()`,
///    matching `resolve_footswitch_job`'s edit-in-place resolution,
/// 3. the CONFIRM came from the read-back, not from an echo: the working-copy `ftsw` a
///    `Session::live_ftsw` reads renders the EDITED function at the same index, with every
///    switch-level field (`colorA`/`colorB`/`customLabel`/`linkGroup`/`switchType`) preserved
///    exactly as the switch already had them,
/// 4. the assign SURVIVES the save — `leveller::verify_fs_persisted_writes` re-reads field 8
///    and finds the solved value as `ftsw`'s `valueA`.
///
/// `TARGET` sits at 407's own base C (`scenario-loudness.json`: "Base C=-14 solves at every
/// default"), so the solve converges on its first seed rather than chasing a curve the offline
/// model doesn't have — same reasoning as the Bake gate's own target pick.
#[test]
fn assign_path_footswitch_edits_its_existing_function_at_its_own_index_and_persists_its_value_a() {
    let _serial = serial();
    let _reset = RegistryReset;
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
    crate::leveller::clear_slot_save_registry();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let stim = test_stim();

    let spec_json = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec_json
        .iter()
        .find(|p| p.list_index == 407)
        .expect("407 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("407 json");
    let ftsw = preset["ftsw"].clone();

    const SWITCH: u32 = 13; // WASHED — an existing `param` fn, no ponytail conflict
    const NODE: &str = "plate1";
    const PARAM: &str = "wetdrymix";
    const TARGET: f64 = -14.0; // 407's own base C

    // The plan really is Assign — switch 7 ALREADY changes (NODE, PARAM), at its own index 0.
    assert_eq!(
        crate::footswitch::existing_param_fn_index(&ftsw, SWITCH, NODE, PARAM),
        Some(0),
        "this fixture's premise is a switch whose ONLY entry already edits this control"
    );
    let plans = crate::footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[crate::footswitch::FsJobKey {
            switch: SWITCH,
            lev_node: NODE,
            lev_param: PARAM,
            target_bits: TARGET.to_bits(),
        }],
    );
    let engaged = match &plans[0] {
        crate::footswitch::FsLevelPlan::Assign { engaged } => engaged.clone(),
        other => panic!("407 switch {SWITCH} must plan as Assign, got {other:?}"),
    };
    assert!(
        !engaged.iter().any(|(_, n, _)| n == NODE),
        "no OTHER switch in 407 toggles this node, so the isolation list must never force it: \
         {engaged:?}"
    );

    // The production resolution: edit the EXISTING function at ITS OWN index, inheriting the
    // switch's own display fields — never append.
    let job = crate::commands::level_footswitch::FootswitchLevelJob {
        switch: SWITCH,
        lev_group_id: "G1".into(),
        lev_node_id: NODE.into(),
        lev_parameter_id: PARAM.into(),
        target_lufs: TARGET,
        scene_context: None,
    };
    let (value_b, write_spec) =
        crate::commands::level_footswitch::resolve_footswitch_job(&ftsw, &preset, &job)
            .expect("resolve the WASHED job");
    assert!(
        value_b.abs() < 1e-6,
        "valueB is plate1's authored base wetdrymix (fully dry): {value_b}"
    );
    let function_index = write_spec.function_index;
    assert_eq!(
        function_index, 0,
        "the leveling param fn EDITS the switch's own existing function — never appends"
    );
    assert_eq!(
        write_spec.color_a, 5,
        "switch-level fields must be preserved: {write_spec:?}"
    );
    assert_eq!(
        write_spec.color_b, 0,
        "switch-level fields must be preserved: {write_spec:?}"
    );
    assert_eq!(
        write_spec.custom_label, "WASHED",
        "switch-level fields must be preserved: {write_spec:?}"
    );
    assert_eq!(
        write_spec.link_group, 0,
        "switch-level fields must be preserved: {write_spec:?}"
    );
    assert_eq!(
        write_spec.switch_type, 0,
        "switch-level fields must be preserved: {write_spec:?}"
    );
    assert!(
        !write_spec.is_active,
        "the fixture's own function is authored disengaged: {write_spec:?}"
    );

    let param = crate::leveller::FsParamTarget::new(NODE, PARAM, value_b);
    assert_eq!(
        param.info.class,
        crate::param_class::ParamClass::WetMix,
        "plate1.wetdrymix must classify wet_mix"
    );
    let r = crate::leveller::level_footswitch(
        407,
        SWITCH,
        ("G1", NODE, PARAM),
        &engaged,
        &crate::leveller::FsWrite::Assign {
            value_b,
            spec: write_spec,
        },
        &stim,
        TARGET,
        true,  // save
        false, // no re-measure verify
        crate::last_loaded_scene(&preset),
        &param,
    )
    .expect("the Assign run must COMPLETE — it used to fail its confirm gate offline");
    assert_eq!(r.method, "assigned");
    assert!(r.saved, "save: true must persist the assign: {r:?}");
    assert!(
        r.clamp_reason.is_none(),
        "plate1 is on the trunk — no routing clamp: {r:?}"
    );

    // (2) the field-54 write, at the resolved (OWN, never appended) index, with the solved
    // valueA and the resolved valueB.
    let events = sim.events();
    let assign = events
        .iter()
        .find_map(|e| match e {
            crate::sim_device::SimEvent::SetFootswitchAssignment {
                addr,
                index,
                function_json,
                swap,
            } if *addr == SWITCH => Some((*index, function_json.clone(), *swap)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no field-54 write for switch {SWITCH}: {events:?}"));
    assert_eq!(
        assign.0, function_index,
        "written at the resolved (own) index"
    );
    assert!(!assign.2, "production never sets the swap flag");
    let func: serde_json::Value = serde_json::from_str(&assign.1).expect("functionJson parses");
    assert_eq!(func["func"], "param");
    assert_eq!(func["nodeId"], NODE);
    assert_eq!(func["parameterId"], PARAM);
    assert_eq!(
        func["customLabel"], "WASHED",
        "the switch's own label survives the edit"
    );
    assert!(
        (func["valueA"].as_f64().expect("valueA") - f64::from(r.final_value)).abs() < 1e-6,
        "valueA must be the SOLVED value {}: {func}",
        r.final_value
    );
    assert!(
        (func["valueB"].as_f64().expect("valueB") - f64::from(value_b)).abs() < 1e-6,
        "valueB must be the switch-OFF base value: {func}"
    );

    // (3) the CONFIRM channel: the working copy a `live_ftsw` read renders carries the EDITED
    // function at the SAME index — the switch still carries exactly one entry.
    {
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(407).expect("load 407");
        let live = s.live_ftsw().expect("the field-2 re-prompt must answer");
        assert_eq!(
            crate::footswitch::existing_param_fn_index(&live, SWITCH, NODE, PARAM),
            Some(0),
            "the edited function must still be at its own index: {live}"
        );
        let value_a = crate::footswitch::existing_param_fn_value_a(&live, SWITCH, NODE, PARAM)
            .unwrap_or_else(|| panic!("no param fn on switch {SWITCH} in the live ftsw: {live}"));
        assert!(
            (value_a - f64::from(r.final_value)).abs() < 1e-6,
            "the live ftsw must render the solved valueA, got {value_a}"
        );
        let sw = live
            .as_array()
            .and_then(|a| a.get(SWITCH as usize))
            .and_then(|s| s.as_array())
            .expect("switch 7's row");
        assert_eq!(
            sw.len(),
            1,
            "an edit-in-place must never grow the row to two entries: {sw:?}"
        );
    }

    // (4) the save round trip, through the production persist verify (field-8 → ftsw valueA).
    let mut results = vec![Some(r.clone())];
    crate::leveller::verify_fs_persisted_writes(
        407,
        &[(0, NODE.to_string(), PARAM.to_string(), r.final_value, true)],
        None,
        &mut results,
    );
    assert_eq!(
        results[0].as_ref().and_then(|x| x.persist_mismatch),
        Some(false),
        "the saved preset must hold the assign's valueA: {:?}",
        results[0]
    );

    // (5) the STALE-LOAD barrier can see it, same contract as the Bake gate above.
    assert!(
        crate::leveller::slot_save_pending_commit(407),
        "the write session must have REGISTERED a witness — without an entry the barrier \
         below is a no-op and asserts nothing"
    );
    let start = std::time::Instant::now();
    assert!(
        crate::leveller::ensure_fresh_load(407, &mut || false).is_ok(),
        "the barrier must clear on the assign's own witness"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "the witness must harvest on the FIRST load, not via the time gate: {:?}",
        start.elapsed()
    );
}

/// COVERAGE row 18 — the WET-FLOOR outcome, end to end offline. 400's SPRING switch (3) is a
/// bare on-off with no `param` fn on `ACD_TMSpring63`, so its plan is `Bake` (the assign gate,
/// user directive 2026-08-19) and its `mix` (authored 0.42) classifies `wet_mix`:
/// `FsParamTarget::bounds` raises the solve's LOW bound to `WET_FLOOR_FRACTION` (0.25) x 0.42
/// = 0.105, because driving a reverb's mix toward 0 to hit a loudness target doesn't make the
/// effect quieter, it DELETES it.
///
/// Two runs, one gate:
/// * an UNREACHABLE target pins at that floor and reports `wet_floor: true` — the honest
///   "quieter ON than OFF, verify it by ear" outcome the UI renders from this flag
///   (`useLevelingFlow.ts`'s `verifyByEar`), and NOT a routing clamp (`clamp_reason` stays
///   `None`, whose contract is "no signal on USB 1/2" alone),
/// * a REACHABLE target converges inside the band with `wet_floor: false`, proving the flag
///   tracks the outcome rather than the param's class.
///
/// This was unprovable offline until two things landed together (either alone leaves it
/// red): the sidecar's `leveledParams` entry for `400 / G1 / ACD_TMSpring63 / mix` on the
/// `wetMix` curve, and `model_lufs`'s widened activation predicate — a Bake's isolation
/// (`siblings_off_excluding`) forces only the OTHER switches' blocks off, so the LEVELED
/// block gets no bypass write and the old `bypass_writes[node] == Some(false)` predicate
/// never fired. With a flat capture model both seeds read the same C, `solve_param_secant`
/// took its no-authority `flat_response` exit at the low SEED (`at_fraction(0.25)` = 0.32875)
/// and never reached the floor at all — which is what row 18 previously mis-attributed to
/// the Bake path skipping the floor.
///
/// `save: false` on BOTH runs, deliberately: the sim's `preset_level` starts at 1.0 and a
/// load only restores the fixture's own 0.32 once the slot has been saved this run, so a
/// saving first run would shift every later capture by -9.9 LU and the second assertion
/// would fail for a reason that looks nothing like its cause. The Bake save round trip is
/// `bake_path_footswitch_writes_the_block_directly_and_persists_its_value`'s job.
#[test]
fn wet_mix_footswitch_bakes_and_pins_at_the_wet_floor_on_an_unreachable_target() {
    let _serial = serial();
    let _reset = RegistryReset;
    let (r, param) = solve_400_spring(-70.0);
    assert_eq!(r.method, "baked");
    assert!(r.clamped, "an unreachable target must clamp: {r:?}");
    assert!(
        r.wet_floor,
        "the clamp's cause is the WET FLOOR, and that flag is the whole UI advisory: {r:?}"
    );
    assert!(
        r.clamp_reason.is_none(),
        "a wet-floor clamp is not a routing clamp — that reason means 'no signal on USB 1/2' \
         only: {r:?}"
    );
    let floor = 0.42 * crate::leveller::WET_FLOOR_FRACTION;
    assert!(
        (floor - 0.105).abs() < 1e-6,
        "the fixture's authored mix must still be 0.42: floor {floor}"
    );
    assert!(
        (r.final_value - floor).abs() < 1e-4,
        "the written value must BE the floor ({floor}), never below it: {r:?}"
    );
    assert!(
        (r.final_value - param.bounds().0).abs() < 1e-6,
        "the floor IS the solve's low bound, so no probe ever went under it: {r:?}"
    );
    assert!(!r.saved, "save: false must write nothing: {r:?}");
}

/// The companion half of the row-18 gate: the SAME switch at a target the wet-mix curve can
/// actually reach converges inside `FS_TOL_LU` with `wet_floor: false`. -16 is the pick
/// because the curve puts the wet floor at -17.18 and the authored mix at -15, so -16 solves
/// near mix 0.27 — a 1.18 LU margin over the floor's own reading, ~12x the 0.1 LU tolerance,
/// so an epsilon in the model can't flip this row into a floor clamp and quietly retire the
/// discrimination this pair exists to prove.
#[test]
fn wet_mix_footswitch_bakes_and_converges_and_stays_off_the_floor_on_a_reachable_target() {
    let _serial = serial();
    let _reset = RegistryReset;
    const TARGET: f64 = -16.0;
    let (r, param) = solve_400_spring(TARGET);
    assert_eq!(r.method, "baked");
    assert!(
        !r.clamped && !r.unconverged,
        "a reachable target must SOLVE: {r:?}"
    );
    assert!(
        !r.wet_floor,
        "wet_floor tracks the OUTCOME, not the param's class: {r:?}"
    );
    assert!(
        (r.predicted_lufs - TARGET).abs() <= 0.1,
        "the achieved loudness must land within FS_TOL_LU of {TARGET}: {r:?}"
    );
    let (lo, hi) = param.bounds();
    assert!(
        r.final_value > lo + 1e-3 && r.final_value < hi,
        "the solved mix must sit strictly inside ({lo}, {hi}): {r:?}"
    );
}

/// Shared body of the two wet-floor gates: install a fresh sim, prove 400's SPRING switch
/// (the on-off, switch 3) really plans as `Bake` — it carries no `param` fn on
/// `(ACD_TMSpring63, mix)`, only the plain on-off (a fixture edit that added such a fn, or
/// that bypassed `ACD_TMSpring63` in base, would otherwise silently turn both gates into a
/// different path) — read the authored base mix off the block, and run the single-switch
/// seam DRY. Returns the result plus the classified target, so each caller can assert against
/// the same `bounds()` the solve used.
fn solve_400_spring(
    target_lufs: f64,
) -> (
    crate::leveller::FootswitchLevelResult,
    crate::leveller::FsParamTarget,
) {
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
    crate::leveller::clear_slot_save_registry();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    let stim = test_stim();

    const SWITCH: u32 = 3; // SPRING — a bare on-off, no `param` fn on it
    const NODE: &str = "ACD_TMSpring63";
    const PARAM: &str = "mix";

    let spec_json = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec_json
        .iter()
        .find(|p| p.list_index == 400)
        .expect("400 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("400 json");
    let ftsw = preset["ftsw"].clone();

    assert!(
        crate::footswitch::existing_param_fn_index(&ftsw, SWITCH, NODE, PARAM).is_none(),
        "this fixture's premise is a switch with NO existing param fn for the control"
    );
    let plans = crate::footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[crate::footswitch::FsJobKey {
            switch: SWITCH,
            lev_node: NODE,
            lev_param: PARAM,
            target_bits: target_lufs.to_bits(),
        }],
    );
    let (engaged, clear_stale, mirror_scenes) = match &plans[0] {
        crate::footswitch::FsLevelPlan::Bake {
            engaged,
            clear_stale,
            mirror_scenes,
        } => (engaged.clone(), *clear_stale, mirror_scenes.clone()),
        other => panic!("400 switch {SWITCH} must plan as Bake, got {other:?}"),
    };
    // CONTRACT CORRECTED 2026-08-19 (HW, fw 1.8.45). This used to assert the opposite — that
    // a Bake isolates only the SIBLINGS and the leveled block gets NO bypass write, on the
    // reasoning that a block already ON in the base needs nothing said about it. Hardware
    // refuted it: a SIBLING row's isolation writes this block's `bypass = true` into the
    // device working copy, every capture's `recall_base` re-asserts that mutated copy, and
    // the row then measures with the very block it is levelling switched OFF (slot 26
    // "Plumes+BD2+OCD", every row clamped — `notes/gotchas.md`). The plan must state the
    // block's own engaged bypass, and must state EXACTLY what `switch_states` hands the
    // ceiling prepass, or plan and prepass describe different sounds.
    assert_eq!(
        engaged,
        crate::footswitch::switch_states(&ftsw, &preset, SWITCH).engaged_bypass,
        "a Bake's isolation must equal the prepass's own engaged state: {engaged:?}"
    );
    assert!(
        engaged.iter().any(|(_, n, byp)| n == NODE && !*byp),
        "the leveled block is ON in this fixture's base, so its own bypass must be asserted \
         OFF-bypass (engaged) rather than left unstated: {engaged:?}"
    );
    assert!(
        clear_stale.is_none(),
        "nothing stale to clear — this switch never had a param fn to begin with: \
         {clear_stale:?}"
    );

    // The `FsWrite::Bake` command path never calls `resolve_footswitch_job` (Assign-only); it
    // reads the block directly.
    let value_b = crate::commands::level_footswitch::node_param_f64(&preset, NODE, PARAM)
        .expect("ACD_TMSpring63.mix must be a numeric dspUnitParameter");
    assert!(
        (value_b - 0.42).abs() < 1e-6,
        "valueB is ACD_TMSpring63's authored base mix: {value_b}"
    );

    let param = crate::leveller::FsParamTarget::new(NODE, PARAM, value_b as f32);
    assert_eq!(
        param.info.class,
        crate::param_class::ParamClass::WetMix,
        "the whole gate rests on `mix` classifying wet_mix"
    );
    let r = crate::leveller::level_footswitch(
        400,
        SWITCH,
        ("G1", NODE, PARAM),
        &engaged,
        &crate::leveller::FsWrite::Bake {
            clear_stale,
            mirror_scenes,
        },
        &stim,
        target_lufs,
        false, // dry — see the gate's own doc for why a save would poison the capture model
        false,
        crate::last_loaded_scene(&preset),
        &param,
    )
    .expect("the SPRING solve must complete");
    (r, param)
}

/// The `ftsw` working-copy semantics of `setFootswitchAssignment`(54) /
/// `clearFootswitchAssignment`(55) at the WIRE level, one fact per step: APPEND at an index
/// past the switch's function count, REPLACE at an existing one, SPLICE (shift down) on a
/// clear, an unsaved edit is discarded by a fresh load, and a saved one survives it. The
/// clear side has no coverage in the Assign gate above — production only reaches it through
/// `FsWrite::Bake`'s `clear_stale` — and its read-back confirm
/// (`footswitch::existing_param_fn_index(..).is_none()`) was ALSO dead offline before the
/// field-2 re-prompt existed, since `live_ftsw` returned `None` for every caller.
#[test]
fn footswitch_assignment_set_and_clear_edit_the_working_copy_and_survive_only_a_save() {
    let _serial = serial();
    set_e2e_env(&[(
        "TMP_E2E_SCENARIO_PRESETS",
        "/../e2e/fixtures/scenario-presets.json",
    )]);
    let sim = crate::sim_device::SimDevice::new();
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));

    // Slot 400 switch 3 (SPRING) ships exactly ONE function, an on-off.
    const SWITCH: u32 = 3;
    let func = |value_a: f64| {
        serde_json::json!({
            "func": "param", "groupId": "G1", "nodeId": "ACD_TMSpring63",
            "parameterId": "mix", "valueA": value_a, "valueB": 0.42, "valueType": 2,
            "colorA": 1, "colorB": 0, "customLabel": "SPRING", "switchType": 0,
            "isActive": false, "linkGroup": 0
        })
        .to_string()
    };
    let fn_count = |s: &mut crate::session::Session| {
        s.live_ftsw()
            .and_then(|f| {
                f.as_array()
                    .and_then(|a| a.get(SWITCH as usize))
                    .and_then(|sw| sw.as_array())
                    .map(Vec::len)
            })
            .expect("the field-2 re-prompt must answer with an ftsw")
    };

    let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
    s.load_preset(400).expect("load 400");
    assert_eq!(
        fn_count(&mut s),
        1,
        "the fixture ships one on-off on SPRING"
    );

    // APPEND: index 1 is past the switch's single function.
    s.set_footswitch_assignment(SWITCH, 1, &func(0.30), false, None)
        .expect("set");
    assert_eq!(fn_count(&mut s), 2, "an index past the end APPENDS");

    // REPLACE: the same index now exists.
    s.set_footswitch_assignment(SWITCH, 1, &func(0.55), false, None)
        .expect("re-set");
    assert_eq!(
        fn_count(&mut s),
        2,
        "an existing index REPLACES, never grows"
    );
    let live = s.live_ftsw().expect("live ftsw");
    assert_eq!(
        crate::footswitch::existing_param_fn_value_a(&live, SWITCH, "ACD_TMSpring63", "mix"),
        Some(0.55),
        "the working copy holds the LAST write: {live}"
    );

    // A fresh load discards the UNSAVED edit (the device's own edit-buffer semantics).
    s.load_preset(400).expect("reload 400");
    assert_eq!(
        fn_count(&mut s),
        1,
        "an unsaved ftsw edit must not survive a load"
    );

    // SPLICE: re-add, then clear the on-off at index 0 — the param fn shifts down to 0.
    s.set_footswitch_assignment(SWITCH, 1, &func(0.55), false, None)
        .expect("set again");
    s.clear_footswitch_assignment(SWITCH, 0).expect("clear");
    let live = s.live_ftsw().expect("live ftsw");
    assert_eq!(
        fn_count(&mut s),
        1,
        "a clear removes the slot, leaving no hole"
    );
    assert_eq!(
        crate::footswitch::existing_param_fn_index(&live, SWITCH, "ACD_TMSpring63", "mix"),
        Some(0),
        "the surviving function SHIFTED down to index 0: {live}"
    );

    // Saved, it survives the load — the `SavedDoc::ftsw` round trip.
    s.save_current_preset(400).expect("save");
    s.load_preset(400).expect("reload after save");
    let live = s.live_ftsw().expect("live ftsw");
    assert_eq!(
        fn_count(&mut s),
        1,
        "the saved switch keeps its single function"
    );
    assert_eq!(
        crate::footswitch::existing_param_fn_value_a(&live, SWITCH, "ACD_TMSpring63", "mix"),
        Some(0.55),
        "a SAVED ftsw edit survives the load: {live}"
    );

    // Both wire ops are recorded, in order, with their decoded addressing.
    let ops: Vec<String> = sim
        .events()
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::SetFootswitchAssignment { addr, index, .. } => {
                Some(format!("set({addr},{index})"))
            }
            crate::sim_device::SimEvent::ClearFootswitchAssignment { addr, index } => {
                Some(format!("clear({addr},{index})"))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        ops,
        vec!["set(3,1)", "set(3,1)", "set(3,1)", "clear(3,0)"],
        "the fake must record every ftsw wire op in order"
    );
}

/// The SCENE-leveling physics for slot 403 through the REAL `level_scenes_apply_batched`
/// command over mock IPC — the same path the offline UI drives, minus the Channel-streaming
/// seam (`.claude/rules/e2e.md`'s "The Channel-streaming seam"): this gate asserts the
/// outcomes on the command's RETURN value instead. At the shipped default target (-23,
/// PR2 re-baseline: +3 from the mono-era -26) the 4 scenes produce the level-defaults outcome
/// set: 3 SOLVABLE (amp `outputLevel` converged to ~-23) + 1 OFF-BRANCH ("Clean", saved with
/// BOTH lane amps' output at zero → no
/// authority over the USB capture → the routing clamp). Proves the graph-echo fix (the prepass
/// classifies gtrParallel1 and picks BOTH lane amps for the joint-k solve) AND the sidecar
/// scene C authoring.
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
    // BOTH lane amps (the backup scan / list_level_blocks resolves the same pair) — this
    // preset re-merges two parallel amps, so the solve is joint-k over the two knobs.
    let amp = serde_json::json!([
        {"groupId": "G2", "nodeId": "ampA", "parameterId": "outputLevel", "value": 1.0},
        {"groupId": "G3", "nodeId": "ampB", "parameterId": "outputLevel", "value": 1.0}
    ]);
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
/// physics: slot 400 (E2E Rig) is the loud class — Base C=-15 solves, and scene 2
/// ("Ceiling", C=-14 with its amp saved at `outputLevel` 1.0) clamps at EVERY shipped
/// default because it has no boost headroom at all. `redistribute_headroom` raises
/// presetLevel by the solved delta and re-levels the base amp + BOTH scenes back to −23, so the
/// previously-clamped scene 2 reaches target (done, not clamped) and every sound lands near −23
/// — AND it records the pre-values (presetLevel + touched knobs) for the Summary's Restore.
/// This is the offline half of "clamped run →
/// redistribute → all done"; the base-scene skip + save-persistence idempotency are online
/// (the sim models no field-8 read-back / saved-state reload, same limit as `level.spec.ts`'s
/// idempotency test).
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
        "groupId": "G1", "nodeId": "ACD_JC120",
        "parameterId": "outputLevel", "value": 0.35
    }]);
    // Base (wire slot 8) + scene 2 (the clamped "Ceiling" one) + scene 1, all to −23 (PR2
    // re-baseline: +3 from the mono-era −26). `worstClampedDeficitDb` 6.0 is more than
    // enough to rescue scene 2's ~0.9 dB deficit.
    let jobs = serde_json::json!([
        {"sceneSlot": 8, "targetLufs": -23.0},
        {"sceneSlot": 2, "targetLufs": -23.0},
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
        "groupId": "G1", "nodeId": "ACD_JC120",
        "parameterId": "outputLevel", "value": 0.35
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
            crate::scene_overlay(&saved, 1, "ampA"),
            crate::SceneOverlay::Full(_)
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
                crate::SceneOverlay::Full(_)
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

/// BUG→GATE (2026-08-20 HW report, "Plumes+BD2+OCD", slot 30): BASE means the preset with NO
/// footswitch engaged. The command briefly measured base AS SAVED instead — every
/// footswitch-owned on-off block keeping its saved bypass — which on a preset whose pedal is
/// saved ON makes "Base" and that pedal's own footswitch row THE SAME SOUND. Measured with the
/// player's own DI through external ffmpeg `ebur128`: base-as-saved and the FS6 row both read
/// -22.99 LUFS (identical to three decimals, and FS6's solve converged on the value it started
/// from), while the same preset with its four pedals forced off sat 4.7 LU away at -27.69 —
/// never measured, never leveled, and unreachable by any row the run offered.
///
/// The as-saved experiment had been argued from an ffmpeg read of -18.3 LUFS against a -23.0
/// target on this same preset. That number was real but CONFOUNDED: on that day the block's own
/// footswitch row was clamping in every multi-row batch (see `footswitch.rs`'s isolation note),
/// which alone leaves the recalled sound exactly that hot. The clamp is fixed; base and its
/// pedal row are separately reachable again.
///
/// Slot 400 ("E2E Rig") is the offline fixture of that shape: three of its four block-acting
/// switches own a block that is `bypass: false` in the saved base graph (`ACD_TubeScreamer`,
/// `ACD_Boost`, `ACD_TMSpring63`) and one owns a block saved OFF (`ACD_CryBabyQ535`). A base run
/// must force EVERY one of them off, whatever its saved state.
///
/// SCOPE HONESTY: this pins the device-visible CAUSE — the same standard
/// `hiwatt_scene_leveling_never_reseeds_an_existing_overlay` documents — not the LU delta. No
/// shipped fixture declares a base-ENGAGED block in the sidecar's `leveledParams`, so the
/// loudness the isolation costs is not expressible against this fixture set (which is also why
/// the C assertion below is unchanged at -15: forcing these blocks off moves no modeled
/// loudness offline); that half is the hardware measurement quoted above. The C assertion is a
/// non-vacuity guard — the capture really went through the physics model — not the discriminator.
#[test]
fn base_leveling_forces_every_footswitch_owned_block_off_not_the_preset_as_saved() {
    let _serial = serial();
    let sim = hiwatt_sim(); // scenario + sidecar + backup + stimulus env, sim installed
    const RIG: u32 = 400;

    // Premise (fixture-drift guard): the fixture really does save footswitch-owned blocks ON,
    // else this gate would pass with the forcing fully restored.
    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec
        .iter()
        .find(|p| p.list_index == RIG)
        .expect("400 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("400 json");
    let onoff = crate::footswitch::all_onoff_blocks(&preset["ftsw"]);
    assert!(
        onoff.len() >= 4,
        "400 must own several on-off blocks: {onoff:?}"
    );
    let engaged_in_base: Vec<&String> = onoff
        .iter()
        .filter(|(g, n)| {
            preset
                .pointer(&format!("/audioGraph/guitarNodes/{g}"))
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|node| node.get("nodeId").and_then(|v| v.as_str()) == Some(n))
                })
                .and_then(|node| node.pointer("/dspUnitParameters/bypass"))
                == Some(&serde_json::Value::Bool(false))
        })
        .map(|(_, n)| n)
        .collect();
    assert!(
        engaged_in_base.len() >= 3,
        "the fixture must save footswitch-owned blocks ENGAGED — that is the incident shape: \
         {engaged_in_base:?}"
    );

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_preset])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    let job = serde_json::json!({
        "slot": RIG, "target_lufs": -23.0, "save": false,
        "topology_id": null, "calibration_lufs": null, "stimulus_path": null, "profile_id": null,
        "block_group_id": null, "block_node_id": null, "block_parameter_id": null,
        "block_value": null
    });
    let r = invoke(&webview, "level_preset", serde_json::json!({ "job": job }))
        .expect("level_preset 400");

    let events = sim.events();
    // Non-vacuity: the run actually engaged re-amp and measured through the physics model
    // (400's base C = -15), so "no bypass write" isn't just "the run died early".
    assert!(
        events
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::ReAmp(true))),
        "the base run engaged re-amp: {events:?}"
    );
    let c = r["constant_c"].as_f64().expect("constant_c");
    assert!(
        (c - (-15.0)).abs() < 0.5,
        "the base capture went through the model (400's base C = -15): {r}"
    );

    // THE GATE: every footswitch-owned block was forced OFF, whatever its saved bypass —
    // base is the preset with nothing switched on.
    let forced_off: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::Bypass { node, on: true }
                if onoff.iter().any(|(_, n)| n == node) =>
            {
                Some(node)
            }
            _ => None,
        })
        .collect();
    let missing: Vec<&String> = onoff
        .iter()
        .map(|(_, n)| n)
        .filter(|n| !forced_off.contains(n))
        .collect();
    assert!(
        missing.is_empty(),
        "base leveling must force EVERY footswitch-owned block off — a block left at its saved \
         ON state makes Base the same sound as that switch's own row. Never forced: {missing:?} \
         (forced: {forced_off:?})"
    );
    // ...and none was forced back ON: an isolation that re-engages a block is not isolation.
    let forced_on: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            crate::sim_device::SimEvent::Bypass { node, on: false }
                if onoff.iter().any(|(_, n)| n == node) =>
            {
                Some(node)
            }
            _ => None,
        })
        .collect();
    assert!(
        forced_on.is_empty(),
        "the base isolation may only force footswitch-owned blocks OFF: {forced_on:?}"
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
/// The shipped discriminator (2026-08-19) is simpler than either scene-reading gate this test
/// used to pin: `plan_footswitch_jobs`'s assign gate decides bake-vs-assign purely off whether
/// the switch ALREADY carries a `param` fn for the selected (node, param) — none of this
/// fixture's four switches do, so all four bake regardless of what any scene overlay does.
/// Mirroring is still VALUE-based (`scenes_restating_base`): this fixture's device-authored
/// overlays carry the full param set for every node in every scene, so a bare "does the key
/// appear" check would mirror into every scene including the ones that authored their own
/// value. Pure planner test (no device): `plan_footswitch_jobs` is the whole decision.
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
            crate::SceneOverlay::Full(p) | crate::SceneOverlay::BypassOnly(p) => {
                p.get(*param).and_then(serde_json::Value::as_f64)
            }
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

/// How many `Loaded` events the sim has recorded so far — the barrier tests' shared probe for
/// "has `ensure_fresh_load_paced` issued another `LoadPreset`".
fn loaded_count(sim: &crate::sim_device::SimDevice) -> usize {
    sim.events()
        .iter()
        .filter(|e| matches!(e, crate::sim_device::SimEvent::Loaded(_)))
        .count()
}

/// [`loaded_count`] taken relative to an earlier reading — the retry count a barrier call has
/// issued since `baseline`.
fn loads_since(sim: &crate::sim_device::SimDevice, baseline: usize) -> usize {
    loaded_count(sim) - baseline
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

/// Scene-witness first-harvest gate (Fix 3 + Fix 2): a scene deferred save, driven through
/// the sim exactly like `save_deferred_scene_writes` would, must be visible to the VERY
/// FIRST harvest — no stale retry — because (a) the witness now carries a scene
/// discriminator and consults that scene's overlay, and (b) the sim's lazy-commit doc now
/// actually persists a scene-scoped write into the rendered TEXT the harvest reads (Fix 2;
/// without it the harvest keeps seeing the pre-save overlay value forever). Slot 402/scene 0
/// is Full-shaped for `ACD_JC120` in the committed fixture (module doc,
/// `scene_jobs::SceneOverlay::Full`), so the write lands on the overlay with no
/// `setNodeSceneEdit` needed (post-review amendment 8).
#[test]
fn fresh_load_barrier_scene_witness_passes_on_first_harvest() {
    let _serial = serial();
    let _reset = RegistryReset;
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
    let sim = install_barrier_sim(0); // commits immediately
    const SLOT: u32 = 402;
    const SCENE: u32 = 0;
    const GROUP: &str = "G1";
    const NODE: &str = "ACD_JC120";
    const PARAM: &str = "outputLevel";
    const VALUE: f32 = 0.77;
    {
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(SLOT).expect("load 402");
        s.load_scene(SCENE).expect("recall scene 0");
        s.change_parameter(GROUP, NODE, PARAM, VALUE)
            .expect("write outputLevel");
        s.save_current_preset(SLOT).expect("save");
    }
    let baseline = loaded_count(&sim);
    crate::leveller::register_slot_save(
        SLOT,
        crate::leveller::SaveWitness::Param {
            node: NODE.to_string(),
            param: PARAM.to_string(),
            value: VALUE,
            scene: Some(SCENE),
        },
    );
    // Cancel bound (not `&mut || false`): the RED form of this gate must never blind-wait
    // the full 150 s commit window — it terminates via the cancel hook right after a SECOND
    // load is observed, which only a still-spinning (unmatched) witness would ever issue.
    let sim_for_cancel = sim.clone();
    let start = std::time::Instant::now();
    let result = crate::leveller::ensure_fresh_load_paced(
        SLOT,
        &mut || loads_since(&sim_for_cancel, baseline) >= 2,
        200,
    );
    let loads = loads_since(&sim, baseline);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        loads, 1,
        "a matching scene witness must pass on the FIRST harvest — no stale retry load"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "first-harvest pass must not have blind-waited, took {:?}",
        start.elapsed()
    );
}

/// Anti-stampede gate (post-review amendment 1): an UNHARVESTABLE witness (a `Param` naming a
/// node no doc the sim renders ever carries) backdated to ~1 s BEFORE the commit-window edge —
/// not past it, unlike the time-gate test above — so the barrier must genuinely retry a
/// handful of times (the elapsed-since-save crosses `COMMIT_WINDOW_SECS` only after real
/// wall-clock time passes) before the time-gate finally fires. Wall-clock pacing
/// (`ensure_fresh_load_paced`'s inner wait loop) bounds that retrying to roughly
/// `elapsed / retry_wait_ms` loads; the pre-fix busy-spin — the sim's `pump` returns instantly,
/// so an unpaced inner loop burns through the same ~1 s wall-clock budget in hundreds of
/// iterations — does not.
#[test]
fn fresh_load_barrier_paces_retries_near_the_commit_window_edge() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(600_000); // the pending save never commits
    save_level_401(&sim, 0.9);
    let baseline = loaded_count(&sim);
    crate::leveller::register_slot_save_at(
        401,
        crate::leveller::SaveWitness::Param {
            node: "no-such-node".into(),
            param: "outputLevel".into(),
            value: 0.9,
            scene: None,
        },
        std::time::Instant::now()
            - std::time::Duration::from_secs(crate::leveller::COMMIT_WINDOW_SECS - 1),
    );
    let start = std::time::Instant::now();
    // Not the 50 ms-sleeping closure this used to pace itself with: that sleep was a test-side
    // busy-spin damper from before `ensure_fresh_load_paced`'s own wait loop measured wall-clock
    // time (Fix 6's `HidTransport::pump` doc). Production pacing now owns the retry cadence on
    // its own, and dropping the sleep here makes the gate decisive — the pre-fix (unpaced) red
    // form burns hundreds of loads in the same wall-clock budget, while the paced form still
    // stays near the ~5 the assertion below expects.
    let result = crate::leveller::ensure_fresh_load_paced(401, &mut || false, 200);
    assert!(result.is_ok(), "{result:?}");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "the time-gate must still fire near the window edge, took {:?}",
        start.elapsed()
    );
    let loads = loads_since(&sim, baseline);
    assert!(
        loads <= 10,
        "wall-clock-paced retries near the window edge must issue at most ~elapsed/cadence \
         LoadPreset events (paced, ~200 ms cadence over ~1 s), not a busy-spin hundreds — got \
         {loads}"
    );
}

/// Block DISCOVERY is a same-slot device load like any other, and the Level wizard's Base
/// handle picker (`list_level_blocks`) fires it seconds after a run's own save. Ungated, its
/// load materializes the PRE-save doc inside the commit window — the preset-24 corruption
/// class (`danger.md`). The barrier belongs at the shared load seam, so this drives
/// `load_then_discover_blocks` directly rather than the Tauri command wrapper.
#[test]
fn load_then_discover_blocks_gates_on_a_pending_same_slot_save() {
    let _serial = serial();
    let _reset = RegistryReset;
    let sim = install_barrier_sim(1_500);
    save_level_401(&sim, 0.81);
    crate::leveller::register_slot_save(401, crate::leveller::SaveWitness::PresetLevel(0.81));
    let _ = crate::load_then_discover_blocks(401);
    // The discovery load must have landed on the COMMITTED doc. `0.32` (the fixture's
    // pre-save level) means the load raced the commit and re-materialized stale bytes —
    // the load whose lazy commit silently reverts the save that preceded it.
    assert!(
        (sim.preset_level() - 0.81).abs() < 1e-3,
        "block discovery materialized the PRE-save doc inside the commit window \
         (preset-24 class), got {}",
        sim.preset_level()
    );
}

/// GATE for the stale-`presetLevel` capture incident (HW, 2026-08-19, "Plumes+BD2+OCD").
///
/// The barrier above is the FIRST line of defence, and it is not enough. Every capture's
/// `recall_base` re-runs the device's OWN level-apply, which serves the COMMITTED
/// `presetLevel` — and that store commits lazily (T+45-100 s, `danger.md`). A preset with no
/// scenes has its base save and its footswitch batch seconds apart, squarely inside that
/// window, so the whole batch measured a chain **5.53 dB** quieter than the one the user had
/// just leveled: base saved 0.51009 and verified -23.0002, while the batch read switch 5's
/// ceiling at -24.44 for a state that truly measures -18.91 — exactly
/// `20·log10(0.51009 / 0.26999998)` against the file's ORIGINAL level. Every row then failed
/// `fs_target_beyond_ceiling` and clamped a target it was comfortably within.
///
/// So a capture must not DEPEND on the barrier having waited (two of its four exits are
/// silent, so which one a run took is not even recoverable from the log): it re-asserts the
/// preset's own saved level itself. The sim models the device's revert faithfully — a recall
/// restores `committed_doc(slot).preset_level` once the slot has been saved this run — so
/// this reproduces the incident offline, without hardware.
#[test]
fn a_capture_renders_at_the_saved_preset_level_not_the_stale_committed_one() {
    let _serial = serial();
    let _reset = RegistryReset;
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
    // 600 s: the base save NEVER commits during the test, i.e. the whole run happens inside
    // the commit window — the worst case, and the one the incident hit.
    let sim = install_barrier_sim(600_000);
    crate::sim_device::set_live(&sim);
    let stim = test_stim();

    let spec_json = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec_json
        .iter()
        .find(|p| p.list_index == 400)
        .expect("400 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("400 json");
    let ftsw = preset["ftsw"].clone();

    const SWITCH: u32 = 2; // the Boost switch, as in the Bake gate above
    const NODE: &str = "ACD_Boost";
    const PARAM: &str = "gain";
    // The level base leveling just solved and saved — pending, not yet committed.
    const SAVED_PL: f32 = 0.51;
    {
        let mut s = crate::session::Session::from_transport(Box::new(sim.clone()));
        s.load_preset(400).expect("load 400");
        s.set_preset_level(SAVED_PL).expect("set");
        s.save_current_preset(400).expect("save");
    }

    let states = crate::footswitch::switch_states(&ftsw, &preset, SWITCH);
    let authored = crate::commands::level_footswitch::node_param_f64(&preset, NODE, PARAM)
        .expect("ACD_Boost.gain") as f32;
    let probe = crate::leveller::FsCeilingProbe {
        scene: None,
        states: &states,
        handle: (
            "G1".to_string(),
            NODE.to_string(),
            crate::leveller::FsParamTarget::new(NODE, PARAM, authored),
        ),
    };

    // THE PRE-FIX SHAPE — assert nothing and let the recall's level-apply decide. It serves
    // the pre-save committed level, which is the entire defect.
    let stale = crate::leveller::measure_fs_ceiling(&probe, &stim, None).expect("stale ceiling");
    let stale_pl = sim.preset_level();
    assert!(
        (stale_pl - SAVED_PL).abs() > 0.05,
        "PREMISE: inside the commit window the recall must serve the PRE-save level, not the \
         saved {SAVED_PL} — got {stale_pl}. Without this the test proves nothing."
    );

    // THE SHIPPED SHAPE — the run re-asserts its own saved level on the capture.
    let fresh =
        crate::leveller::measure_fs_ceiling(&probe, &stim, Some(SAVED_PL)).expect("fresh ceiling");
    assert!(
        (sim.preset_level() - SAVED_PL).abs() < 1e-3,
        "the capture must render at the SAVED level {SAVED_PL}, got {}",
        sim.preset_level()
    );
    assert!(
        sim.events().iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::PresetLevel(v) if (v - SAVED_PL).abs() < 1e-3
        )),
        "the capture must SEND setPresetLevel — the recall is what reverted it, so nothing \
         upstream can be trusted to have left the right value in place"
    );

    // And the reading actually moves by the level difference: this is the ceiling error that
    // made every row of the user's batch clamp.
    let expected = 20.0 * (f64::from(SAVED_PL) / f64::from(stale_pl)).log10();
    let got = fresh.integrated_lufs - stale.integrated_lufs;
    assert!(
        (got - expected).abs() < 0.5,
        "the stale capture must be off by exactly the level ratio: expected {expected:.2} dB, \
         got {got:.2} dB ({:.2} → {:.2} LUFS)",
        stale.integrated_lufs,
        fresh.integrated_lufs
    );
}

/// D3 gate: a scene row given the USER'S OWN handle is solved by the generic param secant
/// (`solve_param_secant`) instead of the amp joint-k, and still lands on target through the
/// Scene-Edit-aware write path. The handle here is the Hiwatt's `outputLevel` in scene 0 —
/// the one control the offline capture model responds to — so the assertion is about the
/// SEAM, not the knob: joint-k's closed-form solve is bypassed entirely (the row carries a
/// `handle`, so `level_scenes_oneshot` dispatches to `handle_one_scene`), and the search has
/// to find the value from the param's own range with no `20·log10(k)` shortcut.
#[test]
fn a_user_chosen_scene_handle_is_solved_by_the_param_secant_and_reaches_target() {
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

    const AMP: &str = "ACD_HiwattDR103CanMod";
    // Scene 0's own authored value for the handle (its overlay carries 0.96).
    const AUTHORED: f32 = 0.96;
    let saved = crate::read_saved_preset(404);
    // The runner's caller contract: the preset is already current.
    {
        let mut s = crate::session::Session::connect_lean().expect("connect");
        s.load_preset(404).expect("load 404");
    }
    let handle = crate::leveller::FsParamTarget::new(AMP, "outputLevel", AUTHORED);
    let (lo, hi) = handle.bounds();
    let job = crate::leveller::SceneJob {
        scene_slot: 0,
        target_lufs: -23.0,
        knobs: vec![crate::leveller::KnobTarget {
            knob: crate::leveller::LevelKnob::Block {
                group_id: "G1".into(),
                node_id: AMP.into(),
                parameter_id: "outputLevel".into(),
                scene_slot: Some(0),
            },
            lo,
            hi,
            current: AUTHORED,
        }],
        skip: None,
        rebalanceable: false,
        handle: Some(handle),
        // Legacy order: the solve takes its own as-is capture (no reordered prepass here).
        prepass: None,
        // This test's job isn't a base job — nothing to isolate.
        force_bypass: vec![],
    };
    let outcomes = crate::leveller::level_scenes_oneshot(
        404,
        &[job],
        &stim,
        false,
        None,
        saved.as_ref(),
        // No headroom trade in this fixture run.
        None,
        // Nothing was isolated for this job, so nothing to undo.
        &[],
        |_, _| {},
        |_| {}, // B6: no progress channel in this fixture
        || false,
    )
    .expect("handle run");

    let o = &outcomes[0];
    assert!(o.failure.is_none(), "the handle row must solve: {o:?}");
    assert!(!o.clamped, "the handle reaches target: {o:?}");
    let achieved = o.final_lufs.expect("a solved row reports its loudness");
    assert!(
        (achieved - (-23.0)).abs() < 0.5,
        "handle solve should land on target, got {achieved:.2} ({o:?})"
    );
    let solved = o.final_level.expect("a solved row reports its value");
    assert!(
        solved > lo && solved < AUTHORED,
        "the solve had to come DOWN from the authored {AUTHORED} and stay in range, got {solved}"
    );
    // The seam actually ran: joint-k is ONE write (its solve is closed-form), the param
    // secant needs several real captures to find the same point.
    assert!(
        o.writes > 2,
        "a searched solve writes once per capture plus the final apply: {o:?}"
    );
}

// ───────────────── P5 external validation: identity + flags on the emitted rows ─────────
//
// The bug class this closes: the first design zipped a scene batch's RESULT vec against
// its REQUEST array by position to label each row. `level_scenes_apply_batched` FILTERS
// failed scenes out of that vec (`commands/level_scenes.rs`), so a mid-batch failure
// either shifted every later label onto the wrong scene or — with a length guard — dropped
// the ENTIRE batch's expectations silently, which the shell consumer reads as "nothing was
// leveled" and reports as a pass. Both are false greens.
//
// So this gate asserts the two halves of the fix against the offline physics:
//   (a) a batch WITH a mid-batch failure returns FEWER rows than it was sent, and every
//       surviving row NAMES ITS OWN scene (`scene_slot`) — no position is involved;
//   (b) the re-measure seam emits one validation row per sound, carrying that identity plus
//       the run's `clamped`/`persist_mismatch` verdicts, so the consumer can SKIP a clamped
//       row instead of failing it against a target it could never reach.
//
// Slot 403 (`E2E Parallel`) is the fixture: 4 scenes, of which the amp-at-zero "Clean" scene
// is the OFF-BRANCH clamp (`level_defaults_403_scenes_solve_and_offbranch` pins that shape).
// Scene 1 is given a HANDLE naming a param the classifier refuses, which is the documented
// per-row skip path (`scene_jobs::handle_scene_job` → `skip_scene_job` → a failed outcome
// the command filters out) — a genuine mid-batch failure driven purely from wire args.
#[test]
fn a_mid_batch_failure_keeps_every_surviving_scenes_identity_and_emits_its_row() {
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
        .invoke_handler(tauri::generate_handler![
            level_scenes_apply_batched,
            super::e2e_measure_sound
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");

    let amp = serde_json::json!([
        {"groupId": "G2", "nodeId": "ampA", "parameterId": "outputLevel", "value": 1.0},
        {"groupId": "G3", "nodeId": "ampB", "parameterId": "outputLevel", "value": 1.0}
    ]);
    // Scene 1 names a control the param classifier refuses (an amp's drive-side knob is
    // never a loudness handle) — that row skips, the other three level.
    let jobs = serde_json::json!([
        {"sceneSlot": 0, "targetLufs": -23.0},
        {"sceneSlot": 1, "targetLufs": -23.0,
         "handle": {"groupId": "G2", "nodeId": "ampA", "parameterId": "drive"}},
        {"sceneSlot": 2, "targetLufs": -23.0},
        {"sceneSlot": 3, "targetLufs": -23.0}
    ]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": 403, "jobs": jobs, "candidates": amp,
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    let rows = res.as_array().expect("results array").clone();

    // (a) SHORTER than the request, and self-naming. If the fixture ever stops refusing
    // that handle the length assert fails loudly rather than the test passing vacuously.
    assert_eq!(
        rows.len(),
        3,
        "the refused-handle scene must be filtered out, the other 3 must survive: {rows:?}"
    );
    let mut got: Vec<u64> = rows
        .iter()
        .map(|r| {
            r["scene_slot"]
                .as_u64()
                .unwrap_or_else(|| panic!("every scene row carries its own scene_slot: {r:?}"))
        })
        .collect();
    got.sort_unstable();
    assert_eq!(
        got,
        vec![0, 2, 3],
        "identity survives the filter — NOT a positional 0,1,2: {rows:?}"
    );

    // (b) Each survivor's re-measure emits one row keyed by that same identity. The log is
    // env-armed; emission ALSO needs a `validate` payload, so no other test can be affected
    // by this var even though env is process-global (and `serial()` is held throughout).
    let dir = std::env::temp_dir().join("tmp-companion-p5-midbatch");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let log = dir.join("expect.jsonl");
    std::env::set_var("TMP_E2E_VALIDATE_LOG", &log);
    std::env::set_var("TMP_E2E_VALIDATE_WAV_DIR", dir.join("wavs"));
    for r in &rows {
        let scene = r["scene_slot"].as_u64().expect("scene_slot");
        let clamped = r["clamped"].as_bool().unwrap_or(false);
        invoke(
            &webview,
            "e2e_measure_sound",
            serde_json::json!({
                "slot": 403,
                "scene": scene,
                "footswitch": null,
                "topologyId": "guitar-humbucker",
                "lev": null,
                "validate": {
                    "targetLufs": r["target_lufs"],
                    "clamped": clamped,
                    "persistMismatch": r["persist_mismatch"],
                },
            }),
        )
        .unwrap_or_else(|e| panic!("e2e_measure_sound scene {scene}: {e:?}"));
    }
    std::env::remove_var("TMP_E2E_VALIDATE_LOG");
    std::env::remove_var("TMP_E2E_VALIDATE_WAV_DIR");

    let body = std::fs::read_to_string(&log).expect("validation log written");
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3, "one row per surviving scene: {body}");
    for scene in [0u64, 2, 3] {
        assert!(
            body.contains(&format!("\"scene_slot\":{scene}")),
            "scene {scene} emitted its own row: {body}"
        );
        assert!(
            body.contains(&format!("\"label\":\"scene:slot403:scene{scene}\"")),
            "scene {scene}'s label is self-describing: {body}"
        );
    }
    assert!(
        !body.contains("\"scene_slot\":1"),
        "the FAILED scene must emit no row at all — there is nothing saved to validate: {body}"
    );
    // The off-branch "Clean" scene clamps, and its row MUST still be emitted with the flag
    // set: the consumer reports it SKIP, never a target miss against an unreachable number.
    let clamped_rows = rows
        .iter()
        .filter(|r| r["clamped"] == serde_json::Value::Bool(true))
        .count();
    assert_eq!(
        clamped_rows, 1,
        "fixture premise: exactly one scene is the amp-at-zero off-branch clamp: {rows:?}"
    );
    assert_eq!(
        body.matches("\"clamped\":true").count(),
        1,
        "the clamped row is EMITTED with clamped:true (SKIP downstream), not dropped: {body}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────── The benefit-aware headroom trade, through the real batched command ─────────
//
// Every clause of the arming rule (`plan_trade_for_batch`) needs its own piece of fixture: a
// SAVE run, a base row leveled by the amp FADER (no user handle), and a scene that CLAMPS and
// BENEFITS. Slot 404's scene 3 ("Base Scene") is that scene — a FULL overlay pins its
// `outputLevel` independently of base, so the compensating fader drop misses it, and the
// authored C table puts it 14 LU below base, far under any shipped target.
//
// WHY 404 SCENE 3 AND NOT 400 SCENE 2 (the obvious "Ceiling" candidate). The fake serves ONE
// graph for every scene recall, so the live prepass reads every scene's knob as BASE's value —
// which is what `classify_scene_knobs` takes as the row's `current`, hence what
// `scene_ceiling_lufs` extrapolates from. On 400 that reads scene 2's knob as 0.35 when the
// preset authors it at 1.0, inventing 9 dB of headroom the sound does not have, and nothing
// ever clamps. 404's scene 3 authors the SAME `outputLevel` as base (0.69), so the plan and the
// capture model agree about that row and its clamp is real.

/// Reset the SCENE LANE'S CANCEL FLAG when the test ends, INCLUDING on a panicking assert.
/// It is process-global and cleared only at a scene command's own entry, so a leaked `true`
/// stops the next serial test that levels a scene before its first solve.
struct SceneCancelReset;
impl Drop for SceneCancelReset {
    fn drop(&mut self) {
        crate::commands::level_scenes::SCENE_LEVEL_CANCEL
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The physics env (fixtures + the authored C table + the backup blob + the stimulus) plus a
/// live sim wired as the transport factory. Returns the fake so the caller can read its event
/// log and its `presetLevel`. Also returns the event index the RUN starts at — the seed below
/// saves, and a `Saved` count has to be able to exclude it.
fn trade_sim() -> (crate::sim_device::SimDevice, usize) {
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
    crate::leveller::clear_slot_save_registry();
    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));
    // MAKE THE FAKE'S TWO PRESET-LEVEL TRUTHS AGREE, which the trade needs and no other gate
    // does. A fresh sim's LIVE `presetLevel` is 1.0 until the slot has been saved this run
    // (`SimState::ever_saved` gates the load-time restore), while the FIELD-8 document the
    // planner reads its headroom from is the fixture's authored 0.6. Left alone the planner
    // sees 4.4 dB of room the capture model does not have and trades against a level the
    // physics is not at. One save round trip settles both on 0.6 — the state a real unit is
    // always in, since a load there restores the saved level.
    {
        let mut s = crate::session::Session::connect_lean().expect("connect");
        s.load_preset(TRADE_SLOT).expect("load the trade fixture");
        s.set_preset_level(TRADE_PRESET_LEVEL)
            .expect("seed presetLevel");
        s.save_current_preset(TRADE_SLOT).expect("seed save");
    }
    let from = sim.events().len();
    (sim, from)
}

/// The trade fixture: slot 404 (E2E Hiwatt 3S), its authored `audioGraph.presetLevel`, its one
/// guitar amp at that amp's authored base `outputLevel` (the fader the trade pays with), and
/// the scene whose FULL overlay authors the SAME `outputLevel` as base — the row whose clamp
/// the offline capture model and the planner agree about (see the section header).
const TRADE_SLOT: u32 = 404;
const TRADE_PRESET_LEVEL: f32 = 0.6;
const TRADE_AMP: &str = "ACD_HiwattDR103CanMod";
const TRADE_BASE_FADER: f32 = 0.69;
const TRADE_SCENE: u64 = 3;

fn trade_amp_candidates() -> serde_json::Value {
    serde_json::json!([{
        "groupId": "G1", "nodeId": TRADE_AMP,
        "parameterId": "outputLevel", "value": TRADE_BASE_FADER
    }])
}

/// One `level_scenes_apply_batched` app + webview, built the way the offline UI drives it. The
/// `App` is returned only to be held: dropping it takes the webview with it.
fn batched_scene_app() -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_scenes_apply_batched])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    (app, webview)
}

/// Every `SceneLevelProgressItem` a run streamed over `onResult`, in the order sent.
type CapturedChannel = std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

/// `batched_scene_app`'s sibling for a test that needs the whole PROGRESS-ITEM SEQUENCE, not
/// just the command's return value — e.g. "no item ever named slot 8" or "some `done` item
/// carried the trade" (F5). The mock runtime's own eval-based `Channel` only remembers the
/// LAST script it evaluated (`MockWebviewDispatcher::last_evaluated_script`), so a sequence
/// assertion needs a `channel_interceptor` instead: it fires on every message a
/// `tauri::ipc::Channel` sends, in order, before the (here, inert) mock eval — exactly the
/// hook `JavaScriptChannelId::channel_on` documents.
fn batched_scene_app_capturing_channel() -> (
    tauri::App<MockRuntime>,
    WebviewWindow<MockRuntime>,
    CapturedChannel,
) {
    let captured: CapturedChannel = Default::default();
    let sink = captured.clone();
    let app = tauri::test::mock_builder()
        .channel_interceptor(move |_webview, _callback_fn, _index, body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
                    sink.lock().unwrap().push(v);
                }
            }
            // Consumed — there is no JS engine behind the mock webview to hand it to anyway.
            true
        })
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![level_scenes_apply_batched])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("wv");
    (app, webview, captured)
}

/// The row a batched scene result names. Base rides the wire as a NULL `scene_slot` — the
/// `BASE_SCENE_SLOT` sentinel is not a `scenes[]` index (`outcome_to_level_result`).
fn scene_row(rows: &[serde_json::Value], scene: Option<u64>) -> Option<&serde_json::Value> {
    rows.iter().find(|r| match scene {
        Some(s) => r["scene_slot"].as_u64() == Some(s),
        None => r["scene_slot"].is_null(),
    })
}

/// ⟦F1⟧ THE TRADE, LANDED AND PERSISTED, through the real batched command. At the shipped −21
/// default 404's scene 3 is 11 LU short with its own knob already at the top, and it BENEFITS
/// (a Full overlay pins that knob independently of base) — the one shape that justifies
/// churning the base pair, so the run raises base `presetLevel` and solves the base amp fader
/// back down to leave base exactly where it was asked for. `presetLevel` is the smaller of the
/// two rooms here, so the raise is TRIMMED to the 4.44 dB left above 0.6 and says so; the row
/// it was bought for is 4.44 LU louder for it and still, honestly, clamped.
///
/// THE PAIR IS ONE EDIT AND HAS TO PERSIST AS ONE (`apply_headroom_trade`'s atomicity note):
/// half of it saved leaves the preset uniformly loud or uniformly quiet, and `danger.md` says
/// a save cannot be undone from the app. So both halves are read back out of the SAVED
/// document after the batch's ONE save, against the summary the run itself reported — that
/// summary is the UI's disclosure AND its restore anchor, so one that disagrees with the
/// device is worse than none.
///
/// TWO ASSERTIONS TOGETHER SEE THE REGRESSION THIS CLOSES, and neither does alone. The hold's
/// fader writes SEED the runner's `written` list; base's own row is left AT target by the hold
/// (`scene_at_target`), so the base slot gets a post-save verdict at all only because the
/// trade's writes are in that list — drop them and `persist_mismatch` reads `None`, the re-read
/// confirming the scenes while saying nothing about the pair that moved every one of them. That
/// the verdict is about the TRADE's value rather than some later base solve is what the saved
/// fader equalling `trade.base_amps[0].value` pins down.
#[test]
fn a_batched_scene_run_persists_both_halves_of_a_landed_headroom_trade() {
    let _serial = serial();
    let _reset = RegistryReset;
    let _cancel_reset = SceneCancelReset;
    let (sim, from) = trade_sim();
    let (_app, webview) = batched_scene_app();

    // Base (wire slot 8) + the deep-quiet benefiting scene, both at the shipped −21 default.
    let jobs = serde_json::json!([
        {"sceneSlot": 8, "targetLufs": -21.0},
        {"sceneSlot": TRADE_SCENE, "targetLufs": -21.0}
    ]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": TRADE_SLOT, "jobs": jobs, "candidates": trade_amp_candidates(),
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    let rows = res.as_array().expect("results array").clone();
    assert_eq!(
        rows.len(),
        2,
        "base + the clamped scene both level: {rows:?}"
    );

    // The trade rides EVERY row: it moved the whole preset's gain structure, not one row's.
    for r in &rows {
        assert_eq!(
            r["trade"]["applied"],
            serde_json::json!(true),
            "an applied (not advisory) trade is disclosed on every row: {r}"
        );
    }
    let trade = rows[0]["trade"].clone();
    let raise_db = trade["raise_db"].as_f64().expect("raise_db");
    let previous_pl = trade["previous_preset_level"].as_f64().expect("previous") as f32;
    let raised_pl = trade["preset_level"].as_f64().expect("preset_level") as f32;
    assert!(
        (raise_db - 4.437).abs() < 0.1,
        "the raise is all the dB `presetLevel` had left above 0.6, not the 11 the clamp \
         wanted: {trade}"
    );
    assert_eq!(
        trade["cap"], "preset_level_max",
        "…and the BINDING cap is named — the fader had 36 dB, presetLevel had 4.4: {trade}"
    );
    assert_eq!(
        trade["benefiting"],
        serde_json::json!([{ "kind": "scene", "sceneSlot": TRADE_SCENE }]),
        "the raise was bought for the Full-overlay scene, by identity: {trade}"
    );

    // HALF ONE — presetLevel really rose, on the device, by exactly the reported raise.
    assert!(
        (previous_pl - TRADE_PRESET_LEVEL).abs() < 1e-3,
        "fixture premise: the trade slot is authored at presetLevel {TRADE_PRESET_LEVEL}, got \
         {previous_pl}"
    );
    assert!(
        (crate::headroom_trade::raised_preset_level(previous_pl, raise_db) - raised_pl).abs()
            < 1e-4,
        "the summary's raised level is the exact linear solution: {trade}"
    );
    assert!(
        (sim.preset_level() - raised_pl).abs() < 1e-3,
        "the unit holds the raised presetLevel, got {}: {trade}",
        sim.preset_level()
    );

    // HALF TWO — the base fader PAID for it, and base is still exactly where it was asked for.
    let amp = trade["base_amps"][0].clone();
    let fader = amp["value"]
        .as_f64()
        .expect("a landed trade SOLVED the fader") as f32;
    let previous_fader = amp["previous_value"].as_f64().expect("previous_value") as f32;
    assert!(
        fader < previous_fader,
        "the compensating fader went DOWN from {previous_fader}, got {fader}"
    );
    let base = scene_row(&rows, None)
        .unwrap_or_else(|| panic!("a base row: {rows:?}"))
        .clone();
    let base_lufs = base["measured_lufs"].as_f64().expect("base lufs");
    assert!(
        (base_lufs + 21.0).abs() < 0.5,
        "base is HELD at its target — the whole point of paying with the fader: {base}"
    );
    // The sound the raise was bought for is `raise_db` louder than its own pre-trade ceiling —
    // and STILL clamped, reported honestly with its own cause. A capped trade buys real
    // loudness without pretending it closed the gap.
    const PRE_TRADE_CEILING: f64 = -32.21; // C −31 at presetLevel 0.6, knob 0.69 → 1.0
    let scene = scene_row(&rows, Some(TRADE_SCENE))
        .unwrap_or_else(|| panic!("a scene {TRADE_SCENE} row: {rows:?}"))
        .clone();
    let scene_lufs = scene["measured_lufs"].as_f64().expect("scene lufs");
    assert!(
        (scene_lufs - (PRE_TRADE_CEILING + raise_db)).abs() < 0.5,
        "the benefiting row gains EXACTLY the raise (presetLevel is exactly linear in dB): \
         {scene}"
    );
    assert_eq!(
        scene["clamped"],
        serde_json::json!(true),
        "…and it is still short of −21, which the row must keep saying: {scene}"
    );
    assert_eq!(
        scene["clamp_kind"], "scene_ceiling",
        "the trade LANDED, so the clamp is the ordinary headroom one — not `trade_floor` or \
         `partial_trade`, which describe the base pair's own fate: {scene}"
    );

    // BOTH HALVES AT THE ONE SAVE. One `saveCurrentPreset`, and the re-read finds the pair.
    assert_eq!(
        sim.events()[from..]
            .iter()
            .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(s) if *s == TRADE_SLOT))
            .count(),
        1,
        "exactly one deferred save persists the batch: {:?}",
        &sim.events()[from..]
    );
    let saved = crate::read_saved_preset(TRADE_SLOT).expect("the saved preset re-reads");
    let saved_pl = crate::audiograph::preset_level(&saved).expect("saved presetLevel") as f32;
    assert!(
        (saved_pl - raised_pl).abs() < 1e-3,
        "the SAVED document carries the raised presetLevel, got {saved_pl}"
    );
    let saved_fader =
        crate::commands::level_footswitch::node_param_f64(&saved, TRADE_AMP, "outputLevel")
            .expect("saved base outputLevel") as f32;
    assert!(
        (saved_fader - fader).abs() < 1e-3,
        "…and the solved base fader alongside it, got {saved_fader} vs the reported {fader}"
    );
    assert_eq!(
        base["persist_mismatch"],
        serde_json::json!(false),
        "the trade's own writes are in the run's verified set — base is left at target by the \
         hold, so this reads `None` the moment they are dropped from it: {base}"
    );
}

/// Fire the scene lane's STOP the moment the sim records a `changeParameter` on `scene`'s own
/// `outputLevel` overlay — a wire event that happens exactly ONCE in the run (the prepass
/// writes nothing, and both the base row and the trade's hold write under the base sentinel),
/// so the cancel lands at one reproducible point with no timer and no watcher thread.
///
/// It stores the LANE flag directly instead of calling `cancel_scene_leveling`, which would
/// also raise `device_gate::OP_ABORT` and kill the capture already in flight. The lane flag is
/// read only at `run_scene_jobs`' loop top, so the scene under way FINISHES and the stop lands
/// on the NEXT job — precisely the "cancel after the trade landed, mid-batch" shape.
struct CancelAtSceneWrite {
    sim: crate::sim_device::SimDevice,
    scene: i64,
    fired: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CancelAtSceneWrite {
    fn check(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        if self.fired.load(SeqCst) {
            return;
        }
        let hit = self.sim.events().iter().any(|e| {
            matches!(e, crate::sim_device::SimEvent::ChangeParameter { scene, param, .. }
                if *scene == self.scene && param == "outputLevel")
        });
        if hit {
            self.fired.store(true, SeqCst);
            crate::commands::level_scenes::SCENE_LEVEL_CANCEL.store(true, SeqCst);
        }
    }
}

impl crate::hid::HidTransport for CancelAtSceneWrite {
    fn send(&self, body: &[u8]) -> Result<(), String> {
        let r = crate::hid::HidTransport::send(&self.sim, body);
        self.check();
        r
    }
    fn transact(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let r = crate::hid::HidTransport::transact(&self.sim, body, pump_ms);
        self.check();
        r
    }
    fn transact_chunked(&self, body: &[u8], pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let r = crate::hid::HidTransport::transact_chunked(&self.sim, body, pump_ms);
        self.check();
        r
    }
    fn pump(&self, pump_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        crate::hid::HidTransport::pump(&self.sim, pump_ms)
    }
    fn transact_eager(&self, body: &[u8], max_ms: u64) -> Result<Vec<Vec<u8>>, String> {
        let r = crate::hid::HidTransport::transact_eager(&self.sim, body, max_ms);
        self.check();
        r
    }
}

/// ⟦F2⟧ CANCEL AFTER A LANDED TRADE RETURNS ITS OUTCOMES AND DISCLOSES THE TRADE — it does not
/// come back as the bare `CANCELLED` sentinel, whose results the command maps to an EMPTY vec.
/// That historical shape is the silent-wrong-numbers outcome this codebase refuses: the save
/// on the stopped path has already persisted the raised `presetLevel` + the base fader holding
/// base on target, every scene already solved was solved AT that raised level, and handing
/// back nothing would leave the preset carrying a gain-structure change the UI never mentioned
/// and cannot offer to restore (danger.md — see ⟦F1⟧'s doc).
///
/// JOB ORDER IS LOAD-BEARING, and so is which row triggers — and A1/A4 (`base-LAST` sort,
/// "base's isolated prepass capture must run LAST in PHASE 1") changed what that order IS:
/// `scene_jobs` is sorted so BASE always runs LAST, whatever position it held on the wire, and
/// PHASE 3 shares that same order. So base is now the batch's own reliable "never runs" job —
/// the trigger must fire on the SECOND job instead of the first, or the stop lands before even
/// ONE row completes. Slot 404's scene 1 ("Rhythm") is, like scene 3, a Full-overlay
/// beneficiary of this same trade (its own authored `outputLevel` also clamps and gets raised —
/// see `TRADE_SCENE`'s doc for the fixture's physics), so it is the trigger: scene 3 (still
/// first among the wire's non-base jobs) completes untouched, scene 1's own write stops the
/// run, and base — sorted last — never starts. It has to land at a LOOP TOP with at least one
/// row already attempted: land it earlier and the runner takes its pre-solve exit, which
/// reloads the preset and discards the untraded-for pair, exactly as the back-out does.
#[test]
fn a_cancel_after_a_landed_trade_returns_its_outcomes_with_the_trade_disclosed() {
    let _serial = serial();
    let _reset = RegistryReset;
    let _cancel_reset = SceneCancelReset;
    let (sim, from) = trade_sim();
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (sf, ff) = (sim.clone(), fired.clone());
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(CancelAtSceneWrite {
                sim: sf.clone(),
                // The SECOND job to run post-sort (base-last) — see the doc above.
                scene: 1,
                fired: ff.clone(),
            })
        }));
    }
    let (_app, webview) = batched_scene_app();

    let jobs = serde_json::json!([
        {"sceneSlot": 8, "targetLufs": -21.0},
        {"sceneSlot": TRADE_SCENE, "targetLufs": -21.0},
        {"sceneSlot": 1, "targetLufs": -21.0}
    ]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": TRADE_SLOT, "jobs": jobs, "candidates": trade_amp_candidates(),
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("a cancelled run that traded must return Ok, not an error");
    let rows = res.as_array().expect("results array").clone();

    assert!(
        fired.load(std::sync::atomic::Ordering::SeqCst),
        "the gate is vacuous unless the stop actually fired: {rows:?}"
    );
    assert_eq!(
        rows.len(),
        2,
        "the two non-base scenes that finished come back; the stop landed on BASE's loop top \
         (base sorts last post-A1/A4) — the historical shape returned an EMPTY vec here: \
         {rows:?}"
    );
    assert!(
        scene_row(&rows, None).is_none(),
        "base — sorted last — is the unstarted job and emits no row: {rows:?}"
    );
    assert!(
        scene_row(&rows, Some(1)).is_some() && scene_row(&rows, Some(TRADE_SCENE)).is_some(),
        "and the two that ran name themselves: {rows:?}"
    );

    // THE DISCLOSURE — on every returned row, with the pre-trade values a Restore needs.
    for r in &rows {
        assert_eq!(
            r["trade"]["applied"],
            serde_json::json!(true),
            "a cancelled run that PERSISTED a trade must say so: {r}"
        );
        assert!(
            r["trade"]["raise_db"].as_f64().expect("raise_db") > 0.0,
            "…with the raise it actually made: {r}"
        );
        assert!(
            r["trade"]["base_amps"][0]["previous_value"].is_number()
                && r["trade"]["previous_preset_level"].is_number(),
            "…and both restore anchors: {r}"
        );
    }

    // PERSISTED, not backed out: one save, and the unit holds the raised pair.
    assert_eq!(
        sim.events()[from..]
            .iter()
            .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(s) if *s == TRADE_SLOT))
            .count(),
        1,
        "the stopped path still fires the batch's ONE save: {:?}",
        &sim.events()[from..]
    );
    let raised = rows[0]["trade"]["preset_level"].as_f64().expect("level") as f32;
    assert!(
        (sim.preset_level() - raised).abs() < 1e-3,
        "the raised pair stands — backing it out would leave every solved scene off target by \
         the raise while the run reported it on: got {}",
        sim.preset_level()
    );
}

// ───────────── A1/A9: the "base anchor" — the trade fires on a WIZARD-SHAPED run ─────────
//
// The wizard never sends a wire job for base itself (that lane is `level_preset`'s, run
// separately) — so `plan_trade_for_batch`'s gate #2 ("a base row in `scene_jobs`") would
// otherwise never see one on a real wizard run, and the trade above would stay reachable
// only from a hand-built `base_requested` batch no UI path produces. `base_anchor` (A1) is
// the fix: it keeps a base job alive through PHASE 1/2 with no wire target of its own, then
// strips it before PHASE 3. This section proves the anchor reaches the SAME trade the
// `base_requested` shape does, and that its isolation (A3/A4 — "base means base") never
// leaks into the saved preset.

/// ⟦A9(i)⟧ THE ANCHOR REACHES THE SAME TRADE A `base_requested` BATCH DOES — same fixture
/// (404/scene 3), same raise, but the wizard's OWN shape: ONE wire job (the clamped scene)
/// plus a bare `baseAnchor`, no slot-8 job at all. If the anchor's target didn't get the
/// same offset a real wire job's does (A1's doc), or if PHASE 3 failed to strip the
/// anchor-only base job, this would either trade against the wrong number or double-solve
/// base on top of `level_preset`'s own apply — both regressions this pins.
#[test]
fn a_wizard_shaped_run_trades_headroom_when_base_arrives_as_an_anchor() {
    let _serial = serial();
    let _reset = RegistryReset;
    let _cancel_reset = SceneCancelReset;
    let (sim, from) = trade_sim();
    let (_app, webview, captured) = batched_scene_app_capturing_channel();

    // The wizard's shape: no slot-8 job — base rides ONLY the anchor.
    let jobs = serde_json::json!([
        {"sceneSlot": TRADE_SCENE, "targetLufs": -21.0}
    ]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": TRADE_SLOT, "jobs": jobs, "candidates": trade_amp_candidates(),
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "baseAnchor": {"targetLufs": -21.0},
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    let rows = res.as_array().expect("results array").clone();

    // Base's own `outputLevel` is never solved by THIS batch when it arrived only as an
    // anchor — that write belongs to the wizard's separate `level_preset` lane.
    assert_eq!(
        rows.len(),
        1,
        "only the wire job (the clamped scene) comes back — no anchor-only base row: {rows:?}"
    );
    let trade = rows[0]["trade"].clone();
    assert_eq!(
        trade["applied"],
        serde_json::json!(true),
        "an anchored base still lands an applied (not advisory) trade: {trade}"
    );
    let raise_db = trade["raise_db"].as_f64().expect("raise_db");
    assert!(
        (raise_db - 4.437).abs() < 0.1,
        "the anchor must drive the IDENTICAL trade math as a base_requested batch (same \
         fixture, same targets): {trade}"
    );
    let raised_pl = trade["preset_level"].as_f64().expect("preset_level") as f32;
    let fader = trade["base_amps"][0]["value"]
        .as_f64()
        .expect("a landed trade solved the fader") as f32;

    // BOTH HALVES persist even though base's own row never rode this batch's PHASE 3.
    assert_eq!(
        sim.events()[from..]
            .iter()
            .filter(|e| matches!(e, crate::sim_device::SimEvent::Saved(s) if *s == TRADE_SLOT))
            .count(),
        1,
        "one deferred save persists the anchor-driven pair: {:?}",
        &sim.events()[from..]
    );
    let saved = crate::read_saved_preset(TRADE_SLOT).expect("the saved preset re-reads");
    let saved_pl = crate::audiograph::preset_level(&saved).expect("saved presetLevel") as f32;
    assert!(
        (saved_pl - raised_pl).abs() < 1e-3,
        "the SAVED document carries the raised presetLevel from an anchor-only trade, got \
         {saved_pl}"
    );
    let saved_fader =
        crate::commands::level_footswitch::node_param_f64(&saved, TRADE_AMP, "outputLevel")
            .expect("saved base outputLevel") as f32;
    assert!(
        (saved_fader - fader).abs() < 1e-3,
        "…and the lowered base fader alongside it, got {saved_fader} vs the reported {fader}"
    );

    // Scene 3's own saved `outputLevel` sits at its solved value (its Full overlay pins the
    // knob independently of base, which is why it benefited from the raise at all).
    let solved = rows[0]["final_level"].as_f64().expect("final_level");
    let scene_saved_output = match crate::scene_overlay(&saved, TRADE_SCENE as u32, TRADE_AMP) {
        crate::SceneOverlay::Full(params) => params
            .get("outputLevel")
            .and_then(serde_json::Value::as_f64)
            .expect("scene 3's Full overlay carries its own outputLevel"),
        _ => panic!("scene 3 must keep a Full overlay for its own outputLevel after the batch"),
    };
    assert!(
        (scene_saved_output - solved).abs() < 1e-3,
        "scene 3's SAVED outputLevel must equal the row's own solved value: saved \
         {scene_saved_output} vs solved {solved}"
    );

    // NO progress item ever named slot 8 — the anchor-only base has no frontend row to
    // route one to (A1's suppression at both the prepass `started` and the PHASE-3
    // `on_scene` send sites — moot for the latter since the job is stripped before PHASE 3,
    // but the prepass DOES run a base capture and must stay silent about it).
    let items = captured.lock().unwrap();
    assert!(
        !items.is_empty(),
        "the channel-interceptor capture must actually see the run's progress items, or this \
         gate is vacuous"
    );
    assert!(
        !items
            .iter()
            .any(|it| it["sceneSlot"].as_u64() == Some(u64::from(session::BASE_SCENE_SLOT))),
        "an anchor-only base must stream NO progress item at all: {items:?}"
    );
    // F5: at least one `done` item discloses the trade over the channel, not just the
    // command's awaited return — the wizard's row-by-row UI reads the channel, not the
    // return value.
    assert!(
        items
            .iter()
            .any(|it| it["status"] == "done" && !it["result"]["trade"].is_null()),
        "at least one done item must carry the trade summary: {items:?}"
    );
}

/// ⟦A9(ii)⟧ THE ANCHOR'S BASE CAPTURE IS ISOLATED, AND THE ISOLATION NEVER PERSISTS. Slot 400
/// ("E2E Rig") saves its "DRIVE" footswitch's own `ACD_TubeScreamer` bypass=false — exactly
/// the shape `base_leveling_forces_every_footswitch_owned_block_off_not_the_preset_as_saved`
/// pins for `level_preset`'s own base lane ("base means base": preset 28's TubeScreamer was
/// saved ON too). A1/A3/A4 give the BATCH command the identical isolation whenever base is in
/// plan, anchor or not — this is the anchor half of that gate.
///
/// LUFS can't prove it: the offline capture model's C table carries no bypass term (same
/// SCOPE HONESTY as the `level_preset` gate above), so this pins the two device-visible
/// facts instead — the isolation WROTE (`changeParameter` on `bypass`, modeled as
/// `SimEvent::Bypass` because the wire message differs from a float `ChangeParameter`), and
/// by the batch's one save the working copy is clean again: the SAVED preset still reads
/// bypass=false, exactly what the user's preset already had.
#[test]
fn a_base_anchor_measures_the_isolated_base_and_never_persists_the_isolation() {
    let _serial = serial();
    let _reset = RegistryReset;
    let _cancel_reset = SceneCancelReset;
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
    crate::leveller::clear_slot_save_registry();
    const RIG: u32 = 400;
    const TUBE_SCREAMER: &str = "ACD_TubeScreamer";

    let sim = crate::sim_device::SimDevice::new();
    crate::sim_device::set_live(&sim);
    let sf = sim.clone();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sf.clone())));

    // Fixture premise: the pedal really is saved ON, or "isolation never leaked" would pass
    // vacuously with no isolation ever needed.
    let saved_before = crate::read_saved_preset(RIG).expect("400 field-8 read");
    assert!(
        !crate::footswitch::block_bypassed_in_base(&saved_before, TUBE_SCREAMER),
        "fixture premise: 400's DRIVE pedal (TubeScreamer) must be saved ON (bypass=false)"
    );

    let from = sim.events().len();
    let (_app, webview) = batched_scene_app();
    let amp = serde_json::json!([{
        "groupId": "G1", "nodeId": "ACD_JC120",
        "parameterId": "outputLevel", "value": 0.35
    }]);
    let jobs = serde_json::json!([{"sceneSlot": 0, "targetLufs": -23.0}]);
    let res = invoke(
        &webview,
        "level_scenes_apply_batched",
        serde_json::json!({
            "slot": RIG, "jobs": jobs, "candidates": amp,
            "save": true, "rebalance": false,
            "topologyId": serde_json::Value::Null, "calibrationLufs": null, "profileId": null,
            "baseAnchor": {"targetLufs": -23.0},
            "onResult": "__CHANNEL__:0"
        }),
    )
    .expect("level_scenes_apply_batched");
    assert!(
        res.as_array().is_some_and(|a| !a.is_empty()),
        "the scene job must actually solve: {res:?}"
    );

    // THE ISOLATED CAPTURE: the pedal was forced off during the run.
    let events = sim.events()[from..].to_vec();
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::sim_device::SimEvent::Bypass { node, on: true } if node == TUBE_SCREAMER
        )),
        "the anchor's base capture must isolate the footswitch-owned pedal exactly like \
         `level_preset`'s own base lane: {events:?}"
    );

    // …AND NEVER PERSISTED: the saved preset still reads the pedal ON, exactly as the user
    // left it — the working copy was cleaned before the batch's one save.
    let saved_after = crate::read_saved_preset(RIG).expect("400 field-8 re-read");
    assert!(
        !crate::footswitch::block_bypassed_in_base(&saved_after, TUBE_SCREAMER),
        "the isolation must NEVER reach the saved preset — the pedal must still read ON"
    );
}

/// ⟦F3⟧ A FOOTSWITCH ROW'S SCENE CONTEXT IS A MEASUREMENT ORDER, not a label. `measure_fs_state`
/// recalls the context FIRST, then writes the switch's engaged state and the pinned handle,
/// and only then engages: re-amp latches preset state AT ENGAGE (`danger.md`), so a recall or
/// a write that goes out after it is simply not in the capture. `scene_context: None` recalls
/// BASE for the same reason — a preset loads into its saved `lastLoadedScene`, so "no scene"
/// still costs an explicit recall or the sound measured is whatever the connection held.
///
/// Slot 400's Boost switch is the fixture and its `gain` is the handle. The two contexts are
/// discriminated TWICE over: by the event order, and by the reading itself — the authored C
/// table puts scene 1 ("Lead") 3 LU above base, so a run that ordered the recall correctly but
/// measured the wrong sound still fails. `gain` is not a `leveledParams` entry on this slot, so
/// the capture model is flat in the handle and the two readings differ by the context alone.
#[test]
fn an_fs_scene_context_recalls_its_scene_before_the_engage_and_base_without_one() {
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

    let spec = crate::probe_api::seed_scenario::scenario_spec().expect("scenario spec");
    let rig = spec
        .iter()
        .find(|p| p.list_index == 400)
        .expect("400 present");
    let preset: serde_json::Value = serde_json::from_str(&rig.preset_json).expect("400 json");
    let ftsw = preset["ftsw"].clone();

    const SWITCH: u32 = 2; // the Boost switch
    const NODE: &str = "ACD_Boost";
    const PARAM: &str = "gain";
    const SCENE: u32 = 1; // "Lead" — 3 LU louder than base in the authored C table
    let states = crate::footswitch::switch_states(&ftsw, &preset, SWITCH);
    let handle = crate::leveller::FsParamTarget::new(NODE, PARAM, 2.5);
    let (_, hi) = handle.bounds();

    // Each probe is read on its own load — a load discards the previous one's edit buffer, so
    // neither reading can inherit the other's writes.
    let read_in = |scene: Option<u32>| -> (Vec<crate::sim_device::SimEvent>, f64) {
        {
            let mut s = crate::session::Session::connect_lean().expect("connect");
            s.load_preset(400).expect("load 400");
        }
        let from = sim.events().len();
        let probe = crate::leveller::FsCeilingProbe {
            scene,
            states: &states,
            handle: ("G1".to_string(), NODE.to_string(), handle.clone()),
        };
        let l = crate::leveller::measure_fs_ceiling(&probe, &stim, None).expect("ceiling read");
        (sim.events()[from..].to_vec(), l.integrated_lufs)
    };
    let position = |events: &[crate::sim_device::SimEvent],
                    what: &str,
                    pred: &dyn Fn(&crate::sim_device::SimEvent) -> bool|
     -> usize {
        events
            .iter()
            .position(pred)
            .unwrap_or_else(|| panic!("no {what} in {events:?}"))
    };
    let engage =
        |e: &crate::sim_device::SimEvent| matches!(e, crate::sim_device::SimEvent::ReAmp(true));

    let (scene_events, scene_lufs) = read_in(Some(SCENE));
    let recall = position(
        &scene_events,
        "scene recall",
        &|e| matches!(e, crate::sim_device::SimEvent::LoadScene(s) if *s == SCENE),
    );
    let write = position(&scene_events, "pinned-handle write", &|e| {
        matches!(e, crate::sim_device::SimEvent::ChangeParameter { node, param, value, .. }
            if node == NODE && param == PARAM && (*value - hi).abs() < 1e-6)
    });
    let engaged = position(&scene_events, "re-amp engage", &engage);
    assert!(
        recall < write && write < engaged,
        "recall → write → engage, in that order (re-amp latches at engage): {scene_events:?}"
    );

    let (base_events, base_lufs) = read_in(None);
    let base_recall = position(&base_events, "base recall", &|e| {
        matches!(e, crate::sim_device::SimEvent::LoadScene(s)
            if *s == crate::session::BASE_SCENE_SLOT)
    });
    let base_engaged = position(&base_events, "re-amp engage", &engage);
    assert!(
        base_recall < base_engaged,
        "no scene context still RECALLS BASE before engaging — a bare capture would measure \
         the preset's saved lastLoadedScene: {base_events:?}"
    );
    assert!(
        !base_events
            .iter()
            .any(|e| matches!(e, crate::sim_device::SimEvent::LoadScene(s) if *s == SCENE)),
        "…and never touches the scene: {base_events:?}"
    );

    // The captures really were taken in the two different contexts, not merely ordered right.
    assert!(
        (scene_lufs - base_lufs - 3.0).abs() < 0.3,
        "scene 1 sits 3 LU above base in the authored C table — got {scene_lufs:.2} vs \
         {base_lufs:.2}"
    );
}
