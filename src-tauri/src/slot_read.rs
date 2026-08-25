//! The truncation-aware saved-preset read seam: field-8 first, falling back to a
//! name-guarded device-backup transport when a caller's REQUIRED tail sections are cut off
//! the field-8 read. Shared by the footswitch/scene leveling commands and ~20 `probe_api`
//! call sites via the crate-root re-export.
use crate::*;

/// Read a slot's field-8 preset JSON on a fresh quiet session and return the parsed preset, a
/// DIAGNOSTIC "has FS scenes" flag (`Some(empty)` = definitely no FS scenes; truncated/unknown or
/// non-empty → conservative `true`) and the raw byte length. Shared by the footswitch leveling
/// command + probes (the connect→drain→read→parse→scene-check boilerplate). The flag is NOT a
/// gate: `footswitch::plan_footswitch_jobs`'s assign gate decides bake-vs-assign purely off
/// whether the switch already carries a `param` fn for the selected control — it doesn't read
/// scene data at all — so this flag only ever informs `probe --fs-list`'s printout.
pub(crate) fn read_slot_preset_parsed(
    slot: u32,
) -> Result<(serde_json::Value, bool, usize), String> {
    let (preset, has_fs_scenes, len, _) = read_slot_preset_sections(slot, &[])?;
    Ok((preset, has_fs_scenes, len))
}

/// [`read_slot_preset_parsed`] plus WHICH of `required` tail sections the read did not
/// deliver whole (`session::json_section_complete`), and the occupant's name from the
/// same session's preset list.
///
/// A large preset's field-8 read comes back TAIL-TRUNCATED and `tolerant_parse_json`
/// salvages the prefix — so the document parses and a tail section (`ftsw`, `scenes`) is
/// simply absent or a short prefix. A caller that reads one of those to decide what to
/// DO then acts on "this preset has no footswitches" for a preset that has ten. The
/// truncation is per-slot-DETERMINISTIC, so re-reading the same slot cannot lengthen it
/// (`notes/gotchas.md`'s field-8 entry) — the choices are the device backup or refusing,
/// never a retry.
///
/// SOFT form: it reports and does not decide. Use it where the answer is best-effort
/// (`level_preset`'s base isolation reads `presetLevel` and `lastLoadedScene` out of the
/// SAME body, both of which survive a cut that takes `ftsw`).
pub(crate) fn read_slot_preset_sections(
    slot: u32,
    required: &[&str],
) -> Result<(serde_json::Value, bool, usize, TailRead), String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(slot + 1)?
        .ok_or_else(|| format!("no preset data for slot {}", slot + 1))?;
    let text = String::from_utf8_lossy(&json);
    let preset = session::tolerant_parse_json(&text)
        .ok_or_else(|| "preset JSON did not parse".to_string())?;
    let has_fs_scenes = session::scene_names_from_slot_json(&json).is_none_or(|n| !n.is_empty());
    let truncated: Vec<String> = required
        .iter()
        .filter(|k| !session::json_section_complete(&text, k))
        .map(|k| (*k).to_string())
        .collect();
    // The occupant's name, read from the LIST rather than the body: `info` is exactly
    // the kind of tail a cut that takes `ftsw` also takes, so the partial cannot always
    // name itself — and the name is what guards the backup fallback's slot mapping.
    // SNAPSHOT FIRST, live list only as a fallback. The live read runs on THIS session,
    // which has just streamed a large preset body — precisely the back-to-back shape the
    // list reassembly is documented to come back short on — and a short list means no
    // entry for this slot, an empty name, and a hard refusal from
    // `read_slot_preset_complete` even though the backup carries the preset perfectly.
    // That is the reported "couldn't read this preset's controls" on the Hiwatt (HW,
    // 2026-08-19: "the preset list did not answer with the slot's name"). The startup
    // snapshot is the same list the UI shows and costs no device I/O.
    let name = if truncated.is_empty() {
        String::new() // nothing to address a fallback with — not read at all
    } else {
        crate::monitor::startup_preset_name(slot)
            .filter(|n| !n.is_empty())
            .or_else(|| {
                s.list_my_presets()
                    .ok()
                    .and_then(|l| l.into_iter().find(|e| e.slot == slot))
                    .map(|e| e.name)
            })
            .unwrap_or_default()
    };
    Ok((
        preset,
        has_fs_scenes,
        json.len(),
        TailRead { truncated, name },
    ))
}

/// What [`read_slot_preset_sections`] found about the required tail sections.
pub(crate) struct TailRead {
    /// The required sections the read did NOT deliver whole (empty = all present).
    pub(crate) truncated: Vec<String>,
    /// The preset list's name for this slot — the backup fallback's address-space
    /// guard. Empty when nothing was truncated (never read) or the list read failed.
    pub(crate) name: String,
}

/// The field-8 read for a caller whose ANSWER DEPENDS on a tail section: it either
/// returns a document that carries every `required` section whole, or it fails.
///
/// On a truncated tail it re-reads the preset off a DEVICE BACKUP — the only transport
/// that carries the complete document (`backup_read::preset_json_from_backup`,
/// name-guarded against the preset list) — and replaces the WHOLE document with it
/// rather than grafting the missing section on: the same read is also the caller's
/// audioGraph and scene-overlay source, and the backup doc is complete on every axis.
/// Both transports read SAVED state, so the two are equivalent in freshness.
///
/// Never fires offline: the trigger is section completeness and the SimDevice serves its
/// committed bodies whole.
pub(crate) fn read_slot_preset_complete(
    slot: u32,
    required: &[&str],
) -> Result<(serde_json::Value, bool, usize), String> {
    let (preset, has_fs_scenes, len, tail) = read_slot_preset_sections(slot, required)?;
    if tail.truncated.is_empty() {
        return Ok((preset, has_fs_scenes, len));
    }
    if tail.name.is_empty() {
        return Err(format!(
            "slot {}: the preset is too large to read over USB — its {} section(s) were cut \
             off the field-8 read, and the preset list did not answer with the slot's name, \
             so a backup re-read cannot be addressed safely. Refusing rather than acting on \
             a partial preset",
            slot + 1,
            tail.truncated.join(", ")
        ));
    }
    log::warn!(
        "slot {} ({:?}): field-8 read truncated before its {} section(s) ({len} B) — \
         re-reading the complete preset off a device backup",
        slot + 1,
        tail.name,
        tail.truncated.join(", ")
    );
    // Its own fresh connection: the backup is a multi-second whole-library transfer and
    // the re-amp rules keep it off any held session.
    crate::settle(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let blob = {
        let mut s = Session::connect()?;
        s.device_backup(60, |_| {})?.0
    };
    let doc = backup_read::preset_json_from_backup(&blob, i64::from(slot) + 1, &tail.name)
        .map_err(|e| {
            format!(
                "slot {}: the preset is too large to read over USB — its {} section(s) were cut \
                 off the field-8 read, and the complete backup re-read failed ({e}). Refusing \
                 rather than acting on a partial preset",
                slot + 1,
                tail.truncated.join(", ")
            )
        })?;
    let len = doc.to_string().len();
    // Same rule as `scene_names_from_slot_json` (which maps `scenes[].sceneName`, so its
    // emptiness IS the array's) — but read off a COMPLETE document, so an absent `scenes`
    // key here means the preset carries none rather than "the read stopped short".
    let has_fs_scenes = doc
        .get("scenes")
        .and_then(|s| s.as_array())
        .is_none_or(|a| !a.is_empty());
    Ok((doc, has_fs_scenes, len))
}
