//! Decode the device saved-block ("block preset") + user-IR stores.

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

/// One user impulse-response slot on the device (`UserIRListRecord`).
#[derive(Debug, Clone, Serialize)]
pub struct UserIr {
    pub name: String,
    /// The device reports whether the IR file is actually present.
    pub exists: bool,
}

/// Decode every `userIRListResponse` (UserIRMessage field 13 → field 3 → `record`
/// field 2 = `{ name=1, exists=2 }`) carried in a set of inbound bodies.
pub(crate) fn find_user_irs(bodies: &[Vec<u8>]) -> Vec<UserIr> {
    // The device can answer the IR-list request more than once in a burst (the
    // in-handshake reply + an explicit re-send), and an IR name can recur across
    // slots — without de-duping, the frontend gets duplicate-named rows → duplicate
    // React keys → broken list navigation. An IR is referenced by name (its file
    // link), so de-dupe by name: keep first-seen order, OR `exists` so a present copy
    // wins over a missing one.
    let mut out: Vec<UserIr> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for b in bodies {
        let top = proto::parse(b);
        let Some(um) = proto::first_bytes(&top, 13) else {
            continue;
        };
        let inner = proto::parse(um);
        let Some(resp) = proto::first_bytes(&inner, 3) else {
            continue;
        };
        let r = proto::parse(resp);
        for rec in proto::all_bytes(&r, 2) {
            let rp = proto::parse(rec);
            let name = proto::first_bytes(&rp, 1)
                .map(|x| String::from_utf8_lossy(x).into_owned())
                .unwrap_or_default();
            let exists = proto::first_varint(&rp, 2).unwrap_or(0) != 0;
            if name.is_empty() {
                continue;
            }
            match seen.get(&name) {
                Some(&i) => out[i].exists = out[i].exists || exists,
                None => {
                    seen.insert(name.clone(), out.len());
                    out.push(UserIr { name, exists });
                }
            }
        }
    }
    out
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

    #[test]
    fn find_user_irs_decodes_records() {
        // Minimal protobuf encoders (all our fields are short, <128-byte lengths).
        fn ld(field: u32, inner: &[u8]) -> Vec<u8> {
            let mut out = vec![((field << 3) | 2) as u8, inner.len() as u8];
            out.extend_from_slice(inner);
            out
        }
        fn vfield(field: u32, v: u64) -> Vec<u8> {
            vec![(field << 3) as u8, v as u8]
        }
        // UserMessage(13) → userIRListResponse(3) → record(2) = { name=1, exists=2 }.
        let rec = |name: &str, exists: u64| {
            let mut inner = ld(1, name.as_bytes());
            inner.extend(vfield(2, exists));
            inner
        };
        let mut resp = ld(2, &rec("Oversize 4x12", 1));
        resp.extend(ld(2, &rec("Matchless", 0)));
        let body = ld(13, &ld(3, &resp));
        let irs = find_user_irs(std::slice::from_ref(&body));
        assert_eq!(irs.len(), 2);
        assert_eq!(irs[0].name, "Oversize 4x12");
        assert!(irs[0].exists);
        assert!(!irs[1].exists);

        // De-dupe by name across repeated responses (burst reply + re-send): the
        // device can echo the same list twice → one row per name, first-seen order,
        // exists OR-ed so a present copy wins. (Else duplicate React keys break the UI.)
        let irs_dup = find_user_irs(&[body.clone(), body.clone()]);
        assert_eq!(
            irs_dup.len(),
            2,
            "duplicate responses must collapse by name"
        );
        // A "missing" copy followed by a "present" copy resolves to present.
        let a = ld(13, &ld(3, &ld(2, &rec("Twin", 0))));
        let b = ld(13, &ld(3, &ld(2, &rec("Twin", 1))));
        let merged = find_user_irs(&[a, b]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].exists);
    }
}
