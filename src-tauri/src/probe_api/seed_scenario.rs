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

/// Substring probe (truncation-proof vs the field-8 partial). Pure.
fn is_fixture_body(bytes: &[u8]) -> bool {
    let body = String::from_utf8_lossy(bytes);
    FIXTURE_MARKERS.iter().any(|m| body.contains(m))
}

// ── Seed manifest: the ownership signal that survives a spec's save ──────────
//
// A CONTENT marker cannot carry ownership on its own. `info.source_id` and the
// `e2e00000-` scene uuids are fixture-INJECTED fields: the device rewrites the
// preset body on `saveCurrentPreset`, dropping the unknown `source_id` and
// regenerating scene uuids. So the moment a spec legitimately writes and saves a
// fixture — `level.spec.ts` levels and saves "E2E Reference" at slot 400 — that
// fixture reads as un-owned forever after. Both guards then refuse it: the
// re-seed won't overwrite it AND teardown's guarded clear won't clean it, so it
// strands and hard-blocks every later run at the seed step.
//
// HW-isolated 2026-07-25 (fw 1.8.45): seed → seed skips cleanly (`imported slots
// []`, markers intact through IMPORT), while seed → level.spec.ts → seed refuses
// on slot 400 ONLY — the single fixture the specs save. 401/402/403 stay owned.
//
// The fix is provenance recorded by the WRITER at import time. Machine-local file,
// keyed slot → name; ownership is manifest-hit AND current-name match, so a slot
// whose name has since changed is NOT blessed (still fail-closed).
//
// The claim is a LEASE, not a deed: `forget_seeded` drops it the moment the fixture
// is verifiably cleared, so it never outlives what it describes. That matters — an
// immortal claim would bless whatever occupies a scratch slot next (e.g. a Pro
// Control backup restore putting a levelled, marker-free fixture back), letting the
// harness overwrite and delete content the marker-only guard used to protect. With
// the lease, ownership is claimed and released inside exactly the window the harness
// genuinely owns the slot, and the residual risk really is the pre-marker one: a
// preset placed at a scratch slot, under the exact fixture name, WHILE a run holds
// the lease. The scratch zone is documented as harness-owned — an accepted trade.

/// Where the seed records what it placed. Env-overridable so tests never touch the
/// real one; defaults alongside the runner's own logs (`scripts/e2e.sh` `LOG_DIR`).
fn manifest_path() -> std::path::PathBuf {
    std::env::var("TMP_E2E_SEED_MANIFEST")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into()))
                .join("tmp-companion-e2e")
                .join("seeded-slots.json")
        })
}

/// Claim `list_index` (0-based) under `name`. Best-effort: a manifest we fail to write
/// only costs the NEXT run a manual clear — it must never fail a seed that already
/// landed on the device.
fn record_seeded(list_index: u32, name: &str) {
    let mut map = read_manifest();
    map.insert(list_index.to_string(), name.to_string());
    write_manifest(&map);
}

/// Release the claim on `list_index` — called ONLY after a VERIFIED successful clear.
/// The claim is a LEASE, not a deed: it must not outlive the fixture it describes.
/// Left immortal, a stale entry would bless whatever later occupies a scratch slot —
/// e.g. a Pro Control backup restore putting a levelled (marker-free) fixture back —
/// and the harness would then overwrite and delete content the marker-only guard used
/// to protect. Pruning only on SUCCESS matters: dropping the claim before/despite a
/// failed clear would strand the slot with neither manifest nor marker, which is the
/// original bug.
// Its one production caller (`e2e_server::e2e_clear_preset`) is behind `--features e2e`,
// so a plain lib build sees no user; the unit test below is the other caller.
#[allow(dead_code)]
pub(crate) fn forget_seeded(list_index: u32) {
    let mut map = read_manifest();
    if map.remove(&list_index.to_string()).is_some() {
        write_manifest(&map);
    }
}

fn write_manifest(map: &std::collections::BTreeMap<String, String>) {
    let path = manifest_path();
    let tmp = path.with_extension("json.tmp");
    // Write-then-rename (atomic within a directory): a plain `fs::write` truncates in
    // place, so a kill mid-write (this harness does get killed) would leave partial JSON
    // that reads back as "nothing was ever seeded" — dropping every lease at once, the
    // exact stranding this manifest exists to prevent, just for all slots instead of one.
    let ok = path
        .parent()
        .is_none_or(|d| std::fs::create_dir_all(d).is_ok())
        && serde_json::to_vec(map).is_ok_and(|b| std::fs::write(&tmp, b).is_ok())
        && std::fs::rename(&tmp, &path).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        eprintln!("[seed] could not write the seed manifest at {path:?} — a fixture saved by a spec may need a manual clear");
    }
}

fn read_manifest() -> std::collections::BTreeMap<String, String> {
    let path = manifest_path();
    let Ok(bytes) = std::fs::read(&path) else {
        return Default::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(map) => map,
        Err(e) => {
            // Distinct from "no manifest file yet": a CORRUPT manifest silently treated as
            // empty would drop every recorded claim at once, same failure mode the atomic
            // write above closes for the write side.
            eprintln!(
                "[seed] seed manifest at {path:?} is unreadable ({e}) — treating every slot \
                 as un-owned; seeded fixtures may need a manual clear"
            );
            Default::default()
        }
    }
}

/// Did THIS machine's harness seed `list_index` under `name`? Pure given the file.
fn manifest_owns(list_index: u32, name: &str) -> bool {
    read_manifest()
        .get(&list_index.to_string())
        .is_some_and(|n| n == name)
}

/// Is this slot ours to overwrite or clear — either we RECORDED seeding it (survives a
/// spec's save) or its body is still PRISTINE (covers a fixture seeded before the
/// manifest existed, and any run whose best-effort write failed). Cheap check first, so
/// the device read is skipped whenever the manifest already answers.
///
/// `list_index` is 0-BASED, the same space as the clears and imports this gates — as is
/// [`slot_is_pristine_fixture`]. ONE index convention across both probes: this repo has
/// already lost a real preset to a guard that read one space while the mutation acted in
/// another. Callers must be on a QUIET line (`drain_until_quiet` first) — a field-8 read
/// fired mid-flood is dropped device-side.
pub(crate) fn slot_is_ours(s: &mut Session, list_index: u32, name: &str) -> bool {
    manifest_owns(list_index, name) || slot_is_pristine_fixture(s, list_index)
}

/// Does the body still carry an injected marker — i.e. NO spec has saved over it since
/// the import? Answers "is this untouched", NOT "did we put it here"; conflating the two
/// is what stranded fixtures. `list_index` is 0-BASED (the field-8 read takes +1).
pub(crate) fn slot_is_pristine_fixture(s: &mut Session, list_index: u32) -> bool {
    matches!(s.read_slot_preset_json(list_index + 1), Ok(Some(bytes)) if is_fixture_body(&bytes))
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
        let owned = slot_is_pristine_fixture(s, slot);
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
        let name_ok = e.name == p.name;
        // PRISTINE: the body still carries the injected marker, so no spec has saved
        // over it — byte-identical to what we would import. Skip (the old fast path).
        let pristine = name_ok && slot_is_pristine_fixture(&mut s, p.list_index);
        if pristine {
            // Record it even though we're not importing: a verified fixture in place IS
            // ours, and this is the only chance to say so while the marker is still
            // readable. Without this, a slot seeded by an OLDER build (or by a run whose
            // manifest was wiped) is skipped here, never recorded, and strands the first
            // time a spec saves it — the exact bug, just one run later.
            record_seeded(p.list_index, &p.name);
            continue;
        }
        // OURS BUT DIRTY: we seeded this slot, and the marker is gone — a spec levelled
        // and saved it. Re-seed so the next spec gets a pristine fixture instead of the
        // previous run's leftovers. This is the "clear at the start" arm; without it a
        // manifest hit would silently bless stale, modified content.
        if name_ok && manifest_owns(p.list_index, &p.name) {
            to_seed.push(p);
            continue;
        }
        return Err(format!(
            "target slot {} is occupied by {:?}, which this harness did not seed \
             (no manifest entry, no fixture content marker) — refusing to seed over it \
             (move that preset, then rerun)",
            p.list_index, e.name
        ));
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
            Some(name) if *name == p.name => slot_is_ours(&mut s, p.list_index, name),
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
        // Record BEFORE anything can save over it — this is the ownership signal
        // teardown will need once a spec has rewritten the body.
        record_seeded(p.list_index, &p.name);
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

    /// The stranding bug (HW 2026-07-25): once `level.spec.ts` levels and SAVES
    /// "E2E Reference", the device rewrites the body and the fixture's injected
    /// markers are gone — so a content-only ownership probe reports un-owned, the
    /// re-seed refuses to overwrite it AND teardown refuses to clear it, and every
    /// later online run dies at the seed step. The manifest is what survives that
    /// save, so it must bless the slot with NO readable marker in the body at all.
    /// Name-keyed, so a slot whose name has since changed stays fail-closed.
    /// Restores `TMP_E2E_SEED_MANIFEST` to whatever it held before (not just unset)
    /// and removes the temp dir on drop, so a panic mid-test (an assert failing)
    /// can't leak the override into later tests in the same process —
    /// `std::env::set_var` is process-wide, and this file's `record_seeded` /
    /// `forget_seeded` calls in OTHER tests would otherwise silently target it too.
    struct EnvGuard {
        dir: std::path::PathBuf,
        prev: Option<std::ffi::OsString>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(prev) => std::env::set_var("TMP_E2E_SEED_MANIFEST", prev),
                None => std::env::remove_var("TMP_E2E_SEED_MANIFEST"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn manifest_owns_a_fixture_whose_body_no_longer_carries_a_marker() {
        let dir =
            std::env::temp_dir().join(format!("tmp-companion-seedmanifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let prev = std::env::var_os("TMP_E2E_SEED_MANIFEST");
        let _guard = EnvGuard {
            dir: dir.clone(),
            prev,
        };
        let path = dir.join("seeded-slots.json");
        std::env::set_var("TMP_E2E_SEED_MANIFEST", &path);
        let _ = std::fs::remove_file(&path);

        // Nothing recorded yet → not owned (a pre-seed run must never bless a slot).
        assert!(!manifest_owns(400, "E2E Reference"));

        record_seeded(400, "E2E Reference");

        // Owned by provenance alone — this is the case a body-marker probe misses.
        assert!(manifest_owns(400, "E2E Reference"));
        // A saved body really has lost its markers: the content probe says no.
        assert!(!is_fixture_body(
            br#"{"info":{"preset_id":"9f2c-device-generated"}}"#
        ));
        // Fail-closed on the axes that matter: wrong slot, and a renamed occupant.
        assert!(!manifest_owns(401, "E2E Reference"));
        assert!(!manifest_owns(400, "My Own Preset"));

        // The claim is a LEASE: a verified clear releases it, so it can never outlive
        // the fixture and bless whatever occupies the scratch slot next.
        forget_seeded(400);
        assert!(!manifest_owns(400, "E2E Reference"));
    }

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
        assert_eq!(spec.len(), 4);
        for p in &spec {
            assert!(
                is_fixture_body(p.preset_json.as_bytes()),
                "{} carries no fixture marker",
                p.name
            );
        }
    }
}
