//! Pure decoders for device pushes and preset JSON: protobuf field digging, push
//! descriptions, the tolerant JSON parse and the live-scene harvest. No I/O.

use super::*;

// ─── Live-push decoders (shared by `describe_push` AND the device monitor) ─────
// PresetMessage push field numbers (the unit's unsolicited pushes, TMS field 2).
pub(super) const PRESET_LOADED_PUSH: u32 = 11; // PresetLoaded

pub(super) const CURRENT_PRESET_DATA_CHANGED: u32 = 3; // currentPresetDataChanged (LZ4 preset JSON)

pub(super) const SCENE_LOADED: u32 = 102; // SceneLoaded { sceneSlot, sceneJson (LZ4) }

pub(super) const SCENE_LIST_RESPONSE: u32 = 125; // sceneListResponse { dummy, sceneList[] }

/// Decoded `PresetLoaded` (TMS[2] → [11]) addressing fields. All slots are the
/// device's own (1-based where applicable); the caller applies the list-index
/// translation. The full set the unit pushes on a preset change.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PresetLoadedFields {
    pub tab_enum: u64,
    pub setlist_slot: u64,
    pub setlist_song_slot: u64,
    pub song_slot: u64,
    pub song_preset_slot: u64,
    pub preset_slot: u64,
    pub sync: u64,
}

/// Whether `loadScene(n)` is safe on the preset `doc` describes: the base sentinel
/// ([`BASE_SCENE_SLOT`]) always is; any other index must name one of `doc`'s `scenes[]`.
/// On a scene-less preset the unit swaps in its EMPTY template (presetError 6, static RE),
/// and an index past the scenes aborts the device server (static RE).
pub(crate) fn scene_index_ok(doc: &serde_json::Value, n: u32) -> bool {
    n == BASE_SCENE_SLOT
        || doc
            .get("scenes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|scenes| (n as usize) < scenes.len())
}

/// `presetError` code 6: the unit substituted its EMPTY template for the working copy
/// (fw 1.8.58: a body rejected at load, device-confirmed; `loadScene(N≠8)` on a
/// scene-less preset, static RE).
pub(crate) const PRESET_ERROR_TEMPLATE: u64 = 6;

/// Whether `body` is a `presetError` (TMS[2] → [53]) with [`PRESET_ERROR_TEMPLATE`].
pub(crate) fn is_template_substitution(body: &[u8]) -> bool {
    dig(body, TMS_PRESET, PRESET_ERROR)
        .is_some_and(|err| proto::first_varint(&err, 1) == Some(PRESET_ERROR_TEMPLATE))
}

/// Decode a `PresetLoaded` push (TMS[2] → [11]). `None` if this body is not one.
pub(crate) fn decode_preset_loaded(body: &[u8]) -> Option<PresetLoadedFields> {
    let f = dig(body, TMS_PRESET, PRESET_LOADED_PUSH)?;
    let g = |n| field_n(&f, n).and_then(Val::as_u64).unwrap_or(0);
    Some(PresetLoadedFields {
        tab_enum: g(1),
        setlist_slot: g(2),
        setlist_song_slot: g(3),
        song_slot: g(4),
        song_preset_slot: g(5),
        preset_slot: g(6),
        sync: g(7),
    })
}

/// Decode a `CurrentPresetInfoChanged` push (TMS[2] → [22]) into
/// `(displayName, isDirty, isFavorite)`. `None` if this body is not one.
/// Per `CurrentPresetInfoChanged.proto`: displayName(1) bytes, isDirty(2) bool,
/// isFavorite(3) bool.
pub(crate) fn decode_info_changed(body: &[u8]) -> Option<(String, bool, bool)> {
    let f = dig(body, TMS_PRESET, CURRENT_PRESET_INFO_CHANGED)?;
    let name = field_n(&f, 1)
        .and_then(Val::as_bytes)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let is_dirty = field_n(&f, 2).and_then(Val::as_u64).unwrap_or(0) != 0;
    let is_favorite = field_n(&f, 3).and_then(Val::as_u64).unwrap_or(0) != 0;
    Some((name, is_dirty, is_favorite))
}

/// Decode a `SceneLoaded` push (TMS[2] → [102]) into `(sceneSlot, sceneName)`.
/// `sceneName` is parsed from the LZ4-block `sceneJson` (field 2) — `None` when
/// the payload is absent, not LZ4, or truncated before the `sceneName` property.
/// `None` for the whole tuple if this body is not a `SceneLoaded`.
pub(crate) fn decode_scene_loaded(body: &[u8]) -> Option<(u32, Option<String>)> {
    let f = dig(body, TMS_PRESET, SCENE_LOADED)?;
    let slot = field_n(&f, 1).and_then(Val::as_u64).unwrap_or(0) as u32;
    let name = proto::first_bytes(&f, 2)
        .and_then(decode_preset_json)
        .and_then(|text| extract_partial_json_string(&text, "sceneName"));
    Some((slot, name))
}

/// Decode a `sceneListResponse` push (TMS[2] → [125]) into the live scene NAMES
/// of the ACTIVE preset (repeated string `sceneList`, field 2). `None` if this
/// body is not a `sceneListResponse`. An empty Vec is a valid result (a preset
/// with no FS scenes).
pub(crate) fn decode_scene_list(body: &[u8]) -> Option<Vec<String>> {
    let f = dig(body, TMS_PRESET, SCENE_LIST_RESPONSE)?;
    Some(
        proto::all_bytes(&f, 2)
            .into_iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect(),
    )
}

/// Decode a `currentPresetDataChanged` push (TMS[2] → [3]) into an [`ActiveGraph`]
/// — the same decode pipeline `current_audio_graph` runs (LZ4 → tolerant JSON →
/// `extract_active_graph`), with the SAME known-routing-template guard so a
/// too-truncated partial yields `None` rather than a garbage chain. `None` if
/// this body is not a `currentPresetDataChanged`, or the partial truncated before
/// a complete `audioGraph.template`. Thin wrapper over [`decode_current_preset_live`]
/// so a body is parsed ONCE when both the scene trio and the graph are wanted.
#[cfg(test)]
pub(crate) fn decode_current_preset_data(body: &[u8]) -> Option<ActiveGraph> {
    decode_current_preset_live(body)?.graph
}

/// Extract the [`ActiveGraph`] from an already-parsed field-3 preset JSON, applying
/// the known-routing-template guard (mirrors `current_audio_graph`). `text` supplies
/// the truncation-tolerant `template` hint.
pub(super) fn active_graph_guarded(v: &serde_json::Value, text: &str) -> Option<ActiveGraph> {
    let template_hint = extract_partial_json_string(text, "template");
    let graph = extract_active_graph(v, template_hint.as_deref());
    // A non-empty chain must carry a recognized routing template, else the strip could
    // draw parallel as series; an empty-node partial (no template) is also rejected.
    if graph.nodes.is_empty() || !is_known_routing_template(graph.template.as_deref()) {
        return None;
    }
    Some(graph)
}

/// The device's base scene slot on the wire: `loadScene`/`SceneLoaded`/`lastLoadedScene`
/// address FS scenes as 0-based `scenes[]` indices (0..=7) and the base scene as the
/// CONSTANT `8` — even for a preset with zero FS scenes (HW-observed: Cello, 0 scenes,
/// base SceneLoaded sceneSlot=8; Guitar dump `lastLoadedScene: 8` while on base). NOT
/// `scene_count + 1` — that old theory is refuted by the Cello observation.
pub(crate) const BASE_SCENE_SLOT: u32 = 8;

/// The live scene metadata harvested from ONE `currentPresetDataChanged` push
/// (TMS[2] → [3], LZ4) — the same field-3 body as [`decode_current_preset_data`], but
/// surfacing the scene-facing trio instead of the graph (the monitor parses both from
/// one push). All three come from a SINGLE coherent preset-JSON document, so their
/// index spaces agree by construction:
/// - `scene_names` — `scenes[].sceneName` in slot order (the canonical scene rows),
/// - `last_loaded_scene` — the ACTIVE scene slot (0-based into `scenes[]`;
///   [`BASE_SCENE_SLOT`] = base), present in every push,
/// - `ftsw` — the footswitch assignments (`sceneSlot` indexes `scenes[]`).
///
/// Each half is `None` when the tolerant parse of a truncated partial lost it; on a
/// healthy dense-heartbeat session the payload carries all three (HW-verified: a 17 KB live field-3 truncates only inside the FINAL scene's `uuid`,
/// after every `sceneName`).
pub(crate) struct CurrentPresetLive {
    pub scene_names: Option<Vec<String>>,
    pub last_loaded_scene: Option<u32>,
    pub ftsw: Option<serde_json::Value>,
    /// The signal-chain graph from the SAME parse (template-guarded; `None` when the
    /// partial truncated before a complete `audioGraph.template`). The monitor reads
    /// both halves from one decode rather than parsing the ~17 KB body twice per push.
    pub graph: Option<ActiveGraph>,
}

/// Decode the scene-facing trio + the signal-chain graph from a
/// `currentPresetDataChanged` push, in ONE LZ4-decompress + tolerant-parse pass.
/// `None` if this is not a field-3 body or the LZ4/parse failed entirely.
pub(crate) fn decode_current_preset_live(body: &[u8]) -> Option<CurrentPresetLive> {
    let f = dig(body, TMS_PRESET, CURRENT_PRESET_DATA_CHANGED)?;
    let payload = proto::first_bytes(&f, 1)?;
    let text = decode_preset_json(payload)?;
    decode_preset_live_text(&text)
}

/// Decode the scene-facing trio + signal-chain graph from plaintext preset JSON
/// bytes, such as a slot-addressed field-9 `presetDataChanged` partial. This is the
/// same tolerant parse used for live field-3, minus the TMS/LZ4 wrapper step.
pub(crate) fn decode_plain_preset_live(json: &[u8]) -> Option<CurrentPresetLive> {
    let text = String::from_utf8_lossy(json);
    decode_preset_live_text(&text)
}

pub(super) fn decode_preset_live_text(text: &str) -> Option<CurrentPresetLive> {
    let v = tolerant_parse_json(text)?;
    let scene_names = v.get("scenes").and_then(|s| s.as_array()).map(|arr| {
        arr.iter()
            .map(|sc| {
                sc.get("sceneName")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    });
    let last_loaded_scene = v
        .get("lastLoadedScene")
        .and_then(|n| n.as_u64())
        .map(|n| n as u32);
    let graph = active_graph_guarded(&v, text);
    Some(CurrentPresetLive {
        scene_names,
        last_loaded_scene,
        ftsw: v.get("ftsw").cloned(),
        graph,
    })
}

/// Scene names (slot order) from a slot-addressed field-9 **plaintext**
/// preset-JSON partial ([`Session::read_slot_preset_json`]). Same tolerant
/// parse as the live field-3 decode — large presets device-truncate inside the
/// FINAL scene's `uuid`, after every `sceneName` (HW: slot 1 = 17264 B, all 8
/// names present). `None` = the partial truncated before the `scenes` key
/// (scene presence unknown), distinct from `Some(vec![])` (no scenes).
pub(crate) fn scene_names_from_slot_json(json: &[u8]) -> Option<Vec<String>> {
    decode_plain_preset_live(json)?.scene_names
}

/// Descend two protobuf levels: parse `body` as FenderMessageTMS, take the
/// `tms_field` sub-message, parse it, take the `inner_field` sub-message, and
/// return its decoded fields. None if either level is absent.
pub(super) fn dig(body: &[u8], tms_field: u32, inner_field: u32) -> Option<Vec<(u32, Val)>> {
    let top = proto::parse(body);
    let sub = proto::first_bytes(&top, tms_field)?;
    let mid = proto::parse(sub);
    let inner = proto::first_bytes(&mid, inner_field)?;
    Some(proto::parse(inner))
}

pub(super) fn field1(fields: &[(u32, Val)]) -> Option<&Val> {
    field_n(fields, 1)
}

/// One-line human description of an inbound stream for the push-listener
/// experiment (`listen_dump`). Generic field-path summary like `send_and_dump`,
/// with the known push messages decoded: PresetLoaded 2[11], SceneLoaded 2[102]
/// (sceneSlot + a sceneJson size/prefix peek), currentPresetDataChanged 2[3],
/// CurrentPresetInfoChanged 2[22].
pub(super) fn describe_push(body: &[u8]) -> String {
    let top = proto::parse(body);
    // presetMessage pushes get the detailed treatment.
    if let Some(pm) = proto::first_bytes(&top, TMS_PRESET) {
        let inner = proto::parse(pm);
        if let Some(pl) = decode_preset_loaded(body) {
            // Byte-identical to the previous inline decode (now via the shared
            // `decode_preset_loaded`, so the monitor and the listener agree).
            return format!(
                "PresetLoaded 2[11]: tab={} setlist={} setlistSong={} song={} songPreset={} presetSlot={} sync={}",
                pl.tab_enum, pl.setlist_slot, pl.setlist_song_slot, pl.song_slot, pl.song_preset_slot, pl.preset_slot, pl.sync
            );
        }
        if let Some(sl) = proto::first_bytes(&inner, 102) {
            let f = proto::parse(sl);
            let slot = field_n(&f, 1).and_then(Val::as_u64).unwrap_or(0);
            let json = proto::first_bytes(&f, 2).unwrap_or(&[]);
            let text = decode_preset_json(json).unwrap_or_default();
            let prefix: String = text.chars().take(160).collect();
            return format!("SceneLoaded 2[102]: sceneSlot={slot} sceneJson={}B (decoded {}B) prefix={prefix:?}", json.len(), text.len());
        }
        if let Some(ch) = proto::first_bytes(&inner, 3) {
            let f = proto::parse(ch);
            let json = proto::first_bytes(&f, 1).unwrap_or(&[]);
            let text = decode_preset_json(json).unwrap_or_default();
            let prefix: String = text.chars().take(80).collect();
            return format!(
                "currentPresetDataChanged 2[3]: payload={}B (decoded {}B) prefix={prefix:?}",
                json.len(),
                text.len()
            );
        }
        if let Some(ci) = proto::first_bytes(&inner, 22) {
            let f = proto::parse(ci);
            let parts: Vec<String> = f
                .iter()
                .map(|(n, v)| match (v.as_u64(), v.as_bytes()) {
                    (Some(u), _) => format!("{n}={u}"),
                    (_, Some(b)) => format!("{n}={:?}", String::from_utf8_lossy(b)),
                    _ => format!("{n}=?"),
                })
                .collect();
            return format!("CurrentPresetInfoChanged 2[22]: {{{}}}", parts.join(", "));
        }
        let fields: Vec<u32> = inner.iter().map(|(g, _)| *g).collect();
        return format!("presetMessage 2{fields:?} ({}B)", body.len());
    }
    // ConnectionMessage (TMS field 4): connectionRequest=1 / connectionResponse=2 /
    // connectionError=3 / connectionHeartbeat=4. Hex-dump the body so the error
    // code is visible (the heartbeat draws a connectionError — code unknown).
    if let Some(cm) = proto::first_bytes(&top, 4) {
        let inner = proto::parse(cm);
        let which = inner.first().map(|(g, _)| *g).unwrap_or(0);
        let label = match which {
            1 => "connectionRequest",
            2 => "connectionResponse",
            3 => "connectionError",
            4 => "connectionHeartbeat",
            _ => "connection?",
        };
        let detail: Vec<String> = inner
            .iter()
            .map(|(n, v)| match (v.as_u64(), v.as_bytes()) {
                (Some(u), _) => format!("{n}={u}"),
                (_, Some(b)) => format!("{n}={:?}", String::from_utf8_lossy(b)),
                _ => format!("{n}=?"),
            })
            .collect();
        return format!(
            "ConnectionMessage 4[{which}] {label} {{{}}} hex={}",
            detail.join(","),
            hexs(body)
        );
    }
    // Everything else: top-level field path, expanding one level where useful.
    let mut desc = Vec::new();
    for (f, _) in &top {
        if let Some(b) = proto::first_bytes(&top, *f) {
            let inner: Vec<u32> = proto::parse(b).iter().map(|(g, _)| *g).collect();
            if !inner.is_empty() {
                desc.push(format!("{f}{inner:?}"));
                continue;
            }
        }
        desc.push(f.to_string());
    }
    // Hex-dump small unrecognized streams (≤32B) so nothing is lost to the summary.
    let hex = if body.len() <= 32 {
        format!(" hex={}", hexs(body))
    } else {
        String::new()
    };
    format!("fields={} ({}B){hex}", desc.join(","), body.len())
}

/// Lowercase hex of a byte slice (capped to 64 bytes) for the listener's dumps.
pub(super) fn hexs(b: &[u8]) -> String {
    b.iter().take(64).map(|x| format!("{x:02x}")).collect()
}

pub(super) fn field_n(fields: &[(u32, Val)], field_no: u32) -> Option<&Val> {
    fields.iter().find(|(f, _)| *f == field_no).map(|(_, v)| v)
}

/// Decode a `presetJson` payload to text: try LZ4-block decompress (the field-3
/// `currentPresetDataChanged` wire form), else treat as literal UTF-8 (the
/// field-79 raw form). Lossy UTF-8 so a stream truncated mid-multibyte still
/// yields the valid prefix (the device sends a partial preset in the change
/// event — `audioGraph` lives in the first ~1.5 KB and survives).
pub(super) fn decode_preset_json(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    if let Ok(raw) = proto::lz4_block_decompress(payload) {
        if !raw.is_empty() {
            return Some(String::from_utf8_lossy(&raw).into_owned());
        }
    }
    Some(String::from_utf8_lossy(payload).into_owned())
}

/// [`tolerant_parse_json`] with the readers' shared error.
pub(super) fn parse_preset_text(text: &str) -> Result<serde_json::Value, String> {
    tolerant_parse_json(text)
        .ok_or_else(|| "could not parse preset JSON, even tolerantly".to_string())
}

/// Parse possibly-TRUNCATED preset JSON. Tries a strict parse first; on failure
/// (the preset-data stream is routinely truncated mid-object), rewinds to the
/// last `,` outside a string and closes all open `[`/`{` containers in reverse
/// nesting order,
/// recovering everything up to that boundary. Port of `drive_replace_node.py`'s
/// `_tolerant_parse_partial_json` — `audioGraph.guitarNodes` sits early enough to
/// always survive this for our enumeration purposes.
pub(crate) fn tolerant_parse_json(text: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return Some(v);
    }
    let (mut in_str, mut esc) = (false, false);
    let mut stack = Vec::new();
    let mut last_cut: Option<(usize, Vec<char>)> = None; // (byte pos before ',', open containers)
    for (i, c) in text.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            _ if in_str => {}
            '{' | '[' => stack.push(c),
            '}' if stack.pop() != Some('{') => return None,
            ']' if stack.pop() != Some('[') => return None,
            ',' if !stack.is_empty() => last_cut = Some((i, stack.clone())),
            _ => {}
        }
    }
    let (pos, stack) = last_cut?;
    let mut padded = text[..pos].to_string();
    padded.extend(
        stack
            .into_iter()
            .rev()
            .map(|c| if c == '[' { ']' } else { '}' }),
    );
    serde_json::from_str(&padded).ok()
}

/// Did the read deliver `key`'s section WHOLE — i.e. did the bytes reach its closing
/// bracket?
///
/// A slot-addressed field-8 read returns a device-TRUNCATED partial on a large preset,
/// and [`tolerant_parse_json`] salvages the prefix: the document parses, a tail section
/// is simply gone, and a caller reading it sees "this preset has no footswitches" on a
/// preset that has ten. The parsed value cannot tell those apart — a section that never
/// arrived and a section the preset genuinely does not carry both read as an absent key,
/// and a section cut mid-way reads as a short-but-plausible array. Only the RAW bytes
/// carry the answer, which is why this scans them.
///
/// Order-free on purpose: it neither assumes where `key` sits in the document nor how
/// many entries the section should hold (a real device export has been seen with 9
/// `ftsw` rows, not 20). A body that strict-parses is complete by construction, so an
/// absent or empty section in one is GENUINE, not a truncation.
pub(crate) fn json_section_complete(text: &str, key: &str) -> bool {
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        return true;
    }
    let needle = format!("\"{key}\"");
    // Every occurrence, not the first: the key name can also appear as a string VALUE
    // (a footswitch `customLabel`, say), which is not the section.
    text.match_indices(&needle)
        .any(|(i, _)| section_closes(&text[i + needle.len()..]))
}

/// Does the `: [ … ]` / `: { … }` immediately after a key close within `tail`? Escape-
/// and string-aware, like [`tolerant_parse_json`]'s scanner — the field-8 tail can cut
/// anywhere, including inside a string.
pub(super) fn section_closes(tail: &str) -> bool {
    let Some(tail) = tail.trim_start().strip_prefix(':') else {
        return false;
    };
    let tail = tail.trim_start();
    let open = match tail.chars().next() {
        Some(c @ ('[' | '{')) => c,
        // OBJECT/ARRAY sections only. A scalar value (a number, a string) here always
        // reads as INCOMPLETE — this scanner has no way to tell a scalar's own tail
        // truncation from an intact one, so it never signals "closed" for one. Callers
        // must not pass a scalar key to `json_section_complete`.
        _ => return false,
    };
    let close = if open == '[' { ']' } else { '}' };
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for c in tail.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            _ if in_str => {}
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Recover a JSON string value from a truncated payload. The live active-preset
/// stream can end in the middle of `"template":"gtrParallel2"`, after enough
/// bytes to identify the routing template but before strict/tolerant parsing can
/// retain that final property.
pub(super) fn extract_partial_json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let tail = text.rsplit_once(&needle)?.1.trim_start();
    let tail = tail.strip_prefix(':')?.trim_start();
    let mut chars = tail.strip_prefix('"')?.chars();
    let mut out = String::new();
    let mut escaped = false;
    for c in chars.by_ref() {
        if escaped {
            out.push(c);
            escaped = false;
        } else {
            match c {
                '\\' => escaped = true,
                '"' => break,
                _ => out.push(c),
            }
        }
    }
    (!out.is_empty()).then_some(out)
}
