//! OFFLINE-library bulk-run helpers for the `probe` bulk arms (the app has no bulk tab).
use crate::{bulk_cmd, bulkrun, library, preset_io, search, ListIndex};

/// Block-category map for facet indexing. TODO: derive amp/cab categories
/// from the firmware `product_profile.json`; until then facets index blocks/IRs/SICs
/// /name/level/template (everything except the amp/cab classification).
pub(crate) fn library_categories() -> search::CategoryMap {
    search::CategoryMap::new()
}

/// Build `PresetTarget`s for the selected list indices from the imported library.
/// Errors if any index is not a matched, writable record (the destructive-op guard:
/// unmatched/ambiguous records have `list_index = None` and never reach here).
pub(crate) fn targets_from_library(
    lib: &library::Library,
    selection: &[ListIndex],
) -> Result<Vec<bulkrun::PresetTarget>, String> {
    selection
        .iter()
        .map(|&idx| {
            lib.records
                .iter()
                .find(|r| r.list_index == Some(idx))
                .ok_or_else(|| {
                    format!("list index {idx} is not a matched, writable library record")
                })
                .and_then(|r| r.to_target())
        })
        .collect()
}

/// Construct the right `PresetIo` for a run's path (built fresh per run; both are
/// stateless and open their own device connections).
pub(crate) fn io_for_path(path: bulk_cmd::IoPath) -> Box<dyn bulkrun::PresetIo> {
    match path {
        bulk_cmd::IoPath::Live => Box::new(preset_io::LiveIo),
        bulk_cmd::IoPath::Offline => Box::new(preset_io::OfflineIo),
    }
}

pub(crate) fn format_dry_run(entries: &[bulkrun::DryRunEntry]) -> String {
    let mut s = format!("[probe bulk dry-run] {} preset(s)\n", entries.len());
    for e in entries {
        s += &format!(
            "  slot {:>3} {:<24} {:?}{}\n",
            e.list_index,
            e.display_name,
            e.status,
            match &e.error {
                Some(err) => format!("  ERROR: {err}"),
                None if !e.changes.is_empty() => format!("  ({} field(s) change)", e.changes.len()),
                None => String::new(),
            }
        );
    }
    s
}
