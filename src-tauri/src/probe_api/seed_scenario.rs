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

/// Seed-owned ownership markers that SURVIVE a device import — `info.preset_id`
/// cannot serve (the device stamps a fresh uuid on import, HW 2026-07-17):
/// the fixture's `info.source_id` stamp + the Reference's scene-uuid prefix
/// (the latter also covers pre-stamp legacy copies).
const FIXTURE_MARKERS: [&str; 2] = ["tmp-companion-e2e-fixture", "e2e00000-"];

/// Substring probe (truncation-proof vs the field-8 partial). Pure.
fn is_fixture_body(bytes: &[u8]) -> bool {
    let body = String::from_utf8_lossy(bytes);
    FIXTURE_MARKERS.iter().any(|m| body.contains(m))
}

/// Field-8-read `device_slot` (1-based) and require a fixture marker — the one
/// ownership probe the sweep and the target classification share.
fn slot_is_fixture_owned(s: &mut Session, device_slot: u32) -> bool {
    matches!(s.read_slot_preset_json(device_slot), Ok(Some(bytes)) if is_fixture_body(&bytes))
}

/// Clear every stray on the GIVEN session — but only after a per-candidate
/// field-8 read finds a [`FIXTURE_MARKERS`] hit (a name is not ownership; a
/// user preset coincidentally named "E2E Reference" is skipped, fail-closed).
/// One session for reads+clears (each extra open risks the post-close lockout);
/// settles after the last clear (the device's list lags its own writes).
fn sweep_on(
    s: &mut Session,
    list: &[session::PresetEntry],
    spec: &[ScenarioPreset],
) -> Result<Vec<u32>, String> {
    let strays = scenario_strays(list, spec);
    if strays.is_empty() {
        return Ok(Vec::new());
    }
    // Field-8 reads on a mid-flood line are dropped device-side — drain first.
    s.drain_until_quiet(250, 20)?;
    let mut swept = Vec::new();
    for (slot, name) in strays {
        let owned = slot_is_fixture_owned(s, slot + 1);
        if !owned {
            eprintln!(
                "[seed] slot {slot} ({name:?}) matches a scenario name but not a \
                 fixture content marker — leaving it untouched"
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

/// TOLERANT list read + a full-bank completeness floor. Tolerant because the
/// strict harvest fails on interleaved back-to-back-session responses (see the
/// CLAUDE.md 0xe00002c5 entry); the floor is the real safety — a partial view
/// must never drive clears or imports, and truncation is tail-only, so a
/// length check IS the completeness check.
const MY_PRESETS_BANK_SIZE: usize = 504; // fw 1.8.45; fail-loud if a fw rev resizes the bank

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

    // Classify every TARGET before ANY import: seedable = empty (or swept this
    // run); skippable = verified fixture (name + marker — a name-only skip
    // would bless a user preset and hand it to teardown's clear); anything
    // else aborts before `replace_inplace_with` can overwrite user data.
    s.drain_until_quiet(250, 20)?;
    let mut to_seed: Vec<&ScenarioPreset> = Vec::new();
    for p in &spec {
        let entry = list.iter().find(|e| e.slot == p.list_index);
        let empty = swept.contains(&p.list_index)
            || entry.is_none_or(|e| session::is_empty_slot_name(&e.name));
        if empty {
            to_seed.push(p);
            continue;
        }
        let e = entry.expect("occupied entries exist in the floored list");
        let owned = e.name == p.name && slot_is_fixture_owned(&mut s, p.list_index + 1);
        if !owned {
            return Err(format!(
                "target slot {} is occupied by {:?} and does not carry a fixture \
                 content marker — refusing to seed over it (move that preset, then rerun)",
                p.list_index, e.name
            ));
        }
        // Verified fixture already in place — nothing to redo for this slot.
    }
    drop(s);

    let mut seeded = Vec::new();
    for p in to_seed {
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

    /// A fixture regen that drops the `source_id` stamp must fail here, not on
    /// the unit (the guards would refuse to manage unmarked copies).
    #[test]
    fn committed_fixtures_carry_an_ownership_marker() {
        let spec = scenario_spec().expect("committed spec parses");
        assert_eq!(spec.len(), 3);
        for p in &spec {
            assert!(
                is_fixture_body(p.preset_json.as_bytes()),
                "{} carries no fixture marker",
                p.name
            );
        }
    }
}
