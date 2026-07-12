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
    // One read → the node's value, base bypass, switch fn count, and engaged force-list.
    type Snap = (f64, bool, usize, Vec<(String, String, bool)>);
    let snapshot = || -> Result<Snap, String> {
        let (p, _, _) = read_slot_preset_parsed(slot)?;
        let ftsw = p.get("ftsw").cloned().unwrap_or(serde_json::Value::Null);
        let v = node_param_f64(&p, node, param).ok_or("param not found after read")?;
        let fns = ftsw
            .as_array()
            .and_then(|a| a.get(switch as usize)?.as_array().map(Vec::len))
            .unwrap_or(usize::MAX);
        let engaged = footswitch::engaged_bypass_for_switch(&ftsw, &p, switch);
        Ok((
            v,
            footswitch::block_bypassed_in_base(&p, node),
            fns,
            engaged,
        ))
    };

    let (orig, byp0, fns0, engaged) = snapshot()?;
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
        &leveller::FsWrite::Bake { clear_stale: None },
        &stim,
        target_lufs,
        true,
        false,
    )?;
    out += &format!(
        "  baked: method={} value={:.4}{}\n",
        r.method,
        r.final_value,
        if r.clamped { " [clamped]" } else { "" }
    );

    // Verify field-8: the value landed, bypass unchanged, NO param fn added.
    let (after, byp1, fns1, _) = snapshot()?;
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
    let (restored, _, _, _) = snapshot()?;
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

/// Probe (read-only): list a slot's block-acting footswitches with each acted-on block's base
/// bypass + the bake/assign classification for its first level param — to find bake-eligible
/// presets (an active on-off enabling an OFF-in-base block).
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
                has_fs_scenes,
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
            leveller::SETTLE_AFTER_LOAD_MS,
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

    let (preset, has_fs_scenes, _) = read_slot_preset_parsed(slot)?;
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
        has_fs_scenes,
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
        footswitch::FsLevelPlan::Bake { clear_stale, .. } => (
            leveller::FsWrite::Bake {
                clear_stale: *clear_stale,
            },
            "BAKE → value written onto the block".to_string(),
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
    let (preset, has_fs_scenes, _) = read_slot_preset_parsed(list_index)?;
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
    let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys, has_fs_scenes);

    let mut pends: Vec<leveller::FsPendingWrite> = Vec::new();
    for (idx, (job, plan)) in jobs.iter().zip(&plans).enumerate() {
        let value = values.get(idx).copied().unwrap_or(0.5);
        let lev = (
            job.lev_group_id.clone(),
            job.lev_node_id.clone(),
            job.lev_parameter_id.clone(),
        );
        match plan {
            footswitch::FsLevelPlan::Bake { clear_stale, .. } => {
                out += &format!(
                    "  FS{} {}/{}.{} → BAKE value {value}\n",
                    job.switch, lev.0, lev.1, lev.2
                );
                pends.push(leveller::FsPendingWrite {
                    switch: job.switch,
                    lev,
                    write: leveller::FsWrite::Bake {
                        clear_stale: *clear_stale,
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
    leveller::write_footswitch_values(list_index, &pends)?;
    out += &format!(
        "wrote {} switch(es) on ONE session + ONE save — export the slot to verify\n",
        pends.len()
    );
    Ok(out)
}
