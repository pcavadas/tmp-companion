//! Firmware migration assistant: diff two firmware effect catalogs
//! (added / removed / renamed), scan the library for presets referencing
//! removed/renamed blocks, and dispatch a replacement mapping through the
//! block-replace path.
//!
//! OFFLINE: catalogs are id lists (the app derives them from
//! `firmware/<v>/recon/dsp-effects-catalog.md`, version-pinned); the rename mapping
//! is curated (the recon registry knows old→new). Single from→to per run.

use crate::slot::ListIndex;
use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::audiograph::{count_model_nodes, replace_block};

/// Raw set difference between two catalogs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CatalogDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Diff `old` → `new` block-id catalogs (sorted).
pub fn diff_catalogs(old: &[String], new: &[String]) -> CatalogDiff {
    let o: HashSet<&String> = old.iter().collect();
    let n: HashSet<&String> = new.iter().collect();
    let mut added: Vec<String> = n.difference(&o).map(|s| (*s).clone()).collect();
    let mut removed: Vec<String> = o.difference(&n).map(|s| (*s).clone()).collect();
    added.sort();
    removed.sort();
    CatalogDiff { added, removed }
}

/// A diff classified against a curated `rename_map` (old id → new id): renamed pairs
/// (removed id whose rename target was added), and the ids that were purely
/// removed/added.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassifiedDiff {
    pub renamed: Vec<(String, String)>,
    pub removed_only: Vec<String>,
    pub added_only: Vec<String>,
}

pub fn classify(diff: &CatalogDiff, rename_map: &BTreeMap<String, String>) -> ClassifiedDiff {
    let added_set: HashSet<&String> = diff.added.iter().collect();
    let mut renamed = Vec::new();
    let mut removed_only = Vec::new();
    let mut renamed_targets: HashSet<String> = HashSet::new();
    for r in &diff.removed {
        match rename_map.get(r) {
            Some(to) if added_set.contains(to) => {
                renamed.push((r.clone(), to.clone()));
                renamed_targets.insert(to.clone());
            }
            _ => removed_only.push(r.clone()),
        }
    }
    let added_only: Vec<String> = diff
        .added
        .iter()
        .filter(|a| !renamed_targets.contains(*a))
        .cloned()
        .collect();
    ClassifiedDiff {
        renamed,
        removed_only,
        added_only,
    }
}

/// Presets referencing any of `ids`, with the matched ids. `presets` is a list of
/// `(key, decoded_json)`; the key is the caller's (a `ListIndex`, or a library position).
pub fn scan_affected<K: Copy>(
    presets: &[(K, Value)],
    ids: &HashSet<String>,
) -> Vec<(K, Vec<String>)> {
    let mut out = Vec::new();
    for (idx, preset) in presets {
        let hits: Vec<String> = ids
            .iter()
            .filter(|id| count_model_nodes(preset, id) > 0)
            .cloned()
            .collect();
        if !hits.is_empty() {
            let mut hits = hits;
            hits.sort();
            out.push((*idx, hits));
        }
    }
    out
}

/// One planned replacement: in preset `list_index`, swap `from` → `to`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replacement {
    pub list_index: ListIndex,
    pub from: String,
    pub to: String,
}

/// Build the replacement plan: for each affected preset + matched renamed id, emit a
/// `from → to` swap. Only ids present in `rename_map` are planned; purely-removed ids
/// are reported by `scan_affected` for manual handling.
pub fn plan_replacements(
    affected: &[(ListIndex, Vec<String>)],
    rename_map: &BTreeMap<String, String>,
) -> Vec<Replacement> {
    let mut plan = Vec::new();
    for (idx, ids) in affected {
        for id in ids {
            if let Some(to) = rename_map.get(id) {
                plan.push(Replacement {
                    list_index: *idx,
                    from: id.clone(),
                    to: to.clone(),
                });
            }
        }
    }
    plan
}

/// Apply one replacement to a preset (dispatches to the block-replace with B
/// defaults). Returns the number of blocks swapped.
pub fn apply_replacement(preset: &mut Value, r: &Replacement) -> usize {
    replace_block(preset, &r.from, &r.to, &Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    fn set(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    fn preset_with(id: &str) -> Value {
        serde_json::json!({ "audioGraph": { "guitarNodes": { "G1": [
            { "nodeId": id, "dspUnitParameters": {} }
        ] } } })
    }

    // AC1 — catalog diff + rename classification.
    #[test]
    fn catalog_diff_added_removed_renamed() {
        let diff = diff_catalogs(&ids(&["A", "B", "C"]), &ids(&["B", "C", "A2", "D"]));
        assert_eq!(diff.removed, ids(&["A"]));
        assert_eq!(diff.added, ids(&["A2", "D"]));

        let mut rename = BTreeMap::new();
        rename.insert("A".to_string(), "A2".to_string());
        let c = classify(&diff, &rename);
        assert_eq!(c.renamed, vec![("A".to_string(), "A2".to_string())]);
        assert_eq!(c.removed_only, Vec::<String>::new());
        assert_eq!(
            c.added_only,
            ids(&["D"]),
            "A2 is a rename target, not a pure add"
        );
    }

    // AC2 — scan flags presets referencing affected ids.
    #[test]
    fn scan_flags_affected_presets() {
        let presets = vec![
            (0u32, preset_with("A")),
            (1, preset_with("B")),
            (2, preset_with("A")),
        ];
        let affected = scan_affected(&presets, &set(&["A"]));
        assert_eq!(affected, vec![(0, ids(&["A"])), (2, ids(&["A"]))]);
    }

    // AC3 — dispatch a replacement mapping (and apply it via block-replace).
    #[test]
    fn dispatch_replacement_mapping() {
        let presets = vec![
            (ListIndex(0), preset_with("A")),
            (ListIndex(5), preset_with("A")),
        ];
        let affected = scan_affected(&presets, &set(&["A"]));
        let mut rename = BTreeMap::new();
        rename.insert("A".to_string(), "A2".to_string());
        let plan = plan_replacements(&affected, &rename);
        assert_eq!(plan.len(), 2);
        assert_eq!(
            plan[0],
            Replacement {
                list_index: ListIndex(0),
                from: "A".into(),
                to: "A2".into()
            }
        );

        // Applying the plan swaps the block (dispatches to block-replace).
        let mut p = preset_with("A");
        assert_eq!(apply_replacement(&mut p, &plan[0]), 1);
        assert_eq!(count_model_nodes(&p, "A"), 0);
        assert_eq!(count_model_nodes(&p, "A2"), 1);
    }

    /// A firmware migration must reach a SECOND instance of a renamed model (`A_1`), which
    /// carries the model only in `FenderId`.
    fn preset_with_second_instance_only() -> Value {
        serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "FenderId": "B", "nodeId": "B" },
                { "FenderId": "A", "nodeId": "A_1" }
            ] } }
        })
    }

    #[test]
    fn scan_and_apply_reach_a_second_instance() {
        let presets = vec![(3u32, preset_with_second_instance_only())];
        let affected = scan_affected(&presets, &set(&["A"]));
        assert_eq!(affected, vec![(3, ids(&["A"]))], "A_1 is an A block");
        let mut p = preset_with_second_instance_only();
        let r = Replacement {
            list_index: ListIndex(3),
            from: "A".into(),
            to: "A2".into(),
        };
        assert_eq!(apply_replacement(&mut p, &r), 1);
        assert_eq!(count_model_nodes(&p, "A"), 0);
        assert_eq!(count_model_nodes(&p, "A2"), 1);
    }
}
