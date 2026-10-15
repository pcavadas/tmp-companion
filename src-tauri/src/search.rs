//! Advanced search: index intrinsic preset facets and filter over them.
//!
//! Read-only + OFFLINE: operates on decoded preset JSON (the `.preset` codec
//! output). Extracts each preset's intrinsic facets — name, signal-path template,
//! `presetLevel`, the full block list (`audioGraph.*Nodes[].nodeId`), the amp/cab
//! subset (classified via a caller-supplied category map, since categories live in
//! the firmware `product_profile.json`, not in the preset), IR files
//! (`dspUnitParameters.file`), and Speaker Impedance Curves (`dspUnitParameters.sicid`).
//!
//! In-memory index only (no DB/search engine); exact + substring + range filters
//! AND-combined; no saved-search persistence. The category map is decoupled (a param)
//! so this module stays pure + unit-testable and the firmware coupling lives in the
//! app layer.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

/// The intrinsic, searchable facets of one preset.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Facets {
    pub name: String,
    pub template: String,
    pub preset_level: Option<f64>,
    /// Every block's `nodeId`, sorted + de-duplicated.
    pub blocks: Vec<String>,
    /// Blocks whose category (per the supplied map) is `amp`.
    pub amps: Vec<String>,
    /// Blocks whose category is `cab` / `cabinet` / `speaker`. (On the TMP the cab is
    /// usually the IR block; this captures any explicitly-categorized cab models.)
    pub cabs: Vec<String>,
    /// IR file names referenced by any block (`dspUnitParameters.file`).
    pub irs: Vec<String>,
    /// Speaker Impedance Curve ids referenced by any block (`dspUnitParameters.sicid`).
    pub sics: Vec<String>,
}

/// Map of `ACD_*` block id → lowercase category (`"amp"`, `"cab"`, …), as the app
/// derives from the firmware `product_profile.json`. Tests supply a small one.
pub type CategoryMap = std::collections::HashMap<String, String>;

/// Index one decoded preset JSON into its facets. `categories` classifies block ids
/// into amp/cab; an empty map simply yields empty `amps`/`cabs` (graceful).
pub fn index_preset(preset: &Value, categories: &CategoryMap) -> Facets {
    let name = preset
        .pointer("/info/displayName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let ag = preset.get("audioGraph");
    let template = ag
        .and_then(|a| a.get("template"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let preset_level = crate::audiograph::preset_level(preset);

    let mut blocks = BTreeSet::new();
    let mut irs = BTreeSet::new();
    let mut sics = BTreeSet::new();

    // Walk every node under audioGraph.{guitarNodes,micNodes}.{group}[].
    crate::audiograph::for_each_node(preset, |node| {
        // The block index is by MODEL: a second instance (`X_1`) is still an `X` block.
        let id = node.model().unwrap_or("");
        if !id.is_empty() {
            blocks.insert(id.to_string());
        }
        if let Some(params) = node.get("dspUnitParameters").and_then(Value::as_object) {
            if let Some(f) = params.get("file").and_then(Value::as_str) {
                if !f.is_empty() {
                    irs.insert(f.to_string());
                }
            }
            if let Some(s) = params.get("sicid").and_then(Value::as_str) {
                if !s.is_empty() {
                    sics.insert(s.to_string());
                }
            }
        }
    });

    let cat = |id: &str| categories.get(id).map(String::as_str);
    let amps: Vec<String> = blocks
        .iter()
        .filter(|b| cat(b) == Some("amp"))
        .cloned()
        .collect();
    let cabs: Vec<String> = blocks
        .iter()
        .filter(|b| matches!(cat(b), Some("cab") | Some("cabinet") | Some("speaker")))
        .cloned()
        .collect();

    Facets {
        name,
        template,
        preset_level,
        blocks: blocks.into_iter().collect(),
        amps,
        cabs,
        irs: irs.into_iter().collect(),
        sics: sics.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The `.preset` codec XOR; decodes a fixture file to its JSON.
    fn decode_preset(bytes: &[u8]) -> String {
        String::from_utf8_lossy(&crate::backup::xor_jld(bytes)).into_owned()
    }

    fn fixture_path() -> PathBuf {
        // src-tauri/ is three levels below the repo root (apps/<app>/src-tauri).
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/Guitar.preset")
    }

    // AC1 — indexing the Guitar fixture yields the expected facets. Auto-skips when
    // the gitignored fixture is absent (fresh worktree), per repo convention.
    #[test]
    fn index_extracts_facets_from_fixture() {
        let path = fixture_path();
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skip: fixture {} absent", path.display());
            return;
        };
        let json: Value =
            serde_json::from_str(&decode_preset(&bytes)).expect("fixture decodes to JSON");

        // The app derives this from product_profile.json; here we pin the fixture's amps.
        let categories: CategoryMap = [
            ("ACD_TwinReverb65NoFx", "amp"),
            ("ACD_OrangeRockerverb50MKIII", "amp"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let f = index_preset(&json, &categories);
        assert_eq!(f.name, "Guitar");
        assert_eq!(f.template, "gtrSeries");
        // All 11 dspUnits present as blocks.
        assert!(f.blocks.contains(&"ACD_KlonCentaur".to_string()));
        assert!(f.blocks.contains(&"ACD_UserIRTMS".to_string()));
        assert_eq!(f.blocks.len(), 11, "blocks: {:?}", f.blocks);
        // Amps classified via the map (sorted by BTreeSet order).
        assert_eq!(
            f.amps,
            vec![
                "ACD_OrangeRockerverb50MKIII".to_string(),
                "ACD_TwinReverb65NoFx".to_string()
            ]
        );
        // SIC pulled from the IR node's sicid.
        assert_eq!(f.sics, vec!["FenDlxRvb_c12k".to_string()]);
        // The IR node carries a file reference.
        assert_eq!(f.irs.len(), 1, "irs: {:?}", f.irs);
    }
}
