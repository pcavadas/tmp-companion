//! Probe entry points: footswitch-scene leveling (list / bake-validate / level / repro / forced measure).

use super::ftsw::read_slot_ftsw;
use super::stimulus::read_stimulus_48k;
use super::stimulus::read_stimulus_calibrated;
use crate::audio;
use crate::footswitch;
use crate::leveller;
use crate::session::Session;
use crate::{node_param_f64, read_slot_preset_parsed, resolve_footswitch_job, FootswitchLevelJob};

/// Probe entry: isolate the in-process CoreAudio → chunked-HID failure. Sends a chunked
/// `set_footswitch_assignment` (1) BEFORE any audio, (2) after ONE re-amp CoreAudio capture,
/// (3) after a SECOND capture — reporting the device's reply fields each time ([54] = landed,
/// [] = dropped). Tells us whether one capture is enough to break chunked sends, or if it
/// accumulates. Targets slot 23 / FS6 (the BD2 preset); restores after each set.
pub fn probe_repro_chunked() -> Result<String, String> {
    let slot = 23u32;
    let switch = 6u32;
    let json = r#"{"func":"param","groupId":"G1","nodeId":"ACD_BluesDriver","parameterId":"gain","valueA":0.5,"valueB":0.35,"valueType":2,"colorA":3,"colorB":0,"customLabel":"REPRO","switchType":0,"isActive":true,"linkGroup":0}"#;
    let mut out = String::from("[probe --repro-chunked]\n");

    let try_set = |label: &str, out: &mut String| {
        let r = (|| -> Result<Vec<u32>, String> {
            let mut s = Session::connect()?;
            s.begin_live_edit()?;
            s.load_preset(slot)?;
            // Pump heartbeats (NOT a passive sleep) to keep the session live up to the set.
            for _ in 0..8 {
                let _ = s.heartbeat();
                let _ = s.pump_collect(150);
            }
            s.set_footswitch_assignment(switch, 1, json, false, None)?;
            let seen = s.seen_preset_fields();
            let _ = s.clear_footswitch_assignment(switch, 1);
            let _ = s.save_current_preset(slot);
            Ok(seen)
        })();
        match r {
            Ok(seen) => out.push_str(&format!(
                "  [{label}] chunked set → device fields {seen:?}  ({})\n",
                if seen.contains(&54) {
                    "LANDED"
                } else {
                    "DROPPED"
                }
            )),
            Err(e) => out.push_str(&format!("  [{label}] error: {e}\n")),
        }
    };

    let capture_once = || -> Result<(), String> {
        let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
            .map_err(|_| "set TMP_LEVELLER_STIMULUS".to_string())?;
        let stim = read_stimulus_48k(&stim_path)?;
        {
            let mut s = Session::connect()?;
            s.load_preset(slot)?;
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        let mut s = Session::connect()?;
        s.set_reamp_mode(true)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        Ok(())
    };

    try_set("A: before any audio", &mut out);
    out.push_str("  … one re-amp CoreAudio capture …\n");
    capture_once()?;
    try_set("B: after 1 capture", &mut out);
    out.push_str("  … second re-amp CoreAudio capture …\n");
    capture_once()?;
    try_set("C: after 2 captures", &mut out);
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));
    Ok(out)
}

/// Probe entry: clear one footswitch function (restore/cleanup after a `--level-footswitch
/// --commit`). Loads `slot`, clears `(switch, index)`, saves, and field-8 verifies.
pub fn probe_clear_footswitch(slot: u32, switch: u32, index: u32) -> Result<String, String> {
    let count_at = |f: &Option<serde_json::Value>| -> usize {
        f.as_ref()
            .and_then(|f| f.as_array())
            .and_then(|a| a.get(switch as usize))
            .and_then(|sw| sw.as_array())
            .map(|fns| fns.len())
            .unwrap_or(usize::MAX)
    };
    let before = count_at(&read_slot_ftsw(slot + 1));
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    let name = s.active_preset_name().unwrap_or_default();
    if !name.is_empty() && !s.await_active_preset(&name, 20) {
        return Err("after load, active preset changed — aborting".into());
    }
    // Keep the session live with a heartbeat burst right up to the edit (a passive sleep
    // lets the live-controller status lapse and the device silently drops the edit).
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    s.clear_footswitch_assignment(switch, index)?;
    if s.saw_preset_error() {
        return Err("device rejected clear (presetError)".into());
    }
    s.save_current_preset(slot)?;
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));
    let count = read_slot_ftsw(slot + 1)
        .and_then(|f| {
            f.as_array()
                .and_then(|a| a.get(switch as usize))
                .and_then(|sw| sw.as_array())
                .map(|fns| fns.len())
        })
        .unwrap_or(usize::MAX);
    Ok(format!(
        "[probe --clear-ftsw] slot {} FS{switch} index {index}: before clear {before} function(s) → cleared + saved → now {count} function(s)\n",
        slot + 1
    ))
}

/// Probe (self-restoring): commit a BAKE on `(switch, group, node, param)`, verify the value
/// landed on the block (bypass unchanged, no param fn added), then RESTORE the original value.
/// Mirrors `--ftsw-validate --commit`'s commit-then-restore. Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_bake_validate(
    slot: u32,
    switch: u32,
    group: &str,
    node: &str,
    param: &str,
    target_lufs: f64,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    // One read → the node's value, base bypass, switch fn count, engaged force-list, and
    // the saved `lastLoadedScene` (the commit's save must re-stamp it).
    type Snap = (
        f64,
        bool,
        usize,
        Vec<(String, String, bool)>,
        Option<u32>,
        leveller::FsParamTarget,
    );
    let snapshot = || -> Result<Snap, String> {
        let (p, _, _) = read_slot_preset_parsed(slot)?;
        let ftsw = p.get("ftsw").cloned().unwrap_or(serde_json::Value::Null);
        let v = node_param_f64(&p, node, param).ok_or("param not found after read")?;
        let fns = ftsw
            .as_array()
            .and_then(|a| a.get(switch as usize)?.as_array().map(Vec::len))
            .unwrap_or(usize::MAX);
        let engaged = footswitch::engaged_bypass_for_switch(&ftsw, &p, switch);
        let restore = crate::last_loaded_scene(&p);
        // The classified solve target, off this SAME read — no extra field-8 round trip.
        let lev_param = leveller::FsParamTarget::from_preset(&p, node, param);
        Ok((
            v,
            footswitch::block_bypassed_in_base(&p, node),
            fns,
            engaged,
            restore,
            lev_param,
        ))
    };

    let (orig, byp0, fns0, engaged, restore, lev_param) = snapshot()?;
    let mut out = format!(
        "[probe --bake-validate] slot {} · FS{switch} · {group}/{node}.{param}\n  before: value={orig:.4} bypass={byp0} switch_fns={fns0}\n",
        slot + 1
    );
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    // Commit the bake (engaged-measured, value written onto the block).
    let r = leveller::level_footswitch(
        slot,
        switch,
        (group, node, param),
        &engaged,
        &leveller::FsWrite::Bake {
            clear_stale: None,
            // Diagnostic arm: validate the raw base bake — no scene mirroring.
            mirror_scenes: vec![],
        },
        &stim,
        target_lufs,
        true,
        false,
        restore,
        &lev_param,
    )?;
    out += &format!(
        "  baked: method={} value={:.4}{}\n",
        r.method,
        r.final_value,
        if r.clamped { " [clamped]" } else { "" }
    );

    // Verify field-8: the value landed, bypass unchanged, NO param fn added.
    let (after, byp1, fns1, _, _, _) = snapshot()?;
    let landed = (after - r.final_value as f64).abs() < 1e-3;
    out += &format!(
        "  after : value={after:.4} bypass={byp1} switch_fns={fns1}  ⇒  {}\n",
        if landed && byp1 == byp0 && fns1 == fns0 {
            "PASS (value baked, bypass intact, no fn added)"
        } else {
            "FAIL"
        }
    );

    // Restore the original value (change_parameter + save on a heartbeat-live session).
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    {
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        s.load_preset(slot)?;
        for _ in 0..8 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(150);
        }
        s.change_parameter(group, node, param, orig as f32)?;
        s.save_current_preset(slot)?;
    }
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));
    let (restored, _, _, _, _, _) = snapshot()?;
    out += &format!(
        "  restore: value={restored:.4}  ⇒  {}\n",
        if (restored - orig).abs() < 1e-3 {
            "RESTORED"
        } else {
            "RESTORE MISMATCH (recover from unit backup)"
        }
    );
    Ok(out)
}

/// `probe --fs-sweep <listIdx> <switch> <group> <node> <param> <v1,v2,…>` — the response
/// curve the footswitch solver actually feeds on. Unlike `--knob-sweep` (which forces NO
/// bypass, so a base-bypassed pedal stays inert and every point reads the base sound), this
/// takes the isolation list from `<switch>`'s own plan — the target switch's block(s)
/// engaged, every sibling block-switch forced off — exactly the `engaged_bypass` the
/// production `measure_footswitch` sweeps under. `<node>.<param>` is arbitrary, so the same
/// arm also measures an AMP knob under a switch's engaged state (e.g. the amp `outputLevel`
/// with a drive pedal on).
///
/// Diagnostic only: no writes are persisted (the final reload discards the working-copy
/// sweep), and it ends with the guaranteed re-amp OFF. Stimulus via `TMP_LEVELLER_STIMULUS`
/// (injected verbatim, like a calibrated profile's DI capture).
pub fn probe_fs_sweep(
    list_index: u32,
    switch: u32,
    group: &str,
    node: &str,
    param: &str,
    values: &[f32],
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    let (preset, _, _) = read_slot_preset_parsed(list_index)?;
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    // The isolation list comes from the switch's own plan, so the sweep sits in exactly the
    // engaged state `measure_footswitch` measures — never a hand-built approximation.
    let infos = footswitch::enumerate_block_footswitches(&ftsw, &preset);
    let info = infos
        .iter()
        .find(|i| i.switch == switch)
        .ok_or_else(|| format!("switch {switch} is not block-acting in this preset"))?;
    let lp = info
        .level_params
        .first()
        .ok_or("switch has no level-param candidate")?;
    let plan = footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[footswitch::FsJobKey {
            switch,
            lev_node: &lp.node_id,
            lev_param: &lp.parameter_id,
            target_bits: (-23.0f64).to_bits(),
        }],
    )
    .into_iter()
    .next()
    .ok_or("planner returned no plan")?;
    let engaged = match &plan {
        footswitch::FsLevelPlan::Bake { engaged, .. }
        | footswitch::FsLevelPlan::Assign { engaged } => engaged.clone(),
        other => {
            return Err(format!(
                "switch {switch} plans as {other:?}, no isolation list"
            ))
        }
    };

    {
        let mut s = Session::connect_lean()?;
        s.load_preset(list_index)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let mut out = format!(
        "[probe --fs-sweep] list_index={list_index} FS{switch} {group}/{node}.{param}\n  isolation: {engaged:?}\n"
    );
    for v in values {
        // A silent point is DATA here (the knob's bottom end), not a failure — record it
        // and keep sweeping instead of aborting the whole curve like the solver does.
        // Probe sweep of a saved preset: no run-owned `presetLevel` to assert.
        match leveller::measure_fs_at(None, (group, node, param), &engaged, &stim, *v, None) {
            Ok(l) => {
                out += &format!(
                    "  {param}={v:.3} → integrated {:.3} LUFS  short-term-max {:.3}  spread {:.2} LU\n",
                    l.integrated_lufs,
                    l.short_term_max_lufs,
                    l.spread_lu()
                );
            }
            Err(e) => out += &format!("  {param}={v:.3} → ERR {e}\n"),
        }
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    }
    if let Ok(mut s) = Session::connect_lean() {
        let _ = s.load_preset(list_index);
    }
    leveller::reamp_off_guaranteed("fs-sweep");
    Ok(out)
}

/// `probe --amp-recipe <listIdx> <group> <ampNode> <gain> <outputLevel>` — measure a candidate
/// AMP operating point across a preset's whole footswitch set in ONE pass: writes the two amp
/// knobs, then captures base (every block-acting switch forced OFF) and each switch engaged in
/// turn, under the production force-list (`doctor_force_bypass` — the off-in-base/
/// switch-enables-it composition; a PARAM-only footswitch has no on-off flip, so its isolation
/// collapses to base and its gap row is structurally ≈0, not evidence about that switch).
///
/// This exists because the base↔boost GAP is a property of the amp's operating point, not of any
/// one switch — a per-knob sweep can't show it. Diagnostic only: nothing is saved (the final
/// reload discards the working-copy writes) and it ends with the guaranteed re-amp OFF.
/// Stimulus via `TMP_LEVELLER_STIMULUS` (injected verbatim, like a calibrated profile's capture).
pub fn probe_amp_recipe(
    list_index: u32,
    group: &str,
    amp_node: &str,
    gain: f32,
    output_level: f32,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    let (preset, _, _) = read_slot_preset_parsed(list_index)?;
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let infos = footswitch::enumerate_block_footswitches(&ftsw, &preset);
    if infos.is_empty() {
        return Err("preset has no block-acting footswitches".to_string());
    }
    // Base = every block-acting switch's ON-OFF node forced OFF — `doctor_force_bypass(None)`,
    // the one cross-module owner of this force-list (same definition the Base leveling job
    // uses: "all footswitches off", NOT as-saved).
    let base_off = crate::commands::doctor::doctor_force_bypass(&ftsw, &preset, None);

    {
        let mut s = Session::connect_lean()?;
        s.load_preset(list_index)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let measure = |engaged: &[(String, String, bool)]| -> Result<f64, String> {
        let mut s = Session::connect_lean()?;
        // Recall base FIRST: a load activates the saved `lastLoadedScene`, and the recall also
        // re-asserts base's own bypass state — wiping the previous iteration's forced writes so
        // nothing leaks between measurements. Every write below lands after it.
        s.load_scene(crate::session::BASE_SCENE_SLOT)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_SCENE_RECALL_MS,
        ));
        s.change_parameter(group, amp_node, "gain", gain)?;
        s.change_parameter(group, amp_node, "outputLevel", output_level)?;
        for (g, n, byp) in engaged {
            s.change_parameter_bool(g, n, "bypass", *byp)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_SET_MS,
        ));
        Ok(leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs)
    };

    let mut out = format!(
        "[probe --amp-recipe] idx {list_index} · {group}/{amp_node}  gain={gain} outputLevel={output_level}\n"
    );
    let base = measure(&base_off);
    match &base {
        Ok(l) => out += &format!("  base (all switches off)      {l:8.2} LUFS\n"),
        Err(e) => out += &format!("  base (all switches off)      ERR {e}\n"),
    }
    for info in &infos {
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        // The production composition (siblings forced off PLUS this switch's own engaged
        // flip) via its one cross-module owner — never the flip alone, which would leave
        // earlier iterations' pedals audible in this switch's capture.
        let engaged =
            crate::commands::doctor::doctor_force_bypass(&ftsw, &preset, Some(info.switch));
        let label = info
            .functions
            .first()
            .map(|f| f.node_id.clone())
            .unwrap_or_default();
        match measure(&engaged) {
            Ok(l) => {
                let gap = base.as_ref().map(|b| l - b).unwrap_or(f64::NAN);
                out += &format!(
                    "  FS{} {label:24} {l:8.2} LUFS   gap {gap:+.2} LU\n",
                    info.switch
                );
            }
            Err(e) => out += &format!("  FS{} {label:24} ERR {e}\n", info.switch),
        }
    }
    if let Ok(mut s) = Session::connect_lean() {
        let _ = s.load_preset(list_index);
    }
    leveller::reamp_off_guaranteed("amp-recipe");
    Ok(out)
}

/// Probe (read-only): list a slot's block-acting footswitches with each acted-on block's base
/// bypass + the bake/assign classification for its first level param — to find bake-eligible
/// presets (an active on-off enabling an OFF-in-base block). `has_fs_scenes` is printed as a raw
/// read DIAGNOSTIC only — the bake/assign gate is per-node (a scene overlay CHANGING that
/// block's `bypass` OR its leveled param vs base), so a `has_fs_scenes=true` preset can
/// still classify as Bake.
pub fn probe_fs_list(slot: u32) -> Result<String, String> {
    let (preset, has_fs_scenes, json_len) = read_slot_preset_parsed(slot)?;
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let infos = footswitch::enumerate_block_footswitches(&ftsw, &preset);
    let mut out = format!(
        "[probe --fs-list] slot {} · {} block-footswitch(es) · has_fs_scenes={has_fs_scenes} ({json_len}B)\n",
        slot + 1,
        infos.len()
    );
    for fi in &infos {
        for f in &fi.functions {
            let byp = footswitch::block_bypassed_in_base(&preset, &f.node_id);
            out += &format!(
                "  FS{} {:7} {}/{}  base_bypass={byp}\n",
                fi.switch, f.func, f.group_id, f.node_id
            );
        }
        if let Some(lp) = fi.level_params.first() {
            let plan = footswitch::plan_footswitch_jobs(
                &ftsw,
                &preset,
                &[footswitch::FsJobKey {
                    switch: fi.switch,
                    lev_node: &lp.node_id,
                    lev_param: &lp.parameter_id,
                    target_bits: (-23.0f64).to_bits(),
                }],
            );
            out += &format!(
                "      → level {}.{}  ⇒  {:?}\n",
                lp.node_id, lp.parameter_id, plan[0]
            );
        }
    }
    Ok(out)
}

/// Probe GO/NO-GO spike: prove the device honors a LIVE `change_parameter_bool(bypass=false)`.
/// Measures `(group,node)`'s contribution with the block left as-is vs forced active. If the
/// block is OFF in base, the base capture is the preset WITHOUT it and the forced capture is
/// WITH it, so a meaningful loudness delta proves the live bypass write took effect (the bake
/// path depends on this). Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_measure_forced(slot: u32, group: &str, node: &str) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::settle_after_load_ms(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let measure = |force: Option<bool>| -> Result<f64, String> {
        let mut s = Session::connect()?;
        if let Some(byp) = force {
            s.change_parameter_bool(group, node, "bypass", byp)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_SET_MS,
        ));
        Ok(leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs)
    };

    let base = measure(None);
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let off = measure(Some(true)); // force bypassed
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let on = measure(Some(false)); // force active
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));

    let row = |label: &str, v: &Result<f64, String>| match v {
        Ok(l) => format!("  {label}: {l:.2} LUFS\n"),
        Err(e) => format!("  {label}: ERROR {e}\n"),
    };
    let mut out = format!(
        "[probe --measure-forced] slot {} · {group}/{node}\n",
        slot + 1
    );
    out += &row("base (as-is)       ", &base);
    out += &row("forced bypass=true ", &off);
    out += &row("forced bypass=false", &on);
    if let (Ok(off), Ok(on)) = (&off, &on) {
        // The two forced states differ by the block's whole contribution → the live bypass write
        // is honored. Whichever matches base reveals the base state.
        out += &format!(
            "  on−off = {:+.2} LU  ⇒  live bypass write {}\n",
            on - off,
            if (on - off).abs() > 0.5 {
                "HONORED (go)"
            } else {
                "NO EFFECT (no-go)"
            }
        );
        if let Ok(b) = &base {
            let base_state = if (b - on).abs() < (b - off).abs() {
                "ON in base"
            } else {
                "OFF in base"
            };
            out += &format!("  base matches forced-{base_state}\n");
        }
    }
    Ok(out)
}

/// READ-ONLY re-measure of ONE footswitch's ENGAGED sound — the footswitch twin of
/// `probe --measure-scene`, and the flag that closes P5's external-validation hole (a
/// footswitch row used to be leveled with no independent capture path, so it could only
/// be reported as "not externally verified").
///
/// Composes EXISTING primitives, adding no engage/disengage sequencing of its own:
/// * `commands::doctor::doctor_force_bypass` over the SAVED doc — the ONE shared
///   isolation derivation the leveling, Doctor and strict-harness lanes use (siblings
///   off + this switch's own engaged flip, isActive-aware);
/// * `footswitch::existing_param_fn_value_a` — an ASSIGN switch's engaged sound is its
///   leveled param at the saved `valueA`; a BAKED switch (or one with no `param`
///   function on `lev`) needs no write, its engaged sound IS the base value;
/// * `leveller::measure_sound_asis_strict` — the same floor-guarded production capture
///   path (fresh load → base recall → isolation → ONE engage → guaranteed re-amp OFF)
///   that `e2e_measure_sound` drives online, including its `--dump-wav`-shaped
///   external-validation add-on.
///
/// `lev` is `Some((group, node, param))` for an ASSIGN switch — the same triple the
/// `--level-footswitch` run used. `dump_dir`/`target_lufs` arm the validation row (a
/// WAV plus one line in `TMP_E2E_VALIDATE_LOG`) exactly like `--measure-scene`'s dump.
/// Read-only throughout: every write lands on a throwaway connection's working copy and
/// nothing is ever saved.
pub fn probe_measure_footswitch(
    slot: u32,
    switch: u32,
    topology_id: &str,
    lev: Option<(&str, &str, &str)>,
    target_lufs: Option<f64>,
    dump_dir: Option<&str>,
) -> Result<String, String> {
    let stim = read_stimulus_calibrated(&super::stimulus::probe_stimulus_path(topology_id)?, None)?;
    let saved = crate::read_saved_preset(slot)
        .ok_or_else(|| format!("field-8 read failed for slot {slot}"))?;
    let force = crate::commands::doctor::doctor_force_bypass(&saved["ftsw"], &saved, Some(switch));
    let fs_value = lev.and_then(|(g, n, p)| {
        footswitch::existing_param_fn_value_a(&saved["ftsw"], switch, n, p)
            .map(|v| ((g.to_string(), n.to_string(), p.to_string()), v as f32))
    });
    // `--dump-wav <dir>` routes through the SAME validation-log add-on the online lane
    // uses, so `scripts/level-validate.sh` consumes one row shape from both callers. The
    // row needs a promised target; without one there is nothing to validate against, so
    // the dump is simply not armed (the measurement still prints).
    let row = match (dump_dir, target_lufs) {
        (Some(dir), Some(target)) => Some(
            crate::validate_log::ValidationRow::footswitch(slot, switch, target).with_wav_dir(dir),
        ),
        _ => None,
    };
    let result =
        leveller::measure_sound_asis_strict(slot, None, &force, fs_value, &stim, row.as_ref());
    // Run-end backstop, success or failure (see `reamp_off_guaranteed`: the device DROPS
    // an in-session OFF sent on a session that has idled >~1 s, and this lane's capture
    // idles ~7 s — so `engage_capture_disengage`'s in-session disengage cannot be trusted
    // as the last word). A standalone `probe --measure-footswitch` would otherwise be able
    // to leave the unit input-muted. Bound BEFORE the `?` for exactly that reason: an Err
    // path is the one that most needs the OFF. Same shape as `probe --levelpreset`
    // (`probe_api/level.rs`) — no added gap, so the two probe arms behave identically.
    leveller::reamp_off_guaranteed("probe --measure-footswitch");
    let loud = result?;
    Ok(format!(
        "slot={slot} switch={switch} topology={topology_id} lev={lev:?} \
         integrated_lufs={:.3} short_term_max_lufs={:.3} spread_lu={:.3} isolation_blocks={}\n",
        loud.integrated_lufs,
        loud.short_term_max_lufs,
        loud.spread_lu(),
        force.len(),
    ))
}

/// Probe entry: level one footswitch on the active/`slot` preset for HW re-validation.
/// DRY by default (measure + solve, no write); `commit` writes `valueA` + saves.
/// Stimulus via `TMP_LEVELLER_STIMULUS` (+ optional `TMP_LEVELLER_CAL_LUFS`).
pub fn probe_level_footswitch(
    slot: u32,
    switch: u32,
    lev_group: &str,
    lev_node: &str,
    lev_param: &str,
    target_lufs: f64,
    commit: bool,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    let (preset, _, _) = read_slot_preset_parsed(slot)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let job = FootswitchLevelJob {
        switch,
        lev_group_id: lev_group.to_string(),
        lev_node_id: lev_node.to_string(),
        lev_parameter_id: lev_param.to_string(),
        target_lufs,
        // probe: solve-and-write, never the verify-only row.
        scene_context: None,
    };
    let plan = footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[footswitch::FsJobKey {
            switch,
            lev_node,
            lev_param,
            target_bits: target_lufs.to_bits(),
        }],
    )
    .into_iter()
    .next()
    .ok_or("planner returned no plan")?;
    let lev = (lev_group, lev_node, lev_param);
    let (write, plan_label) = match &plan {
        footswitch::FsLevelPlan::Clamp(msg) => return Err(msg.clone()),
        footswitch::FsLevelPlan::BakeShared { .. } => {
            return Err("single-job probe cannot be a shared bake".into())
        }
        footswitch::FsLevelPlan::Bake {
            clear_stale,
            mirror_scenes,
            ..
        } => (
            leveller::FsWrite::Bake {
                clear_stale: *clear_stale,
                mirror_scenes: mirror_scenes.clone(),
            },
            format!("BAKE → value written onto the block (mirror scenes {mirror_scenes:?})"),
        ),
        footswitch::FsLevelPlan::Assign { .. } => {
            let (value_b, spec) = resolve_footswitch_job(&ftsw, &preset, &job)?;
            let label = format!(
                "ASSIGN → param fn @ index {} (valueB={value_b:.4})",
                spec.function_index
            );
            (leveller::FsWrite::Assign { value_b, spec }, label)
        }
    };
    let engaged = match &plan {
        footswitch::FsLevelPlan::Bake { engaged, .. }
        | footswitch::FsLevelPlan::Assign { engaged } => engaged.clone(),
        _ => Vec::new(),
    };
    let restore = crate::last_loaded_scene(&preset);
    let r = leveller::level_footswitch(
        slot,
        switch,
        lev,
        &engaged,
        &write,
        &stim,
        target_lufs,
        commit,
        true,
        restore,
        &leveller::FsParamTarget::from_preset(&preset, lev_node, lev_param),
    )?;
    let mut out = format!(
        "[probe --level-footswitch] preset slot {} · FS{switch} · {lev_group}/{lev_node}.{lev_param}  ({})\n",
        slot + 1,
        if commit { "COMMIT — wrote + saved" } else { "DRY — not written" }
    );
    out += &format!("  plan: {plan_label}  ·  method={}\n", r.method);
    out += &format!(
        "  measured(seed) {:.2} LUFS → target {:.1}  ⇒  valueA={:.4}{}  (engaged {:.2} LUFS, {} iters, spread {:.1} LU)\n",
        r.measured_lufs,
        r.target_lufs,
        r.final_value,
        if r.clamped {
            match &r.clamp_reason {
                Some(reason) => format!("  [CLAMPED — {reason}]"),
                None => "  [CLAMPED]".to_string(),
            }
        } else {
            String::new()
        },
        r.predicted_lufs,
        r.iterations,
        r.dynamic_spread_lu.unwrap_or(0.0),
    );
    if let Some(v) = r.verify_lufs {
        out += &format!(
            "  verify (fresh engaged capture @ valueA): {v:.2} LUFS  (err {:+.2} LU)\n",
            v - r.target_lufs
        );
    }
    if r.saved {
        out += "  [SAVED to preset]\n";
    }
    Ok(out)
}

/// HW validation for the batched footswitch WRITE phase (`probe --fs-batch
/// <listIndex> <v1> <v2> …`): enumerate the preset's block-acting switches, plan
/// bake-vs-assign exactly like `level_footswitches_apply`, pair each with the given
/// FIXED value (no measurement captures — the sweep phase is unchanged, shipped
/// code), then commit every write on ONE live-edit session with ONE save
/// (`write_footswitch_values`). Verify persistence with `--export` afterwards.
/// Point it at a SCRATCH preset: it persists.
pub fn probe_fs_batch(list_index: u32, values: Vec<f32>) -> Result<String, String> {
    let (preset, _, _) = read_slot_preset_parsed(list_index)?;
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let infos = footswitch::enumerate_block_footswitches(&ftsw, &preset);
    if infos.is_empty() {
        return Err("preset has no block-acting footswitches".to_string());
    }
    let mut out = format!(
        "[probe --fs-batch] idx {list_index} · {} block-acting switch(es) · values {values:?}\n",
        infos.len()
    );

    // One job per switch: its first level-param candidate, target irrelevant (fixed values).
    let jobs: Vec<FootswitchLevelJob> = infos
        .iter()
        .filter_map(|info| {
            let p = info.level_params.first()?;
            Some(FootswitchLevelJob {
                switch: info.switch,
                lev_group_id: p.group_id.clone(),
                lev_node_id: p.node_id.clone(),
                lev_parameter_id: p.parameter_id.clone(),
                target_lufs: -24.0,
                scene_context: None,
            })
        })
        .collect();
    let keys: Vec<footswitch::FsJobKey> = jobs
        .iter()
        .map(|j| footswitch::FsJobKey {
            switch: j.switch,
            lev_node: &j.lev_node_id,
            lev_param: &j.lev_parameter_id,
            target_bits: j.target_lufs.to_bits(),
        })
        .collect();
    let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys);

    let mut pends: Vec<leveller::FsPendingWrite> = Vec::new();
    for (idx, (job, plan)) in jobs.iter().zip(&plans).enumerate() {
        let value = values.get(idx).copied().unwrap_or(0.5);
        let lev = (
            job.lev_group_id.clone(),
            job.lev_node_id.clone(),
            job.lev_parameter_id.clone(),
        );
        match plan {
            footswitch::FsLevelPlan::Bake {
                clear_stale,
                mirror_scenes,
                ..
            } => {
                out += &format!(
                    "  FS{} {}/{}.{} → BAKE value {value} (mirror scenes {mirror_scenes:?})\n",
                    job.switch, lev.0, lev.1, lev.2
                );
                pends.push(leveller::FsPendingWrite {
                    switch: job.switch,
                    lev,
                    write: leveller::FsWrite::Bake {
                        clear_stale: *clear_stale,
                        mirror_scenes: mirror_scenes.clone(),
                    },
                    value,
                });
            }
            footswitch::FsLevelPlan::Assign { .. } => {
                let (value_b, spec) = resolve_footswitch_job(&ftsw, &preset, job)?;
                out += &format!(
                    "  FS{} {}/{}.{} → ASSIGN fn#{} valueA {value} valueB {value_b}\n",
                    job.switch, lev.0, lev.1, lev.2, spec.function_index
                );
                pends.push(leveller::FsPendingWrite {
                    switch: job.switch,
                    lev,
                    write: leveller::FsWrite::Assign { value_b, spec },
                    value,
                });
            }
            footswitch::FsLevelPlan::BakeShared { rep } => {
                out += &format!(
                    "  FS{} → shares FS-job #{rep}'s bake (no write)\n",
                    job.switch
                );
            }
            footswitch::FsLevelPlan::Clamp(msg) => {
                out += &format!("  FS{} → CLAMP: {msg}\n", job.switch);
            }
        }
    }
    let restore = crate::last_loaded_scene(&preset);
    leveller::write_footswitch_values(list_index, &pends, restore)?;
    out += &format!(
        "wrote {} switch(es) on ONE session + ONE save — export the slot to verify\n",
        pends.len()
    );
    Ok(out)
}

/// `probe --set-param-save <listIdx> <expectName> <group> <node> <param> <value> [save]` —
/// write ONE numeric block parameter and (with `save`) PERSIST it to any slot — unlike
/// `--set-param` (slot_write.rs), which is scratch-zone-only and working-copy-only. The
/// mandatory `expectName` is checked against the slot's non-destructive `displayName` read
/// INSIDE this function (the destructive-op guard rule: same address space as the mutation —
/// a dry-run eyeball is not a guard, since the save re-run is a separate invocation that can
/// carry a different index). DRY by default; the save path reuses the `--bake-validate`
/// write+save shape (heartbeat-live session, never re-amped, BASE recalled before the write
/// per the scene-context rule) and read-back-verifies the stored value. Scene-overlay
/// mirroring is out of scope (this is a probe diagnostic, not the product write path) — on a
/// preset WITH scenes, overlays that restate the param keep their authored values.
pub fn probe_set_param_save(
    list_index: u32,
    expect_name: &str,
    group: &str,
    node: &str,
    param: &str,
    value: f32,
    save: bool,
) -> Result<String, String> {
    // `changeParameter` carries the value in the block's OWN real units verbatim (dB,
    // Hz, …) — not all params are `[0,1]` (HW: `ACD_Boost.gain` accepts raw dB; see
    // `param_class`'s doc). A diagnostic seam only guards a sane wire value.
    if !value.is_finite() {
        return Err(format!("refusing non-finite value {value}"));
    }
    let (preset, _, _) = read_slot_preset_parsed(list_index)?;
    let name = preset
        .get("info")
        .and_then(|i| i.get("displayName"))
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    if name != expect_name {
        return Err(format!(
            "refusing slot {}: reads \u{201c}{name}\u{201d}, expected \u{201c}{expect_name}\u{201d}",
            list_index + 1
        ));
    }
    let before = node_param_f64(&preset, node, param)
        .ok_or_else(|| format!("{node}.{param} not found in slot {}", list_index + 1))?;
    let mut out = format!(
        "[probe --set-param-save] slot {} \u{201c}{name}\u{201d} \u{b7} {group}/{node}.{param}\n  before: {before:.4}\n",
        list_index + 1
    );
    if !save {
        out += &format!("  DRY \u{2014} would write {value:.4} (re-run with `save` to persist)\n");
        return Ok(out);
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    {
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        s.load_preset(list_index)?;
        for _ in 0..8 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(150);
        }
        s.load_scene(crate::session::BASE_SCENE_SLOT)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_SCENE_RECALL_MS,
        ));
        s.change_parameter(group, node, param, value)?;
        s.save_current_preset(list_index)?;
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let (after_preset, _, _) = read_slot_preset_parsed(list_index)?;
    let after = node_param_f64(&after_preset, node, param)
        .ok_or_else(|| format!("{node}.{param} vanished after save"))?;
    out += &format!(
        "  after : {after:.4}  \u{21d2}  {}\n",
        if (after - value as f64).abs() < 1e-3 {
            "SAVED"
        } else {
            "MISMATCH \u{2014} verify the slot manually"
        }
    );
    Ok(out)
}
