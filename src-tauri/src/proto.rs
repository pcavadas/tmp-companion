//! Hand-rolled FenderMessageTMS wire codec.
//!
//! We hand-roll rather than pull in `prost` because only a dozen messages are
//! needed and `PresetMessage` transitively imports ~130 `.proto` files. The unit
//! tests pin every builder to a golden byte vector, so any drift is caught at
//! `cargo test`.
//!
//! Wire envelope (HID report payload, 63 bytes):
//!   `0x35 0x00 <body_len> <protobuf FenderMessageTMS>` zero-padded to 63.
//!
//! Top-level `FenderMessageTMS` oneof slots used here:
//!   2 presetMessage · 4 connectionMessage · 5 mixerMessage · field 10 = batchStatus.

/// Maximum protobuf body that fits in a single 63-byte output report
/// (`0x35 0x00 <len>` header = 3 bytes).
pub const MAX_BODY: usize = 60;

const MAGIC_OUT: u8 = 0x35;
/// Device → host framing magics (see `reassemble_streams`).
pub const MAGIC_IN_FRAME: u8 = 0x34; // multi-packet continuation
pub const MAGIC_IN_START: u8 = 0x33; // multi-packet stream start

// ─── low-level wire encoding ────────────────────────────────────────────────

fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

fn tag(field_no: u32, wire_type: u8) -> u64 {
    ((field_no as u64) << 3) | (wire_type as u64)
}

/// Append a varint field (wire type 0).
pub(crate) fn field_varint(out: &mut Vec<u8>, field_no: u32, value: u64) {
    put_varint(out, tag(field_no, 0));
    put_varint(out, value);
}

/// Append a length-delimited field (wire type 2).
fn field_bytes(out: &mut Vec<u8>, field_no: u32, value: &[u8]) {
    put_varint(out, tag(field_no, 2));
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

/// Append a 32-bit float field (wire type 5, little-endian). The Python
/// `pb_field` only supports varint/len-delim; `SetPresetLevel.presetLevel` is a
/// `float`, so this helper is the one addition called out in the plan.
pub(crate) fn field_f32(out: &mut Vec<u8>, field_no: u32, value: f32) {
    put_varint(out, tag(field_no, 5));
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn len_delimited(field_no: u32, inner: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    field_bytes(&mut out, field_no, inner);
    out
}

// ─── envelope ───────────────────────────────────────────────────────────────

/// Wrap a protobuf body in the `0x35 0x00 <len> …` envelope, zero-padded to 63
/// bytes. Panics if the body exceeds a single report — callers split or use a
/// message that fits (all leveling messages do).
pub fn make_envelope(body: &[u8]) -> [u8; 63] {
    assert!(
        body.len() <= MAX_BODY,
        "body too large for single report: {}",
        body.len()
    );
    let mut pkt = [0u8; 63];
    pkt[0] = MAGIC_OUT;
    pkt[1] = 0x00;
    pkt[2] = body.len() as u8;
    pkt[3..3 + body.len()].copy_from_slice(body);
    pkt
}

/// Frame a FenderMessageTMS body into one or more 63-byte output reports.
///
/// A body ≤ `MAX_BODY` is a single `0x35` frame (identical to `make_envelope`).
/// A larger body is split into `0x35`-final / `0x34`-continuation / `0x33`-start
/// frames: `0x33` first, `0x34` for the middle, `0x35` last. Each frame is
/// `MAGIC 0x00 <len> <≤60 payload>`. This is the outbound multi-packet scheme
/// observed from Pro Control driving an `importPresetRequest` — the
/// same framing the device uses inbound (`reassemble_streams`).
pub fn make_chunked_envelopes(body: &[u8]) -> Vec<[u8; 63]> {
    if body.len() <= MAX_BODY {
        return vec![make_envelope(body)];
    }
    let pieces: Vec<&[u8]> = body.chunks(MAX_BODY).collect();
    let last = pieces.len() - 1;
    pieces
        .iter()
        .enumerate()
        .map(|(i, piece)| {
            let magic = if i == 0 {
                MAGIC_IN_START // 0x33 — first of many
            } else if i == last {
                MAGIC_OUT // 0x35 — final
            } else {
                MAGIC_IN_FRAME // 0x34 — continuation
            };
            let mut pkt = [0u8; 63];
            pkt[0] = magic;
            pkt[1] = 0x00;
            pkt[2] = piece.len() as u8;
            pkt[3..3 + piece.len()].copy_from_slice(piece);
            pkt
        })
        .collect()
}

/// Encode `data` as an all-literal LZ4 *block* (a single literals-only sequence,
/// no back-references). Valid per the LZ4 block spec and accepted by
/// [`lz4_block_decompress`]; emitting matches would only shrink the payload, not
/// change correctness, so this avoids an LZ4-compressor dependency. The on-wire
/// `importPresetRequest.presetJson` is `LZ4_block(raw .preset bytes)` (AC3).
pub fn lz4_block_compress_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 1 + data.len() / 255);
    let lit = data.len();
    out.push(if lit >= 15 { 0xF0 } else { (lit as u8) << 4 });
    if lit >= 15 {
        let mut rem = lit - 15;
        while rem >= 255 {
            out.push(255);
            rem -= 255;
        }
        out.push(rem as u8);
    }
    out.extend_from_slice(data);
    out
}

// ─── message builders (each returns the FenderMessageTMS body) ───────────────

/// `TMS.<tms_field>{ <inner_field>{ field1 = inner_value } }` with optional
/// `batchStatus` (field 10). Heartbeat omits batch — the device rejects a
/// heartbeat that carries one.
fn request(tms_field: u32, inner_field: u32, inner_value: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, inner_value);
    let mut body = len_delimited(tms_field, &len_delimited(inner_field, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// ConnectionMessage.connectionRequest{ dummy = true }.
pub fn connection_request() -> Vec<u8> {
    request(4, 1, 1, None)
}

/// ConnectionMessage.connectionHeartbeat{ dummy = true }. NO batchStatus
/// (load-bearing: the device replies `ConnectionError` if one is present).
pub fn heartbeat() -> Vec<u8> {
    request(4, 4, 1, None)
}

/// PresetMessage.presetListRequest{ listEnum = filter } (1 = My Presets).
pub fn preset_list_request(filter: u64, batch: u64) -> Vec<u8> {
    request(2, 4, filter, Some(batch))
}

/// PresetMessage.currentPresetInfoRequest{ dummy = true }.
pub fn current_preset_info_request(batch: u64) -> Vec<u8> {
    request(2, 1, 1, Some(batch))
}

/// PresetMessage.sceneListRequest{ dummy = true } (field **126**) — fetch the
/// active preset's live scene list on demand. The device replies with
/// `sceneListResponse` (field 125) carrying `sceneList` (repeated string) — the
/// same push the unit emits unsolicited on every preset LOAD. Like `loadScene`
/// it addresses the CURRENT preset only (no slot), so it carries NO `batchStatus`
/// (a command/request-on-current-preset, same rule as the setters). The monitor
/// already surfaces the unsolicited push as the primary path; this is a manual /
/// first-paint top-up for the mid-preset connect case.
pub fn scene_list_request() -> Vec<u8> {
    request(2, 126, 1, None)
}

// Remaining first-connect handshake messages the device expects.
// Replicated so the device sees the canonical sequence.
pub fn favorite_list_request(batch: u64) -> Vec<u8> {
    request(2, 6, 1, Some(batch))
}
pub fn current_preset_data_request(batch: u64) -> Vec<u8> {
    request(2, 2, 1, Some(batch))
}
pub fn product_profile_request(batch: u64) -> Vec<u8> {
    request(2, 41, 1, Some(batch))
}
pub fn settings_field66(batch: u64) -> Vec<u8> {
    request(3, 66, 1, Some(batch))
}
pub fn userir_field2(batch: u64) -> Vec<u8> {
    request(13, 2, 1, Some(batch))
}

/// PresetMessage.requestAllBlockPresets{ dummy = true } (field **135**) → the device
/// replies with `allBlockPresetsResponse` (field 136) carrying `blockPresetsMap`
/// (bytes) — the user's saved-block store. `batch = None` omits the batchStatus
/// trailer (the request/response framing, like the ReplaceNode family); `Some(b)`
/// rides the handshake burst like the other batch-2 reads.
pub fn request_all_block_presets(batch: Option<u64>) -> Vec<u8> {
    request(2, 135, 1, batch)
}

/// SettingsMessage.currentFwRequest{ dummy = true } → the device replies with
/// `currentFwResponse{ data = "<firmware version>" }`. **NO `batchStatus`** and
/// it must ride INSIDE the handshake burst: with a batch the device answers
/// nothing, and standalone after the burst it replies `ConnectionError`
/// (HW-confirmed — same omit-batch rule as the setters). The reply
/// is harvested by `session::extract_fw_version`.
pub fn current_fw_request() -> Vec<u8> {
    request(3, 1, 1, None)
}

/// PresetMessage.currentPresetDataJsonRequest{ dummy = true }.
pub fn current_preset_data_json_request(batch: u64) -> Vec<u8> {
    request(2, 78, 1, Some(batch))
}

/// FenderMessageTMS.backupMessage(8).backupRequest(1){ dummy = true }. Triggers
/// the device's bulk library dump: `tm-stomp-server`'s `fmic::BackupManager`
/// builds (in RAM, `/var/tmp` tmpfs) a GNU-tar + LZ4-frame archive of `/data`
/// (`normalDb.db3` user-preset DB + settings/userIRs/ACD defaults) via libarchive
/// and streams it back as `backupRestoreData(2){uuid,crc,numBytes,numChunks,
/// chunkNum,chunkData}` chunks interleaved with `backupRestoreState(3)` progress.
/// NO `batchStatus` — it's a one-shot command, same omit-batch rule as the setters.
pub fn backup_request() -> Vec<u8> {
    let mut br = Vec::new();
    field_varint(&mut br, 1, 1); // BackupRequest.dummy = true
    len_delimited(8, &len_delimited(1, &br))
}

/// PresetMessage.loadPreset{ tabEnum, presetSlot }. `tab_enum` 0 omitted (1 =
/// UserPresets). presetSlot is field 6. NO `batchStatus` — LoadPreset is a
/// command; with a batchStatus the device silently ignores it (confirmed live:
/// with-batch → 0 response streams, no-batch → responds). Same rule as the
/// other setters; only requests (PresetListRequest, …) carry a batch.
pub fn load_preset(preset_slot: u64, tab_enum: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if tab_enum != 0 {
        field_varint(&mut inner, 1, tab_enum);
    }
    field_varint(&mut inner, 6, preset_slot);
    len_delimited(2, &len_delimited(10, &inner))
}

/// PresetMessage.setPresetLevel{ presetLevel } — float clamped to 0.0..=1.0.
/// NO `batchStatus`: like the re-amp setter, the device silently ignores this
/// mutation if a batchStatus is present (confirmed live — a level sweep was
/// flat until the batch was dropped). Setters omit batch; requests include it.
pub fn set_preset_level(level: f32) -> Vec<u8> {
    let mut inner = Vec::new();
    field_f32(&mut inner, 1, level.clamp(0.0, 1.0));
    len_delimited(2, &len_delimited(76, &inner))
}

/// PresetMessage.changeParameter{ groupId, nodeId, parameterId, floatVal } —
/// set one block control. groupId/nodeId/parameterId are strings (e.g. "G1",
/// "ACD_TM59Bassman", "outputLevel"); the value goes in the `value` oneof's
/// floatVal slot (field 5). NO `batchStatus` (setter rule — with it the device
/// silently ignores the change). HW-proven to move loudness; needs no
/// SetNodeSceneEdit and is latched by re-amp at engage.
pub fn change_parameter(group_id: &str, node_id: &str, parameter_id: &str, value: f32) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    field_bytes(&mut inner, 3, parameter_id.as_bytes());
    field_f32(&mut inner, 5, value);
    len_delimited(2, &len_delimited(12, &inner))
}

/// `changeParameter` for a STRING-valued parameter (the `value` oneof's `stringVal`,
/// field 6) — e.g. a user-IR node's `file` (the impulse-response filename). Same
/// framing/NO-batchStatus rule as the float [`change_parameter`].
pub fn change_parameter_str(
    group_id: &str,
    node_id: &str,
    parameter_id: &str,
    value: &str,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    field_bytes(&mut inner, 3, parameter_id.as_bytes());
    field_bytes(&mut inner, 6, value.as_bytes());
    len_delimited(2, &len_delimited(12, &inner))
}

/// `changeParameter` for a BOOL-valued parameter (the `value` oneof's `boolVal`,
/// field 7) — e.g. a block's `bypass`. The oneof field is ALWAYS emitted (even
/// `false`), so the device sees an explicit value. Same framing / NO-batchStatus
/// rule as the float [`change_parameter`]. Used to force a block active/bypassed
/// while measuring an off-in-base footswitch block for the bake path.
pub fn change_parameter_bool(
    group_id: &str,
    node_id: &str,
    parameter_id: &str,
    value: bool,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    field_bytes(&mut inner, 3, parameter_id.as_bytes());
    field_varint(&mut inner, 7, u64::from(value));
    len_delimited(2, &len_delimited(12, &inner))
}

/// PresetMessage.replaceNode{ destGroup, nodeIdToReplace, fenderId } (field **39**) —
/// swap a node to a different model; the device fills the new model's default
/// parameters and emits `nodeReplaced` (40). Unlike the pure setters this is
/// request/response-shaped, so it REQUIRES a top-level `batchStatus` (field 10) — the
/// device silently ignores it without one (HW-confirmed; matches the proven
/// `drive_replace_node.py` framing, which sends batch=11).
/// PresetMessage.nodeJsonRequest (field 119){ groupId=1, nodeId=2 } — Pro Control
/// sends this immediately BEFORE `replaceNode` to put the device into the
/// node-edit context (the device replies `nodeJsonResponse`, field 120). Without
/// it, fw 1.8.45 silently DROPS the subsequent `replaceNode` (HW-confirmed via a
/// Pro Control USB capture). NO batchStatus (matches the capture).
pub fn node_json_request(group_id: &str, node_id: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    len_delimited(2, &len_delimited(119, &inner))
}

pub fn replace_node(
    dest_group: &str,
    node_id: &str,
    fender_id: &str,
    batch: Option<u64>,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, dest_group.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    field_bytes(&mut inner, 3, fender_id.as_bytes());
    let mut body = len_delimited(2, &len_delimited(39, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.insertNode{ groupId=1, nodeIdInsertLocation=2, fenderId=3 } (field
/// **34**) — ADD a new block to the chain. Byte-exact from an observed add-block
/// exchange (fw 1.8.45): the device sends `insertNode` **BARE** — NO `nodeJsonRequest`(119) edit-context preamble
/// (unlike [`replace_node`]; mirroring replace's preamble made the device REJECT the
/// insert) and NO `batchStatus`. Field 2 is OPTIONAL: omit to **APPEND** at the end of
/// the group, or set it to the **FenderId to insert BEFORE** (HW-verified fw 1.8.45: the
/// new block lands AHEAD of the field-2 node — the "insert after" label from the capture
/// was a misread). `group_id` is the group KEY ("G1".."G7" / "M1".."M4"), the same
/// convention as [`replace_node`]. The device confirms with `nodeInserted` (field 33).
pub fn insert_node(
    group_id: &str,
    insert_before_fender_id: Option<&str>,
    fender_id: &str,
    batch: Option<u64>,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    if let Some(loc) = insert_before_fender_id {
        field_bytes(&mut inner, 2, loc.as_bytes());
    }
    field_bytes(&mut inner, 3, fender_id.as_bytes());
    let mut body = len_delimited(2, &len_delimited(34, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.insertNodeAtBlockIndex{ groupId, fenderId, index } (field **99**) —
/// insert a fresh `fender_id` into `group_id` at a POSITION (`index`, group-relative),
/// the index-based sibling of [`insert_node`]'s `before`-anchor. Inner field numbers:
/// groupId = 1, fenderId = 3, index = 4. Same bare framing as [`insert_node`].
/// HW (fw 1.8.45): `index = 0` into a single-block group landed the new block AFTER the
/// existing one (it does NOT prepend), so the production Copy path uses [`insert_node`]'s
/// `before`-anchor instead; this stays a TOOLING primitive (`probe --insert-map
/// --at-index`) for characterising the index semantics.
pub fn insert_node_at_block_index(
    group_id: &str,
    index: u64,
    fender_id: &str,
    batch: Option<u64>,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, group_id.as_bytes());
    field_bytes(&mut inner, 3, fender_id.as_bytes());
    field_varint(&mut inner, 4, index);
    let mut body = len_delimited(2, &len_delimited(99, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.replaceNodeWithBlock{ destGroup, nodeIdToReplace, fenderId, index }
/// (field **100**) — swap a node to one of the user's SAVED blocks; `index` selects
/// the saved entry within that fenderId's library list (the `blockPresetsMap` array
/// position). Same request/response framing
/// as `replace_node`: REQUIRES `batchStatus`.
pub fn replace_node_with_block(
    dest_group: &str,
    node_id: &str,
    fender_id: &str,
    index: u64,
    batch: Option<u64>,
) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, dest_group.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    field_bytes(&mut inner, 3, fender_id.as_bytes());
    field_varint(&mut inner, 4, index);
    let mut body = len_delimited(2, &len_delimited(100, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.removeNode{ destGroup=1, nodeToRemove=2 } (field **35**) — delete a
/// node from the chain (the device re-links the surrounding nodes). Same node-edit
/// sequence and framing as [`replace_node`] (preceded by `nodeJsonRequest`, NO
/// batchStatus); the device confirms with `nodeRemoved` (field 36).
pub fn remove_node(dest_group: &str, node_id: &str, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, dest_group.as_bytes());
    field_bytes(&mut inner, 2, node_id.as_bytes());
    let mut body = len_delimited(2, &len_delimited(35, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.saveCurrentPreset{ userSlot }. No `batchStatus` (setter rule).
pub fn save_current_preset(user_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, user_slot);
    len_delimited(2, &len_delimited(14, &inner))
}

/// PresetMessage.renameCurrentPreset{ displayName } — rename the current edit
/// buffer (field **13**). Setter: NO `batchStatus`. Persisted via a subsequent
/// `saveCurrentPreset`. Goldens here are spec-derived from `PresetMessage.proto` +
/// `RenameCurrentPreset.proto`; confirm byte-exact against a Pro Control capture
/// before relying on framing.
pub fn rename_current_preset(display_name: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, display_name.as_bytes());
    len_delimited(2, &len_delimited(13, &inner))
}

/// PresetMessage.moveUserPreset{ oldSlot, newSlot } — relocate a user preset
/// (field **16**). Setter: NO `batchStatus`. proto3 zero-omission applies.
pub fn move_user_preset(old_slot: u64, new_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if old_slot != 0 {
        field_varint(&mut inner, 1, old_slot);
    }
    if new_slot != 0 {
        field_varint(&mut inner, 2, new_slot);
    }
    len_delimited(2, &len_delimited(16, &inner))
}

/// PresetMessage.clearUserPreset{ userSlot } — empty a user slot (field **15**).
/// Setter: NO `batchStatus`. proto3 zero-omission applies.
pub fn clear_user_preset(user_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if user_slot != 0 {
        field_varint(&mut inner, 1, user_slot);
    }
    len_delimited(2, &len_delimited(15, &inner))
}

/// PresetMessage.setFootswitchAssignment{ footswitchAddress=1, functionIndex=2,
/// functionJson=3, swap=4 } (field **54**). Sets ONE function (slot `function_index`,
/// 0-based, of the firmware's 5-per-switch cap) on footswitch `footswitch_address` to the
/// assignment object `function_json` — the SAME JSON shape carried in the preset's
/// top-level `ftsw[switch][func]` (e.g. an `on-off` or a `param` function; schemas RE'd
/// verbatim from the Pro Control 1.8.2 binary's embedded reference preset). `swap` is a
/// bool flag (semantics empirically TBD — `probe --ftsw-validate`). Setter framing: by
/// default NO `batchStatus` (mirrors `changeParameter`); the `batch` param exists only so
/// the validation probe can A/B whether the device requires one. proto3 zero-omission: a 0
/// `footswitchAddress`/`functionIndex` and `swap=false` are dropped to match the real
/// serializer.
pub fn set_footswitch_assignment(
    footswitch_address: u64,
    function_index: u64,
    function_json: &str,
    swap: bool,
    batch: Option<u64>,
) -> Vec<u8> {
    let mut inner = Vec::new();
    if footswitch_address != 0 {
        field_varint(&mut inner, 1, footswitch_address);
    }
    if function_index != 0 {
        field_varint(&mut inner, 2, function_index);
    }
    field_bytes(&mut inner, 3, function_json.as_bytes());
    if swap {
        field_varint(&mut inner, 4, 1);
    }
    let mut body = len_delimited(2, &len_delimited(54, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.clearFootswitchAssignment{ footswitchAddress=1, functionIndex=2 }
/// (field **55**) — remove ONE function from a footswitch. Setter framing (NO
/// batchStatus); proto3 zero-omission.
pub fn clear_footswitch_assignment(footswitch_address: u64, function_index: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if footswitch_address != 0 {
        field_varint(&mut inner, 1, footswitch_address);
    }
    if function_index != 0 {
        field_varint(&mut inner, 2, function_index);
    }
    len_delimited(2, &len_delimited(55, &inner))
}

/// Set re-amp mode via the Global Settings path:
/// `SettingsMessage(3) → reampModeActive(30) → ReampModeActive{ value }`.
///
/// Byte-verified against the device's live USB-HID traffic:
/// ON = `1a 05 f2 01 02 08 01`, OFF = `1a 03 f2 01 00`. Two things
/// are load-bearing and were the reason an earlier version was silently ignored:
///   1. NO `batchStatus` (field 10). Like the heartbeat, the device rejects a
///      Global-Settings setter that carries one.
///   2. OFF sends an EMPTY `ReampModeActive` (proto3 omits the default `false`),
///      not an explicit `value=0`.
pub fn set_reamp_mode(active: bool) -> Vec<u8> {
    let mut inner = Vec::new();
    if active {
        field_varint(&mut inner, 1, 1); // value=true; false is the omitted default
    }
    len_delimited(3, &len_delimited(30, &inner))
}

/// PresetMessage.exportPresetRequest{ listEnum, presetSlot } — field **115** in
/// `PresetMessage` (TMS field 2); inner `ExportPresetRequest{ listEnum=1,
/// presetSlot=2 }`.
///
/// **DEAD on firmware 1.7.75 (AC1 finding):** the device sends NO
/// `exportPresetResponse` (116), in or out of the handshake burst, with any batch
/// grouping — it appears unimplemented. Kept (with its golden test) to document
/// the wire format and make re-testing on a future firmware trivial; the live
/// slot-read path is `preset_data_request` (field 8). proto3 zero-value omission:
/// `listEnum`/`presetSlot` of 0 are dropped to match the real serializer.
#[allow(dead_code)]
pub fn export_preset_request(list_enum: u64, preset_slot: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    if list_enum != 0 {
        field_varint(&mut inner, 1, list_enum);
    }
    if preset_slot != 0 {
        field_varint(&mut inner, 2, preset_slot);
    }
    let mut body = len_delimited(2, &len_delimited(115, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.presetDataRequest{ listEnum, presetSlot } — the *slot-addressed*
/// full-preset read (field **8**), distinct from `exportPresetRequest` (115).
/// The device replies with `presetDataChanged` (field 9) carrying `presetJson`.
/// This is the carrier `best_json_payload` already harvests as `(9, 3)`, so it
/// is the most likely full-read path. Same optional-batch + proto3-zero-omission
/// rules as `export_preset_request`.
pub fn preset_data_request(list_enum: u64, preset_slot: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    if list_enum != 0 {
        field_varint(&mut inner, 1, list_enum);
    }
    if preset_slot != 0 {
        field_varint(&mut inner, 2, preset_slot);
    }
    let mut body = len_delimited(2, &len_delimited(8, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// PresetMessage.importPresetRequest{ presetJson } — write a full preset back to
/// the device (field **117**; inner `ImportPresetRequest{ presetJson=1 bytes }`).
///
/// Returns only the protobuf BODY (no batchStatus — sent standalone). `payload`
/// is the on-wire `presetJson`, which is `lz4_block_compress_stored(raw .preset
/// bytes)` (AC3, from a Pro Control import capture). The body is multi-KB; send it
/// via [`make_chunked_envelopes`], not `make_envelope`.
pub fn import_preset_request(payload: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, payload);
    len_delimited(2, &len_delimited(117, &inner))
}

/// PresetMessage.requestNextEmptyPresetSlot{ afterSlot } — field **81**; inner
/// `RequestNextEmptyPresetSlot{ afterSlot=1 }`; reply `nextEmptyPresetSlotResponse`
/// (82). proto3 zero-omission on `afterSlot`; optional `batchStatus`.
///
/// **DEAD on firmware 1.7.75 (like `exportPresetRequest`):** the device sends NO
/// `nextEmptyPresetSlotResponse`, standalone or in-burst, any batch. Kept (with its
/// golden + decode test) to document the wire format and make re-testing on a future
/// firmware trivial; the live scratch-slot picker finds the empty slot by
/// **observation** (import, then re-list) — see `lib::run_replace_inplace`.
#[allow(dead_code)]
pub fn request_next_empty_preset_slot(after_slot: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    if after_slot != 0 {
        field_varint(&mut inner, 1, after_slot);
    }
    let mut body = len_delimited(2, &len_delimited(81, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// SongMessage.songPresetListRequest{ songSlot } — read a Song's preset
/// assignments. Top-level `FenderMessageTMS.songMessage` is field **11** (not the
/// usual `presetMessage` field 2); inner `SongPresetListRequest{ songSlot=1 }` is
/// field **12**. The device replies with `songPresetListResponse` (13), whose
/// records carry `userPresetSlot` (the real device slot a Song row points at) and
/// `presetSceneSlot` — the positional binding AC7 must preserve. proto3
/// zero-omission on `songSlot`; optional `batchStatus` for in-burst sends.
pub fn song_preset_list_request(song_slot: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    let mut body = len_delimited(11, &len_delimited(12, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// SongMessage.songListRequest{ dummy } — read the Song list. Top-level
/// `FenderMessageTMS.songMessage` is field **11**; inner `SongListRequest` is field
/// **2** with `dummy` (field 1) always set true (a request marker, like the other
/// `dummy=true` requests). The device replies with `songListResponse` (field 3).
/// Optional `batchStatus` (field 10) for an in-burst send.
pub fn song_list_request(batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, 1); // dummy = true (always set)
    let mut body = len_delimited(11, &len_delimited(2, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// SetlistMessage.setlistListRequest{ dummy } — read the Setlist list. Top-level
/// `FenderMessageTMS.setlistMessage` is field **12**; inner `SetlistListRequest`
/// is field **2** with `dummy` (field 1) always set true (a request marker, like
/// `songListRequest`). The device replies with `setlistListResponse` (field 3),
/// whose records carry only `setlistName` (field 1) per `SetlistListRecord.proto`
/// — a setlist's song membership is a SEPARATE read (`setlistSongListRequest`, 12).
/// Optional `batchStatus` (field 10) for an in-burst send (setlist reads ride the
/// handshake burst and need a batch, same as song reads).
pub fn setlist_list_request(batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, 1); // dummy = true (always set)
    let mut body = len_delimited(12, &len_delimited(2, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// SetlistMessage.setlistSongListRequest{ setlistSlot } — read which songs a
/// Setlist contains. SetlistMessage is TMS field **12**; inner
/// `setlistSongListRequest` is field **12**; `SetlistSongListRequest{ setlistSlot=1 }`.
/// The device replies with `setlistSongListResponse` (13), whose repeated records
/// (`SetlistSongListRecord{ songSlot=1 }`) reference songs by slot. proto3
/// zero-omission on `setlistSlot`; optional `batchStatus` (10) for an in-burst send.
pub fn setlist_song_list_request(setlist_slot: u64, batch: Option<u64>) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    let mut body = len_delimited(12, &len_delimited(12, &inner));
    if let Some(b) = batch {
        field_varint(&mut body, 10, b);
    }
    body
}

/// SongMessage.assignSongPreset — bind a user preset (+ scene) to a Song row.
/// SongMessage is TMS field **11**; inner `assignSongPreset` is field
/// **14**; `AssignSongPreset{ songSlot=1, songPresetSlot=2, userPresetSlot=3,
/// footswitchLabel=4, footswitchColor=5, presetSceneSlot=6 }`. `userPresetSlot` is
/// 1-based (device slot = list index + 1; the caller applies the +1, like the other
/// setters). A setter → NO batchStatus. proto3 zero-omission on numeric fields.
pub fn assign_song_preset(
    song_slot: u64,
    song_preset_slot: u64,
    user_preset_slot: u64,
    footswitch_label: &str,
    footswitch_color: u64,
    preset_scene_slot: u64,
) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if song_preset_slot != 0 {
        field_varint(&mut inner, 2, song_preset_slot);
    }
    if user_preset_slot != 0 {
        field_varint(&mut inner, 3, user_preset_slot);
    }
    if !footswitch_label.is_empty() {
        field_bytes(&mut inner, 4, footswitch_label.as_bytes());
    }
    if footswitch_color != 0 {
        field_varint(&mut inner, 5, footswitch_color);
    }
    if preset_scene_slot != 0 {
        field_varint(&mut inner, 6, preset_scene_slot);
    }
    len_delimited(11, &len_delimited(14, &inner))
}

/// SongMessage.moveSongPreset — reorder a Song row. Field **15**;
/// `MoveSongPreset{ songSlot=1, oldSongPresetSlot=2, newSongPresetSlot=3 }`. Setter.
pub fn move_song_preset(song_slot: u64, old_slot: u64, new_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if old_slot != 0 {
        field_varint(&mut inner, 2, old_slot);
    }
    if new_slot != 0 {
        field_varint(&mut inner, 3, new_slot);
    }
    len_delimited(11, &len_delimited(15, &inner))
}

/// SongMessage.swapSongPreset — swap two Song rows. Field **16**;
/// `SwapSongPreset{ songSlot=1, songPresetSlotA=2, songPresetSlotB=3 }`. Setter.
pub fn swap_song_preset(song_slot: u64, slot_a: u64, slot_b: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if slot_a != 0 {
        field_varint(&mut inner, 2, slot_a);
    }
    if slot_b != 0 {
        field_varint(&mut inner, 3, slot_b);
    }
    len_delimited(11, &len_delimited(16, &inner))
}

/// SongMessage.clearSongPreset — empty a Song row. Field **17**;
/// `ClearSongPreset{ songSlot=1, songPresetSlot=2 }`. Setter.
pub fn clear_song_preset(song_slot: u64, song_preset_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if song_preset_slot != 0 {
        field_varint(&mut inner, 2, song_preset_slot);
    }
    len_delimited(11, &len_delimited(17, &inner))
}

/// PresetMessage.loadScene{ sceneSlot } — activate a scene within the current preset
/// (enables per-scene re-amp capture). PresetMessage is TMS field **2**;
/// inner `loadScene` is field **101**; `LoadScene{ sceneSlot=1 }`. A setter → NO
/// batchStatus. The slot is ALWAYS emitted — even 0: canonical proto3 omits default
/// values, but the device IGNORES an empty `LoadScene{}` (HW-found: scene
/// slot 0 never loaded during leveling — the unit's screen never changed and every
/// measurement hit the prior state; an explicit 0 is non-canonical but parses fine).
pub fn load_scene(scene_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, scene_slot);
    len_delimited(2, &len_delimited(101, &inner))
}

/// PresetMessage.setNodeSceneEdit{ nodeId, groupId, sceneEditEnable } — make a block's
/// parameters scene-specific for the active scene (so a per-scene `changeParameter`
/// writes the scene overlay, not the base). PresetMessage is TMS field **2**; inner
/// `setNodeSceneEdit` is field **107**; inner fields `nodeId=1`, `groupId=2`,
/// `sceneEditEnable=3` (per `SetNodeSceneEdit.proto`). A setter → NO batchStatus;
/// proto3 omit-false on the bool.
pub fn set_node_scene_edit(group_id: &str, node_id: &str, enable: bool) -> Vec<u8> {
    let mut inner = Vec::new();
    field_bytes(&mut inner, 1, node_id.as_bytes());
    field_bytes(&mut inner, 2, group_id.as_bytes());
    if enable {
        field_varint(&mut inner, 3, 1);
    }
    len_delimited(2, &len_delimited(107, &inner))
}

/// PresetMessage.loadPreset addressed BY SONG — make a song the ACTIVE song.
/// `LoadPreset{ tabEnum=1, …, songSlot=4, songPresetSlot=5, presetSlot=6 }` with
/// `tabEnum=5` (the Songs context). RE'd from a Pro Control capture (#786:
/// `12 0a 52 08 0805 2002 2804 3001`) — setting a song's BPM requires the song to
/// be active, which PC achieves by loading one of the song's footswitch presets
/// with `tabEnum=5`. A setter → NO batchStatus.
pub fn load_song(song_slot: u64, song_preset_slot: u64, preset_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    field_varint(&mut inner, 1, 5); // tabEnum = 5 (Songs context)
    if song_slot != 0 {
        field_varint(&mut inner, 4, song_slot);
    }
    if song_preset_slot != 0 {
        field_varint(&mut inner, 5, song_preset_slot);
    }
    if preset_slot != 0 {
        field_varint(&mut inner, 6, preset_slot);
    }
    len_delimited(2, &len_delimited(10, &inner))
}

/// SettingsMessage.tapTempoBpm — set the global tap-tempo BPM, which the device
/// stores as the **active song's** BPM. This is the ONLY per-song BPM write path
/// (there is no `SetSongBpm` message); RE'd byte-exact from a Pro Control capture:
/// `1a09 6207 0d<f32le> 1001`. SettingsMessage is TMS field **3**; inner
/// `tapTempoBpm` is field **12**; `{ value(1)=float, originatorId(2) }`.
/// `originator_id`=1 is what Pro Control sends. A setter → NO batchStatus.
pub fn set_tap_tempo_bpm(bpm: f32, originator_id: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    field_f32(&mut inner, 1, bpm);
    if originator_id != 0 {
        field_varint(&mut inner, 2, originator_id);
    }
    len_delimited(3, &len_delimited(12, &inner))
}

// ─── Song CRUD setters (SongMessage = TMS field 11) ──────────────────────────
// All setters → NO batchStatus (only reads carry one).

/// SongMessage.addSong{ songName } (field 4) — create a song (name only; notes/BPM
/// set via separate messages).
pub fn add_song(name: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    if !name.is_empty() {
        field_bytes(&mut inner, 1, name.as_bytes());
    }
    len_delimited(11, &len_delimited(4, &inner))
}

/// SongMessage.renameSong{ songSlot, songName } (field 7).
pub fn rename_song(song_slot: u64, name: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if !name.is_empty() {
        field_bytes(&mut inner, 2, name.as_bytes());
    }
    len_delimited(11, &len_delimited(7, &inner))
}

/// SongMessage.removeSong{ songSlot } (field 6) — DELETE a song.
pub fn remove_song(song_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    len_delimited(11, &len_delimited(6, &inner))
}

/// SongMessage.songNotes{ songSlot, songNotes } (field 22) — set a song's notes.
pub fn set_song_notes(song_slot: u64, notes: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if !notes.is_empty() {
        field_bytes(&mut inner, 2, notes.as_bytes());
    }
    len_delimited(11, &len_delimited(22, &inner))
}

/// SongMessage.setSongBpmActive{ songSlot, active } (field 23) — toggle a song's
/// BPM display on/off.
pub fn set_song_bpm_active(song_slot: u64, active: bool) -> Vec<u8> {
    let mut inner = Vec::new();
    if song_slot != 0 {
        field_varint(&mut inner, 1, song_slot);
    }
    if active {
        field_varint(&mut inner, 2, 1);
    }
    len_delimited(11, &len_delimited(23, &inner))
}

// ─── Setlist CRUD setters (SetlistMessage = TMS field 12) ────────────────────

/// SetlistMessage.addSetlist{ setlistName } (field 4) — create a setlist.
pub fn add_setlist(name: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    if !name.is_empty() {
        field_bytes(&mut inner, 1, name.as_bytes());
    }
    len_delimited(12, &len_delimited(4, &inner))
}

/// SetlistMessage.renameSetlist{ setlistSlot, setlistName } (field 7).
pub fn rename_setlist(setlist_slot: u64, name: &str) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    if !name.is_empty() {
        field_bytes(&mut inner, 2, name.as_bytes());
    }
    len_delimited(12, &len_delimited(7, &inner))
}

/// SetlistMessage.removeSetlist{ setlistSlot } (field 6) — DELETE a setlist.
pub fn remove_setlist(setlist_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    len_delimited(12, &len_delimited(6, &inner))
}

/// SetlistMessage.addSetlistSong{ setlistSlot, songSlot } (field 14) — add a song
/// (by slot) to a setlist.
pub fn add_setlist_song(setlist_slot: u64, song_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    if song_slot != 0 {
        field_varint(&mut inner, 2, song_slot);
    }
    len_delimited(12, &len_delimited(14, &inner))
}

/// SetlistMessage.removeSetlistSong{ setlistSlot, setlistSongSlot } (field 16) —
/// remove a song from a setlist BY ITS POSITION within the setlist
/// (`setlistSongSlot` = the positional index in `setlistSongListResponse`, NOT the
/// global song slot — a different number space from [`add_setlist_song`]). A
/// setter → NO batchStatus. **HW-confirmed: `setlistSongSlot` is
/// 1-BASED** (removing position 2 of `[1,3,6]` yields `[1,6]`).
pub fn remove_setlist_song(setlist_slot: u64, setlist_song_slot: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    if setlist_song_slot != 0 {
        field_varint(&mut inner, 2, setlist_song_slot);
    }
    len_delimited(12, &len_delimited(16, &inner))
}

/// SetlistMessage.moveSetlistSong{ setlistSlot, oldSetlistSongSlot,
/// newSetlistSongSlot } (field 15) — reorder a song within a setlist BY POSITION
/// (both `*SongSlot`s are positional indices within the setlist, NOT global song
/// slots). A setter → NO batchStatus. **HW-confirmed: 1-BASED positions
/// with array-splice semantics** — `[1,6,11]` move 3→1 yields `[11,1,6]` (remove at
/// `old_pos`, insert at `new_pos` in the rendered order).
pub fn move_setlist_song(setlist_slot: u64, old_pos: u64, new_pos: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    if setlist_slot != 0 {
        field_varint(&mut inner, 1, setlist_slot);
    }
    if old_pos != 0 {
        field_varint(&mut inner, 2, old_pos);
    }
    if new_pos != 0 {
        field_varint(&mut inner, 3, new_pos);
    }
    len_delimited(12, &len_delimited(15, &inner))
}

/// Decode a `presetDataChanged` body (TMS[2] → presetDataChanged[9] →
/// presetJson[3]) to the raw `presetJson` bytes.
pub fn preset_data_changed_json(tms_body: &[u8]) -> Option<Vec<u8>> {
    let top = parse(tms_body);
    let pm = first_bytes(&top, 2)?;
    let pm_fields = parse(pm);
    let changed = first_bytes(&pm_fields, 9)?;
    let changed_fields = parse(changed);
    first_bytes(&changed_fields, 3).map(|b| b.to_vec())
}

/// Decode an `exportPresetResponse` body (the reassembled TMS stream) to the
/// raw `presetJson` bytes. Walks TMS[2] → exportPresetResponse[116] →
/// presetJson[1]. The caller decides whether those bytes are LZ4-block
/// compressed (like `currentPresetDataChanged`) or already plaintext JSON.
pub fn export_response_preset_json(tms_body: &[u8]) -> Option<Vec<u8>> {
    let top = parse(tms_body);
    let pm = first_bytes(&top, 2)?;
    let pm_fields = parse(pm);
    let resp = first_bytes(&pm_fields, 116)?;
    let resp_fields = parse(resp);
    first_bytes(&resp_fields, 1).map(|b| b.to_vec())
}

/// Decode a `nextEmptyPresetSlotResponse` body (TMS[2] → field 82 → presetSlot[1]
/// varint). Paired with [`request_next_empty_preset_slot`] — **dead on 1.7.75**,
/// kept as documented wire format.
#[allow(dead_code)]
pub fn next_empty_preset_slot_response(tms_body: &[u8]) -> Option<u64> {
    let top = parse(tms_body);
    let pm = first_bytes(&top, 2)?;
    let pm_fields = parse(pm);
    let resp = first_bytes(&pm_fields, 82)?;
    first_varint(&parse(resp), 1)
}

/// Decode a `songPresetListResponse` body to its raw record field-sets. Walks
/// TMS → songMessage[11] → songPresetListResponse[13] → repeated record[2], and
/// returns each record's parsed fields. The caller pulls `isEmpty[1]`,
/// `userPresetSlot[2]`, `presetSceneSlot[5]`, `presetSceneName[6]` per
/// `SongPresetListRecord.proto`.
pub fn song_preset_list_records(tms_body: &[u8]) -> Vec<Vec<(u32, Val)>> {
    let top = parse(tms_body);
    let Some(sm) = first_bytes(&top, 11) else {
        return Vec::new();
    };
    let sm_fields = parse(sm);
    let Some(resp) = first_bytes(&sm_fields, 13) else {
        return Vec::new();
    };
    let resp_fields = parse(resp);
    all_bytes(&resp_fields, 2).into_iter().map(parse).collect()
}

// The non-strict `song_list_records` / `setlist_list_records` /
// `setlist_song_list_records` decoders were removed — every list-read path uses the
// fail-closed `*_records_strict` variants (a tolerant decode could accept a tail-
// truncated record set, which these reads must never do). `song_preset_list_records`
// above stays: the song-preset read still uses the tolerant decode.

/// Decode an LZ4 *block* (not frame) payload — the on-wire `presetJson` format.
/// The caller already knows the exact compressed length (protobuf length-delimited
/// field), so no frame header / output-size is needed.
pub fn lz4_block_decompress(src: &[u8]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let (mut i, n) = (0usize, src.len());
    while i < n {
        let token = src[i];
        i += 1;
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            while i < n {
                let b = src[i];
                i += 1;
                lit_len += b as usize;
                if b != 0xFF {
                    break;
                }
            }
        }
        let end = (i + lit_len).min(n);
        out.extend_from_slice(&src[i..end]);
        i = end;
        if i >= n {
            break; // final sequence: literals only
        }
        if i + 1 >= n {
            break;
        }
        let offset = (src[i] as usize) | ((src[i + 1] as usize) << 8);
        i += 2;
        if offset == 0 {
            return Err("LZ4 offset==0 (invalid)".into());
        }
        let mut match_len = ((token & 0x0F) as usize) + 4;
        if (token & 0x0F) == 15 {
            while i < n {
                let b = src[i];
                i += 1;
                match_len += b as usize;
                if b != 0xFF {
                    break;
                }
            }
        }
        if offset > out.len() {
            return Err("LZ4 offset past output start".into());
        }
        let start = out.len() - offset;
        for k in 0..match_len {
            out.push(out[start + k]); // byte-by-byte: LZ4 overlap = RLE
        }
    }
    Ok(out)
}

// ─── decoding ────────────────────────────────────────────────────────────────

/// One decoded protobuf field value.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Varint(u64),
    Fixed32([u8; 4]),
    Fixed64([u8; 8]),
    Bytes(Vec<u8>),
}

impl Val {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Val::Varint(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Val::Fixed32(b) => Some(f32::from_le_bytes(*b)),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Val::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift = 0;
    while *pos < buf.len() {
        let b = buf[*pos];
        *pos += 1;
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

/// Decode a protobuf message into `(field_no, Val)` pairs. Tolerates truncation
/// (a length running past the buffer yields the remaining bytes) so a partially
/// reassembled stream still parses what it can — mirrors `parse_pb`.
pub fn parse(buf: &[u8]) -> Vec<(u32, Val)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(t) = read_varint(buf, &mut pos) else {
            break;
        };
        let field_no = (t >> 3) as u32;
        let wire = (t & 7) as u8;
        match wire {
            0 => {
                let Some(v) = read_varint(buf, &mut pos) else {
                    break;
                };
                out.push((field_no, Val::Varint(v)));
            }
            5 => {
                if pos + 4 > buf.len() {
                    break;
                }
                let mut b = [0u8; 4];
                b.copy_from_slice(&buf[pos..pos + 4]);
                pos += 4;
                out.push((field_no, Val::Fixed32(b)));
            }
            1 => {
                if pos + 8 > buf.len() {
                    break;
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&buf[pos..pos + 8]);
                pos += 8;
                out.push((field_no, Val::Fixed64(b)));
            }
            2 => {
                let Some(len) = read_varint(buf, &mut pos) else {
                    break;
                };
                let end = (pos + len as usize).min(buf.len());
                out.push((field_no, Val::Bytes(buf[pos..end].to_vec())));
                pos = end;
            }
            _ => break, // groups (3/4) unused by this protocol
        }
    }
    out
}

/// Find the first length-delimited field `field_no` and return its bytes.
pub fn first_bytes(fields: &[(u32, Val)], field_no: u32) -> Option<&[u8]> {
    fields
        .iter()
        .find(|(f, _)| *f == field_no)
        .and_then(|(_, v)| v.as_bytes())
}

/// Find the first varint field `field_no` and return its value.
pub fn first_varint(fields: &[(u32, Val)], field_no: u32) -> Option<u64> {
    fields
        .iter()
        .find(|(f, _)| *f == field_no)
        .and_then(|(_, v)| v.as_u64())
}

/// Collect every length-delimited field `field_no` (a repeated message field).
pub fn all_bytes(fields: &[(u32, Val)], field_no: u32) -> Vec<&[u8]> {
    fields
        .iter()
        .filter(|(f, _)| *f == field_no)
        .filter_map(|(_, v)| v.as_bytes())
        .collect()
}

/// Like [`parse`], but FAILS on ANY truncation — a length-delimited field declaring
/// more bytes than remain, a truncated varint/tag, field 0, or an unknown wire type.
/// The tolerant [`parse`] silently CLIPS a cut field into a plausible shorter value,
/// which made truncated multi-packet list reads look valid (a cut name / dropped
/// trailing records). `parse_strict` is used to validate that a reassembled response
/// is COMPLETE before accepting it (fail-closed). Per the reassembly review.
pub fn parse_strict(buf: &[u8]) -> Result<Vec<(u32, Val)>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let t = read_varint(buf, &mut pos).ok_or("truncated tag")?;
        let field_no = (t >> 3) as u32;
        if field_no == 0 {
            return Err("invalid field 0".into());
        }
        match (t & 7) as u8 {
            0 => {
                let v = read_varint(buf, &mut pos).ok_or("truncated varint")?;
                out.push((field_no, Val::Varint(v)));
            }
            5 => {
                if pos + 4 > buf.len() {
                    return Err("truncated fixed32".into());
                }
                let mut b = [0u8; 4];
                b.copy_from_slice(&buf[pos..pos + 4]);
                pos += 4;
                out.push((field_no, Val::Fixed32(b)));
            }
            1 => {
                if pos + 8 > buf.len() {
                    return Err("truncated fixed64".into());
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&buf[pos..pos + 8]);
                pos += 8;
                out.push((field_no, Val::Fixed64(b)));
            }
            2 => {
                let len = read_varint(buf, &mut pos).ok_or("truncated length")? as usize;
                let end = pos.checked_add(len).ok_or("length overflow")?;
                if end > buf.len() {
                    return Err("truncated length-delimited field".into());
                }
                out.push((field_no, Val::Bytes(buf[pos..end].to_vec())));
                pos = end;
            }
            _ => return Err("unsupported wire type".into()),
        }
    }
    Ok(out)
}

/// Strictly decode a COMPLETE list response into its repeated record field-sets, or
/// `None` if the reassembled body is truncated / not the expected response. Walks
/// TMS[`tms_field`] → response[`resp_field`] → repeated record[2], strict-parsing at
/// every level (so a tail-truncated multi-packet read is rejected, not clipped).
fn list_records_strict(
    tms_body: &[u8],
    tms_field: u32,
    resp_field: u32,
) -> Option<Vec<Vec<(u32, Val)>>> {
    let top = parse_strict(tms_body).ok()?;
    let sm = first_bytes(&top, tms_field)?.to_vec();
    let sm_fields = parse_strict(&sm).ok()?;
    let resp = first_bytes(&sm_fields, resp_field)?.to_vec();
    let resp_fields = parse_strict(&resp).ok()?;
    let mut recs = Vec::new();
    for r in all_bytes(&resp_fields, 2) {
        recs.push(parse_strict(r).ok()?);
    }
    Some(recs)
}

/// Strict, completeness-validated `presetListResponse` decode (TMS[2]→resp[5]→rec[2]).
pub fn preset_list_records_strict(tms_body: &[u8]) -> Option<Vec<Vec<(u32, Val)>>> {
    list_records_strict(tms_body, 2, 5)
}
/// Strict, completeness-validated `songListResponse` decode (TMS[11]→resp[3]→rec[2]).
pub fn song_list_records_strict(tms_body: &[u8]) -> Option<Vec<Vec<(u32, Val)>>> {
    list_records_strict(tms_body, 11, 3)
}
/// Strict, completeness-validated `setlistListResponse` decode (TMS[12]→resp[3]→rec[2]).
pub fn setlist_list_records_strict(tms_body: &[u8]) -> Option<Vec<Vec<(u32, Val)>>> {
    list_records_strict(tms_body, 12, 3)
}
/// Strict, completeness-validated `setlistSongListResponse` decode (TMS[12]→resp[13]→rec[2]).
pub fn setlist_song_list_records_strict(tms_body: &[u8]) -> Option<Vec<Vec<(u32, Val)>>> {
    list_records_strict(tms_body, 12, 13)
}

// ─── device → host stream reassembly ─────────────────────────────────────────

/// A reassembled device message body (one or more 60-byte chunks concatenated).
#[derive(Debug, Clone)]
pub struct Stream {
    pub body: Vec<u8>,
}

/// Group raw 64-byte input reports into message bodies. macOS prepends a
/// report-id `0x00`; byte 1 is the magic: `0x35` single-frame, `0x33` stream
/// start, `0x34` continuation. Port of `reassemble_streams`.
pub fn reassemble_streams(reports: &[Vec<u8>]) -> Vec<Stream> {
    let mut streams = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for data in reports {
        if data.len() < 4 || data[0] != 0x00 {
            continue;
        }
        let magic = data[1];
        let body_len = data[3] as usize;
        let end = (4 + body_len).min(data.len());
        let body = &data[4..end];
        match magic {
            MAGIC_OUT => {
                // A single-frame (0x35) response interleaved mid-stream — e.g. a
                // heartbeat reply or small event arriving during a large 0x33/0x34
                // stream — must NOT close the open stream. Push it as its own
                // stream but keep `current` open so the continuation 0x34 frames
                // still append (interleaved single-frames were truncating large
                // flow-controlled streams like the preset-data JSON).
                streams.push(Stream {
                    body: body.to_vec(),
                });
            }
            MAGIC_IN_START => {
                if let Some(c) = current.take() {
                    streams.push(Stream { body: c });
                }
                current = Some(body.to_vec());
            }
            MAGIC_IN_FRAME => match current {
                Some(ref mut c) => c.extend_from_slice(body),
                None => current = Some(body.to_vec()),
            },
            _ => {}
        }
    }
    if let Some(c) = current {
        streams.push(Stream { body: c });
    }
    streams
}

/// Like [`reassemble_streams`], but treats an inbound `0x35` as the **final frame**
/// of the open multi-packet stream (append + close), not a standalone single-frame.
/// HW-verified (raw frame dump): device list responses are `0x33 start · 0x34 cont* ·
/// 0x35 final` — a `0x34` never follows a `0x35` within a stream, so the trailing
/// `0x35` carries the tail (last record). The global [`reassemble_streams`] keeps the
/// `0x35` separate (load-bearing for the partial preset-JSON path, where an interleaved
/// `0x35` heartbeat must NOT close the stream), which DROPS the final frame of complete
/// list responses → systematic tail truncation. The strict list harvests use this
/// variant; a standalone `0x35` with no open stream is still kept as its own message.
pub fn reassemble_streams_final(reports: &[Vec<u8>]) -> Vec<Stream> {
    let mut streams = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for data in reports {
        if data.len() < 4 || data[0] != 0x00 {
            continue;
        }
        let magic = data[1];
        let body_len = data[3] as usize;
        let end = (4 + body_len).min(data.len());
        let body = &data[4..end];
        match magic {
            MAGIC_IN_START => {
                if let Some(c) = current.take() {
                    streams.push(Stream { body: c });
                }
                current = Some(body.to_vec());
            }
            MAGIC_IN_FRAME => match current {
                Some(ref mut c) => c.extend_from_slice(body),
                None => current = Some(body.to_vec()),
            },
            MAGIC_OUT => match current.take() {
                Some(mut c) => {
                    c.extend_from_slice(body);
                    streams.push(Stream { body: c });
                }
                None => streams.push(Stream {
                    body: body.to_vec(),
                }),
            },
            _ => {}
        }
    }
    if let Some(c) = current {
        streams.push(Stream { body: c });
    }
    streams
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    // Golden vectors captured from a known-good live exchange.
    #[test]
    fn connection_request_matches_golden() {
        assert_eq!(hex(&connection_request()), "22040a020801");
    }

    #[test]
    fn heartbeat_matches_golden_and_has_no_batch() {
        assert_eq!(hex(&heartbeat()), "220422020801");
    }

    #[test]
    fn preset_list_request_matches_golden() {
        assert_eq!(hex(&preset_list_request(1, 1)), "1204220208015001");
    }

    #[test]
    fn current_preset_info_request_matches_golden() {
        assert_eq!(hex(&current_preset_info_request(2)), "12040a0208015002");
    }

    #[test]
    fn current_preset_data_json_request_matches_golden() {
        assert_eq!(
            hex(&current_preset_data_json_request(4)),
            "1205f2040208015004"
        );
    }

    #[test]
    fn scene_list_request_matches_golden_and_has_no_batch() {
        // PresetMessage[2]{ sceneListRequest[126]{ dummy[1]=1 } }, NO batchStatus.
        //   inner = 08 01 · [126] tag = (126<<3)|2 = 1010 = f2 07 · TMS[2] = 12 05 <..>
        assert_eq!(hex(&scene_list_request()), "1205f207020801");
        // Setter/request-on-current-preset → no batchStatus (field 10) at the top level.
        assert!(parse(&scene_list_request()).iter().all(|(n, _)| *n != 10));
    }

    #[test]
    fn load_preset_no_batch() {
        // slot=7, tab=1, no batchStatus → 12 06 52 04 08 01 30 07
        assert_eq!(hex(&load_preset(7, 1)), "1206520408013007");
    }

    // The structural-edit node builders (no golden capture handy) — assert the wire
    // framing: TMS[2] → the right inner field, the string args round-trip, and no
    // stray batchStatus (field 10) where Pro Control sends none. `edit_inner` digs
    // TMS[2] → the inner edit message at `field`.
    fn edit_inner(body: &[u8], field: u32) -> Vec<(u32, Val)> {
        let top = parse(body);
        let pm = first_bytes(&top, 2).expect("TMS field 2");
        let pm_fields = parse(pm);
        parse(first_bytes(&pm_fields, field).unwrap_or_else(|| panic!("inner field {field}")))
    }

    #[test]
    fn node_json_request_frames_field_119() {
        let inner = edit_inner(&node_json_request("guitarNodes", "ACD_Comp"), 119);
        assert_eq!(first_bytes(&inner, 1), Some(&b"guitarNodes"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_Comp"[..]));
    }

    #[test]
    fn replace_node_frames_field_39_no_batch() {
        let body = replace_node("guitarNodes", "ACD_Comp", "ACD_KlonCentaur", None);
        let inner = edit_inner(&body, 39);
        assert_eq!(first_bytes(&inner, 1), Some(&b"guitarNodes"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_Comp"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"ACD_KlonCentaur"[..]));
        assert!(parse(&body).iter().all(|(n, _)| *n != 10));
    }

    #[test]
    fn replace_node_with_block_frames_field_100_with_index() {
        let inner = edit_inner(
            &replace_node_with_block("guitarNodes", "ACD_Comp", "ACD_CabSimTMS", 4, None),
            100,
        );
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_Comp"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"ACD_CabSimTMS"[..]));
        assert_eq!(
            inner
                .iter()
                .find(|(n, _)| *n == 4)
                .and_then(|(_, v)| v.as_u64()),
            Some(4)
        );
    }

    #[test]
    fn remove_node_frames_field_35_no_batch() {
        let body = remove_node("guitarNodes", "ACD_Comp", None);
        let inner = edit_inner(&body, 35);
        assert_eq!(first_bytes(&inner, 1), Some(&b"guitarNodes"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_Comp"[..]));
        // request-on-current-preset edit → NO batchStatus (field 10) at the top level.
        assert!(parse(&body).iter().all(|(n, _)| *n != 10));
    }

    #[test]
    fn insert_node_matches_pc_capture_append() {
        // Observed add-block exchange:
        // insertNode{ groupId="G1", fenderId="ACD_TM59BassmanCabIR" } — field 2 OMITTED
        // (APPEND), no nodeJsonRequest preamble, no batchStatus. Byte-exact incl. the
        // 0x35-envelope + zero pad (compare via make_envelope, the device's wire form).
        let pkt = make_envelope(&insert_node("G1", None, "ACD_TM59BassmanCabIR", None));
        assert_eq!(
            hex(&pkt[..]),
            "35001f121d92021a0a0247311a144143445f544d3539426173736d616e43616249520000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn insert_node_matches_pc_capture_insert_before() {
        // Pro Control capture #324: insertNode{ groupId="G1",
        // nodeIdInsertLocation="ACD_TM59BassmanCabIR", fenderId="ACD_TCIntegratedPre" }.
        // field-2 = the FenderId to insert BEFORE (HW-verified fw 1.8.45) — the bytes are
        // the capture's verbatim; only the placement interpretation was corrected.
        let pkt = make_envelope(&insert_node(
            "G1",
            Some("ACD_TM59BassmanCabIR"),
            "ACD_TCIntegratedPre",
            None,
        ));
        assert_eq!(
            hex(&pkt[..]),
            "350034123292022f0a02473112144143445f544d3539426173736d616e43616249521a134143445f5443496e74656772617465645072650000000000000000"
        );
        // structural: TMS[2] → insertNode[34] carries the anchor (2) + new id (3).
        let inner = edit_inner(&insert_node("G1", Some("ACD_X"), "ACD_Y", None), 34);
        assert_eq!(first_bytes(&inner, 1), Some(&b"G1"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_X"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"ACD_Y"[..]));
        // append → field 2 absent; never a top-level batchStatus (field 10).
        let append = insert_node("G1", None, "ACD_Y", None);
        assert_eq!(first_bytes(&edit_inner(&append, 34), 2), None);
        assert!(parse(&append).iter().all(|(n, _)| *n != 10));
    }

    #[test]
    fn insert_node_at_block_index_structure() {
        // No Pro Control capture (PC uses the bare field-34 after-anchor) — assert the
        // structural encoding: TMS[2] → insertNodeAtBlockIndex[99]{ groupId=1, fenderId=3,
        // index=4 }, no top-level batchStatus.
        let inner = edit_inner(&insert_node_at_block_index("G2", 0, "ACD_Klon", None), 99);
        assert_eq!(first_bytes(&inner, 1), Some(&b"G2"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"ACD_Klon"[..]));
        assert_eq!(first_varint(&inner, 4), Some(0));
        let bare = insert_node_at_block_index("G2", 0, "ACD_Klon", None);
        assert!(parse(&bare).iter().all(|(n, _)| *n != 10));
    }

    #[test]
    fn change_parameter_str_uses_stringval_field_6() {
        // Set a user-IR node's `file` param (a STRING value → ChangeParameter.stringVal,
        // field 6) — used after replaceNode→ACD_UserIRTMS to point at the chosen IR.
        let body = change_parameter_str("guitarNodes", "ACD_UserIRTMS", "file", "Oversize.wav");
        let inner = edit_inner(&body, 12); // PresetMessage.changeParameter
        assert_eq!(first_bytes(&inner, 1), Some(&b"guitarNodes"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_UserIRTMS"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"file"[..]));
        assert_eq!(first_bytes(&inner, 6), Some(&b"Oversize.wav"[..])); // stringVal
                                                                        // the float slot (field 5) must NOT be present for a string set
        assert!(inner.iter().all(|(n, _)| *n != 5));
    }

    #[test]
    fn change_parameter_bool_uses_boolval_field_7() {
        // Set a block's `bypass` (a BOOL value → ChangeParameter.boolVal, field 7) — used to
        // force a block active/bypassed during footswitch-bake measurement. The oneof field
        // is ALWAYS emitted, even for `false`.
        let on = change_parameter_bool("G1", "ACD_BluesDriver", "bypass", false);
        let inner = edit_inner(&on, 12); // PresetMessage.changeParameter
        assert_eq!(first_bytes(&inner, 1), Some(&b"G1"[..]));
        assert_eq!(first_bytes(&inner, 2), Some(&b"ACD_BluesDriver"[..]));
        assert_eq!(first_bytes(&inner, 3), Some(&b"bypass"[..]));
        assert_eq!(first_varint(&inner, 7), Some(0)); // boolVal=false STILL present
        assert!(inner.iter().all(|(n, _)| *n != 5 && *n != 6)); // not float/string
                                                                // true → boolVal=1
        let off = change_parameter_bool("G1", "ACD_BluesDriver", "bypass", true);
        assert_eq!(first_varint(&edit_inner(&off, 12), 7), Some(1));
    }

    #[test]
    fn set_preset_level_matches_golden() {
        // 0.5f, no batchStatus: 12 08 e2 04 05 0d 00 00 00 3f
        assert_eq!(hex(&set_preset_level(0.5)), "1208e204050d0000003f");
    }

    #[test]
    fn set_preset_level_clamps() {
        // 2.0 clamps to 1.0 (0x3f800000), -1.0 clamps to 0.0 (0x00000000).
        assert_eq!(hex(&set_preset_level(2.0)), "1208e204050d0000803f");
        assert_eq!(hex(&set_preset_level(-1.0)), "1208e204050d00000000");
    }

    #[test]
    fn set_reamp_mode_matches_pro_control_capture() {
        // Byte-exact against Pro Control's live HID frames (#218 ON / #242 OFF):
        //   ON  = 1a 05 f2 01 02 08 01   OFF = 1a 03 f2 01 00   (no batchStatus)
        assert_eq!(hex(&set_reamp_mode(true)), "1a05f201020801");
        assert_eq!(hex(&set_reamp_mode(false)), "1a03f20100");
    }

    #[test]
    fn save_current_preset_structure() {
        // TMS[2]{ [14]{ [1]=7 } } no batch → 12 04 72 02 08 07
        assert_eq!(hex(&save_current_preset(7)), "120472020807");
    }

    #[test]
    fn envelope_frames_and_pads() {
        let env = make_envelope(&connection_request());
        assert_eq!(env[0], 0x35);
        assert_eq!(env[1], 0x00);
        assert_eq!(env[2], 6); // body len
        assert_eq!(&env[3..9], &connection_request()[..]);
        assert!(env[9..].iter().all(|&b| b == 0));
    }

    #[test]
    fn lz4_literal_only() {
        // token hi-nibble 4 = 4 literals, no match follows (source exhausted).
        assert_eq!(
            lz4_block_decompress(&[0x40, b't', b'e', b's', b't']).unwrap(),
            b"test"
        );
    }

    #[test]
    fn lz4_rle_overlap() {
        // 1 literal 'a' then match offset=1 len=4 (overlap RLE) → "aaaaa".
        assert_eq!(
            lz4_block_decompress(&[0x10, b'a', 0x01, 0x00]).unwrap(),
            b"aaaaa"
        );
    }

    #[test]
    fn parse_roundtrips_preset_level_changed() {
        // Build a PresetLevelChanged{ presetLevel=0.5, originatorId=42 } body and
        // confirm we decode the float + varint back out.
        let mut inner = Vec::new();
        field_f32(&mut inner, 1, 0.5);
        field_varint(&mut inner, 2, 42);
        let fields = parse(&inner);
        assert_eq!(fields[0].1.as_f32(), Some(0.5));
        assert_eq!(fields[1].1.as_u64(), Some(42));
    }

    #[test]
    fn parse_tolerates_truncation() {
        // length-delimited field claiming 10 bytes but only 3 present.
        let buf = [0x0a, 0x0a, 0x41, 0x42, 0x43];
        let fields = parse(&buf);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].1.as_bytes(), Some(&b"ABC"[..]));
    }

    #[test]
    fn change_parameter_structure_roundtrips() {
        // TMS(2) → changeParameter(12) → {1="G1", 2="amp", 3="outputLevel", 5=f32}.
        let body = change_parameter("G1", "amp", "outputLevel", 0.5);
        let top = parse(&body);
        let pm = first_bytes(&top, 2).expect("presetMessage");
        let pm_fields = parse(pm);
        let cp = first_bytes(&pm_fields, 12).expect("changeParameter @ field 12");
        let f = parse(cp);
        assert_eq!(first_bytes(&f, 1), Some(&b"G1"[..]));
        assert_eq!(first_bytes(&f, 2), Some(&b"amp"[..]));
        assert_eq!(first_bytes(&f, 3), Some(&b"outputLevel"[..]));
        assert_eq!(
            f.iter()
                .find(|(n, _)| *n == 5)
                .and_then(|(_, v)| v.as_f32()),
            Some(0.5)
        );
        // No batchStatus (field 10) at the top level.
        assert!(top.iter().all(|(n, _)| *n != 10), "must omit batchStatus");
    }

    #[test]
    fn rename_current_preset_matches_golden() {
        // TMS[2]{ renameCurrentPreset[13]{ displayName[1]="Lead" } }, no batch.
        //   [1] = 0a 04 4c 65 61 64 · [13] = 6a 06 <..> · TMS[2] = 12 08 <..>
        assert_eq!(hex(&rename_current_preset("Lead")), "12086a060a044c656164");
        // No batchStatus at top level.
        assert!(parse(&rename_current_preset("Lead"))
            .iter()
            .all(|(n, _)| *n != 10));
    }

    #[test]
    fn move_user_preset_matches_golden() {
        // TMS[2]{ moveUserPreset[16]{ oldSlot[1]=5, newSlot[2]=7 } }, no batch.
        //   inner = 08 05 10 07 · [16] tag = 82 01 (field 16 wire2) · TMS[2] = 12 07 <..>
        assert_eq!(hex(&move_user_preset(5, 7)), "120782010408051007");
    }

    #[test]
    fn clear_user_preset_matches_golden() {
        // TMS[2]{ clearUserPreset[15]{ userSlot[1]=7 } }, no batch.
        //   inner = 08 07 · [15] = 7a 02 <..> · TMS[2] = 12 04 <..>
        assert_eq!(hex(&clear_user_preset(7)), "12047a020807");
    }

    #[test]
    fn export_preset_request_matches_golden() {
        // Spec-derived golden (PresetMessage.proto: exportPresetRequest = field
        // 115; ExportPresetRequest{listEnum=1, presetSlot=2}). Hand-computed:
        //   inner            = 08 01 10 0b           (listEnum=1, presetSlot=11)
        //   [115] len-delim  = 9a 07 04 <inner>      (tag 922 = 9a 07, len 4)
        //   TMS[2] len-delim = 12 07 <[115]>
        //   batchStatus[10]  = 50 02                 (batch=2)
        assert_eq!(
            hex(&export_preset_request(1, 11, Some(2))),
            "12079a07040801100b5002"
        );
        // No-batch variant (the setter-style form AC1 will also try live).
        assert_eq!(
            hex(&export_preset_request(1, 11, None)),
            "12079a07040801100b"
        );
    }

    #[test]
    fn export_preset_request_omits_proto3_zero_listenum() {
        // listEnum=0 dropped (proto3 default); presetSlot=11 kept.
        //   inner = 10 0b · [115] = 9a 07 02 10 0b · TMS[2] = 12 05 <[115]>
        assert_eq!(hex(&export_preset_request(0, 11, None)), "12059a0702100b");
    }

    #[test]
    fn export_response_decodes_preset_json() {
        // Build TMS[2]{ exportPresetResponse[116]{ presetJson[1]=b"{json}",
        // listEnum[2]=1, presetSlot[3]=11 } } and recover the presetJson bytes.
        let mut resp = Vec::new();
        field_bytes(&mut resp, 1, b"{\"json\":1}");
        field_varint(&mut resp, 2, 1);
        field_varint(&mut resp, 3, 11);
        let body = len_delimited(2, &len_delimited(116, &resp));
        assert_eq!(
            export_response_preset_json(&body).as_deref(),
            Some(&b"{\"json\":1}"[..])
        );
    }

    #[test]
    fn import_preset_request_roundtrips() {
        // TMS[2] → importPresetRequest[117] → presetJson[1] = bytes (structural).
        let body = import_preset_request(b"{\"uuid\":\"x\"}");
        let top = parse(&body);
        let pm = parse(first_bytes(&top, 2).expect("presetMessage"));
        let req = parse(first_bytes(&pm, 117).expect("importPresetRequest @ 117"));
        assert_eq!(first_bytes(&req, 1), Some(&b"{\"uuid\":\"x\"}"[..]));
    }

    /// Reassemble chunked output frames the way the device does: strip each
    /// frame's 3-byte `MAGIC 0x00 LEN` header and concatenate the payloads.
    fn reassemble_out_frames(frames: &[[u8; 63]]) -> Vec<u8> {
        let mut out = Vec::new();
        for f in frames {
            let len = f[2] as usize;
            out.extend_from_slice(&f[3..3 + len]);
        }
        out
    }

    #[test]
    fn chunked_single_frame_equals_make_envelope() {
        let body = b"hello world";
        let frames = make_chunked_envelopes(body);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0][0], MAGIC_OUT); // 0x35 — sole/final
        assert_eq!(frames[0], make_envelope(body));
    }

    #[test]
    fn chunked_multi_frame_framing_and_reassembly() {
        let body: Vec<u8> = (0..250u32).map(|i| (i % 256) as u8).collect(); // 5 chunks (60·4 + 10)
        let frames = make_chunked_envelopes(&body);
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0][0], MAGIC_IN_START); // 0x33 first
        assert!(frames[1..4].iter().all(|f| f[0] == MAGIC_IN_FRAME)); // 0x34 middle
        assert_eq!(frames[4][0], MAGIC_OUT); // 0x35 final
        assert!(frames[..4].iter().all(|f| f[2] == MAX_BODY as u8)); // full payloads
        assert_eq!(frames[4][2], 10); // remainder
        assert_eq!(reassemble_out_frames(&frames), body);
    }

    #[test]
    fn lz4_stored_roundtrips_through_decompress() {
        for n in [0usize, 1, 14, 15, 16, 60, 270, 5000] {
            let data: Vec<u8> = (0..n).map(|i| (i * 7 % 256) as u8).collect();
            let comp = lz4_block_compress_stored(&data);
            assert_eq!(
                lz4_block_decompress(&comp).unwrap(),
                data,
                "roundtrip n={n}"
            );
        }
    }

    #[test]
    fn import_chunks_reassemble_to_lz4_presetjson() {
        // End-to-end AC3 encode path: .preset bytes → LZ4 → field 117 → chunks.
        // Reassembling + LZ4-decompressing must recover the original bytes.
        let preset: Vec<u8> = b"{\"info\":{\"displayName\":\"T\"}}".repeat(40); // > MAX_BODY, multi-chunk
        let body = import_preset_request(&lz4_block_compress_stored(&preset));
        let reassembled = reassemble_out_frames(&make_chunked_envelopes(&body));
        let pm = parse(first_bytes(&parse(&reassembled), 2).unwrap());
        let req = parse(first_bytes(&pm, 117).unwrap());
        let pj = first_bytes(&req, 1).unwrap();
        assert_eq!(lz4_block_decompress(pj).unwrap(), preset);
    }

    #[test]
    fn preset_data_request_matches_golden() {
        //   inner = 08 01 10 0b · [8] = 42 04 <inner> (tag (8<<3)|2 = 0x42)
        //   TMS[2] = 12 06 42 04 08 01 10 0b · batch[10]=2 → 50 02
        assert_eq!(
            hex(&preset_data_request(1, 11, Some(2))),
            "120642040801100b5002"
        );
        assert_eq!(hex(&preset_data_request(1, 11, None)), "120642040801100b");
    }

    #[test]
    fn preset_data_changed_decodes_json() {
        // TMS[2]{ presetDataChanged[9]{ listEnum[1]=1, presetSlot[2]=11,
        // presetJson[3]="{json}" } }.
        let mut changed = Vec::new();
        field_varint(&mut changed, 1, 1);
        field_varint(&mut changed, 2, 11);
        field_bytes(&mut changed, 3, b"{\"json\":1}");
        let body = len_delimited(2, &len_delimited(9, &changed));
        assert_eq!(
            preset_data_changed_json(&body).as_deref(),
            Some(&b"{\"json\":1}"[..])
        );
    }

    #[test]
    fn export_response_missing_field_is_none() {
        // A TMS body with no presetMessage yields None rather than panicking.
        assert_eq!(export_response_preset_json(&connection_request()), None);
    }

    #[test]
    fn request_next_empty_preset_slot_matches_golden() {
        // TMS[2]{ requestNextEmptyPresetSlot[81]{ afterSlot[1]=5 } }, no batch.
        //   inner = 08 05 · [81] tag = 8a 05 · TMS[2] = 12 05 <..>
        assert_eq!(
            hex(&request_next_empty_preset_slot(5, None)),
            "12058a05020805"
        );
        // afterSlot=0 dropped (proto3 default).  inner empty · [81] = 8a 05 00
        assert_eq!(hex(&request_next_empty_preset_slot(0, None)), "12038a0500");
        // with batchStatus (field 10) for an in-burst send.
        assert_eq!(
            hex(&request_next_empty_preset_slot(5, Some(2))),
            "12058a050208055002"
        );
    }

    #[test]
    fn next_empty_preset_slot_response_decodes() {
        // TMS[2]{ nextEmptyPresetSlotResponse[82]{ presetSlot[1]=42 } }.
        let mut resp = Vec::new();
        field_varint(&mut resp, 1, 42);
        let body = len_delimited(2, &len_delimited(82, &resp));
        assert_eq!(next_empty_preset_slot_response(&body), Some(42));
        // No presetMessage → None rather than panic.
        assert_eq!(next_empty_preset_slot_response(&connection_request()), None);
    }

    #[test]
    fn song_preset_list_request_matches_golden() {
        // SongMessage[11]{ songPresetListRequest[12]{ songSlot[1]=1 } }, no batch.
        //   inner = 08 01 · [12] = 62 02 <..> · songMessage[11] = 5a 04 <..>
        assert_eq!(hex(&song_preset_list_request(1, None)), "5a0462020801");
        // songSlot=0 dropped (proto3 default).
        assert_eq!(hex(&song_preset_list_request(0, None)), "5a026200");
        // with batchStatus for an in-burst send.
        assert_eq!(
            hex(&song_preset_list_request(1, Some(2))),
            "5a04620208015002"
        );
    }

    #[test]
    fn song_list_request_matches_golden() {
        // SongMessage[11]{ songListRequest[2]{ dummy[1]=1 } }, no batch.
        //   inner = 08 01 · [2] = 12 02 <..> · songMessage[11] = 5a 04 <..>
        assert_eq!(hex(&song_list_request(None)), "5a0412020801");
        // with batchStatus for an in-burst send.
        assert_eq!(hex(&song_list_request(Some(2))), "5a04120208015002");
    }

    #[test]
    fn setlist_list_request_matches_golden() {
        // SetlistMessage[12]{ setlistListRequest[2]{ dummy[1]=1 } }, no batch.
        //   inner = 08 01 · [2] = 12 02 <..> · setlistMessage[12] = 62 04 <..>
        //   (only the top-level tag differs from songListRequest's 5a.)
        assert_eq!(hex(&setlist_list_request(None)), "620412020801");
        // with batchStatus for an in-burst send.
        assert_eq!(hex(&setlist_list_request(Some(2))), "6204120208015002");
    }

    #[test]
    fn setlist_song_list_request_matches_golden() {
        // SetlistMessage[12]{ setlistSongListRequest[12]{ setlistSlot[1]=1 } }, no batch.
        //   inner = 08 01 · inner[12] = 62 02 <..> · setlistMessage[12] = 62 04 <..>
        assert_eq!(hex(&setlist_song_list_request(1, None)), "620462020801");
        // setlistSlot=0 dropped (proto3 default).
        assert_eq!(hex(&setlist_song_list_request(0, None)), "62026200");
        // with batchStatus for an in-burst send.
        assert_eq!(
            hex(&setlist_song_list_request(1, Some(2))),
            "6204620208015002"
        );
    }

    #[test]
    fn song_setlist_write_setters_match_golden() {
        // tapTempoBpm — BYTE-EXACT vs the live Pro Control capture body (#815,
        // BPM 137.0): settingsMessage[3]{ tapTempoBpm[12]{ value[1]=137.0f, originatorId[2]=1 } }.
        assert_eq!(hex(&set_tap_tempo_bpm(137.0, 1)), "1a0962070d000009431001");
        // load_song — BYTE-EXACT vs the capture body (#786):
        // presetMessage[2]{ loadPreset[10]{ tabEnum=5, songSlot=2, songPresetSlot=4, presetSlot=1 } }.
        assert_eq!(hex(&load_song(2, 4, 1)), "120a52080805200228043001");
        // addSong{ songName="Toto" }: songMessage[11]{ addSong[4]{ songName[1]="Toto" } }.
        assert_eq!(hex(&add_song("Toto")), "5a0822060a04546f746f");
        // addSetlist{ setlistName="Tutu" }: setlistMessage[12]{ addSetlist[4]{ setlistName[1]="Tutu" } }.
        assert_eq!(hex(&add_setlist("Tutu")), "620822060a0454757475");
        // removeSong{ songSlot=23 }: songMessage[11]{ removeSong[6]{ songSlot[1]=23 } }.
        assert_eq!(hex(&remove_song(23)), "5a0432020817");
        // removeSetlist{ setlistSlot=4 }: setlistMessage[12]{ removeSetlist[6]{ setlistSlot[1]=4 } }.
        assert_eq!(hex(&remove_setlist(4)), "620432020804");
        // addSetlistSong{ setlistSlot=4, songSlot=23 }: setlistMessage[12]{ addSetlistSong[14]{ … } }.
        assert_eq!(hex(&add_setlist_song(4, 23)), "6206720408041017");
        // setSongBpmActive{ songSlot=23, active=true }.
        assert_eq!(hex(&set_song_bpm_active(23, true)), "5a07ba010408171001");
    }

    #[test]
    fn setlist_membership_setters_match_golden() {
        // removeSetlistSong{ setlistSlot=4, setlistSongSlot=2 }:
        //   setlistMessage[12]{ removeSetlistSong[16]{ setlistSlot[1]=4, setlistSongSlot[2]=2 } }
        //   inner = 08 04 10 02 · [16] tag = (16<<3)|2 = 130 = 82 01 · TMS[12] = 62 07 <..>
        assert_eq!(hex(&remove_setlist_song(4, 2)), "620782010408041002");
        // setlistSongSlot=0 dropped (proto3 default).
        assert_eq!(hex(&remove_setlist_song(4, 0)), "62058201020804");
        // moveSetlistSong{ setlistSlot=4, old=2, new=5 }:
        //   setlistMessage[12]{ moveSetlistSong[15]{ [1]=4, [2]=2, [3]=5 } }
        //   inner = 08 04 10 02 18 05 · [15] tag = (15<<3)|2 = 122 = 7a · TMS[12] = 62 08 <..>
        assert_eq!(hex(&move_setlist_song(4, 2, 5)), "62087a06080410021805");
        // newSetlistSongSlot=0 dropped (proto3 default).
        assert_eq!(hex(&move_setlist_song(4, 1, 0)), "62067a0408041001");
        // Both are setters → NO top-level batchStatus (field 10).
        assert!(parse(&remove_setlist_song(4, 2))
            .iter()
            .all(|(n, _)| *n != 10));
        assert!(parse(&move_setlist_song(4, 2, 5))
            .iter()
            .all(|(n, _)| *n != 10));
    }

    #[test]
    fn parse_strict_rejects_truncation() {
        // Complete len-delimited field 1 = "hi".
        assert!(parse_strict(&[0x0a, 0x02, 0x68, 0x69]).is_ok());
        // Declares len 2 but only 1 byte present → reject.
        assert!(parse_strict(&[0x0a, 0x02, 0x68]).is_err());
        // Truncated varint (continuation bit, no next byte) → reject.
        assert!(parse_strict(&[0x08, 0x80]).is_err());
        // A COMPLETE songListResponse with one record {songName="Hi"} decodes strictly:
        //   TMS[11]{ songListResponse[3]{ record[2]{ songName[1]="Hi" } } }
        let complete = [0x5a, 0x08, 0x1a, 0x06, 0x12, 0x04, 0x0a, 0x02, 0x48, 0x69];
        let recs = song_list_records_strict(&complete).expect("complete decodes");
        assert_eq!(recs.len(), 1);
        assert_eq!(first_bytes(&recs[0], 1), Some(&b"Hi"[..]));
        // Drop the trailing byte → the wrapper's declared length exceeds the buffer → None.
        assert!(song_list_records_strict(&complete[..complete.len() - 1]).is_none());
    }

    #[test]
    fn preset_list_records_strict_rejects_truncation() {
        // Synthetic presetListResponse:
        //   TMS[2]{ presetListResponse[5]{ record[2]{ displayName[1]=<name> } ×3 } }
        let rec = |name: &str| len_delimited(2, &len_delimited(1, name.as_bytes()));
        let resp: Vec<u8> = [rec("Guitar"), rec("Plexi"), rec("--")].concat();
        let body = len_delimited(2, &len_delimited(5, &resp));

        // Complete body decodes with all 3 records, names intact.
        let recs = preset_list_records_strict(&body).expect("complete decodes");
        assert_eq!(recs.len(), 3);
        assert_eq!(first_bytes(&recs[0], 1), Some(&b"Guitar"[..]));
        assert_eq!(first_bytes(&recs[2], 1), Some(&b"--"[..]));

        // Mid-record cut (trailing byte dropped) → declared length exceeds the
        // buffer at some level → None, never a clipped list.
        assert!(preset_list_records_strict(&body[..body.len() - 1]).is_none());

        // Boundary-clean-but-SHORT body (trailing frames dropped exactly between
        // records): structurally valid, so strict ACCEPTS it with fewer records —
        // documenting that the re-arm RETRY, not strict parsing, recovers the
        // dropped-trailing-frames truncation mode (the observed 371-of-504 case).
        let short: Vec<u8> = [rec("Guitar"), rec("Plexi")].concat();
        let short_body = len_delimited(2, &len_delimited(5, &short));
        assert_eq!(
            preset_list_records_strict(&short_body).map(|r| r.len()),
            Some(2)
        );
    }

    #[test]
    fn preset_list_strict_rejects_a_dropped_final_frame() {
        // The harvest contract behind `session::harvest_preset_list_strict`:
        // streams_final + strict decode. A multi-packet list whose terminal 0x35
        // frame is lost reassembles into a mid-record-truncated body (the open
        // stream is flushed at the tail) — strict must reject it, never clip.
        let rec = |name: &str| len_delimited(2, &len_delimited(1, name.as_bytes()));
        let resp: Vec<u8> = [rec("Guitar"), rec("Plexi"), rec("--")].concat();
        let body = len_delimited(2, &len_delimited(5, &resp));
        let cut = body.len() - 3; // inside the final record
        let f_start = [&[0x00, MAGIC_IN_START, 0x00, cut as u8][..], &body[..cut]].concat();
        let f_final = [
            &[0x00, MAGIC_OUT, 0x00, (body.len() - cut) as u8][..],
            &body[cut..],
        ]
        .concat();

        // Both frames arrive → full body → strict decodes all 3 records.
        let full = reassemble_streams_final(&[f_start.clone(), f_final]);
        assert!(full
            .iter()
            .any(|s| preset_list_records_strict(&s.body).is_some_and(|r| r.len() == 3)));

        // Final 0x35 dropped → every reassembled stream is truncated → strict None.
        let dropped = reassemble_streams_final(&[f_start]);
        assert!(!dropped.is_empty());
        assert!(dropped
            .iter()
            .all(|s| preset_list_records_strict(&s.body).is_none()));
    }

    // song-write setters encode into SongMessage[11]{ <14|15|16|17> }.
    #[test]
    fn song_write_setters_encode() {
        // assignSongPreset[14]{ songSlot=1, songPresetSlot=2, userPresetSlot=3,
        //   footswitchLabel="Lead", footswitchColor=7, presetSceneSlot=4 }
        let body = assign_song_preset(1, 2, 3, "Lead", 7, 4);
        let inner = parse(first_bytes(&parse(&body), 11).unwrap());
        let assign = parse(first_bytes(&inner, 14).unwrap());
        assert_eq!(first_varint(&assign, 1), Some(1));
        assert_eq!(first_varint(&assign, 2), Some(2));
        assert_eq!(first_varint(&assign, 3), Some(3));
        assert_eq!(first_bytes(&assign, 4), Some(b"Lead".as_ref()));
        assert_eq!(first_varint(&assign, 5), Some(7));
        assert_eq!(first_varint(&assign, 6), Some(4));

        // move[15], swap[16], clear[17]
        let mv = parse(first_bytes(&parse(&move_song_preset(1, 2, 0)), 11).unwrap());
        let mv = parse(first_bytes(&mv, 15).unwrap());
        assert_eq!(
            (first_varint(&mv, 1), first_varint(&mv, 2)),
            (Some(1), Some(2))
        );
        let sw = parse(first_bytes(&parse(&swap_song_preset(1, 2, 3)), 11).unwrap());
        let sw = parse(first_bytes(&sw, 16).unwrap());
        assert_eq!(
            (first_varint(&sw, 2), first_varint(&sw, 3)),
            (Some(2), Some(3))
        );
        let cl = parse(first_bytes(&parse(&clear_song_preset(1, 2)), 11).unwrap());
        let cl = parse(first_bytes(&cl, 17).unwrap());
        assert_eq!(
            (first_varint(&cl, 1), first_varint(&cl, 2)),
            (Some(1), Some(2))
        );
    }

    // setFootswitchAssignment[54]{ footswitchAddress, functionIndex, functionJson, swap }
    // + clearFootswitchAssignment[55]{ footswitchAddress, functionIndex }.
    #[test]
    fn footswitch_assignment_setters_encode() {
        let body = set_footswitch_assignment(2, 1, "{\"func\":\"param\"}", false, None);
        let pm = parse(first_bytes(&parse(&body), 2).unwrap());
        let sfa = parse(first_bytes(&pm, 54).unwrap());
        assert_eq!(first_varint(&sfa, 1), Some(2));
        assert_eq!(first_varint(&sfa, 2), Some(1));
        assert_eq!(first_bytes(&sfa, 3), Some(b"{\"func\":\"param\"}".as_ref()));
        assert_eq!(first_varint(&sfa, 4), None, "swap=false omitted (proto3)");
        assert_eq!(
            first_varint(&parse(&body), 10),
            None,
            "setter: no batchStatus"
        );

        // swap=true sets field 4; batch=Some(11) adds top-level field 10.
        let body2 = set_footswitch_assignment(2, 1, "x", true, Some(11));
        let sfa2 = parse(first_bytes(&parse(first_bytes(&parse(&body2), 2).unwrap()), 54).unwrap());
        assert_eq!(first_varint(&sfa2, 4), Some(1));
        assert_eq!(first_varint(&parse(&body2), 10), Some(11));

        // functionIndex 0 is omitted (proto3) but still decodes as 0 on the device.
        let zero = set_footswitch_assignment(0, 0, "x", false, None);
        let sfa0 = parse(first_bytes(&parse(first_bytes(&parse(&zero), 2).unwrap()), 54).unwrap());
        assert_eq!(first_varint(&sfa0, 1), None);
        assert_eq!(first_varint(&sfa0, 2), None);

        let clr = clear_footswitch_assignment(3, 2);
        let cfa = parse(first_bytes(&parse(first_bytes(&parse(&clr), 2).unwrap()), 55).unwrap());
        assert_eq!(first_varint(&cfa, 1), Some(3));
        assert_eq!(first_varint(&cfa, 2), Some(2));
    }

    // loadScene activates a scene: PresetMessage[2]{ loadScene[101]{ sceneSlot } }.
    #[test]
    fn load_scene_encodes() {
        let pm = parse(first_bytes(&parse(&load_scene(2)), 2).unwrap());
        let ls = parse(first_bytes(&pm, 101).unwrap());
        assert_eq!(first_varint(&ls, 1), Some(2));
        // sceneSlot=0 is emitted EXPLICITLY (non-canonical proto3) — the device
        // ignores an empty LoadScene{}, so omission made scenes[0] unloadable
        // (HW-found).
        let pm0 = parse(first_bytes(&parse(&load_scene(0)), 2).unwrap());
        let ls0 = parse(first_bytes(&pm0, 101).unwrap());
        assert_eq!(first_varint(&ls0, 1), Some(0));
    }

    #[test]
    fn set_node_scene_edit_encodes() {
        let pm = parse(first_bytes(&parse(&set_node_scene_edit("G1", "ampA", true)), 2).unwrap());
        let sne = parse(first_bytes(&pm, 107).unwrap());
        // nodeId = field 1, groupId = field 2, sceneEditEnable = field 3.
        assert_eq!(first_bytes(&sne, 1), Some(&b"ampA"[..]));
        assert_eq!(first_bytes(&sne, 2), Some(&b"G1"[..]));
        assert_eq!(first_varint(&sne, 3), Some(1));
        // enable=false omits field 3 (proto3 default).
        let pm_off =
            parse(first_bytes(&parse(&set_node_scene_edit("G1", "ampA", false)), 2).unwrap());
        let sne_off = parse(first_bytes(&pm_off, 107).unwrap());
        assert_eq!(first_varint(&sne_off, 3), None);
    }

    #[test]
    fn song_preset_list_response_decodes_records() {
        // songMessage[11]{ songPresetListResponse[13]{
        //   record[2]={ userPresetSlot[2]=12, presetSceneSlot[5]=3, presetSceneName[6]="A" },
        //   record[2]={ isEmpty[1]=1 } } }.
        let mut r0 = Vec::new();
        field_varint(&mut r0, 2, 12);
        field_varint(&mut r0, 5, 3);
        field_bytes(&mut r0, 6, b"A");
        let mut r1 = Vec::new();
        field_varint(&mut r1, 1, 1);
        let mut resp = Vec::new();
        field_bytes(&mut resp, 2, &r0);
        field_bytes(&mut resp, 2, &r1);
        let body = len_delimited(11, &len_delimited(13, &resp));

        let recs = song_preset_list_records(&body);
        assert_eq!(recs.len(), 2);
        assert_eq!(first_varint(&recs[0], 2), Some(12)); // userPresetSlot
        assert_eq!(first_varint(&recs[0], 5), Some(3)); // presetSceneSlot
        assert_eq!(first_bytes(&recs[0], 6), Some(&b"A"[..])); // presetSceneName
        assert_eq!(first_varint(&recs[1], 1), Some(1)); // isEmpty
                                                        // A body with no songMessage yields no records (no panic).
        assert!(song_preset_list_records(&connection_request()).is_empty());
    }

    #[test]
    fn reassembly_concatenates_continuation_chunks() {
        // 0x33 start carries "AB", 0x34 continuation carries "CD".
        let r1 = vec![0x00, MAGIC_IN_START, 0x00, 0x02, b'A', b'B'];
        let r2 = vec![0x00, MAGIC_IN_FRAME, 0x00, 0x02, b'C', b'D'];
        let streams = reassemble_streams(&[r1, r2]);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].body, b"ABCD");
    }
}
