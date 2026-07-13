//! Headless `probe`-bin entry points + their private helpers, extracted from
//! `lib.rs` (Phase 2 refactor). Each submodule groups one probe concern; the
//! `pub fn probe_*` entry points are re-exported so `bin/probe.rs` reaches them
//! as `<libcrate>::probe_xxx`. Helpers that stayed-in-lib commands still call are
//! `pub(crate)` and re-listed in `lib.rs`'s explicit `pub(crate) use` seam.

pub(crate) mod doctor_calib;
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
pub(crate) mod setlists;
pub(crate) mod slot_read;
pub(crate) mod slot_write;
pub(crate) mod songs;
pub(crate) mod stimulus;

pub use doctor_calib::*;
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
pub use setlists::*;
pub use slot_read::*;
pub use slot_write::*;
pub use songs::*;
pub use stimulus::*;
