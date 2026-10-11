//! Scene commands: the scene list request, `load_scene` and the per-node scene edit.
//! Calls link primitives and `timing` consts only.

use super::*;

impl Session {
    /// Request the active preset's scene list on demand — `sceneListRequest`
    /// (field 126, no batch; addresses the CURRENT preset like `loadScene`).
    /// Harvests the `sceneListResponse` (125) `sceneList` strings. The device
    /// also pushes this UNSOLICITED on every preset load (the monitor's primary
    /// path); this is a manual / first-paint top-up for a mid-preset connect.
    pub fn request_scene_list(&mut self) -> Result<Vec<String>, String> {
        self.raw.clear();
        self.send_and_collect(&proto::scene_list_request(), KEEPALIVE_WINDOW_MS)?;
        for _ in 0..6 {
            if let Some(names) = self.push_bodies().iter().find_map(|b| decode_scene_list(b)) {
                return Ok(names);
            }
            self.keepalive_pump()?;
        }
        self.push_bodies()
            .iter()
            .find_map(|b| decode_scene_list(b))
            .ok_or_else(|| "no sceneListResponse received from device".to_string())
    }

    /// Activate scene `scene_slot` within the current preset — `loadScene`.
    /// Non-destructive (changes the active scene, not stored data); enables per-scene
    /// re-amp capture. Load a preset first, then drive scenes.
    pub fn load_scene(&mut self, scene_slot: u32) -> Result<(), String> {
        // Eager pump — same fire-and-forget rationale as `load_preset`.
        self.transact_eager(&proto::load_scene(scene_slot as u64), LOAD_ECHO_MS)?;
        Ok(())
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
