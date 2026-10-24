//! The post-save proof for a structural edit: the edit counts as saved only once the slot's
//! STORED copy (the field-8 `presetDataRequest` read) shows it.
//!
//! A confirmed edit and the working copy (field 2/3) prove only the LIVE preset. A save made
//! while a non-base scene is current can store a different block chain than the live one
//! (HW, fw 1.8.58): the unit then reports a successful save, and the working copy still
//! shows the edit. So every structural save path (`held_edit::held_replace_one`,
//! `copy_apply`) compares the stored copy against the working copy it read just before the
//! save, and reports a mismatch as an error, never as "updated".
//!
//! What is compared:
//! - each group's block roster, as ordered `(nodeId, FenderId)` pairs;
//! - the `dspUnitParameters` of every EDITED node (a node id the pre-edit preset lacked, or a
//!   replace target still present), key by key from the live node.
//!
//! An edited node's stored value passes when it matches either the stored base or the stored
//! current scene's overlay of it, since the working copy can show either with a scene current.
//! The other nodes' parameters are not compared.
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
/// scene, when some OTHER scene has no entry for a target node. The firmware assumes every
/// node has an entry in every scene; a replace of one that lacks it is confirmed but not
/// stored by a save made with a scene current (HW, fw 1.8.58), while the same replace made on
/// base (`loadScene` 8) and saved after the recall back is (HW). `None`: base is current, or
/// every scene has an entry for every target.
pub(crate) fn scene_to_step_out_of(pre: &Value, targets: &[NodeRef]) -> Option<u32> {
    let scenes = pre.get("scenes").and_then(Value::as_array)?;
    let current = usize::try_from(pre.get("lastLoadedScene").and_then(Value::as_u64)?).ok()?;
    if current >= scenes.len() {
        return None;
    }
    let missing = |(group, id): &NodeRef| {
        let pointer = format!("/{}/{group}/{id}", audiograph::graph_of(group));
        scenes
            .iter()
            .enumerate()
            .any(|(i, scene)| i != current && scene.pointer(&pointer).is_none())
    };
    targets
        .iter()
        .any(missing)
        .then(|| u32::try_from(current).ok())
        .flatten()
}

/// One preset's structural edit on a held session, from the pre-edit read to the proven save:
/// the replace targets, the pre-edit document the proof tells edited nodes apart by, and the
/// scene the edits step out of to base ([`scene_to_step_out_of`]).
pub(crate) struct HeldEdit {
    pre: Value,
    targets: Vec<NodeRef>,
    scene: Option<u32>,
}

impl HeldEdit {
    /// Take the pre-edit document: the session's last parse, right after the roster read.
    pub(crate) fn begin_read(s: &Session, targets: Vec<NodeRef>) -> Result<Self, String> {
        let pre = s.current_preset_value()?;
        let scene = scene_to_step_out_of(&pre, &targets);
        Ok(Self {
            pre,
            targets,
            scene,
        })
    }

    /// Recall base before the first edit when a target needs it. `scene_discard`: DISCARD
    /// CHANGES is known to be set, whose recall back would drop the base edits, so such an
    /// edit is refused before anything is sent.
    pub(crate) fn step_out(&self, s: &mut Session, scene_discard: bool) -> Result<(), String> {
        let Some(n) = self.scene else {
            return Ok(());
        };
        if scene_discard {
            return Err(format!(
                "a block you're replacing has no setting in one of this preset's scenes, so the \
                 change has to be made on the base scene and then scene {} recalled, which \
                 DISCARD CHANGES would undo. On the unit's touchscreen, set SETTINGS → Scene \
                 Change Behavior to MAINTAIN CHANGES, then unplug and replug the USB cable and \
                 try again — not edited",
                n + 1
            ));
        }
        recall(s, session::BASE_SCENE_SLOT)
    }

    /// Recall the preset's scene again after the edits, so it keeps opening on it.
    pub(crate) fn end_edits(&self, s: &mut Session) -> Result<(), String> {
        match self.scene {
            Some(n) => recall(s, n),
            None => Ok(()),
        }
    }

    /// Rename-to-itself + save, then prove the stored copy against `live`, the working copy
    /// read before the save. A save the stored copy does not show, or one that cannot be
    /// proven, is an `Err`; so is an edit made on base that the scene recall dropped (DISCARD
    /// CHANGES, whose setting a probe run cannot read), refused before anything is saved.
    pub(crate) fn save(
        &self,
        s: &mut Session,
        list_index: ListIndex,
        name: &str,
        live: Result<Value, String>,
    ) -> Result<(), String> {
        if let (Some(_), Ok(live)) = (self.scene, &live) {
            recall_kept_edit(&self.pre, live, &self.targets)?;
        }
        // Identity-preserving persist (Pro Control's rename(current name) → save(slot)):
        // keeps the preset's name and song link.
        if !name.is_empty() {
            s.rename_current_preset(name)?;
        }
        s.save_current_preset(list_index)?;
        let live = live.map_err(|why| {
            format!(
                "saved, but the save is not proven: the edited preset could not be read ({why})"
            )
        })?;
        prove_saved(s, list_index, &self.pre, &live, &self.targets)
    }
}

/// `Err` when `live`, read after the recall back to the preset's scene, shows none of the
/// edit made on base: the same roster as `pre` and the same parameters on every edited node.
fn recall_kept_edit(pre: &Value, live: &Value, targets: &[NodeRef]) -> Result<(), String> {
    let same_params = edited_nodes(pre, live, targets)
        .iter()
        .all(|node| params(pre, node) == params(live, node));
    if roster(pre) == roster(live) && same_params {
        return Err(
            "the edit made on the base scene was gone after the scene recall (Scene \
                    Change Behavior DISCARD CHANGES?) — NOT saved"
                .to_string(),
        );
    }
    Ok(())
}

/// A scene recall that keeps the held session alive (no idle gap over 300 ms).
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
    let scene_view = stored_scene_view(live, stored);
    for node in edited {
        let Some(live_params) = params(live, node) else {
            continue;
        };
        let stored_params = params(stored, node);
        let scene_params = scene_view.as_ref().and_then(|v| params(v, node));
        for (key, value) in live_params {
            let stored_value = stored_params.and_then(|p| p.get(key));
            let scene_value = scene_params.and_then(|p| p.get(key));
            if stored_value != Some(value) && scene_value != Some(value) {
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

/// `stored` with the live preset's current scene overlaid on its block chain, the values the
/// working copy shows on that scene; `None` on base or when the stored copy lacks the scene.
fn stored_scene_view(live: &Value, stored: &Value) -> Option<Value> {
    let scene = usize::try_from(live.get("lastLoadedScene")?.as_u64()?).ok()?;
    let overlay = stored.get("scenes")?.as_array()?.get(scene)?;
    let mut view = stored.clone();
    crate::scene_jobs::overlay_scene_onto_graph(view.get_mut("audioGraph")?, overlay);
    Some(view)
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
/// `live`, the working copy read just before the save. `Err` names why: the stored copy
/// differs (the edit was lost), or it could not be read (the save is not proven).
fn prove_saved(
    s: &mut Session,
    list_index: ListIndex,
    pre: &Value,
    live: &Value,
    replace_targets: &[NodeRef],
) -> Result<(), String> {
    let unproven = |why: &str| format!("saved, but the save is not proven: {why}");
    let raw = s
        .read_slot_preset_json(list_index.into())
        .map_err(|e| unproven(&e))?
        .ok_or_else(|| unproven("the stored preset did not answer the read-back"))?;
    let stored = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
        .filter(|v| v.get("audioGraph").is_some())
        .ok_or_else(|| unproven("the stored preset's read-back had no block chain"))?;
    check_stored(live, &stored, &edited_nodes(pre, live, replace_targets))
        .map_err(|why| format!("saved, but the unit did not store the edit: {why}"))
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

    /// An edited node showing its current scene's value matches the stored scene overlay,
    /// not the stored base; a value neither holds is still a mismatch.
    #[test]
    fn an_edited_node_showing_its_scene_value_matches_the_stored_overlay() {
        let pre = doc(json!([amp(1.0), cab("cab1", "ACD_Twin212", false)]));
        let mut live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        live["lastLoadedScene"] = json!(0);
        live["audioGraph"]["guitarNodes"]["G1"][1]["dspUnitParameters"]["bypass"] = json!(true);
        let mut stored = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        stored["lastLoadedScene"] = json!(0);
        stored["scenes"] = json!([{"guitarNodes": {"G1": {
            "ACD_CabSimTMS": {"dspUnitParameters": {"bypass": true}}}}}]);
        let edited = edited_nodes(&pre, &live, &targets());
        assert_eq!(check_stored(&live, &stored, &edited), Ok(()));
        stored["scenes"][0]["guitarNodes"]["G1"]["ACD_CabSimTMS"]["dspUnitParameters"]["bypass"] =
            json!(false);
        assert!(check_stored(&live, &stored, &edited).is_err());
    }

    /// Under DISCARD CHANGES the recall back to the scene reverts the base edit, so the
    /// working copy equals the pre-edit one and the stored copy would "prove" the revert.
    #[test]
    fn a_base_edit_the_scene_recall_dropped_is_refused() {
        let pre = doc(json!([amp(1.0), cab("cab1", "ACD_Twin212", false)]));
        let err = recall_kept_edit(&pre, &pre, &targets()).unwrap_err();
        assert!(err.contains("NOT saved"), "{err}");
        let live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        assert_eq!(recall_kept_edit(&pre, &live, &targets()), Ok(()));
        let same_id = doc(json!([amp(1.0), cab("cab1", "Diezel412FV", true)]));
        assert_eq!(recall_kept_edit(&pre, &same_id, &targets()), Ok(()));
    }

    #[test]
    fn a_stored_copy_missing_an_inserted_block_is_an_error() {
        let pre = doc(json!([amp(1.0)]));
        let live = doc(json!([amp(1.0), cab("ACD_CabSimTMS", "Diezel412FV", true)]));
        assert!(check_stored(&live, &pre, &edited_nodes(&pre, &live, &[])).is_err());
    }
}
