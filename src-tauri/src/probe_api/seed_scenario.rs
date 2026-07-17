//! Online-e2e scenario seeding — sweep stray imports, then place the three committed
//! scenario presets at their slots. Shared by `probe --seed-scenario` (a FRESH process
//! per seed, invoked by the runner BEFORE the bridge server starts — keeps the seed's
//! many fresh connections clear of the in-process `0xe00002c5` open lockout that
//! aborted the original in-spec seeds) and by the `e2e_seed_scenario` bridge command
//! (the in-process fallback for specs run without the runner).

use crate::backup;
use crate::replace_inplace::replace_inplace_with;
use crate::session::{self, Session};

#[derive(serde::Deserialize)]
pub(crate) struct ScenarioPreset {
    #[serde(rename = "listIndex")]
    pub list_index: u32,
    pub name: String,
    #[serde(rename = "presetJson")]
    preset_json: String,
}

/// The committed scenario-preset spec (`e2e/fixtures/scenario-presets.json`,
/// overridable via `TMP_E2E_SCENARIO_PRESETS`) — the one source of truth for the
/// seed, the presence checks, and the stray sweep.
pub(crate) fn scenario_spec() -> Result<Vec<ScenarioPreset>, String> {
    let path = std::env::var("TMP_E2E_SCENARIO_PRESETS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/scenario-presets.json"
        )
        .into()
    });
    let raw = std::fs::read(&path).map_err(|e| format!("scenario presets {path}: {e}"))?;
    serde_json::from_slice(&raw).map_err(|e| format!("parse scenario presets: {e}"))
}

/// Entries holding a scenario NAME at the wrong slot — leftovers of a seed aborted
/// between its import and its guarded scratch-clear (the import lands at the first
/// EMPTY slot, so each aborted run strands one copy in the user's bank; HW-observed:
/// 13 stray "E2E Reference" copies accumulated at list indices 27–39 across failed
/// runs, and the duplicates then broke the next seed's landing detection). Pure.
fn scenario_strays(list: &[session::PresetEntry], spec: &[ScenarioPreset]) -> Vec<(u32, String)> {
    list.iter()
        .filter(|e| {
            spec.iter()
                .any(|p| e.name == p.name && e.slot != p.list_index)
        })
        .map(|e| (e.slot, e.name.clone()))
        .collect()
}

/// The fixture `info.preset_id` values carried by the committed scenario presets —
/// the seed-owned ownership marker the stray sweep requires before clearing a slot.
/// Pure.
fn spec_preset_ids(spec: &[ScenarioPreset]) -> Vec<String> {
    spec.iter()
        .filter_map(|p| {
            let v: serde_json::Value = serde_json::from_str(&p.preset_json).ok()?;
            crate::library::preset_id_of(&v).map(str::to_string)
        })
        .collect()
}

/// Clear every stray on the GIVEN session. Two guards, both in the same list-index
/// address space as the clears: (1) the completeness-floored list taken seconds
/// before on the SAME connection classifies the candidates; (2) a per-candidate
/// field-8 read must find a FIXTURE `preset_id` in the slot before it is cleared —
/// a display name is not an ownership marker, so a real user preset that happens to
/// be named "E2E Reference" (its own uuid) is skipped, never deleted. A candidate
/// whose read fails or mismatches is left untouched (fail-closed) and reported.
/// A per-stray fresh reconnect would be safer-looking but each extra open is
/// another chance to land in the post-close open lockout. Settles after the last
/// clear so a follow-up list read reflects the freed slots (clears are
/// fire-and-forget and the device's list lags its own writes).
fn sweep_on(
    s: &mut Session,
    list: &[session::PresetEntry],
    spec: &[ScenarioPreset],
) -> Result<Vec<u32>, String> {
    let fixture_ids = spec_preset_ids(spec);
    let mut swept = Vec::new();
    for (slot, name) in scenario_strays(list, spec) {
        // The field-8 partial always contains the leading `info` block, so a
        // plain substring probe is truncation-proof; parsing isn't needed.
        let owned = match s.read_slot_preset_json(slot + 1) {
            Ok(Some(bytes)) => {
                let body = String::from_utf8_lossy(&bytes);
                fixture_ids.iter().any(|id| body.contains(id.as_str()))
            }
            Ok(None) | Err(_) => false,
        };
        if !owned {
            eprintln!(
                "[seed] slot {slot} ({name:?}) matches a scenario name but not the \
                 fixture preset_id — leaving it untouched"
            );
            continue;
        }
        s.clear_user_preset(slot)?;
        swept.push(slot);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if !swept.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(1_500));
    }
    Ok(swept)
}

/// Standalone stray sweep (one fresh session): the teardown/recovery arm.
pub(crate) fn sweep_strays_core() -> Result<Vec<u32>, String> {
    let spec = scenario_spec()?;
    let mut s = Session::connect()?;
    let list = read_full_list(&mut s)?;
    sweep_on(&mut s, &list, &spec)
}

/// TOLERANT list read + a hard completeness floor. Tolerant, not strict: the strict
/// harvest decodes only terminal-frame streams and FAILS on the interleaved mid-flood
/// responses back-to-back lean sessions produce (HW-observed on a healthy device:
/// tolerant read 504/504 every time while strict returned "no PresetListResponse" or
/// truncated 190–236-record fallbacks, and its re-arm retries left the line in a
/// state that armed the open lockout for the following attempts). The floor is the
/// actual safety: a partial view must never drive clears or imports — index 400
/// "missing" reads as out-of-range / every high slot reads as empty, and a
/// tail-truncated list would hide strays above the cut from the sweep. Truncation
/// is tail-only (present entries keep correct slots — `preset_entries` derives
/// each slot from its index), so a full-bank length check IS the completeness
/// check.
const MY_PRESETS_BANK_SIZE: usize = 504; // fw 1.8.45 My-Presets bank; fail-loud if a fw rev resizes it

fn read_full_list(s: &mut Session) -> Result<Vec<session::PresetEntry>, String> {
    let list = s.list_my_presets()?;
    if list.len() < MY_PRESETS_BANK_SIZE {
        return Err(format!(
            "preset list read truncated ({} of {MY_PRESETS_BANK_SIZE} entries) — refusing \
             to seed on a partial view of the bank",
            list.len()
        ));
    }
    Ok(list)
}

pub(crate) struct SeedOutcome {
    /// Slots freed by the stray sweep.
    pub swept: Vec<u32>,
    /// Scenario slots imported this run (already-correct slots are skipped).
    pub seeded: Vec<u32>,
}

/// Sweep strays, then place each missing scenario preset in-place at its slot.
pub(crate) fn seed_scenario_core() -> Result<SeedOutcome, String> {
    let spec = scenario_spec()?;
    let mut s = Session::connect()?;
    let list = read_full_list(&mut s)?;
    let swept = sweep_on(&mut s, &list, &spec)?;
    drop(s);

    let mut seeded = Vec::new();
    for p in &spec {
        // Per-preset presence skip: a partially-seeded bank only redoes the gaps.
        if list
            .iter()
            .any(|e| e.slot == p.list_index && e.name == p.name)
        {
            continue;
        }
        if !seeded.is_empty() {
            // Quiet gap between imports: each lands via several fresh connections
            // (import → landing read → load/confirm/save → guarded clear), and the
            // device needs the gap for its read-after-write list propagation.
            std::thread::sleep(std::time::Duration::from_secs(8));
        }
        // A `.preset` file is `xor_jld(compact JSON)`; `import_preset` adds the outer
        // LZ4. Lean mode (no Song-binding/report reads): scratch slots have no Song
        // rows, and the seed must conserve the device's open/close budget.
        let bytes = backup::xor_jld(p.preset_json.as_bytes());
        replace_inplace_with(p.list_index, &bytes, false)?;
        seeded.push(p.list_index);
    }
    Ok(SeedOutcome { swept, seeded })
}

/// `probe --seed-scenario` — fresh-process seed for the online e2e runner.
pub fn probe_seed_scenario() -> Result<String, String> {
    let o = seed_scenario_core()?;
    Ok(format!(
        "[probe --seed-scenario] swept strays at {:?}; imported slots {:?}\n",
        o.swept, o.seeded
    ))
}

/// `probe --clear-strays` — attended stray cleanup without seeding.
pub fn probe_clear_strays() -> Result<String, String> {
    let swept = sweep_strays_core()?;
    Ok(format!(
        "[probe --clear-strays] swept strays at {swept:?}\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stray classifier flags scenario names at the WRONG slot only — the
    /// legitimate scenario slots and real user presets are never candidates (the HW
    /// incident: 13 stray "E2E Reference" copies stranded at 27–39 by aborted seeds).
    #[test]
    fn scenario_strays_flags_wrong_slot_copies_only() {
        let spec: Vec<ScenarioPreset> = serde_json::from_str(
            r#"[
                {"listIndex": 400, "name": "E2E Reference", "presetJson": ""},
                {"listIndex": 401, "name": "E2E Target 1", "presetJson": ""}
            ]"#,
        )
        .expect("spec json");
        let entry = |slot: u32, name: &str| session::PresetEntry {
            slot,
            name: name.into(),
        };
        let list = vec![
            entry(27, "E2E Reference"),  // stray (aborted-seed leftover)
            entry(39, "E2E Reference"),  // stray
            entry(40, "Guitar Boost"),   // real preset — untouched
            entry(400, "E2E Reference"), // legitimate scenario slot
            entry(401, "E2E Reference"), // scenario NAME at another scenario's slot → stray
            entry(402, "--"),            // empty
        ];
        let strays = scenario_strays(&list, &spec);
        assert_eq!(
            strays,
            vec![
                (27, "E2E Reference".to_string()),
                (39, "E2E Reference".to_string()),
                (401, "E2E Reference".to_string()),
            ]
        );
        // No spec → nothing is ever a stray.
        assert!(scenario_strays(&list, &[]).is_empty());
    }
}
