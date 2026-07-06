//! OFFLINE library import + bulk-run apply/revert Tauri commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ─── OFFLINE library + bulk-run commands ─────────────────────────────────────────

/// Block-category map for facet indexing. TODO: derive amp/cab categories
/// from the firmware `product_profile.json`; until then facets index blocks/IRs/SICs
/// /name/level/template (everything except the amp/cab classification).
pub(crate) fn library_categories() -> search::CategoryMap {
    search::CategoryMap::new()
}

/// Zero-padded epoch-millis id for a bulk run (sortable, collision-free per ms).
fn now_stamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("run-{ms:020}")
}

/// Build `PresetTarget`s for the selected list indices from the imported library.
/// Errors if any index is not a matched, writable record (the destructive-op guard:
/// unmatched/ambiguous records have `list_index = None` and never reach here).
pub(crate) fn targets_from_library(
    lib: &library::Library,
    selection: &[u32],
) -> Result<Vec<bulkrun::PresetTarget>, String> {
    selection
        .iter()
        .map(|&idx| {
            lib.records
                .iter()
                .find(|r| r.list_index == Some(idx))
                .ok_or_else(|| {
                    format!("list index {idx} is not a matched, writable library record")
                })
                .and_then(|r| r.to_target())
        })
        .collect()
}

/// Construct the right `PresetIo` for a run's path (built fresh per run; both are
/// stateless and open their own device connections).
pub(crate) fn io_for_path(path: bulk_cmd::IoPath) -> Box<dyn bulkrun::PresetIo> {
    match path {
        bulk_cmd::IoPath::Live => Box::new(preset_io::LiveIo),
        bulk_cmd::IoPath::Offline => Box::new(preset_io::OfflineIo),
    }
}

/// Import a folder of Pro-Control-exported `.preset` files as the canonical library
/// and reconcile it against the live device slot list. Returns the reconcile report
/// (matched / unmatched / ambiguous) for the user to confirm before any write.
#[tauri::command]
pub(crate) async fn import_library(
    folder: String,
    state: State<'_, AppState>,
) -> Result<library::ReconcileReport, String> {
    let folder_path = std::path::PathBuf::from(&folder);
    let (mut records, errors) =
        library::load_library_from_dir(&folder_path, &library_categories())?;
    if !errors.is_empty() {
        log::warn!(
            "[import_library] {} file(s) skipped: {errors:?}",
            errors.len()
        );
    }
    let device_list: Vec<PresetEntry> = with_released_seize(state.session.clone(), || {
        Session::connect()?.list_my_presets()
    })
    .await
    .unwrap_or_default();
    let report = library::reconcile_with_device(&mut records, &device_list);
    *lock_ok(&state.library) = Some(library::Library {
        folder: folder_path,
        records,
    });
    Ok(report)
}

/// The imported library's records (decoded JSON omitted from the wire — it's large
/// and the UI doesn't need it; bulk ops resolve it backend-side).
#[tauri::command]
pub(crate) fn library_records(
    state: State<'_, AppState>,
) -> Result<Vec<library::LibraryRecord>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard
        .as_ref()
        .ok_or("no library imported — import a .preset folder first")?;
    Ok(lib.records.clone())
}

/// Filter args mirroring `search::Filter` (which has no serde derive).
#[derive(serde::Deserialize, Default)]
pub(crate) struct FilterArgs {
    name_substr: Option<String>,
    amp: Option<String>,
    block: Option<String>,
    ir: Option<String>,
    sic: Option<String>,
    level_lt: Option<f64>,
    level_gt: Option<f64>,
}

/// List indices of library records matching `filter` (selection feeder).
/// Only **writable** (matched) records are returned — unmatched/ambiguous ones
/// (sentinel `u32::MAX`) are dropped so they can't be selected for a write.
#[tauri::command]
pub(crate) fn library_filter(
    filter: FilterArgs,
    state: State<'_, AppState>,
) -> Result<Vec<u32>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let f = search::Filter {
        name_substr: filter.name_substr,
        amp: filter.amp,
        block: filter.block,
        ir: filter.ir,
        sic: filter.sic,
        level_lt: filter.level_lt,
        level_gt: filter.level_gt,
    };
    let records: Vec<search::PresetRecord> =
        lib.records.iter().map(|r| r.search_record()).collect();
    Ok(search::filter_records(&records, &f)
        .into_iter()
        .map(|r| r.list_index)
        .filter(|&i| i != u32::MAX)
        .collect())
}

/// Preview a bulk operation over a selection — per-preset change list, writes nothing.
#[tauri::command]
pub(crate) fn bulk_dry_run(
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::DryRunEntry>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let targets = targets_from_library(lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(bulkrun::dry_run(&targets, operation.as_ref()))
}
#[derive(serde::Serialize)]
pub(crate) struct BulkApplyResult {
    run_id: String,
    report: bulkrun::RunReport,
}

/// Apply a bulk operation to a selection, snapshotting each preset first, and record
/// the run so it can be reverted. Runs with the app's HID seize released (the io
/// adapters open their own connections).
#[tauri::command]
pub(crate) async fn bulk_apply(
    app: tauri::AppHandle,
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    backup: bool,
    state: State<'_, AppState>,
) -> Result<BulkApplyResult, String> {
    let targets = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        targets_from_library(lib, &selection)?
    };
    if targets.is_empty() {
        return Err("selection is empty (no matched, writable presets)".into());
    }
    let io_path = op.io_path();
    let op_label = bulk_cmd::build_operation(&op)?.label();
    let backup_dir = if backup {
        backup::backups_dir(&app)?
    } else {
        std::env::temp_dir().join("tmp-companion-nobackup")
    };
    let run_id = now_stamp_id();

    // Build the operation INSIDE the blocking closure (OpSpec is Send; a boxed
    // Operation is not), so the closure stays Send for spawn_blocking.
    let report = with_released_seize(state.session.clone(), move || {
        let operation = bulk_cmd::build_operation(&op)?;
        let mut io = io_for_path(io_path);
        Ok(bulkrun::apply(
            &targets,
            operation.as_ref(),
            io.as_mut(),
            &backup_dir,
        ))
    })
    .await?;

    lock_ok(&state.runs).insert(
        run_id.clone(),
        bulk_cmd::StoredRun {
            report: report.clone(),
            op_label,
        },
    );
    Ok(BulkApplyResult { run_id, report })
}

/// Revert a recorded run — restore every touched preset to its pre-run snapshot.
#[tauri::command]
pub(crate) async fn bulk_revert(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::RevertEntry>, String> {
    let stored = {
        let guard = lock_ok(&state.runs);
        guard
            .get(&run_id)
            .cloned()
            .ok_or_else(|| format!("no run {run_id} to revert"))?
    };
    let report = stored.report;
    with_released_seize(state.session.clone(), move || {
        // Revert ALWAYS restores via OFFLINE re-import of the captured original
        // `.preset` (the snapshot is a complete offline-file). This is correct for
        // both forward paths: a LIVE (ParamEdit) run can't be reverted by `LiveIo`
        // — its `write` diffs before-vs-after, and on revert both are the snapshot,
        // so the diff is empty and the device keeps the applied change while falsely
        // reporting `restored`. OfflineIo re-imports the original bytes in place
        // (identity-preserving), which undoes either forward path.
        let mut io = io_for_path(bulk_cmd::IoPath::Offline);
        Ok(bulkrun::revert(&report, io.as_mut()))
    })
    .await
}

pub(crate) fn format_dry_run(entries: &[bulkrun::DryRunEntry]) -> String {
    let mut s = format!("[probe bulk dry-run] {} preset(s)\n", entries.len());
    for e in entries {
        s += &format!(
            "  slot {:>3} {:<24} {:?}{}\n",
            e.list_index,
            e.display_name,
            e.status,
            match &e.error {
                Some(err) => format!("  ERROR: {err}"),
                None if !e.changes.is_empty() => format!("  ({} field(s) change)", e.changes.len()),
                None => String::new(),
            }
        );
    }
    s
}
