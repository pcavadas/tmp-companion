//! Online-e2e scenario seeding — sweep stray imports, then place the committed
//! scenario presets at their slots (every entry of `scenario-presets.json`, 400-404). Shared by `probe --seed-scenario` (a FRESH process
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
    pub(crate) preset_json: String,
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

/// The CURRENT fixture revision (`info.source_id`, which survives a device import).
/// BUMP the `#rN` suffix on ANY fixture regen: with the fixtures RESIDENT on the unit
/// between runs, a resident copy of an older rev fails the pristine check below and
/// self-migrates via one re-import — even when its `presetLevel` happens to match.
/// The ownership probe ([`FIXTURE_MARKERS`]) matches the version-less prefix, so
/// old-rev copies stay clearable/overwritable. `committed_fixtures_carry_an_ownership_marker`
/// pins every committed fixture to this exact stamp.
pub(crate) const FIXTURE_SOURCE_STAMP: &str = "tmp-companion-e2e-fixture#r2";

/// Substring probe (truncation-proof vs the field-8 partial). Pure.
fn is_fixture_body(bytes: &[u8]) -> bool {
    let body = String::from_utf8_lossy(bytes);
    FIXTURE_MARKERS.iter().any(|m| body.contains(m))
}

/// Field-8-read `device_slot` (1-based) and require a fixture marker — the one
/// ownership probe the sweep, the target classification, and the e2e clear
/// guard share. Callers must be on a QUIET line (`drain_until_quiet` first) —
/// a field-8 read fired mid-flood is dropped device-side.
pub(crate) fn slot_is_fixture_owned(s: &mut Session, device_slot: u32) -> bool {
    matches!(s.read_slot_preset_json(device_slot), Ok(Some(bytes)) if is_fixture_body(&bytes))
}

/// Substring-extract `audioGraph.presetLevel` (the one `presetLevel` key a
/// preset body carries). Substring, not serde: the field-8 read can return a
/// TAIL-TRUNCATED partial, and a full parse would fail on it even though the
/// early `audioGraph` object made it through. `None` on a body the level
/// never reached.
fn extract_preset_level(body: &[u8]) -> Option<f64> {
    let body = String::from_utf8_lossy(body);
    let rest = &body[body.find("\"presetLevel\":")? + "\"presetLevel\":".len()..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c)))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Pristine = the on-device body's `presetLevel` still matches the fixture's.
/// A strict-harness run LEVELS the fixtures with save — the ownership marker
/// survives that, so a marker-only skip hands the NEXT run pre-leveled state
/// (HW: the Hiwatt at 404 carried presetLevel 0.37495 vs the fixture's 0.5999
/// → the base lane skipped as "already at target" and the spec fast-failed).
/// Unreadable/truncated-past-the-level bodies count as NOT pristine: ownership
/// is already proven, so the worst case is a redundant re-import.
fn body_is_pristine(body: &[u8], fixture_json: &str) -> bool {
    // Rev gate first: a resident copy of an OLDER fixture revision is never pristine,
    // whatever its levels read (see `FIXTURE_SOURCE_STAMP`).
    if !String::from_utf8_lossy(body).contains(FIXTURE_SOURCE_STAMP) {
        return false;
    }
    match (
        extract_preset_level(body),
        extract_preset_level(fixture_json.as_bytes()),
    ) {
        (Some(dev), Some(fix)) => (dev - fix).abs() < 1e-3,
        _ => false,
    }
}

/// Does the field-8 body identify itself as `name`? Back-to-back slot reads on one
/// session can deliver the PREVIOUS request's body (HW: the unpaced classify loop
/// re-imported 400/401/402 on every seed from false `presetLevel` mismatches while a
/// genuinely drifted 404 once read "pristine" — wrong-slot bodies both ways). The
/// marker probe never noticed (every fixture body carries the marker); the pristine
/// compare is slot-SENSITIVE, so it may only act on a body that names this preset.
fn body_names(body: &[u8], name: &str) -> bool {
    String::from_utf8_lossy(body).contains(&format!("\"displayName\":\"{name}\""))
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

/// TOLERANT list read + an EXACT-bank-size gate. Tolerant because the strict
/// harvest fails on interleaved back-to-back-session responses (see the
/// CLAUDE.md 0xe00002c5 entry); the size gate is the real safety — a partial
/// view must never drive clears or imports (truncation is tail-only, so a
/// length check IS the completeness check), and a LARGER bank means a fw rev
/// moved the slot layout out from under our destructive slot assumptions.
const MY_PRESETS_BANK_SIZE: usize = 504; // fw 1.8.45; fail-loud if a fw rev resizes the bank

fn read_full_list(s: &mut Session) -> Result<Vec<session::PresetEntry>, String> {
    let list = s.list_my_presets()?;
    if list.len() != MY_PRESETS_BANK_SIZE {
        return Err(format!(
            "preset list size {} != the expected {MY_PRESETS_BANK_SIZE} (truncated read, or a \
             fw rev resized the bank) — refusing to seed on an unexpected bank shape",
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
/// `check_pristine` — ONLINE-only self-repair: re-import a marker-owned slot whose
/// saved `presetLevel` drifted from the fixture's (a prior attended run leveled it).
/// OFFLINE the specs level the SimDevice's slots as part of normal coverage, so the
/// same check would re-import mid-suite on every seed — polluting the events-equality
/// oracle and churning the sim — hence the flag, not an unconditional check.
pub(crate) fn seed_scenario_core(check_pristine: bool) -> Result<SeedOutcome, String> {
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
        let body = if e.name == p.name {
            // Pace the loop's reads apart — an immediate follow-on field-8 read can
            // answer with the PREVIOUS slot's body (see `body_names`).
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.read_slot_preset_json(p.list_index + 1).ok().flatten()
        } else {
            None
        };
        let Some(body) = body.filter(|b| is_fixture_body(b)) else {
            return Err(format!(
                "target slot {} is occupied by {:?} and does not carry a fixture \
                 content marker — refusing to seed over it (move that preset, then rerun)",
                p.list_index, e.name
            ));
        };
        // Verified fixture in place — but a PRIOR run may have leveled it with
        // save (the marker survives). Re-import a drifted body over itself; the
        // per-target `still_safe` recheck below already licenses that write. Only a
        // body that IDENTIFIES as this preset may drive the decision — a wrong-slot
        // body falls back to the marker-skip (the pre-pristine-check behavior)
        // instead of churning a redundant ~30 s re-import every seed.
        if !check_pristine {
            // Marker-verified and pristine-checking is off — the pre-check skip.
        } else if !body_names(&body, &p.name) {
            eprintln!(
                "[seed] slot {} ({:?}): field-8 body does not identify itself as this \
                 preset (stale/mismatched read) — keeping the marker-skip",
                p.list_index, p.name
            );
        } else if !body_is_pristine(&body, &p.preset_json) {
            eprintln!(
                "[seed] slot {} ({:?}) is fixture-owned but not pristine \
                 (presetLevel drifted) — re-importing",
                p.list_index, p.name
            );
            to_seed.push(p);
        }
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
        // Re-confirm THIS target in the SAME address space as the mutation,
        // immediately before it: the classification pass above ran off one
        // snapshot, but seeding multiple presets spans many seconds and several
        // connections per target — a later target's real state could have moved
        // since. `replace_inplace_with` itself only verifies the SCRATCH slot
        // before saving, never the destination, so this is the one guard.
        let mut s = Session::connect()?;
        let list = read_full_list(&mut s)?;
        let entry = list
            .iter()
            .find(|e| e.slot == p.list_index)
            .map(|e| e.name.clone());
        let still_safe = match &entry {
            None => true,
            Some(name) if session::is_empty_slot_name(name) => true,
            Some(name) if *name == p.name => slot_is_fixture_owned(&mut s, p.list_index + 1),
            Some(_) => false,
        };
        drop(s);
        if !still_safe {
            return Err(format!(
                "target slot {} changed since classification and is no longer safe \
                 to seed over — refusing (rerun to re-classify)",
                p.list_index
            ));
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

/// `probe --import-file <path> <listIdx>` — repro instrumentation: import an arbitrary
/// `.preset` FILE (raw xor_jld bytes on disk) into an EMPTY target list index via the
/// production `replace_inplace_with` machinery. Refuses an occupied target.
pub fn probe_import_file(path: &str, list_index: u32) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let mut s = Session::connect()?;
    let list = read_full_list(&mut s)?;
    if let Some(e) = list.iter().find(|e| e.slot == list_index) {
        if !session::is_empty_slot_name(&e.name) {
            return Err(format!(
                "target list index {list_index} is occupied by {:?} — refusing to import over it",
                e.name
            ));
        }
    }
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(1000));
    replace_inplace_with(list_index, &bytes, false)?;
    Ok(format!(
        "[probe --import-file] imported {path} → list index {list_index} (device slot {})\n",
        list_index + 1
    ))
}

/// `probe --seed-scenario` — fresh-process seed for the online e2e runner.
pub fn probe_seed_scenario() -> Result<String, String> {
    let o = seed_scenario_core(true)?;
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

    /// The pristine probe must survive tail-truncated field-8 partials and flag
    /// a leveled (drifted-presetLevel) body — the HW incident: a prior strict
    /// run left the Hiwatt at 0.37495 vs the fixture's 0.5999 and the marker-only
    /// skip handed the next run pre-leveled state.
    #[test]
    fn pristine_check_flags_leveled_bodies() {
        let fixture = r#"{"audioGraph":{"nodes":[],"presetLevel":0.5999999046325684},"info":{"source_id":"tmp-companion-e2e-fixture#r2"}}"#;
        let same = fixture.as_bytes();
        let leveled = r#"{"audioGraph":{"nodes":[],"presetLevel":0.37495},"info":{"source_id":"tmp-companion-e2e-fixture#r2"}}"#.as_bytes();
        assert!(body_is_pristine(same, fixture));
        assert!(!body_is_pristine(leveled, fixture));
        // Tail-truncated AFTER presetLevel → still comparable.
        let truncated = &fixture.as_bytes()[..fixture.len() - 3];
        assert!(body_is_pristine(truncated, fixture));
        // Truncated BEFORE the level (or unreadable) → NOT pristine, fail-open
        // to a redundant re-import (ownership is already proven by the marker).
        assert!(!body_is_pristine(b"{\"audioGraph\":{\"nod", fixture));
        // An OLDER-rev resident copy is never pristine, whatever its level reads.
        let old_rev = r#"{"audioGraph":{"nodes":[],"presetLevel":0.5999999046325684},"info":{"source_id":"tmp-companion-e2e-fixture"}}"#;
        assert!(!body_is_pristine(old_rev.as_bytes(), fixture));
        assert_eq!(extract_preset_level(b"junk"), None);
        // The identity guard that gates the pristine decision: a wrong-slot body
        // names a DIFFERENT preset and must not drive a re-import.
        let named = br#"{"info":{"displayName":"E2E Target 2"}}"#;
        assert!(body_names(named, "E2E Target 2"));
        assert!(!body_names(named, "E2E Hiwatt 3S"));
    }

    /// A fixture regen that drops the `source_id` stamp must fail here, not on
    /// the unit (the guards would refuse to manage unmarked copies).
    #[test]
    fn committed_fixtures_carry_an_ownership_marker() {
        let spec = scenario_spec().expect("committed spec parses");
        assert_eq!(spec.len(), 5, "every committed scenario preset is checked");
        for p in &spec {
            assert!(
                is_fixture_body(p.preset_json.as_bytes()),
                "{} carries no fixture marker",
                p.name
            );
            assert!(
                p.preset_json.contains(FIXTURE_SOURCE_STAMP),
                "{} does not carry the CURRENT fixture rev stamp {FIXTURE_SOURCE_STAMP:?} — \
                 bump the stamp (and this const) on every fixture regen",
                p.name
            );
        }
    }
}
