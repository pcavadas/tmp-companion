//! Decode the device saved-block ("block preset") store.

use crate::proto;
use serde::Serialize;

/// One saved block ("block preset") from the device store.
/// Identity + cab config only; the actual saved
/// `dspUnitParameters` live on the device and are applied live by `index` via
/// `ReplaceNodeWithBlock`, NOT carried here. `dual_cabs_enabled` + `cab1_id`/`cab2_id`
/// fully describe a saved dual-cab.
#[derive(Debug, Clone, Serialize)]
pub struct SavedBlock {
    pub fender_id: String,
    /// Position within this fenderId's saved list = the `ReplaceNodeWithBlock` index.
    pub index: u32,
    pub name: String,
    pub favorite: bool,
    pub dual_cabs_enabled: bool,
    pub cab1_id: String,
    pub cab2_id: String,
}

/// Decode the `allBlockPresetsResponse.blockPresetsMap` blob (LZ4-block-compressed
/// JSON map `{ fenderId: [ {cab1Id,cab2Id,dualCabsEnabled,favorite,name}, … ] }`)
/// into a flat list keyed by `(fender_id, index)`. Auto-generated default entries are
/// flattened too (the frontend filters by name); the index is the device library slot.
pub fn parse_block_presets_map(blob: &[u8]) -> Result<Vec<SavedBlock>, String> {
    let json = proto::lz4_block_decompress(blob)?;
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&json).map_err(|e| format!("parse blockPresetsMap json: {e}"))?;
    let mut out = Vec::new();
    for (fender_id, arr) in &map {
        let Some(entries) = arr.as_array() else {
            continue;
        };
        for (i, e) in entries.iter().enumerate() {
            let s = |k: &str| {
                e.get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            let b = |k: &str| {
                e.get(k)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            out.push(SavedBlock {
                fender_id: fender_id.clone(),
                index: i as u32,
                name: s("name"),
                favorite: b("favorite"),
                dual_cabs_enabled: b("dualCabsEnabled"),
                cab1_id: s("cab1Id"),
                cab2_id: s("cab2Id"),
            });
        }
    }
    Ok(out)
}

/// Find the `allBlockPresetsResponse` (PresetMessage field 136 → `blockPresetsMap`
/// field 1) blob in a set of reassembled inbound bodies.
pub(crate) fn find_block_presets_blob(bodies: &[Vec<u8>]) -> Option<Vec<u8>> {
    for b in bodies {
        let top = proto::parse(b);
        if let Some(pm) = proto::first_bytes(&top, 2) {
            let inner = proto::parse(pm);
            if let Some(resp) = proto::first_bytes(&inner, 136) {
                let map_bytes = proto::parse(resp);
                return Some(proto::first_bytes(&map_bytes, 1).unwrap_or(resp).to_vec());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_block_presets_map_flattens_saved_blocks() {
        // The device store is LZ4-block-compressed JSON: { fenderId: [ {…}, … ] }.
        let json = br#"{"ACD_AC30Brilliant":[{"cab1Id":"","cab2Id":"","dualCabsEnabled":false,"favorite":false,"name":"Crunch"}],"ACD_CabSimTMS":[{"cab1Id":"Diezel412FV","cab2Id":"Diezel412FV","dualCabsEnabled":true,"favorite":true,"name":"Nashville blend"}]}"#;
        let blob = proto::lz4_block_compress_stored(json);
        let mut out = parse_block_presets_map(&blob).expect("decode");
        out.sort_by(|a, b| a.fender_id.cmp(&b.fender_id).then(a.index.cmp(&b.index)));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].fender_id, "ACD_AC30Brilliant");
        assert_eq!(out[0].name, "Crunch");
        assert_eq!(out[0].index, 0);
        assert!(!out[0].dual_cabs_enabled);
        // The dual-cab is fully described: enabled flag + both cab ids.
        assert_eq!(out[1].fender_id, "ACD_CabSimTMS");
        assert!(out[1].dual_cabs_enabled);
        assert_eq!(out[1].cab1_id, "Diezel412FV");
        assert_eq!(out[1].cab2_id, "Diezel412FV");
        assert!(out[1].favorite);
    }
}
