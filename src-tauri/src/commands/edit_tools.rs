//! Block-template capture, variant creation, and bulk-rename commands.
#![allow(clippy::too_many_arguments)]
use crate::*;

// ─── Block templates (persisted in the app config dir, applied via OpSpec::ApplyBlock) ─

/// Path to the persisted block-template library (app config dir).
fn block_lib_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app config dir: {e}"))?
        .join("block_library.json"))
}

/// The saved block templates.
#[tauri::command]
pub(crate) fn list_block_templates(
    app: tauri::AppHandle,
) -> Result<Vec<blocklib::BlockTemplate>, String> {
    Ok(blocklib::load_library_from_path(&block_lib_path(&app)?).unwrap_or_default())
}

/// Capture the first block of `model` from a source preset into the library under
/// `name` (replaces a same-named entry). PURE capture + persist to disk; no device.
#[tauri::command]
pub(crate) fn save_block_template(
    app: tauri::AppHandle,
    source_list_index: u32,
    model: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Vec<blocklib::BlockTemplate>, String> {
    let template = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let v: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        blocklib::capture_block(&v, &model, &name)
            .ok_or_else(|| format!("source preset has no block of model {model:?}"))?
    };
    let path = block_lib_path(&app)?;
    let mut lib = blocklib::load_library_from_path(&path).unwrap_or_default();
    lib.retain(|t| t.name != name); // replace a same-named template
    lib.push(template);
    blocklib::save_library_to_path(&path, &lib)?;
    Ok(lib)
}

// ─── Create variants (create = append-import) ────────────────────────────────────

/// Serde-friendly mirror of `variants::VariantEdit`.
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub(crate) enum VariantEditArg {
    SetParam {
        model: String,
        param: String,
        value: f64,
    },
    ReplaceBlock {
        from: String,
        to: String,
    },
    SetBpm {
        bpm: f64,
    },
}

#[derive(serde::Deserialize, Clone)]
pub(crate) struct RecipeArg {
    name_suffix: String,
    edits: Vec<VariantEditArg>,
}
impl RecipeArg {
    fn to_recipe(&self) -> variants::Recipe {
        variants::Recipe {
            name_suffix: self.name_suffix.clone(),
            edits: self
                .edits
                .iter()
                .map(|e| match e.clone() {
                    VariantEditArg::SetParam {
                        model,
                        param,
                        value,
                    } => variants::VariantEdit::SetParam {
                        model,
                        param,
                        value,
                    },
                    VariantEditArg::ReplaceBlock { from, to } => {
                        variants::VariantEdit::ReplaceBlock { from, to }
                    }
                    VariantEditArg::SetBpm { bpm } => variants::VariantEdit::SetBpm(bpm),
                })
                .collect(),
        }
    }
}

/// Create a variant on the device (LIVE, append-only): clone + recipe -> .preset bytes
/// -> `import_preset` (the device files it at the next empty slot — variants carry no
/// inherited Song membership, so an append is correct, NOT an in-place overwrite).
/// HW-validation pending.
#[tauri::command]
pub(crate) async fn create_variant(
    source_list_index: u32,
    recipe: RecipeArg,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let variant_bytes = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let source: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        let variant = variants::apply_recipe(&source, &recipe.to_recipe())?;
        backup::xor_jld(
            serde_json::to_string(&variant)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )
    };
    with_released_seize(state.session.clone(), move || {
        let reported = Session::connect()?.import_preset(&variant_bytes)?;
        Ok(match reported {
            Some((le, slot)) => {
                format!("variant imported (device reported listEnum {le}, slot {slot})")
            }
            None => "variant imported (device gave no slot echo — re-list to confirm)".to_string(),
        })
    })
    .await
}

// ─── Bulk rename (apply = LIVE) ──────────────────────────────────────────────────

/// Max preset-name length for the rename validator. Generous pending an exact HW
/// limit (better not to falsely flag than to block a valid name).
const RENAME_MAX: usize = 60;

/// Serde-friendly mirror of `rename::RenameSpec` (which has no serde derive).
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub(crate) enum RenameSpecArg {
    FindReplace { from: String, to: String },
    Template { pattern: String },
    Number { width: usize, start: u32 },
}
impl RenameSpecArg {
    fn to_spec(&self) -> rename::RenameSpec {
        match self.clone() {
            RenameSpecArg::FindReplace { from, to } => rename::RenameSpec::FindReplace { from, to },
            RenameSpecArg::Template { pattern } => rename::RenameSpec::Template { pattern },
            RenameSpecArg::Number { width, start } => rename::RenameSpec::Number { width, start },
        }
    }
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct RenameRow {
    list_index: Option<u32>,
    name: String,
    new_name: String,
    /// Set on a no-op rename (new == old) or a validation failure; the row is then skipped on apply.
    note: Option<String>,
}

/// Compute new names for the selection (PURE — no device). The `{n}` token / Number
/// offset use each preset's position within the selection.
fn rename_rows(
    lib: &library::Library,
    selection: &[u32],
    spec: &rename::RenameSpec,
) -> Vec<RenameRow> {
    selection
        .iter()
        .enumerate()
        .filter_map(|(i, idx)| {
            let r = lib.records.iter().find(|r| r.list_index == Some(*idx))?;
            let new_name = rename::apply_rename(&r.display_name, i, spec);
            let note = if new_name == r.display_name {
                Some("unchanged".to_string())
            } else {
                rename::validate_name(&new_name, RENAME_MAX).err()
            };
            Some(RenameRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                new_name,
                note,
            })
        })
        .collect()
}

#[derive(serde::Serialize)]
pub(crate) struct RenameApplyRow {
    list_index: u32,
    new_name: String,
    applied: bool,
    error: Option<String>,
}

/// Apply the rename to each selected preset on the device (LIVE): per preset
/// `load_preset → rename_current_preset → save_current_preset` (the PC "save under a
/// new name" pair). Rows with a validation note / no-op are skipped.
#[tauri::command]
pub(crate) async fn bulk_rename(
    selection: Vec<u32>,
    spec: RenameSpecArg,
    state: State<'_, AppState>,
) -> Result<Vec<RenameApplyRow>, String> {
    let rows: Vec<RenameRow> = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        rename_rows(lib, &selection, &spec.to_spec())
    };
    // Only valid, changed, matched rows are applied.
    let jobs: Vec<(u32, String, String)> = rows
        .into_iter()
        .filter(|r| r.note.is_none())
        .filter_map(|r| r.list_index.map(|i| (i, r.name, r.new_name)))
        .collect();
    if jobs.is_empty() {
        return Err("nothing to rename (all rows unchanged or invalid)".into());
    }
    with_released_seize(state.session.clone(), move || {
        let mut out = Vec::new();
        for (idx, name_before, new_name) in jobs {
            let mut row = RenameApplyRow {
                list_index: idx,
                new_name: new_name.clone(),
                applied: false,
                error: None,
            };
            let res = (|| -> Result<(), String> {
                let mut s = Session::connect()?;
                // Load (accumulating the PresetLoaded echo) then CONFIRM the target is
                // active before renaming+saving — a dropped load would otherwise
                // rename+save the WRONG preset over this slot (same fix as the
                // single-preset rename_save_preset path).
                s.clear_raw();
                s.send_and_collect(&proto::load_preset((idx + 1) as u64, 1), 200)?;
                s.confirm_active(idx, Some(&name_before))?;
                s.rename_current_preset(&new_name)?;
                s.save_current_preset(idx)?; // persist (rename = save-under-new-name)
                Ok(())
            })();
            match res {
                Ok(()) => row.applied = true,
                Err(e) => row.error = Some(e),
            }
            out.push(row);
        }
        Ok(out)
    })
    .await
}
