//! Firmware-migration scan/plan/apply + loudness audit + snapshot listing.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ─── Firmware-migration analysis over the imported library (no device) ────────────

/// One preset affected by a firmware migration: the in-use block models it
/// references that are absent from the target catalog.
#[derive(serde::Serialize)]
pub(crate) struct MigrationRow {
    list_index: Option<u32>,
    name: String,
    affected_blocks: Vec<String>,
}

/// Scan the imported library against a target firmware's `target_catalog` (valid block
/// model ids) and list presets that reference models the target no longer has
/// (OFFLINE read-only). The user resolves them via Block Replace.
#[tauri::command]
pub(crate) fn migration_scan(
    target_catalog: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationRow>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    // The "old" catalog is every block model actually used across the library.
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, &target_catalog);
    let removed: std::collections::HashSet<String> = diff.removed.into_iter().collect();
    // Index by array position (stable identity), then map results back to records.
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            serde_json::from_str(&r.decoded_json)
                .ok()
                .map(|v| (i as u32, v))
        })
        .collect();
    Ok(migration::scan_affected(&presets, &removed)
        .into_iter()
        .map(|(pos, blocks)| {
            let r = &lib.records[pos as usize];
            MigrationRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                affected_blocks: blocks,
            }
        })
        .collect())
}

/// The migration plan: how the catalog changed + the concrete block swaps to run.
#[derive(serde::Serialize)]
pub(crate) struct MigrationPlan {
    classified: migration::ClassifiedDiff,
    plan: Vec<migration::Replacement>,
}

/// Build the classified catalog diff + per-preset replacement plan from a target
/// catalog + a user rename map (matched presets only, keyed by real list_index).
fn compute_migration_plan(
    lib: &library::Library,
    target_catalog: &[String],
    rename_map: &std::collections::BTreeMap<String, String>,
) -> MigrationPlan {
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, target_catalog);
    let classified = migration::classify(&diff, rename_map);
    let removed: std::collections::HashSet<String> = diff.removed.iter().cloned().collect();
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .filter_map(|r| r.list_index.zip(serde_json::from_str(&r.decoded_json).ok()))
        .collect();
    let affected = migration::scan_affected(&presets, &removed);
    let plan = migration::plan_replacements(&affected, rename_map);
    MigrationPlan { classified, plan }
}

/// Preview the renamed/removed/added block ids + the planned swaps for a
/// target firmware catalog and a rename map (pure, no device).
#[tauri::command]
pub(crate) fn migration_plan(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> Result<MigrationPlan, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    Ok(compute_migration_plan(lib, &target_catalog, &rename_map))
}

/// One preset's migration-apply outcome.
#[derive(serde::Serialize)]
pub(crate) struct MigrationApplyRow {
    list_index: u32,
    swaps: usize,
    applied: bool,
    error: Option<String>,
}

/// Snapshot a preset's pre-edit JSON before an in-place migration write, so a mid-run
/// failure still leaves it revertible (bulkrun's AC2 discipline, which `migration_apply`
/// bypasses by calling `replace_inplace_core` directly). `Err` ⇒ DO NOT WRITE. `source`
/// is `"offline-file"`: migration edits a complete `.preset` from the offline library.
pub(crate) fn snapshot_before_migrate(
    backup_dir: &std::path::Path,
    list_index: u32,
    display_name: &str,
    before_json: &str,
) -> Result<std::path::PathBuf, String> {
    // list_enum 1 = My Presets; source "offline-file" = migration edits a complete
    // `.preset` from the offline library.
    bulkrun::save_pre_write_snapshot(
        backup_dir,
        1,
        list_index,
        display_name,
        "offline-file",
        before_json,
    )
}

/// Apply the migration plan: per affected preset, swap each renamed block
/// (Block Replace defaults) and re-import OFFLINE in place. `dry_run` previews the swap
/// counts without writing (HW-pending — device write gated by the read-only policy).
#[tauri::command]
pub(crate) async fn migration_apply(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    dry_run: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationApplyRow>, String> {
    // Build the plan + each affected preset's (original json, edited bytes) under the lock.
    let mut jobs: Vec<(u32, String, usize, String, Vec<u8>)> = Vec::new();
    {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let plan = compute_migration_plan(lib, &target_catalog, &rename_map).plan;
        // Group the plan by preset, then apply all its swaps to one decoded copy.
        let mut by_preset: std::collections::BTreeMap<u32, Vec<migration::Replacement>> =
            Default::default();
        for r in plan {
            by_preset.entry(r.list_index).or_default().push(r);
        }
        for (li, reps) in by_preset {
            let Some(rec) = lib.records.iter().find(|r| r.list_index == Some(li)) else {
                continue;
            };
            let before_json = rec.decoded_json.clone();
            let mut v: serde_json::Value =
                serde_json::from_str(&before_json).map_err(|e| e.to_string())?;
            let swaps: usize = reps
                .iter()
                .map(|r| migration::apply_replacement(&mut v, r))
                .sum();
            let bytes = backup::xor_jld(
                serde_json::to_string(&v)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            );
            jobs.push((li, rec.display_name.clone(), swaps, before_json, bytes));
        }
    }
    // Dry run — preview the swap counts without touching the device or a backup.
    if dry_run {
        return Ok(jobs
            .into_iter()
            .map(|(li, _name, swaps, _before, _bytes)| MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: None,
            })
            .collect());
    }
    // A write must be revertible: snapshot each preset's pre-edit state before its
    // in-place re-import (bulkrun's AC2, which migration bypasses by calling
    // replace_inplace_core directly). Resolve the backups dir once up front so a whole
    // run refuses rather than writing the first preset unbacked.
    let backup_dir = backup::backups_dir(&app)?;
    let mut rows = Vec::with_capacity(jobs.len());
    for (li, display_name, swaps, before_json, bytes) in jobs {
        // AC2 — snapshot before writing; refuse the write if it fails (no backup = not
        // revertible). Uses the ORIGINAL decoded_json, not the edited bytes.
        if let Err(e) = snapshot_before_migrate(&backup_dir, li, &display_name, &before_json) {
            rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(format!("snapshot: {e} — NOT written (kept revertible)")),
            });
            continue;
        }
        let outcome = with_released_seize(state.session.clone(), move || {
            replace_inplace_core(li, &bytes)
        })
        .await;
        match outcome {
            Ok(o) if o.edit_landed => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: true,
                error: None,
            }),
            Ok(_) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some("edit did not land".into()),
            }),
            Err(e) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(e),
            }),
        }
    }
    Ok(rows)
}

// ─── Loudness audit ──────────────────────────────────────────────────────────────

/// Re-amp each selected preset and flag clipping + loudness outliers (vs the
/// median), the gain-stage audit (MEASURE — drives the device; HW-pending).
#[tauri::command]
pub(crate) async fn audit_loudness(
    app: tauri::AppHandle,
    slots: Vec<u32>,
    topology_id: Option<String>,
    outlier_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<lint::Finding>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let mut measures = Vec::with_capacity(slots.len());
        for slot in slots {
            let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
            let peak = samples.iter().fold(0f32, |m, &s| m.max(s.abs()));
            let peak_dbfs = if peak > 0.0 {
                20.0 * (peak as f64).log10()
            } else {
                -120.0
            };
            let loud = lufs::measure_mono(&samples, rate)?;
            measures.push(lint::AuditMeasure {
                list_index: slot,
                peak_dbfs,
                loudness_lufs: loud.integrated_lufs,
            });
        }
        Ok(lint::audit_measures(&measures, outlier_lu))
    })
    .await
}
/// List the bulk-run backup snapshots on disk (newest first), as file paths — the
/// audit trail of what `bulk_apply` snapshotted. A missing dir = no backups.
#[tauri::command]
pub(crate) fn list_snapshots(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    Ok(backup::list_snapshots_in_dir(&backup::backups_dir(&app)?)?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}
