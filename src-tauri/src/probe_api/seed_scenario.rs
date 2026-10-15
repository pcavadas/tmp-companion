//! Probe entry points for the online-e2e seed: `--seed-scenario`, `--clear-strays` and
//! `--import-file`. The seed itself lives in [`crate::e2e_seed`], shared with the e2e
//! bridge server.

use crate::e2e_seed::{read_full_list, seed_scenario_core, sweep_strays_core};
use crate::replace_inplace::replace_inplace_with;
use crate::session::{self, Session};
use crate::slot::ListIndex;

/// `probe --import-file <path> <listIdx>` — repro instrumentation: import an arbitrary
/// `.preset` FILE (raw xor_jld bytes on disk) into an EMPTY target list index via the
/// production `replace_inplace_with` machinery. Refuses an occupied target.
pub fn probe_import_file(path: &str, list_index: ListIndex) -> Result<String, String> {
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
    crate::device_gate::settle(std::time::Duration::from_millis(1000));
    replace_inplace_with(list_index, &bytes, false)?;
    Ok(format!(
        "[probe --import-file] imported {path} → list index {list_index} (device slot {})\n",
        list_index.device_slot()
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
