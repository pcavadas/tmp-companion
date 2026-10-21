//! The post-save proof for a structural edit: the edit counts as saved only once the slot's
//! STORED copy (the field-8 `presetDataRequest` read) shows it.
//!
//! A confirmed edit and the working copy (field 2/3) prove only the LIVE preset. A save made
//! while a non-base scene is current can store a different block chain than the live one
//! [HW, fw 1.8.58]: the unit then reports a successful save, and the working copy still
//! shows the edit. So every structural save path (`held_edit::held_replace_one`,
//! `copy_apply`) compares the stored copy against the working copy it read just before the
//! save, and reports a mismatch as an error, never as "updated".
//!
//! What is compared:
//! - each group's block roster, as ordered `(nodeId, FenderId)` pairs;
//! - the `dspUnitParameters` of every EDITED node (a node id the pre-edit preset lacked, or a
//!   replace target still present), key by key from the live node.
//!
//! The other nodes' parameters are not compared: with a scene current, the live copy shows
//! that scene's values while the stored copy holds base.
use std::collections::BTreeSet;

use serde_json::Value;

use crate::{audiograph, session, ListIndex, Session};

/// A node addressed as `(group, nodeId)`.
pub(crate) type NodeRef = (String, String);

/// Each group's ordered `(nodeId, FenderId)` pairs, as `group → pairs`.
fn roster(preset: &Value) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    audiograph::walk_nodes(preset, |_, group, node| {
        let pair = (
            node.instance().unwrap_or_default().to_string(),
            node.model().unwrap_or_default().to_string(),
        );
        match out.last_mut() {
            Some((g, pairs)) if g == group => pairs.push(pair),
            _ => out.push((group.to_string(), vec![pair])),
        }
    });
    out
}

fn node_ids(preset: &Value) -> BTreeSet<NodeRef> {
    let mut ids = BTreeSet::new();
    audiograph::walk_nodes(preset, |_, group, node| {
        if let Some(id) = node.instance() {
            ids.insert((group.to_string(), id.to_string()));
        }
    });
    ids
}

fn params<'a>(
    preset: &'a Value,
    (group, id): &NodeRef,
) -> Option<&'a serde_json::Map<String, Value>> {
    let mut found = None;
    audiograph::walk_nodes(preset, |_, g, node| {
        if g == group && node.matches_instance(id) {
            found = node.params();
        }
    });
    found
}

/// The nodes an edit produced or kept in place: every node id of `live` the pre-edit
/// preset lacked (an insert, or a replace that re-minted the id), plus each replace
/// target still present (a replace that kept its id).
pub(crate) fn edited_nodes(
    pre: &Value,
    live: &Value,
    replace_targets: &[NodeRef],
) -> BTreeSet<NodeRef> {
    let before = node_ids(pre);
    let now = node_ids(live);
    let mut edited: BTreeSet<NodeRef> = now.difference(&before).cloned().collect();
    edited.extend(replace_targets.iter().filter(|t| now.contains(*t)).cloned());
    edited
}

/// The scene a replace of `targets` must step out of and back into: the working copy's current
/// scene, when some OTHER scene has no entry for a target node. The firmware expects every
/// node to have an entry in every scene; a replace of one that lacks it is confirmed but not
/// stored by a save made with a scene current [HW, fw 1.8.58], while the same replace made on
/// base (`loadScene` 8) and saved after the recall back is [HW]. `None`: base is current, or
/// every scene has an entry for every target.
pub(crate) fn scene_to_step_out_of(pre: &Value, targets: &[NodeRef]) -> Option<u32> {
    let scenes = pre.get("scenes").and_then(Value::as_array)?;
    let current = usize::try_from(pre.get("lastLoadedScene").and_then(Value::as_u64)?).ok()?;
    if current >= scenes.len() {
        return None;
    }
    let missing = |(group, id): &NodeRef| {
        let graph = if group.starts_with('M') {
            "micNodes"
        } else {
            "guitarNodes"
        };
        scenes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != current)
            .any(|(_, scene)| scene.pointer(&format!("/{graph}/{group}/{id}")).is_none())
    };
    targets
        .iter()
        .any(missing)
        .then(|| u32::try_from(current).ok())
        .flatten()
}

/// Recall base (scene 8) on the held session before a replace that
/// [`scene_to_step_out_of`] names, keeping the session alive (no idle gap over 300 ms).
pub(crate) fn step_to_base(s: &mut Session) -> Result<(), String> {
    recall(s, session::BASE_SCENE_SLOT)
}

/// Recall `scene` again after the edits, before the save, so the preset keeps opening on it.
/// Under MAINTAIN CHANGES the recall keeps the base edits; under DISCARD it drops them, so
/// that case is refused before the step out.
pub(crate) fn step_back(s: &mut Session, scene: u32) -> Result<(), String> {
    recall(s, scene)
}

fn recall(s: &mut Session, scene: u32) -> Result<(), String> {
    s.load_scene(scene)?;
    s.heartbeat()?;
    s.pump_silent(150)?;
    s.heartbeat()
}

/// `Ok` when `stored` holds `live`'s block roster, its current scene and each `edited` node's
/// live parameters; otherwise the first difference, worded for the item's detail.
pub(crate) fn check_stored(
    live: &Value,
    stored: &Value,
    edited: &BTreeSet<NodeRef>,
) -> Result<(), String> {
    if let Some(scene) = live.get("lastLoadedScene") {
        if stored.get("lastLoadedScene") != Some(scene) {
            return Err(format!(
                "the stored preset opens on scene {} where the edited preset is on {scene}",
                stored
                    .get("lastLoadedScene")
                    .map_or_else(|| "none".to_string(), Value::to_string)
            ));
        }
    }
    let (want, got) = (roster(live), roster(stored));
    if want != got {
        return Err(format!(
            "the stored preset's blocks {} differ from the edited preset's {}",
            describe(&got),
            describe(&want)
        ));
    }
    for node in edited {
        let Some(live_params) = params(live, node) else {
            continue;
        };
        let stored_params = params(stored, node);
        for (key, value) in live_params {
            let stored_value = stored_params.and_then(|p| p.get(key));
            if stored_value != Some(value) {
                return Err(format!(
                    "the stored {}/{} has {key} = {} where the edited preset has {value}",
                    node.0,
                    node.1,
                    stored_value.map_or_else(|| "nothing".to_string(), Value::to_string)
                ));
            }
        }
    }
    Ok(())
}

fn describe(roster: &[(String, Vec<(String, String)>)]) -> String {
    roster
        .iter()
        .map(|(g, pairs)| {
            let ids: Vec<&str> = pairs.iter().map(|(id, _)| id.as_str()).collect();
            format!("{g}[{}]", ids.join(", "))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The working copy right before a save, as the document [`check_stored`] compares against:
/// a field-2 re-prompt held until its payload stops growing
/// (`Session::live_preset_value_stable`).
pub(crate) fn read_live(s: &mut Session) -> Result<Value, String> {
    s.live_preset_value_stable(|v: &Value| v.get("audioGraph").is_some())
}

/// Read `list_index`'s stored copy (field 8) on the held session and check it against
/// `live`, the working copy read just before the save. `Err` names why the save is not
/// proven: the stored copy differs, or it could not be read.
pub(crate) fn prove_saved(
    s: &mut Session,
    list_index: ListIndex,
    pre: &Value,
    live: &Value,
    replace_targets: &[NodeRef],
) -> Result<(), String> {
    let raw = s
        .read_slot_preset_json(list_index.into())?
        .ok_or_else(|| "the stored preset did not answer the read-back".to_string())?;
    let stored = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
        .filter(|v| v.get("audioGraph").is_some())
        .ok_or_else(|| "the stored preset's read-back had no block chain".to_string())?;
    check_stored(live, &stored, &edited_nodes(pre, live, replace_targets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(nodes: Value) -> Value {
        json!({"audioGraph": {"guitarNodes": {"G1": nodes}}})
    }

    fn cab(id: &str, cab: &str, dual: bool) -> Value {
        json!({"FenderId": "ACD_CabSimTMS", "nodeId": id,
               "dspUnitParameters": {"bypass": false, "cabsimid": cab, "cabsim2enabled": dual}})
    }

    fn amp(level: f64) -> Value {
        json!({"FenderId": "ACD_MarshallPlexi", "nodeId": "ACD_MarshallPlexi",
               "dspUnitParameters": {"outputLevel": level}})
    }

    fn targets() -> Vec<NodeRef> {
        vec![("G1".into(), "cab1".into())]
    }

    #[test]
    fn a_stored_copy_holding_the_edit_proves_the_save() {
        let pre = doc(json!([amp(1.0), cab("cab1", "ACD_Twin212", false)]));
        let live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        let edited = edited_nodes(&pre, &live, &targets());
        assert_eq!(check_stored(&live, &live, &edited), Ok(()));
    }

    /// The HW loss: the stored copy keeps the pre-edit block (old id, old cab).
    #[test]
    fn a_stored_copy_that_kept_the_old_block_is_an_error() {
        let pre = doc(json!([amp(1.0), cab("cab1", "ACD_Twin212", false)]));
        let live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        let edited = edited_nodes(&pre, &live, &targets());
        let err = check_stored(&live, &pre, &edited).unwrap_err();
        assert!(
            err.contains("cab1") && err.contains("ACD_CabSimTMS"),
            "{err}"
        );
    }

    /// The other HW shape: the stored node has the edit's id but not its parameters.
    #[test]
    fn a_stored_node_without_the_edited_parameters_is_an_error() {
        let pre = doc(json!([cab("cab1", "ACD_Twin212", false)]));
        let live = doc(json!([cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        let bare = doc(
            json!([{"FenderId": "ACD_CabSimTMS", "nodeId": "ACD_CabSimTMS",
                               "dspUnitParameters": {"bypass": false}}]),
        );
        let err = check_stored(&live, &bare, &edited_nodes(&pre, &live, &targets())).unwrap_err();
        assert!(err.contains("= nothing where"), "{err}");
    }

    /// A replace that kept its node id is still checked, through its target.
    #[test]
    fn a_replace_that_kept_its_id_has_its_parameters_checked() {
        let pre = doc(json!([cab("cab1", "ACD_Twin212", false)]));
        let live = doc(json!([cab("cab1", "Diezel412FV", true)]));
        let edited = edited_nodes(&pre, &live, &targets());
        assert!(edited.contains(&("G1".to_string(), "cab1".to_string())));
        assert!(check_stored(&live, &pre, &edited).is_err());
    }

    /// With a scene current the live copy shows the scene's values on untouched nodes,
    /// while the stored copy holds base: only edited nodes' parameters are compared.
    #[test]
    fn an_untouched_node_showing_a_scene_value_is_not_a_mismatch() {
        let pre = doc(json!([amp(1.0), cab("cab1", "ACD_Twin212", false)]));
        let live = doc(json!([amp(0.5), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        let stored = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        assert_eq!(
            check_stored(&live, &stored, &edited_nodes(&pre, &live, &targets())),
            Ok(())
        );
    }

    #[test]
    fn a_stored_copy_missing_an_inserted_block_is_an_error() {
        let pre = doc(json!([amp(1.0)]));
        let live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        assert!(check_stored(&live, &pre, &edited_nodes(&pre, &live, &[])).is_err());
    }
}
