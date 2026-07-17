//! Decode a device backup archive into preset/scene/song data.

use crate::{
    audiograph, decode_preset_scenes, filter_amp_candidates, footswitch, is_amp_model_id,
    is_amp_output_level_param, session, LevelBlockArg,
};
use serde::Serialize;

/// One scene of a backup-read preset: its name + real footswitch tag (1-based
/// `FSn`, `None` when the scene has no footswitch). Mirrors the live field-3
/// scene model so the frontend treats backup-loaded and live scenes identically.
#[derive(Debug, Clone, Serialize)]
pub struct SceneInfo {
    pub name: String,
    pub fs: Option<u32>,
}

/// One preset row read from the backup DB (`UserPresets`).
#[derive(Debug, Clone, Serialize)]
pub struct BackupPresetRow {
    /// Device user slot (DB `slot`; = list index + 1).
    pub slot: i64,
    pub name: String,
    /// Number of scenes (`scenes.len()`); `-1` if the `presetJson` could not be
    /// parsed (full plaintext doc, so this is rare).
    pub scene_count: i64,
    /// Every scene with its name + footswitch tag, parsed from the DB
    /// `presetJson` (same shape as the live field-3 preset doc). Empty for a
    /// scene-less preset or an unparseable row.
    pub scenes: Vec<SceneInfo>,
    /// The preset's amp `outputLevel` leveling candidates, extracted from the same
    /// `presetJson` audioGraph at backup time — so per-scene leveling never has to
    /// run a live block-discovery session. Empty for a scene-less/unparseable row.
    pub amp_candidates: Vec<LevelBlockArg>,
    /// Every block in the preset's audioGraph (`(group, node_id, fender_id)`), parsed
    /// from the same `presetJson`. Drives Bulk Block Edit's Step-1 "blocks present"
    /// list + per-preset CPU total without any extra device round-trip. Empty for an
    /// unparseable row.
    pub blocks: Vec<BackupBlock>,
    /// The preset's routed signal-chain graph (lanes / topology / ordered stages),
    /// extracted from the SAME `presetJson` audioGraph via the same decoder the live
    /// active read uses ([`session::extract_active_graph`]). Lets the "Copy blocks
    /// between presets" frontend render each NON-active preset's real signal path
    /// without a per-preset device round-trip. A default (empty) [`ActiveGraph`] for an
    /// unparseable row.
    pub graph: session::ActiveGraph,
    /// Block-acting footswitches (on/off + parameter change) with leveling-candidate
    /// params, parsed from the same `presetJson` — drives the footswitch picker +
    /// preset-list tags for the WHOLE library with no extra device read. Empty otherwise.
    pub footswitches: Vec<footswitch::FootswitchInfo>,
    /// JSON-visible cause of a silent leveling capture ([`silence_hint`]), refining the
    /// generic "not on USB 1/2" verdict in the Level flow. `None` = no static cause.
    pub silence_hint: Option<&'static str>,
}

/// One block in a backup preset's audioGraph roster (see [`BackupPresetRow::blocks`]).
#[derive(Debug, Clone, Serialize)]
pub struct BackupBlock {
    /// audioGraph group key (`G1`…`G7`, `M1`…`M4`).
    pub group_id: String,
    /// The node's `nodeId` (falls back to `FenderId`).
    pub node_id: String,
    /// The node's `FenderId` (the exact, possibly suffixed, model id, e.g.
    /// `ACD_HiwattDR103CanModCabIR`) — what Bulk Block Edit matches on.
    pub fender_id: String,
}

/// One song→preset binding read from the backup `SongPresets` table. `song_slot` is
/// the device song slot (`Songs.slot`, 1-based positional — aligns 1:1 with the live
/// song list's `slot`, the same DB↔live positional alignment the setlist-membership
/// read relies on); `preset_slot` is the bound preset's device slot (`UserPresets.slot`,
/// 1-based = list index + 1). Read-only: which songs use a preset is set ON THE UNIT.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SongPresetBinding {
    pub song_slot: u32,
    pub preset_slot: u32,
}

// Songs/setlists from the backup reuse the live read types (`session::SongRecord`,
// `session::SetlistRecord`) — field-for-field identical, same snake_case wire — so the
// Songs tab maps the backup payload exactly like a live read with no conversion layer.

/// One setlist→song membership row (`SetlistSongs`): `position` is the song's 1-based
/// order within the setlist; `setlist_slot`/`song_slot` are the device slots.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BackupSetlistSong {
    pub setlist_slot: u32,
    pub song_slot: u32,
    pub position: u32,
}

/// Structured result of decoding a device backup archive into preset/scene data.
#[derive(Debug, Clone, Serialize)]
pub struct BackupReadResult {
    /// Archive entry names + sizes (`databaseBackup`, `settingsBackup`, …).
    pub members: Vec<(String, u64)>,
    /// Decompressed `normalDb.db3` (the `databaseBackup` entry) size in bytes.
    pub db_bytes: usize,
    /// Total `UserPresets` rows (matches the device My Presets slot count).
    pub total_rows: i64,
    /// How scene counts were obtained (or why not).
    pub scene_mode: String,
    /// Every non-empty named preset with its scene count.
    pub presets: Vec<BackupPresetRow>,
    /// Song→preset bindings (`SongPresets` table), so the Songs tab's Presets axis can
    /// show "which songs use this preset" without a live device read. Empty when the DB
    /// has no `Songs`/`SongPresets` tables (e.g. a UserPresets-only test fixture).
    pub song_presets: Vec<SongPresetBinding>,
    /// The full `Songs` table (name/notes/bpm) so the Songs tab paints from the backup
    /// instead of the slow live `read_song_list`. Empty when the DB has no `Songs` table.
    pub songs: Vec<session::SongRecord>,
    /// The full `Setlists` table (name) — same backup-first sourcing for the setlist list.
    pub setlists: Vec<session::SetlistRecord>,
    /// `SetlistSongs` membership (which songs are in each setlist, in order). Empty when
    /// the DB has no `SetlistSongs` table.
    pub setlist_songs: Vec<BackupSetlistSong>,
}

impl BackupReadResult {
    /// Sum of scene counts across parsed presets.
    pub fn total_scenes(&self) -> i64 {
        self.presets.iter().map(|p| p.scene_count.max(0)).sum()
    }
}

/// Decode a streamed device backup archive (GNU-tar + LZ4-frame) IN MEMORY and read
/// every preset + scene count out of its `databaseBackup` (= `/data/normalDb.db3`)
/// SQLite entry via the system `sqlite3`. The DB is written to a temp file (sqlite
/// needs a path) that is DELETED on every exit; the archive itself is never written
/// to disk — nothing persists (no stacking backups).
pub fn read_backup_archive(blob: &[u8]) -> Result<BackupReadResult, String> {
    use std::io::Read;

    if blob.is_empty() {
        return Err("backup archive is empty".to_string());
    }

    // LZ4-frame decode → tar bytes (libarchive's `archive_write_add_filter_lz4`
    // writes the standard LZ4 frame, magic 04 22 4d 18; lz4_flex reads it). Defensive:
    // skip any stray leading bytes before the magic (the archive is self-contained
    // from there) so a rare reassembly glitch can't block the decode.
    const LZ4_MAGIC: [u8; 4] = [0x04, 0x22, 0x4d, 0x18];
    let head: Vec<String> = blob.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let frame_off = if blob.starts_with(&LZ4_MAGIC) {
        0
    } else {
        blob.windows(4)
            .take(64)
            .position(|w| w == LZ4_MAGIC)
            .ok_or_else(|| {
                format!(
                    "no LZ4 frame magic in archive head (got {})",
                    head.join(" ")
                )
            })?
    };
    let mut tar_bytes = Vec::new();
    lz4_flex::frame::FrameDecoder::new(std::io::Cursor::new(&blob[frame_off..]))
        .read_to_end(&mut tar_bytes)
        .map_err(|e| {
            format!(
                "LZ4-frame decode failed (archive head {}): {e}",
                head.join(" ")
            )
        })?;

    // Untar in memory; pull out the DB entry, list the members. The user-preset DB
    // is stored under the logical tar entry name `databaseBackup` (firmware names
    // entries by role, not path).
    let mut members: Vec<(String, u64)> = Vec::new();
    let mut db_bytes: Option<Vec<u8>> = None;
    let mut ar = tar::Archive::new(std::io::Cursor::new(&tar_bytes));
    for entry in ar.entries().map_err(|e| format!("tar read: {e}"))? {
        let mut e = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = e
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path == "databaseBackup" || path.ends_with("normalDb.db3") {
            let mut buf = Vec::with_capacity(e.size() as usize);
            e.read_to_end(&mut buf)
                .map_err(|e| format!("tar extract db: {e}"))?;
            db_bytes = Some(buf);
        }
        members.push((path, e.size())); // move (read above happens first)
    }
    let db_bytes = db_bytes.ok_or_else(|| {
        format!(
            "databaseBackup entry not found; members: {}",
            members
                .iter()
                .map(|(p, _)| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    // Write the DB to a temp file (sqlite needs a path); delete it on every exit.
    struct TempDb(std::path::PathBuf);
    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let db_path = std::env::temp_dir().join(format!(
        "tmp-companion-backup-{}-{}.db3",
        std::process::id(),
        blob.len()
    ));
    std::fs::write(&db_path, &db_bytes).map_err(|e| format!("write temp db: {e}"))?;
    let _guard = TempDb(db_path.clone());

    let run_sql = |sql: &str| -> Result<serde_json::Value, String> {
        let out = std::process::Command::new("sqlite3")
            .arg("-json")
            .arg(&db_path)
            .arg(sql)
            .output()
            .map_err(|e| format!("sqlite3 spawn ({e}); is the CLI on PATH?"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let s = s.trim();
        if s.is_empty() {
            return Ok(serde_json::Value::Array(vec![]));
        }
        serde_json::from_str(s).map_err(|e| format!("parse sqlite json: {e}"))
    };

    // Pull the full plaintext preset doc per row; scene names + footswitch tags are
    // parsed in Rust by the SAME decoder the live field-3 / field-8 path uses
    // (`decode_preset_scenes`), so backup-loaded scenes match live scenes exactly.
    let rows = run_sql(
        "SELECT slot, displayName, presetJson FROM UserPresets \
         WHERE displayName IS NOT NULL ORDER BY slot",
    )?;

    let total_rows = run_sql("SELECT count(*) AS n FROM UserPresets")
        .ok()
        .and_then(|v| v.as_array()?.first()?.get("n")?.as_i64())
        .unwrap_or(-1);

    // Song→preset bindings from the SAME DB (no extra device read). `.ok()` so a
    // fixture/older DB without the `Songs`/`SongPresets` tables degrades to [] rather
    // than failing the whole backup read.
    let song_presets: Vec<SongPresetBinding> = run_sql(
        "SELECT s.slot AS song_slot, up.slot AS preset_slot \
         FROM SongPresets sp \
         JOIN Songs s        ON sp.Songs_id = s.id \
         JOIN UserPresets up ON sp.UserPresets_id = up.id \
         ORDER BY s.slot, sp.slot",
    )
    .ok()
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default()
    .iter()
    .filter_map(|r| {
        Some(SongPresetBinding {
            song_slot: r.get("song_slot")?.as_i64()? as u32,
            preset_slot: r.get("preset_slot")?.as_i64()? as u32,
        })
    })
    .collect();

    // The Songs/Setlists tabs read from the SAME backup (no live device reads). Each
    // `.ok()` so a fixture / older DB lacking a table degrades to [] rather than failing
    // the whole backup read.
    let rows_of = |sql: &str| -> Vec<serde_json::Value> {
        run_sql(sql)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };
    let songs: Vec<session::SongRecord> =
        rows_of("SELECT slot, name, notes, bpmActive, bpm FROM Songs ORDER BY slot")
            .iter()
            .filter_map(|r| {
                Some(session::SongRecord {
                    slot: r.get("slot")?.as_i64()? as u32,
                    name: r.get("name")?.as_str()?.to_string(),
                    notes: r
                        .get("notes")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    bpm: r.get("bpm").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    bpm_active: r.get("bpmActive").and_then(|v| v.as_i64()).unwrap_or(0) != 0,
                })
            })
            .collect();
    let setlists: Vec<session::SetlistRecord> =
        rows_of("SELECT slot, name FROM Setlists ORDER BY slot")
            .iter()
            .filter_map(|r| {
                Some(session::SetlistRecord {
                    slot: r.get("slot")?.as_i64()? as u32,
                    name: r.get("name")?.as_str()?.to_string(),
                })
            })
            .collect();
    let setlist_songs: Vec<BackupSetlistSong> = rows_of(
        "SELECT sl.slot AS setlist_slot, s.slot AS song_slot, ss.slot AS position \
         FROM SetlistSongs ss \
         JOIN Setlists sl ON ss.Setlists_id = sl.id \
         JOIN Songs s     ON ss.Songs_id    = s.id \
         ORDER BY sl.slot, ss.slot",
    )
    .iter()
    .filter_map(|r| {
        Some(BackupSetlistSong {
            setlist_slot: r.get("setlist_slot")?.as_i64()? as u32,
            song_slot: r.get("song_slot")?.as_i64()? as u32,
            position: r.get("position")?.as_i64()? as u32,
        })
    })
    .collect();

    let mut presets = Vec::new();
    let mut parsed = 0usize;
    let mut failed = 0usize;
    for r in rows.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let name = r.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
        if session::is_empty_slot_name(name) {
            continue;
        }
        // presetJson is plaintext JSON in the DB; decode scenes + FS tags. A parse
        // failure (missing/garbled scenes) → scene_count -1 so the UI can tell
        // "unknown" from a genuine scene-less preset (count 0).
        let js = r.get("presetJson").and_then(|v| v.as_str());
        let (scenes, scene_count) = match js.map(|js| decode_preset_scenes(js.as_bytes())) {
            Some(Ok(ps)) => {
                parsed += 1;
                let infos: Vec<SceneInfo> = ps
                    .scenes
                    .into_iter()
                    .zip(ps.fs)
                    .map(|(name, fs)| SceneInfo { name, fs })
                    .collect();
                let n = infos.len() as i64;
                (infos, n)
            }
            _ => {
                failed += 1;
                (Vec::new(), -1)
            }
        };
        // Amp leveling candidates + the full block roster straight from the same
        // presetJson audioGraph, so per-scene leveling skips the live block-discovery
        // round-trip and Bulk Block Edit gets its block list/CPU for free.
        let parsed_graph = js.and_then(session::tolerant_parse_json);
        let amp_candidates = parsed_graph
            .as_ref()
            .map(|v| filter_amp_candidates(session::extract_level_blocks(v)))
            .unwrap_or_default();
        let blocks = parsed_graph
            .as_ref()
            .map(|v| {
                audiograph::roster(v)
                    .into_iter()
                    .map(|(group_id, node_id, fender_id)| BackupBlock {
                        group_id,
                        node_id,
                        fender_id,
                    })
                    .collect()
            })
            .unwrap_or_default();
        // The routed signal-chain graph (lanes/topology/stages) from the same
        // presetJson — `extract_active_graph` reads `audioGraph.template` itself, so
        // no template hint is needed. A default (empty) graph for an unparseable row.
        let graph = parsed_graph
            .as_ref()
            .map(|v| session::extract_active_graph(v, None))
            .unwrap_or_default();
        // Block-acting footswitches from the same presetJson — no extra device read.
        let footswitches = parsed_graph
            .as_ref()
            .and_then(|v| {
                v.get("ftsw")
                    .map(|ftsw| footswitch::enumerate_block_footswitches(ftsw, v))
            })
            .unwrap_or_default();
        presets.push(BackupPresetRow {
            slot: r.get("slot").and_then(|v| v.as_i64()).unwrap_or(-1),
            name: name.to_string(),
            scene_count,
            scenes,
            amp_candidates,
            blocks,
            graph,
            footswitches,
            silence_hint: parsed_graph.as_ref().and_then(silence_hint),
        });
    }
    let scene_mode = format!("parsed scenes from presetJson ({parsed} ok, {failed} unparseable)");

    Ok(BackupReadResult {
        members,
        db_bytes: db_bytes.len(),
        total_rows,
        scene_mode,
        presets,
        song_presets,
        songs,
        setlists,
        setlist_songs,
    })
}

/// Layer-1 static pre-flight for the "not on USB 1/2" clamp: the JSON-visible cause of
/// a silent leveling capture, or `None` when the preset shows none (a silent capture
/// then really is output routing on the unit).
///
/// - `"amp_zero"` — every ACTIVE (non-bypassed) amp's `outputLevel` is saved at ~0.
///   `outputLevel`=0 is deep digital silence on the real TMP, so nothing reaches the
///   USB 1/2 capture regardless of routing (HW-verified: a zeroed-amp preset measures
///   the −120 silent sentinel).
/// - `"exp_mute"` — an expression-pedal binding drives an amp `outputLevel` with ~0 at
///   one end: a physical pedal parked there mutes the preset live even though the
///   SAVED value is healthy (the HW-confirmed field case — a pedal-less unit levels
///   the same preset perfectly). Conditional, hence outranked by `amp_zero`.
///
/// Amp identity/param via the same predicates the scene leveller uses
/// (`is_amp_model_id` / `is_amp_output_level_param`).
// ponytail: misses a zeroed amp in SERIES with a live one (all-active-zero rule);
// refine to lane-aware if a real preset ever hits it.
pub(crate) fn silence_hint(v: &serde_json::Value) -> Option<&'static str> {
    const EPS: f64 = 1e-3;
    let mut active = 0usize;
    let mut zeroed = 0usize;
    audiograph::for_each_node(v, |node| {
        let model = node.get("FenderId").and_then(|x| x.as_str()).unwrap_or("");
        if !is_amp_model_id(model) {
            return;
        }
        let params = node.get("dspUnitParameters");
        let bypassed = params
            .and_then(|p| p.get("bypass"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        if bypassed {
            return; // a bypassed amp passes signal flat — its outputLevel is inert
        }
        active += 1;
        let out = params
            .and_then(|p| p.get("outputLevel"))
            .and_then(|x| x.as_f64());
        if matches!(out, Some(f) if f <= EPS) {
            zeroed += 1;
        }
    });
    if active > 0 && zeroed == active {
        return Some("amp_zero");
    }
    for pedal in ["exp1", "exp2"] {
        let entries = v
            .get("exp")
            .and_then(|e| e.get(pedal))
            .and_then(|a| a.as_array());
        for b in entries.into_iter().flatten() {
            let is_param = b.get("func").and_then(|f| f.as_str()) == Some("param");
            let param = b.get("paramId").and_then(|p| p.as_str()).unwrap_or("");
            // NaN-safe: a missing heel/toe compares false, never flags.
            let end = |k: &str| b.get(k).and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
            let group_id = b.get("groupId").and_then(|g| g.as_str()).unwrap_or("");
            let node_id = b.get("nodeId").and_then(|n| n.as_str()).unwrap_or("");
            if is_param
                && is_amp_output_level_param(param)
                && (end("heel") <= EPS || end("toe") <= EPS)
                && is_active_amp_node(v, group_id, node_id)
            {
                return Some("exp_mute");
            }
        }
    }
    None
}

/// Resolve an expression-binding's `groupId`/`nodeId` against the graph and confirm
/// it targets a present, non-bypassed amp — a stale binding, a missing target, or
/// another block that merely exposes a same-named `outputLevel` param must NOT flag
/// `exp_mute` (that would replace the routing fallback with misleading advice).
fn is_active_amp_node(v: &serde_json::Value, group_id: &str, node_id: &str) -> bool {
    if group_id.is_empty() || node_id.is_empty() {
        return false;
    }
    for graph in ["guitarNodes", "micNodes"] {
        let Some(nodes) = v
            .get("audioGraph")
            .and_then(|ag| ag.get(graph))
            .and_then(|g| g.get(group_id))
            .and_then(|a| a.as_array())
        else {
            continue;
        };
        for node in nodes {
            if audiograph::node_id(node) != Some(node_id) {
                continue;
            }
            let model = node.get("FenderId").and_then(|x| x.as_str()).unwrap_or("");
            let bypassed = node
                .get("dspUnitParameters")
                .and_then(|p| p.get("bypass"))
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            return is_amp_model_id(model) && !bypassed;
        }
    }
    false
}

#[cfg(test)]
#[path = "backup_read_tests.rs"]
mod tests;
