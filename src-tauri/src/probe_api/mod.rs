//! Headless `probe`-bin entry points + their private helpers, extracted from
//! `lib.rs` (Phase 2 refactor). Each submodule groups one probe concern; the
//! `pub fn probe_*` entry points are re-exported so `bin/probe.rs` reaches them
//! as `<libcrate>::probe_xxx`. Helpers that stayed-in-lib commands still call are
//! `pub(crate)` and re-listed in `lib.rs`'s explicit `pub(crate) use` seam.

pub(crate) mod analyze;
pub(crate) mod doctor_calib;
pub(crate) mod doctor_defects;
pub(crate) mod doctor_inject;
pub(crate) mod doctor_window_ab;
pub(crate) mod fs_level;
pub(crate) mod ftsw;
pub(crate) mod insert;
pub(crate) mod inspect;
pub(crate) mod level;
pub(crate) mod overlay_ab;
pub(crate) mod replace;
pub(crate) mod scene_bench;
pub(crate) mod scene_jobs;
pub(crate) mod scene_level;
pub(crate) mod scene_scan;
pub(crate) mod seed_scenario;
pub(crate) mod setlists;
pub(crate) mod slot_read;
pub(crate) mod slot_write;
pub(crate) mod songs;
pub(crate) mod stimulus;

/// Validate a `--family` CLI id and resolve it — shared by every doctor-calib-
/// style probe subcommand so a typo'd family can't silently sweep + report/
/// derive under the wrong band layout.
pub(crate) fn parse_family_arg(family_id: &str) -> Result<crate::doctor::Family, String> {
    if !matches!(
        family_id.to_ascii_lowercase().as_str(),
        "guitar" | "bass" | "bass-vi"
    ) {
        return Err(format!(
            "unrecognized --family '{family_id}' (expected guitar|bass|bass-vi)"
        ));
    }
    Ok(crate::doctor::Family::from_topology(family_id))
}

/// Belt-and-braces: leave the unit re-amp OFF even after a mid-sweep capture
/// error — every doctor-calib-style probe sweep ends on this, on a fresh
/// connection, best-effort (a failure here is not itself surfaced).
pub(crate) fn reamp_off_best_effort() {
    let _ =
        crate::session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
}

pub use doctor_calib::*;
pub use doctor_defects::*;
pub use doctor_inject::*;
pub use doctor_window_ab::*;
pub use fs_level::*;
pub use ftsw::*;
pub use insert::*;
pub use inspect::*;
pub use level::*;
pub use overlay_ab::*;
pub use replace::*;
pub use scene_bench::*;
pub use scene_level::*;
pub use scene_scan::*;
pub use seed_scenario::{probe_clear_strays, probe_seed_scenario};
pub use setlists::*;
pub use slot_read::*;
pub use slot_write::*;
pub use songs::*;
pub use stimulus::*;
