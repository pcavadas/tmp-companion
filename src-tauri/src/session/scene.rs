//! Scene commands: the scene list request, `load_scene` and the per-node scene edit.
//! Calls link primitives and `timing` consts only.

use super::*;

impl Session {
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
