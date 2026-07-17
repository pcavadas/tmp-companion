//! `probe --doctor-defects <slot>` — the versioned KNOWN-DEFECT fixture sweep.
//!
//! Generalizes `--doctor-inject` (one ad-hoc gains vector per invocation) into
//! a COMMITTED table of named defect recipes with pinned pass/fail verdicts:
//! each recipe injects ops into a clean preset's live edit buffer (the same
//! `ops_session` vehicle the Doctor's own prescriptions use), captures
//! before/after, and checks the after-capture's fired verdicts against
//! `must_fire` (all required) / `must_not_fire` (none allowed). This is the
//! repo's "author presets with known issues and verify the doctor against
//! them" harness — the recipe table below IS the fixture set; no `.preset`
//! files, no import machinery. Never saves (the edit buffer is discarded by a
//! stored-preset reload after every recipe, and again belt-and-braces at the
//! end); ends re-amp OFF.
//!
//! Usage:
//! ```text
//! probe --doctor-defects <slot> [--out <report.json>]
//! ```
//! `slot` is a 0-based list index carrying a CLEAN preset (any chain — the
//! recipes append their own EQ-10/reverb inserts, they don't touch existing
//! blocks). If the stored preset itself already fires a verdict, each
//! affected recipe prints a loud warning and is still scored (the result is
//! flagged unreliable via `beforeClean` in the JSON row, not skipped).
//! Attended; loads the probed slot.

use crate::doctor;
use crate::leveller;

use super::analyze::DoctorRead;
use super::doctor_inject::{last_guitar_group_anchor, measure, tail_ms_for_doc};
use super::stimulus::{probe_stimulus_path, read_stimulus_48k};

/// One versioned defect recipe: inject `ops` into a clean preset's live edit
/// buffer, capture, diagnose — `must_fire` verdicts must ALL appear in the
/// after-capture, and no verdict in `must_not_fire` may appear (an empty
/// `must_fire` is a pure must-NOT-fire guard, vacuously HIT unless violated).
struct DefectRecipe {
    name: &'static str,
    /// Why this recipe produces (or probes for) the defect — shown verbatim
    /// in the report.
    rationale: &'static str,
    ops: Vec<doctor::DoctorOp>,
    must_fire: &'static [&'static str],
    must_not_fire: &'static [&'static str],
}

/// An `ACD_TenBandEQStereo` insert at `group_id` with the given `controlId=dB`
/// band gains (empty = the clean-insert control — an all-zero-gain EQ proves
/// the insert itself doesn't shift verdicts).
fn eq10_insert(group_id: &str, gains: &[(&str, f64)]) -> doctor::DoctorOp {
    doctor::DoctorOp::InsertNode {
        group_id: group_id.to_string(),
        before_fender_id: None,
        fender_id: "ACD_TenBandEQStereo".to_string(),
        params: gains.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
    }
}

/// An `ACD_FiveBandParamEQ` insert with explicit per-filter params — the
/// controlIds (`filterNfrequency`/`filterNgaindb`/`filterNq`, HW-verified
/// 2026-07-17: an injected 1 kHz/+12 dB/Q8 read back at 1008 Hz/q 7.5) let a
/// recipe place an EXACT (freq, height, Q) resonance. Bands 2–4 default to
/// active peaks; band 1 defaults to a high-pass (never use it for a peak).
fn peq_insert(group_id: &str, params: &[(&str, f64)]) -> doctor::DoctorOp {
    doctor::DoctorOp::InsertNode {
        group_id: group_id.to_string(),
        before_fender_id: None,
        fender_id: "ACD_FiveBandParamEQ".to_string(),
        params: params.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
    }
}

/// The versioned recipe table (see the module doc for the design). `group_id`
/// is the chain's last group — the same post-amp/cab anchor `--doctor-inject`
/// uses — resolved once per slot by the runner (it needs the live graph) and
/// threaded in here since the ops can't be `const`.
fn recipes(group_id: &str) -> Vec<DefectRecipe> {
    vec![
        DefectRecipe {
            name: "control",
            rationale: "clean-insert control: an all-zero-gain EQ-10 insert — proves the insert itself doesn't shift any verdict.",
            ops: vec![eq10_insert(group_id, &[])],
            must_fire: &[],
            // Every verdict the engine can emit: the control's whole point is that
            // a clean insert shifts NOTHING.
            must_not_fire: &[
                "muddy", "boomy", "harsh", "fizzy", "lost", "thin", "washed", "spiky",
                "buried", "dark", "bright", "resonant", "boxy",
            ],
        },
        DefectRecipe {
            name: "muddy",
            rationale: "EQ-10 gain250hz=+12 dB — a textbook low-mid buildup, the band the muddy rule reads directly.",
            ops: vec![eq10_insert(group_id, &[("gain250hz", 12.0)])],
            must_fire: &["muddy"],
            must_not_fire: &["thin", "boomy"],
        },
        DefectRecipe {
            name: "lost",
            rationale: "EQ-10 gain500hz=-12 dB, gain1khz=-12 dB — a mids scoop, the band the lost rule reads directly.",
            ops: vec![eq10_insert(group_id, &[("gain500hz", -12.0), ("gain1khz", -12.0)])],
            must_fire: &["lost"],
            must_not_fire: &[],
        },
        DefectRecipe {
            name: "washed",
            rationale: "ACD_TMSmallHall reverb inserted with `mix` (its `REVERB_MIX`-table controlId — the SAME one the washed Rx turns down) cranked to 0.9 of range. Decay is left at the freshly-inserted block's device default: no decay controlId is documented anywhere in this repo, and mix alone should already push the post-stimulus tail well past the washed threshold.",
            ops: vec![doctor::DoctorOp::InsertNode {
                group_id: group_id.to_string(),
                before_fender_id: None,
                fender_id: "ACD_TMSmallHall".to_string(),
                params: vec![("mix".to_string(), 0.9)],
            }],
            must_fire: &["washed"],
            must_not_fire: &[],
        },
        DefectRecipe {
            name: "resonant_wah",
            rationale: "ACD_CryBabyGCB95 inserted at its DEFAULT (cocked) pedal position — the canonical playable resonance (HW: a wide mid bump, transfer Q≈6, inside the resonant [2,16] Q window; the chain's own cab-comb lines sit at Q 39+ and are excluded by the ceiling).",
            ops: vec![doctor::DoctorOp::InsertNode {
                group_id: group_id.to_string(),
                before_fender_id: None,
                fender_id: "ACD_CryBabyGCB95".to_string(),
                params: Vec::new(),
            }],
            // Physics regression guard (matrix round, 2026-07-17): a cocked
            // wah is a WIDE hump — peak-space h ≈ 6.7 while its mid-band
            // local reads +15.7 dB — so it must surface via the BAND rules
            // (thin fires today, an honest co-fire), never as peak-space
            // `resonant` (that would mean the octave-envelope discriminator
            // regressed) and never as WASHED.
            must_fire: &["thin"],
            must_not_fire: &["washed", "resonant"],
        },
        DefectRecipe {
            name: "resonant_peq",
            rationale: "EQ-5 parametric: TWO stacked +12 dB Q14 peaks at 2.6 kHz — a flagrant narrow ring (the Q14 saturation ceiling ≈17 dB clears the 13.5 gate on any site; matrix-calibrated 2026-07-17). The calibrated resonant positive.",
            ops: vec![peq_insert(
                group_id,
                &[
                    ("filter3frequency", 2_600.0),
                    ("filter3gaindb", 12.0),
                    ("filter3q", 14.0),
                    ("filter4frequency", 2_600.0),
                    ("filter4gaindb", 12.0),
                    ("filter4q", 14.0),
                ],
            )],
            must_fire: &["resonant"],
            must_not_fire: &["washed"],
        },
        DefectRecipe {
            name: "boxy_peq",
            rationale: "EQ-5 parametric: TWO stacked +12 dB Q8 peaks at 420 Hz — a clearly audible cardboard hump in the 300–500 Hz boxy range (single +12/Q8 measured 8.3 vs the 7.5 floor; stacking doubles the physical boost for cross-site margin).",
            ops: vec![peq_insert(
                group_id,
                &[
                    ("filter2frequency", 420.0),
                    ("filter2gaindb", 12.0),
                    ("filter2q", 8.0),
                    ("filter3frequency", 420.0),
                    ("filter3gaindb", 12.0),
                    ("filter3q", 8.0),
                ],
            )],
            must_fire: &["boxy"],
            must_not_fire: &["resonant", "washed"],
        },
    ]
}

/// Run one recipe: before capture (stored preset) → inject on a live edit
/// buffer → after capture (edit buffer, no reload) → discard. Mirrors
/// `probe_doctor_inject`'s single-recipe body exactly (shares its `measure`
/// helper for both captures) — see that module's doc for the capture/restore
/// sequencing. On an op-apply failure `ops_session` already restores the
/// stored preset before returning; on an after-capture failure the edit
/// buffer is left injected — same as `--doctor-inject` — but the NEXT
/// recipe's before-capture reloads the stored preset regardless (a `load`
/// always discards an unsaved edit buffer), and the runner's own final
/// belt-and-braces restore covers the last recipe.
///
/// `before_tail_ms` is the STORED preset's production tail (resolved ONCE by
/// the caller — constant across recipes since each one restores the same
/// base before the next injects). The AFTER capture re-resolves its OWN tail
/// off the EDITED graph (`tail_ms_for_doc`) — same reasoning as
/// `probe_doctor_inject`: the `washed` recipe inserts a reverb, which can
/// turn a known-dry base preset wet, and reusing `before_tail_ms` would
/// under-capture that recipe's own wash tail.
fn run_recipe(
    slot: u32,
    stim: &[f32],
    before_tail_ms: u64,
    preset_name: &str,
    recipe: &DefectRecipe,
) -> Result<(DoctorRead, DoctorRead, String), String> {
    let mut text = String::new();
    let (before, line) = measure(
        stim,
        "before",
        leveller::doctor_capture(slot, None, &[], stim, Some(0.5), before_tail_ms, false),
    )?;
    text += &line;
    if !before.verdicts.is_empty() {
        text += &format!(
            "  !!! WARNING: slot {slot}'s stored preset already fires {:?} before injecting — not clean, this recipe's result is unreliable !!!\n",
            before.verdicts
        );
    }

    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let mut ops_s = crate::commands::doctor::ops_session(slot, preset_name, &recipe.ops, "inject")?;
    let _ = ops_s.pump_collect(700);
    let after_tail_ms = ops_s
        .current_preset_value()
        .map_or(before_tail_ms, |doc| u64::from(tail_ms_for_doc(&doc)));
    drop(ops_s);
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let after_res = measure(
        stim,
        "after",
        leveller::doctor_capture_current(stim, None, &[], Some(0.5), after_tail_ms),
    );
    // Restore BEFORE propagating an after-capture failure: a mid-table recipe's
    // next `before` load self-heals the buffer, but the LAST recipe has no next
    // load — its error must not strand the injected edit (best-effort here; the
    // ?-path restore below still reports its own failure on the happy path).
    let (after, line) = match after_res {
        Ok(v) => v,
        Err(e) => {
            // The capture error stays primary, but a restore failure here means
            // the injected edit may still be live — append it, never drop it.
            return Err(leveller::append_restore_err(
                e,
                leveller::restore_saved_preset(slot),
            ));
        }
    };
    text += &line;

    leveller::restore_saved_preset(slot)?;
    text += "  (edit buffer discarded — stored preset reloaded)\n";
    Ok((before, after, text))
}

/// Score `recipe` against `after`'s fired verdicts and build its JSON row.
/// Pure (no device I/O) — unit-tested directly.
fn recipe_row(recipe: &DefectRecipe, before: &DoctorRead, after: &DoctorRead) -> serde_json::Value {
    let before_clean = before.verdicts.is_empty();
    let violation = recipe
        .must_not_fire
        .iter()
        .any(|k| after.verdicts.contains(k));
    let hit = recipe.must_fire.iter().all(|k| after.verdicts.contains(k));
    // A dirty baseline (something already fired pre-injection) can't be attributed
    // to the injected defect — a HIT or VIOLATION would be trivially explained by
    // pre-existing state, not the injection, so it can't count as evidence either way.
    let status = if !before_clean {
        "UNRELIABLE"
    } else if violation {
        "VIOLATION"
    } else if hit {
        "HIT"
    } else {
        "MISS"
    };
    serde_json::json!({
        "recipe": recipe.name,
        "rationale": recipe.rationale,
        "mustFire": recipe.must_fire,
        "mustNotFire": recipe.must_not_fire,
        "beforeClean": before_clean,
        "before": {
            "verdicts": before.verdicts,
            "tiltDbPerOct": before.tilt_slope,
            "tailRatioDb": before.tail_ratio_db,
        },
        "after": {
            "verdicts": after.verdicts,
            "tiltDbPerOct": after.tilt_slope,
            "tailRatioDb": after.tail_ratio_db,
            "deviations": after.deviations,
            "locals": after.locals,
        },
        "status": status,
    })
}

/// See the module doc. `out_path` optionally writes a deterministic JSON
/// report (rows + summary counts), mirroring `--doctor-window-ab`'s `--out`.
pub fn probe_doctor_defects(slot: u32, out_path: Option<&str>) -> Result<String, String> {
    let stim = leveller::doctor_stim_slice(read_stimulus_48k(&probe_stimulus_path(
        "guitar-humbucker",
    )?)?);

    // Resolve the insert anchor (last group) + preset name + the STORED
    // preset's own production tail ONCE — the anchor/name/before-tail are all
    // stable across every recipe since each one restores the stored preset
    // before the next injects (each recipe's AFTER capture re-resolves its
    // own tail off the edited graph — see `run_recipe`'s doc).
    // Fresh-connection re-amp OFF on EVERY exit path from here down (the
    // drop-guard form of the old sweep-tail call).
    let _reamp_off = super::ReampOffGuard;
    let (group_id, name, before_tail) = last_guitar_group_anchor(slot)?;
    let before_tail = u64::from(before_tail);
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let mut out = format!("doctor-defects slot {slot} ({name})\n");
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let (mut hits, mut misses, mut violations, mut unreliable, mut errors) =
        (0u32, 0u32, 0u32, 0u32, 0u32);

    for recipe in recipes(&group_id) {
        out += &format!("\n[{}] {}\n", recipe.name, recipe.rationale);
        match run_recipe(slot, &stim, before_tail, &name, &recipe) {
            Ok((before, after, text)) => {
                out += &text;
                let row = recipe_row(&recipe, &before, &after);
                let status = row["status"].as_str().unwrap_or("?");
                match status {
                    "HIT" => hits += 1,
                    "MISS" => misses += 1,
                    "VIOLATION" => violations += 1,
                    "UNRELIABLE" => unreliable += 1,
                    _ => {}
                }
                out += &format!("  → {status}\n");
                rows.push(row);
            }
            Err(e) => {
                out += &format!("  FAILED: {e}\n");
                errors += 1;
                rows.push(serde_json::json!({ "recipe": recipe.name, "error": e }));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    }

    // Belt-and-braces: whatever the last recipe left behind (a failed
    // after-capture can leave an injected edit buffer, see `run_recipe`'s
    // doc), restore the stored preset (re-amp OFF is the fn-top drop guard).
    // A restore failure is surfaced in the report — the sweep's rows are still
    // valid evidence, but the operator must know the edit buffer may be dirty.
    if let Err(e) = leveller::restore_saved_preset(slot) {
        out += &format!(
            "\n!!! WARNING: final edit-buffer restore failed ({e}) — slot {slot}'s live edit \
             buffer may still hold the last injected defect; reload the preset to discard !!!\n"
        );
    }

    out += &format!(
        "\ndoctor-defects summary: {} recipe(s) — HIT={hits} MISS={misses} VIOLATION={violations} UNRELIABLE={unreliable} error={errors}\n",
        rows.len()
    );

    if let Some(path) = out_path {
        let report = serde_json::json!({
            "slot": slot,
            "presetName": name,
            "rows": rows,
            "summary": {
                "hit": hits,
                "miss": misses,
                "violation": violations,
                "unreliable": unreliable,
                "error": errors,
            },
        });
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(path, &json).map_err(|e| format!("write {path}: {e}"))?;
        out += &format!("  → {path}\n");
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full verdict-key set `doctor::diagnose_kind` can push (from
    /// `doctor.rs`'s `push`/`localized` calls) — pinned here so a recipe
    /// typo'ing a key is caught without HW.
    const KNOWN_VERDICT_KEYS: &[&str] = &[
        "muddy", "boomy", "harsh", "fizzy", "lost", "thin", "washed", "spiky", "buried", "dark",
        "bright", "resonant", "boxy",
    ];

    fn fake_read(verdicts: Vec<&'static str>) -> DoctorRead {
        DoctorRead {
            band_db: vec![],
            deviations: vec![],
            tilt_slope: None,
            locals: vec![],
            tail_ratio_db: -20.0,
            spread_lu: 0.0,
            verdicts,
            onset_confident: true,
            peaks: Vec::new(),
        }
    }

    #[test]
    fn recipes_nonempty_and_every_key_known() {
        let rs = recipes("G1");
        assert!(!rs.is_empty());
        for r in &rs {
            for k in r.must_fire.iter().chain(r.must_not_fire.iter()) {
                assert!(
                    KNOWN_VERDICT_KEYS.contains(k),
                    "recipe {}: unknown verdict key {k:?}",
                    r.name
                );
            }
        }
    }

    #[test]
    fn control_recipe_injects_all_zero_gains() {
        let rs = recipes("G1");
        let control = rs
            .iter()
            .find(|r| r.name == "control")
            .expect("control recipe present");
        match control.ops.as_slice() {
            [doctor::DoctorOp::InsertNode {
                fender_id, params, ..
            }] => {
                assert_eq!(fender_id, "ACD_TenBandEQStereo");
                assert!(
                    params.is_empty(),
                    "control must write no gain params — by construction a no-op insert"
                );
            }
            other => panic!("control recipe should be a single EQ-10 insert, got {other:?}"),
        }
    }

    #[test]
    fn scoring_hit_miss_violation_info() {
        let rs = recipes("G1");
        let muddy = rs.iter().find(|r| r.name == "muddy").unwrap();
        let clean_before = fake_read(vec![]);

        let hit = recipe_row(muddy, &clean_before, &fake_read(vec!["muddy"]));
        assert_eq!(hit["status"], "HIT");

        let miss = recipe_row(muddy, &clean_before, &fake_read(vec![]));
        assert_eq!(miss["status"], "MISS");

        let violation = recipe_row(muddy, &clean_before, &fake_read(vec!["muddy", "boomy"]));
        assert_eq!(violation["status"], "VIOLATION");

        let boxy_peq = rs.iter().find(|r| r.name == "boxy_peq").unwrap();
        let hit = recipe_row(boxy_peq, &clean_before, &fake_read(vec!["boxy"]));
        assert_eq!(hit["status"], "HIT");
        let violation2 = recipe_row(
            boxy_peq,
            &clean_before,
            &fake_read(vec!["resonant", "boxy"]),
        );
        assert_eq!(violation2["status"], "VIOLATION");
    }

    #[test]
    fn a_dirty_baseline_reads_unreliable_never_hit_or_violation() {
        let rs = recipes("G1");
        let muddy = rs.iter().find(|r| r.name == "muddy").unwrap();
        // "muddy" already fires BEFORE injection — a same after-capture can't be
        // attributed to the recipe's own defect.
        let dirty_before = fake_read(vec!["muddy"]);

        let row = recipe_row(muddy, &dirty_before, &fake_read(vec!["muddy"]));
        assert_eq!(row["status"], "UNRELIABLE");
        assert_eq!(row["beforeClean"], false);

        // Even a would-be VIOLATION (an unrelated must_not_fire verdict) is masked —
        // the baseline itself is untrustworthy, not just the must_fire signal.
        let row2 = recipe_row(muddy, &dirty_before, &fake_read(vec!["muddy", "boomy"]));
        assert_eq!(row2["status"], "UNRELIABLE");
    }
}
