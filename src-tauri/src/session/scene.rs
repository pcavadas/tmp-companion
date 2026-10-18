//! Scene commands: the scene list request, `load_scene` (bounds-checked against it) and
//! the per-node scene edit.
//! Calls link primitives and `timing` consts only.

use super::*;

impl Session {
    /// Activate scene `scene_slot` within the current preset — `loadScene`.
    /// Non-destructive (changes the active scene, not stored data); enables per-scene
    /// re-amp capture. Load a preset first, then drive scenes.
    ///
    /// Any index but [`BASE_SCENE_SLOT`] is first checked against the live scene count
    /// ([`Self::live_scene_count`]) and refused, unsent, at or past it: on a scene-less
    /// preset the unit swaps in its EMPTY template, and past the scenes it likely aborts
    /// its server (static RE, fw 1.8.58).
    pub fn load_scene(&mut self, scene_slot: u32) -> Result<(), String> {
        self.check_scene_index(scene_slot)?;
        // Eager pump — same fire-and-forget rationale as `load_preset`.
        self.transact_eager(&proto::load_scene(scene_slot as u64), LOAD_ECHO_MS)?;
        Ok(())
    }

    /// [`Self::load_scene`] (same bounds check, same refusal), then the recalled scene's
    /// live document from the field-3 push the recall draws: `None` when none parses
    /// within 4 kept-alive beats.
    pub(crate) fn recall_scene_doc(
        &mut self,
        scene_slot: u32,
    ) -> Result<Option<serde_json::Value>, String> {
        self.check_scene_index(scene_slot)?;
        self.raw.clear();
        self.send_and_collect(&proto::load_scene(scene_slot as u64), LOAD_ECHO_MS)?;
        for _ in 0..4 {
            self.heartbeat()?;
            self.pump_silent(RECALL_DOC_BEAT_MS)?;
            if let Ok(v) = self.current_preset_value() {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    /// Refuse a `loadScene` index the loaded preset lacks: [`BASE_SCENE_SLOT`] always
    /// passes, any other index must be under the scene count proven on this connection
    /// ([`Self::reported_scene_count`]), else under [`Self::live_scene_count`].
    fn check_scene_index(&mut self, scene_slot: u32) -> Result<(), String> {
        if scene_slot == BASE_SCENE_SLOT {
            return Ok(());
        }
        let count = match self.reported_scene_count() {
            Some(count) => count,
            None => self.live_scene_count()?,
        };
        if scene_slot as usize >= count {
            return Err(format!(
                "scene {} isn't on the loaded preset ({count} scenes) — loadScene not sent",
                scene_slot + 1
            ));
        }
        Ok(())
    }

    /// The active preset's scene count, asked fresh: `sceneListRequest` (126) answered with
    /// `sceneListResponse` (125), one name per `scenes[]` entry. Only reports that land
    /// after the request are read, so a list buffered before a preset change cannot answer;
    /// no reply is an error, never a guess.
    pub(crate) fn live_scene_count(&mut self) -> Result<usize, String> {
        let mark = self.raw.len();
        let reply = |s: &Self| {
            proto::reassemble_streams(&s.raw[mark..])
                .iter()
                .find_map(|st| decode_scene_list(&st.body))
                .map(|names| names.len())
        };
        let reports = self.transact_eager(&proto::scene_list_request(), SCENE_LIST_POLL_MS)?;
        self.raw.extend(reports);
        for _ in 0..SCENE_LIST_POLLS {
            if let Some(count) = reply(self) {
                return Ok(count);
            }
            self.pump_collect_alive(SCENE_LIST_POLL_MS)?;
        }
        reply(self).ok_or_else(|| {
            "the unit did not answer sceneListRequest, so its scene count is unknown — \
             loadScene not sent"
                .to_string()
        })
    }

    /// Enable/disable per-block Scene Edit on `(group_id, node_id)` for the ACTIVE
    /// scene — once enabled, a `change_parameter` on that block writes the scene
    /// overlay (per-scene), not the base. Fire-and-forget setter (no batchStatus),
    /// like `change_parameter`. Used by per-scene leveling.
    pub fn set_node_scene_edit(
        &mut self,
        group_id: &str,
        node_id: &str,
        enable: bool,
    ) -> Result<(), String> {
        self.transact(
            &proto::set_node_scene_edit(group_id, node_id, enable),
            SCENE_EDIT_ECHO_MS,
        )?;
        Ok(())
    }
}
