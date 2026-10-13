//! `probe --import-reload-check <file.preset>`: does an import that lands on the
//! CURRENTLY LOADED slot reload it? fw 1.8.58 skips a same-slot `loadPreset` while the
//! working copy is clean, so if an import onto the loaded slot did NOT reload it, a
//! following same-slot load would be a no-op and the old working copy could be saved back
//! over the import. Static RE says the firmware reloads that slot itself; one HW check on
//! 2026-10-11 agreed. This arm makes the check repeatable, as `probe --restore-check` is
//! for the same-slot restore.
//!
//! Sequence: predict the landing slot (the lowest empty one) and refuse unless its stored
//! row is the empty template → load it and confirm the working copy is that empty slot →
//! import the file, renamed [`CHECK_NAME`], with NO further load → read the working copy
//! on the import's own connection and on a fresh one → prove the landing from the stored
//! row → clear the slot only when both its stored row and its list entry carry
//! [`CHECK_NAME`] → reload the preset that was active at the start.
//!
//! It saves nothing, never engages re-amp, and logs any `importPresetResponse` (118) that
//! arrives within ~5 s of the import, which settles whether the echo is ever sent.

use crate::replace_inplace::{guarded_clear, StoredProof};
use crate::session::{self, Session};
use serde_json::Value;

/// The name the imported copy carries, so the cleanup can never clear anything else.
const CHECK_NAME: &str = "Import Reload Check";
/// How long after the import the echo is waited for, in total.
const ECHO_LOG_MS: u64 = 5_000;

/// What one working-copy read showed.
#[derive(Debug, PartialEq)]
struct Seen {
    name: Option<String>,
    preset_id: Option<String>,
    blocks: usize,
}

impl Seen {
    fn of(doc: &Value) -> Self {
        Seen {
            name: doc
                .pointer("/info/displayName")
                .and_then(Value::as_str)
                .map(str::to_string),
            preset_id: crate::library::preset_id_of(doc).map(str::to_string),
            blocks: block_count(doc),
        }
    }

    /// The imported body: the check's name, the file's id and its block count.
    fn is_import(&self, file_id: Option<&str>, blocks: usize) -> bool {
        self.name.as_deref() == Some(CHECK_NAME)
            && self.preset_id.as_deref() == file_id
            && self.blocks == blocks
    }

    /// The empty template, as a loaded empty slot shows it.
    fn is_template(&self) -> bool {
        self.preset_id.as_deref() == Some(crate::library::TEMPLATE_PRESET_ID) && self.blocks == 0
    }
}

fn block_count(doc: &Value) -> usize {
    session::extract_active_graph(doc, None).nodes.len()
}

/// The file, renamed [`CHECK_NAME`]. Refuses a block-less file: its import could not be
/// told apart from the empty template it lands on.
fn renamed_check_body(file: &Value) -> Result<(Value, usize), String> {
    let mut doc = file.clone();
    *doc.pointer_mut("/info/displayName")
        .ok_or("the preset file has no info.displayName")? = Value::from(CHECK_NAME);
    let blocks = block_count(&doc);
    if blocks == 0 {
        return Err(
            "the preset file has no blocks, so its import could not be told from the empty \
             template — pass a fixture with blocks"
                .to_string(),
        );
    }
    Ok((doc, blocks))
}

fn working_copy(s: &mut Session) -> Result<Seen, String> {
    Ok(Seen::of(&s.live_complete_value()?))
}

pub fn probe_import_reload_check(path: &str) -> Result<String, String> {
    let raw = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let file: Value = crate::library::decode_preset_bytes(&raw)
        .and_then(|t| serde_json::from_str(&t).map_err(|e| format!("preset JSON: {e}")))?;
    let (doc, blocks) = renamed_check_body(&file)?;
    let file_id = crate::library::preset_id_of(&doc).map(str::to_string);
    let bytes = crate::backup::xor_jld(
        serde_json::to_string(&doc)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    );

    // Where the unit started, and the predicted landing slot (refused unless template).
    let (start, list) = {
        let mut s = Session::connect()?;
        let list = s.list_my_presets()?;
        (s.active_slot_live(), list)
    };
    let stored = StoredProof::before(
        &doc,
        list.iter()
            .filter(|p| session::is_empty_slot_name(&p.name))
            .map(|p| p.slot),
    )?;
    let landing = stored.slot();
    let mut out = format!(
        "[probe --import-reload-check] file={path} ({blocks} blocks), landing list {landing} \
         (device slot {}); started on list {start:?}\n",
        landing.device_slot()
    );

    let res = (|| -> Result<bool, String> {
        // Load the empty landing slot and confirm it is what the unit now holds.
        Session::connect()?.load_preset(landing)?;
        let mut s = Session::connect()?;
        let active = s.active_slot_live();
        let before = working_copy(&mut s)?;
        drop(s);
        out.push_str(&format!(
            "  loaded the landing slot: active {active:?}, working copy {before:?}\n"
        ));
        if active != Some(landing) || !before.is_template() {
            return Err("the landing slot did not load as the empty template".to_string());
        }

        // Import with no further load; read the working copy on the same connection.
        let mut s = Session::connect()?;
        let echo = s.import_preset_echo(&bytes)?;
        let echo = match echo {
            Some(e) => Some(e),
            None => s.await_import_echo(ECHO_LOG_MS)?,
        };
        let same = working_copy(&mut s)?;
        drop(s);
        let fresh = working_copy(&mut Session::connect()?)?;
        out.push_str(&format!(
            "  importPresetResponse within ~{} s: {}\n  working copy, import's connection: {same:?}\n  working copy, fresh connection:    {fresh:?}\n",
            ECHO_LOG_MS / 1000,
            echo.map_or("none".to_string(), |(l, sl)| format!("listEnum {l}, presetSlot {sl}")),
        ));
        stored.after(landing)?;
        out.push_str("  stored row: the import (preset_id and name match)\n");
        let reloaded = same.is_import(file_id.as_deref(), blocks)
            && fresh.is_import(file_id.as_deref(), blocks);
        out.push_str(&format!(
            "  VERDICT: the import {} the loaded slot\n",
            if reloaded {
                "RELOADED"
            } else {
                "did NOT reload"
            }
        ));
        Ok(reloaded)
    })();

    // Clear the landing slot only when its stored row AND its list entry are the check's.
    let cleanup = crate::read_slot_preset_parsed(landing).and_then(|(row, _, _)| {
        match row.pointer("/info/displayName").and_then(Value::as_str) {
            Some(CHECK_NAME) => {
                guarded_clear(landing, CHECK_NAME).map(|()| format!("cleared list {landing}"))
            }
            n => Ok(format!(
                "list {landing}'s stored row is {n:?}, not the check's — left in place"
            )),
        }
    });
    out.push_str(&format!(
        "  cleanup: {}\n",
        cleanup.as_deref().unwrap_or_else(|e| e.as_str())
    ));
    let back = match start {
        Some(l) if l != landing => Session::connect()
            .and_then(|mut s| s.load_preset(l))
            .map(|()| format!("reloaded list {l}")),
        _ => Ok("starting preset unknown — left as is".to_string()),
    };
    out.push_str(&format!(
        "  end: {}\n",
        back.as_deref().unwrap_or_else(|e| e.as_str())
    ));
    match (res, cleanup, back) {
        (Ok(_), Ok(_), Ok(_)) => Ok(out),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(format!("{e}\n{out}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(nodes: Value) -> Value {
        json!({
            "info": {"displayName": "Fixture", "preset_id": "aaaaaaaa-0000-0000-0000-000000000408"},
            "audioGraph": {"guitarNodes": {"G1": nodes}},
        })
    }

    #[test]
    fn the_check_body_is_renamed_and_must_have_blocks() {
        let f = file(json!([{"FenderId": "ACD_Boost", "nodeId": "ACD_Boost"}]));
        let (doc, blocks) = renamed_check_body(&f).unwrap();
        assert_eq!(doc["info"]["displayName"], CHECK_NAME);
        assert_eq!(doc["info"]["preset_id"], f["info"]["preset_id"]);
        assert_eq!(blocks, block_count(&f));
        assert!(blocks > 0);
        let e = renamed_check_body(&file(json!([]))).unwrap_err();
        assert!(e.contains("no blocks"), "{e}");
    }

    #[test]
    fn verdicts_read_the_name_id_and_block_count() {
        let id = "aaaaaaaa-0000-0000-0000-000000000408";
        let import = Seen {
            name: Some(CHECK_NAME.into()),
            preset_id: Some(id.into()),
            blocks: 3,
        };
        assert!(import.is_import(Some(id), 3));
        assert!(!import.is_import(Some(id), 2));
        assert!(!import.is_import(Some("other"), 3));
        let template = Seen::of(&json!({
            "info": {"displayName": "Empty", "preset_id": crate::library::TEMPLATE_PRESET_ID},
            "audioGraph": {"guitarNodes": {}},
        }));
        assert!(template.is_template());
        assert!(!template.is_import(Some(id), 0));
    }
}
