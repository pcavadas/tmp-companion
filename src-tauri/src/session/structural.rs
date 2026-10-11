//! Structural block edits: replace, remove and insert with their confirm gate,
//! `presetError` rejection, landed checks and node-JSON reads. Calls link primitives and
//! `timing` consts only.

use super::*;

impl Session {
    /// `nodeJsonRequest` (field 119){groupId,nodeId} — the node-edit-context preamble
    /// Pro Control sends immediately before a structural edit (device replies
    /// `nodeJsonResponse`, 120). Without it fw 1.8.45 drops the following `replaceNode`.
    /// On fw 1.8.58 it is a stateless read broadcast to every client (static RE): harmless,
    /// not required. Rides the live heartbeat cadence.
    pub(super) fn node_json_request(&mut self, group: &str, node_id: &str) -> Result<(), String> {
        self.send_chunked_collect(
            &proto::node_json_request(group, node_id),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.heartbeat()?;
        self.pump_silent(LIVE_BEAT_MS)?;
        Ok(())
    }

    /// True if any reply body carries `presetMessage` inner `field` (e.g. 40
    /// `nodeReplaced`, 53 `presetError`).
    pub(super) fn saw_preset_field(&self, field: u32) -> bool {
        self.preset_field(field).is_some()
    }

    /// The parsed inner fields of the LAST reply carrying `presetMessage` inner `field`.
    pub(super) fn preset_field(&self, field: u32) -> Option<Vec<(u32, proto::Val)>> {
        self.push_bodies().iter().rev().find_map(|b| {
            let top = proto::parse(b);
            let pm = proto::parse(proto::first_bytes(&top, TMS_PRESET)?);
            Some(proto::parse(proto::first_bytes(&pm, field)?))
        })
    }

    /// After sending a structural edit, pump (heartbeat) until the device CONFIRMS
    /// with `success_field` (`nodeReplaced` 40 for a replace, `nodeRemoved` 36 for a
    /// remove) → `Ok(true)`, or REJECTS with `presetError` (53) → `Ok(false)`. No reply
    /// within the window also → `Ok(false)`. The caller MUST NOT save on `Ok(false)`: a
    /// rejected/unconfirmed edit means the wrong preset is active or the node is absent,
    /// and saving would persist the WRONG content (HW: an unconfirmed save corrupted a
    /// slot).
    pub(super) fn confirm_structural_edit(&mut self, success_field: u32) -> Result<bool, String> {
        for _ in 0..10 {
            self.heartbeat()?;
            self.pump_silent(LIVE_BEAT_MS)?;
            if self.saw_preset_field(success_field) {
                return Ok(true);
            }
            if self.saw_preset_field(PRESET_ERROR) {
                return Ok(false);
            }
        }
        Ok(false)
    }

    /// Replace the node `node_id` in `dest_group` with model `fender_id` —
    /// `replaceNode` (field 39). Sends Pro Control's proven fw-1.8.45 sequence:
    /// `nodeJsonRequest` (edit context) → `replaceNode` with **NO batchStatus**, both
    /// chunked-safe and ridden on the live heartbeat the caller established via
    /// [`Self::begin_live_edit`]. **Returns `true` only when the device confirms with
    /// `nodeReplaced`** (and `false` on a `presetError`/no reply) — the caller gates
    /// the save on this.
    pub fn replace_node(
        &mut self,
        dest_group: &str,
        node_id: &str,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node(dest_group, node_id, fender_id, None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.confirm_structural_edit(NODE_REPLACED)
    }

    /// Replace the node `node_id` in `dest_group` with the user's SAVED block
    /// (`fender_id` at library `index`) — `replaceNodeWithBlock` (field 100). Same
    /// confirmed live sequence as [`Self::replace_node`]; returns `true` only on a
    /// `nodeReplaced` confirmation.
    pub fn replace_node_with_block(
        &mut self,
        dest_group: &str,
        node_id: &str,
        fender_id: &str,
        index: u64,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node_with_block(dest_group, node_id, fender_id, index, None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.confirm_structural_edit(NODE_REPLACED)
    }

    /// Remove the node `node_id` from `dest_group` — `removeNode` (field 35). Same
    /// confirmed live sequence as [`Self::replace_node`] (`nodeJsonRequest` edit-context
    /// preamble → `removeNode` with NO batchStatus, chunked-safe, on the live
    /// heartbeat); the device re-links the chain and confirms with `nodeRemoved` (36).
    /// **Returns `true` only on a `nodeRemoved` confirmation** (and `false` on a
    /// `presetError`/no reply) — the caller gates the save on this.
    pub fn remove_node(&mut self, dest_group: &str, node_id: &str) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::remove_node(dest_group, node_id, None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.confirm_structural_edit(NODE_REMOVED)
    }

    /// INSERT a new block (`fender_id`) into `dest_group` — `insertNode` (field 34).
    /// Unlike [`Self::replace_node`], Pro Control sends this **BARE**: NO
    /// `nodeJsonRequest` edit-context preamble (mirroring replace's preamble made the
    /// device REJECT the insert) and NO batchStatus — RE'd byte-exact from a PC add-block
    /// capture. `before` = the NODE ID to insert AHEAD of (the device's field 2 inserts
    /// the new block BEFORE the referenced node — HW-verified fw 1.8.45: a short-anchor
    /// insert "before X" landed the new block ahead of X), or `None` to APPEND at the end
    /// of the group. Field 2 is matched against the group's CURRENT node ids, and on fw
    /// 1.8.58 a missing one aborts the device server (static RE, danger.md): resolve it
    /// through `copy_apply`'s `DeviceIds`, never pass a FenderId. Rides the live heartbeat established by
    /// [`Self::begin_live_edit`]. Returns `true` when the device confirms with
    /// `nodeInserted` (field 33, HW-confirmed), `false` on a `presetError`(53) or no reply.
    pub fn insert_node(
        &mut self,
        dest_group: &str,
        before: Option<&str>,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::insert_node(dest_group, before, fender_id, None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        let confirmed = self.confirm_structural_edit(NODE_INSERTED)?;
        // HW evidence for whether `nodeInserted` carries the minted id (never decoded).
        log::info!(
            "insertNode {dest_group} {fender_id} before {before:?}: confirmed={confirmed} \
             device id {:?}",
            self.confirmed_node_id()
        );
        Ok(confirmed)
    }

    /// INSERT a block at a POSITION (`index`, group-relative) within `dest_group` —
    /// `insertNodeAtBlockIndex` (field 99), the index-based sibling of [`Self::insert_node`].
    /// Same bare framing (no `nodeJsonRequest` preamble), confirms on `nodeInserted` (33).
    /// HW (fw 1.8.45): `index = 0` into a single-block group landed the new block AFTER the
    /// existing one (it does NOT prepend), so the production Copy path uses the field-34
    /// `before` anchor in [`Self::insert_node`] instead; this stays as a TOOLING primitive
    /// (`probe --insert-map --at-index`) for characterising the index semantics.
    pub fn insert_node_at_index(
        &mut self,
        dest_group: &str,
        index: u32,
        fender_id: &str,
    ) -> Result<bool, String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::insert_node_at_block_index(dest_group, index as u64, fender_id, None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.confirm_structural_edit(NODE_INSERTED)
    }

    /// Did an unconfirmed insert of `fender_id` into `group` land anyway? Its confirm can
    /// be lost while the block is added, and a blind re-send would then add a second one.
    /// Reads the working copy and compares the group's count of `fender_id` with `was`
    /// (the count before the insert): one more → landed, the same → not, anything else
    /// (or no read, or one cut inside `audioGraph`) → `None`, never guessed.
    pub(super) fn insert_landed(
        &mut self,
        group: &str,
        fender_id: &str,
        was: usize,
    ) -> Option<bool> {
        let doc = self
            .live_complete_value()
            .map_err(|e| log::warn!("insert read-back failed: {e}"))
            .ok()?;
        let now = crate::blockcaps::roster_from_preset(&doc)
            .iter()
            .filter(|e| e.group == group && e.fender_id == fender_id)
            .count();
        match now.checked_sub(was) {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        }
    }

    /// The id the device gave the node the last confirmed insert/replace added: the
    /// `nodeId` in `nodeInserted`/`nodeReplaced`'s `nodeJson` (field 3 of both). The
    /// firmware mints it from the FenderId, made unique in the group with `_1`, `_2`, …
    /// (fw 1.8.58 static RE), so it depends on what the group already holds.
    pub fn confirmed_node_id(&self) -> Option<String> {
        let reply = self
            .preset_field(NODE_INSERTED)
            .or_else(|| self.preset_field(NODE_REPLACED))?;
        let v: serde_json::Value = serde_json::from_slice(proto::first_bytes(&reply, 3)?).ok()?;
        crate::audiograph::node_id(&v).map(str::to_string)
    }

    /// [`Self::confirmed_node_id`], else the one node `group` holds that is not in `known`
    /// (a working-copy read) — for a confirm that carried no id. `None` unless exactly
    /// one.
    pub(crate) fn inserted_node_id(&mut self, group: &str, known: &[String]) -> Option<String> {
        if let Some(id) = self.confirmed_node_id() {
            return Some(id);
        }
        let roster = crate::blockcaps::roster_from_preset(&self.live_complete_value().ok()?);
        let mut new = roster
            .into_iter()
            .filter(|e| e.group == group && !known.contains(&e.node_id));
        match (new.next(), new.next()) {
            (Some(e), None) => Some(e.node_id),
            _ => None,
        }
    }

    /// Insert like [`Self::insert_node`], but an unconfirmed insert with no `presetError`
    /// is read back before it is re-sent: its confirm can be lost while the block lands,
    /// and a blind re-send would add a second one. `was` is the group's count of
    /// `fender_id` before the insert; `None` never re-sends.
    pub(crate) fn insert_node_once(
        &mut self,
        group: &str,
        before: Option<&str>,
        fender_id: &str,
        was: Option<usize>,
    ) -> Result<bool, String> {
        self.insert_once_with(group, fender_id, was, |s| {
            s.insert_node(group, before, fender_id)
        })
    }

    /// [`Self::insert_node_once`] for [`Self::insert_node_at_index`].
    pub(crate) fn insert_node_at_index_once(
        &mut self,
        group: &str,
        index: u32,
        fender_id: &str,
        was: Option<usize>,
    ) -> Result<bool, String> {
        self.insert_once_with(group, fender_id, was, |s| {
            s.insert_node_at_index(group, index, fender_id)
        })
    }

    pub(super) fn insert_once_with(
        &mut self,
        group: &str,
        fender_id: &str,
        was: Option<usize>,
        send: impl Fn(&mut Self) -> Result<bool, String>,
    ) -> Result<bool, String> {
        if send(self)? {
            return Ok(true);
        }
        if self.saw_preset_error() {
            return Ok(false);
        }
        match was.and_then(|was| self.insert_landed(group, fender_id, was)) {
            Some(true) => Ok(true),
            Some(false) => send(self),
            None => Ok(false),
        }
    }

    /// True if the device REJECTED the last edit with `presetError` (53) — lets a caller
    /// distinguish a rejection (never retry, never save) from a missing confirm (read the
    /// working copy back, and re-send only an edit that visibly did not land).
    pub fn saw_preset_error(&self) -> bool {
        self.saw_preset_field(PRESET_ERROR)
    }

    /// Why the device refused the last edit, for a log or error line: `presetError`'s
    /// code (`PresetErrorMessage.error`, field 1), named where fw 1.8.58 static RE pins
    /// it, or "no confirm" when none arrived.
    pub fn rejection(&self) -> String {
        let code = self
            .preset_field(PRESET_ERROR)
            .map(|err| proto::first_varint(&err, 1).unwrap_or(0));
        match code {
            Some(14) => "presetError 14: over the device's CPU budget".to_string(),
            Some(c) => format!("presetError {c}"),
            None => "no confirm".to_string(),
        }
    }

    /// Every distinct PresetMessage inner field number present in the accumulated reply
    /// bodies, sorted ascending — a diagnostic for discovering the device's actual
    /// structural-edit reply (the add-block inbound capture caught nothing, so the insert
    /// confirm field is unverified). Used by the `--insert-active` probe report.
    pub fn seen_preset_fields(&self) -> Vec<u32> {
        let mut seen = std::collections::BTreeSet::new();
        for b in self.push_bodies() {
            let top = proto::parse(&b);
            if let Some(pm) = proto::first_bytes(&top, TMS_PRESET) {
                for (n, _) in proto::parse(pm) {
                    seen.insert(n);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Read a STRING `dspUnitParameter` of `node_id` in `dest_group` by re-requesting
    /// its node JSON (`nodeJsonResponse` 120 → `nodeJsonString` field 1). `None` if the
    /// node/param isn't found — used to VERIFY a string param set (e.g. a user-IR file)
    /// landed before saving, so an unapplied set can never persist a half-edited node.
    pub(super) fn read_node_param_str(
        &mut self,
        dest_group: &str,
        node_id: &str,
        param: &str,
    ) -> Result<Option<String>, String> {
        self.clear_raw();
        self.send_chunked_collect(
            &proto::node_json_request(dest_group, node_id),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.heartbeat()?;
        self.pump_silent(LIVE_BEAT_MS)?;
        Ok(self.push_bodies().iter().rev().find_map(|b| {
            let pm_fields = proto::parse(b);
            let pm = proto::first_bytes(&pm_fields, TMS_PRESET)?;
            let resp_fields = proto::parse(pm);
            let resp = proto::first_bytes(&resp_fields, NODE_JSON_RESPONSE)?;
            let resp_inner = proto::parse(resp);
            let node_json = proto::first_bytes(&resp_inner, 1)?; // nodeJsonString
            let v: serde_json::Value = serde_json::from_slice(node_json).ok()?;
            v.get("dspUnitParameters")?
                .get(param)?
                .as_str()
                .map(String::from)
        }))
    }

    /// Replace `node_id` in `dest_group` with a USER IR (`ACD_UserIRTMS`) pointing at
    /// `ir_file`. Two device edits on the held session: `replaceNode` → the IR block,
    /// then a STRING `changeParameter` setting the new node's `file` param to the chosen
    /// IR — VERIFIED by re-reading the node JSON. **Returns `true` only when the replace
    /// confirms (`nodeReplaced` 40) AND the `file` param reads back as `ir_file`**, so a
    /// rejected replace or an unapplied file never persists a fileless/half-edited IR
    /// node (the caller gates the save on this). NOTE: software-green, HW-validation
    /// pending (the string-param IR-file link is derived from the proto + preset JSON
    /// model, not a Pro Control capture).
    pub fn replace_node_with_ir(
        &mut self,
        dest_group: &str,
        node_id: &str,
        ir_file: &str,
    ) -> Result<bool, String> {
        self.node_json_request(dest_group, node_id)?;
        self.clear_raw();
        self.send_chunked_collect(
            &proto::replace_node(dest_group, node_id, "ACD_UserIRTMS", None),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        if !self.confirm_structural_edit(NODE_REPLACED)? {
            return Ok(false);
        }
        // The IR node's id after the replace (from nodeReplaced.nodeJson); fall back to
        // the FenderId the device assigns when no echo carries it.
        let new_id = self
            .confirmed_node_id()
            .unwrap_or_else(|| "ACD_UserIRTMS".to_string());
        self.clear_raw();
        self.send_chunked_collect(
            &proto::change_parameter_str(dest_group, &new_id, "file", ir_file),
            STRUCTURAL_EDIT_ECHO_MS,
        )?;
        self.heartbeat()?;
        self.pump_silent(IR_SETTLE_MS)?;
        Ok(self
            .read_node_param_str(dest_group, &new_id, "file")?
            .as_deref()
            == Some(ir_file))
    }
}
