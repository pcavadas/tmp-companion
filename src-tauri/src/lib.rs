//! TMP Companion — Tauri backend entry point.
//!
//! The app drives a USB-connected Fender Tone Master Pro in re-amp mode to
//! auto-level presets to a LUFS target via a closed loop:
//!   load preset → play sample → capture processed output → measure LUFS →
//!   adjust `presetLevel` → repeat until on target → save.
//!
//! Module layout (filled in milestone by milestone — see the plan):
//!   hid       — IOKit exclusive-seize HID transport (runloop thread)         [M1]
//!   proto     — hand-rolled FenderMessageTMS encode/decode                   [M1]
//!   session   — handshake + preset list / load / level / save / re-amp       [M1]
//!   audio     — cpal re-amp playback + capture, window alignment             [M2]
//!   lufs      — ebur128 loudness measurement                                 [M2]
//!   leveller  — the closed loop, emits progress events                       [M3]

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

// Several builders/methods are exercised only from M2/M3 onward; silence
// dead-code noise until then without weakening warnings elsewhere.
#[allow(dead_code)]
mod audio;
mod audiograph;
mod audition;
mod backup;
mod blockcaps;
mod blocklib;
mod bulk_cmd;
mod bulkrun;
#[cfg(target_os = "macos")]
mod dock;
mod footswitch;
#[allow(dead_code)]
mod hid;
mod ir;
#[allow(dead_code)]
mod leveller;
mod library;
mod lint;
#[allow(dead_code)]
mod lufs;
mod migration;
mod monitor;
mod paramedit;
mod preset_io;
mod presetmeta;
mod profiles;
#[allow(dead_code)]
mod proto;
mod rename;
mod scenes;
mod search;
#[allow(dead_code)]
mod session;
#[cfg(any(test, feature = "e2e"))]
mod sim_device;
mod spectrum;
// `pub` so the `gen_samples` bin (a separate crate) can reach the shared
// catalog as `tmp_companion_lib::topologies`.
pub mod topologies;
mod variants;
mod watcher;

pub use session::PresetEntry;
use session::Session;
pub use session::{ActiveGraph, GraphNode, Stage};

/// Headless hardware probe used by the `probe` bin: connect (seizing the
/// device), run the handshake, and return the "My Presets" list. Lets us verify
/// the HID stack against a real TMP without launching the GUI.
pub fn probe_connect_and_list() -> Result<Vec<PresetEntry>, String> {
    let mut s = Session::connect()?;
    s.list_my_presets()
}

/// Headless firmware-version read (`probe --fw`): connect, request the version
/// in-burst (`currentFwRequest`), and return the `currentFwResponse` data.
pub fn probe_firmware_version() -> Result<String, String> {
    let s = Session::connect_with_firmware()?;
    s.firmware_version()
        .ok_or_else(|| "handshake carried no currentFwResponse".to_string())
}

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

/// READ-ONLY RE spike (`probe --re-blocks`): fire `RequestAllBlockPresets` (135) +
/// the user-IR list request inside the handshake burst and dump every saved-block /
/// IR response the device streams back, so the opaque `blockPresetsMap` blob and the
/// IR list shape can be decoded. No device writes; nothing persists (optional raw
/// dumps only when `TMP_RE_OUT=<dir>` is set). Used once to derive the wire schema;
/// the production readers (`list_saved_blocks` / `list_user_irs`) build on what it finds.
pub fn probe_re_blocks() -> Result<String, String> {
    let hexn = |b: &[u8], n: usize| -> String {
        b.iter()
            .take(n)
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Attempt 1: ride the request inside the handshake burst (batch-2 group).
    let mut s = Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
    for _ in 0..4 {
        s.pump_collect(250)?;
    }
    // Attempt 2: send it post-handshake as a request/response message (no batchStatus),
    // keeping the session alive with heartbeats — the framing the ReplaceNode family uses.
    s.heartbeat()?;
    s.pump_collect(80)?;
    s.send_and_collect(&proto::request_all_block_presets(None), 600)?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(250)?;
    }
    let bodies = s.push_bodies();
    drop(s); // release the HID seize before host-side work

    let out_dir = std::env::var("TMP_RE_OUT").ok();
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --re-blocks] scanned {} reassembled bodies\n",
        bodies.len()
    ));

    // Top-level TMS field histogram, so we can see what arrived.
    let mut top_hist: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for b in &bodies {
        for (f, _) in proto::parse(b) {
            *top_hist.entry(f).or_default() += 1;
        }
    }
    report.push_str(&format!("top TMS fields seen: {top_hist:?}\n"));

    // Per-body inner-field map (so we can see what the device actually streamed and
    // whether 135/136 ever appears under PresetMessage).
    for (bi, b) in bodies.iter().enumerate() {
        let top = proto::parse(b);
        let tf: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
        let mut inner_desc = String::new();
        for (f, _) in &top {
            if let Some(inner_bytes) = proto::first_bytes(&top, *f) {
                let inner = proto::parse(inner_bytes);
                let ifields: Vec<u32> = inner.iter().map(|(x, _)| *x).collect();
                inner_desc.push_str(&format!(" top{f}{ifields:?}"));
            }
        }
        report.push_str(&format!("  body {bi}: top {tf:?} ·{inner_desc}\n"));
    }

    let mut found_blocks = 0usize;
    let mut found_updated = 0usize;
    let mut found_ir = 0usize;
    for (bi, b) in bodies.iter().enumerate() {
        let top = proto::parse(b);
        // PresetMessage (top field 2): inner 136 = AllBlockPresetsResponse, 96 = BlockPresetUpdated.
        if let Some(pm) = proto::first_bytes(&top, 2) {
            let inner = proto::parse(pm);
            let inner_fields: Vec<u32> = inner.iter().map(|(f, _)| *f).collect();
            if let Some(resp) = proto::first_bytes(&inner, 136) {
                // AllBlockPresetsResponse { bytes blockPresetsMap = 1 }.
                let map_bytes = proto::parse(resp);
                let blob = proto::first_bytes(&map_bytes, 1).unwrap_or(resp);
                found_blocks += 1;
                report.push_str(&format!(
                    "\n=== AllBlockPresetsResponse (body {bi}) ===\n  inner fields {inner_fields:?}\n  blockPresetsMap: {} bytes\n  head: {}\n",
                    blob.len(),
                    hexn(blob, 64)
                ));
                if let Some(dir) = &out_dir {
                    let p = format!("{dir}/blockPresetsMap.bin");
                    std::fs::write(&p, blob).map_err(|e| format!("write {p}: {e}"))?;
                    report.push_str(&format!("  wrote {p}\n"));
                }
            }
            if let Some(upd) = proto::first_bytes(&inner, 96) {
                // BlockPresetUpdated { fenderId=2, blockNames=3, dualCabsEnabled=4, cab1Names=5, cab2Names=6 }.
                let u = proto::parse(upd);
                let fid = proto::first_bytes(&u, 2)
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .unwrap_or_default();
                let names: Vec<String> = proto::all_bytes(&u, 3)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                let cab1: Vec<String> = proto::all_bytes(&u, 5)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                let cab2: Vec<String> = proto::all_bytes(&u, 6)
                    .iter()
                    .map(|x| String::from_utf8_lossy(x).into_owned())
                    .collect();
                found_updated += 1;
                report.push_str(&format!(
                    "\n=== BlockPresetUpdated (body {bi}) ===\n  fenderId={fid:?} names={names:?} cab1={cab1:?} cab2={cab2:?}\n"
                ));
            }
        }
        // UserMessage (top field 13): the UserIRListResponse.
        if let Some(um) = proto::first_bytes(&top, 13) {
            let inner = proto::parse(um);
            let inner_fields: Vec<u32> = inner.iter().map(|(f, _)| *f).collect();
            found_ir += 1;
            report.push_str(&format!(
                "\n=== UserMessage (body {bi}) ===\n  inner fields {inner_fields:?}\n"
            ));
            // Dump each inner length-delimited field as candidate records.
            for (f, v) in &inner {
                if let proto::Val::Bytes(bytes) = v {
                    let rec = proto::parse(bytes);
                    let rfields: Vec<u32> = rec.iter().map(|(rf, _)| *rf).collect();
                    let strs: Vec<String> = rec
                        .iter()
                        .filter_map(|(_, rv)| {
                            if let proto::Val::Bytes(s) = rv {
                                Some(String::from_utf8_lossy(s).into_owned())
                            } else {
                                None
                            }
                        })
                        .collect();
                    report.push_str(&format!(
                        "  field {f}: subfields {rfields:?} strings {strs:?} head {}\n",
                        hexn(bytes, 32)
                    ));
                }
            }
            if let Some(dir) = &out_dir {
                let p = format!("{dir}/userMessage_body{bi}.bin");
                std::fs::write(&p, um).map_err(|e| format!("write {p}: {e}"))?;
            }
        }
    }
    report.push_str(&format!(
        "\nsummary: AllBlockPresetsResponse×{found_blocks}, BlockPresetUpdated×{found_updated}, UserMessage×{found_ir}\n\
         NOTE: read-only; no device writes; raw dumps only when TMP_RE_OUT is set.\n"
    ));
    Ok(report)
}

/// Instrumented single-node ReplaceNode diagnostic (`probe --replace-debug SLOT FROM TO`):
/// loads the preset, settles, sends `replaceNode`, and DUMPS the device's reply (looking
/// for `nodeReplaced`/40 vs `connectionError`) so we can see why a replace does/doesn't
/// take. Then saves + re-reads to verify. Read+write (gated on explicit invocation).
pub fn probe_replace_debug(dev_slot: u32, from_id: &str, to_id: &str) -> Result<String, String> {
    let hexn = |b: &[u8], n: usize| {
        b.iter()
            .take(n)
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let list_index = dev_slot.saturating_sub(1);
    let mut report = String::new();

    // ── Connection 1: find the target node, then LOAD the preset (make it the
    //    device's active/edit preset) and DROP — the active preset persists across
    //    reconnects, so a fresh session then edits it the way Pro Control does
    //    (PC never re-loads; it edits whatever is active). TMP_REPLACE_ONECONN keeps
    //    the old single-connection load+replace for A/B.
    let oneconn = std::env::var("TMP_REPLACE_ONECONN").is_ok();
    let (group, node_id, cur_name) = {
        let mut s1 = Session::connect()?;
        s1.drain_until_quiet(250, 20)?;
        let raw = s1
            .read_slot_preset_json(dev_slot)?
            .ok_or("no field-8 JSON")?;
        let value = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)).ok_or("parse")?;
        let node = audiograph::roster(&value)
            .into_iter()
            .find(|(_, _, fid)| fid == from_id);
        let Some((group, node_id, _)) = node else {
            return Ok(format!("slot {dev_slot}: no node {from_id} found\n"));
        };
        let cur_name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        report.push_str(&format!(
            "target: slot {dev_slot} group={group} nodeId={node_id} name={cur_name:?} → {to_id}\n"
        ));
        s1.load_preset(list_index)?;
        s1.heartbeat()?;
        s1.pump_collect(900)?;
        (group, node_id, cur_name)
    };

    // ── Connection 2: fresh session on the now-active preset (NO reload), mirroring
    //    Pro Control. The seize was released by dropping s1; reconnect.
    let mut s = Session::connect()?;
    // CRITICAL: do NOT drain_until_quiet here — a multi-second silent drain lets the
    // session LAPSE, and the device then answers every structural request with an
    // empty connectionError (the live-controller-status gotcha; same reason
    // device_backup must heartbeat throughout). Instead hold a dense ~200 ms
    // heartbeat to KEEP live-controller status, the way Pro Control does.
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    if oneconn {
        s.load_preset(list_index)?;
        for _ in 0..4 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        report.push_str("conn2: ONECONN — reloaded preset in the edit connection\n");
    } else {
        report.push_str("conn2: fresh session, dense-heartbeat live-controller (no reload)\n");
    }

    // 3) Pro Control sends nodeJsonRequest(119){groupId,nodeId} immediately BEFORE
    //    replaceNode to enter the node-edit context (device replies nodeJsonResponse/
    //    120). Sent INSIDE the live heartbeat cadence. TMP_REPLACE_NO_NODEJSON skips it.
    let send_nodejson = std::env::var("TMP_REPLACE_NO_NODEJSON").is_err();
    if send_nodejson {
        s.clear_raw();
        s.send_and_collect(&proto::node_json_request(&group, &node_id), 200)?;
        s.heartbeat()?;
        s.pump_collect(200)?;
        let got_120 = s.push_bodies().iter().any(|b| {
            proto::first_bytes(&proto::parse(b), 2)
                .map(|pm| proto::first_bytes(&proto::parse(pm), 120).is_some())
                .unwrap_or(false)
        });
        report.push_str(&format!(
            "nodeJsonRequest(119) sent → nodeJsonResponse(120): {got_120}\n"
        ));
    } else {
        report.push_str("nodeJsonRequest(119): SKIPPED (TMP_REPLACE_NO_NODEJSON)\n");
    }

    // 4) send replaceNode INSIDE the live heartbeat cadence — framing per
    //    TMP_REPLACE_NOBATCH (Pro Control uses NO batchStatus).
    let nobatch = std::env::var("TMP_REPLACE_NOBATCH").is_ok();
    let batch = if nobatch { None } else { Some(11u64) };
    report.push_str(&format!(
        "framing: {}\n",
        if nobatch {
            "NO batchStatus"
        } else {
            "WITH batchStatus=11"
        }
    ));
    s.clear_raw();
    s.send_chunked_collect(&proto::replace_node(&group, &node_id, to_id, batch), 200)?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    // 4) dump every reply body (streams + push bodies) — TMS top + presetMessage
    //    inner fields, flagging nodeReplaced(40) and any NON-empty connectionError.
    let bodies: Vec<Vec<u8>> = s.push_bodies();
    report.push_str(&format!("  {} reply bodies\n", bodies.len()));
    for (bi, body) in bodies.iter().enumerate() {
        let top = proto::parse(body);
        let tf: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
        let mut inner_desc = String::new();
        if let Some(pm) = proto::first_bytes(&top, 2) {
            let inner = proto::parse(pm);
            let ifs: Vec<u32> = inner.iter().map(|(x, _)| *x).collect();
            inner_desc = format!(" presetMsg inner {ifs:?}");
            if proto::first_bytes(&inner, 40).is_some() {
                inner_desc.push_str("  ← nodeReplaced(40)!");
            }
            if proto::first_bytes(&inner, 3).is_some() {
                inner_desc.push_str("  ← currentPresetDataChanged(3)!");
            }
        }
        if let Some(cm) = proto::first_bytes(&top, 4) {
            let inner = proto::parse(cm);
            if let Some(err) = proto::first_bytes(&inner, 3) {
                let tag = if err.is_empty() {
                    "(empty/ack)"
                } else {
                    "NON-EMPTY"
                };
                inner_desc.push_str(&format!("  ← connectionError {tag} {}", hexn(err, 16)));
            }
        }
        report.push_str(&format!(
            "  reply {bi}: [{}B] top {tf:?}{inner_desc}\n",
            body.len()
        ));
    }

    // 5) optional save (TMP_REPLACE_SAVE) + re-read verify via the reliable field-8.
    //    Pro Control persists a structural edit as replaceNode → renameCurrentPreset
    //    (current name) → saveCurrentPreset(slot) — the rename may be load-bearing
    //    for persistence. TMP_REPLACE_NORENAME skips it for A/B.
    if std::env::var("TMP_REPLACE_SAVE").is_ok() {
        if std::env::var("TMP_REPLACE_NORENAME").is_err() && !cur_name.is_empty() {
            s.send_and_collect(&proto::rename_current_preset(&cur_name), 300)?;
            report.push_str(&format!("renameCurrentPreset({cur_name:?}) sent\n"));
        }
        s.save_current_preset(list_index)?;
        s.pump_collect(400)?;
        let vraw = s.read_slot_preset_json(dev_slot)?.unwrap_or_default();
        if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
            report.push_str(&format!(
                "verify after save: {from_id}×{}  {to_id}×{}\n",
                audiograph::count_nodes_with_id(&vval, from_id),
                audiograph::count_nodes_with_id(&vval, to_id)
            ));
        }
    } else {
        report.push_str("(not saved — set TMP_REPLACE_SAVE=1 to persist + verify)\n");
    }
    drop(s);
    Ok(report)
}

/// End-to-end test of Bulk Block Edit's live REPLACE (`probe --bulk-replace FROM TO
/// SLOTS [--commit]`): SLOTS are 1-based device slots. Without `--commit` it is a
/// READ-ONLY dry run — it loads each preset, prints its amp/block roster, and reports
/// which would change (matching `FROM`) vs skip (no match). With `--commit` it applies
/// the swap via the exact held-session production path (`replace_many_held`) + saves,
/// then re-reads to verify. This is the same code path the UI's `bulk_replace_live`
/// command serves.
pub fn probe_bulk_replace(
    from_id: &str,
    to_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --bulk-replace] {} → {} on slots {:?} ({})\n",
        from_id,
        to_id,
        device_slots,
        if commit {
            "COMMIT (device write + save)"
        } else {
            "DRY RUN (read-only)"
        }
    ));
    let repl = ReplArg::Model {
        fender_id: to_id.to_string(),
    };

    // Phase 1 — discovery: read every selected slot ONCE and build the target plans.
    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "\n  slot {dev_slot:03} (list {}) {:?}\n    {} matching {from_id}  → would {}\n",
            plan.list_index,
            plan.name,
            plan.targets.len(),
            if plan.targets.is_empty() {
                "SKIP (no matching block)"
            } else {
                "REPLACE"
            }
        ));
    }
    if !commit {
        report.push_str(
            "\nNOTE: dry run is read-only; --commit writes + saves each matching preset.\n",
        );
        return Ok(report);
    }
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Phase 2 — commit: the EXACT production path (`replace_many_held`: ONE held
    // session, re-armed per preset, with the active-preset + nodeReplaced guards).
    report.push_str("\n  COMMIT (held session):\n");
    let t_commit = std::time::Instant::now();
    let items = replace_many_held(&plans, &repl, true, |_item| {})?;
    for item in &items {
        let dev_slot = item.slot + 1;
        report.push_str(&format!(
            "    slot {dev_slot:03}: {} ({})\n",
            item.outcome, item.detail
        ));
    }
    report.push_str(&format!(
        "    (commit total {:.2}s)\n",
        t_commit.elapsed().as_secs_f64()
    ));
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Phase 3 — verify: one read-only session re-reads each slot and counts ids.
    report.push_str("\n  VERIFY (after save):\n");
    {
        let mut s = Session::connect()?;
        s.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = s.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}

/// Read-only full-roster dump for the perf experiments (`probe --roster <slots_csv>`):
/// every `(group, node_id, fenderId)` per slot via the proven field-8 read, so an
/// experiment can pick a FROM/TO that actually exists in the target presets.
pub fn probe_roster(device_slots: &[u32]) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!("[probe --roster] slots {device_slots:?}\n"));
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    for &dev_slot in device_slots {
        let Some(raw) = s.read_slot_preset_json(dev_slot)? else {
            report.push_str(&format!("  slot {dev_slot:03}: (no JSON)\n"));
            continue;
        };
        let Some(value) = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)) else {
            report.push_str(&format!("  slot {dev_slot:03}: (parse failed)\n"));
            continue;
        };
        let name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        report.push_str(&format!("  slot {dev_slot:03} {name:?}:\n"));
        for (group, node_id, fid) in audiograph::roster(&value) {
            report.push_str(&format!("    {group:<10} {node_id:<30} {fid}\n"));
        }
    }
    Ok(report)
}

/// Read-only raw field-8 preset-JSON dump for a single slot (`probe --slot-json <slot>`),
/// pretty-printed. Used to inspect node params the roster summary drops (e.g. a saved
/// dual-cab's `dualCabsEnabled`/`cab1`/`cab2` after a `replaceNodeWithBlock`).
pub fn probe_slot_json(device_slot: u32) -> Result<String, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let raw = s
        .read_slot_preset_json(device_slot)?
        .ok_or_else(|| format!("slot {device_slot}: field-8 read returned no JSON"))?;
    let text = String::from_utf8_lossy(&raw);
    match session::tolerant_parse_json(&text) {
        Some(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())),
        None => Ok(text.to_string()),
    }
}

/// E1 — held-session architecture experiment (`probe --replace-held <FROM> <TO> <slots_csv> [--commit]`).
/// Tests whether ONE session, re-armed after each load, accepts a CONFIRMED
/// `replaceNode` — vs the proven two-connection path (conn1 load → drop → conn2
/// re-attach). If it works, the per-preset cost collapses from ~8 s (two handshakes
/// plus a settle) to ~2 s (zero reopens). Discovery is done up front on a CLEAN session
/// (the proven field-8 path) so this isolates the one variable: does a held re-armed
/// session's edit get `nodeReplaced(40)` and persist? Per-preset timing is printed.
/// The same safety gate as production: a save only when the active preset matches the
/// target AND every replace is confirmed — so even a `--commit` run can't corrupt.
pub fn probe_replace_held(
    from_id: &str,
    to_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    use std::time::Instant;
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --replace-held] {from_id} → {to_id} on slots {device_slots:?} ({})\n",
        if commit {
            "COMMIT (held-session write + save)"
        } else {
            "DRY RUN (edit attempted, NOT saved)"
        }
    ));
    let repl = ReplArg::Model {
        fender_id: to_id.to_string(),
    };

    // Discovery up front on a CLEAN session (proven field-8 path): isolates the
    // experiment to the held-session EDIT, not field-3 read reliability.
    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "  slot {dev_slot:03} (list {}) {:?}: {} matching {from_id}\n",
            plan.list_index,
            plan.name,
            plan.targets.len()
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(600));

    // ── ONE held session via the PRODUCTION held path (`held_replace_one`), with
    //    per-preset timing. `save=commit`: a dry run attempts the edit (confirming
    //    `nodeReplaced`) but never persists — the field-8 verify below is the oracle. ──
    let t_conn = Instant::now();
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    report.push_str(&format!(
        "\n  HELD-SESSION (one connection, re-armed per preset; connect+warmup {:.2}s):\n",
        t_conn.elapsed().as_secs_f64()
    ));
    for plan in &plans {
        let dev_slot = plan.list_index + 1;
        let t0 = Instant::now();
        let item =
            held_replace_one(&mut s, plan, &repl, commit).unwrap_or_else(|e| BulkReplaceItem {
                slot: plan.list_index,
                name: plan.name.clone(),
                outcome: "error".to_string(),
                detail: e,
            });
        report.push_str(&format!(
            "    slot {dev_slot:03} {:?}: {} ({})  [{:.2}s]\n",
            plan.name,
            item.outcome,
            item.detail,
            t0.elapsed().as_secs_f64()
        ));
    }
    drop(s);

    // ── Verify (fresh CLEAN session, field-8) ──
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  VERIFY (after, field-8):\n");
    {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = v.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}

/// ADD a block to the device's CURRENT ACTIVE preset over USB — the live `insertNode`
/// (field 34) path, RE'd byte-exact from a Pro Control add-block capture
/// but never before confirmed on hardware. Mirrors the proven held-session replace
/// architecture (`held_replace_one`): identify the active preset, then on ONE held
/// session load+re-arm that slot, `insertNode` the new block, and (if `commit`) persist
/// in-place (`renameCurrentPreset` → `saveCurrentPreset`, song-link-safe). A DRY run
/// (no `--commit`) inserts, reports what the device replied + whether the block shows up
/// in a read-back, then RELOADS the preset to DISCARD the edit (nothing saved). The
/// active preset is resolved by: explicit `slot_override` (1-based device slot) →
/// `loaded_slot()` echo → unique active-name match in the list; ambiguous/unknown errors
/// out asking for `--slot`. Append by default (`after = None`), or insert after a given
/// FenderId. Group defaults to the primary guitar group "G1" (the capture's group).
pub fn probe_insert_active(
    fender_id: &str,
    group: Option<&str>,
    after: Option<&str>,
    slot_override: Option<u32>,
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --insert-active] add {fender_id} ({})\n",
        if commit {
            "COMMIT (insert + save in-place)"
        } else {
            "DRY RUN (insert + verify, NOT saved — reverted)"
        }
    ));

    // ── Identify the ACTIVE preset on a clean session ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?; // warmup harvests the connect-time field-22/field-3 pushes
    let active_name = s.active_preset_name();
    let loaded = s.loaded_slot(); // 0-based list index, or None (no load this session)
    let presets = s.list_my_presets().unwrap_or_default();

    // Resolve the 0-based list index (loaded_slot + PresetEntry.slot are both 0-based).
    let list_index = if let Some(dev) = slot_override {
        dev.saturating_sub(1)
    } else if let Some(idx) = loaded {
        idx
    } else if let Some(ref nm) = active_name {
        let matches: Vec<u32> = presets
            .iter()
            .filter(|p| &p.name == nm)
            .map(|p| p.slot)
            .collect();
        match matches.as_slice() {
            [one] => *one,
            [] => return Err(format!("active preset {nm:?} not found in the preset list — pass --slot <deviceSlot>")),
            _ => return Err(format!("active preset name {nm:?} is ambiguous ({} matching slots) — pass --slot <deviceSlot>", matches.len())),
        }
    } else {
        return Err("could not determine the active preset (no loaded-slot echo, no active name) — pass --slot <deviceSlot>".to_string());
    };
    let name = active_name
        .clone()
        .or_else(|| {
            presets
                .iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default();

    // Pick the target group: explicit, else the capture's "G1" if present, else the
    // first guitar group in the live roster (a non-existent group → device presetError,
    // which the safety gate rejects without saving).
    let target_group = match group {
        Some(g) => g.to_string(),
        None => {
            let mut groups: Vec<String> = s
                .current_preset_value()
                .ok()
                .map(|v| {
                    audiograph::roster(&v)
                        .into_iter()
                        .map(|(g, _, _)| g)
                        .collect()
                })
                .unwrap_or_default();
            groups.sort();
            groups.dedup();
            // sorted groups → the first "G*" is G1 when present, else the first guitar
            // group; default to "G1" (the capture's group) when none/none-guitar.
            groups
                .into_iter()
                .find(|g| g.starts_with('G'))
                .unwrap_or_else(|| "G1".to_string())
        }
    };
    report.push_str(&format!(
        "  active preset {name:?}  list_index={list_index} (device slot {})  group={target_group}  insert_after={after:?}\n",
        list_index + 1
    ));
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));

    // ── Held session: load+re-arm the active preset, insert, verify, save|revert ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let item = held_insert_one(
        &mut s,
        list_index,
        &name,
        &target_group,
        after,
        fender_id,
        commit,
    )
    .unwrap_or_else(|e| BulkReplaceItem {
        slot: list_index,
        name: name.clone(),
        outcome: "error".to_string(),
        detail: e,
    });
    report.push_str(&format!("  result: {} — {}\n", item.outcome, item.detail));
    drop(s);

    // ── Verify the PERSISTED state on a fresh clean session (field-8 slot read) ──
    std::thread::sleep(std::time::Duration::from_millis(600));
    let dev_slot = list_index + 1;
    let mut v = Session::connect()?;
    v.drain_until_quiet(250, 20)?;
    match v.read_slot_preset_json(dev_slot)? {
        Some(raw) => {
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&raw)) {
                let n = audiograph::count_nodes_with_id(&vval, fender_id);
                report.push_str(&format!(
                    "  VERIFY (field-8, persisted) slot {dev_slot:03}: {fender_id}×{n}  {}\n",
                    if commit {
                        "(expect ×1 after commit)"
                    } else {
                        "(expect ×0 after dry run)"
                    }
                ));
            } else {
                report.push_str(&format!(
                    "  VERIFY slot {dev_slot:03}: (re-read did not parse)\n"
                ));
            }
        }
        None => report.push_str(&format!(
            "  VERIFY slot {dev_slot:03}: (field-8 read returned no JSON)\n"
        )),
    }
    Ok(report)
}

/// The `ftsw` array of the SAVED preset at `device_slot` (1-based), via a field-8 slot
/// read on a fresh quiet session — the reliable post-save source.
fn read_slot_ftsw(device_slot: u32) -> Option<serde_json::Value> {
    let mut v = Session::connect().ok()?;
    v.drain_until_quiet(250, 20).ok()?;
    let raw = v.read_slot_preset_json(device_slot).ok()??;
    session::tolerant_parse_json(&String::from_utf8_lossy(&raw))?
        .get("ftsw")
        .cloned()
}

/// HW validation harness for the footswitch-assignment protocol (`probe --ftsw-validate
/// [switchIndex] [--commit]`). Answers, on the real unit, the open unknowns for the
/// block-acting-footswitch feature: (1) does `setFootswitchAssignment`(54) land on the
/// working copy and what (if anything) confirms it, (2) the `footswitchAddress`↔`ftsw`
/// array-index mapping, (3) the `valueType`/value round-trip, (4) what `swap` does, (5)
/// that `clearFootswitchAssignment`(55) removes it, and — with `--commit` — that
/// `saveCurrentPreset`(14) persists it (then RESTORES the preset to its original ftsw).
/// DRY by default: edits run on the working copy and are discarded by reloading the
/// preset; nothing is saved without `--commit`. Targets the ACTIVE preset.
pub fn probe_ftsw_validate(switch_override: Option<u32>, commit: bool) -> Result<String, String> {
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --ftsw-validate] {}\n",
        if commit {
            "COMMIT (tests persistence, then restores original ftsw)"
        } else {
            "DRY RUN (working-copy only; reverted, nothing saved)"
        }
    ));

    // Scan an ftsw array for our sentinel (customLabel == "PROBE") → (switch, func index).
    let find_probe = |ftsw: &serde_json::Value| -> Option<(usize, usize)> {
        for (i, sw) in ftsw.as_array()?.iter().enumerate() {
            if let Some(fns) = sw.as_array() {
                for (j, f) in fns.iter().enumerate() {
                    if f.get("customLabel").and_then(|v| v.as_str()) == Some("PROBE") {
                        return Some((i, j));
                    }
                }
            }
        }
        None
    };
    let func_count = |ftsw: &serde_json::Value, i: usize| -> usize {
        ftsw.as_array()
            .and_then(|a| a.get(i))
            .and_then(|s| s.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    };
    let func_at = |ftsw: &serde_json::Value, i: usize, j: usize| -> Option<serde_json::Value> {
        ftsw.as_array()?.get(i)?.as_array()?.get(j).cloned()
    };

    // ── Phase 0: resolve the active preset, read & print its ftsw ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let active_name = s.active_preset_name();
    let loaded = s.loaded_slot();
    let presets = s.list_my_presets().unwrap_or_default();
    let list_index = loaded
        .or_else(|| {
            active_name.as_ref().and_then(|nm| {
                let m: Vec<u32> = presets
                    .iter()
                    .filter(|p| &p.name == nm)
                    .map(|p| p.slot)
                    .collect();
                if m.len() == 1 {
                    Some(m[0])
                } else {
                    None
                }
            })
        })
        .ok_or("could not determine the active preset — load one on the unit first")?;
    let name = active_name
        .clone()
        .or_else(|| {
            presets
                .iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default();
    let graph = s.current_preset_value().ok();
    let ftsw0 = graph
        .as_ref()
        .and_then(|v| v.get("ftsw").cloned())
        .ok_or("no ftsw in the active preset's field-3 push")?;
    report.push_str(&format!(
        "  active {name:?} list_index={list_index} (device slot {})\n  current ftsw ({} switches):\n{}\n",
        list_index + 1,
        ftsw0.as_array().map(|a| a.len()).unwrap_or(0),
        serde_json::to_string_pretty(&ftsw0).unwrap_or_default()
    ));

    // Pick the target switch: explicit, else the first on-off switch.
    let target = switch_override
        .map(|x| x as usize)
        .or_else(|| {
            ftsw0.as_array()?.iter().position(|sw| {
                sw.as_array()
                    .and_then(|a| a.first())
                    .and_then(|f| f.get("func"))
                    .and_then(|v| v.as_str())
                    == Some("on-off")
            })
        })
        .ok_or("no on-off footswitch found — pass an explicit switch index")?;

    // The block that switch toggles, and a REAL param of it (first non-bypass), so the
    // device can't reject the functionJson on an unknown parameterId.
    let f0 = func_at(&ftsw0, target, 0).ok_or("target switch is empty")?;
    let n0 = f0
        .get("nodes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .ok_or("target on-off has no nodes[]")?;
    let grp = n0
        .get("groupId")
        .and_then(|v| v.as_str())
        .unwrap_or("G1")
        .to_string();
    let node = n0
        .get("nodeId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut param = "volume".to_string();
    if let Some(g) = &graph {
        audiograph::for_each_node(g, |obj| {
            if obj.get("nodeId").and_then(|v| v.as_str()) == Some(node.as_str()) {
                if let Some(ps) = obj.get("dspUnitParameters").and_then(|v| v.as_object()) {
                    if let Some(k) = ps
                        .keys()
                        .find(|k| !matches!(k.as_str(), "bypass" | "bypassType"))
                    {
                        param = k.clone();
                    }
                }
            }
        });
    }
    let target_index = func_count(&ftsw0, target) as u32; // next free slot (stack on the on-off)
    report.push_str(&format!(
        "  target switch [{target}] (on-off on {grp}/{node}); adding a 'param' fn on '{param}' at functionIndex {target_index}\n"
    ));
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Sentinel param function (customLabel "PROBE" = find marker; isActive but harmless —
    // it only changes audio when the footswitch is physically engaged, and we never save
    // it except under --commit, which restores).
    let func_json = serde_json::to_string(&serde_json::json!({
        "func": "param", "groupId": grp, "nodeId": node, "parameterId": param,
        "valueA": 0.123, "valueB": 0.456, "valueType": 2, "colorA": 3, "colorB": 0,
        "customLabel": "PROBE", "switchType": 0, "isActive": true
    }))
    .unwrap();

    // ── Phase 1: held session — set / swap / clear on the working copy ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(list_index)?;
    if !s.await_active_preset(&name, 20) {
        return Err(format!(
            "after load, active preset != {name:?} — aborting before any edit (safety)"
        ));
    }

    // UNKNOWN 1 — write + confirm gate. Try setter framing (no batchStatus) first.
    s.set_footswitch_assignment(target as u32, target_index, &func_json, false, None)?;
    report.push_str(&format!(
        "  [set no-batch] PresetMessage fields the device replied with: {:?}\n",
        s.seen_preset_fields()
    ));
    let mut after = s.live_ftsw();
    let mut framing = "no-batch";
    if after.as_ref().and_then(&find_probe).is_none() {
        s.set_footswitch_assignment(target as u32, target_index, &func_json, false, Some(11))?;
        report.push_str(&format!(
            "  [set batch=11] PresetMessage fields the device replied with: {:?}\n",
            s.seen_preset_fields()
        ));
        after = s.live_ftsw();
        framing = "batch=11";
    }
    let landed = after.as_ref().and_then(&find_probe);
    match (landed, &after) {
        (Some((si, fi)), Some(f)) => {
            report.push_str(&format!(
                "  ✓ LANDED via {framing}: PROBE at ftsw[{si}][{fi}] (sent footswitchAddress={target} functionIndex={target_index})\n"
            ));
            if si == target {
                report.push_str("    → footswitchAddress maps 1:1 to the ftsw array index\n");
            } else {
                report.push_str(&format!(
                    "    ⚠ footswitchAddress {target} landed at ftsw index {si} (offset {})\n",
                    si as i64 - target as i64
                ));
            }
            report.push_str(&format!(
                "    stored functionJson (valueType/value round-trip): {}\n",
                func_at(f, si, fi).map(|v| v.to_string()).unwrap_or_default()
            ));
        }
        _ => report
            .push_str("  ✗ did NOT land via either framing — setFootswitchAssignment had no working-copy effect\n"),
    }

    // UNKNOWN 2 — swap semantics (re-send same fn with swap=true, diff the stored object).
    if let Some((si, fi)) = landed {
        let before = func_at(&after.clone().unwrap(), si, fi);
        s.set_footswitch_assignment(target as u32, target_index, &func_json, true, None)?;
        let after_swap = s.live_ftsw();
        let after_fn = after_swap
            .as_ref()
            .and_then(&find_probe)
            .and_then(|(a, b)| func_at(after_swap.as_ref().unwrap(), a, b));
        report.push_str(&format!(
            "  [swap=true] before: {}\n              after:  {}\n",
            before.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
            after_fn
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into())
        ));
    }

    // UNKNOWN 3 — clear.
    s.clear_footswitch_assignment(target as u32, target_index)?;
    let after_clear = s.live_ftsw();
    let gone = after_clear.as_ref().and_then(&find_probe).is_none();
    report.push_str(&format!(
        "  [clear] {} — switch [{target}] func count: was {} now {}\n",
        if gone {
            "✓ PROBE removed"
        } else {
            "✗ PROBE still present"
        },
        func_count(&ftsw0, target),
        after_clear
            .as_ref()
            .map(|f| func_count(f, target))
            .unwrap_or(0)
    ));

    // ── Phase 2: persistence (commit only), then RESTORE ──
    if commit && landed.is_some() {
        s.set_footswitch_assignment(target as u32, target_index, &func_json, false, None)?;
        s.save_current_preset(list_index)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let persisted = read_slot_ftsw(list_index + 1);
        report.push_str(&format!(
            "  [commit] field-8 readback slot {:03}: PROBE persisted = {}\n",
            list_index + 1,
            persisted.as_ref().and_then(&find_probe).is_some()
        ));
        // RESTORE: clear the probe fn and re-save.
        let mut r = Session::connect()?;
        r.begin_live_edit()?;
        r.load_preset(list_index)?;
        if r.await_active_preset(&name, 20) {
            r.clear_footswitch_assignment(target as u32, target_index)?;
            r.save_current_preset(list_index)?;
        }
        drop(r);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let restored = read_slot_ftsw(list_index + 1);
        report.push_str(&format!(
            "  [restore] field-8 readback: PROBE gone = {}, switch [{target}] func count = {} (original {})\n",
            restored.as_ref().and_then(&find_probe).is_none(),
            restored.as_ref().map(|f| func_count(f, target)).unwrap_or(99),
            func_count(&ftsw0, target)
        ));
    } else {
        // DRY: discard the working copy by reloading the preset.
        let _ = s.load_preset(list_index);
        drop(s);
    }

    Ok(report)
}

/// Ordered FenderIds of the blocks in `group`, in signal order, from a parsed preset.
fn group_roster_fender_ids(v: &serde_json::Value, group: &str) -> Vec<String> {
    audiograph::roster(v)
        .into_iter()
        .filter(|(g, _, _)| g == group)
        .map(|(_, _, fid)| fid)
        .collect()
}

/// Live ordered FenderIds in `group` off the held session, retry-pumping because the
/// post-edit field-3 push can lag a single heartbeat window.
fn ordered_group(s: &mut Session, group: &str) -> Vec<String> {
    for _ in 0..10 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(250);
        if let Ok(v) = s.current_preset_value() {
            let roster = group_roster_fender_ids(&v, group);
            if !roster.is_empty() {
                return roster;
            }
        }
    }
    Vec::new()
}

/// Ordered FenderIds in `group` of the SAVED preset at `device_slot` (field-8 read, the
/// reliable post-save order source — the live working copy doesn't refresh after an edit
/// on a lean session).
fn field8_group_order(device_slot: u32, group: &str) -> Vec<String> {
    let read = || -> Result<Vec<String>, String> {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        let raw = v
            .read_slot_preset_json(device_slot)?
            .ok_or_else(|| "no field-8 JSON".to_string())?;
        let val = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
            .ok_or_else(|| "field-8 did not parse".to_string())?;
        Ok(group_roster_fender_ids(&val, group))
    };
    read().unwrap_or_default()
}

/// EMPIRICAL insert-placement mapping (`probe --insert-map <slot> <group> <fenderId>
/// [--before <id>] [--at-index <n>]`). Loads the slot on a held re-armed session, prints
/// the ORDERED group roster, sends ONE insert (field-34 before-anchor when `--before`,
/// else field-99 `insertNodeAtBlockIndex` when `--at-index`, else a bare append), prints
/// the ordered roster again, then either COMMITs (saves + field-8 readback) or REVERTs
/// (reload, live readback). Used to nail down what each wire op does to the in-group ORDER.
pub fn probe_insert_map(
    device_slot: u32,
    group: &str,
    fender_id: &str,
    before: Option<&str>,
    at_index: Option<u32>,
    commit: bool,
) -> Result<String, String> {
    let list_index = device_slot.saturating_sub(1);
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --insert-map] slot {device_slot:03} group={group} insert={fender_id} before={before:?} at_index={at_index:?} ({})\n",
        if commit { "COMMIT (saves, field-8 readback)" } else { "DRY (reverted, live readback)" }
    ));

    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let name = s
        .list_my_presets()
        .ok()
        .and_then(|ps| {
            ps.into_iter()
                .find(|p| p.slot == list_index)
                .map(|p| p.name)
        })
        .unwrap_or_default();

    // Load + re-arm the target preset (the held_insert_one preamble).
    s.clear_raw();
    s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
    if !s.active_matches(list_index, Some(&name)) {
        return Err(format!(
            "could not confirm slot {device_slot} loaded (loaded={:?}, active={:?})",
            s.loaded_slot(),
            s.active_preset_name()
        ));
    }

    report.push_str(&format!(
        "  BEFORE {group}: {:?}\n",
        ordered_group(&mut s, group)
    ));

    // ONE insert (retry once past the cold-first-edit silent drop, never past a reject).
    let do_insert = |s: &mut Session| match at_index {
        Some(idx) => s.insert_node_at_index(group, idx, fender_id),
        None => s.insert_node(group, before, fender_id),
    };
    let mut confirmed = do_insert(&mut s)?;
    if !confirmed && !s.saw_preset_error() {
        confirmed = do_insert(&mut s)?;
    }
    let seen = s.seen_preset_fields();
    let rejected = s.saw_preset_error();

    if rejected || !confirmed {
        report.push_str(&format!(
            "  REJECTED/UNCONFIRMED confirmed={confirmed} presetError={rejected} reply_fields={seen:?} — reverting\n"
        ));
        s.clear_raw();
        let _ = s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200);
        return Ok(report);
    }

    // COMMIT → identity-preserving save + field-8 readback (reliable); DRY → re-prompt a
    // best-effort live read, then revert by reloading.
    let after_order = if commit {
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(600));
        field8_group_order(device_slot, group)
    } else {
        let _ = s.send_and_collect(&proto::connection_request(), 80);
        let _ = s.send_and_collect(&proto::current_preset_data_request(2), 200);
        let order = ordered_group(&mut s, group);
        s.clear_raw();
        s.send_and_collect(&proto::load_preset(device_slot as u64, 1), 200)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        order
    };
    report.push_str(&format!(
        "  AFTER ({}) {group}: {after_order:?}\n  confirmed={confirmed} reply_fields={seen:?}\n",
        if commit { "field-8, saved" } else { "live" }
    ));
    Ok(report)
}

/// Insert one block into the preset at 0-based `list_index` on a HELD session — the
/// `held_replace_one` shape, with `insertNode` instead of `replaceNode`. Load + re-arm +
/// the same SAFETY gate (only proceed when the held session re-attached to the TARGET
/// preset). The insert gets a single RETRY on a silent DROP (the held path's cold first
/// structural edit after a fresh load can be dropped; an immediate retry lands it), but
/// NEVER on a `presetError` (a rejection — never saved). Saves only when the edit is
/// confirmed (nodeInserted) OR read back as present, and never on a presetError.
#[allow(clippy::too_many_arguments)]
fn held_insert_one(
    s: &mut Session,
    list_index: u32,
    name: &str,
    group: &str,
    after: Option<&str>,
    fender_id: &str,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    // LOAD on the held session + RE-ARM the edit context to the just-loaded preset.
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(name, 8); // pump for the fresh currentPresetInfoChanged
                                            // SAFETY — confirm the held session re-attached to the TARGET preset (active_matches
                                            // prefers the PresetLoaded slot echo, falling back to the active name) before editing.
    if !s.active_matches(list_index, Some(name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded (slot {:?} ≠ {list_index}, active {:?} ≠ {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }

    // INSERT — bare insertNode, with a single retry for the cold-first-edit DROP.
    let mut confirmed = s.insert_node(group, after, fender_id)?;
    let mut seen = s.seen_preset_fields();
    let mut rejected = s.saw_preset_error();
    if !confirmed && !rejected {
        confirmed = s.insert_node(group, after, fender_id)?;
        seen = s.seen_preset_fields();
        rejected = s.saw_preset_error();
    }

    // Content read-back: coax a fresh field-3 push, then check the block is present.
    s.heartbeat()?;
    s.pump_collect(250)?;
    let present = s
        .current_preset_value()
        .ok()
        .map(|v| {
            audiograph::roster(&v)
                .iter()
                .any(|(g, _, fid)| g == group && fid == fender_id)
        })
        .unwrap_or(false);
    let detail = format!(
        "nodeInserted(33)={confirmed} presetError={rejected} readback_present={present} reply_fields={seen:?}"
    );

    if rejected {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "rejected".to_string(),
            detail: format!("device sent presetError — NOT saved. {detail}"),
        });
    }
    if !confirmed && !present {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "unconfirmed".to_string(),
            detail: format!("no nodeInserted + block absent from read-back — NOT saved. {detail}"),
        });
    }

    if save {
        if !name.is_empty() {
            s.rename_current_preset(name)?;
        }
        s.save_current_preset(list_index)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "inserted+saved".to_string(),
            detail,
        })
    } else {
        // DRY: discard the live edit by reloading the same preset.
        s.clear_raw();
        s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
        s.heartbeat()?;
        s.pump_collect(120)?;
        Ok(BulkReplaceItem {
            slot: list_index,
            name: name.to_string(),
            outcome: "inserted (dry, reverted)".to_string(),
            detail,
        })
    }
}

/// E6 — replace a block with a saved DUAL-CAB block across slots (`probe
/// --bulk-replace-saved <FROM> <slots_csv> [--commit]`), via the proven
/// two-connection `replace_one_live` path with `ReplArg::Saved`
/// (`replaceNodeWithBlock`, field 100 +index). Auto-picks the first
/// `dual_cabs_enabled` saved block on the device. Validates the saved-block replace
/// end-to-end (previously only the stock-model/IR path was HW-validated). FROM should
/// name a cabinet node currently in the target presets (see `probe --roster`).
pub fn probe_bulk_replace_saved(
    from_id: &str,
    device_slots: &[u32],
    commit: bool,
) -> Result<String, String> {
    let mut report = String::new();
    // Resolve the saved dual-cab block (synchronous decode, same path as probe_saved_blocks).
    let saved = {
        let mut s =
            Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
        for _ in 0..4 {
            s.pump_collect(250)?;
        }
        let bodies = s.push_bodies();
        drop(s);
        let blob = find_block_presets_blob(&bodies)
            .ok_or_else(|| "device sent no allBlockPresetsResponse".to_string())?;
        parse_block_presets_map(&blob)?
    };
    let duals: Vec<&SavedBlock> = saved.iter().filter(|b| b.dual_cabs_enabled).collect();
    for d in &duals {
        report.push_str(&format!(
            "  dual-cab candidate: {} [idx {}] {:?} cab1={:?} cab2={:?}{}\n",
            d.fender_id,
            d.index,
            d.name,
            d.cab1_id,
            d.cab2_id,
            if d.cab1_id != d.cab2_id {
                " (TRUE dual)"
            } else {
                " (cab1==cab2)"
            }
        ));
    }
    // Prefer a GENUINE user-named dual-cab over an autogen default (cab1≠cab2 is the
    // strongest signal, but a user-named block is the realistic test even if the two
    // cabs share a model).
    let is_autogen =
        |b: &&SavedBlock| b.name.to_lowercase().contains("autogen") || b.name.is_empty();
    let dual = duals
        .iter()
        .find(|b| !is_autogen(b) && b.cab1_id != b.cab2_id)
        .or_else(|| duals.iter().find(|b| !is_autogen(b)))
        .or_else(|| duals.iter().find(|b| b.cab1_id != b.cab2_id))
        .or_else(|| duals.first())
        .copied()
        .ok_or_else(|| "no dual-cab saved block on the device".to_string())?;
    report.push_str(&format!(
        "[probe --bulk-replace-saved] {from_id} → saved dual-cab {} [idx {}] {:?} (cab1={:?} cab2={:?}) on slots {device_slots:?} ({})\n",
        dual.fender_id, dual.index, dual.name, dual.cab1_id, dual.cab2_id,
        if commit { "COMMIT (device write + save)" } else { "DRY RUN (read-only)" }
    ));
    let repl = ReplArg::Saved {
        fender_id: dual.fender_id.clone(),
        index: dual.index as u64,
    };
    let to_id = dual.fender_id.clone();
    std::thread::sleep(std::time::Duration::from_millis(600));

    let plans = discover_replace_plans(device_slots, from_id)?;
    for (plan, &dev_slot) in plans.iter().zip(device_slots) {
        report.push_str(&format!(
            "  slot {dev_slot:03} {:?}: {} matching {from_id}\n",
            plan.name,
            plan.targets.len()
        ));
    }
    if !commit {
        report.push_str("\nNOTE: dry run is read-only; --commit applies the saved dual-cab.\n");
        return Ok(report);
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  COMMIT (held session):\n");
    let items = replace_many_held(&plans, &repl, true, |_item| {})?;
    for item in &items {
        let dev_slot = item.slot + 1;
        report.push_str(&format!(
            "    slot {dev_slot:03}: {} ({})\n",
            item.outcome, item.detail
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    report.push_str("\n  VERIFY (field-8):\n");
    {
        let mut v = Session::connect()?;
        v.drain_until_quiet(250, 20)?;
        for &dev_slot in device_slots {
            let vraw = v.read_slot_preset_json(dev_slot)?.unwrap_or_default();
            if let Some(vval) = session::tolerant_parse_json(&String::from_utf8_lossy(&vraw)) {
                let now_from = audiograph::count_nodes_with_id(&vval, from_id);
                let now_to = audiograph::count_nodes_with_id(&vval, &to_id);
                report.push_str(&format!(
                    "    slot {dev_slot:03}: {from_id}×{now_from}  {to_id}×{now_to}\n"
                ));
            } else {
                report.push_str(&format!("    slot {dev_slot:03}: (re-read failed)\n"));
            }
        }
    }
    Ok(report)
}

/// E5 — discovery-source diff (`probe --discover-diff <FROM> <slots_csv>`). Builds the
/// replace-target set for each slot from BOTH the per-slot field-8 read
/// (`discover_replace_plans`) and the whole-library device backup (complete presetJson
/// roster), then diffs the `(group, node_id)` sets. For large N the backup (~22 s flat,
/// COMPLETE JSON) beats per-slot field-8 (~0.9 s × N) AND can't miss a block past the
/// field-8 truncation point — this proves the two sources agree (or surfaces a miss).
/// Read-only.
pub fn probe_discover_diff(from_id: &str, device_slots: &[u32]) -> Result<String, String> {
    use std::collections::BTreeSet;
    let mut report = String::new();
    report.push_str(&format!(
        "[probe --discover-diff] {from_id} on slots {device_slots:?}\n"
    ));

    // Source A — per-slot field-8 reads (the current production discovery).
    let t_f8 = std::time::Instant::now();
    let plans = discover_replace_plans(device_slots, from_id)?;
    let f8_secs = t_f8.elapsed().as_secs_f64();
    let f8: std::collections::HashMap<u32, BTreeSet<(String, String)>> = plans
        .iter()
        .map(|p| ((p.list_index + 1), p.targets.iter().cloned().collect()))
        .collect();

    std::thread::sleep(std::time::Duration::from_millis(600));

    // Source B — whole-library device backup (one stream, complete JSON).
    let t_bk = std::time::Instant::now();
    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_p| {})?;
    drop(s);
    let result = read_backup_archive(&blob)?;
    let bk_secs = t_bk.elapsed().as_secs_f64();
    let want: BTreeSet<u32> = device_slots.iter().copied().collect();
    let bk: std::collections::HashMap<u32, BTreeSet<(String, String)>> = result
        .presets
        .iter()
        .filter(|p| p.slot > 0 && want.contains(&(p.slot as u32)))
        .map(|p| {
            let set: BTreeSet<(String, String)> = p
                .blocks
                .iter()
                .filter(|b| b.fender_id == from_id)
                .map(|b| (b.group_id.clone(), b.node_id.clone()))
                .collect();
            (p.slot as u32, set)
        })
        .collect();

    report.push_str(&format!(
        "  field-8 discovery: {f8_secs:.2}s for {} slots\n  backup discovery:  {bk_secs:.2}s (whole library, {} presets)\n\n",
        device_slots.len(),
        result.presets.len()
    ));
    let mut agree = 0usize;
    let mut disagree = 0usize;
    for &slot in device_slots {
        let a = f8.get(&slot).cloned().unwrap_or_default();
        let b = bk.get(&slot).cloned().unwrap_or_default();
        if a == b {
            agree += 1;
            report.push_str(&format!(
                "    slot {slot:03}: AGREE ({} target(s))\n",
                a.len()
            ));
        } else {
            disagree += 1;
            let only_f8: Vec<_> = a.difference(&b).collect();
            let only_bk: Vec<_> = b.difference(&a).collect();
            report.push_str(&format!(
                "    slot {slot:03}: DIFFER  field-8-only={only_f8:?}  backup-only={only_bk:?}\n"
            ));
        }
    }
    report.push_str(&format!("\n  {agree} agree, {disagree} differ.\n"));
    Ok(report)
}

/// Validate the PRODUCTION saved-block decode path live (`probe --saved-blocks`):
/// the exact `list_saved_blocks` flow (RequestAllBlockPresets → decode →
/// `parse_block_presets_map`), printed as a summary. Read-only.
pub fn probe_saved_blocks() -> Result<String, String> {
    let mut s = Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
    for _ in 0..4 {
        s.pump_collect(250)?;
    }
    let bodies = s.push_bodies();
    drop(s);
    let blob = find_block_presets_blob(&bodies)
        .ok_or_else(|| "device sent no allBlockPresetsResponse".to_string())?;
    let blocks = parse_block_presets_map(&blob)?;
    let named: Vec<&SavedBlock> = blocks
        .iter()
        .filter(|b| !b.name.is_empty() && !b.name.to_lowercase().contains("autogen default"))
        .collect();
    let dual: Vec<&SavedBlock> = blocks.iter().filter(|b| b.dual_cabs_enabled).collect();
    let sample: String = named
        .iter()
        .take(12)
        .map(|b| {
            format!(
                "    {} [{}] {:?}{}",
                b.fender_id,
                b.index,
                b.name,
                if b.dual_cabs_enabled {
                    " (dual-cab)"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "[probe --saved-blocks] production decode of allBlockPresetsResponse.blockPresetsMap\n\
         total entries:   {}\n\
         user-named:      {}\n\
         dual-cabs:       {}\n\
         sample (named):\n{sample}\n\
         NOTE: read-only; this is the exact path list_saved_blocks serves the UI.\n",
        blocks.len(),
        named.len(),
        dual.len(),
    ))
}

/// Fast full-library read via the device backup (`probe --device-backup`): one
/// `BackupRequest` streams a GNU-tar + LZ4-frame archive of `/data`, decoded in
/// memory (see [`read_backup_archive`]). Replaces ~500 per-preset round-trips with
/// one stream + one local SQLite read, and yields COMPLETE presets (not the
/// USB-partial slot-read). Prints live transfer progress (the same `BackupProgress`
/// that drives the UI's `tmp://backup-progress` bar).
pub fn probe_device_backup() -> Result<String, String> {
    let mut s = Session::connect()?;
    let mut last_bucket = -1i32;
    let (blob, stats) = s.device_backup(60, |p| {
        if p.phase == "building" {
            eprintln!(
                "[probe]   building… (build_size={}, build_ticks={})",
                p.build_size, p.build_ticks
            );
        } else {
            let bucket = (p.percent as i32 / 10) * 10;
            if bucket > last_bucket {
                last_bucket = bucket;
                eprintln!(
                    "[probe]   streaming {:>3.0}%  {}/{} chunks  {} KiB",
                    p.percent,
                    p.received,
                    p.total,
                    p.bytes / 1024
                );
            }
        }
    })?;
    drop(s); // release the HID seize before host-side work

    // Diagnostic escape hatch (OFF by default → nothing persists): dump the raw
    // streamed archive when TMP_BACKUP_RAW=<path> is set.
    if let Ok(path) = std::env::var("TMP_BACKUP_RAW") {
        std::fs::write(&path, &blob).map_err(|e| format!("dump raw: {e}"))?;
        eprintln!("[probe] wrote raw archive ({} B) to {path}", blob.len());
    }

    let crc_ok = stats.bytes_assembled == stats.num_bytes as usize;
    let magic: Vec<String> = blob.iter().take(16).map(|b| format!("{b:02x}")).collect();
    let result = read_backup_archive(&blob)?;

    let throughput = if stats.elapsed_secs > 0.0 {
        stats.bytes_assembled as f64 / 1024.0 / stats.elapsed_secs
    } else {
        0.0
    };
    let members: String = result
        .members
        .iter()
        .map(|(p, sz)| format!("    {p} ({sz} B)"))
        .collect::<Vec<_>>()
        .join("\n");
    let sample: String = result
        .presets
        .iter()
        .take(5)
        .map(|p| {
            let names: Vec<&str> = p.scenes.iter().map(|s| s.name.as_str()).collect();
            let count = if p.scene_count < 0 {
                "?".to_string()
            } else {
                p.scene_count.to_string()
            };
            format!(
                "    slot {}: {:?} ({count} scene(s): {})",
                p.slot,
                p.name,
                names.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let scenes_known = result.presets.iter().filter(|p| p.scene_count >= 0).count();

    Ok(format!(
        "[probe --device-backup] full library via BackupRequest (one stream, no per-preset round-trips)\n\
         transfer:\n\
         \telapsed        {:.2} s  (first chunk at {:.2} s = handshake + device build)\n\
         \tchunks         {}/{} received\n\
         \tarchive bytes  {} (declared numBytes={}, crc=0x{:08x}, integrity {})\n\
         \tthroughput     {throughput:.0} KiB/s   archive magic [{}]\n\
         \tbuild progress device-reported: size={} ticks={} ({})\n\
         \tstate log      {:?}\n\
         archive members ({}):\n{members}\n\
         normalDb.db3: {} bytes decompressed\n\
         UserPresets rows: {} total ({} non-empty named)\n\
         total scenes: {} across {scenes_known} presets (scene count via {})\n\
         sample:\n{sample}\n\
         NOTE: archive held in RAM only; temp DB deleted on exit (no backup persisted).\n",
        stats.elapsed_secs,
        stats.first_chunk_secs,
        stats.chunks_received,
        stats.num_chunks,
        stats.bytes_assembled,
        stats.num_bytes,
        stats.crc,
        if crc_ok { "ok" } else { "SIZE MISMATCH" },
        magic.join(" "),
        stats.build_size,
        stats.build_ticks,
        if stats.build_size > 0 { "determinate" } else { "not reported → use indeterminate spinner for build phase" },
        stats.state_log,
        result.members.len(),
        result.db_bytes,
        result.total_rows,
        result.presets.len(),
        result.total_scenes(),
        result.scene_mode,
    ))
}

/// Read the full preset/scene library via the device backup (one `BackupRequest` →
/// tar.lz4 stream → in-memory decode). Emits `tmp://backup-progress`
/// ([`session::BackupProgress`]) as the transfer advances so the UI can drive a
/// determinate progress bar (the chunk percentage is exact). Read-only on the
/// device; nothing persists (archive in RAM, temp DB deleted). Routed through
/// `with_released_seize` so it serializes via `DEVICE_OP_LOCK` (pausing the monitor)
/// like every device op.
#[tauri::command]
async fn read_library_via_backup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<BackupReadResult, String> {
    // Offline e2e: decode a built fixture blob (LZ4-frame(tar(normalDb.db3)), the exact
    // device shape) through the SAME `read_backup_archive` path instead of streaming the
    // bulk backup over USB — faking that multi-chunk wire stream buys no fidelity the
    // real decode (lz4 → tar → sqlite → audiograph) doesn't already exercise.
    #[cfg(feature = "e2e")]
    if let Ok(path) = std::env::var("TMP_E2E_BACKUP_FIXTURE") {
        let blob = std::fs::read(&path).map_err(|e| format!("e2e backup fixture {path}: {e}"))?;
        return read_backup_archive(&blob);
    }
    use tauri::Emitter;
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let (blob, _stats) = s.device_backup(60, move |p| {
            let _ = app.emit("tmp://backup-progress", p);
        })?;
        drop(s); // release the HID seize before host-side decode
        read_backup_archive(&blob)
    })
    .await
}

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
fn find_block_presets_blob(bodies: &[Vec<u8>]) -> Option<Vec<u8>> {
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
fn find_user_irs(bodies: &[Vec<u8>]) -> Vec<UserIr> {
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

/// List the user's saved blocks (`RequestAllBlockPresets` → `AllBlockPresetsResponse`).
/// Instant (rides one handshake burst, no 22 s backup). Read-only. Powers the Bulk
/// Block Edit Step-3 "Your saved blocks" palette (incl. saved dual-cabs).
///
/// The in-burst `135` read can transiently MISS on a cold/first connect (the device
/// doesn't answer that round — HW-observed: 1st cold read returned no `136`,
/// the next two succeeded). So retry independent fresh reads until the response lands
/// (mirrors [`read_song_list`]'s fail-closed retry) rather than spuriously surfacing an
/// empty saved-block palette. Each attempt early-exits the moment the `136` arrives.
#[tauri::command]
async fn list_saved_blocks(state: State<'_, AppState>) -> Result<Vec<SavedBlock>, String> {
    with_released_seize(state.session.clone(), move || {
        for _attempt in 0..4 {
            let mut s =
                Session::connect_with_burst_request(&proto::request_all_block_presets(Some(2)))?;
            for _ in 0..8 {
                if let Some(blob) = find_block_presets_blob(&s.push_bodies()) {
                    return parse_block_presets_map(&blob);
                }
                s.pump_collect(250)?;
            }
        }
        Err("device sent no allBlockPresetsResponse after retries".to_string())
    })
    .await
}

/// List the user's impulse responses (`UserIRListRequest` → `UserIRListResponse`).
/// Instant + read-only. Powers the Bulk Block Edit Step-3 "Your impulse responses"
/// palette. Returns an empty list when the device has no user IRs loaded.
#[tauri::command]
async fn list_user_irs(state: State<'_, AppState>) -> Result<Vec<UserIr>, String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?; // handshake already issues userIRListRequest(batch 2)
                                         // A standalone re-send + a few pump windows in case the burst reply was missed.
        s.heartbeat()?;
        s.pump_collect(80)?;
        s.send_and_collect(&proto::userir_field2(2), 500)?;
        for _ in 0..5 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        let bodies = s.push_bodies();
        drop(s);
        Ok(find_user_irs(&bodies))
    })
    .await
}

/// The per-node edit Bulk Block Edit applies on the held session (matches the TS union
/// on the `kind` tag; nested keys arrive camelCase, so `fenderId` is renamed).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplArg {
    /// Stock model — `replaceNode` fills the model's default params.
    Model {
        #[serde(rename = "fenderId")]
        fender_id: String,
    },
    /// User IR — `replaceNode` to `ACD_UserIRTMS`, then a string `changeParameter` sets
    /// the new node's `file` param to the chosen IR (verified by re-read before save).
    Ir {
        #[serde(rename = "fenderId")]
        fender_id: String,
        file: String,
    },
    /// Saved block (user block / dual cab) — `replaceNodeWithBlock` by the device
    /// library `index`.
    Saved {
        #[serde(rename = "fenderId")]
        fender_id: String,
        index: u64,
    },
    /// Remove the block from the chain — `removeNode` (the device re-links).
    Remove,
}

/// One preset's outcome from a live bulk-replace run (streamed to the UI per preset).
#[derive(Debug, Clone, Serialize)]
pub struct BulkReplaceItem {
    /// 0-based list index of the preset (the UI keys its rows by this).
    pub slot: u32,
    pub name: String,
    /// `"updated"` | `"skipped"` | `"error"`.
    pub outcome: String,
    pub detail: String,
}

/// The block content a copy [`CopyOp`] applies — the SAME three "with a block"
/// variants [`ReplArg`] supports (no `Remove`; that is a [`CopyOp::Remove`] op).
/// Nested keys arrive camelCase, so `fenderId` is renamed.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyRepl {
    /// Stock model — `replaceNode` / `insertNode` fills the model's default params.
    Model {
        #[serde(rename = "fenderId")]
        fender_id: String,
    },
    /// User IR — `replaceNode`/`insert` to `ACD_UserIRTMS`, then a string
    /// `changeParameter` points the new node's `file` param at the chosen IR.
    Ir {
        #[serde(rename = "fenderId")]
        fender_id: String,
        file: String,
    },
    /// Saved block (user block / dual cab) — `replaceNodeWithBlock` by the device
    /// library `index`.
    Saved {
        #[serde(rename = "fenderId")]
        fender_id: String,
        index: u64,
    },
}

impl CopyRepl {
    /// The fender id this content resolves to (the model id, the IR placeholder
    /// `ACD_UserIRTMS`, or the saved block's id).
    fn insert_fender_id(&self) -> &str {
        match self {
            CopyRepl::Model { fender_id } => fender_id,
            CopyRepl::Ir { .. } => "ACD_UserIRTMS",
            CopyRepl::Saved { fender_id, .. } => fender_id,
        }
    }
}

/// One ordered structural op the "Copy blocks between presets" feature applies to a
/// target preset. Tagged on `kind`; nested ids arrive camelCase.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyOp {
    /// Replace the block `node_id` in `group` with `repl` — `replaceNode` /
    /// `replaceNodeWithBlock` / `replaceNode`→`ACD_UserIRTMS`+file, per the variant.
    Replace {
        group: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        repl: CopyRepl,
    },
    /// Insert `repl` into `group` via field-34 `insert_node`. `before_fender_id` is the
    /// block to insert AHEAD of (the device's field-2 inserts BEFORE the referenced node,
    /// HW-verified fw 1.8.45); `None` appends at the group end. `diffToOps` sets it to the
    /// inserted block's in-array successor's FenderId, or `None` when it's last.
    Insert {
        group: String,
        #[serde(rename = "beforeFenderId")]
        before_fender_id: Option<String>,
        repl: CopyRepl,
    },
    /// Remove the block `node_id` from `group` — `removeNode` (the device re-links).
    Remove {
        group: String,
        #[serde(rename = "nodeId")]
        node_id: String,
    },
}

/// One target preset for a [`copy_apply`] run: its 0-based `list_index`, display
/// `name` (for the identity-preserving rename-before-save), and the ORDERED list of
/// structural `ops` to apply before saving it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopyJob {
    #[serde(rename = "listIndex")]
    pub list_index: u32,
    pub name: String,
    pub ops: Vec<CopyOp>,
}

/// One preset's outcome from a [`copy_apply`] run (streamed per preset). Like
/// [`BulkReplaceItem`] (`slot`/`name`/`outcome`/`detail`) plus the post-save signal
/// `graph` read back off the held session, so the Copy view can patch its cached library
/// in place (no ~22 s re-scan) after a write. `graph` is `None` when the preset wasn't
/// saved or its graph couldn't be read back.
#[derive(Debug, Clone, Serialize)]
pub struct CopyApplyItem {
    pub slot: u32,
    pub name: String,
    pub outcome: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<session::ActiveGraph>,
}

/// Cooperative cancel for [`bulk_replace_live`] — set by `cancel_bulk_replace` (the
/// wizard's Stop), checked between presets so the held-session sweep stops WRITING the
/// remaining presets (not just hiding them in the UI). Presets already saved stay
/// changed; the rest are left untouched.
static BULK_REPLACE_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`bulk_replace_live`] sweep after the current preset. Lightweight
/// (just sets the flag) so it does NOT take the device-op lock — it must run while the
/// sweep holds it.
#[tauri::command]
fn cancel_bulk_replace() {
    BULK_REPLACE_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Bulk Block Edit — apply one per-node edit (replace with a model/saved block, or
/// remove) across the selected presets, live, via the device's own structural edit
/// (`replaceNode` / `replaceNodeWithBlock` / `removeNode`). This is the faithful path:
/// the device fills the new block's correct params / re-links the chain on removal
/// (there is no host-side default-param catalog — see the feature's design).
/// DEVICE WRITE — gated behind the UI's backup acknowledgment. Streams a
/// `BulkReplaceItem` per preset; one preset's failure degrades to an `error` row and
/// the sweep continues. Stop (`cancel_bulk_replace`) halts before the next preset. Each
/// preset is loaded, edited in place (identity preserved), and saved only when `save`.
#[tauri::command]
async fn bulk_replace_live(
    state: State<'_, AppState>,
    selection: Vec<u32>,
    from_id: String,
    repl: ReplArg,
    save: bool,
    on_result: tauri::ipc::Channel<BulkReplaceItem>,
) -> Result<Vec<BulkReplaceItem>, String> {
    BULK_REPLACE_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        // Discovery: per-slot field-8 reads for small selections, ONE whole-library
        // device backup once the selection crosses the measured break-even (E5).
        let device_slots: Vec<u32> = selection.iter().map(|i| i + 1).collect();
        let plans = discover_replace_plans_smart(&device_slots, &from_id)?;
        // Edit on ONE HELD session (E1): connect once, warm the live-controller
        // heartbeat once, then edit every preset in place with no reopens and no
        // inter-preset settles (~3.4 s/preset vs ~8 s for the two-connection path).
        // If the held session can't be ESTABLISHED (connect/warmup), fall back to the
        // proven per-preset two-connection loop BEFORE any result is emitted (so no row
        // is double-sent). A per-preset failure inside the held path is an `error` row,
        // not a fallback trigger — the session stays alive for the remaining presets.
        match replace_many_held(&plans, &repl, save, |item| {
            let _ = on_result.send(item.clone());
        }) {
            Ok(out) => Ok(out),
            Err(e) => {
                log::warn!(
                    "[bulk-replace] held-session path failed to establish ({e}); falling back to two-connection"
                );
                std::thread::sleep(std::time::Duration::from_millis(400));
                let mut out = Vec::new();
                let total = plans.len();
                for (i, plan) in plans.iter().enumerate() {
                    if BULK_REPLACE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                        break; // Stop pressed — leave the remaining presets untouched.
                    }
                    let item = replace_one_live(plan, &repl, save).unwrap_or_else(|e| BulkReplaceItem {
                        slot: plan.list_index,
                        name: plan.name.clone(),
                        outcome: "error".to_string(),
                        detail: e,
                    });
                    let _ = on_result.send(item.clone());
                    out.push(item);
                    if i + 1 < total {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                    }
                }
                Ok(out)
            }
        }
    })
    .await
}

/// One preset's matching target nodes, discovered up front (field-8 read) so the
/// edit hot path never does a fragile post-reconnect read.
struct ReplacePlan {
    list_index: u32,
    name: String,
    /// `(group, node_id)` of every node matching the requested `from_id`.
    targets: Vec<(String, String)>,
}

/// Read the selected slots ONCE on a single session and build the per-preset target
/// list. Field-8 reads are reliable on a clean session but flaky right after a
/// reconnect, so all reads are batched here, away from the edit reconnect churn.
fn discover_replace_plans(device_slots: &[u32], from_id: &str) -> Result<Vec<ReplacePlan>, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let mut plans = Vec::new();
    for &dev_slot in device_slots {
        let raw = s
            .read_slot_preset_json(dev_slot)?
            .ok_or_else(|| format!("slot {dev_slot}: field-8 read returned no JSON"))?;
        let value = session::tolerant_parse_json(&String::from_utf8_lossy(&raw))
            .ok_or_else(|| format!("slot {dev_slot}: preset JSON did not parse"))?;
        let name = value
            .pointer("/info/displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let targets: Vec<(String, String)> = audiograph::roster(&value)
            .into_iter()
            .filter(|(_, _, fid)| fid == from_id)
            .map(|(group, node_id, _)| (group, node_id))
            .collect();
        plans.push(ReplacePlan {
            list_index: dev_slot.saturating_sub(1),
            name,
            targets,
        });
    }
    Ok(plans)
}

/// Selection size at which whole-library backup discovery beats per-slot field-8 reads
/// (E5, HW-measured: field-8 ≈ 1.2 s/slot, backup ≈ 24 s flat → break-even
/// ~20 slots). At/above this, the backup is also strictly more correct — it reads the
/// COMPLETE preset JSON, so it can't miss a block past the field-8 truncation point.
const BACKUP_DISCOVERY_THRESHOLD: usize = 22;

/// Build replace plans, choosing the discovery source by selection size (E5).
fn discover_replace_plans_smart(
    device_slots: &[u32],
    from_id: &str,
) -> Result<Vec<ReplacePlan>, String> {
    if device_slots.len() < BACKUP_DISCOVERY_THRESHOLD {
        discover_replace_plans(device_slots, from_id)
    } else {
        discover_replace_plans_via_backup(device_slots, from_id)
    }
}

/// Build replace plans from ONE whole-library device backup (E5) — the complete preset
/// JSON, so no field-8 truncation can hide a target block. Slots not present in the
/// backup (empty) get an empty plan (skipped downstream).
fn discover_replace_plans_via_backup(
    device_slots: &[u32],
    from_id: &str,
) -> Result<Vec<ReplacePlan>, String> {
    let mut s = Session::connect()?;
    let (blob, _stats) = s.device_backup(60, |_p| {})?;
    drop(s);
    let result = read_backup_archive(&blob)?;
    let by_slot: std::collections::HashMap<u32, &BackupPresetRow> = result
        .presets
        .iter()
        .filter(|p| p.slot > 0)
        .map(|p| (p.slot as u32, p))
        .collect();
    let plans = device_slots
        .iter()
        .map(|&dev_slot| {
            let (name, targets) = match by_slot.get(&dev_slot) {
                Some(row) => {
                    let targets = row
                        .blocks
                        .iter()
                        .filter(|b| b.fender_id == from_id)
                        .map(|b| (b.group_id.clone(), b.node_id.clone()))
                        .collect();
                    (row.name.clone(), targets)
                }
                None => (String::new(), Vec::new()),
            };
            ReplacePlan {
                list_index: dev_slot.saturating_sub(1),
                name,
                targets,
            }
        })
        .collect();
    Ok(plans)
}

// ─── blockcaps guard plumbing ──────────────────────────────────────────────────
//
// Shared by every LIVE structural-edit writer (`replace_one_live`, `held_replace_one`,
// `copy_apply_one`): the device does NOT enforce the 5 firmware block-count caps (see
// `blockcaps.rs`'s module docs — two RE passes confirmed the cap logic is entirely
// client-side in `tone-master-stomp-client` and can never emit a `presetError` for an
// over-cap edit), so this Rust guard is the SOLE enforcement before a save. Every
// writer: (1) reads the PRE-edit roster right after load+attach-confirm (before its
// first structural edit), fail-closed if it can't be read; (2) checks each op's delta
// against the running counts BEFORE emitting it; (3) on a violation, aborts the WHOLE
// target with an `error` outcome carrying the reason — never a partial, unvalidated
// save.

/// Read the just-loaded preset's pre-edit node roster (+ its cap counts) off the held
/// session, retrying (mirrors [`ordered_group`]'s pattern — the post-load field-3 push
/// can lag a heartbeat window). Fail-closed: `Err` propagates as the caller's `error`
/// outcome rather than ever letting an edit through unvalidated.
fn blockcaps_pre_edit_roster(
    s: &mut Session,
) -> Result<(Vec<blockcaps::RosterEntry>, blockcaps::Counts), String> {
    for i in 0..10 {
        if i > 0 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(250);
        }
        if let Ok(v) = s.current_preset_value() {
            let roster = blockcaps::roster_from_preset(&v);
            if !roster.is_empty() {
                let counts = blockcaps::counts(&roster);
                return Ok((roster, counts));
            }
        }
    }
    Err(
        "blockcaps: could not read the pre-edit node roster after load — refusing to \
         emit an unvalidated edit (the device does not enforce the block-count caps, \
         so an unreadable roster must not save)"
            .to_string(),
    )
}

/// What a Replace/Remove op's `(group, node_id)` target is replacing/removing, off the
/// pre-edit roster (an Insert has no target; it only adds). Linear scan — rosters are
/// tens of nodes; a node inserted by an EARLIER op in the same job resolves to `None`
/// (its contribution is simply never freed — the strict, safe direction).
fn blockcaps_replaced<'a>(
    roster: &'a [blockcaps::RosterEntry],
    group: &str,
    node_id: &str,
) -> Option<(&'a str, bool)> {
    roster
        .iter()
        .find(|e| e.group == group && e.node_id == node_id)
        .map(|e| (e.fender_id.as_str(), e.dual_cab))
}

/// Cap-check ONE candidate insert/replace against the running pre-edit `counts`.
/// `candidate_id = None` is a bare Remove — no candidate to check; removing a node can
/// only shrink counts, never push a cap over its max. `replaced` is the (fender_id,
/// dual_cab) of the node a Replace targets, from [`blockcaps_replaced`] (`None` for
/// an Insert/Remove). `cand_dual_cab` is always `false`: the wire format
/// (`CopyRepl`/`ReplArg`) never carries a candidate's dual-cab flag — a fresh Model/Ir
/// insert is never dual by construction; a `Saved` (device-library) block COULD be
/// dual, but that's only knowable by reading the library entry, which the copy/replace
/// wire protocol doesn't surface. Known residual gap (documented, not silently assumed).
fn blockcaps_check(
    counts: &blockcaps::Counts,
    candidate_id: Option<&str>,
    is_replace: bool,
    replaced: Option<(&str, bool)>,
) -> Result<(), String> {
    let Some(candidate_id) = candidate_id else {
        return Ok(());
    };
    let (replaced_id, replaced_dual) = replaced.map_or((None, false), |(id, d)| (Some(id), d));
    blockcaps::check_op(
        counts,
        candidate_id,
        replaced_id,
        is_replace,
        false,
        replaced_dual,
    )
    .map_err(|reason| reason.to_string())
}

/// Roll the running `counts` forward after a CONFIRMED op, so the NEXT op in the same
/// job/plan is checked against the up-to-date state (a job can carry multiple ops that
/// each affect the same caps — e.g. two conv inserts in one job must be checked
/// cumulatively, not each against the stale pre-edit snapshot).
fn blockcaps_advance(
    counts: &mut blockcaps::Counts,
    candidate_id: Option<&str>,
    replaced: Option<(&str, bool)>,
) {
    if let Some(id) = candidate_id {
        counts.add(id, false); // see `blockcaps_check`'s doc on `cand_dual_cab`.
    }
    if let Some((id, dual)) = replaced {
        counts.remove(id, dual);
    }
}

/// The FenderId a [`ReplArg`] would insert, or `None` for `Remove` (no candidate to
/// cap-check — see [`blockcaps_check`]).
fn repl_arg_fender_id(repl: &ReplArg) -> Option<&str> {
    match repl {
        ReplArg::Model { fender_id }
        | ReplArg::Ir { fender_id, .. }
        | ReplArg::Saved { fender_id, .. } => Some(fender_id.as_str()),
        ReplArg::Remove => None,
    }
}

/// Replace every pre-discovered target node in ONE preset and (if `save`) persist it.
/// Owns TWO connections — the proven fw-1.8.45 path: conn1 loads the preset (making it
/// the device's active/edit preset), then conn2's fresh handshake RE-ATTACHES to that
/// active preset before editing it. (Load + edit in one session leaves the session
/// attached to the pre-load preset, so the device rejects the edit.) No field-8 read
/// here — `plan.targets` were read up front by [`discover_replace_plans`].
///
/// SAFETY (a wrong-content save corrupted a slot): the save is gated on
/// (1) conn2's active preset matching the target name (the load took) AND (2) EVERY
/// `replaceNode` being confirmed by `nodeReplaced(40)` — a `presetError(53)`/no-ack
/// aborts WITHOUT saving, so a misattached or rejected edit can never persist the
/// wrong audioGraph. (3) blockcaps guard — see the module docs above: EVERY target's
/// delta against the 5 firmware block-count caps is checked against the PRE-edit
/// roster before it's emitted, since the device enforces none of them.
fn replace_one_live(
    plan: &ReplacePlan,
    repl: &ReplArg,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    let list_index = plan.list_index;
    let name = plan.name.clone();
    if plan.targets.is_empty() {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no matching block".to_string(),
        });
    }

    // ── conn1: LOAD the preset (make it active). Fire-and-forget; no read. ──
    {
        let mut s1 = Session::connect()?;
        s1.load_preset(list_index)?;
        s1.heartbeat()?;
        s1.pump_collect(500)?;
    }
    // Quiet settle before reconnecting — avoids the HID open-lockout/congestion a rapid
    // drop→reopen triggers.
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── conn2: fresh handshake re-attaches to the now-active preset; edit it ──
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    // SAFETY 1 — confirm conn2 is on the TARGET preset (the load took). Prefer the
    // `PresetLoaded` slot echo (identity); fall back to the active-preset NAME. If
    // NEITHER confirms (empty/duplicate name + no slot echo), SKIP — editing+saving an
    // unverified preset would corrupt this slot.
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }
    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(&mut s)?;
    let candidate_id = repl_arg_fender_id(repl);

    // SAFETY 2 — only persist if EVERY replace is confirmed (nodeReplaced/40).
    let mut applied = 0usize;
    for (group, node_id) in &plan.targets {
        let replaced = blockcaps_replaced(&roster, group, node_id);
        if let Err(reason) = blockcaps_check(&counts, candidate_id, true, replaced) {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "blocked by block-count cap ({reason}) replacing {group}/{node_id} — NOT saved"
                ),
            });
        }
        let confirmed = match repl {
            ReplArg::Saved { fender_id, index } => {
                s.replace_node_with_block(group, node_id, fender_id, *index)?
            }
            ReplArg::Model { fender_id } => s.replace_node(group, node_id, fender_id)?,
            ReplArg::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file)?,
            ReplArg::Remove => s.remove_node(group, node_id)?,
        };
        if !confirmed {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!("device rejected replace of {group}/{node_id} (presetError / no nodeReplaced) — NOT saved"),
            });
        }
        blockcaps_advance(&mut counts, candidate_id, replaced);
        applied += 1;
    }
    if save {
        // Pro Control persists a structural edit as renameCurrentPreset(current name)
        // → saveCurrentPreset(slot); the rename preserves the preset's name/identity.
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    Ok(BulkReplaceItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{applied} block(s)"),
    })
}

/// Edit ONE preset on a HELD session (the validated fast path, E1): load it, re-arm so
/// the session re-attaches to it, confirm attachment, replace every pre-discovered
/// target node, and (if `save`) persist — all WITHOUT reopening the connection. The
/// session's live-controller heartbeat (established once by `begin_live_edit`) must
/// already be warm; this keeps it warm for the next preset. The re-arm
/// (`connection_request` + `preset_list_request` + `current_preset_info_request`) is
/// what re-attaches the edit context to the just-loaded preset — my earlier
/// "load+edit in one session is rejected" finding was missing it (HW-proven:
/// 3/3 presets attached + confirmed + persisted on one connection, ~3.4 s/preset vs
/// ~8 s for the two-connection path). Echo-gated waits (E3) exit each settle window on
/// the device's echo. SAME safety gate as [`replace_one_live`]: a save only when the
/// active preset matches the target AND every replace returns `nodeReplaced` (40).
fn held_replace_one(
    s: &mut Session,
    plan: &ReplacePlan,
    repl: &ReplArg,
    save: bool,
) -> Result<BulkReplaceItem, String> {
    let list_index = plan.list_index;
    let name = plan.name.clone();
    if plan.targets.is_empty() {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no matching block".to_string(),
        });
    }

    // ── LOAD on the held session (the bench-scene-leveling precedent: a held session
    //    accepts a load + decodes its field-3 push). ──
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    // ── RE-ARM: re-arm the device's reply state on the open connection + force a fresh
    //    currentPresetInfoChanged (field 22). Echo-gated: await_active_preset exits as
    //    soon as the field-22 for `name` lands. ──
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
                                             // SAFETY 1 — confirm the held session re-attached to the TARGET preset before
                                             // editing+saving (a load that didn't take leaves a DIFFERENT preset active, and
                                             // saving it corrupts this slot — HW). active_matches prefers the `PresetLoaded` slot
                                             // echo (identity, immune to duplicate display names), falling back to the active
                                             // preset NAME only when no slot echo arrived; if NEITHER confirms, SKIP.
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(BulkReplaceItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded on held session (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
        });
    }
    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(s)?;
    let candidate_id = repl_arg_fender_id(repl);

    // SAFETY 2 — only persist if EVERY replace is confirmed (nodeReplaced/40).
    let mut applied = 0usize;
    for (group, node_id) in &plan.targets {
        let replaced = blockcaps_replaced(&roster, group, node_id);
        if let Err(reason) = blockcaps_check(&counts, candidate_id, true, replaced) {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "blocked by block-count cap ({reason}) replacing {group}/{node_id} — NOT saved"
                ),
            });
        }
        let confirmed = match repl {
            ReplArg::Saved { fender_id, index } => {
                s.replace_node_with_block(group, node_id, fender_id, *index)?
            }
            ReplArg::Model { fender_id } => s.replace_node(group, node_id, fender_id)?,
            ReplArg::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file)?,
            ReplArg::Remove => s.remove_node(group, node_id)?,
        };
        if !confirmed {
            return Ok(BulkReplaceItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!("device rejected replace of {group}/{node_id} (presetError / no nodeReplaced) — NOT saved"),
            });
        }
        blockcaps_advance(&mut counts, candidate_id, replaced);
        applied += 1;
    }
    if save {
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    // Keep live-controller status before the next preset (no long quiet gap).
    s.heartbeat()?;
    s.pump_collect(120)?;
    Ok(BulkReplaceItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{applied} block(s)"),
    })
}

/// Run a bulk replace across `plans` on ONE held session (the E1 architecture):
/// connect once, warm the live-controller heartbeat once (`begin_live_edit`), then
/// `held_replace_one` each preset — no reopens, no inter-preset settles. A per-preset
/// failure degrades to an `error`/`skipped` row (the session stays alive); only a
/// failure to ESTABLISH the session (connect/warmup) propagates as `Err`, so the
/// caller can fall back to the two-connection path before any result is emitted.
/// `on_each` is called as each preset completes (streams to the UI channel).
fn replace_many_held(
    plans: &[ReplacePlan],
    repl: &ReplArg,
    save: bool,
    mut on_each: impl FnMut(&BulkReplaceItem),
) -> Result<Vec<BulkReplaceItem>, String> {
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    let mut out = Vec::with_capacity(plans.len());
    for plan in plans {
        if BULK_REPLACE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            break; // Stop pressed — leave the remaining presets untouched.
        }
        let item = held_replace_one(&mut s, plan, repl, save).unwrap_or_else(|e| BulkReplaceItem {
            slot: plan.list_index,
            name: plan.name.clone(),
            outcome: "error".to_string(),
            detail: e,
        });
        on_each(&item);
        out.push(item);
    }
    Ok(out)
}

/// Cooperative cancel for [`copy_apply`] — set by `cancel_copy_apply` (the Copy
/// wizard's Stop), checked between presets so the held-session run stops WRITING the
/// remaining presets. Presets already saved stay changed; the rest are untouched.
static COPY_APPLY_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stop an in-flight [`copy_apply`] run after the current preset. Lightweight (just
/// sets the flag) so it does NOT take the device-op lock — it must run while the run
/// holds it.
#[tauri::command]
fn cancel_copy_apply() {
    COPY_APPLY_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// "Copy blocks between presets" — apply an ORDERED list of structural ops
/// (replace / insert / remove) to EACH target preset, live, then save that preset in
/// place (only when every op confirmed AND `save`). Mirrors [`bulk_replace_live`]'s
/// architecture exactly: ONE held re-armed session per preset (`copy_apply_one`), the
/// same DEVICE_OP_LOCK / monitor-pause bookend (`with_released_seize`), streamed
/// `CopyApplyItem`s, and a cooperative cancel (`cancel_copy_apply`). DEVICE WRITE —
/// gated behind the UI's backup acknowledgment. A per-preset failure degrades to an
/// `error`/`skipped`/`rejected` row and the run CONTINUES; an empty `ops` list →
/// `skipped`.
#[tauri::command]
async fn copy_apply(
    state: State<'_, AppState>,
    jobs: Vec<CopyJob>,
    save: bool,
    on_result: tauri::ipc::Channel<CopyApplyItem>,
) -> Result<Vec<CopyApplyItem>, String> {
    COPY_APPLY_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        // ONE held session for the whole run (the E1 architecture): connect once, warm
        // the live-controller heartbeat once (`begin_live_edit`), then `copy_apply_one`
        // each preset with no reopens. A per-preset failure stays an `error` row (the
        // session stays alive); only a failure to ESTABLISH the session propagates.
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        let mut out = Vec::with_capacity(jobs.len());
        for job in &jobs {
            if COPY_APPLY_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break; // Stop pressed — leave the remaining presets untouched.
            }
            let item = copy_apply_one(&mut s, job, save).unwrap_or_else(|e| CopyApplyItem {
                slot: job.list_index,
                name: job.name.clone(),
                outcome: "error".to_string(),
                detail: e,
                graph: None,
            });
            let _ = on_result.send(item.clone());
            out.push(item);
        }
        Ok(out)
    })
    .await
}

/// Apply one [`CopyJob`]'s ordered ops to its target preset on a HELD re-armed session
/// and (if `save`) persist it — the [`held_replace_one`] shape generalised from one
/// replace to a list of replace/insert/remove ops. Loads the preset, re-arms the edit
/// context, confirms attachment (the SAME safety gate: never edit/save an unverified
/// preset), applies each op (RETRY-HARDENING the cold first op's silent DROP), and
/// saves ONLY when every op confirmed AND no `presetError`. An empty op list → skipped.
fn copy_apply_one(s: &mut Session, job: &CopyJob, save: bool) -> Result<CopyApplyItem, String> {
    let list_index = job.list_index;
    let name = job.name.clone();
    if job.ops.is_empty() {
        return Ok(CopyApplyItem {
            slot: list_index,
            name,
            outcome: "skipped".to_string(),
            detail: "no ops".to_string(),
            graph: None,
        });
    }

    // ── LOAD on the held session + RE-ARM the edit context to the just-loaded preset
    //    (mirrors `held_replace_one`). ──
    s.clear_raw();
    s.send_and_collect(&proto::load_preset((list_index + 1) as u64, 1), 200)?;
    s.send_and_collect(&proto::connection_request(), 80)?;
    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
    s.send_and_collect(&proto::current_preset_info_request(2), 120)?;
    let _ = s.await_active_preset(&name, 8); // pump for the fresh currentPresetInfoChanged
                                             // SAFETY — confirm the held session re-attached to the TARGET preset before
                                             // editing/saving (active_matches prefers the PresetLoaded slot echo, falling back to
                                             // the active name only when no slot echo arrived).
    if !s.active_matches(list_index, Some(&name)) {
        return Ok(CopyApplyItem {
            slot: list_index,
            name: name.clone(),
            outcome: "error".to_string(),
            detail: format!(
                "could not confirm target preset loaded on held session (slot {:?} ≠ {list_index}, active {:?} ≠ target {name:?}) — not edited",
                s.loaded_slot(),
                s.active_preset_name()
            ),
            graph: None,
        });
    }

    // ── blockcaps guard — read the PRE-edit roster now, before the first structural
    //    edit (fail-closed: an unreadable roster refuses the WHOLE target). ──
    let (roster, mut counts) = blockcaps_pre_edit_roster(s)?;

    // Apply each op in order. The FIRST structural edit after a fresh load can be
    // silently DROPPED — retry it once (but NEVER on a presetError, a real rejection).
    let total = job.ops.len();
    for (i, op) in job.ops.iter().enumerate() {
        let first = i == 0;

        // Candidate/mode/target per op kind: Remove has no candidate (only shrinks —
        // never a cap check); Replace subtracts its target's contribution, Insert
        // doesn't (mirrors the TS `checkOp` mode-aware formula).
        let (candidate_id, is_replace, target): (Option<&str>, bool, Option<(&str, &str)>) =
            match op {
                CopyOp::Replace {
                    group,
                    node_id,
                    repl,
                } => (
                    Some(repl.insert_fender_id()),
                    true,
                    Some((group.as_str(), node_id.as_str())),
                ),
                CopyOp::Insert { repl, .. } => (Some(repl.insert_fender_id()), false, None),
                CopyOp::Remove { group, node_id } => {
                    (None, false, Some((group.as_str(), node_id.as_str())))
                }
            };
        let replaced = target.and_then(|(g, n)| blockcaps_replaced(&roster, g, n));
        if let Err(reason) = blockcaps_check(&counts, candidate_id, is_replace, replaced) {
            return Ok(CopyApplyItem {
                slot: list_index,
                name: name.clone(),
                outcome: "error".to_string(),
                detail: format!(
                    "op {}/{total} ({}) blocked by block-count cap: {reason} — NOT saved",
                    i + 1,
                    describe_copy_op(op)
                ),
                graph: None,
            });
        }

        match apply_copy_op(s, op, first) {
            Ok(true) => {
                blockcaps_advance(&mut counts, candidate_id, replaced);
            }
            Ok(false) => {
                return Ok(CopyApplyItem {
                    slot: list_index,
                    name: name.clone(),
                    outcome: "error".to_string(),
                    detail: format!(
                        "device rejected op {}/{total} ({}) — presetError / no confirm — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                    graph: None,
                });
            }
            Err(e) => {
                return Ok(CopyApplyItem {
                    slot: list_index,
                    name: name.clone(),
                    outcome: "error".to_string(),
                    detail: format!(
                        "op {}/{total} ({}) failed: {e} — NOT saved",
                        i + 1,
                        describe_copy_op(op)
                    ),
                    graph: None,
                });
            }
        }
    }

    if save {
        // Identity-preserving persist (Pro Control's rename(current name) → save(slot)):
        // keeps the preset's name and song link.
        if !name.is_empty() {
            s.rename_current_preset(&name)?;
        }
        s.save_current_preset(list_index)?;
    }
    // Keep the live-controller status warm before the next preset.
    s.heartbeat()?;
    s.pump_collect(120)?;
    // Read back the post-save graph so the Copy view can patch its cached library in
    // place (no ~22 s re-scan). The held session's dense heartbeat carries the full
    // `guitarNodes`; `None` when the field-3 isn't readable, and the frontend then falls
    // back to a re-scan. Only on save — an unsaved edit must not poison the cache.
    let graph = if save {
        s.current_preset_value()
            .ok()
            .map(|v| session::extract_active_graph(&v, None))
    } else {
        None
    };
    Ok(CopyApplyItem {
        slot: list_index,
        name,
        outcome: "updated".to_string(),
        detail: format!("{total} op(s)"),
        graph,
    })
}

/// Apply ONE [`CopyOp`] on the held session, returning whether the device CONFIRMED it
/// (`nodeReplaced`(40) / `nodeRemoved`(36) / `nodeInserted`(33)). `retry_drop` re-tries
/// a single SILENT drop (the cold first edit after a fresh load) but never a
/// `presetError`. IR/saved INSERT re-resolves the newly-added node id and applies the
/// IR-file / saved-block follow-up; if the new id can't be resolved it FALLS BACK to a
/// bare Model insert and `log::warn!`s the degradation.
fn apply_copy_op(s: &mut Session, op: &CopyOp, retry_drop: bool) -> Result<bool, String> {
    match op {
        CopyOp::Replace {
            group,
            node_id,
            repl,
        } => {
            let confirmed = apply_copy_replace(s, group, node_id, repl)?;
            if !confirmed && retry_drop && !s.saw_preset_error() {
                return apply_copy_replace(s, group, node_id, repl);
            }
            Ok(confirmed)
        }
        CopyOp::Remove { group, node_id } => {
            let confirmed = s.remove_node(group, node_id)?;
            if !confirmed && retry_drop && !s.saw_preset_error() {
                return s.remove_node(group, node_id);
            }
            Ok(confirmed)
        }
        CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } => {
            let confirmed =
                apply_copy_insert(s, group, before_fender_id.as_deref(), repl, retry_drop)?;
            Ok(confirmed)
        }
    }
}

/// `CopyRepl` REPLACE dispatch — the `ReplArg` dispatch from `held_replace_one`, minus
/// the (absent) Remove variant.
fn apply_copy_replace(
    s: &mut Session,
    group: &str,
    node_id: &str,
    repl: &CopyRepl,
) -> Result<bool, String> {
    match repl {
        CopyRepl::Model { fender_id } => s.replace_node(group, node_id, fender_id),
        CopyRepl::Saved { fender_id, index } => {
            s.replace_node_with_block(group, node_id, fender_id, *index)
        }
        CopyRepl::Ir { file, .. } => s.replace_node_with_ir(group, node_id, file),
    }
}

/// INSERT a block. The Model insert is the faithful one-shot (`insert_node` 34). For an
/// IR/saved insert we insert the bare model/placeholder, then RE-RESOLVE the
/// newly-added node id (the node present after the insert that was absent before, read
/// off the held session's roster) and apply the IR-file link / saved-block swap to it.
/// If the new id can't be resolved, FALL BACK to the bare Model insert and warn.
fn apply_copy_insert(
    s: &mut Session,
    group: &str,
    before_fender_id: Option<&str>,
    repl: &CopyRepl,
    retry_drop: bool,
) -> Result<bool, String> {
    let insert_id = repl.insert_fender_id();
    // Roster of this group BEFORE the insert — to diff the new node id afterwards.
    let before: std::collections::HashSet<String> = roster_node_ids_in_group(s, group);

    // field-34 insert: `before_fender_id` is the anchor to insert AHEAD of (the device's
    // field-2 inserts BEFORE the referenced node); `None` appends at the group end.
    let do_insert = |s: &mut Session| s.insert_node(group, before_fender_id, insert_id);
    let mut confirmed = do_insert(s)?;
    if !confirmed && retry_drop && !s.saw_preset_error() {
        confirmed = do_insert(s)?;
    }
    if !confirmed {
        return Ok(false);
    }

    // A plain Model insert is complete.
    let CopyRepl::Model { .. } = repl else {
        // IR / Saved → re-resolve the new node id and apply the content follow-up.
        let after: std::collections::HashSet<String> = roster_node_ids_in_group(s, group);
        let new_id = after.difference(&before).next().cloned();
        match new_id {
            Some(id) => match repl {
                CopyRepl::Ir { file, .. } => {
                    // Replace the just-inserted node WITH the IR (two-step: → ACD_UserIRTMS
                    // + the string `file` param), full-fidelity.
                    return s.replace_node_with_ir(group, &id, file);
                }
                CopyRepl::Saved { fender_id, index } => {
                    return s.replace_node_with_block(group, &id, fender_id, *index);
                }
                CopyRepl::Model { .. } => unreachable!("handled above"),
            },
            None => {
                log::warn!(
                    "[copy_apply] IR/saved INSERT into {group} degraded to a bare insert: could not \
                     re-resolve the newly-added node id on the held session (inserted {insert_id})"
                );
                // The bare insert DID land (confirmed) — report success, but the block is
                // a bare placeholder/model, not the IR/saved content.
                return Ok(true);
            }
        }
    };
    Ok(true)
}

/// The node ids currently in `group` on the held session (from a fresh field-3
/// roster read). Empty if no preset JSON is available yet.
fn roster_node_ids_in_group(s: &mut Session, group: &str) -> std::collections::HashSet<String> {
    let _ = s.heartbeat();
    let _ = s.pump_collect(200);
    s.current_preset_value()
        .ok()
        .map(|v| {
            audiograph::roster(&v)
                .into_iter()
                .filter(|(g, _, _)| g == group)
                .map(|(_, node_id, _)| node_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Short human description of a `CopyOp` for the per-preset `error` detail.
fn describe_copy_op(op: &CopyOp) -> String {
    match op {
        CopyOp::Replace {
            group,
            node_id,
            repl,
        } => {
            format!("replace {group}/{node_id} → {}", repl.insert_fender_id())
        }
        CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } => format!(
            "insert {} into {group}{}",
            repl.insert_fender_id(),
            match before_fender_id.as_deref() {
                Some(b) => format!(" before {b}"),
                None => " (append)".to_string(),
            }
        ),
        CopyOp::Remove { group, node_id } => format!("remove {group}/{node_id}"),
    }
}

/// Retry active-graph discovery because the TMP's field-3 stream length varies
/// slightly between handshakes. A graph is usable only after its routing
/// template arrives; otherwise a parallel path can be rendered as series.
fn discover_active_graph() -> Result<(session::ActiveGraph, String), String> {
    let mut errors = Vec::new();
    for _ in 0..3 {
        match Session::connect_for_discovery() {
            Ok(mut s) => {
                for _ in 0..4 {
                    let diagnostics = format!(
                        "{}\n{}",
                        s.slot_read_diagnostics(),
                        s.active_graph_diagnostics()
                    );
                    match s.current_audio_graph() {
                        Ok(mut graph) => {
                            if graph.slot.is_none() {
                                graph.slot = s.resolve_unique_my_preset_slot(graph.name.as_deref());
                            }
                            return Ok((graph, diagnostics));
                        }
                        Err(e) => errors.push(format!("{e}\n{diagnostics}")),
                    }
                    s.pump_more(250)?;
                }
            }
            Err(e) => errors.push(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err(errors.join("\n--- retry ---\n"))
}

/// Read-only diagnostic for the active-preset signal chain. Prints the
/// live discovery payload summary and the routing-aware graph sent to React.
pub fn probe_active_graph() -> Result<String, String> {
    let (graph, diagnostics) = discover_active_graph()?;
    let graph = serde_json::to_string_pretty(&graph)
        .map_err(|e| format!("serialize active graph diagnostic: {e}"))?;
    Ok(format!("{diagnostics}\n{graph}\n"))
}

/// Force re-amp mode OFF on a fresh connection — the recovery path for a unit left
/// input-muted ("no sound") by an interrupted leveling run (re-amp routes the input
/// to USB; a dropped fire-and-forget OFF strands it there). `probe --reamp-off`.
pub fn probe_reamp_off() -> Result<(), String> {
    Session::connect()?.set_reamp_mode(false).map(|_| ())
}

/// NO-SAVE joint-k leveling run (`probe --level-scenes <listIdx> <target> <topology> [scene…]`):
/// the REAL `build_scene_jobs` → `level_scenes_oneshot` path with `save=false`, so it
/// measures/solves/applies (writing the amp `outputLevel`(s) to the live edit buffer) and
/// then RELOADS the stored preset to discard the edit — nothing is persisted. Validates
/// joint-k on hardware: for a parallel preset both lane amps are scaled by one factor and
/// the verify capture reports the achieved LUFS vs target. Ends with a guaranteed re-amp OFF.
pub fn probe_level_scenes_oneshot(
    list_index: u32,
    target_lufs: f64,
    topology_id: String,
    scene_slots: Vec<u32>,
    rebalance: bool,
) -> Result<String, String> {
    let scene_slots = if scene_slots.is_empty() {
        vec![session::BASE_SCENE_SLOT]
    } else {
        scene_slots
    };
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &scene_slots)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
    // NO SAVE — restores the stored preset after measuring.
    let outcomes = if rebalance {
        leveller::level_scenes_rebalance(
            list_index,
            &jobs,
            &stim,
            target_lufs,
            false,
            |_, _| {},
            || false,
        )
    } else {
        leveller::level_scenes_oneshot(
            list_index,
            &jobs,
            &stim,
            target_lufs,
            false,
            |_, _| {},
            || false,
        )
    };
    // Guaranteed re-amp OFF regardless of outcome (a stranded re-amp mutes the input).
    let _ = Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));
    let outcomes = outcomes?;
    let mut out = format!(
        "NO-SAVE leveling preset list_index={list_index} → target {target_lufs:.1} LUFS (topology {topology_id})\n"
    );
    for o in &outcomes {
        match &o.failure {
            Some(f) => out += &format!("  scene {} → FAILED/SKIP: {f}\n", o.scene_slot),
            None => {
                let lufs = o.final_lufs.unwrap_or(f64::NAN);
                out += &format!(
                    "  scene {} → achieved {lufs:.2} LUFS (err {:+.2})  level={:.4}{}\n",
                    o.scene_slot,
                    lufs - target_lufs,
                    o.final_level.unwrap_or(0.0),
                    if o.clamped { "  CLAMPED" } else { "" },
                );
            }
        }
    }
    Ok(out)
}

/// NON-DESTRUCTIVE classifier check (`probe --classify <listIdx> [scene…]`): load the
/// preset, harvest the pre-pass scene docs, and print how `build_scene_jobs` classifies
/// each scene's amp-knob set (routing → series last-amp / parallel joint-k / skip).
/// No re-amp, no parameter writes, no save — just loads + reads field-3. The headless
/// proof that the routing-aware classifier sees a real preset (e.g. 027 parallel) right.
pub fn probe_classify_scenes(list_index: u32, scene_slots: Vec<u32>) -> Result<String, String> {
    let scene_slots = if scene_slots.is_empty() {
        vec![session::BASE_SCENE_SLOT]
    } else {
        scene_slots
    };
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &scene_slots)?;
    let template = structure_graph(&docs)
        .and_then(|g| g.template)
        .unwrap_or_else(|| "<unknown>".to_string());
    let mut out = format!(
        "preset list_index={list_index} template={template}\n  amp outputLevel candidates: {}\n",
        if candidates.is_empty() {
            "(none)".to_string()
        } else {
            candidates
                .iter()
                .map(|c| format!("{}/{}={:.3}", c.group_id, c.node_id, c.value))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
    for j in &jobs {
        if let Some(reason) = &j.skip {
            out += &format!("  scene {} → SKIP: {reason}\n", j.scene_slot);
            continue;
        }
        let knobs = j
            .knobs
            .iter()
            .map(|kt| match &kt.knob {
                leveller::LevelKnob::Block {
                    group_id, node_id, ..
                } => {
                    format!("{group_id}/{node_id}@{:.3}", kt.current)
                }
                leveller::LevelKnob::PresetLevel => "presetLevel".to_string(),
            })
            .collect::<Vec<_>>();
        let mode = if knobs.len() > 1 { "JOINT-K" } else { "single" };
        out += &format!("  scene {} → {mode} {:?}\n", j.scene_slot, knobs);
    }
    Ok(out)
}

/// Recall scene `scene_slot` (0-based `scenes[]` index; 8 = base) on the device's
/// CURRENT preset — the
/// headless runbook entry for HW-validating `loadScene` (PresetMessage 101).
/// Non-destructive: a live state change, persists nothing. Verify the recall by
/// diffing `--activegraph` bypass states before/after.
pub fn probe_load_scene(scene_slot: u32) -> Result<(), String> {
    Session::connect()?.load_scene(scene_slot)
}

/// Measure the currently selected preset/scene through re-amp without changing
/// preset level or block parameters. Optional `slot` loads a preset first in its
/// own connection; optional `scene_slot` recalls a scene before capture. No save.
pub fn probe_measure_current_lufs(
    topology_id: &str,
    slot: Option<u32>,
    scene_slot: Option<u32>,
    calibration_lufs: Option<f32>,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    if let Some(slot) = slot {
        {
            let mut s = Session::connect()?;
            s.load_preset(slot)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    let mut s = Session::connect()?;
    if let Some(scene) = scene_slot {
        s.load_scene(scene)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    s.set_reamp_mode(true)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    let cap = audio::reamp_capture(&stim, 48_000, 800);
    let _ = s.set_reamp_mode(false);
    let cap = cap?;
    let (ch, _) = cap.loudest_channel();
    let loud = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?;
    if !loud.integrated_lufs.is_finite() {
        return Err("no finite signal captured (re-amp may not have routed)".to_string());
    }
    Ok(format!(
        "slot={} topology={topology_id} scene={} channel={ch} integrated_lufs={:.3} short_term_max_lufs={:.3}",
        slot.map(|s| s.to_string()).unwrap_or_else(|| "current".to_string()),
        scene_slot.map(|s| s.to_string()).unwrap_or_else(|| "current".to_string()),
        loud.integrated_lufs,
        loud.short_term_max_lufs,
    ))
}

/// HW probe: does re-amp survive a DISENGAGE → settle → RE-ENGAGE on ONE held HID
/// connection? The whole leveling speed story hinges on this. If a single held
/// session can do N `[load_scene → engage → capture → disengage]` cycles and read
/// the SAME loudness a fresh connection reads, we can keep Pro Control's one
/// persistent session (instant scene changes). If not, the proven once-engage-per-
/// connection rule stands. Non-destructive: loads + scene recalls + captures, NO
/// parameter writes. Measures each scene twice — once on a HELD session, once via
/// the proven FRESH-connection control — and compares.
pub fn probe_held_reengage(
    topology_id: &str,
    slot: u32,
    scenes: &[u32],
    calibration_lufs: Option<f32>,
) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path(topology_id)?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;

    let measure = |cap: Result<audio::Capture, String>| -> f64 {
        match cap {
            Ok(cap) => {
                let (ch, _) = cap.loudest_channel();
                lufs::measure_mono(&cap.channel(ch), cap.sample_rate)
                    .map(|l| l.integrated_lufs)
                    .unwrap_or(f64::NAN)
            }
            Err(_) => f64::NAN,
        }
    };

    // Load the preset in its OWN throwaway connection (the load+engage→silence rule).
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
    }
    std::thread::sleep(Duration::from_millis(800));

    let mut out = format!(
        "HELD-SESSION RE-ENGAGE probe — slot={slot} topology={topology_id} scenes={scenes:?}\n\n"
    );

    // ── HELD: ONE connection, N [load_scene → engage → capture → disengage] cycles.
    out += "[A] HELD session (one connection, re-engage per scene):\n";
    let mut held = Vec::new();
    {
        let mut s = Session::connect()?;
        for (i, &scene) in scenes.iter().enumerate() {
            s.load_scene(scene)?;
            std::thread::sleep(Duration::from_millis(500));
            let echo = s.set_reamp_mode(true)?;
            std::thread::sleep(Duration::from_millis(500));
            let cap = audio::reamp_capture(&stim, 48_000, 800);
            let _ = s.set_reamp_mode(false);
            std::thread::sleep(Duration::from_millis(500)); // disengage settle before next cycle
            let m = measure(cap);
            out += &format!(
                "    cycle {i}: scene={scene} engage_echo={echo:?} integrated_lufs={m:.3}\n"
            );
            held.push(m);
        }
    }

    // ── CONTROL: FRESH connection per scene (the proven measure_scene_asis shape).
    out += "\n[B] FRESH connection per scene (proven control):\n";
    let mut fresh = Vec::new();
    for &scene in scenes {
        let mut s = Session::connect()?;
        s.load_scene(scene)?;
        std::thread::sleep(Duration::from_millis(500));
        s.set_reamp_mode(true)?;
        std::thread::sleep(Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        let m = measure(cap);
        out += &format!("    scene={scene} integrated_lufs={m:.3}\n");
        fresh.push(m);
    }

    // ── Verdict.
    let all_finite = held.iter().all(|m| m.is_finite());
    let (mut mn, mut mx) = (f64::MAX, f64::MIN);
    for m in held.iter().chain(fresh.iter()).filter(|m| m.is_finite()) {
        mn = mn.min(*m);
        mx = mx.max(*m);
    }
    let scenes_differ = (mx - mn).abs() > 1.0;
    let matches_fresh = held
        .iter()
        .zip(&fresh)
        .all(|(h, f)| h.is_finite() && f.is_finite() && (h - f).abs() < 1.5);
    out += "\nVERDICT:\n";
    out += &format!("    held all non-silent:                 {all_finite}\n");
    out += &format!("    scenes genuinely differ (>1 LU):     {scenes_differ}\n");
    out += &format!("    held matches fresh (per-scene <1.5LU): {matches_fresh}\n");
    out += &format!(
        "    => HELD SESSION {}\n",
        if all_finite && matches_fresh {
            "VIABLE — re-engage works on one connection; persistent-session leveling is on the table"
        } else {
            "NOT VIABLE — re-engage is unreliable on a held connection; keep fresh-connection-per-scene"
        }
    );
    Ok(out)
}

/// Tier-4 diagnostic: capture the live `currentPresetDataChanged` (field 3) preset
/// JSON on a dense-heartbeat session, write the full decompressed body to
/// `out_path`, and report whether it is COMPLETE (the `scenes` array + `ftsw` map
/// present and the JSON parses to the end) — the go/no-go for live FS-tags and the
/// "truncates at scenes" gotcha. `slot` (Some) loads that preset first to trigger a
/// fresh push. Non-destructive (load + reads only).
pub fn probe_dump_preset_data(slot: Option<u32>, out_path: &str) -> Result<String, String> {
    let raw = Session::connect()?.capture_full_preset_json(slot, 2000)?;
    std::fs::write(out_path, &raw).map_err(|e| format!("write {out_path}: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    // A healthy dense-heartbeat field-3 is ~17 KB and routinely truncates only at
    // the LAST scene's uuid — so a full serde parse fails even though `ftsw` and
    // most of `scenes` ARE present. Detect presence by raw key search (reliable);
    // report the full-parse result separately. `ftsw` (footswitch→scene map) sorts
    // well before `scenes`, so its presence is the FS-tag go/no-go.
    let complete = serde_json::from_str::<serde_json::Value>(&text).is_ok();
    let has_ftsw = text.contains("\"ftsw\"");
    let has_scenes = text.contains("\"scenes\"");
    let scene_names = text.matches("\"sceneName\"").count();
    Ok(format!(
        "wrote {} bytes to {out_path}\n  full-parse-complete: {complete}\n  ftsw present: {has_ftsw}\n  scenes present: {has_scenes} ({scene_names} sceneName entries seen)\n  -> live FS-tags feasible: {}",
        raw.len(),
        if has_ftsw { "YES (ftsw survives in the live field-3 partial)" } else { "NO (ftsw truncated away -> degrade to em-dash)" },
    ))
}

/// Push-listener discovery experiment: full handshake, then park `seconds`
/// printing every inbound stream (the unit's unsolicited pushes) as it lands.
/// Read-only apart from the ConnectionHeartbeat every `hb_ms` MILLISECONDS (≈250 =
/// Pro Control's 4/sec keepalive) and the optional current-preset poll every
/// `poll_secs`.
pub fn probe_listen(seconds: u64, hb_ms: u64, poll_secs: u64) -> Result<(), String> {
    Session::connect()?.listen_dump(seconds, hb_ms, poll_secs)
}

/// Discover a preset's level-type block controls (the leveling-knob candidates).
///
/// Primary path is the 1.8.45-SAFE RICH LEAN SESSION (the bench intel-session /
/// `prepass_scene_docs` pattern): heartbeat warmup → `send_and_collect(LoadPreset)`
/// → pump past the 125 hit → read `current_preset_blocks` from the accumulated
/// field-3 push bodies. `connect_for_discovery` (field-78) is effectively DEAD on
/// fw 1.8.45 — it never delivers `currentPresetDataChanged` — so it can no longer be
/// the primary; it stays only as a fallback for older firmware. Without this, FS-scene
/// leveling found zero amp candidates and silently skipped every scene (the device
/// never switched scenes).
fn load_then_discover_blocks(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    match discover_blocks_rich(slot) {
        Ok(blocks) if !blocks.is_empty() => return Ok(blocks),
        Ok(_) => log::warn!("rich block discovery for slot={slot}: loaded but no level blocks"),
        Err(e) => log::warn!("rich block discovery for slot={slot}: {e}"),
    }
    // Fallback for older firmware where the field-78 discovery handshake works.
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    match Session::connect_for_discovery()?.current_preset_blocks() {
        Ok(blocks) => Ok(blocks),
        Err(first_err) => {
            log::warn!("block discovery fallback for slot={slot}: {first_err}");
            let mut s = Session::connect()?;
            let raw = s.capture_full_preset_json(Some(slot), 2000)?;
            let text = String::from_utf8_lossy(&raw);
            let value = session::tolerant_parse_json(&text)
                .ok_or_else(|| format!("{first_err}; fallback field-3 JSON did not parse"))?;
            let blocks = session::extract_level_blocks(&value);
            if blocks.is_empty() {
                Err(format!(
                    "{first_err}; fallback field-3 JSON had no level blocks"
                ))
            } else {
                Ok(blocks)
            }
        }
    }
}

/// 1.8.45-safe block discovery: a single rich lean session loads the preset via
/// `send_and_collect` (NOT `load_preset`, which discards the reports the field-3 push
/// rides on) and reads the level blocks from the accumulated push bodies. Mirrors the
/// bench intel session + `prepass_scene_docs`.
fn discover_blocks_rich(slot: u32) -> Result<Vec<session::LevelBlock>, String> {
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
    // Keep pumping past the 125 hit — the multi-packet field-3 push (block discovery)
    // needs the extra turns to finish arriving.
    for _ in 0..10 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    s.current_preset_blocks()
}

/// Enumerate a preset's level-type block controls (the leveling-knob candidates).
/// Used by `probe --blocks`.
pub fn probe_list_blocks(slot: u32) -> Result<String, String> {
    let blocks = load_then_discover_blocks(slot)?;
    if blocks.is_empty() {
        return Err(
            "no level-type block controls found (preset JSON may have truncated \
                    before audioGraph completed, or the preset has no level params)"
                .to_string(),
        );
    }
    let mut out = format!(
        "slot {slot}: {} level-type block control(s):\n",
        blocks.len()
    );
    for b in &blocks {
        out += &format!(
            "  {} / {} [{}] / {} = {:.4}\n",
            b.group_id, b.node_id, b.model_id, b.parameter_id, b.value
        );
    }
    Ok(out)
}

/// AC1: read a library slot's preset JSON over USB and report whether
/// it is a complete preset or a partial. **RESOLVED on 1.7.75 HW:** USB does NOT
/// yield a complete preset — `presetDataRequest` (field 8 → `presetDataChanged`
/// 9, plaintext) returns a per-slot-DETERMINISTIC partial (e.g. slot 0 = 1669 B
/// empty nodes; slot 1 = 17264 B with scenes but cut mid-`uuid`); the device
/// truncates the stream at the source. `exportPresetRequest` (115) is unimplemented
/// (no response). So the canonical full-preset source is OFFLINE `.preset` files;
/// this path serves USB partials (search/inventory/quick reads), not backup.
///
/// The request MUST ride inside the handshake burst with NO batchStatus — a
/// standalone post-handshake request, or one carrying a batch, gets no reply.
pub fn probe_export_preset(list_enum: u32, slot: u32) -> Result<String, String> {
    // Slot-addressed full read on a MINIMAL re-armed burst (connect() +
    // read_slot_preset_json), NOT the full-handshake Classic burst. The field-9
    // reply's unkeyed 0x33/0x34/0x35 framing collides with the ~17 KB ProductProfile
    // flood when the read is appended to the full handshake (HW `probe --slotread-x`:
    // Classic burst = NO REPLY, every minimal/re-arm variant = clean reply). This is
    // the same reliable path scan_preset_scenes / probe_slot_json use. (`read_slot_preset_json`
    // addresses My Presets, so `list_enum` is effectively 1 here — the only validated case.)
    let _ = list_enum;
    let raw = {
        let mut s = Session::connect()?;
        s.drain_until_quiet(250, 20)?;
        s.read_slot_preset_json(slot)?
            .ok_or_else(|| format!("slot {slot}: field-8 slot read returned no JSON"))?
    };

    // presetDataChanged.presetJson is plaintext; currentPresetDataChanged is LZ4.
    // Try LZ4 first so this reporter also handles an LZ4 carrier if one appears.
    let (decoded, encoding) = match proto::lz4_block_decompress(&raw) {
        Ok(d) if !d.is_empty() => (d, "lz4-block"),
        _ => (raw.clone(), "plaintext"),
    };
    let text = String::from_utf8_lossy(&decoded);

    let parses_complete = serde_json::from_str::<serde_json::Value>(&text).is_ok();
    let has = |needle: &str| text.contains(needle);
    let verdict = if parses_complete && has("\"scenes\"") {
        "FULL — complete JSON with scenes"
    } else if has("\"scenes\"") {
        "PARTIAL — 'scenes' present but JSON truncated (device-side cut; OFFLINE needed for full)"
    } else {
        "PARTIAL — no 'scenes' (truncated early; OFFLINE needed for full)"
    };

    let preview: String = text.chars().take(200).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(120)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Ok(format!(
        "[probe --export] listEnum={list_enum} slot={slot} via presetDataRequest(field 8, no-batch, in-burst)\n\
         raw_bytes={} encoding={encoding} decoded_bytes={}\n\
         parses_complete_json={parses_complete}\n\
         has_scenes={} has_ftsw={} has_exp={} has_audioGraph={}\n\
         VERDICT: {verdict}\n\
         --- head(200) ---\n{preview}\n--- tail(120) ---\n{tail}\n",
        raw.len(),
        decoded.len(),
        has("\"scenes\""),
        has("\"ftsw\""),
        has("\"exp\""),
        has("\"audioGraph\""),
    ))
}

/// Frame/marker forensics over a session's raw accumulated reports: frame
/// counts by magic, plus plaintext-JSON marker counts in the concatenated
/// frame bodies — the field-9 `presetJson` is PLAINTEXT (not LZ4), so its
/// markers are visible in the raw bytes even when the unkeyed `0x33` stream
/// reassembly mangles it. `expected_name` adds a slot-specific needle
/// (`"displayName":"<name>"`) that the protobuf preset lists can't fake.
/// Distinguishes "device never sent it" from "host reassembly lost it".
fn slotread_forensics(raw: &[Vec<u8>], expected_name: &str) -> String {
    let (mut n33, mut n34, mut n35) = (0u32, 0u32, 0u32);
    let mut all = Vec::new();
    for r in raw {
        if r.len() < 4 || r[0] != 0 {
            continue;
        }
        match r[1] {
            0x33 => n33 += 1,
            0x34 => n34 += 1,
            0x35 => n35 += 1,
            _ => {}
        }
        let l = r[3] as usize;
        all.extend_from_slice(&r[4..(4 + l).min(r.len())]);
    }
    let hay = String::from_utf8_lossy(&all);
    let count = |n: &str| hay.matches(n).count();
    let name_kv = count(&format!("\"displayName\":\"{expected_name}\""))
        + count(&format!("\"displayName\": \"{expected_name}\""));
    format!(
        "frames 33/34/35={n33}/{n34}/{n35} rawmarkers: displayName(slot)={name_kv} sceneName={} audioGraph={}",
        count("\"sceneName\""),
        count("\"audioGraph\""),
    )
}

/// One result line for a slot-read experiment attempt: reply size + identity
/// check (does the JSON's displayName match the slot's list name — the
/// non-destructive mapping confirmation) + raw-frame forensics.
fn slotread_report(
    tag: &str,
    slot: u32,
    expected_name: &str,
    reply: Option<&[u8]>,
    s: &Session,
) -> String {
    let forensics = slotread_forensics(&s.raw, expected_name);
    match reply {
        Some(b) => {
            let text = String::from_utf8_lossy(b);
            let name_ok = text.contains(&format!("\"displayName\":\"{expected_name}\""))
                || text.contains(&format!("\"displayName\": \"{expected_name}\""));
            format!(
                "  [{tag}] slot {slot} ({expected_name}): REPLY {}B nameMatch={name_ok} sceneNames={} | {forensics}\n",
                b.len(),
                text.matches("\"sceneName\"").count(),
            )
        }
        None => format!(
            "  [{tag}] slot {slot} ({expected_name}): NO REPLY | {forensics} | diag {}\n",
            s.slot_read_diagnostics()
        ),
    }
}

/// Pump until the field-9 reply stops growing (2 stable windows), bounded.
/// A lighter `harvest_slot_read` for the experiment matrix (12×400 ms instead
/// of 20×500 ms — 9 connections back-to-back must not take minutes).
fn slotread_harvest(s: &mut Session) -> Option<Vec<u8>> {
    let mut last = 0usize;
    let mut stable = 0u32;
    for _ in 0..12 {
        if s.pump_more(400).is_err() {
            break;
        }
        let len = s.try_preset_data_json().map(|b| b.len()).unwrap_or(0);
        if len > 0 && len == last {
            stable += 1;
            if stable >= 2 {
                break;
            }
        } else {
            stable = 0;
        }
        last = len;
    }
    s.try_preset_data_json()
}

/// Investigation (`probe --slotread-x [deviceSlot…]`): can the slot-addressed
/// `presetDataRequest` (field 8 → `presetDataChanged` 9) serve a
/// NON-DESTRUCTIVE per-slot scene read — no LoadPreset, the unit's selected
/// preset never changes? The connect-fast benchmark scored the
/// classic in-burst read 0/25 on fw 1.8.45 ("ProductProfile collision"); this
/// matrix separates a device-side drop from a host-side reassembly loss:
///   B          post-handshake read on a warmed dense-heartbeat LIVE session
///   C-early    in-burst, read fired BEFORE the flood requests
///   C-minimal  trimmed burst: connection_request + My Presets + read only
///   A          classic full-burst baseline (the 0/25 configuration)
/// Slots are 1-based DEVICE slots (list index + 1); default = first 3
/// non-empty presets. Sends ZERO LoadPreset.
pub fn probe_slotread_experiments(device_slots: Vec<u32>) -> Result<String, String> {
    use session::SlotReadBurst;

    // ── Exp B: warmed live session (also sources the preset list + slots). ──
    let mut s = Session::connect()?;
    let presets = s.list_my_presets()?;
    let name_of = |dev_slot: u32| {
        presets
            .get((dev_slot - 1) as usize)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "?".to_string())
    };
    let slots: Vec<u32> = if device_slots.is_empty() {
        presets
            .iter()
            .filter(|p| !session::is_empty_slot_name(&p.name))
            .take(3)
            .map(|p| p.slot + 1)
            .collect()
    } else {
        device_slots
    };
    if slots.is_empty() {
        return Err("no non-empty presets to read".to_string());
    }
    let mut out = format!(
        "[slotread-x] device slots {slots:?} — field-8 presetDataRequest, NO LoadPreset\n\
         \n── Exp B: post-handshake on a warmed LIVE session (16×120ms heartbeats) ──\n"
    );
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    for &slot in &slots {
        s.raw.clear();
        s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 400)?;
        let reply = slotread_harvest(&mut s);
        out += &slotread_report("B", slot, &name_of(slot), reply.as_deref(), &s);
        let _ = s.heartbeat();
    }
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── Exps C-early / C-minimal / A-classic: one fresh connection per slot. ──
    // ── Exp D: MULTIPLE field-8 reads on ONE minimal-burst connection — the
    // production-scan shape (one connection for the whole launch scan). The
    // first read rides the burst window; each later read re-tests whether the
    // device keeps answering data requests on the same session.
    out += "\n── Exp D: sequential reads on ONE minimal-burst connection ──\n";
    {
        let first_req = proto::preset_data_request(1, slots[0] as u64, None);
        match Session::connect_slotread(SlotReadBurst::Minimal, &first_req) {
            Ok(mut s) => {
                let reply = slotread_harvest(&mut s);
                out += &slotread_report("D", slots[0], &name_of(slots[0]), reply.as_deref(), &s);
                for &slot in &slots[1..] {
                    s.raw.clear();
                    s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 400)?;
                    let reply = slotread_harvest(&mut s);
                    out += &slotread_report("D", slot, &name_of(slot), reply.as_deref(), &s);
                }
            }
            Err(e) => out += &format!("  [D] connect FAILED: {e}\n"),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    // ── Exp E: re-arm the burst window ON THE SAME CONNECTION by re-sending
    // connection_request (+ My Presets) before each read. If the device treats
    // connection_request as a session reset, a whole-library scan needs only
    // ONE connection — no open/close churn (the congestion gotcha).
    out += "\n── Exp E: connection_request re-arm per read, ONE connection ──\n";
    {
        let t0 = std::time::Instant::now();
        let first_req = proto::preset_data_request(1, slots[0] as u64, None);
        match Session::connect_slotread(SlotReadBurst::Minimal, &first_req) {
            Ok(mut s) => {
                let reply = slotread_harvest(&mut s);
                out += &format!(
                    "  ({:.2}s){}",
                    t0.elapsed().as_secs_f64(),
                    slotread_report("E", slots[0], &name_of(slots[0]), reply.as_deref(), &s)
                );
                for &slot in &slots[1..] {
                    let t = std::time::Instant::now();
                    s.raw.clear();
                    s.send_and_collect(&proto::connection_request(), 100)?;
                    s.send_and_collect(&proto::preset_list_request(1, 1), 20)?;
                    s.send_and_collect(&proto::preset_data_request(1, slot as u64, None), 200)?;
                    let reply = slotread_harvest(&mut s);
                    out += &format!(
                        "  ({:.2}s){}",
                        t.elapsed().as_secs_f64(),
                        slotread_report("E", slot, &name_of(slot), reply.as_deref(), &s)
                    );
                }
            }
            Err(e) => out += &format!("  [E] connect FAILED: {e}\n"),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    for (tag, variant) in [
        ("C-early", SlotReadBurst::Early),
        ("C-minimal", SlotReadBurst::Minimal),
        ("A-classic", SlotReadBurst::Classic),
    ] {
        out += &format!("\n── Exp {tag}: in-burst read, {variant:?} burst ──\n");
        for &slot in &slots {
            let req = proto::preset_data_request(1, slot as u64, None);
            match Session::connect_slotread(variant, &req) {
                Ok(mut s) => {
                    let reply = slotread_harvest(&mut s);
                    out += &slotread_report(tag, slot, &name_of(slot), reply.as_deref(), &s);
                }
                Err(e) => out += &format!("  [{tag}] slot {slot}: connect FAILED: {e}\n"),
            }
            // Seize-recycle settle between fresh connections.
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    }
    Ok(out)
}

/// HW validation for the live-session field-8 read (`probe --slotread-live <slot> [rounds]`).
/// Warms a dense heartbeat → live-controller status, then compares the two shipped
/// reads on that session: `read_slot_preset_json` (sends the `connection_request`
/// re-arm) vs `read_slot_preset_json_live` (skips it — the monitor's path), counting
/// inbound `connectionError` frames + field-9 successes + wall-time per read.
/// HW result: the re-arm path draws 1 `connectionError`/read and runs ~140 ms slower;
/// the live path is 0 errors, both return the same field-9. NON-DESTRUCTIVE — zero
/// LoadPreset, no re-amp.
pub fn probe_slotread_live(device_slot: u32, rounds: u32) -> Result<String, String> {
    // connectionError = ConnectionMessage (TMS field 4) → inner field 3.
    fn is_connection_error(body: &[u8]) -> bool {
        proto::first_bytes(&proto::parse(body), 4)
            .map(|cm| proto::parse(cm).first().map(|(g, _)| *g) == Some(3))
            .unwrap_or(false)
    }
    let run = |label: &str, live: bool| -> Result<String, String> {
        let mut s = Session::connect()?;
        // Warm a Pro-Control-style dense heartbeat → live-controller status.
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        let mut out = format!("── {label} on a warmed dense-heartbeat live session ──\n");
        for r in 0..rounds {
            let t0 = std::time::Instant::now();
            let res = if live {
                s.read_slot_preset_json_live(device_slot)?
            } else {
                s.read_slot_preset_json(device_slot)?
            };
            // connectionError frames that arrived DURING this read (raw cleared on entry).
            let errs = s
                .push_bodies()
                .iter()
                .filter(|b| is_connection_error(b))
                .count();
            out += &format!(
                "  read {r}: {} field-9 ({}B), {errs} connectionError, {:?}\n",
                if res.is_some() { "GOT " } else { "MISS" },
                res.as_ref().map(|b| b.len()).unwrap_or(0),
                t0.elapsed(),
            );
        }
        Ok(out)
    };

    let mut out = format!(
        "[slotread-live] device slot {device_slot}, {rounds} rounds — field-8 on a LIVE session\n\n"
    );
    out += &run(
        "WITH connection_request re-arm (read_slot_preset_json)",
        false,
    )?;
    std::thread::sleep(std::time::Duration::from_millis(800));
    out += &run("LIVE, no re-arm (read_slot_preset_json_live)", true)?;
    Ok(out)
}

/// Retained passive-scene re-validation probe: minimal connect, then a
/// NON-DESTRUCTIVE field-8 read per non-empty preset (connection_request re-arm,
/// one connection, zero LoadPreset). The UI no longer runs this eagerly; it uses
/// `read_preset_scenes` lazily per selected preset. Compare against `probe --scenes`
/// (the destructive LoadPreset→125 benchmark) for parity.
pub fn probe_scan_scenes_passive() -> Result<String, String> {
    use std::time::Instant;
    let overall = Instant::now();
    let mut s = Session::connect()?;
    let presets = s.list_my_presets()?;
    // Drain the handshake flood before the first re-armed read (a read fired
    // mid-flood is dropped device-side — the classic 0/25).
    s.drain_until_quiet(250, 20)?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let mut out = format!(
        "[scenes-passive] {} presets — field-8 slot reads, NO LoadPreset\n",
        non_empty.len()
    );
    let (mut ok, mut missed) = (0u32, 0u32);
    for p in &non_empty {
        let t0 = Instant::now();
        match s.read_slot_preset_json(p.slot + 1)? {
            Some(json) => {
                ok += 1;
                let names = session::scene_names_from_slot_json(&json);
                let desc = match &names {
                    Some(n) if n.is_empty() => "(no scenes)".to_string(),
                    Some(n) => format!("{} scenes: {}", n.len(), n.join(", ")),
                    None => format!("(scenes unknown — partial cut early, {}B)", json.len()),
                };
                out += &format!(
                    "  {:>3}  {:34}  {desc}  {:.2}s\n",
                    p.slot,
                    p.name,
                    t0.elapsed().as_secs_f64()
                );
            }
            None => {
                missed += 1;
                out += &format!(
                    "  {:>3}  {:34}  NO REPLY  {:.2}s\n",
                    p.slot,
                    p.name,
                    t0.elapsed().as_secs_f64()
                );
            }
        }
    }
    out += &format!(
        "\n[scenes-passive] {ok}/{} OK, {missed} unanswered | {:.1}s total, {:.2}s avg\n",
        non_empty.len(),
        overall.elapsed().as_secs_f64(),
        overall.elapsed().as_secs_f64() / non_empty.len().max(1) as f64,
    );
    Ok(out)
}

/// POC: LoadPreset → sceneListResponse(125) loop on a single heartbeat session.
/// One handshake, then rapid LoadPreset + harvest scene names per slot.
pub fn probe_scan_scenes_load() -> Result<String, String> {
    use std::time::Instant;

    let presets = probe_connect_and_list()?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let to_scan = non_empty.len();

    let mut s = Session::connect()?;
    // Sustain dense heartbeats for ~2s to enter "live controller" mode — the
    // device only pushes unsolicited data (sceneListResponse, PresetLoaded) on
    // a session with sustained heartbeat cadence.
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }

    let mut out =
        format!("[scenes-load] {to_scan} presets — LoadPreset → sceneList(125) on live session\n");
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;
    let overall_start = Instant::now();

    for p in &non_empty {
        let t0 = Instant::now();
        s.raw.clear();
        // LoadPreset via send_and_collect (not load_preset which discards
        // the HID reports — the sceneListResponse push would be lost).
        s.send_and_collect(&proto::load_preset((p.slot + 1) as u64, 1), 300)?;
        // Pump for the unsolicited sceneListResponse(125) push.
        let mut scenes: Option<Vec<String>> = None;
        let mut seen = 0usize;
        for _ in 0..8 {
            s.pump_collect(150)?;
            let bodies = s.push_bodies();
            for b in bodies.iter().skip(seen) {
                if let Some(names) = session::decode_scene_list(b) {
                    scenes = Some(names);
                    break;
                }
            }
            seen = bodies.len();
            if scenes.is_some() {
                break;
            }
        }
        if scenes.is_none() {
            s.raw.clear();
            let _ = s.send_and_collect(&proto::scene_list_request(), 300);
            for _ in 0..4 {
                let bodies = s.push_bodies();
                if let Some(names) = bodies.iter().find_map(|b| session::decode_scene_list(b)) {
                    scenes = Some(names);
                    break;
                }
                let _ = s.pump_collect(200);
            }
        }
        let elapsed = t0.elapsed();
        match scenes {
            Some(names) => {
                ok_count += 1;
                if names.is_empty() {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  (no scenes)  {:.2}s\n",
                        p.slot,
                        p.name,
                        elapsed.as_secs_f64(),
                    ));
                } else {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  {} scenes: {}  {:.2}s\n",
                        p.slot,
                        p.name,
                        names.len(),
                        names.join(", "),
                        elapsed.as_secs_f64(),
                    ));
                }
            }
            None => {
                fail_count += 1;
                out.push_str(&format!(
                    "  {:>3}  {:34}  FAIL  {:.2}s\n",
                    p.slot,
                    p.name,
                    elapsed.as_secs_f64(),
                ));
            }
        }
        // Keep alive.
        let _ = s.heartbeat();
    }

    let total_elapsed = overall_start.elapsed();
    let avg = if ok_count + fail_count > 0 {
        total_elapsed.as_secs_f64() / (ok_count + fail_count) as f64
    } else {
        0.0
    };
    out.push_str(&format!(
        "\n[scenes-load] {ok_count}/{to_scan} OK, {fail_count} failed | {:.1}s total, {:.2}s avg\n",
        total_elapsed.as_secs_f64(),
        avg,
    ));
    Ok(out)
}

/// Fast full scene scan: LoadPreset on a live session → harvest the field-3
/// `currentPresetDataChanged` push (~17KB JSON with scenes, ftsw, audioGraph).
/// Same speed as `--scenes` (~0.5s/preset) but with full block details.
/// Changes the active preset on the device.
pub fn probe_scan_scenes_full_live() -> Result<String, String> {
    use std::time::Instant;

    let presets = probe_connect_and_list()?;
    let non_empty: Vec<_> = presets
        .iter()
        .filter(|p| !session::is_empty_slot_name(&p.name))
        .cloned()
        .collect();
    let to_scan = non_empty.len();

    let mut s = Session::connect()?;
    for _ in 0..16 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }

    let mut out = format!(
        "[scenes-full-live] {to_scan} presets — LoadPreset → field-3 currentPresetDataChanged\n"
    );
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;
    let overall_start = Instant::now();

    for p in &non_empty {
        let t0 = Instant::now();
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((p.slot + 1) as u64, 1), 300)?;
        let mut live: Option<session::CurrentPresetLive> = None;
        let mut seen = 0usize;
        for _ in 0..12 {
            s.pump_collect(150)?;
            let bodies = s.push_bodies();
            for b in bodies.iter().skip(seen) {
                if let Some(l) = session::decode_current_preset_live(b) {
                    if l.scene_names.is_some() || l.graph.is_some() {
                        live = Some(l);
                        break;
                    }
                }
            }
            seen = bodies.len();
            if live.is_some() {
                break;
            }
        }
        let elapsed = t0.elapsed();
        match live {
            Some(l) => {
                ok_count += 1;
                let scenes = l.scene_names.as_deref().unwrap_or(&[]);
                let has_ftsw = l.ftsw.is_some();
                let has_graph = l.graph.is_some();
                if scenes.is_empty() {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  (no scenes)  ftsw={} graph={}  {:.2}s\n",
                        p.slot,
                        p.name,
                        has_ftsw,
                        has_graph,
                        elapsed.as_secs_f64(),
                    ));
                } else {
                    out.push_str(&format!(
                        "  {:>3}  {:34}  {} scenes: {}  ftsw={} graph={}  {:.2}s\n",
                        p.slot,
                        p.name,
                        scenes.len(),
                        scenes.join(", "),
                        has_ftsw,
                        has_graph,
                        elapsed.as_secs_f64(),
                    ));
                }
            }
            None => {
                fail_count += 1;
                out.push_str(&format!(
                    "  {:>3}  {:34}  FAIL  {:.2}s\n",
                    p.slot,
                    p.name,
                    elapsed.as_secs_f64(),
                ));
            }
        }
        let _ = s.heartbeat();
    }

    let total_elapsed = overall_start.elapsed();
    let avg = if ok_count + fail_count > 0 {
        total_elapsed.as_secs_f64() / (ok_count + fail_count) as f64
    } else {
        0.0
    };
    out.push_str(&format!(
        "\n[scenes-full-live] {ok_count}/{to_scan} OK, {fail_count} failed | {:.1}s total, {:.2}s avg\n",
        total_elapsed.as_secs_f64(), avg,
    ));
    Ok(out)
}

/// Probe (AC3): re-import a `.preset` file to the device over USB and
/// report where it landed. Reads the raw file bytes (the OFFLINE codec's XOR'd
/// output), sends the chunked `importPresetRequest`, and re-lists "My Presets" to
/// show the change. The device chooses the slot; this is additive (creates a new
/// user preset) — clear it afterward to restore state.
pub fn probe_import_preset(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    // Baseline list on its own session, then import on a second, then re-list on a
    // third fresh session — the post-import re-list must NOT reuse the importing
    // session's buffer (the chunked send pollutes its accumulated reports).
    let before = Session::connect()?.list_my_presets()?;
    let before_pairs: std::collections::HashSet<(u32, String)> =
        before.iter().map(|p| (p.slot, p.name.clone())).collect();

    let resp = Session::connect()?.import_preset(&bytes)?;

    let after = Session::connect()?.list_my_presets()?;
    let changed: Vec<String> = after
        .iter()
        .filter(|p| !before_pairs.contains(&(p.slot, p.name.clone())))
        .map(|p| format!("slot {} = {:?}", p.slot, p.name))
        .collect();
    let resp_str = match resp {
        Some((le, slot)) => format!("importPresetResponse: listEnum={le} presetSlot={slot}"),
        None => {
            "no importPresetResponse echo (device may not reply — verify via the slot diff)".into()
        }
    };
    Ok(format!(
        "[probe --import] file={path} raw_bytes={}\n{resp_str}\n\
         My Presets: before={} slots, after={} slots\n\
         slots whose name changed after import: {changed:?}\n",
        bytes.len(),
        before.len(),
        after.len(),
    ))
}

/// Probe: clear (delete) a user preset slot — `clearUserPreset` (AC4 setter).
/// Used to undo a `--import` test and to exercise the setter on hardware. Guarded:
/// only clears a slot that currently reads `expect_name` (pass the imported name,
/// e.g. "Guitar") so a mistyped slot can't nuke an unrelated preset.
pub fn probe_clear_preset(slot: u32, expect_name: &str) -> Result<String, String> {
    let before = Session::connect()?.list_my_presets()?;
    let cur = before
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    if cur.as_deref() != Some(expect_name) {
        return Ok(format!(
            "[probe --clear] slot {slot} reads {cur:?}, not {expect_name:?} — refused (no change)\n"
        ));
    }
    Session::connect()?.clear_user_preset(slot)?;
    let after = Session::connect()?.list_my_presets()?;
    let now = after
        .iter()
        .find(|p| p.slot == slot)
        .map(|p| p.name.clone());
    Ok(format!(
        "[probe --clear] slot {slot} was {expect_name:?}; cleared → now reads {now:?}\n"
    ))
}

/// Read a Song's preset assignments. Song reads ride the handshake burst AND
/// need a top-level `batchStatus` (like preset *reads*; unlike setters, which omit
/// it) — a no-batch or standalone request gets no reply (HW-confirmed 1.7.75, via
/// a batchStatus sweep). The accepted batch tracks the burst's active group, so we
/// sweep a few and take the first that yields records. Empty Vec = the song is
/// genuinely empty.
fn read_song_presets(song_slot: u32) -> Result<Vec<session::SongPresetRecord>, String> {
    for batch in [2u64, 3, 1, 4] {
        let req = proto::song_preset_list_request(song_slot as u64, Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..6 {
            let r = s.harvest_song_presets();
            if !r.is_empty() {
                return Ok(r);
            }
            s.pump_more(400)?;
        }
    }
    Ok(Vec::new())
}

/// Probe (AC7 verification): read a Song's preset rows over USB. Prints each
/// row's `userPresetSlot` (the device slot the Song points at — the positional
/// binding an in-place edit must preserve) and `presetSceneSlot`. Used as the
/// baseline before an edit and the check after.
pub fn probe_song_presets(song_slot: u32) -> Result<String, String> {
    let recs = read_song_presets(song_slot)?;
    if recs.is_empty() {
        return Ok(format!(
            "[probe --songpresets] song {song_slot}: no rows \
             (empty song, or songPresetListResponse(13) unsupported on this firmware)\n"
        ));
    }
    let mut out = format!(
        "[probe --songpresets] song {song_slot}: {} row(s)\n",
        recs.len()
    );
    for (i, r) in recs.iter().enumerate() {
        if r.is_empty {
            out += &format!("  row {i}: (empty)\n");
        } else {
            out += &format!(
                "  row {i}: userPresetSlot={} presetSceneSlot={} scene={:?}\n",
                r.user_preset_slot, r.preset_scene_slot, r.preset_scene_name
            );
        }
    }
    Ok(out)
}

/// Read every Song's metadata (name / notes / BPM) — the net-new live
/// `songListResponse` read. Song reads ride the handshake burst AND need a
/// top-level `batchStatus` (same constraint as [`read_song_presets`]), so sweep a
/// few batch values and take the first that yields records. Empty Vec = no songs.
fn read_song_list() -> Result<Vec<session::SongRecord>, String> {
    // FAIL-CLOSED: a single multi-packet read can be tail-truncated by concurrent
    // device streams (reassemble_streams can't demux concurrent 0x33 streams), so
    // accept ONLY a strictly-complete response (`harvest_songs_strict`), retrying
    // independent reads until one lands. See the reassembly review.
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::song_list_request(Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_songs_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err("could not read a complete song list (multi-packet response kept truncating)".into())
}

/// Read every Setlist's name — the net-new live `setlistListResponse` read. Same
/// in-burst + `batchStatus`-sweep contract as [`read_song_list`] (setlist reads ride
/// the handshake burst). A `SetlistListRecord` is name-only; per-setlist song
/// membership is a separate read. Empty Vec = no setlists defined.
fn read_setlist_list() -> Result<Vec<session::SetlistRecord>, String> {
    // Fail-closed, like read_song_list: accept only a strictly-complete
    // setlistListResponse, retrying until one lands (the multi-packet response
    // truncates non-deterministically, worse right after a write).
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::setlist_list_request(Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlists_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err("could not read a complete setlist list (multi-packet response kept truncating)".into())
}

/// Probe (`--songs`): list every Song on the device with its notes and BPM.
/// Read-only; prints one line per song.
pub fn probe_list_songs() -> Result<String, String> {
    let songs = read_song_list()?;
    if songs.is_empty() {
        return Ok("[probe --songs] no songs on the device \
                   (none defined, or songListResponse(3) unsupported on this firmware)\n"
            .to_string());
    }
    let mut out = format!("[probe --songs] {} song(s):\n", songs.len());
    for s in &songs {
        let bpm = if s.bpm_active {
            format!("{} BPM", s.bpm)
        } else {
            format!("{} BPM (off)", s.bpm)
        };
        let notes = if s.notes.is_empty() {
            "—".to_string()
        } else {
            format!("{:?}", s.notes)
        };
        out += &format!(
            "  {:>2}. {:<28}  {:<14}  notes: {}\n",
            s.slot, s.name, bpm, notes
        );
    }
    Ok(out)
}

/// Probe (`--setlists`): list every Setlist on the device (name only — per-setlist
/// song membership is a separate read). Read-only; prints one line per setlist.
pub fn probe_list_setlists() -> Result<String, String> {
    let setlists = read_setlist_list()?;
    if setlists.is_empty() {
        return Ok("[probe --setlists] no setlists on the device \
                   (none defined, or setlistListResponse(3) unsupported on this firmware)\n"
            .to_string());
    }
    let mut out = format!("[probe --setlists] {} setlist(s):\n", setlists.len());
    for l in &setlists {
        out += &format!("  {:>2}. {}\n", l.slot, l.name);
    }
    Ok(out)
}

/// Read the RAW song slots a Setlist contains — `setlistSongListResponse` records,
/// each a `songSlot`, INCLUDING trailing `songSlot==0` padding (unassigned slots).
/// Same in-burst + `batchStatus`-sweep contract as the other song/setlist reads.
/// Most callers want [`read_setlist_songs`] (dense); only the HW-characterization
/// probe needs the raw padding to count empties.
fn read_setlist_songs_raw(setlist_slot: u32) -> Result<Vec<u32>, String> {
    // Fail-closed strict read; a complete response for an empty setlist is Some(vec![]).
    for attempt in 0..10 {
        let batch = [2u64, 3, 1, 4][attempt % 4];
        let req = proto::setlist_song_list_request(setlist_slot as u64, Some(batch));
        let mut s = Session::connect_with_burst_request(&req)?;
        for _ in 0..8 {
            if let Some(r) = s.harvest_setlist_songs_strict() {
                return Ok(r);
            }
            s.pump_more(250)?;
        }
    }
    Err(format!(
        "could not read a complete song list for setlist slot {setlist_slot}"
    ))
}

/// A setlist's member song slots in device order, DENSE — trailing `songSlot==0`
/// padding (unassigned slots) dropped. This is the membership every command + the UI
/// consume, so the "songSlot 0 = not a member" invariant lives HERE, once, rather
/// than re-applied at each call site. Empty Vec = an empty setlist.
fn read_setlist_songs(setlist_slot: u32) -> Result<Vec<u32>, String> {
    Ok(read_setlist_songs_raw(setlist_slot)?
        .into_iter()
        .filter(|&s| s != 0)
        .collect())
}

/// Probe (`--setlist-songs`): for every Setlist on the device, list the songs it
/// contains, resolving each referenced `songSlot` against the song list. Read-only.
/// Each line shows the raw `songSlot` next to the resolved name so any slot-base
/// mismatch is self-evident rather than silently mislabeled.
pub fn probe_setlist_songs() -> Result<String, String> {
    let songs = read_song_list()?;
    let setlists = read_setlist_list()?;
    if setlists.is_empty() {
        return Ok("[probe --setlist-songs] no setlists on the device\n".to_string());
    }
    let name_for = |slot: u32| -> String {
        songs
            .iter()
            .find(|s| s.slot == slot)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("(unknown song slot {slot})"))
    };
    let mut out = format!("[probe --setlist-songs] {} setlist(s):\n", setlists.len());
    for l in &setlists {
        // songSlot is 1-based (matches the song list); a returned songSlot==0 is a
        // trailing empty/unassigned slot, not a song — count + label it separately.
        // This diagnostic wants the RAW list to report the padding count; everyone
        // else uses the dense `read_setlist_songs`.
        let raw = read_setlist_songs_raw(l.slot)?;
        let real: Vec<u32> = raw.iter().copied().filter(|&s| s != 0).collect();
        let empties = raw.len() - real.len();
        out += &format!("\n  {}. {}  ({} song(s))\n", l.slot, l.name, real.len());
        if real.is_empty() {
            out += "     (empty)\n";
        } else {
            for (i, slot) in real.iter().enumerate() {
                out += &format!(
                    "     {:>2}. [songSlot {}] {}\n",
                    i + 1,
                    slot,
                    name_for(*slot)
                );
            }
        }
        if empties > 0 {
            out += &format!("     (+ {empties} empty/unassigned slot(s) returned as songSlot 0)\n");
        }
    }
    Ok(out)
}

// ─── Song / Setlist WRITE primitives (name-addressed, read-confirmed) ─────────
// All look up the target by EXACT NAME in a fresh read (avoiding slot off-by-one),
// refuse on no-match / duplicate-name ambiguity, mutate, then re-read to verify.

/// Resolve a song's 1-based slot by exact name. Errors on no-match or ambiguity.
fn find_song_slot(songs: &[session::SongRecord], name: &str) -> Result<u32, String> {
    let hits: Vec<u32> = songs
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.slot)
        .collect();
    match hits.len() {
        0 => Err(format!("no song named {name:?} on the device")),
        1 => Ok(hits[0]),
        n => Err(format!(
            "{n} songs named {name:?} — ambiguous, refusing to mutate"
        )),
    }
}

/// Resolve a setlist's 1-based slot by exact name. Errors on no-match or ambiguity.
fn find_setlist_slot(setlists: &[session::SetlistRecord], name: &str) -> Result<u32, String> {
    let hits: Vec<u32> = setlists
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.slot)
        .collect();
    match hits.len() {
        0 => Err(format!("no setlist named {name:?} on the device")),
        1 => Ok(hits[0]),
        n => Err(format!(
            "{n} setlists named {name:?} — ambiguous, refusing to mutate"
        )),
    }
}

fn song_line(r: &session::SongRecord) -> String {
    let bpm = if r.bpm_active {
        format!("{} BPM", r.bpm)
    } else {
        format!("{} BPM (off)", r.bpm)
    };
    let notes = if r.notes.is_empty() {
        "—".to_string()
    } else {
        format!("{:?}", r.notes)
    };
    format!(
        "  slot {:>2}  {:<14}  [{}]  notes: {}",
        r.slot, r.name, bpm, notes
    )
}

/// `--add-song <name>` — create a song, verify it appears.
pub fn probe_add_song(name: &str) -> Result<String, String> {
    {
        let mut s = Session::connect()?;
        s.add_song(name)?;
    }
    let songs = read_song_list()?;
    let found = songs.iter().find(|x| x.name == name);
    let mut out = format!(
        "[probe --add-song] add {name:?} → {} ({} songs total)\n",
        if found.is_some() {
            "FOUND ✓"
        } else {
            "NOT FOUND ✗"
        },
        songs.len()
    );
    if let Some(r) = found {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// `--add-setlist <name>` — create a setlist, verify it appears.
///
/// `addSetlist` works from a bare connection (the earlier "failures" were a
/// truncating multi-packet READ, not a write problem — see [`read_setlist_list`]).
pub fn probe_add_setlist(name: &str) -> Result<String, String> {
    {
        let mut s = Session::connect()?;
        s.add_setlist(name)?;
    }
    let setlists = read_setlist_list()?;
    let found = setlists.iter().find(|x| x.name == name);
    Ok(format!(
        "[probe --add-setlist] add {name:?} → {} ({} setlists total)\n",
        if found.is_some() {
            "FOUND ✓"
        } else {
            "NOT FOUND ✗"
        },
        setlists.len()
    ))
}

/// `--add-setlist-song <setlist-name> <song-name>` — add the song to the setlist
/// (by resolved slots), verify membership.
pub fn probe_add_setlist_song(setlist_name: &str, song_name: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let setlists = read_setlist_list()?;
    let song_slot = find_song_slot(&songs, song_name)?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    {
        let mut s = Session::connect()?;
        s.add_setlist_song(setlist_slot, song_slot)?;
    }
    let members = read_setlist_songs(setlist_slot)?;
    let present = members.contains(&song_slot);
    Ok(format!(
        "[probe --add-setlist-song] {song_name:?} (songSlot {song_slot}) → {setlist_name:?} (setlistSlot {setlist_slot}) → {}\n  members now: {:?}\n",
        if present { "PRESENT ✓" } else { "NOT PRESENT ✗" }, members
    ))
}

/// `--rename-song <old> <new>` — rename (resolved by old name), verify.
pub fn probe_rename_song(old: &str, new: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, old)?;
    {
        let mut s = Session::connect()?;
        s.rename_song(slot, new)?;
    }
    let after = read_song_list()?;
    let ok = after.iter().any(|x| x.slot == slot && x.name == new);
    let old_gone = !after.iter().any(|x| x.name == old);
    let mut out = format!(
        "[probe --rename-song] slot {slot} {old:?} → {new:?}: {} (old-name-gone={})\n",
        if ok { "OK ✓" } else { "FAILED ✗" },
        old_gone
    );
    if let Some(r) = after.iter().find(|x| x.slot == slot) {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// `--rename-setlist <old> <new>` — rename (resolved by old name), verify.
pub fn probe_rename_setlist(old: &str, new: &str) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let slot = find_setlist_slot(&setlists, old)?;
    {
        let mut s = Session::connect()?;
        s.rename_setlist(slot, new)?;
    }
    let after = read_setlist_list()?;
    let ok = after.iter().any(|x| x.slot == slot && x.name == new);
    Ok(format!(
        "[probe --rename-setlist] slot {slot} {old:?} → {new:?}: {} (now: {:?})\n",
        if ok { "OK ✓" } else { "FAILED ✗" },
        after.iter().map(|x| x.name.clone()).collect::<Vec<_>>()
    ))
}

/// `--song-notes <name> <notes>` — set a song's notes, verify.
pub fn probe_set_song_notes(name: &str, notes: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?;
    {
        let mut s = Session::connect()?;
        s.set_song_notes(slot, notes)?;
    }
    let after = read_song_list()?;
    let r = after.iter().find(|x| x.slot == slot);
    let ok = r.map(|x| x.notes == notes).unwrap_or(false);
    let mut out = format!(
        "[probe --song-notes] slot {slot} {name:?} notes → {notes:?}: {}\n",
        if ok { "OK ✓" } else { "MISMATCH ✗" }
    );
    if let Some(r) = r {
        out += &(song_line(r) + "\n");
    }
    Ok(out)
}

/// The RE'd per-song BPM ritual (there is NO dedicated BPM setter — BPM is the global
/// `tapTempoBpm` applied to the ACTIVE song, so the song must be activated via a
/// footswitch). **Non-destructive:** if the song already has a footswitch binding we
/// activate THAT one and never overwrite it; we only `assignSongPreset` (purely
/// additively — nothing to clobber) when the song has no footswitch at all. (The
/// device does not echo a binding's footswitch label/color on read, so re-asserting
/// an existing binding could only blank them — hence we never re-write it.) Then
/// retry ≤5× { `load_song` + `tapTempoBpm` + enable BPM display + read-back-verify
/// within ±1.5 } — the first load often doesn't settle. Finally **restore the prior
/// active preset** so editing BPM doesn't leave the amp on the song's footswitch
/// preset. Returns `(fresh_song_list, converged)`; `converged == false` means the
/// retries were exhausted (the caller decides whether that's an error). `Err` only on
/// an actual connection/transact failure. Shared by the `set_song_bpm` command and
/// `probe_set_song_bpm` so this fragile, HW-derived flow lives once.
fn converge_song_bpm(slot: u32, bpm: f32) -> Result<(Vec<session::SongRecord>, bool), String> {
    // Read the currently-active preset (to restore afterward) + the song's existing
    // footswitch bindings (to avoid clobbering one). Both best-effort reads.
    let prior_active = discover_active_graph().ok().and_then(|(g, _)| g.slot);

    // FAIL-CLOSED read of the footswitch bindings. `read_song_presets` returns an
    // EMPTY Vec only when it got NO usable response (a genuinely footswitch-less song
    // still returns its all-empty row set), so an empty result = we couldn't read the
    // bindings authoritatively → refuse rather than additively assign over a binding
    // we just failed to see. (A truncated record defaults `is_empty=false`, so a
    // partial read can never FALSELY report a bound position as empty — it errs
    // toward "activate the existing binding", never toward clobbering.)
    let rows = read_song_presets(slot)?;
    if rows.is_empty() {
        return Err(
            "could not read this song's footswitch bindings — refusing to set \
                    BPM to avoid overwriting a footswitch preset (load the song on the \
                    unit and retry)"
                .into(),
        );
    }
    let existing = rows.iter().enumerate().find(|(_, r)| !r.is_empty);

    // 1) Resolve the footswitch to activate. Existing binding → use it untouched;
    //    authoritatively none → assign position 1 additively (the original behavior,
    //    only when there's nothing to overwrite). `user_preset_slot` is the device
    //    1-based slot already stored in the binding, which `load_song.presetSlot` wants.
    let (fs_pos, fs_preset_slot) = match existing {
        Some((i, r)) => ((i + 1) as u32, r.user_preset_slot),
        None => {
            let mut s = Session::connect()?;
            s.assign_song_preset(slot, 1, 0, "", 0, 1)?;
            (1u32, 1u32)
        }
    };

    // 2) Activate + set tempo, retry until the read-back lands within ±1.5.
    let mut last = Vec::new();
    let mut converged = false;
    for _ in 0..5 {
        {
            let mut s = Session::connect()?;
            s.load_song(slot, fs_pos, fs_preset_slot)?;
            s.set_tap_tempo_bpm(bpm)?;
            s.set_song_bpm_active(slot, true)?;
        }
        last = read_song_list()?;
        if let Some(r) = last.iter().find(|x| x.slot == slot) {
            if (r.bpm as f32 - bpm).abs() < 1.5 {
                converged = true;
                break;
            }
        }
    }

    // 3) Restore the prior active preset so the live tone returns to what it was
    //    (best-effort — only if we could read it).
    if let Some(prior) = prior_active {
        if let Ok(mut s) = Session::connect() {
            let _ = s.load_preset(prior);
        }
    }

    Ok((last, converged))
}

/// `--song-bpm <name> <bpm>` — set a song's numeric BPM via [`converge_song_bpm`]
/// (resolve slot by name, then run the shared ritual). Verify by re-read.
pub fn probe_set_song_bpm(name: &str, bpm: f32) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?;
    let (after, converged) = converge_song_bpm(slot, bpm)?;
    let r = after.iter().find(|x| x.slot == slot);
    let got = r.map(|x| x.bpm).unwrap_or(0);
    if converged {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: OK ✓ (read-back={got})\n  {}\n",
            r.map(song_line).unwrap_or_default()
        ))
    } else {
        Ok(format!(
            "[probe --song-bpm] {name:?} BPM → {bpm}: read-back={got} — did NOT converge after retries \
             (active-song targeting may differ for this slot)\n"
        ))
    }
}

/// `--remove-song <name>` — DELETE a song resolved by exact name (guard: refuses on
/// no-match / ambiguity, so the slot used for deletion is the one that reads as `name`).
pub fn probe_remove_song(name: &str) -> Result<String, String> {
    let songs = read_song_list()?;
    let slot = find_song_slot(&songs, name)?; // guard in the same (read) space we will delete
    {
        let mut s = Session::connect()?;
        s.remove_song(slot)?;
    }
    let after = read_song_list()?;
    let gone = !after.iter().any(|x| x.name == name);
    Ok(format!(
        "[probe --remove-song] delete slot {slot} {name:?}: {} ({} songs remain)\n",
        if gone {
            "GONE ✓"
        } else {
            "STILL PRESENT ✗"
        },
        after.len()
    ))
}

/// `--remove-setlists-named <name>` — DELETE every setlist with this exact name
/// (handles duplicates: find first match → remove its slot → re-read → repeat).
pub fn probe_remove_setlists_named(name: &str) -> Result<String, String> {
    let mut removed = 0;
    loop {
        let setlists = read_setlist_list()?;
        let Some(rec) = setlists.iter().find(|x| x.name == name) else {
            break;
        };
        let slot = rec.slot;
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        removed += 1;
        if removed > 30 {
            break; // safety bound
        }
    }
    let after = read_setlist_list()?;
    Ok(format!(
        "[probe --remove-setlists-named] removed {removed} setlist(s) named {name:?}; {} setlists remain\n",
        after.len()
    ))
}

/// `--remove-setlist <name>` — DELETE a setlist resolved by exact name (guarded).
pub fn probe_remove_setlist(name: &str) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let slot = find_setlist_slot(&setlists, name)?;
    {
        let mut s = Session::connect()?;
        s.remove_setlist(slot)?;
    }
    let after = read_setlist_list()?;
    let gone = !after.iter().any(|x| x.name == name);
    Ok(format!(
        "[probe --remove-setlist] delete slot {slot} {name:?}: {} ({} setlists remain)\n",
        if gone {
            "GONE ✓"
        } else {
            "STILL PRESENT ✗"
        },
        after.len()
    ))
}

/// `--remove-setlist-song <setlist-name> <position>` — remove the song at a given
/// POSITION within the setlist (setlist resolved by name). Prints before/after
/// membership so the 0-vs-1-based `setlistSongSlot` semantics are self-evident.
/// Used to HW-pin the index base before the UI relies on it.
pub fn probe_remove_setlist_song(setlist_name: &str, position: u32) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    let before = read_setlist_songs(setlist_slot)?;
    {
        let mut s = Session::connect()?;
        s.remove_setlist_song(setlist_slot, position)?;
    }
    let after = read_setlist_songs(setlist_slot)?;
    Ok(format!(
        "[probe --remove-setlist-song] {setlist_name:?} (slot {setlist_slot}) remove position {position}\n  \
         before: {before:?}\n  after:  {after:?}\n"
    ))
}

/// `--move-setlist-song <setlist-name> <old-pos> <new-pos>` — reorder a song within
/// a setlist by POSITION. Prints before/after membership (pins the move semantics).
pub fn probe_move_setlist_song(
    setlist_name: &str,
    old_pos: u32,
    new_pos: u32,
) -> Result<String, String> {
    let setlists = read_setlist_list()?;
    let setlist_slot = find_setlist_slot(&setlists, setlist_name)?;
    let before = read_setlist_songs(setlist_slot)?;
    {
        let mut s = Session::connect()?;
        s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
    }
    let after = read_setlist_songs(setlist_slot)?;
    Ok(format!(
        "[probe --move-setlist-song] {setlist_name:?} (slot {setlist_slot}) move {old_pos}→{new_pos}\n  \
         before: {before:?}\n  after:  {after:?}\n"
    ))
}

/// Diagnostic (`--diag-frames`): dump the raw inbound frame magic/len sequence for a
/// setlist-list read, so the multi-packet framing (0x33 start / 0x34 cont / 0x35 final)
/// and any interleaved foreign streams are visible, plus whether a strict decode lands.
pub fn probe_diag_frames() -> Result<String, String> {
    let req = proto::setlist_list_request(Some(2));
    let mut s = Session::connect_with_burst_request(&req)?;
    let mut out = String::from("[diag-frames] setlist read, per-pump frame magics:\n");
    for i in 0..6 {
        let strict = s.harvest_setlists_strict().map(|v| v.len());
        out += &format!(
            "  pump {i}: strict={strict:?}\n    frames: {}\n",
            s.raw_frame_summary()
        );
        s.pump_more(300)?;
    }
    Ok(out)
}

/// Diagnostic (`--diag-writes`): compare the device's REPLY to addSong (known-good)
/// vs addSetlist (silently failing) to distinguish an error reply from a silent
/// ignore, and whether setlist context changes the reply. Creates throwaway
/// "Diag*" entries (clean up after).
pub fn probe_diag_writes() -> Result<String, String> {
    let mut out = String::new();
    {
        let mut s = Session::connect()?;
        out += "[diag] addSong \"DiagSong\" (default context) reply:\n";
        out += &s.send_and_dump(&proto::add_song("DiagSong"), 800)?;
    }
    {
        let mut s = Session::connect()?;
        out += "[diag] addSetlist \"DiagBare\" reply:\n";
        out += &s.send_and_dump(&proto::add_setlist("DiagBare"), 800)?;
    }
    Ok(out)
}

/// Probe (AC7 prerequisite): report the data needed to understand the
/// list-index ↔ device-userSlot relationship.
///
/// `list_my_presets` returns 0-based list positions; the `userSlot` setters
/// (`saveCurrentPreset`/`clearUserPreset`/`moveUserPreset`) address a device
/// userSlot. `requestNextEmptyPresetSlot` (81→82) is **dead on 1.7.75**, so we
/// can't enumerate device empties; instead we report the list's `--` placeholders
/// and Song-1's `userPresetSlot`s. Correlating a Song row's `userPresetSlot` with
/// the list index of the preset assigned to it (you supply that preset) pins the
/// numbering: if they match, list index == device userSlot. The shipped leveller
/// already saves via list index, which is consistent with that.
pub fn probe_map_slots() -> Result<String, String> {
    let list = Session::connect()?.list_my_presets()?;
    let empty_list: Vec<u32> = list
        .iter()
        .filter(|p| session::is_empty_slot_name(&p.name))
        .map(|p| p.slot)
        .collect();
    let songs = read_song_presets(1).unwrap_or_default();
    let song_rows: Vec<(u32, u32)> = songs
        .iter()
        .filter(|r| !r.is_empty)
        .map(|r| (r.user_preset_slot, r.preset_scene_slot))
        .collect();

    let mut out = format!(
        "[probe --map-slots]\n\
         My Presets list entries: {}\n\
         empty ('--') list indices (first 12): {:?}\n\
         song-1 (userPresetSlot, presetSceneSlot) rows: {:?}\n",
        list.len(),
        empty_list.iter().take(12).collect::<Vec<_>>(),
        song_rows,
    );
    out += "note: requestNextEmptyPresetSlot (81→82) is dead on 1.7.75 — scratch slots are \
            found by observation (import then re-list), not prediction. To pin the \
            list↔userSlot numbering, assign a known preset to a song and compare its \
            list index with the userPresetSlot reported above.\n";
    Ok(out)
}

/// Probe (AC7 positive case): edit a preset IN PLACE on its original slot and
/// report whether the slot, its Song assignment, and scene binding survive.
/// Compare against `--import` (bare append) as the negative control — that one
/// lands the edit at a new slot and the Song row then points at the stale copy.
pub fn probe_replace_inplace(orig_list_index: u32, path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    run_replace_inplace(orig_list_index, &bytes, &format!("file={path}"))
}

/// Shared in-place-edit core (used by `--replace-inplace` and AC5 `--restore`).
///
/// Sequence: import (appends a scratch copy) → locate the scratch by re-listing
/// (`requestNextEmptyPresetSlot` is dead, so we OBSERVE where it landed) →
/// `load_preset(scratch)` → `save_current_preset(orig)` to overwrite the original
/// slot in place → **guarded** `clear_user_preset(scratch)`. All addresses are
/// 0-based list indices; `session.rs` translates each to the 1-based device
/// userSlot. The Song-1 link is read before/after to confirm the binding survives.
/// Structured result of the in-place edit core — what landed where, whether the
/// edit took, and whether the Song binding survived. Consumed by the `--replace-inplace`
/// / `--restore` probe formatter AND by `preset_io::OfflineIo::write`.
pub(crate) struct ReplaceOutcome {
    pub orig_list_index: u32,
    pub scratch_slot: u32,
    pub scratch_name: String,
    pub orig_name_before: String,
    pub orig_name_after: Option<String>,
    pub scratch_name_after: Option<String>,
    pub edit_landed: bool,
    pub had_binding: bool,
    pub binding_preserved: bool,
    pub songs_before: Vec<session::SongPresetRecord>,
    pub songs_after: Vec<session::SongPresetRecord>,
}

/// Reusable in-place-edit core (AC7): import a scratch copy → locate it by observing
/// which previously-empty slot filled → `load(scratch)` → `save_current_preset(orig)`
/// to overwrite the original slot → **guarded** `clear(scratch)` → re-read to confirm
/// the edit landed and the Song-1 binding survived. All addresses are 0-based list
/// indices; `session.rs` translates each to the 1-based device userSlot.
pub(crate) fn replace_inplace_core(
    orig_list_index: u32,
    bytes: &[u8],
) -> Result<ReplaceOutcome, String> {
    let before = Session::connect()?.list_my_presets()?;
    let orig_name_before = before
        .iter()
        .find(|p| p.slot == orig_list_index)
        .map(|p| p.name.clone())
        .ok_or_else(|| {
            format!(
                "orig list index {orig_list_index} out of range ({} entries)",
                before.len()
            )
        })?;
    // Slots that were EMPTY before — import will fill exactly one of these.
    let empty_before: std::collections::HashSet<u32> = before
        .iter()
        .filter(|p| session::is_empty_slot_name(&p.name))
        .map(|p| p.slot)
        .collect();
    let songs_before = read_song_presets(1).unwrap_or_default();

    // 1) Import — appends a scratch copy of the edited preset into an empty slot.
    Session::connect()?.import_preset(bytes)?;

    // 2) Observe where it landed: a slot that was EMPTY before and is now occupied.
    // Keying on the previously-empty set (not a name diff) means a flaky/partial
    // baseline list can't misidentify a *real* pre-existing preset as the scratch
    // and get it cleared in step 3.
    let after_import = Session::connect()?.list_my_presets()?;
    let (scratch_slot, scratch_name) = after_import
        .iter()
        .find(|p| empty_before.contains(&p.slot) && !session::is_empty_slot_name(&p.name))
        .map(|p| (p.slot, p.name.clone()))
        .ok_or_else(|| "could not locate the imported scratch preset (no previously-empty slot became occupied)".to_string())?;

    // 3) Land it on the original slot. The session layer translates these 0-based
    // list indices to the device's 1-based userSlot (HW-confirmed 1.7.75).
    Session::connect()?.load_preset(scratch_slot)?; // scratch becomes current (persists across reconnect)
                                                    // Fresh connection re-attaches to the now-current preset; CONFIRM it is the scratch
                                                    // copy BEFORE saving it over the (real, irreplaceable) original slot. A dropped load
                                                    // would leave a DIFFERENT preset current, and saving that over orig_list_index is
                                                    // silent data loss — so the guard lives in the SAME connection as the mutation. On
                                                    // failure ABORT before the save (and before the clear), leaving the scratch import on
                                                    // the device for manual recovery.
    let mut save_conn = Session::connect()?;
    save_conn
        .confirm_active(scratch_slot, Some(&scratch_name))
        .map_err(|e| {
            format!(
                "{e}. Left the scratch import at list index {scratch_slot} ({scratch_name:?}); \
                 the original slot {orig_list_index} was NOT modified."
            )
        })?;
    save_conn.save_current_preset(orig_list_index)?; // overwrite the original slot in place
    guarded_clear(scratch_slot, &scratch_name)?; // remove the scratch copy (guarded)

    // 4) Re-read and confirm slot / Song-link survival. Settle first: clear/save are
    // fire-and-forget (no ACK); give the device a moment or the read returns pre-clear state.
    std::thread::sleep(std::time::Duration::from_millis(800));
    let after = Session::connect()?.list_my_presets()?;
    let orig_name_after = after
        .iter()
        .find(|p| p.slot == orig_list_index)
        .map(|p| p.name.clone());
    let scratch_name_after = after
        .iter()
        .find(|p| p.slot == scratch_slot)
        .map(|p| p.name.clone());
    let songs_after = read_song_presets(1).unwrap_or_default();
    // A meaningful binding check needs a binding to begin with — equal-but-empty
    // (both reads returned no rows) is NOT evidence the link survived.
    let had_binding = songs_before.iter().any(|r| !r.is_empty);
    let binding_preserved = had_binding && songs_before == songs_after;
    let edit_landed = orig_name_after.as_deref() != Some(orig_name_before.as_str());

    Ok(ReplaceOutcome {
        orig_list_index,
        scratch_slot,
        scratch_name,
        orig_name_before,
        orig_name_after,
        scratch_name_after,
        edit_landed,
        had_binding,
        binding_preserved,
        songs_before,
        songs_after,
    })
}

/// String-reporting wrapper over [`replace_inplace_core`] for the probe subcommands.
fn run_replace_inplace(orig_list_index: u32, bytes: &[u8], src: &str) -> Result<String, String> {
    let o = replace_inplace_core(orig_list_index, bytes)?;
    let ac7 = if o.had_binding {
        format!(
            "AC7 PASS = edit_landed && binding_preserved = {}",
            o.edit_landed && o.binding_preserved
        )
    } else {
        format!(
            "AC7 = edit_landed={}; binding NOT CHECKED (song 1 has no rows to preserve)",
            o.edit_landed
        )
    };
    Ok(format!(
        "[probe in-place] {src}\n\
         orig list index = {}; scratch landed at list index {} ({:?})\n\
         orig name:    before={:?}  after={:?}  (edit_landed={})\n\
         scratch slot name after (expect cleared/'--'): {:?}\n\
         song-1 rows: before={} after={}  (had_binding={}, binding_preserved={})\n\
         {ac7}\n\
         songs_before={:?}\n\
         songs_after={:?}\n",
        o.orig_list_index,
        o.scratch_slot,
        o.scratch_name,
        o.orig_name_before,
        o.orig_name_after,
        o.edit_landed,
        o.scratch_name_after,
        o.songs_before.len(),
        o.songs_after.len(),
        o.had_binding,
        o.binding_preserved,
        o.songs_before,
        o.songs_after,
    ))
}

/// Clear the user preset at LIST index `list_index`, but only if that list entry
/// reads `expect_name`. The guard checks the slot in **list-index space** and then
/// clears the matching **device userSlot = list_index + 1** — so the verification
/// and the mutation address the *same* preset (the earlier guard bug checked the
/// list index but cleared a same-numbered device slot = a different preset).
fn guarded_clear(list_index: u32, expect_name: &str) -> Result<(), String> {
    let list = Session::connect()?.list_my_presets()?;
    let cur = list
        .iter()
        .find(|p| p.slot == list_index)
        .map(|p| p.name.as_str());
    if cur != Some(expect_name) {
        return Err(format!(
            "guarded clear refused: list index {list_index} reads {cur:?}, expected {expect_name:?}"
        ));
    }
    Session::connect()?.clear_user_preset(list_index) // session translates list → device slot
}

/// Probe (AC5): restore a backup snapshot to the device IN PLACE — onto the
/// snapshot's original slot, preserving its Song link. Reads the snapshot JSON,
/// validates it is a faithful offline backup (refuses `usb-partial` — re-importing
/// a partial would overwrite the slot with truncated data), re-XORs it to `.preset`
/// bytes, and routes through the AC7 in-place path. `snapshot.slot` is the list
/// index the backup was captured at.
pub fn probe_restore(snapshot_path: &str) -> Result<String, String> {
    let snap = backup::load_snapshot_from_path(std::path::Path::new(snapshot_path))?;
    let bytes = backup::restore_bytes(&snap)?;
    run_replace_inplace(
        snap.slot,
        &bytes,
        &format!("restore={snapshot_path} (slot {})", snap.slot),
    )
}

/// Lock a state mutex, recovering the guard if a previous holder panicked and poisoned it
/// (`into_inner`). These mutexes guard single-writer state (the session slot, the library,
/// the run registry, the monitor caches); recovery is always the right move — a poisoned
/// `unwrap()` would otherwise brick the always-running monitor or every future device op.
/// Used at every lock site across lib.rs / monitor.rs / watcher.rs.
pub(crate) fn lock_ok<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod lock_ok_tests {
    use super::lock_ok;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_a_poisoned_mutex_instead_of_panicking() {
        let m = Arc::new(Mutex::new(5));
        let m2 = Arc::clone(&m);
        // Poison the mutex: a thread panics while holding the lock.
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(m.lock().is_err(), "the mutex must be poisoned");
        // A plain .lock().unwrap() would panic here; lock_ok recovers the guard.
        assert_eq!(*lock_ok(&m), 5);
        *lock_ok(&m) = 9;
        assert_eq!(*lock_ok(&m), 9);
    }
}

/// Shared device session. `None` until the user connects. Behind an `Arc<Mutex>`
/// so blocking HID work can run off the UI thread via `spawn_blocking`.
#[derive(Default)]
struct AppState {
    session: Arc<Mutex<Option<Session>>>,
    /// The imported OFFLINE `.preset` library (None until `import_library`). The
    /// canonical full-preset source every bulk feature edits.
    library: Arc<Mutex<Option<library::Library>>>,
    /// Completed bulk runs, keyed by run_id, so `bulk_revert` can restore one.
    runs: Arc<Mutex<bulk_cmd::RunRegistry>>,
    /// Rendered audition clips, keyed by slot+topology, so re-auditioning
    /// skips the re-amp pass. Session-scoped (see `audition` module caveat).
    clip_cache: Arc<Mutex<audition::ClipCache>>,
}

#[derive(Serialize)]
struct AppInfo {
    name: String,
    version: String,
}

/// Frontend handshake on mount — confirms the backend is reachable.
#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "TMP Companion".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Result of a combined connect+discover handshake. The frontend receives both
/// the firmware version AND the active signal graph in one shot, eliminating the
/// separate `read_active_preset` round-trip that previously doubled the connect time.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectResult {
    firmware: Option<String>,
    graph: Option<session::ActiveGraph>,
}

/// Start the monitor-owned startup session and wait for its first snapshot. The
/// monitor's single handshake supplies firmware + preset list + (usually) the active
/// graph, so startup doesn't serialize separate HID sessions for connect/list/live.
///
/// IDEMPOTENT against an already-running monitor (the webview-reload case): when
/// `MONITOR_ENABLED` is already set, the live pump never re-runs its one-shot
/// handshake, so clearing the snapshot here would wait 8 s for a snapshot that can
/// never be re-produced. Instead the already-enabled path serves the cached
/// snapshot (its graph is kept current by `monitor::refresh_snapshot_graph` on
/// every field-3 push) — or, when the monitor is mid-connect/device-absent, polls
/// WITHOUT clearing so the monitor's own connect error / next snapshot surfaces.
#[tauri::command]
async fn connect_device(state: State<'_, AppState>) -> Result<ConnectResult, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ConnectResult, String> {
        if !MONITOR_ENABLED.load(SeqCst) {
            // Genuinely-disabled path (first connect / post-stop): fresh start.
            *lock_ok(&arc) = None;
            monitor::reset_startup_state();
            MONITOR_ENABLED.store(true, SeqCst);
        }
        // Shared poll: snapshot | monitor connect error | 8 s deadline. On the
        // already-enabled-with-snapshot path the first iteration returns at once.
        let t0 = std::time::Instant::now();
        let deadline = std::time::Duration::from_secs(8);
        loop {
            if let Some(snapshot) = monitor::startup_snapshot() {
                log::info!(
                    "connect_device: monitor snapshot in {} ms, firmware={:?}, graph={}",
                    t0.elapsed().as_millis(),
                    snapshot.firmware,
                    if snapshot.graph.is_some() {
                        "ok"
                    } else {
                        "none"
                    }
                );
                return Ok(ConnectResult {
                    firmware: snapshot.firmware,
                    graph: snapshot.graph,
                });
            }
            if let Some(e) = monitor::last_connect_error() {
                if !e.contains("no TMP found") && !e.contains("IOHIDDeviceSetReport failed") {
                    log::warn!("connect_device failed via monitor: {e}");
                }
                return Err(e);
            }
            if t0.elapsed() >= deadline {
                return Err("monitor startup timed out waiting for TMP handshake".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    })
    .await
    .map_err(|e| format!("connect task failed: {e}"))?
}

// (There is intentionally no `disconnect_device` command — the app has no manual
// Connect/Disconnect buttons, nothing invoked it, and a sync command that drops the
// seize can't be serialized by the device-op gate without risking a main-thread
// stall. The session is released by `with_released_seize` / `connect_device` only.)

/// Enumerate the device's "My Presets" list. Under the app-level monitor this is a
/// snapshot read (no HID, no device-op lock), so the list paints with connect. A
/// fresh-session fallback remains for monitor-disabled diagnostic contexts.
#[tauri::command]
async fn list_presets(state: State<'_, AppState>) -> Result<Vec<PresetEntry>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<PresetEntry>, String> {
        if MONITOR_ENABLED.load(SeqCst) {
            if let Some(snapshot) = monitor::startup_snapshot() {
                return Ok(snapshot.presets);
            }
            if let Some(e) = monitor::last_connect_error() {
                return Err(e);
            }
            return Err("not connected — monitor startup snapshot is not ready".to_string());
        }
        let _op = lock_device_op();
        let maybe_session = {
            let mut guard = lock_ok(&arc);
            guard.take()
        };
        if let Some(mut session) = maybe_session {
            let result = session.list_my_presets_strict();
            *lock_ok(&arc) = Some(session);
            return result;
        }
        Session::connect()?.list_my_presets_strict()
    })
    .await
    .map_err(|e| format!("list task failed: {e}"))?
}

/// Read a WAV file and downmix to mono f32 in [-1, 1] (fixed mono convention).
/// Returns (samples, sample_rate).
fn read_wav_mono(path: &str) -> Result<(Vec<f32>, u32), String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max)
                .collect()
        }
    };
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

/// Read a WAV file, downmix to mono f32, and measure its loudness. Used by
/// `probe --lufs <wav>` to validate `lufs.rs` against an external oracle
/// (pyloudnorm / ffmpeg ebur128) without any device.
pub fn measure_wav_file(path: &str) -> Result<lufs::Loudness, String> {
    let (mono, rate) = read_wav_mono(path)?;
    lufs::measure_mono(&mono, rate)
}

/// Write a mono f32 buffer to a WAV (the offline reference-clip corpus format).
fn write_wav_mono(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("create {path}: {e}"))?;
    for &s in samples {
        w.write_sample(s)
            .map_err(|e| format!("write {path}: {e}"))?;
    }
    w.finalize().map_err(|e| format!("finalize {path}: {e}"))
}

/// OFFLINE HARNESS (1/3): capture ONE full re-amp clip of `slot` at the leveling
/// reference level (0.5) via the topology stimulus and write it to `out` (mono,
/// 48 kHz f32) — building a corpus of real processed clips so the adaptive-capture
/// constants can be tuned with no device. DEVICE OP (load + engage + full capture).
pub fn probe_capture_reference(slot: u32, topology_id: &str, out: &str) -> Result<String, String> {
    let stim = read_stimulus_48k(&probe_stimulus_path(topology_id)?)?;
    let (mono, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
    write_wav_mono(out, &mono, rate)?;
    Ok(format!(
        "captured slot {slot} ({} samples = {:.2}s @ {rate} Hz) → {out}\n",
        mono.len(),
        mono.len() as f32 / rate as f32
    ))
}

/// OFFLINE HARNESS (2/3): recompute integrated LUFS over increasing prefixes of a
/// reference clip vs the full read, to anchor `min_measure_ms`/`max_capture_ms`. No
/// device — pure analysis on a clip captured by `--capture-reference`.
pub fn probe_measure_prefix_sweep(wav: &str) -> Result<String, String> {
    let (mono, rate) = read_wav_mono(wav)?;
    let full = lufs::measure_mono(&mono, rate)?.integrated_lufs;
    let total_s = mono.len() as f32 / rate as f32;
    let mut out = format!(
        "prefix sweep {wav}  ({total_s:.2}s @ {rate} Hz)  full integrated = {full:.3} LUFS\n  \
         prefix_s   integrated   Δ vs full\n"
    );
    for &secs in &[0.5f32, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0] {
        let n = ((secs * rate as f32) as usize).min(mono.len());
        if n == 0 {
            continue;
        }
        let v = lufs::measure_mono(&mono[..n], rate)
            .map(|l| l.integrated_lufs)
            .unwrap_or(f64::NAN);
        out += &format!("  {secs:>6.1}    {v:>9.3}    {:>+8.3}\n", v - full);
        if secs >= total_s {
            break;
        }
    }
    Ok(out)
}

/// OFFLINE HARNESS (3/3): replay a reference clip through the SAME convergence state
/// machine `reamp_measure` uses with the given (eps, k, preroll), reporting the exit
/// time and the error vs the full-clip read. Use to confirm a candidate tuning stays
/// within ±0.07 LU on every corpus clip. No device.
pub fn probe_measure_converge_replay(
    wav: &str,
    eps_lu: f64,
    stable_k: u32,
    preroll_ms: u64,
) -> Result<String, String> {
    let (mono, rate) = read_wav_mono(wav)?;
    let full = lufs::measure_mono(&mono, rate)?.integrated_lufs;
    // Exercise the adaptive early-exit path (the harness exists to tune it).
    let opts = audio::MeasureOpts {
        eps_lu,
        stable_k,
        preroll_ms,
        ..audio::MeasureOpts::adaptive()
    };
    let r = audio::replay_measure(&mono, rate, opts)?;
    Ok(format!(
        "replay {wav}\n  opts: preroll={}ms hop={}ms eps={} k={} min={}ms max={}ms\n  \
         exit={}ms converged={}  integrated={:.3} LUFS  full={full:.3}  Δ={:+.3} LU\n",
        opts.preroll_ms,
        opts.hop_ms,
        opts.eps_lu,
        opts.stable_k,
        opts.min_measure_ms,
        opts.max_capture_ms,
        r.exit_ms,
        r.converged,
        r.integrated_lufs,
        r.integrated_lufs - full
    ))
}

/// HW A/B for the RE-BASELINE decision: on one preset, measure the captured LUFS
/// the FULL way (production metric: settle → full stimulus + 0.8 s tail → integrate)
/// and the ADAPTIVE way (`reamp_measure` early-exit), on two fresh connections, and
/// report both values, their delta, and both wall-clock times. This is the live
/// counterpart to the offline `--measure-converge-replay`; it keeps the adaptive
/// `reamp_measure` reachable and lets the operator judge the speed/accuracy trade on real
/// presets before any decision to wire adaptive into the leveling path. Read-only
/// (no save); ends with a guaranteed re-amp OFF.
pub fn probe_measure_adaptive(slot: u32, topology_id: &str) -> Result<String, String> {
    let stim = read_stimulus_48k(&probe_stimulus_path(topology_id)?)?;
    // Load once in its own connection (the set-after-load override gotcha).
    {
        let mut s = session::Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }

    let measure_once = |adaptive: bool| -> Result<(f64, u128), String> {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let t = std::time::Instant::now();
        let mut s = session::Session::connect()?;
        s.set_preset_level(0.5)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = s.set_reamp_mode(true)?;
        let lufs = if adaptive {
            audio::reamp_measure(&stim, 48_000, audio::MeasureOpts::adaptive())
        } else {
            std::thread::sleep(std::time::Duration::from_millis(500));
            leveller::loudest_lufs(audio::reamp_capture(&stim, 48_000, 800))
        };
        let _ = s.set_reamp_mode(false);
        Ok((lufs?, t.elapsed().as_millis()))
    };

    let (full_lufs, full_ms) = measure_once(false)?;
    let (adapt_lufs, adapt_ms) = measure_once(true)?;
    // Guaranteed re-amp OFF on a fresh connection.
    let _ = session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()));

    Ok(format!(
        "preset {slot} @ presetLevel 0.5 ({topology_id})\n  \
         FULL     {full_lufs:>8.3} LUFS   {full_ms:>5} ms\n  \
         ADAPTIVE {adapt_lufs:>8.3} LUFS   {adapt_ms:>5} ms\n  \
         Δ(adaptive−full) = {:+.3} LU   time saved = {} ms\n",
        adapt_lufs - full_lufs,
        full_ms.saturating_sub(adapt_ms)
    ))
}

/// HW A/B of TWO stimuli per preset: for each slot, `measure_c` with stimulus A then
/// B and report the solved ceilings + dynamics spreads and ΔC. Quantifies how
/// sensitive each preset's leveling is to the stimulus character (e.g. the shipped
/// plucked noise vs a real chord DI captured with `--capture-input`) — data for the
/// playing-style question before any product change. Read-only (no save, no level
/// write persists); ends with a guaranteed re-amp OFF. Per-slot errors print in-row
/// and the sweep continues.
pub fn probe_stim_ab(
    slots: &[u32],
    wav_a: &str,
    wav_b: &str,
    ref_level: f32,
) -> Result<String, String> {
    let stim_a = read_stimulus_48k(wav_a)?;
    let stim_b = read_stimulus_48k(wav_b)?;
    let mut out = format!(
        "stimulus A/B @ ref {ref_level:.2}\n  A = {wav_a}\n  B = {wav_b}\n\
         \n  slot |      C_A |      C_B |      ΔC | spread_A | spread_B\n"
    );
    for &slot in slots {
        // measure_c owns its own connection/gap pacing (the level_setlist precedent).
        let row = leveller::measure_c(slot, &stim_a, ref_level, &[])
            .and_then(|a| leveller::measure_c(slot, &stim_b, ref_level, &[]).map(|b| (a, b)));
        match row {
            Ok((a, b)) => {
                out += &format!(
                    "  {slot:>4} | {:>8.3} | {:>8.3} | {:>+7.3} | {:>8.2} | {:>8.2}\n",
                    a.c,
                    b.c,
                    b.c - a.c,
                    a.dynamic_spread_lu,
                    b.dynamic_spread_lu
                );
            }
            Err(e) => out += &format!("  {slot:>4} | FAILED: {e}\n"),
        }
    }
    // Guaranteed re-amp OFF on a fresh connection — propagate a cleanup failure so a
    // "successful" A/B can't silently leave the device stuck in re-amp mode.
    session::Session::connect().and_then(|mut s| s.set_reamp_mode(false).map(|_| ()))?;
    Ok(out)
}

/// Read a stimulus WAV as mono f32, requiring the device's 48 kHz clock rate.
fn read_stimulus_48k(path: &str) -> Result<Vec<f32>, String> {
    let (stim, srate) = read_wav_mono(path)?;
    if srate != 48_000 {
        return Err(format!("stimulus must be 48 kHz (got {srate})"));
    }
    Ok(stim)
}

/// Read a 48 kHz stimulus and, if the instrument profile is Tier-2 calibrated,
/// scale it so its **K-weighted loudness (LUFS)** matches the measured real output
/// so the amp is driven as the real guitar drives it (re-amp inject is not AGC'd
/// — verified on device). K-weighted (not flat RMS): the perceptual weighting that
/// tracks how hard a pickup actually drives the amp — bright pickups aren't
/// under-counted — and it's the same scale the leveler targets on the output.
/// Caps the gain so the scaled peak stays ≤ 0.99 (no digital clip); when that cap
/// engages the calibrated loudness is UNREACHABLE and every measurement under-drives
/// the amp — the second return element is how many LU short the stimulus falls
/// (`None` when the target is reachable). Surfaced to the user at calibrate time
/// (`calibrate_profile`); the `log::warn!` covers every leveling caller.
fn read_stimulus_calibrated_with_shortfall(
    path: &str,
    calibration_lufs: Option<f32>,
) -> Result<(Vec<f32>, Option<f32>), String> {
    let mut stim = read_stimulus_48k(path)?;
    let mut shortfall_lu = None;
    if let Some(target_lufs) = calibration_lufs {
        let stim_lufs = lufs::measure_mono(&stim, 48_000)?.integrated_lufs;
        if stim_lufs.is_finite() {
            let mut g = 10f32.powf((target_lufs - stim_lufs as f32) / 20.0);
            let peak = stim.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
            if peak * g > 0.99 {
                let shortfall = 20.0 * (peak * g / 0.99).log10();
                log::warn!(
                    "stimulus calibration capped: {path} cannot reach {target_lufs:.1} LUFS \
                     without clipping — driving {shortfall:.1} LU softer"
                );
                shortfall_lu = Some(shortfall);
                g = 0.99 / peak; // guard against clipping the injected signal
            }
            for s in &mut stim {
                *s *= g;
            }
        }
    }
    Ok((stim, shortfall_lu))
}

/// [`read_stimulus_calibrated_with_shortfall`] for the callers that only need the
/// samples (all leveling/probe paths — the warn above still fires for them).
fn read_stimulus_calibrated(path: &str, calibration_lufs: Option<f32>) -> Result<Vec<f32>, String> {
    read_stimulus_calibrated_with_shortfall(path, calibration_lufs).map(|(stim, _)| stim)
}

#[cfg(test)]
mod stimulus_shortfall_tests {
    use super::read_stimulus_calibrated_with_shortfall;

    fn wav() -> String {
        format!(
            "{}/resources/samples/guitar-humbucker.wav",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    #[test]
    fn unreachable_target_reports_shortfall_and_caps_peak() {
        let (stim, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), Some(0.0)).unwrap();
        let shortfall = shortfall.expect("0 LUFS is far above the stimulus ceiling");
        assert!(
            shortfall > 0.0,
            "shortfall must be positive LU: {shortfall}"
        );
        let peak = stim.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak <= 0.99 + 1e-4, "capped peak must stay ≤ 0.99: {peak}");
    }

    #[test]
    fn reachable_target_has_no_shortfall() {
        let (_, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), Some(-60.0)).unwrap();
        assert_eq!(shortfall, None);
    }

    #[test]
    fn uncalibrated_has_no_shortfall() {
        let (_, shortfall) = read_stimulus_calibrated_with_shortfall(&wav(), None).unwrap();
        assert_eq!(shortfall, None);
    }
}

/// M3 one-shot leveling (the real path): fresh-connect, load `slot`, measure at
/// a reference level, solve the linear model for the exact `presetLevel` that
/// hits `target_lufs`, set it, and (if `save`) persist. Optionally re-measures
/// on a second fresh connection to confirm. Re-amp is always restored OFF.
pub fn probe_level_preset(
    slot: u32,
    target_lufs: f64,
    save: bool,
    verify: bool,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    // Optional Tier-2 calibration: scale the stimulus to a measured LUFS.
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    let opts = leveller::LevelOptions {
        save,
        verify,
        ..Default::default()
    };
    let r = leveller::level_preset(slot, &stim, target_lufs, opts, &[], || false)?;

    let mut out = format!(
        "slot {slot}: measured {:.2} LUFS @ ref {:.2}  (C={:.2})\n\
         → target {:.1} LUFS  ⇒  presetLevel={:.4}{}  (predicted {:.2} LUFS){}\n",
        r.measured_lufs,
        r.ref_level,
        r.constant_c,
        r.target_lufs,
        r.final_level,
        if r.clamped {
            " [CLAMPED — target unreachable]"
        } else {
            ""
        },
        r.predicted_lufs,
        if r.saved { "  [SAVED]" } else { "" },
    );
    if let Some(m) = r.verify_lufs {
        out += &format!(
            "verify (fresh capture @ {:.4}): {:.2} LUFS  (target {:.1}, err {:+.2} LU)\n",
            r.final_level,
            m,
            target_lufs,
            m - target_lufs
        );
    }
    Ok(out)
}

/// `probe --live-lufs` — install an advisory live-LUFS sink that PRINTS each streamed
/// reading, then run the SAME path as [`probe_level_preset`], validating the whole
/// live-LUFS backend headless before any frontend exists. The final `LevelResult` summary
/// must match a plain `--levelpreset` run (the advisory meter must not perturb the solve);
/// run the A/B on a REVERB/DELAY preset to catch any capture-length re-baseline.
pub fn probe_live_lufs(
    slot: u32,
    target_lufs: f64,
    save: bool,
    verify: bool,
) -> Result<String, String> {
    audio::set_live_lufs_sink(Box::new(|lufs| println!("live {lufs:.2} LUFS")));
    let r = probe_level_preset(slot, target_lufs, save, verify);
    audio::clear_live_lufs_sink();
    r
}

/// Filter already-discovered blocks to amp `outputLevel` leveling candidates — amp
/// blocks' `outputLevel` controls, the only tone-safe per-scene leveling knob. The
/// single definition of "what counts as a leveling candidate", shared by every caller
/// (the scene-leveling driver, the diagnostics, and the bench's intel session — which
/// brings its own pre-discovered blocks).
fn filter_amp_candidates(blocks: Vec<session::LevelBlock>) -> Vec<LevelBlockArg> {
    blocks
        .into_iter()
        .filter(|b| is_amp_model_id(&b.model_id) && is_amp_output_level_param(&b.parameter_id))
        .map(|b| LevelBlockArg {
            group_id: b.group_id,
            node_id: b.node_id,
            parameter_id: b.parameter_id,
            value: b.value,
        })
        .collect()
}

/// Run the 1.8.45-safe block discovery (`load_then_discover_blocks`) and filter it to
/// amp `outputLevel` leveling candidates.
fn load_and_filter_amp_candidates(list_index: u32) -> Result<Vec<LevelBlockArg>, String> {
    Ok(filter_amp_candidates(load_then_discover_blocks(
        list_index,
    )?))
}

// Isolated per-scene re-amp measurement (the proven `measure_knob_at` shape, but
// with explicit control over scene-edit). Loads the preset in its OWN connection
// (revert + latch rule), then a fresh connection: load scene → optional scene-edit →
// set the knob → engage → capture → measure the loudest channel's integrated LUFS.
// Fresh stream per call (NOT the BatchedLive shared stream, whose windowed reads
// mis-measured scenes — Klon read -6.96 LUFS when the knob's true range is -40..-14).
// NO save.
#[allow(clippy::too_many_arguments)] // cohesive per-scene measurement params; a struct would only add ceremony
fn measure_scene_knob_isolated(
    list_index: u32,
    scene_slot: u32,
    group_id: &str,
    node_id: &str,
    param: &str,
    value: f32,
    scene_edit: bool,
    stim: &[f32],
) -> Result<f64, String> {
    use std::time::Duration;
    {
        let mut s = Session::connect()?;
        s.load_preset(list_index)?;
        std::thread::sleep(Duration::from_millis(1200));
    }
    std::thread::sleep(Duration::from_millis(400));
    let mut s = Session::connect()?;
    s.load_scene(scene_slot)?;
    std::thread::sleep(Duration::from_millis(300));
    if scene_edit {
        s.set_node_scene_edit(group_id, node_id, true)?;
        std::thread::sleep(Duration::from_millis(300));
    }
    s.change_parameter(group_id, node_id, param, value)?;
    std::thread::sleep(Duration::from_millis(300));
    s.set_reamp_mode(true)?;
    std::thread::sleep(Duration::from_millis(500));
    let cap = audio::reamp_capture(stim, 48_000, 800);
    let _ = s.set_reamp_mode(false);
    leveller::loudest_lufs(cap)
}

/// READ-ONLY outcome proof: measure each scene's ACTUAL captured loudness from the
/// SAVED preset state — load preset, load scene, engage, measure (NO knob change). The
/// honest "did leveling land?" check, independent of the leveling math.
pub fn probe_measure_scene_levels(list_index: u32, topology_id: String) -> Result<String, String> {
    use std::time::Duration;
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let scenes = read_preset_scenes_fresh(list_index)?;
    let mut slots: Vec<(u32, String)> = scenes
        .scenes
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32, n.clone()))
        .collect();
    slots.push((session::BASE_SCENE_SLOT, "Base".to_string()));

    let measure = |scene_slot: u32| -> Result<f64, String> {
        {
            let mut s = Session::connect()?;
            s.load_preset(list_index)?;
            std::thread::sleep(Duration::from_millis(1200));
        }
        std::thread::sleep(Duration::from_millis(400));
        let mut s = Session::connect()?;
        s.load_scene(scene_slot)?;
        std::thread::sleep(Duration::from_millis(400));
        s.set_reamp_mode(true)?;
        std::thread::sleep(Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        leveller::loudest_lufs(cap)
    };

    let mut out = format!(
        "=== preset {:03} saved-state scene loudness ===\n",
        list_index + 1
    );
    for (slot, name) in slots {
        match measure(slot) {
            Ok(m) => out += &format!("scene {slot:>2} {name:<18} {m:.2} LUFS\n"),
            Err(e) => out += &format!("scene {slot:>2} {name:<18} [FAIL: {e}]\n"),
        }
    }
    Ok(out)
}

/// HW driver for the per-scene leveling goal: level a preset's Base + every FS scene
/// to a target, with optional per-scene-NAME target overrides. Mirrors the app's run:
///   • Base → `presetLevel` (one-shot `level_preset`), done FIRST so the FS scenes
///     measure under the final preset gain.
///   • FS scenes → amp `outputLevel` via the BatchedLive runner, GROUPED by resolved
///     target (so scenes wanting different targets each get their own batched pass).
/// Reuses the 1.8.45-safe block discovery (`load_then_discover_blocks`). Stimulus comes
/// from `topology_id`; `save` persists. Returns a human-readable per-scene report.
pub fn probe_level_preset_scenes(
    list_index: u32,
    default_target: f64,
    topology_id: String,
    save: bool,
    overrides: Vec<(String, f64)>,
) -> Result<String, String> {
    if !default_target.is_finite() {
        return Err("default target LUFS must be finite".to_string());
    }
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Resolve a per-scene target by NAME (case-insensitive), else the default.
    let resolve = |name: &str| -> f64 {
        overrides
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
            .unwrap_or(default_target)
    };

    let mut out = String::new();
    out += &format!(
        "=== preset {:03} (list index {list_index}) · default {:.1} LU · save={save} ===\n",
        list_index + 1,
        default_target,
    );

    // 1) scene names (field-8 read; 1.8.45-safe).
    let scenes = read_preset_scenes_fresh(list_index)?;
    out += &format!("scenes ({}): {:?}\n", scenes.scenes.len(), scenes.scenes);

    // 2) Base → presetLevel FIRST (a "base"/"BASE" override targets it).
    let base_target = overrides
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("base"))
        .map(|(_, t)| *t)
        .unwrap_or(default_target);
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    let br = leveller::level_preset(list_index, &stim, base_target, opts, &[], || false)?;
    out += &format!(
        "Base  → target {:.1}  presetLevel={:.4}  verify {:.2} LU (err {:+.2}){}{}\n",
        base_target,
        br.final_level,
        br.verify_lufs.unwrap_or(f64::NAN),
        br.verify_lufs.map(|m| m - base_target).unwrap_or(f64::NAN),
        if br.clamped { "  [CLAMPED]" } else { "" },
        if br.saved { "  [SAVED]" } else { "" },
    );

    if scenes.scenes.is_empty() {
        out += "(no FS scenes)\n";
        return Ok(out);
    }

    // 3a) amp candidates via the 1.8.45-safe rich-session discovery.
    let candidates = load_and_filter_amp_candidates(list_index)?;
    if candidates.is_empty() {
        return Err("no amp outputLevel controls found — cannot scene-level".to_string());
    }
    out += &format!("amp candidates: {}\n", candidates.len());

    // 3b) ONE un-engaged pre-pass over every FS scene → pick each scene's active amp.
    let all_slots: Vec<u32> = (0..scenes.scenes.len() as u32).collect();
    let docs = prepass_scene_docs(list_index, &all_slots)?;

    // 3c) ONE-SHOT open-loop per scene on the active amp `outputLevel`. HW-verified:
    //     captured_LUFS = 20*log10(outputLevel) + C is LINEAR with ~25 LU authority, so
    //     measure ONCE at a reference level, solve C, set the exact level — no closed
    //     loop, no clamp-flailing. Scene-edit ON isolates the write to the scene overlay.
    //     Measurement is ISOLATED (fresh stream per point) because the BatchedLive shared
    //     stream mis-measured scenes (Klon read -6.96 when its true range is -40..-14).
    const SCENE_REF: f32 = 0.5;
    for slot in all_slots {
        let name = scenes.scenes[slot as usize].clone();
        let target = resolve(&name);
        // active amp for this scene (first un-bypassed amp outputLevel)
        let knob = match build_scene_jobs(&[slot], &candidates, &docs)
            .ok()
            .and_then(|j| j.into_iter().next())
            .and_then(|j| j.knobs.into_iter().next())
            .map(|kt| kt.knob)
        {
            Some(leveller::LevelKnob::Block {
                group_id,
                node_id,
                parameter_id,
                ..
            }) => (group_id, node_id, parameter_id),
            _ => {
                out += &format!("FS[{slot}] {name:<18} [SKIP: no active amp outputLevel]\n");
                continue;
            }
        };
        let (g, n, p) = knob;
        // measure once at the reference outputLevel (scene-edit on for isolation)
        let measured =
            match measure_scene_knob_isolated(list_index, slot, &g, &n, &p, SCENE_REF, true, &stim)
            {
                Ok(m) => m,
                Err(e) => {
                    out += &format!("FS[{slot}] {name:<18} [FAIL measure: {e}]\n");
                    continue;
                }
            };
        let c = measured - 20.0 * (SCENE_REF as f64).log10();
        let (final_level, clamped, predicted) = leveller::solve_level(c, target);
        // apply + save: own load connection, then a fresh set connection.
        if save {
            {
                let mut s = Session::connect()?;
                s.load_preset(list_index)?;
                std::thread::sleep(std::time::Duration::from_millis(1200));
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
            let mut s = Session::connect()?;
            s.load_scene(slot)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.set_node_scene_edit(&g, &n, true)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.change_parameter(&g, &n, &p, final_level)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            s.save_current_preset(list_index)?;
        }
        // verify: re-measure isolated at the solved level (validates the model end-to-end).
        let verify =
            measure_scene_knob_isolated(list_index, slot, &g, &n, &p, final_level, true, &stim)
                .ok();
        out += &format!(
            "FS[{slot}] {name:<18} target {:.1}  C={:.2}  outputLevel={:.4}  predicted {:.2}  verify {:.2} (err {:+.2}){}{}\n",
            target,
            c,
            final_level,
            predicted,
            verify.unwrap_or(f64::NAN),
            verify.map(|v| v - target).unwrap_or(f64::NAN),
            if clamped { "  [CLAMPED]" } else { "" },
            if save { "  [SAVED]" } else { "" },
        );
    }
    Ok(out)
}

/// READ-ONLY diagnostic for the scene-leveling amp-pick: for a preset, harvest each
/// scene's live doc (un-engaged pre-pass) and print, per scene, every level-type
/// control (model/node/param = value) plus each amp candidate's live BYPASS state.
/// Reveals which amp `build_scene_jobs` would pick (first un-bypassed) and whether
/// the scene's loudness is governed by that amp or by something else (the other amp,
/// a post-amp boost, a delay/IR `level`). No writes, no re-amp.
pub fn probe_scene_amp_diag(list_index: u32) -> Result<String, String> {
    let scenes = read_preset_scenes_fresh(list_index)?;
    let amp_cands = load_and_filter_amp_candidates(list_index)?;
    let mut all_slots: Vec<u32> = (0..scenes.scenes.len() as u32).collect();
    all_slots.push(session::BASE_SCENE_SLOT);
    let docs = prepass_scene_docs(list_index, &all_slots)?;

    let mut out = format!(
        "preset {:03} · scenes {:?}\namp candidates: {:?}\n",
        list_index + 1,
        scenes.scenes,
        amp_cands
            .iter()
            .map(|c| format!("{}/{} outputLevel={:.3}", c.group_id, c.node_id, c.value))
            .collect::<Vec<_>>(),
    );
    for (slot, doc) in &docs {
        let name = if *slot >= session::BASE_SCENE_SLOT {
            "Base".to_string()
        } else {
            scenes
                .scenes
                .get(*slot as usize)
                .cloned()
                .unwrap_or_default()
        };
        out += &format!("\n── scene {slot} ({name}) ──\n");
        let Some(d) = doc else {
            out += "  (no doc harvested)\n";
            continue;
        };
        out += "  amp bypass:\n";
        for c in &amp_cands {
            let bypass = scenes::block_bypass_in_live_graph(d, &c.group_id, &c.node_id);
            out += &format!("    {}/{} bypass={bypass:?}\n", c.group_id, c.node_id);
        }
        out += "  level controls in this scene's doc:\n";
        for b in session::extract_level_blocks(d) {
            out += &format!(
                "    {}/{} [{}] {} = {:.4}\n",
                b.group_id, b.node_id, b.model_id, b.parameter_id, b.value
            );
        }
    }
    Ok(out)
}

/// DECISIVE diagnostic: does the active amp's `outputLevel` actually move ONE scene's
/// loudness, and does scene-edit ISOLATE the write? For `scene_slot`, picks the active
/// amp (un-bypassed) and measures captured LUFS at a LOW and HIGH outputLevel under two
/// write modes:
///   • GLOBAL  — `change_parameter` only (writes the base/global value).
///   • SCENE   — `set_node_scene_edit(true)` then `change_parameter` (scene overlay).
/// Reads back the BASE scene's outputLevel after the SCENE write to confirm isolation.
/// Interpretation: GLOBAL Δ large but SCENE Δ ≈ 0 ⇒ scene-edit not isolating on 1.8.45;
/// both Δ ≈ 0 ⇒ genuine no authority (effect-dominated); both large ⇒ authority fine,
/// the clamp is the loop's noisy-slope bug. NO SAVE (reloads the stored preset each step).
pub fn probe_scene_knob_authority(
    list_index: u32,
    scene_slot: u32,
    topology_id: String,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Pick the active amp for this scene (same logic as build_scene_jobs).
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &[scene_slot])?;
    let job = build_scene_jobs(&[scene_slot], &candidates, &docs)?;
    let knob = job
        .into_iter()
        .next()
        .and_then(|j| j.knobs.into_iter().next())
        .ok_or("no scene job built")?
        .knob;
    let (group_id, node_id, param) = match &knob {
        leveller::LevelKnob::Block {
            group_id,
            node_id,
            parameter_id,
            ..
        } => (group_id.clone(), node_id.clone(), parameter_id.clone()),
        _ => return Err("expected a Block knob".to_string()),
    };

    // measure the scene's loudness after setting outputLevel=v under a write mode
    // (the shared isolated-measure helper; global write = no scene-edit).
    let measure = |scene_edit: bool, v: f32| -> Result<f64, String> {
        measure_scene_knob_isolated(
            list_index, scene_slot, &group_id, &node_id, &param, v, scene_edit, &stim,
        )
    };

    let lo = 0.05f32;
    let hi = 0.95f32;
    let g_lo = measure(false, lo)?;
    let g_hi = measure(false, hi)?;
    let s_lo = measure(true, lo)?;
    let s_hi = measure(true, hi)?;

    Ok(format!(
        "scene {scene_slot} active amp {group_id}/{node_id} {param}\n\
         GLOBAL write: outputLevel {lo} → {g_lo:.2} LUFS | {hi} → {g_hi:.2} LUFS | Δ = {:.2} LU\n\
         SCENE  write: outputLevel {lo} → {s_lo:.2} LUFS | {hi} → {s_hi:.2} LUFS | Δ = {:.2} LU\n\
         interpretation: global Δ large + scene Δ≈0 ⇒ scene-edit NOT isolating; \
         both Δ≈0 ⇒ no authority; both large ⇒ authority OK (clamp = loop bug)\n",
        g_hi - g_lo,
        s_hi - s_lo,
    ))
}

/// READ-ONLY mute-isolation diagnostic (`probe --mute-floor`) for the rebalance flow: for a
/// 2-amp MERGED scene, report the combined output, the both-lanes-muted floor, and each lane
/// SOLO with its margin above the floor. A small margin ⇒ `outputLevel`=0 isn't deep silence
/// and the muted lane bleeds into the solo, so the equal-solo balance is only approximate.
/// NO SAVE. Validates the `verify_by_ear` heuristic `level_scenes_rebalance` applies.
pub fn probe_mute_floor(
    list_index: u32,
    scene_slot: u32,
    topology_id: String,
) -> Result<String, String> {
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Build the scene job the same way the leveler does, then take its first two amp lanes.
    let candidates = load_and_filter_amp_candidates(list_index)?;
    let docs = prepass_scene_docs(list_index, &[scene_slot])?;
    let job = build_scene_jobs(&[scene_slot], &candidates, &docs)?
        .into_iter()
        .next()
        .ok_or("no scene job built")?;
    if job.knobs.len() < 2 {
        return Err(format!(
            "scene {scene_slot} has {} amp knob(s) — mute-floor needs a 2-amp merged parallel scene",
            job.knobs.len()
        ));
    }
    let a = &job.knobs[0];
    let b = &job.knobs[1];
    leveller::mute_floor_report(list_index, &a.knob, a.current, &b.knob, b.current, &stim)
}

/// Closed-loop block-control leveling on the real device: enumerate `slot` to
/// find the chosen block's current value (for sensible search bounds), then drive
/// it via `ChangeParameter` in a closed loop to `target_lufs`. Amplitude params
/// (current value in 0..1) search [0,1]; dB-unit params (e.g. an IR `outputlevel`)
/// search a ±range around the current value. Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_level_block(
    slot: u32,
    target_lufs: f64,
    group_id: String,
    node_id: String,
    parameter_id: String,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    // Discover the block's current value to choose search bounds.
    let blocks = load_then_discover_blocks(slot)?;
    let cur = blocks
        .iter()
        .find(|b| b.group_id == group_id && b.node_id == node_id && b.parameter_id == parameter_id)
        .map(|b| b.value)
        .ok_or_else(|| {
            format!(
                "{group_id}/{node_id}/{parameter_id} not found among this preset's level blocks"
            )
        })?;
    let (lo, hi) = knob_bounds(cur);

    let knob = leveller::LevelKnob::Block {
        group_id,
        node_id,
        parameter_id,
        scene_slot: None,
    };
    let opts = leveller::LevelOptions {
        save: false,
        verify: true,
        ..Default::default()
    };
    let r = leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, || false)?;

    let mut out = format!(
        "slot {slot}  knob {}  (current {cur:.4}, bounds [{lo:.3}, {hi:.3}])\n\
         → solved {:.4} in {} iterations  (measured {:.2} LUFS, target {:.1}{})\n",
        knob.label(),
        r.final_level,
        r.iterations,
        r.measured_lufs,
        target_lufs,
        if r.clamped {
            "  [CLAMPED — target unreachable with this knob]"
        } else {
            ""
        },
    );
    if let Some(m) = r.verify_lufs {
        out += &format!(
            "verify (fresh capture @ {:.4}): {:.2} LUFS  (err {:+.2} LU)\n",
            r.final_level,
            m,
            m - target_lufs
        );
    }
    Ok(out)
}

/// N1 diagnostic (read-only): re-amp `slot` at `presetLevel = 0.5` and report
/// PER-CHANNEL integrated LUFS + RMS for every captured USB-Out channel. Tells us
/// whether a mono preset is MIRRORED onto both USB-Out 1&2 (ch0 ≈ ch1 → the
/// single-channel measure's +3 offset is uniform and cancels across presets) or
/// sits on ONE channel (ch1 ≪ ch0 → cross-preset variance for a stereo rig).
/// Loads + re-amps only; never writes/saves/clears. Stimulus = humbucker sample
/// (override with `TMP_LEVELLER_STIMULUS`).
pub fn probe_channels(slot: u32) -> Result<String, String> {
    let stim_path = match std::env::var("TMP_LEVELLER_STIMULUS") {
        Ok(p) => p,
        Err(_) => probe_stimulus_path("guitar-humbucker")?,
    };
    let stim = read_stimulus_48k(&stim_path)?;
    let cap = leveller::capture_full(slot, &stim, 0.5)?;
    let lufs_at = |c: usize| -> Option<f64> {
        lufs::measure_mono(&cap.channel(c), cap.sample_rate)
            .ok()
            .map(|l| l.integrated_lufs)
            .filter(|v| v.is_finite())
    };
    let mut out = format!(
        "slot {slot}: {} channels @ {} Hz\n",
        cap.channels, cap.sample_rate
    );
    for c in 0..cap.channels {
        let lufs = lufs_at(c).map_or("  -inf".to_string(), |v| format!("{v:>7.2}"));
        let rms = cap.channel_rms(c);
        let rms_db = if rms > 1e-9 {
            20.0 * (rms as f64).log10()
        } else {
            -120.0
        };
        out.push_str(&format!("  ch{c}: {lufs} LUFS   rms {rms_db:>7.2} dBFS\n"));
    }
    if let (Some(a), Some(b)) = (lufs_at(0), lufs_at(1)) {
        out.push_str(&format!("  ch0-ch1 delta: {:+.2} LU\n", a - b));
    }
    Ok(out)
}

fn probe_stimulus_path(topology_id: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("current dir: {e}"))?;
    let candidates = [
        cwd.join("resources")
            .join("samples")
            .join(format!("{topology_id}.wav")),
        cwd.join("apps")
            .join("tmp-companion")
            .join("src-tauri")
            .join("resources")
            .join("samples")
            .join(format!("{topology_id}.wav")),
    ];
    candidates
        .iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| format!("no bundled stimulus found for topology {topology_id:?}"))
}

fn is_amp_category(category: &str) -> bool {
    matches!(
        category,
        "Combo Amps" | "Amp Heads" | "Bass Amps" | "Half Stacks"
    )
}

fn amp_model_ids() -> &'static std::collections::HashSet<String> {
    static IDS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        let Ok(catalog) = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../src/models/tmp-model-guide.json"
        )) else {
            return std::collections::HashSet::new();
        };
        let Some(rows) = catalog.get("blocks").and_then(|v| v.as_array()) else {
            return std::collections::HashSet::new();
        };
        rows.iter()
            .filter_map(|row| {
                let block_id = row.get("block_id").and_then(|v| v.as_str())?;
                let category = row.get("category").and_then(|v| v.as_str())?;
                is_amp_category(category).then(|| block_id.to_string())
            })
            .collect()
    })
}

fn is_amp_model_id(model_id: &str) -> bool {
    // Device FenderIds carry cab/IR/convolution suffixes the catalog's bare amp bids
    // omit (e.g. "ACD_HiwattDR103CanModCabIR", "ACD_PrincetonReverb68CabIRConvRvb").
    // Strip them one at a time, checking after each — mirrors the frontend
    // `baseDeviceId` / blockArt `SUFFIX`. ("NoFx" is part of real base ids, not stripped.)
    const SUFFIXES: [&str; 5] = ["ConvRvb", "CabIR", "NoCab", "Cab", "IR"];
    let amps = amp_model_ids();
    let mut m = model_id;
    loop {
        if amps.contains(m) {
            return true;
        }
        match SUFFIXES.iter().find_map(|s| m.strip_suffix(s)) {
            Some(next) => m = next,
            // Last-gap bridge: a wet amp id (…CabIRConvRvb) strips to a bare id the
            // catalog only carries WITH the NoFx token (…BlondeVibratoNoFx). NoFx is
            // never stripped, so try appending it once. Mirrors blockArt.ts.
            None => return !m.ends_with("NoFx") && amps.contains(&format!("{m}NoFx")),
        }
    }
}

fn is_amp_output_level_param(parameter_id: &str) -> bool {
    parameter_id == "outputLevel"
}

pub fn probe_bench_scene_leveling(
    slots: Vec<u32>,
    target_lufs: f64,
    topology_id: String,
    out_path: String,
) -> Result<String, String> {
    if slots.is_empty() {
        return Err("no preset slots supplied".to_string());
    }
    if !target_lufs.is_finite() {
        return Err("target LUFS must be finite".to_string());
    }
    let stim_path = probe_stimulus_path(&topology_id)?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;
    let save = matches!(
        std::env::var("TMP_SCENE_LEVEL_SAVE").ok().as_deref(),
        Some("1" | "true" | "yes")
    );
    let include_base = !matches!(
        std::env::var("TMP_SCENE_LEVEL_INCLUDE_BASE")
            .ok()
            .as_deref(),
        Some("0" | "false" | "no")
    );
    // BatchedLive only: the phase-1 matrix established the
    // per-scene numbers (legacy 80-93 s/scene; isolated live rows 22-45 s/scene
    // dominated by per-scene session ceremony; liveSecant noise-fragile,
    // proportional stalls on compressed knobs, full jumps overshoot steep ones).
    // BatchedLive amortizes ONE session + ONE re-amp engage across a preset's
    // scenes with trust-region slope jumps — the <10 s/scene candidate.
    let mut rows = Vec::<leveller::SceneLevelBenchmarkRow>::new();
    // Stream each row to disk as it lands (an OOM reboot lost a full 40-minute
    // run that only wrote at the end).
    let jsonl_path = format!("{out_path}.jsonl");
    let emit = |row: &leveller::SceneLevelBenchmarkRow| {
        if let Ok(line) = serde_json::to_string(row) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&jsonl_path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    };
    // The bench glue chains many short sessions; 800 ms between them is the
    // EMPIRICAL sweet spot (HW A/B): at 800 ms the open after a
    // live-cadence session close succeeded 4/4; at 1500 ms it failed 0xe00002c5
    // even on a freshly power-cycled unit — the device seems to accept a QUICK
    // re-open after a close, then lock out for minutes. Do not "safely" widen.
    let session_gap = || std::thread::sleep(std::time::Duration::from_millis(800));

    for slot in slots {
        // Scenes AND level blocks from ONE rich session (the `--scenes-load`
        // recipe + the monitor's graph-from-push-bodies approach): heartbeat
        // warmup → send_and_collect LoadPreset → pump; the load's pushes carry
        // BOTH the sceneListResponse(125) and the field-3 currentPresetDataChanged
        // (full audioGraph) on a live-cadence session. One session because every
        // alternative failed on HW: field-8 reads and extra re-opens
        // wedge the device's next exclusive open, and `connect_for_discovery`'s
        // field-78 burst kills field-3 delivery for its whole session on 1.8.45.
        eprintln!(
            "[bench] preset {:03}: load + scene + block-discovery session…",
            slot + 1
        );
        type PresetIntel = (
            Option<Vec<String>>,
            Vec<session::LevelBlock>,
            Vec<(u32, Option<serde_json::Value>)>,
        );
        let intel: Result<PresetIntel, String> = (|| {
            let mut s = Session::connect()?;
            // Dense heartbeats ~2 s: the device only pushes unsolicited data
            // (125, PresetLoaded, field-3) on a live-cadence session.
            for _ in 0..16 {
                s.heartbeat()?;
                s.pump_collect(120)?;
            }
            s.raw.clear();
            s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
            let mut scenes: Option<Vec<String>> = None;
            let mut seen = 0usize;
            // Keep pumping past the 125 hit — the multi-packet field-3 push
            // (block discovery) needs the extra turns to finish arriving.
            for _ in 0..10 {
                s.heartbeat()?;
                s.pump_collect(200)?;
                let bodies = s.push_bodies();
                for b in bodies.iter().skip(seen) {
                    if let Some(names) = session::decode_scene_list(b) {
                        scenes = Some(names);
                    }
                }
                seen = bodies.len();
            }
            if scenes.is_none() {
                // Explicit request fallback — WITHOUT raw.clear() (the field-3
                // payload for block discovery lives in the accumulated raw).
                let _ = s.send_and_collect(&proto::scene_list_request(), 300);
                for _ in 0..4 {
                    if let Some(names) = s
                        .push_bodies()
                        .iter()
                        .find_map(|b| session::decode_scene_list(b))
                    {
                        scenes = Some(names);
                        break;
                    }
                    let _ = s.pump_collect(200);
                }
            }
            let blocks = s.current_preset_blocks()?;
            // Un-engaged per-scene doc pre-pass (same session): loadScene →
            // field-3 push → the scene's LIVE doc, for the knob pick + that
            // scene's actual knob value. Must happen BEFORE any engage — the
            // device pushes no field-3 while re-amp is engaged.
            let mut docs: Vec<(u32, Option<serde_json::Value>)> =
                vec![(session::BASE_SCENE_SLOT, s.current_preset_value().ok())];
            if let Some(names) = &scenes {
                for idx in 0..names.len() as u32 {
                    s.raw.clear();
                    s.send_and_collect(&proto::load_scene(idx as u64), 300)?;
                    let mut doc = None;
                    for _ in 0..4 {
                        s.heartbeat()?;
                        s.pump_collect(150)?;
                        if let Ok(v) = s.current_preset_value() {
                            doc = Some(v);
                            break;
                        }
                    }
                    docs.push((idx, doc));
                }
            }
            Ok((scenes, blocks, docs))
        })();
        let (scenes, discovered, docs) = match intel {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[bench] preset {:03}: intel session failed: {e}", slot + 1);
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
                    elapsed_ms: 0,
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some(e),
                };
                emit(&row);
                rows.push(row);
                continue;
            }
        };
        if scenes.is_none() {
            eprintln!(
                "[bench] preset {:03}: no scene list harvested; Base only",
                slot + 1
            );
        }
        session_gap();
        let mut scene_rows: Vec<(u32, String)> = if include_base {
            vec![(session::BASE_SCENE_SLOT, "Base".to_string())]
        } else {
            Vec::new()
        };
        if let Some(names) = scenes {
            scene_rows.extend(
                names
                    .into_iter()
                    .enumerate()
                    .map(|(idx, name)| (idx as u32, name)),
            );
        }
        if scene_rows.is_empty() {
            let row = leveller::SceneLevelBenchmarkRow {
                preset_slot: slot,
                ui_label: format!("{:03}", slot + 1),
                scene_slot: session::BASE_SCENE_SLOT,
                scene_name: "Base".to_string(),
                strategy: leveller::SceneLevelStrategy::BatchedLive,
                elapsed_ms: 0,
                capture_windows: 0,
                parameter_writes: 0,
                final_lufs: None,
                error_lu: None,
                final_output_level: None,
                clamped: false,
                saved: false,
                failure: Some("no scene rows harvested".to_string()),
            };
            emit(&row);
            rows.push(row);
            continue;
        }

        let candidates = filter_amp_candidates(discovered);
        if candidates.is_empty() {
            for (scene_slot, scene_name) in &scene_rows {
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: *scene_slot,
                    scene_name: scene_name.clone(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
                    elapsed_ms: 0,
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some("no amp outputLevel controls found".to_string()),
                };
                emit(&row);
                rows.push(row);
            }
            continue;
        }

        SCENE_LEVEL_CANCEL.store(false, SeqCst);
        eprintln!(
            "[bench] preset {:03}: batched-live run over {} scene rows…",
            slot + 1,
            scene_rows.len()
        );
        let wire_slots: Vec<u32> = scene_rows.iter().map(|(s, _)| *s).collect();
        let jobs = match build_scene_jobs(&wire_slots, &candidates, &docs) {
            Ok(jobs) => jobs,
            Err(e) => {
                for (scene_slot, scene_name) in &scene_rows {
                    let row = leveller::SceneLevelBenchmarkRow {
                        preset_slot: slot,
                        ui_label: format!("{:03}", slot + 1),
                        scene_slot: *scene_slot,
                        scene_name: scene_name.clone(),
                        strategy: leveller::SceneLevelStrategy::BatchedLive,
                        elapsed_ms: 0,
                        capture_windows: 0,
                        parameter_writes: 0,
                        final_lufs: None,
                        error_lu: None,
                        final_output_level: None,
                        clamped: false,
                        saved: false,
                        failure: Some(e.clone()),
                    };
                    emit(&row);
                    rows.push(row);
                }
                continue;
            }
        };
        let t0 = std::time::Instant::now();
        let outcome = leveller::level_scenes_live_batched(
            slot,
            &jobs,
            &stim,
            target_lufs,
            save,
            |_, _| {},
            || SCENE_LEVEL_CANCEL.load(SeqCst),
        );
        match outcome {
            Ok(outcomes) => {
                for o in outcomes {
                    let name = scene_rows
                        .iter()
                        .find(|(s, _)| *s == o.scene_slot)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    match (&o.failure, o.final_lufs) {
                        (None, Some(lufs)) => eprintln!(
                            "[bench]   → scene {} ({name}): {:.2} LUFS (err {:+.2} LU) in {:.1}s clamped={} windows={} writes={}",
                            o.scene_slot,
                            lufs,
                            lufs - target_lufs,
                            o.elapsed_ms as f64 / 1000.0,
                            o.clamped,
                            o.windows,
                            o.writes,
                        ),
                        _ => eprintln!(
                            "[bench]   → scene {} ({name}): FAILED in {:.1}s: {}",
                            o.scene_slot,
                            o.elapsed_ms as f64 / 1000.0,
                            o.failure.as_deref().unwrap_or("?"),
                        ),
                    }
                    let row = leveller::SceneLevelBenchmarkRow {
                        preset_slot: slot,
                        ui_label: format!("{:03}", slot + 1),
                        scene_slot: o.scene_slot,
                        scene_name: name,
                        strategy: leveller::SceneLevelStrategy::BatchedLive,
                        elapsed_ms: o.elapsed_ms,
                        capture_windows: o.windows,
                        parameter_writes: o.writes,
                        final_lufs: o.final_lufs,
                        error_lu: o.final_lufs.map(|l| l - target_lufs),
                        final_output_level: o.final_level,
                        clamped: o.clamped,
                        saved: save && o.failure.is_none(),
                        failure: o.failure,
                    };
                    emit(&row);
                    rows.push(row);
                }
                eprintln!(
                    "[bench] preset {:03}: batched-live run total {:.1}s",
                    slot + 1,
                    t0.elapsed().as_millis() as f64 / 1000.0
                );
            }
            Err(e) => {
                eprintln!(
                    "[bench] preset {:03}: batched-live run FAILED: {e}",
                    slot + 1
                );
                let row = leveller::SceneLevelBenchmarkRow {
                    preset_slot: slot,
                    ui_label: format!("{:03}", slot + 1),
                    scene_slot: session::BASE_SCENE_SLOT,
                    scene_name: "Base".to_string(),
                    strategy: leveller::SceneLevelStrategy::BatchedLive,
                    elapsed_ms: t0.elapsed().as_millis(),
                    capture_windows: 0,
                    parameter_writes: 0,
                    final_lufs: None,
                    error_lu: None,
                    final_output_level: None,
                    clamped: false,
                    saved: false,
                    failure: Some(e),
                };
                emit(&row);
                rows.push(row);
            }
        }
        session_gap();
    }

    let json = serde_json::to_string_pretty(&rows).map_err(|e| format!("serialize report: {e}"))?;
    std::fs::write(&out_path, json).map_err(|e| format!("write {out_path}: {e}"))?;
    Ok(format!(
        "wrote {} benchmark rows to {out_path} (target {target_lufs:.1} LUFS, topology {topology_id})\n",
        rows.len()
    ))
}

/// Pick the route STRUCTURE graph from the pre-pass docs: the first doc that decodes
/// to a KNOWN routing template (`session::is_known_routing_template`). Routing is
/// scene-invariant, so one complete-enough doc defines lane membership for every
/// scene. Returns `None` when no doc carries a known template — the live field-3
/// partial truncates before the `template` tail, and silently defaulting to "series"
/// would re-introduce the parallel mislevel, so the caller must skip instead.
fn structure_graph(docs: &[(u32, Option<serde_json::Value>)]) -> Option<session::ActiveGraph> {
    docs.iter()
        .filter_map(|(_, d)| d.as_ref())
        .map(|d| session::extract_active_graph(d, None))
        .find(|g| session::is_known_routing_template(g.template.as_deref()))
}

/// Preset-wide gate: the routing template must be KNOWN (the live field-3 partial
/// truncates before the `template` tail, and silently defaulting to "series" would
/// re-introduce the parallel mislevel). A known template — series, parallel-merged,
/// split-output, or dual-input — is classifiable; only an unknown/incomplete one is a
/// hard error. (Mic-only paths produce no guitar amp candidate and skip per-scene.)
fn check_levelable_routing(structure: &session::ActiveGraph) -> Result<(), String> {
    if !session::is_known_routing_template(structure.template.as_deref()) {
        return Err("routing template unknown or read incomplete — cannot classify".to_string());
    }
    Ok(())
}

/// One resolved amp knob: `(group_id, node_id, current_outputLevel)`.
type AmpKnobSpec = (String, String, f32);

/// How a scene's amp knob set relates to the signal sum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParallelKind {
    /// One knob (series master / single amp) — no rebalance concept.
    Single,
    /// Two+ lane amps that RE-MERGE into one path (`gtrParallel*`): their mix is
    /// rebalanceable (rebalance only on a path merge).
    Merged,
    /// Two+ lane amps on SEPARATE physical outputs (`gtrSplit`/`gtrMicParallel`):
    /// joint-k for level, but NO rebalance (no shared mix between separate outs).
    SplitOutput,
}

/// Classify a scene into the SET of guitar-amp `outputLevel` knobs to drive, by amp
/// POSITION in the route graph (not the template string). Assumes [`check_levelable_routing`]
/// passed (known template). Levels against the USB 1/2 capture; no output→USB routing is
/// read (the user owns routing). Returns the knobs (`(group, node, current)`; >1 only for
/// a parallel/split scene → joint-k) or `Err(per-scene skip reason)`:
///
/// - Series → the LAST active amp in flow order (a post-merge amp counts as the series
///   master: scaling it scales the whole summed output).
/// - Parallel-merged / split-output / independent rails → the last active amp PER lane
///   (joint-k); a lane routed off USB contributes nothing to the capture but its amp is
///   still scaled by the shared factor.
/// - No active guitar amp (incl. mic-only presets), an active-amp lane with no
///   `outputLevel` control, multi-split amp spread, or a pre-split amp mixed with lane
///   amps → `Err` (never a partial joint-k).
fn classify_scene_knobs(
    structure: &session::ActiveGraph,
    scene_doc: &serde_json::Value,
    candidates: &[LevelBlockArg],
) -> Result<(Vec<AmpKnobSpec>, ParallelKind), String> {
    use session::Stage;
    // The amp's outputLevel candidate value, if it has one (None = no outputLevel knob).
    let ol = |g: &str, n: &str| {
        candidates
            .iter()
            .find(|c| {
                c.group_id == g && c.node_id == n && is_amp_output_level_param(&c.parameter_id)
            })
            .map(|c| c.value)
    };
    // Current value: the scene overlay's outputLevel if present, else the candidate value.
    let current = |g: &str, n: &str, fallback: f32| {
        session::extract_level_blocks(scene_doc)
            .into_iter()
            .find(|b| {
                b.group_id == g && b.node_id == n && is_amp_output_level_param(&b.parameter_id)
            })
            .map(|b| b.value)
            .unwrap_or(fallback)
    };
    // Active (non-bypassed in this scene) amp nodes, in route-graph flow order. Restricted
    // to GUITAR groups: re-amp drives the instrument input, so only the guitar chain is
    // captured at USB-Out (the leveling target); mic-input amps aren't reachable and have
    // no outputLevel candidate anyway. Bypass comes from the scene overlay, falling back
    // to the structure node when the scene doc doesn't carry it.
    let active: Vec<&session::GraphNode> = structure
        .nodes
        .iter()
        .filter(|nd| nd.group_id.starts_with('G') && is_amp_model_id(&nd.model))
        .filter(|nd| {
            match scenes::block_bypass_in_live_graph(scene_doc, &nd.group_id, &nd.node_id) {
                Some(b) => !b,
                None => !nd.bypassed,
            }
        })
        .collect();
    if active.is_empty() {
        return Err("no active guitar amp in scene".to_string());
    }

    // Parallel lanes that sum into / are captured at the USB-Out: every re-merging
    // stage split's two lanes, PLUS split-OUTPUT lanes (`gtrSplit`) and independent rails
    // (`gtrMicParallel`). We deliberately do NOT read the device's output→USB routing —
    // the leveler simply levels whatever the preset sends to USB 1/2 (the loudest-channel
    // capture); the user owns which path(s) reach USB 1/2. A split lane routed OFF USB
    // contributes nothing to the capture, so the joint-k solve is driven by the on-USB
    // lane; its amp is still scaled by the same factor (a side effect the user accepts by
    // managing routing). `post_merge` (Series stages after the last split) only applies to
    // re-merging stage splits, where a post-merge amp is the single series master.
    let group_of = |blocks: &[session::GraphNode]| -> Vec<String> {
        blocks.iter().map(|b| b.group_id.clone()).collect()
    };
    // Each split carries its KIND: a re-merging stage split is `Merged` (its lanes sum
    // back into one path → rebalancing their mix is meaningful); split-OUTPUT / rail
    // splits are `SplitOutput` (lanes go to separate physical outs → no shared mix to
    // rebalance).
    let mut splits: Vec<(Vec<String>, Vec<String>, ParallelKind)> = Vec::new();
    let mut post_merge: Vec<String> = Vec::new();
    let mut seen_split = false;
    for st in &structure.stages {
        match st {
            Stage::Series { blocks } => {
                if seen_split {
                    post_merge.extend(group_of(blocks));
                }
            }
            Stage::Split { a, b } => {
                seen_split = true;
                post_merge.clear(); // only Series groups after the LAST split count
                splits.push((group_of(a), group_of(b), ParallelKind::Merged));
            }
        }
    }
    if let Some(op) = &structure.outputs {
        splits.push((
            group_of(&op.a.blocks),
            group_of(&op.b.blocks),
            ParallelKind::SplitOutput,
        ));
        post_merge.clear();
    }
    if let Some(lanes) = &structure.lanes {
        if lanes.len() == 2 {
            splits.push((
                group_of(&lanes[0].blocks),
                group_of(&lanes[1].blocks),
                ParallelKind::SplitOutput,
            ));
            post_merge.clear();
        }
    }
    let in_groups = |gs: &[String], g: &str| gs.iter().any(|x| x == g);
    let split_groups: Vec<String> = splits
        .iter()
        .flat_map(|(a, b, _)| a.iter().chain(b))
        .cloned()
        .collect();
    // `active` is in structure.nodes order = flow order, so `.last()` of a filtered
    // subset is the last amp in flow.
    let last_in = |gs: &[String]| {
        active
            .iter()
            .rev()
            .copied()
            .find(|nd| in_groups(gs, &nd.group_id))
    };

    let resolve = |nd: &session::GraphNode| -> Result<(String, String, f32), String> {
        let v = ol(&nd.group_id, &nd.node_id).ok_or_else(|| {
            format!(
                "active amp {} has no outputLevel control — can't scene-level it",
                nd.node_id
            )
        })?;
        Ok((
            nd.group_id.clone(),
            nd.node_id.clone(),
            current(&nd.group_id, &nd.node_id, v),
        ))
    };

    // 1. A post-merge amp is the series master → single knob.
    if let Some(nd) = last_in(&post_merge) {
        return Ok((vec![resolve(nd)?], ParallelKind::Single));
    }

    // 2. Parallel: active amps in split lanes. Only the clean case — a SINGLE split's
    //    lanes, no pre-split/inter-split amp mixed in — joint-ks; anything more tangled
    //    is skipped rather than risk a wrong scaling.
    let mut amp_split_kind: Option<ParallelKind> = None;
    let mut amp_splits = 0usize;
    let mut lane_amps: Vec<&session::GraphNode> = Vec::new();
    for (a, b, kind) in &splits {
        let mut this = 0;
        if let Some(nd) = last_in(a) {
            lane_amps.push(nd);
            this += 1;
        }
        if let Some(nd) = last_in(b) {
            lane_amps.push(nd);
            this += 1;
        }
        if this > 0 {
            amp_splits += 1;
            amp_split_kind = Some(*kind);
        }
    }
    let trunk_amp = active
        .iter()
        .copied()
        .any(|nd| !in_groups(&split_groups, &nd.group_id) && !in_groups(&post_merge, &nd.group_id));
    if !lane_amps.is_empty() {
        if amp_splits > 1 {
            return Err("complex multi-split routing — level manually".to_string());
        }
        if trunk_amp {
            return Err("mixed pre-split + parallel amps — level manually".to_string());
        }
        let kind = amp_split_kind.unwrap_or(ParallelKind::Merged);
        let knobs = lane_amps
            .into_iter()
            .map(resolve)
            .collect::<Result<Vec<_>, _>>()?;
        // A single-amp parallel (only one lane has an amp) is just a single knob.
        let kind = if knobs.len() < 2 {
            ParallelKind::Single
        } else {
            kind
        };
        return Ok((knobs, kind));
    }

    // 3. Pure series (no split-lane amps): the last active amp overall is the master.
    Ok((
        vec![resolve(active.last().copied().unwrap())?],
        ParallelKind::Single,
    ))
}

/// Build per-scene [`leveller::SceneJob`]s from the pre-pass docs, ROUTING-AWARE:
/// classify each scene's amp set by position in the route graph (series=last amp;
/// parallel-merged=one amp per lane → joint-k) via [`classify_scene_knobs`], taking
/// each knob's CURRENT value from that scene's overlay. A scene the classifier can't
/// safely level (unknown/incomplete routing, mic/dual-input, split-output pending the
/// routing read, an amp lane with no outputLevel knob, tangled multi-split) becomes an
/// `Err` for that scene — never a silent single-amp fallback.
fn build_scene_jobs(
    scene_slots: &[u32],
    candidates: &[LevelBlockArg],
    docs: &[(u32, Option<serde_json::Value>)],
) -> Result<Vec<leveller::SceneJob>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs an amp outputLevel control".to_string());
    }
    let structure = structure_graph(docs).ok_or_else(|| {
        "no complete routing read (template missing from every scene doc) — \
         can't classify scene routing safely"
            .to_string()
    })?;
    // Preset-wide un-levelable routing (unknown template / mic / split-output) is a hard
    // error — the whole preset can't be scene-leveled. Per-SCENE issues below become skip
    // jobs so one bad scene doesn't abort the batch.
    check_levelable_routing(&structure)?;
    let jobs = scene_slots
        .iter()
        .map(|scene| {
            let doc = docs
                .iter()
                .find(|(s2, _)| s2 == scene)
                .and_then(|(_, d)| d.clone())
                .unwrap_or(serde_json::Value::Null);
            let scene_slot = if *scene >= session::BASE_SCENE_SLOT {
                None
            } else {
                Some(*scene)
            };
            match classify_scene_knobs(&structure, &doc, candidates) {
                Ok((triples, kind)) => {
                    let knobs = triples
                        .into_iter()
                        .map(|(group_id, node_id, current)| {
                            let (lo, hi) = knob_bounds(current);
                            leveller::KnobTarget {
                                knob: leveller::LevelKnob::Block {
                                    group_id,
                                    node_id,
                                    parameter_id: "outputLevel".to_string(),
                                    scene_slot,
                                },
                                lo,
                                hi,
                                current,
                            }
                        })
                        .collect::<Vec<_>>();
                    let rebalanceable = kind == ParallelKind::Merged && knobs.len() >= 2;
                    leveller::SceneJob {
                        scene_slot: *scene,
                        knobs,
                        skip: None,
                        rebalanceable,
                    }
                }
                Err(reason) => leveller::SceneJob {
                    scene_slot: *scene,
                    knobs: Vec::new(),
                    skip: Some(reason),
                    rebalanceable: false,
                },
            }
        })
        .collect();
    Ok(jobs)
}

/// Un-engaged pre-pass for the app's batched scene leveling: ONE rich session
/// loads the preset and harvests each requested scene's live field-3 doc (the
/// knob-pick input). Base (wire 8) is captured from the post-load push BEFORE
/// any scene recall. Must run before any re-amp engage — the device pushes no
/// field-3 while engaged.
fn prepass_scene_docs(
    slot: u32,
    scene_slots: &[u32],
) -> Result<Vec<(u32, Option<serde_json::Value>)>, String> {
    let mut s = Session::connect()?;
    for _ in 0..8 {
        s.heartbeat()?;
        s.pump_collect(120)?;
    }
    s.raw.clear();
    s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
    for _ in 0..6 {
        s.heartbeat()?;
        s.pump_collect(200)?;
    }
    let base_doc = s.current_preset_value().ok();
    let mut docs = Vec::with_capacity(scene_slots.len());
    for &scene in scene_slots {
        if scene >= session::BASE_SCENE_SLOT {
            docs.push((scene, base_doc.clone()));
        } else {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(scene as u64), 300)?;
            let mut doc = None;
            for _ in 0..4 {
                s.heartbeat()?;
                s.pump_collect(150)?;
                if let Ok(v) = s.current_preset_value() {
                    doc = Some(v);
                    break;
                }
            }
            docs.push((scene, doc));
        }
    }
    Ok(docs)
}

/// Closed-loop search bounds for a block knob, from its current value: amplitude
/// params (0..1) search the full [0,1]; dB-unit params (current outside [0,1],
/// e.g. an IR `outputlevel`) search a ±window around the current value, capped at
/// 0 dB on top.
fn knob_bounds(current: f32) -> (f32, f32) {
    if (0.0..=1.0).contains(&current) {
        (0.0, 1.0)
    } else {
        (current - 18.0, (current + 6.0).min(0.0))
    }
}

/// Phase-4 GATE 1 spike: capture the device's USB-Out for `secs` seconds in
/// normal mode (no playback) while the user plays their real guitar, and report
/// each input channel's peak/RMS in dBFS. Validates that the dry instrument
/// (USB-Out 3 → input channel index 2) is capturable for Tier-2 calibration.
pub fn probe_capture_input(secs: f32) -> Result<String, String> {
    // Ensure normal mode (re-amp OFF) so the rear instrument input flows.
    if let Ok(mut s) = Session::connect() {
        let _ = s.set_reamp_mode(false);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let cap = audio::capture_input(secs, 48_000)?;
    let mut out = format!(
        "captured {secs:.1}s across {} input channels:\n",
        cap.channels
    );
    for ch in 0..cap.channels {
        let dbfs = |v: f32| {
            if v > 1e-9 {
                20.0 * v.log10()
            } else {
                f32::NEG_INFINITY
            }
        };
        let peak = cap.channel_peak(ch);
        let rms_dbfs = dbfs(cap.channel_rms(ch));
        // Both metrics on the IDENTICAL samples: (LUFS − RMS) is the K-weighting
        // boost; comparing it across guitars cancels playing level → brightness.
        let lufs = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)
            .map(|l| l.integrated_lufs)
            .unwrap_or(f64::NEG_INFINITY);
        let boost = if lufs.is_finite() && rms_dbfs.is_finite() {
            format!("  K-boost {:+.2}", lufs - rms_dbfs as f64)
        } else {
            String::new()
        };
        let note = match ch {
            0 | 1 => " (USB-Out 1/2 — processed)",
            2 => " (USB-Out 3 — DRY INSTRUMENT)",
            3 => " (USB-Out 4 — dry mic/line)",
            _ => "",
        };
        out += &format!(
            "  ch{ch}: peak {:+.1} dBFS  rms {:+.1} dBFS  lufs {:+.1}{boost}{note}\n",
            dbfs(peak),
            rms_dbfs,
            lufs,
        );
    }
    Ok(out)
}

/// Phase-4 GATE 2 spike: map the re-amp inject's input→output transfer by sweeping
/// the injected stimulus amplitude (same `presetLevel`) and measuring captured
/// loudness at each. Each −6 dB amplitude step should drop output ~6 LU IF the
/// path is linear there. A clean preset that stays linear at low drive but
/// compresses near the top = normal amp behavior (Tier-2 valid). A path that's
/// flat at ALL levels = the tap/input is normalized (Tier-2 premise broken).
/// Stimulus via `TMP_LEVELLER_STIMULUS`. Load `slot` = a CLEAN preset.
pub fn probe_reamp_agc_test(slot: u32) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let base = read_stimulus_48k(&stim_path)?;
    let base_peak = base.iter().fold(0.0f32, |m, &x| m.max(x.abs()));

    // Load the preset in its own connection, settle.
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    // Measure the injected stimulus (scaled) at a fixed presetLevel; fresh conn.
    let measure = |scale: f32| -> Result<f64, String> {
        let stim: Vec<f32> = base.iter().map(|x| x * scale).collect();
        let mut s = Session::connect()?;
        s.set_preset_level(0.5)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = s.set_reamp_mode(true)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        let cap = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        let cap = cap?;
        let (ch, _) = cap.loudest_channel();
        let m = lufs::measure_mono(&cap.channel(ch), cap.sample_rate)?.integrated_lufs;
        if !m.is_finite() {
            return Err("no signal captured (re-amp may not have routed)".to_string());
        }
        Ok(m)
    };

    // Sweep amplitude in −6 dB steps: 1.0, 0.5, 0.25, 0.125 of the base peak.
    let scales = [1.0f32, 0.5, 0.25, 0.125];
    let mut out =
        format!("slot {slot} re-amp inject sweep (base peak {base_peak:.3}, presetLevel 0.5):\n");
    let mut prev: Option<f64> = None;
    let mut max_step_drop = 0.0f64; // most negative adjacent Δ (steepest = most linear)
    for sc in scales {
        let l = measure(sc)?;
        let step = prev.map(|p| l - p);
        out += &format!(
            "  peak {:.4}  →  {:.2} LUFS{}\n",
            base_peak * sc,
            l,
            step.map(|d| format!("   (Δ {d:+.2} LU vs prev −6 dB step)"))
                .unwrap_or_default(),
        );
        if let Some(d) = step {
            if d < max_step_drop {
                max_step_drop = d;
            }
        }
        prev = Some(l);
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    let verdict = if max_step_drop < -3.0 {
        "LINEAR somewhere (a −6 dB step dropped >3 LU) → stimulus amplitude drives the chain; \
         Tier-2 calibration is valid ✓"
    } else if max_step_drop > -1.0 {
        "FLAT at every level (no −6 dB step dropped >1 LU) → the re-amp inject is normalized; \
         Tier-2 premise BROKEN ✗"
    } else {
        "WEAK response at all levels → inject amplitude barely matters here; Tier-2 value is \
         marginal — inspect the sweep before building calibration"
    };
    out += &format!("  steepest −6 dB step: {max_step_drop:+.2} LU\n  {verdict}\n");
    Ok(out)
}

/// A bundled stimulus sample the user can pick per preset.
#[derive(Serialize)]
struct SampleInfo {
    /// Display label (file stem, e.g. "humbucker").
    name: String,
    /// Absolute path passed back as `stimulus_path`.
    path: String,
}

/// List the synthetic stimulus samples bundled in the app's resource dir.
#[tauri::command]
fn list_samples(app: tauri::AppHandle) -> Result<Vec<SampleInfo>, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .resolve("resources/samples", tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("resolve samples dir: {e}"))?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("read {dir:?}: {e}"))?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("wav") {
            let name = p
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(path) = p.to_str() {
                out.push(SampleInfo {
                    name,
                    path: path.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A shipped pickup topology surfaced to the UI (Settings "Pickups" dropdown).
/// Mirrors the display-relevant fields of `topologies::Topology`; synth params
/// stay backend-only.
#[derive(Serialize)]
struct TopologyInfo {
    id: String,
    label: String,
    instrument: String,
}

/// List the shipped pickup topologies (the catalog backing instrument profiles).
/// Supersedes `list_samples` for the UI — profiles reference a topology by `id`.
#[tauri::command]
fn list_pickup_topologies() -> Vec<TopologyInfo> {
    topologies::TOPOLOGIES
        .iter()
        .map(|t| TopologyInfo {
            id: t.id.to_string(),
            label: t.label.to_string(),
            instrument: t.instrument.to_string(),
        })
        .collect()
}

/// Load the persisted profile store (instrument profiles + per-slot assignments).
#[tauri::command]
fn get_store<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<profiles::Store, String> {
    profiles::load(&app)
}

/// Replace the profile list (keeps the per-slot assignment map intact).
#[tauri::command]
fn save_profiles(app: tauri::AppHandle, profiles: Vec<profiles::Profile>) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.profiles = profiles;
    // Drop assignments that now point at a deleted profile.
    let live: std::collections::HashSet<&str> =
        store.profiles.iter().map(|p| p.id.as_str()).collect();
    store
        .profile_by_slot
        .retain(|_, id| live.contains(id.as_str()));
    self::profiles::save(&app, &store)
}

/// Replace the user's loudness targets (the named live levels edited in Settings).
#[tauri::command]
fn save_targets(app: tauri::AppHandle, targets: Vec<profiles::Target>) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.targets = targets;
    self::profiles::save(&app, &store)
}

/// Set the playback loudness leveling compensates for (Settings "Playback level").
#[tauri::command]
fn set_playback_level(app: tauri::AppHandle, level: profiles::PlaybackLevel) -> Result<(), String> {
    let mut store = self::profiles::load(&app)?;
    store.playback_level = level;
    self::profiles::save(&app, &store)
}

/// Resolve a topology id to its bundled stimulus WAV path in the resource dir.
/// Returns an error for an unknown id or unbundled WAV.
fn topology_wav_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: &str,
) -> Result<String, String> {
    use tauri::Manager;
    topologies::by_id(topology_id)
        .ok_or_else(|| format!("unknown pickup topology '{topology_id}'"))?;
    let res = app
        .path()
        .resolve(
            format!("resources/samples/{topology_id}.wav"),
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| format!("resolve topology wav: {e}"))?;
    res.to_str()
        .map(str::to_string)
        .ok_or_else(|| "topology wav path not UTF-8".to_string())
}

/// One leveling job from the UI: a preset slot + the LUFS target to hit.
#[derive(serde::Deserialize)]
struct LevelJob {
    slot: u32,
    target_lufs: f64,
    /// Persist the computed `presetLevel` to the preset (SaveCurrentPreset).
    save: bool,
    /// Selected instrument's pickup topology id → its bundled stimulus WAV.
    topology_id: Option<String>,
    /// Tier-2 calibration: the profile's measured real output (K-weighted LUFS).
    /// When set, the stimulus is scaled to this loudness before injection.
    calibration_lufs: Option<f32>,
    /// Optional explicit stimulus override (takes precedence over `topology_id`).
    stimulus_path: Option<String>,
    /// Block-knob leveling: when all three are set, level by driving this block
    /// control (ChangeParameter, closed loop) instead of the master `presetLevel`.
    /// Coordinates come from `list_level_blocks`.
    block_group_id: Option<String>,
    block_node_id: Option<String>,
    block_parameter_id: Option<String>,
    /// The block param's current value (from `list_level_blocks`) — used to pick
    /// closed-loop search bounds (amplitude 0..1 vs dB-unit) without re-enumerating.
    block_value: Option<f32>,
}

/// Enumerate a preset's level-type block controls so the UI can offer them as
/// leveling knobs. Loads `slot` then reconnects (discovery handshake) to read its
/// `audioGraph` — runs with the app's seize released, like the leveling commands.
#[tauri::command]
async fn list_level_blocks(
    state: State<'_, AppState>,
    slot: u32,
) -> Result<Vec<session::LevelBlock>, String> {
    let blocks = with_released_seize(state.session.clone(), move || {
        load_then_discover_blocks(slot)
    })
    .await?;
    log::info!(
        "list_level_blocks slot={slot}: {} block(s): {}",
        blocks.len(),
        blocks
            .iter()
            .map(|b| format!("[{}]{}={:.3}", b.model_id, b.parameter_id, b.value))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(blocks)
}

// ── Active-preset signal chain: live reads + deliberate writes ──────────────────
// The connected device is the single source of truth.
// Reads parse the field-3 partial (block strip) and the songListResponse.
// Writes are DELIBERATE — every one fires only on an explicit human click in the
// ritual UI (confirm → write → read-back verify); none ever runs unattended.

/// The active preset's signal-chain graph for the "now playing" strip
/// (blocks + routing, read live via the field-78 discovery handshake). No load —
/// reads whatever preset is currently active on the device.
#[tauri::command]
async fn read_active_preset(state: State<'_, AppState>) -> Result<session::ActiveGraph, String> {
    with_released_seize(state.session.clone(), move || {
        discover_active_graph().map(|(graph, _)| graph)
    })
    .await
}

/// The monitor's CURRENT cached graph — the startup snapshot's graph, which
/// `monitor::refresh_snapshot_graph` keeps current on every field-3 push. A cheap
/// no-device-I/O, no-lock read (mirrors `list_presets`'s snapshot path) that lets a
/// freshly-mounted view re-seed its hero after a graphless connect, without the
/// heavy `read_active_preset` discovery. `None` when the cache has no graph yet.
#[tauri::command]
async fn current_graph() -> Result<Option<session::ActiveGraph>, String> {
    Ok(monitor::startup_graph())
}

/// Scene metadata for one preset, returned by the pure-lazy field-8 read.
#[derive(Clone, Serialize)]
struct PresetScenes {
    scenes: Vec<String>,
    fs: Vec<Option<u32>>,
    /// Block-acting footswitches (on/off + parameter change), with leveling-candidate
    /// params — empty when the preset has none.
    footswitches: Vec<footswitch::FootswitchInfo>,
}

fn decode_preset_scenes(json: &[u8]) -> Result<PresetScenes, String> {
    let live = session::decode_plain_preset_live(json)
        .ok_or_else(|| "could not parse preset scene JSON".to_string())?;
    let scenes = live
        .scene_names
        .ok_or_else(|| "preset scene JSON truncated before scenes".to_string())?;
    let map = live.ftsw.as_ref().map(footswitch::scene_fs_map);
    let fs = (0..scenes.len())
        .map(|i| {
            map.as_ref()
                .and_then(|m| m.get(&(i as u32)).copied())
                .map(|sw| sw + 1)
        })
        .collect();
    // Block-acting footswitches need the FULL preset (dspUnitParameters), which the
    // ActiveGraph drops — re-parse the raw field-8 JSON (tolerant: it survives the
    // scene-tail truncation; ftsw + audioGraph are well before it).
    let footswitches = match (
        session::tolerant_parse_json(&String::from_utf8_lossy(json)),
        live.ftsw.as_ref(),
    ) {
        (Some(preset), Some(ftsw)) => footswitch::enumerate_block_footswitches(ftsw, &preset),
        _ => Vec::new(),
    };
    Ok(PresetScenes {
        scenes,
        fs,
        footswitches,
    })
}

fn read_preset_scenes_fresh(list_index: u32) -> Result<PresetScenes, String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(list_index + 1)?
        .ok_or_else(|| format!("no preset scene data returned for slot {}", list_index + 1))?;
    decode_preset_scenes(&json)
}

/// Pure-lazy scene read for one preset. It never loads the preset: the command reads
/// slot-addressed field-8 plaintext JSON (`presetDataRequest` → `presetDataChanged`)
/// and decodes scene names + real footswitch tags from `ftsw`. It first tries the
/// monitor's metadata lane; when the monitor is not live it falls back to the proven
/// pause + fresh-session path.
#[tauri::command]
async fn read_preset_scenes(
    state: State<'_, AppState>,
    list_index: u32,
) -> Result<PresetScenes, String> {
    if let Some(result) = monitor::try_metadata_read(list_index) {
        match result {
            Ok(Some(json)) => return decode_preset_scenes(&json),
            Ok(None) => {
                log::warn!("read_preset_scenes: monitor lane returned no data; falling back")
            }
            Err(e) => return Err(e),
        }
    }
    with_released_seize(state.session.clone(), move || {
        read_preset_scenes_fresh(list_index)
    })
    .await
}

/// One streamed row of the Level dialog's selected-preset scene scan. `result`
/// is `None` when the slot read went unanswered or undecodable — the dialog
/// renders that preset as scanned-with-no-scenes (block roles still level it).
#[derive(Clone, Serialize)]
struct SceneScanItem {
    list_index: u32,
    result: Option<PresetScenes>,
}

/// Cooperative cancel for [`scan_preset_scenes`] — set by `cancel_scene_scan`
/// ("Skip — load during the run" / closing the dialog), checked between reads.
static SCENE_SCAN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn cancel_scene_scan() {
    SCENE_SCAN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Batch scene scan for the Level dialog: ONE dedicated lean session reading
/// every selected preset's field-8 plaintext JSON back-to-back (the HW-proven
/// `scan_all_scenes` / `probe --scenes-passive` recipe, ~0.5 s per preset),
/// streaming each preset's scenes over `on_result` as it lands so rows render
/// progressively. NON-DESTRUCTIVE — zero LoadPreset; the device's active preset
/// only ever changes later, in the post-disclaimer leveling RUN. Per-preset
/// monitor-lane reads (`read_preset_scenes`) pay ~3× per read in heartbeat
/// contention + IPC; batches must use this instead.
#[tauri::command]
async fn scan_preset_scenes(
    state: State<'_, AppState>,
    list_indices: Vec<u32>,
    on_result: tauri::ipc::Channel<SceneScanItem>,
) -> Result<(), String> {
    SCENE_SCAN_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Drain the handshake flood before the first re-armed read (a read
        // fired mid-flood is dropped device-side — the classic 0/25).
        s.drain_until_quiet(250, 20)?;
        for &idx in &list_indices {
            if SCENE_SCAN_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            // Per-read failures degrade to a `None` row instead of aborting the
            // sweep — one unanswered slot must not strand the remaining rows.
            let result = match s.read_slot_preset_json(idx + 1) {
                Ok(Some(json)) => decode_preset_scenes(&json).ok(),
                Ok(None) | Err(_) => None,
            };
            let _ = on_result.send(SceneScanItem {
                list_index: idx,
                result,
            });
        }
        Ok(())
    })
    .await
}

/// One row of the active preset's live scene list (`sceneListResponse`). `fs` is the
/// best-effort footswitch tag — `None` for now (FS-tag RE is out of scope; the UI
/// renders an em-dash for null). Mirrors the monitor's `tmp://scene-list` rows.
#[derive(Serialize)]
struct SceneListRow {
    name: String,
    fs: Option<u32>,
}

/// Fetch the ACTIVE preset's scene list on demand — `sceneListRequest` (field 126).
/// The canonical scene-row source is the monitor's field-3 decode (the preset JSON's
/// `scenes[]`, pushed on every device change AND in the connect handshake); the unit
/// pushes `sceneListResponse(125)` itself only on an actual preset SWITCH. This
/// command is a manual diagnostic top-up. Routed through `with_released_seize`
/// so it serializes via `DEVICE_OP_LOCK` (pausing the monitor) like every device op.
#[tauri::command]
async fn request_scene_list(state: State<'_, AppState>) -> Result<Vec<SceneListRow>, String> {
    with_released_seize(state.session.clone(), move || {
        let names = Session::connect()?.request_scene_list()?;
        Ok(names
            .into_iter()
            .map(|name| SceneListRow { name, fs: None })
            .collect())
    })
    .await
}

/// Stop live-sync: clear `MONITOR_ENABLED` (the monitor drops its seize on its next
/// poll), then re-establish the persistent UI session so `list_presets` / commands
/// work as before. Idempotent. Returns the firmware version of the reclaimed session
/// (like `connect_device`), or `None` if the reconnect didn't carry it / no device.
#[tauri::command]
async fn stop_live_sync(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Disable FIRST so the monitor releases its seize, THEN take the device-op gate
        // (which pauses + waits for the monitor to drop) before reconnecting the UI
        // session. Without the gate the reconnect could race the monitor's last seize.
        MONITOR_ENABLED.store(false, SeqCst);
        let _op = lock_device_op();
        *lock_ok(&arc) = None;
        let fw = match Session::connect_with_firmware() {
            Ok(s) => {
                let fw = s.firmware_version();
                *lock_ok(&arc) = Some(s);
                fw
            }
            Err(_) => None, // no device / not ready — UI session stays None (as before)
        };
        log::info!("live-sync stopped — UI session reclaimed (fw={fw:?})");
        fw
    })
    .await
    .map_err(|e| format!("stop_live_sync task failed: {e}"))
}

/// Read every Song's metadata (name / notes / BPM) for the Songs overview — the
/// net-new live `songListResponse` read (rides the handshake burst).
#[tauri::command]
async fn list_songs(state: State<'_, AppState>) -> Result<Vec<session::SongRecord>, String> {
    // Strict, fail-closed read (retry-until-complete): a read immediately after a
    // write is the worst case for the multi-packet truncation, and the Songs page
    // re-reads after every mutation, so accept only a strictly-complete response.
    with_released_seize(state.session.clone(), read_song_list).await
}

/// Make a preset the active one on the amp (`loadPreset`). A DELIBERATE action —
/// it switches the live tone — so it's a kebab item, never a row-tap. `list_index`
/// is 0-based; `session.load_preset` adds the device +1.
#[tauri::command]
async fn load_preset_on_amp(state: State<'_, AppState>, list_index: u32) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path: while live-sync is on, fire the loadPreset on the monitor's
        // persistent session (~0.2 s) instead of the release→handshake→reconnect
        // bookend (~2 s). Falls back to the classic path when the lane isn't live.
        if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadPreset(list_index)) {
            return r;
        }
        with_released_seize_blocking(arc, move || {
            let mut s = Session::connect()?;
            s.load_preset(list_index)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}

/// Permanently clear a user slot (`clearUserPreset`) — DESTRUCTIVE, no undo. Goes
/// through [`guarded_clear`]: a fresh non-destructive read in the SAME 1-based
/// device-slot space must confirm the slot still holds `expect_name` before the
/// clear fires (the lesson from the off-by-one that erased real presets). The §4
/// confirm + read-back verify happen in the UI; this is the safe primitive.
#[tauri::command]
async fn delete_preset(
    state: State<'_, AppState>,
    list_index: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        guarded_clear(list_index, &expect_name)
    })
    .await
}

/// Reorder a user preset (`moveUserPreset`). DESTRUCTIVE to slot positions (no
/// undo). 0-based list indices; `session.move_user_preset` adds the device +1.
#[tauri::command]
async fn move_preset(state: State<'_, AppState>, from: u32, to: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        s.move_user_preset(from, to)
    })
    .await
}

/// Rename a preset in place: load it, `renameCurrentPreset`, then
/// `saveCurrentPreset` over its own slot (Pro Control's rename = rename + save).
/// DESTRUCTIVE (permanent) and it LOADS the slot (switches the live tone), so it's
/// a deliberate confirmed action. `list_index` is 0-based.
#[tauri::command]
async fn rename_save_preset(
    state: State<'_, AppState>,
    list_index: u32,
    name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        // Capture the target's CURRENT name so conn2 can confirm the right preset is
        // active before renaming+saving it — a dropped load would otherwise rename+save
        // a DIFFERENT preset over this slot.
        let name_before = s
            .list_my_presets()?
            .into_iter()
            .find(|p| p.slot == list_index)
            .map(|p| p.name)
            .ok_or_else(|| format!("rename target list index {list_index} out of range"))?;
        s.load_preset(list_index)?;
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
        let mut s = Session::connect()?;
        s.confirm_active(list_index, Some(&name_before))?;
        s.rename_current_preset(&name)?;
        s.save_current_preset(list_index)
    })
    .await
}

/// Recall a scene on the device — `loadScene` (PresetMessage field 101). `scene_slot`
/// is the **0-based** `scenes[]` index within the active preset;
/// `session::BASE_SCENE_SLOT` (8) recalls the base scene (the wire constant — HW-proven
/// by the `--loadscene 1` → scenes[1] "Reverb" activegraph diff + base echoing slot 8
/// even on a 0-scene preset). The proto's `LoadScene` addresses
/// a scene of the CURRENT preset, with no preset addressing of its own. So when
/// `list_index` is `Some`, the preset is loaded first (its own connection — a load
/// and a scene-recall in the SAME connection would have the load override the
/// scene), then a fresh connection recalls the scene; when `None`, the scene is
/// recalled on whatever preset is already active. A DELIBERATE action (it switches
/// the live tone), mirroring `load_preset_on_amp`. `list_index` is 0-based;
/// `session.load_preset` adds the device +1.
#[tauri::command]
async fn load_scene_on_amp(
    state: State<'_, AppState>,
    list_index: Option<u32>,
    scene_slot: u32,
) -> Result<(), String> {
    let arc = state.session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Fast path — ACTIVE-preset scene recall only (`list_index == None`, the
        // shipped UI's normal case): fire the loadScene on the monitor's live
        // session. The `Some` case keeps the classic two-connection path — a load
        // and a scene-recall in the SAME connection have the load override the
        // scene (see the doc above), and that hazard is untested on the monitor's
        // long-lived session.
        if list_index.is_none() {
            if let Some(r) = monitor::try_live_op(monitor::LiveOp::LoadScene(scene_slot)) {
                return r;
            }
        }
        with_released_seize_blocking(arc, move || {
            if let Some(idx) = list_index {
                let mut s = Session::connect()?;
                s.load_preset(idx)?;
                drop(s);
                std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
            }
            Session::connect()?.load_scene(scene_slot)
        })
    })
    .await
    .map_err(|e| format!("device task failed: {e}"))?
}

// ─── Song & Setlist CRUD for the device-backed Songs page ─────────────────────
// Read-back-after-write: each write fires through its own fresh connection, then
// re-reads via the strict fail-closed helpers and returns the fresh authoritative
// list, so the UI never predicts the device's positional slots (which shift on
// every add/remove). All run with the app's seize released (`with_released_seize`).
// These are the proven `probe_*` flows (lib.rs §"Song / Setlist WRITE primitives")
// reshaped as slot-addressed, frontend-callable commands. HW writes are gated by
// the read-only-on-hardware policy (lifted per-session by explicit authorization).

/// Read every Setlist's name for the Songs page — strict fail-closed live read.
#[tauri::command]
async fn read_setlists(state: State<'_, AppState>) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), read_setlist_list).await
}

/// Read a setlist's songs in device order as GLOBAL song slots, trailing empty
/// (`songSlot==0`) entries dropped so the list is dense — position `i` here is the
/// `setlistSongSlot` the membership ops address (the index base is HW-pinned).
#[tauri::command]
async fn list_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Create a song; returns the fresh song list (the device assigns the slot).
#[tauri::command]
async fn add_song(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        read_song_list()
    })
    .await
}

/// Rename a song by slot; returns the fresh song list.
#[tauri::command]
async fn rename_song(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_song(slot, &name)?;
        }
        read_song_list()
    })
    .await
}

/// Delete a song by slot — DESTRUCTIVE. Guarded: a fresh read in the SAME slot space
/// must still show `expect_name` at `slot` (tolerant of duplicate names elsewhere),
/// else refuse. Returns the fresh song list.
#[tauri::command]
async fn remove_song(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_song_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("song slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: song slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_song(slot)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's notes by slot; returns the fresh song list.
#[tauri::command]
async fn set_song_notes(
    state: State<'_, AppState>,
    slot: u32,
    notes: String,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, &notes)?;
        }
        read_song_list()
    })
    .await
}

/// Set a song's numeric BPM by slot via the RE'd mechanism (there is NO dedicated
/// BPM setter): ensure the song has a footswitch (`assignSongPreset`), activate it
/// (`loadPreset tabEnum=5`), send the global `tapTempoBpm` (which the device stores
/// as the ACTIVE song's BPM — so this mutates active-song state as a side effect),
/// enable BPM display, verify by re-read. Retries the activate+tempo (the first load
/// after a fresh assign often doesn't settle). Non-convergence → Err. Returns the
/// fresh song list on success.
#[tauri::command]
async fn set_song_bpm(
    state: State<'_, AppState>,
    slot: u32,
    bpm: f32,
) -> Result<Vec<session::SongRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let (after, converged) = converge_song_bpm(slot, bpm)?;
        if converged {
            return Ok(after);
        }
        let got = after
            .iter()
            .find(|x| x.slot == slot)
            .map(|x| x.bpm)
            .unwrap_or(0);
        Err(format!(
            "BPM for song slot {slot} did not converge to {bpm} after retries (read-back={got}); \
             tempo applies to the active song and this slot may not have activated"
        ))
    })
    .await
}

/// Create a setlist; returns the fresh setlist list.
#[tauri::command]
async fn add_setlist(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist(&name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Rename a setlist by slot; returns the fresh setlist list.
#[tauri::command]
async fn rename_setlist(
    state: State<'_, AppState>,
    slot: u32,
    name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.rename_setlist(slot, &name)?;
        }
        read_setlist_list()
    })
    .await
}

/// Delete a setlist by slot — DESTRUCTIVE (the songs themselves are kept). Guarded
/// by `expect_name` in the same read space. Returns the fresh setlist list.
#[tauri::command]
async fn remove_setlist(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<Vec<session::SetlistRecord>, String> {
    with_released_seize(state.session.clone(), move || {
        let before = read_setlist_list()?;
        let rec = before
            .iter()
            .find(|r| r.slot == slot)
            .ok_or_else(|| format!("setlist slot {slot} no longer exists — refusing to delete"))?;
        if rec.name != expect_name {
            return Err(format!(
                "guarded remove refused: setlist slot {slot} reads {:?}, expected {:?} (list changed)",
                rec.name, expect_name
            ));
        }
        {
            let mut s = Session::connect()?;
            s.remove_setlist(slot)?;
        }
        read_setlist_list()
    })
    .await
}

/// Add a song (by GLOBAL song slot) to a setlist; returns the setlist's fresh
/// ordered member song slots (dense — trailing 0s dropped).
#[tauri::command]
async fn add_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Remove a song from a setlist by its POSITION within the setlist
/// (`setlist_song_slot`, NOT the global song slot). Returns fresh member slots.
#[tauri::command]
async fn remove_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    setlist_song_slot: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.remove_setlist_song(setlist_slot, setlist_song_slot)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Reorder a song within a setlist by POSITION (both indices are positions within
/// the setlist, NOT global song slots). Returns fresh member slots.
#[tauri::command]
async fn move_setlist_song(
    state: State<'_, AppState>,
    setlist_slot: u32,
    old_pos: u32,
    new_pos: u32,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.move_setlist_song(setlist_slot, old_pos, new_pos)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

// ─── Batched song/setlist transactions ────────────────────────────────────────
// The granular commands above pay one full `with_released_seize` bookend + one
// strict fail-closed read PER FIELD (a song create with notes + BPM was 3 bookends
// + 3 reads ≈ 10 s+). These transactions run the same proven per-write fresh
// connections (the wire behavior is untouched) but under ONE bookend, skipping the
// intermediate read-backs: only the final authoritative read(s) remain. Slot
// stability inside a transaction: notes/BPM/membership writes don't shift slots —
// only song add/remove do, and a transaction does at most one add (first).

/// Result of a batched song save: the fresh authoritative song list, the fresh
/// membership of `add_to_setlist` (when requested), and the best-effort BPM
/// warning (BPM is the active-song tap tempo on the unit and can fail to settle —
/// the song itself is kept, mirroring the UI's previous per-call behavior).
#[derive(serde::Serialize)]
struct SongSaveOutcome {
    songs: Vec<session::SongRecord>,
    /// `Some` only when `add_to_setlist` was requested: that setlist's fresh
    /// ordered member song slots.
    members: Option<Vec<u32>>,
    bpm_warning: Option<String>,
}

/// Best-effort BPM step shared by the batched song transactions: returns the
/// fresh song list when the converge ran (success OR non-convergence), plus the
/// warning when it didn't stick. Never fails the transaction — mirrors the UI's
/// previous "Saved, but BPM didn't stick" toast semantics.
fn apply_song_bpm_best_effort(
    slot: u32,
    bpm: f32,
) -> (Option<Vec<session::SongRecord>>, Option<String>) {
    match converge_song_bpm(slot, bpm) {
        Ok((after, true)) => (Some(after), None),
        Ok((after, false)) => {
            let got = after
                .iter()
                .find(|x| x.slot == slot)
                .map(|x| x.bpm)
                .unwrap_or(0);
            (
                Some(after),
                Some(format!("BPM didn't converge to {bpm} (read-back={got})")),
            )
        }
        Err(e) => (None, Some(e)),
    }
}

/// Create a song with optional notes / BPM / setlist membership as ONE device
/// transaction (one bookend, one final read) — replaces the UI's add → read →
/// notes → read → bpm → read → addToSetlist → read chain. The created song is
/// resolved BY NAME from the post-add read (a new song inserts at protocol slot 1
/// and shifts every other song +1 — the device assigns the slot).
#[tauri::command]
async fn create_song_full(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
    bpm: Option<f32>,
    add_to_setlist: Option<u32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        {
            let mut s = Session::connect()?;
            s.add_song(&name)?;
        }
        let mut songs = read_song_list()?;
        let Some(slot) = songs.iter().find(|s| s.name == name).map(|s| s.slot) else {
            // Created but not resolvable by name (duplicate-name edge) — return the
            // fresh list; the optional fields are skipped, surfaced as a warning.
            return Ok(SongSaveOutcome {
                songs,
                members: None,
                bpm_warning: Some(format!(
                    "song {name:?} created, but not resolvable by name — notes/BPM skipped"
                )),
            });
        };
        let notes = notes.filter(|n| !n.trim().is_empty());
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n.trim())?;
        }
        let mut bpm_warning = None;
        match bpm {
            Some(b) => {
                let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
                if let Some(fresh) = fresh {
                    songs = fresh; // the converge already re-read — reuse it
                }
                bpm_warning = warn;
            }
            None if notes.is_some() => songs = read_song_list()?,
            None => {}
        }
        let members = match add_to_setlist {
            Some(setlist_slot) => {
                {
                    let mut s = Session::connect()?;
                    s.add_setlist_song(setlist_slot, slot)?;
                }
                Some(read_setlist_songs(setlist_slot)?)
            }
            None => None,
        };
        Ok(SongSaveOutcome {
            songs,
            members,
            bpm_warning,
        })
    })
    .await
}

/// Update a song's changed fields (rename / notes / BPM) as ONE device
/// transaction (one bookend, one final read) — replaces the UI's per-field
/// command chain. `None` = field unchanged. The caller skips the call entirely
/// when nothing changed.
#[tauri::command]
async fn update_song_full(
    state: State<'_, AppState>,
    slot: u32,
    name: Option<String>,
    notes: Option<String>,
    bpm: Option<f32>,
) -> Result<SongSaveOutcome, String> {
    with_released_seize(state.session.clone(), move || {
        if let Some(n) = &name {
            let mut s = Session::connect()?;
            s.rename_song(slot, n)?;
        }
        if let Some(n) = &notes {
            let mut s = Session::connect()?;
            s.set_song_notes(slot, n)?;
        }
        let mut songs = None;
        let mut bpm_warning = None;
        if let Some(b) = bpm {
            let (fresh, warn) = apply_song_bpm_best_effort(slot, b);
            songs = fresh;
            bpm_warning = warn;
        }
        let songs = match songs {
            Some(s) => s,
            None => read_song_list()?, // one final authoritative read
        };
        Ok(SongSaveOutcome {
            songs,
            members: None,
            bpm_warning,
        })
    })
    .await
}

/// Add several songs (by GLOBAL song slot) to a setlist under ONE bookend with ONE
/// final membership read — replaces the UI's per-song `add_setlist_song` loop
/// (which paid a bookend + membership read per song). Same proven per-write fresh
/// connection per `addSetlistSong`.
#[tauri::command]
async fn add_setlist_songs(
    state: State<'_, AppState>,
    setlist_slot: u32,
    song_slots: Vec<u32>,
) -> Result<Vec<u32>, String> {
    with_released_seize(state.session.clone(), move || {
        for ss in &song_slots {
            let mut s = Session::connect()?;
            s.add_setlist_song(setlist_slot, *ss)?;
        }
        read_setlist_songs(setlist_slot)
    })
    .await
}

/// Resolve the stimulus WAV: explicit path → selected topology → `TMP_LEVELLER_STIMULUS`
/// env → the default bundled synthetic sample.
fn resolve_stimulus<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    explicit: Option<String>,
    topology_id: Option<String>,
) -> Result<String, String> {
    // Offline e2e: a fixed repo stimulus WAV (MockRuntime can't resolve bundle resources).
    #[cfg(feature = "e2e")]
    if let Ok(p) = std::env::var("TMP_E2E_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        return Ok(p);
    }
    if let Some(tid) = topology_id.filter(|t| !t.is_empty()) {
        return topology_wav_path(app, &tid);
    }
    if let Ok(p) = std::env::var("TMP_LEVELLER_STIMULUS") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    topology_wav_path(app, topologies::DEFAULT_TOPOLOGY_ID)
}

/// Process-global device-operation gate (1 permit). The TMP is single-connection
/// exclusive-HID, and `AppState.session`'s `Mutex<Option<Session>>` only guards the
/// held-session SLOT — not the whole open→work→close→reconnect lifecycle of an
/// operation. So two operations can overlap: e.g. the Presets tab's
/// `read_active_preset` is still in its trailing reconnect (`with_released_seize`
/// re-acquire) when the Songs tab's `list_songs` starts, and the two
/// `IOHIDDeviceOpen`s collide with `0xe00002c5` (mis-reported as "close Pro
/// Control"). Every device operation holds this gate for its FULL duration.
/// Acquired INSIDE the `spawn_blocking` closure so the guard's lifetime is the
/// blocking work itself — it survives even if the async command future is dropped
/// (spawn_blocking work is not cancelled), and a panic only poisons it transiently
/// (recovered via `into_inner`, never permanently bricking device IO).
static DEVICE_OP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

/// Monitor intent: when set, the persistent device monitor (`monitor.rs`) owns the
/// idle HID seize, streams unsolicited unit pushes, and publishes the startup
/// snapshot. `connect_device` sets this after releasing any old UI session; commands
/// borrow the device through `DEVICE_OP_LOCK` + pause/ack. `stop_live_sync` is kept
/// for diagnostics/settings paths that explicitly need to reclaim a UI session.
pub(crate) static MONITOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// A command (holding [`DEVICE_OP_LOCK`]) asks the persistent device monitor to
/// yield its exclusive HID seize so the command can open its own connection
/// without a `0xe00002c5` collision. Set true while a command's [`MonitorPauseGuard`]
/// is alive; cleared on its Drop. The monitor polls this every pump iteration.
pub(crate) static MONITOR_PAUSE_REQ: AtomicBool = AtomicBool::new(false);
/// The monitor has dropped its `Session` (its seize is free) in response to a pause
/// request. The command waits (bounded) for this ack before proceeding. Cleared by
/// the monitor when it resumes after the request clears.
pub(crate) static MONITOR_PAUSED_ACK: AtomicBool = AtomicBool::new(false);

/// Bounded wait for the monitor to ack a pause (≈ `PAUSE_WAIT_TRIES × 25 ms`). The
/// monitor pumps in ~120 ms windows, so it checks the flag ~8×/sec; 40 × 25 ms = 1 s
/// is generous. If the budget is exceeded (monitor mid-connect on a congested
/// device), the command proceeds anyway — `hid.rs`'s bounded `IOHIDDeviceOpen` retry
/// (≤0.48 s on `0xe00002c5`) absorbs the residual race, the same safety net that
/// already covers `with_released_seize`'s own drop→reconnect lag.
const PAUSE_WAIT_TRIES: u32 = 40;
const PAUSE_WAIT_STEP_MS: u64 = 25;

/// RAII guard returned by [`lock_device_op`]: holds [`DEVICE_OP_LOCK`] AND keeps the
/// monitor paused (`MONITOR_PAUSE_REQ` true) for the guard's whole lifetime. On Drop
/// it clears the pause request (the monitor resumes + re-reads fresh state) and
/// releases the device-op lock. So the monitor stays parked for exactly the command's
/// release→work→reconnect window — it cannot interleave a seize between the command's
/// own fresh connections (which would break the leveller's latch model). Runs on
/// unwind too, so a command panic still resumes the monitor.
struct MonitorPauseGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for MonitorPauseGuard {
    fn drop(&mut self) {
        MONITOR_PAUSE_REQ.store(false, SeqCst);
    }
}

/// Acquire the device-operation gate (poison-tolerant) AND pause the persistent
/// monitor so this command owns the device exclusively. Serializes against other
/// commands first (the existing behavior), THEN asks the monitor to drop its seize
/// and waits (bounded) for the ack. Hold the returned guard for the whole device
/// operation; its Drop resumes the monitor. See [`DEVICE_OP_LOCK`] / [`MonitorPauseGuard`].
///
/// Deadlock-free by construction: the monitor acquires NO lock, so the command's
/// bounded *sleep* on `MONITOR_PAUSED_ACK` is never a lock-acquire cycle. The monitor
/// owns only the device, which the pause protocol forces it to release.
fn lock_device_op() -> MonitorPauseGuard {
    let g = lock_ok(&DEVICE_OP_LOCK);
    MONITOR_PAUSE_REQ.store(true, SeqCst); // ask the monitor to yield its seize
                                           // Only wait for the ack while the monitor is actually enabled — a disabled
                                           // monitor never acks (it idles in its disabled branch), so waiting would burn
                                           // the full `PAUSE_WAIT_TRIES × 25 ms = 1 s` budget on EVERY command whenever
                                           // live-sync is off. The one transition where the flag is already false while
                                           // the monitor still holds its seize for ≤1 pump (`stop_live_sync` clears it
                                           // before locking) is absorbed by hid.rs's bounded open-retry, as documented
                                           // on PAUSE_WAIT_TRIES.
    if MONITOR_ENABLED.load(SeqCst) {
        let mut acked = false;
        for _ in 0..PAUSE_WAIT_TRIES {
            if MONITOR_PAUSED_ACK.load(SeqCst) {
                acked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(PAUSE_WAIT_STEP_MS));
        }
        if !acked {
            // Proceeding anyway (hid.rs's open-retry covers the seize-recycle race), but a
            // persistent no-ack means the monitor is wedged — every device op then pays the
            // full wait. Surface it instead of silently eating the latency.
            log::warn!(
                "device op proceeding without a monitor pause-ack ({PAUSE_WAIT_TRIES} tries × \
                 {PAUSE_WAIT_STEP_MS}ms) — the monitor may be wedged"
            );
        }
    }
    // Proceed even if not acked within budget (see PAUSE_WAIT_TRIES) — hid.rs's
    // open-retry covers the residual seize-recycle race.
    MonitorPauseGuard(g)
}

/// Settle gap before re-establishing the UI session, so the IOKit seize the
/// device work just released has time to free up before we re-open it.
const RECONNECT_AFTER_MS: u64 = 400;

/// Run blocking device work with the app's HID seize released — the leveller and
/// calibration open their own fresh connections, so the app must NOT hold a
/// competing seize while they run. Re-establishes a live session for the UI
/// afterward regardless of outcome, so the connection/preset list survive. This
/// release→work→reconnect bookend is shared by every command that drives the
/// device through its own connections.
async fn with_released_seize<T, F>(arc: Arc<Mutex<Option<Session>>>, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || with_released_seize_blocking(arc, work))
        .await
        .map_err(|e| format!("device task failed: {e}"))?
}

/// Blocking core of [`with_released_seize`] — split out so commands that try the
/// monitor's live command lane first (`monitor::try_live_op`) can fall back to the
/// release→work→reconnect bookend inside their own `spawn_blocking`.
fn with_released_seize_blocking<T, F>(
    arc: Arc<Mutex<Option<Session>>>,
    work: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let _op = lock_device_op(); // serialize the whole release→work→reconnect
    *lock_ok(&arc) = None;
    let result = work();
    // Re-establish the UI session so the connection / preset list survive the
    // command — UNLESS live-sync is active, in which case the MONITOR owns the
    // device: re-grabbing the UI seize here would leave `session = Some` and
    // permanently block the monitor on its `is_none()` opportunism check (the
    // hero would stay stuck "Reading active preset…"). When live-sync owns the
    // device, leave the seize RELEASED and let the monitor re-take it on its
    // next poll (the `_op` guard's Drop clears the pause that paused it) — and
    // skip the settle sleep too: it only exists to protect OUR immediate re-open
    // below, and the monitor's own connect path already absorbs the kernel's
    // seize-recycle lag (hid.rs bounded open-retry + its reconnect backoff).
    if !MONITOR_ENABLED.load(SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(RECONNECT_AFTER_MS));
        if let Ok(s) = Session::connect() {
            *lock_ok(&arc) = Some(s);
        }
    }
    result
}

/// Fletcher–Munson playback compensation for a leveling job: the LU offset added
/// to the target, from the store's playback level × the stimulus topology's
/// instrument family. Equal-LUFS is equal-loudness only at the SPL the K-weighting
/// curve approximates (~stage volume); at quieter playback the equal-loudness
/// contours steepen and a bass preset matched at equal LUFS sits perceptibly
/// quieter, so its target is raised (see `profiles::playback_offset_lu`). `None` /
/// unknown topology falls back to the guitar default (offset 0); `Stage` (the
/// store default) is always 0, so legacy stores level exactly as before.
fn playback_offset_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    topology_id: Option<&str>,
) -> f64 {
    let level = profiles::load(app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    profiles::playback_offset_lu(level, stimulus_instrument(topology_id))
}

/// The instrument family a leveling job's stimulus belongs to (`None` / unknown
/// topology = the guitar default).
fn stimulus_instrument(topology_id: Option<&str>) -> &'static str {
    topologies::by_id(topology_id.unwrap_or(topologies::DEFAULT_TOPOLOGY_ID))
        .map(|t| t.instrument)
        .unwrap_or("guitar")
}

/// Level one preset to its target (the real, one-shot open-loop path). The
/// leveller opens its own fresh connections (load → measure → set), so the work
/// runs with the app's seize released (see `with_released_seize`).
#[tauri::command]
async fn level_preset<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    job: LevelJob,
) -> Result<leveller::LevelResult, String> {
    let LevelJob {
        slot,
        target_lufs,
        save,
        topology_id,
        calibration_lufs,
        stimulus_path,
        block_group_id,
        block_node_id,
        block_parameter_id,
        block_value,
    } = job;
    let offset_lu = playback_offset_for(&app, topology_id.as_deref());
    if offset_lu != 0.0 {
        log::info!("level_preset slot={slot}: playback compensation {offset_lu:+.1} LU on target {target_lufs:.1}");
    }
    let target_lufs = target_lufs + offset_lu;
    let stim_path = resolve_stimulus(&app, stimulus_path, topology_id)?;
    // A block knob is selected only when all three coordinates are present;
    // otherwise level the master `presetLevel` (the validated one-shot path).
    let block = match (block_group_id, block_node_id, block_parameter_id) {
        (Some(g), Some(n), Some(p)) if !g.is_empty() && !n.is_empty() && !p.is_empty() => {
            Some((g, n, p))
        }
        _ => None,
    };
    // Reset the cooperative cancel flag for this run; `cancel_preset_leveling` sets it
    // (it only flips the atomic — no device lock — so it runs while this op holds it).
    PRESET_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let opts = leveller::LevelOptions { save, verify: true, ..Default::default() };
        let cancelled = || PRESET_LEVEL_CANCEL.load(SeqCst);
        let result = match block {
            Some((group_id, node_id, parameter_id)) => {
                let (lo, hi) = knob_bounds(block_value.unwrap_or(0.5));
                let knob = leveller::LevelKnob::Block { group_id, node_id, parameter_id, scene_slot: None };
                leveller::level_preset_block(slot, &stim, &knob, lo, hi, target_lufs, opts, cancelled)
            }
            None => {
                // Isolate the Base measurement: force EVERY footswitch on/off block OFF so we
                // measure the clean base sound, not "base + whatever pedals are saved on".
                // ponytail: costs one ~1 s preset read per Base run (even presets with no FS
                // blocks). Optimization path: thread an all-on/off force-list hint from the
                // frontend backup scan onto LevelJob (NOT footswitchesPerIndex — that's filtered
                // to levelable-param switches, while isolation needs ALL on-off blocks).
                if cancelled() {
                    return leveller::level_preset(slot, &stim, target_lufs, opts, &[], cancelled);
                }
                // Best-effort: isolation is a quality improvement, not a precondition for
                // leveling at all. A read hiccup (or, offline, a preset-read the fake device
                // doesn't model) must not fail the whole Base run — degrade to no isolation
                // (pre-this-feature behavior) instead of propagating the error.
                let force_bypass: Vec<(String, String, bool)> = match read_slot_preset_parsed(slot)
                {
                    Ok((preset, _, _)) => {
                        std::thread::sleep(std::time::Duration::from_millis(
                            leveller::RECONNECT_GAP_MS,
                        ));
                        footswitch::all_onoff_blocks(
                            preset.get("ftsw").unwrap_or(&serde_json::Value::Null),
                        )
                        .into_iter()
                        .map(|(g, n)| (g, n, true))
                        .collect()
                    }
                    Err(e) => {
                        log::warn!(
                            "level_preset slot={slot}: base-isolation preset read failed ({e}), leveling without isolation"
                        );
                        Vec::new()
                    }
                };
                leveller::level_preset(slot, &stim, target_lufs, opts, &force_bypass, cancelled)
            }
        };
        match &result {
            Ok(r) => log::info!(
                "level_preset slot={} save={} measured={:.2} LUFS target={:.2} LUFS final_level={:.4} verify={:?}",
                r.slot,
                r.saved,
                r.measured_lufs,
                r.target_lufs,
                r.final_level,
                r.verify_lufs,
            ),
            Err(e) => log::warn!("level_preset slot={slot} save={save} failed: {e}"),
        }
        result
    })
    .await
}

/// A candidate leveling knob for `level_scenes_apply` — the frontend passes EVERY
/// amp-level candidate (it owns amp-ness via the models catalog); the backend picks
/// PER SCENE the one whose block is actually ON in that scene.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LevelBlockArg {
    group_id: String,
    node_id: String,
    parameter_id: String,
    value: f32,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneLevelProgressItem {
    scene_slot: u32,
    status: String,
    result: Option<leveller::LevelResult>,
    message: Option<String>,
}

/// Wire payload for `tmp://leveling-lufs` — the advisory live measured loudness streamed
/// while a leveling capture runs, so the UI can show a "measuring…" readout. ADVISORY: this
/// is the loudness at the reference level, NOT the final preset level (the result row is the
/// confirm). Mirrored in `src/lib/types.ts`.
#[derive(Clone, serde::Serialize)]
struct LiveLufsEvent {
    lufs: f64,
}

/// RAII guard: installs an advisory live-LUFS sink that emits `tmp://leveling-lufs` for the
/// lifetime of a leveling run, clearing it on drop (incl. unwind). Every leveling command
/// runs serialized under the device-op lock, so only one guard is ever live at a time.
struct LiveLufsGuard;

impl LiveLufsGuard {
    fn install<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Self {
        use tauri::Emitter;
        audio::set_live_lufs_sink(Box::new(move |lufs| {
            let _ = app.emit("tmp://leveling-lufs", LiveLufsEvent { lufs });
        }));
        LiveLufsGuard
    }
}

impl Drop for LiveLufsGuard {
    fn drop(&mut self) {
        audio::clear_live_lufs_sink();
    }
}

static SCENE_LEVEL_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn cancel_scene_leveling() {
    SCENE_LEVEL_CANCEL.store(true, SeqCst);
}

/// Cooperative cancel for [`level_preset`] (base-preset leveling) — set by
/// `cancel_preset_leveling`, reset at the command's start, read via a closure passed into
/// `leveller::level_preset`/`level_preset_block`, which bail before the apply+save.
static PRESET_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn cancel_preset_leveling() {
    PRESET_LEVEL_CANCEL.store(true, SeqCst);
}

// ───────────────────────── Footswitch (engaged-state) leveling ─────────────────────────

/// One footswitch-leveling request: level switch `switch`'s engaged state by solving the
/// `(lev_group_id, lev_node_id, lev_parameter_id)` param to hit `target_lufs`.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct FootswitchLevelJob {
    switch: u32,
    lev_group_id: String,
    lev_node_id: String,
    lev_parameter_id: String,
    target_lufs: f64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FootswitchLevelProgressItem {
    switch: u32,
    status: String, // active | done | error | cancelled
    result: Option<leveller::FootswitchLevelResult>,
    message: Option<String>,
}

static FOOTSWITCH_LEVEL_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn cancel_footswitch_leveling() {
    FOOTSWITCH_LEVEL_CANCEL.store(true, SeqCst);
}

/// Read a slot's field-8 preset JSON on a fresh quiet session and return the parsed preset, the
/// scene gate (`Some(empty)` = definitely no FS scenes; truncated/unknown or non-empty →
/// conservative `true`), and the raw byte length. Shared by the footswitch leveling command +
/// probes (the connect→drain→read→parse→scene-check boilerplate).
fn read_slot_preset_parsed(slot: u32) -> Result<(serde_json::Value, bool, usize), String> {
    let mut s = Session::connect()?;
    s.drain_until_quiet(250, 20)?;
    let json = s
        .read_slot_preset_json(slot + 1)?
        .ok_or_else(|| format!("no preset data for slot {}", slot + 1))?;
    let preset = session::tolerant_parse_json(&String::from_utf8_lossy(&json))
        .ok_or_else(|| "preset JSON did not parse".to_string())?;
    let has_fs_scenes = session::scene_names_from_slot_json(&json).is_none_or(|n| !n.is_empty());
    Ok((preset, has_fs_scenes, json.len()))
}

/// A numeric `dspUnitParameter` of `node_id` (e.g. the lev param's current value = `valueB`).
fn node_param_f64(preset: &serde_json::Value, node_id: &str, param: &str) -> Option<f64> {
    let mut found = None;
    audiograph::for_each_node(preset, |obj| {
        if obj.get("nodeId").and_then(|v| v.as_str()) == Some(node_id) {
            found = obj
                .get("dspUnitParameters")
                .and_then(|p| p.get(param))
                .and_then(|v| v.as_f64());
        }
    });
    found
}

/// Resolved inputs to `leveller::level_footswitch`: the switch-OFF value (`valueB` = the
/// param's current value) and the write spec.
type FootswitchJobResolution = (f32, leveller::FootswitchWriteSpec);

/// Resolve a footswitch-leveling job against the preset: the lev param's current value
/// (`valueB`) and the write spec (edit an existing matching `param` function, else add at
/// the next free index; enforce the firmware's 5-function cap). The leveler only ever
/// creates/edits a parameter-change assignment — it does not touch on/off.
fn resolve_footswitch_job(
    ftsw: &serde_json::Value,
    preset: &serde_json::Value,
    job: &FootswitchLevelJob,
) -> Result<FootswitchJobResolution, String> {
    let switches = ftsw.as_array().ok_or("preset has no ftsw")?;
    let sw = switches
        .get(job.switch as usize)
        .and_then(|s| s.as_array())
        .ok_or_else(|| format!("footswitch {} not found", job.switch))?;

    let value_b =
        node_param_f64(preset, &job.lev_node_id, &job.lev_parameter_id).ok_or_else(|| {
            format!(
                "parameter {} not found on {}",
                job.lev_parameter_id, job.lev_node_id
            )
        })? as f32;

    // Edit an existing param fn on (lev_node, lev_param), else add (≤5 cap).
    let existing = footswitch::existing_param_fn_index(
        ftsw,
        job.switch,
        &job.lev_node_id,
        &job.lev_parameter_id,
    )
    .and_then(|i| sw.get(i as usize).map(|a| (i, a)));
    let spec = match existing {
        Some((i, a)) => leveller::FootswitchWriteSpec {
            function_index: i,
            color_a: a.get("colorA").and_then(|v| v.as_u64()).unwrap_or(3) as u32,
            color_b: a.get("colorB").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            custom_label: a
                .get("customLabel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            link_group: a.get("linkGroup").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            is_active: a.get("isActive").and_then(|v| v.as_bool()).unwrap_or(true),
        },
        None => {
            if sw.len() >= 5 {
                return Err(format!(
                    "footswitch {} is full (5 functions) — no room to add a leveling param",
                    job.switch
                ));
            }
            leveller::FootswitchWriteSpec {
                function_index: sw.len() as u32,
                color_a: 3,
                color_b: 0,
                custom_label: String::new(),
                link_group: 0,
                is_active: true,
            }
        }
    };
    Ok((value_b, spec))
}

/// Level one or more block-acting footswitches of preset `slot`, streaming a progress item
/// per switch. Each switch's engaged state is measured/solved independently against the
/// base preset; jobs run sequentially. Mirrors `level_scenes_apply_batched`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_footswitches_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    jobs: Vec<FootswitchLevelJob>,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    on_result: tauri::ipc::Channel<FootswitchLevelProgressItem>,
) -> Result<Vec<leveller::FootswitchLevelResult>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id.clone())?;
    let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
    let offset = playback_offset_for(&app, topology_id.as_deref());
    FOOTSWITCH_LEVEL_CANCEL.store(false, SeqCst);
    let app_evt = app.clone();

    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        // Read the preset once (resolve every job) + whether it has FS scenes (the bake gate).
        let (preset, has_fs_scenes, _) = read_slot_preset_parsed(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
        let ftsw = preset
            .get("ftsw")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Plan bake-vs-assign for the whole batch (pure) — block-off-in-base + sole-owner +
        // no-scenes ⇒ bake straight onto the block; otherwise the (engaged-measured) param fn.
        let keys: Vec<footswitch::FsJobKey> = jobs
            .iter()
            .map(|j| footswitch::FsJobKey {
                switch: j.switch,
                lev_node: &j.lev_node_id,
                lev_param: &j.lev_parameter_id,
                target_bits: j.target_lufs.to_bits(),
            })
            .collect();
        let plans = footswitch::plan_footswitch_jobs(&ftsw, &preset, &keys, has_fs_scenes);

        let mut results: Vec<Option<leveller::FootswitchLevelResult>> = vec![None; jobs.len()];
        for (idx, job) in jobs.iter().enumerate() {
            if FOOTSWITCH_LEVEL_CANCEL.load(SeqCst) {
                let _ = on_result.send(FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "cancelled".into(),
                    result: None,
                    message: None,
                });
                break;
            }
            let _ = on_result.send(FootswitchLevelProgressItem {
                switch: job.switch,
                status: "active".into(),
                result: None,
                message: None,
            });
            let lev = (
                job.lev_group_id.as_str(),
                job.lev_node_id.as_str(),
                job.lev_parameter_id.as_str(),
            );
            let outcome: Result<leveller::FootswitchLevelResult, String> = match &plans[idx] {
                footswitch::FsLevelPlan::Clamp(msg) => Err(msg.clone()),
                // A sibling switch already baked this (node, param, target) — reuse its result.
                footswitch::FsLevelPlan::BakeShared { rep } => results[*rep]
                    .clone()
                    .map(|mut r| {
                        r.switch = job.switch;
                        r
                    })
                    .ok_or_else(|| "shared bake produced no result".to_string()),
                footswitch::FsLevelPlan::Bake {
                    engaged,
                    clear_stale,
                } => leveller::level_footswitch(
                    slot,
                    job.switch,
                    lev,
                    engaged,
                    &leveller::FsWrite::Bake {
                        clear_stale: *clear_stale,
                    },
                    &stim,
                    job.target_lufs + offset,
                    save,
                    true,
                ),
                footswitch::FsLevelPlan::Assign { engaged } => {
                    match resolve_footswitch_job(&ftsw, &preset, job) {
                        Err(e) => Err(e),
                        Ok((value_b, spec)) => leveller::level_footswitch(
                            slot,
                            job.switch,
                            lev,
                            engaged,
                            &leveller::FsWrite::Assign { value_b, spec },
                            &stim,
                            job.target_lufs + offset,
                            save,
                            true,
                        ),
                    }
                }
            };
            let item = match outcome {
                Ok(r) => {
                    results[idx] = Some(r.clone());
                    FootswitchLevelProgressItem {
                        switch: job.switch,
                        status: "done".into(),
                        result: Some(r),
                        message: None,
                    }
                }
                Err(e) => FootswitchLevelProgressItem {
                    switch: job.switch,
                    status: "error".into(),
                    result: None,
                    message: Some(e),
                },
            };
            let _ = on_result.send(item);
        }
        // Guarantee re-amp OFF on a fresh connection.
        if let Ok(mut s) = Session::connect() {
            let _ = s.set_reamp_mode(false);
        }
        Ok(results.into_iter().flatten().collect())
    })
    .await
}

/// Probe entry: isolate the in-process CoreAudio → chunked-HID failure. Sends a chunked
/// `set_footswitch_assignment` (1) BEFORE any audio, (2) after ONE re-amp CoreAudio capture,
/// (3) after a SECOND capture — reporting the device's reply fields each time ([54] = landed,
/// [] = dropped). Tells us whether one capture is enough to break chunked sends, or if it
/// accumulates. Targets slot 23 / FS6 (the BD2 preset); restores after each set.
pub fn probe_repro_chunked() -> Result<String, String> {
    let slot = 23u32;
    let switch = 6u32;
    let json = r#"{"func":"param","groupId":"G1","nodeId":"ACD_BluesDriver","parameterId":"gain","valueA":0.5,"valueB":0.35,"valueType":2,"colorA":3,"colorB":0,"customLabel":"REPRO","switchType":0,"isActive":true,"linkGroup":0}"#;
    let mut out = String::from("[probe --repro-chunked]\n");

    let try_set = |label: &str, out: &mut String| {
        let r = (|| -> Result<Vec<u32>, String> {
            let mut s = Session::connect()?;
            s.begin_live_edit()?;
            s.load_preset(slot)?;
            // Pump heartbeats (NOT a passive sleep) to keep the session live up to the set.
            for _ in 0..8 {
                let _ = s.heartbeat();
                let _ = s.pump_collect(150);
            }
            s.set_footswitch_assignment(switch, 1, json, false, None)?;
            let seen = s.seen_preset_fields();
            let _ = s.clear_footswitch_assignment(switch, 1);
            let _ = s.save_current_preset(slot);
            Ok(seen)
        })();
        match r {
            Ok(seen) => out.push_str(&format!(
                "  [{label}] chunked set → device fields {seen:?}  ({})\n",
                if seen.contains(&54) {
                    "LANDED"
                } else {
                    "DROPPED"
                }
            )),
            Err(e) => out.push_str(&format!("  [{label}] error: {e}\n")),
        }
    };

    let capture_once = || -> Result<(), String> {
        let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
            .map_err(|_| "set TMP_LEVELLER_STIMULUS".to_string())?;
        let stim = read_stimulus_48k(&stim_path)?;
        {
            let mut s = Session::connect()?;
            s.load_preset(slot)?;
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        let mut s = Session::connect()?;
        s.set_reamp_mode(true)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = audio::reamp_capture(&stim, 48_000, 800);
        let _ = s.set_reamp_mode(false);
        Ok(())
    };

    try_set("A: before any audio", &mut out);
    out.push_str("  … one re-amp CoreAudio capture …\n");
    capture_once()?;
    try_set("B: after 1 capture", &mut out);
    out.push_str("  … second re-amp CoreAudio capture …\n");
    capture_once()?;
    try_set("C: after 2 captures", &mut out);
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));
    Ok(out)
}

/// Probe entry: clear one footswitch function (restore/cleanup after a `--level-footswitch
/// --commit`). Loads `slot`, clears `(switch, index)`, saves, and field-8 verifies.
pub fn probe_clear_footswitch(slot: u32, switch: u32, index: u32) -> Result<String, String> {
    let count_at = |f: &Option<serde_json::Value>| -> usize {
        f.as_ref()
            .and_then(|f| f.as_array())
            .and_then(|a| a.get(switch as usize))
            .and_then(|sw| sw.as_array())
            .map(|fns| fns.len())
            .unwrap_or(usize::MAX)
    };
    let before = count_at(&read_slot_ftsw(slot + 1));
    let mut s = Session::connect()?;
    s.begin_live_edit()?;
    s.load_preset(slot)?;
    let name = s.active_preset_name().unwrap_or_default();
    if !name.is_empty() && !s.await_active_preset(&name, 20) {
        return Err("after load, active preset changed — aborting".into());
    }
    // Keep the session live with a heartbeat burst right up to the edit (a passive sleep
    // lets the live-controller status lapse and the device silently drops the edit).
    for _ in 0..8 {
        let _ = s.heartbeat();
        let _ = s.pump_collect(150);
    }
    s.clear_footswitch_assignment(switch, index)?;
    if s.saw_preset_error() {
        return Err("device rejected clear (presetError)".into());
    }
    s.save_current_preset(slot)?;
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(600));
    let count = read_slot_ftsw(slot + 1)
        .and_then(|f| {
            f.as_array()
                .and_then(|a| a.get(switch as usize))
                .and_then(|sw| sw.as_array())
                .map(|fns| fns.len())
        })
        .unwrap_or(usize::MAX);
    Ok(format!(
        "[probe --clear-ftsw] slot {} FS{switch} index {index}: before clear {before} function(s) → cleared + saved → now {count} function(s)\n",
        slot + 1
    ))
}

/// Probe (self-restoring): commit a BAKE on `(switch, group, node, param)`, verify the value
/// landed on the block (bypass unchanged, no param fn added), then RESTORE the original value.
/// Mirrors `--ftsw-validate --commit`'s commit-then-restore. Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_bake_validate(
    slot: u32,
    switch: u32,
    group: &str,
    node: &str,
    param: &str,
    target_lufs: f64,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    // One read → the node's value, base bypass, switch fn count, and engaged force-list.
    type Snap = (f64, bool, usize, Vec<(String, String, bool)>);
    let snapshot = || -> Result<Snap, String> {
        let (p, _, _) = read_slot_preset_parsed(slot)?;
        let ftsw = p.get("ftsw").cloned().unwrap_or(serde_json::Value::Null);
        let v = node_param_f64(&p, node, param).ok_or("param not found after read")?;
        let fns = ftsw
            .as_array()
            .and_then(|a| a.get(switch as usize)?.as_array().map(Vec::len))
            .unwrap_or(usize::MAX);
        let engaged = footswitch::engaged_bypass_for_switch(&ftsw, &p, switch);
        Ok((
            v,
            footswitch::block_bypassed_in_base(&p, node),
            fns,
            engaged,
        ))
    };

    let (orig, byp0, fns0, engaged) = snapshot()?;
    let mut out = format!(
        "[probe --bake-validate] slot {} · FS{switch} · {group}/{node}.{param}\n  before: value={orig:.4} bypass={byp0} switch_fns={fns0}\n",
        slot + 1
    );
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    // Commit the bake (engaged-measured, value written onto the block).
    let r = leveller::level_footswitch(
        slot,
        switch,
        (group, node, param),
        &engaged,
        &leveller::FsWrite::Bake { clear_stale: None },
        &stim,
        target_lufs,
        true,
        false,
    )?;
    out += &format!(
        "  baked: method={} value={:.4}{}\n",
        r.method,
        r.final_value,
        if r.clamped { " [clamped]" } else { "" }
    );

    // Verify field-8: the value landed, bypass unchanged, NO param fn added.
    let (after, byp1, fns1, _) = snapshot()?;
    let landed = (after - r.final_value as f64).abs() < 1e-3;
    out += &format!(
        "  after : value={after:.4} bypass={byp1} switch_fns={fns1}  ⇒  {}\n",
        if landed && byp1 == byp0 && fns1 == fns0 {
            "PASS (value baked, bypass intact, no fn added)"
        } else {
            "FAIL"
        }
    );

    // Restore the original value (change_parameter + save on a heartbeat-live session).
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    {
        let mut s = Session::connect()?;
        s.begin_live_edit()?;
        s.load_preset(slot)?;
        for _ in 0..8 {
            let _ = s.heartbeat();
            let _ = s.pump_collect(150);
        }
        s.change_parameter(group, node, param, orig as f32)?;
        s.save_current_preset(slot)?;
    }
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));
    let (restored, _, _, _) = snapshot()?;
    out += &format!(
        "  restore: value={restored:.4}  ⇒  {}\n",
        if (restored - orig).abs() < 1e-3 {
            "RESTORED"
        } else {
            "RESTORE MISMATCH (recover from unit backup)"
        }
    );
    Ok(out)
}

/// Probe (read-only): list a slot's block-acting footswitches with each acted-on block's base
/// bypass + the bake/assign classification for its first level param — to find bake-eligible
/// presets (an active on-off enabling an OFF-in-base block).
pub fn probe_fs_list(slot: u32) -> Result<String, String> {
    let (preset, has_fs_scenes, json_len) = read_slot_preset_parsed(slot)?;
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let infos = footswitch::enumerate_block_footswitches(&ftsw, &preset);
    let mut out = format!(
        "[probe --fs-list] slot {} · {} block-footswitch(es) · has_fs_scenes={has_fs_scenes} ({json_len}B)\n",
        slot + 1,
        infos.len()
    );
    for fi in &infos {
        for f in &fi.functions {
            let byp = footswitch::block_bypassed_in_base(&preset, &f.node_id);
            out += &format!(
                "  FS{} {:7} {}/{}  base_bypass={byp}\n",
                fi.switch, f.func, f.group_id, f.node_id
            );
        }
        if let Some(lp) = fi.level_params.first() {
            let plan = footswitch::plan_footswitch_jobs(
                &ftsw,
                &preset,
                &[footswitch::FsJobKey {
                    switch: fi.switch,
                    lev_node: &lp.node_id,
                    lev_param: &lp.parameter_id,
                    target_bits: (-23.0f64).to_bits(),
                }],
                has_fs_scenes,
            );
            out += &format!(
                "      → level {}.{}  ⇒  {:?}\n",
                lp.node_id, lp.parameter_id, plan[0]
            );
        }
    }
    Ok(out)
}

/// Probe GO/NO-GO spike: prove the device honors a LIVE `change_parameter_bool(bypass=false)`.
/// Measures `(group,node)`'s contribution with the block left as-is vs forced active. If the
/// block is OFF in base, the base capture is the preset WITHOUT it and the forced capture is
/// WITH it, so a meaningful loudness delta proves the live bypass write took effect (the bake
/// path depends on this). Stimulus via `TMP_LEVELLER_STIMULUS`.
pub fn probe_measure_forced(slot: u32, group: &str, node: &str) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let stim = read_stimulus_calibrated(&stim_path, None)?;
    {
        let mut s = Session::connect()?;
        s.load_preset(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_LOAD_MS,
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));

    let measure = |force: Option<bool>| -> Result<f64, String> {
        let mut s = Session::connect()?;
        if let Some(byp) = force {
            s.change_parameter_bool(group, node, "bypass", byp)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(
            leveller::SETTLE_AFTER_SET_MS,
        ));
        Ok(leveller::engage_measure_disengage(&mut s, &stim)?.integrated_lufs)
    };

    let base = measure(None);
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let off = measure(Some(true)); // force bypassed
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let on = measure(Some(false)); // force active
    let _ = Session::connect().map(|mut s| s.set_reamp_mode(false));

    let row = |label: &str, v: &Result<f64, String>| match v {
        Ok(l) => format!("  {label}: {l:.2} LUFS\n"),
        Err(e) => format!("  {label}: ERROR {e}\n"),
    };
    let mut out = format!(
        "[probe --measure-forced] slot {} · {group}/{node}\n",
        slot + 1
    );
    out += &row("base (as-is)       ", &base);
    out += &row("forced bypass=true ", &off);
    out += &row("forced bypass=false", &on);
    if let (Ok(off), Ok(on)) = (&off, &on) {
        // The two forced states differ by the block's whole contribution → the live bypass write
        // is honored. Whichever matches base reveals the base state.
        out += &format!(
            "  on−off = {:+.2} LU  ⇒  live bypass write {}\n",
            on - off,
            if (on - off).abs() > 0.5 {
                "HONORED (go)"
            } else {
                "NO EFFECT (no-go)"
            }
        );
        if let Ok(b) = &base {
            let base_state = if (b - on).abs() < (b - off).abs() {
                "ON in base"
            } else {
                "OFF in base"
            };
            out += &format!("  base matches forced-{base_state}\n");
        }
    }
    Ok(out)
}

/// Probe entry: level one footswitch on the active/`slot` preset for HW re-validation.
/// DRY by default (measure + solve, no write); `commit` writes `valueA` + saves.
/// Stimulus via `TMP_LEVELLER_STIMULUS` (+ optional `TMP_LEVELLER_CAL_LUFS`).
pub fn probe_level_footswitch(
    slot: u32,
    switch: u32,
    lev_group: &str,
    lev_node: &str,
    lev_param: &str,
    target_lufs: f64,
    commit: bool,
) -> Result<String, String> {
    let stim_path = std::env::var("TMP_LEVELLER_STIMULUS")
        .map_err(|_| "set TMP_LEVELLER_STIMULUS to the stimulus WAV".to_string())?;
    let cal = std::env::var("TMP_LEVELLER_CAL_LUFS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    let stim = read_stimulus_calibrated(&stim_path, cal)?;

    let (preset, has_fs_scenes, _) = read_slot_preset_parsed(slot)?;
    std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
    let ftsw = preset
        .get("ftsw")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let job = FootswitchLevelJob {
        switch,
        lev_group_id: lev_group.to_string(),
        lev_node_id: lev_node.to_string(),
        lev_parameter_id: lev_param.to_string(),
        target_lufs,
    };
    let plan = footswitch::plan_footswitch_jobs(
        &ftsw,
        &preset,
        &[footswitch::FsJobKey {
            switch,
            lev_node,
            lev_param,
            target_bits: target_lufs.to_bits(),
        }],
        has_fs_scenes,
    )
    .into_iter()
    .next()
    .ok_or("planner returned no plan")?;
    let lev = (lev_group, lev_node, lev_param);
    let (write, plan_label) = match &plan {
        footswitch::FsLevelPlan::Clamp(msg) => return Err(msg.clone()),
        footswitch::FsLevelPlan::BakeShared { .. } => {
            return Err("single-job probe cannot be a shared bake".into())
        }
        footswitch::FsLevelPlan::Bake { clear_stale, .. } => (
            leveller::FsWrite::Bake {
                clear_stale: *clear_stale,
            },
            "BAKE → value written onto the block".to_string(),
        ),
        footswitch::FsLevelPlan::Assign { .. } => {
            let (value_b, spec) = resolve_footswitch_job(&ftsw, &preset, &job)?;
            let label = format!(
                "ASSIGN → param fn @ index {} (valueB={value_b:.4})",
                spec.function_index
            );
            (leveller::FsWrite::Assign { value_b, spec }, label)
        }
    };
    let engaged = match &plan {
        footswitch::FsLevelPlan::Bake { engaged, .. }
        | footswitch::FsLevelPlan::Assign { engaged } => engaged.clone(),
        _ => Vec::new(),
    };
    let r = leveller::level_footswitch(
        slot,
        switch,
        lev,
        &engaged,
        &write,
        &stim,
        target_lufs,
        commit,
        true,
    )?;
    let mut out = format!(
        "[probe --level-footswitch] preset slot {} · FS{switch} · {lev_group}/{lev_node}.{lev_param}  ({})\n",
        slot + 1,
        if commit { "COMMIT — wrote + saved" } else { "DRY — not written" }
    );
    out += &format!("  plan: {plan_label}  ·  method={}\n", r.method);
    out += &format!(
        "  measured(seed) {:.2} LUFS → target {:.1}  ⇒  valueA={:.4}{}  (engaged {:.2} LUFS, {} iters, spread {:.1} LU)\n",
        r.measured_lufs,
        r.target_lufs,
        r.final_value,
        if r.clamped {
            match &r.clamp_reason {
                Some(reason) => format!("  [CLAMPED — {reason}]"),
                None => "  [CLAMPED]".to_string(),
            }
        } else {
            String::new()
        },
        r.predicted_lufs,
        r.iterations,
        r.dynamic_spread_lu.unwrap_or(0.0),
    );
    if let Some(v) = r.verify_lufs {
        out += &format!(
            "  verify (fresh engaged capture @ valueA): {v:.2} LUFS  (err {:+.2} LU)\n",
            v - r.target_lufs
        );
    }
    if r.saved {
        out += "  [SAVED to preset]\n";
    }
    Ok(out)
}

fn pick_scene_level_knob(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
) -> Result<(leveller::LevelKnob, f32, f32, f32), String> {
    let scene_slot = if scene >= session::BASE_SCENE_SLOT {
        None
    } else {
        Some(scene)
    };
    // ONE rich session (HW-rearchitected): heartbeat warmup → loads
    // via send_and_collect → live doc from the accumulated field-3 pushes. The
    // old connect → load → drop → connect_for_discovery chain is broken on fw
    // 1.8.45 twice over: a close chased by a re-open wedges the device's next
    // exclusive open (0xe00002c5 lockout), and field-78 kills field-3 delivery
    // for its whole session anyway. After each load the raw accumulator is
    // cleared so the doc reflects the POST-scene live state (the pick must read
    // the sounding graph, never stale pre-scene pushes).
    let live_doc = {
        let mut s = Session::connect()?;
        for _ in 0..16 {
            s.heartbeat()?;
            s.pump_collect(120)?;
        }
        s.raw.clear();
        s.send_and_collect(&proto::load_preset((slot + 1) as u64, 1), 300)?;
        for _ in 0..8 {
            s.heartbeat()?;
            s.pump_collect(200)?;
        }
        if let Some(sl) = scene_slot {
            s.raw.clear();
            s.send_and_collect(&proto::load_scene(sl as u64), 300)?;
            for _ in 0..8 {
                s.heartbeat()?;
                s.pump_collect(200)?;
            }
        }
        s.current_preset_value()?
    };
    for c in candidates {
        log::info!(
            "pick_scene_level_knob scene={scene} candidate {}/{}/{} live_bypass={:?}",
            c.group_id,
            c.node_id,
            c.parameter_id,
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id),
        );
    }
    let picked = candidates
        .iter()
        .filter(|c| is_amp_output_level_param(&c.parameter_id))
        .find(|c| {
            scenes::block_bypass_in_live_graph(&live_doc, &c.group_id, &c.node_id) == Some(false)
        })
        .ok_or_else(|| format!("no active amp outputLevel control found for scene slot {scene}"))?;
    let (lo, hi) = knob_bounds(picked.value);
    Ok((
        leveller::LevelKnob::Block {
            group_id: picked.group_id.clone(),
            node_id: picked.node_id.clone(),
            parameter_id: picked.parameter_id.clone(),
            scene_slot,
        },
        lo,
        hi,
        picked.value,
    ))
}

/// Level ONE scene the capture-per-connection way (`level_preset_block`): pick
/// the scene's knob from its live graph, then closed-loop with fresh re-amp
/// captures. The legacy `level_scenes_apply` path; the shipped batched flow is
/// `level_scenes_apply_batched` → `leveller::level_scenes_live_batched`.
fn level_one_scene_legacy(
    slot: u32,
    scene: u32,
    candidates: &[LevelBlockArg],
    stimulus: &[f32],
    target_lufs: f64,
    save: bool,
) -> Result<leveller::LevelResult, String> {
    let (knob, lo, hi, _current) = pick_scene_level_knob(slot, scene, candidates)?;
    // 800 ms before the leveller's first fresh connect — the empirical safe gap
    // after a rich-session close (shorter chases trip the device's open lockout).
    std::thread::sleep(std::time::Duration::from_millis(800));
    let opts = leveller::LevelOptions {
        save,
        verify: true,
        ..Default::default()
    };
    leveller::level_preset_block(slot, stimulus, &knob, lo, hi, target_lufs, opts, || false)
}

/// Per-scene leveling APPLY (chosen mechanism: enable scene mode on the amp
/// block, level only the amp `outputLevel` control). For each selected scene, drive
/// the scene's ACTIVE amp's `outputLevel` knob closed-loop to `target_lufs` with
/// per-block Scene Edit enabled —
/// so the level lands on that scene's overlay, not the base. The knob is resolved
/// PER SCENE from `candidates` by the scene overlay's `bypass` (HW-found:
/// a preset can carry several amps with scenes swapping which is live — leveling a
/// bypassed amp's knob measures flat and clamps).
/// `scene_slots` are the WIRE slots: 0-based `scenes[]` indices for FS scenes;
/// `session::BASE_SCENE_SLOT` (8) = the base/preset value (levelled WITHOUT scene-edit
/// — a preset load activates base, so no scene recall is needed).
/// DEVICE WRITE when `save` — opt-in, gated by the read-only HW policy + the leveling
/// overlay confirm. Reuses `level_preset_block` (the scene context rides the knob and
/// is re-asserted on every connection). Each scene is a self-contained leveling pass.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_scenes_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run = || -> Result<Vec<leveller::LevelResult>, String> {
            let mut results = Vec::with_capacity(scene_slots.len());
            for scene in &scene_slots {
                let r = level_one_scene_legacy(
                    slot,
                    *scene,
                    &candidates,
                    &stim,
                    target_lufs,
                    save,
                )?;
                log::info!(
                    "level_scenes_apply slot={slot} scene={scene} save={save} final_level={:.4} measured={:.2} clamped={}",
                    r.final_level, r.measured_lufs, r.clamped,
                );
                results.push(r);
            }
            Ok(results)
        };
        let result = run();
        // GUARANTEED re-amp OFF on a fresh connection, success or failure. The
        // leveller's in-connection `set_reamp_mode(false)` is fire-and-forget and
        // demonstrably gets dropped under the run's connection churn — HW-observed (TWICE): the unit came out of a scene-leveling run stuck in
        // re-amp (guitar input muted, "no sound") until a power-cycle.
        match Session::connect().and_then(|mut s| s.set_reamp_mode(false)) {
            Ok(_) => log::info!("level_scenes_apply: final re-amp OFF sent"),
            Err(e) => log::warn!("level_scenes_apply: final re-amp OFF failed ({e})"),
        }
        result
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn level_scenes_apply_batched(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u32,
    scene_slots: Vec<u32>,
    candidates: Vec<LevelBlockArg>,
    target_lufs: f64,
    save: bool,
    rebalance: bool,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
    on_result: tauri::ipc::Channel<SceneLevelProgressItem>,
) -> Result<Vec<leveller::LevelResult>, String> {
    if !candidates
        .iter()
        .any(|c| is_amp_output_level_param(&c.parameter_id))
    {
        return Err("per-scene leveling needs at least one amp outputLevel candidate".to_string());
    }
    if scene_slots.is_empty() {
        return Err("no scenes selected".to_string());
    }
    SCENE_LEVEL_CANCEL.store(false, SeqCst);
    let target_lufs = target_lufs + playback_offset_for(&app, topology_id.as_deref());
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    let app_evt = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Stream advisory live LUFS while each capture runs (dropped at closure end).
        let _lufs = LiveLufsGuard::install(app_evt);
        let stim = read_stimulus_calibrated(&stim_path, calibration_lufs)?;
        let run_batched = |save_run: bool| -> Result<Vec<leveller::BatchedSceneOutcome>, String> {
            // Un-engaged pre-pass (scene docs → jobs), then the ONE-SHOT runner:
            // amp `outputLevel` is linear in dB, so each scene is measured once at a
            // reference level (ISOLATED fresh re-amp capture) and solved exactly — the
            // BatchedLive shared-stream loop mis-measured scenes (HW).
            let docs = prepass_scene_docs(slot, &scene_slots)?;
            // Inter-session HID gap: the prepass session has just closed; the one-shot
            // runner opens a fresh one. Reuse the leveller's HW-proven open-after-close
            // gap (was a hard-coded 800, copied from the bench). build_scene_jobs below
            // is pure CPU, so this is the only wait here.
            std::thread::sleep(std::time::Duration::from_millis(leveller::RECONNECT_GAP_MS));
            let jobs = build_scene_jobs(&scene_slots, &candidates, &docs)?;
            let on_scene = |scene, done: Option<&leveller::BatchedSceneOutcome>| match done {
                None => {
                    let _ = on_result.send(SceneLevelProgressItem {
                        scene_slot: scene,
                        status: "active".to_string(),
                        result: None,
                        message: None,
                    });
                }
                Some(o) => {
                    let item = match &o.failure {
                        None => SceneLevelProgressItem {
                            scene_slot: scene,
                            status: "done".to_string(),
                            result: Some(outcome_to_level_result(slot, target_lufs, save_run, o)),
                            message: None,
                        },
                        Some(e) => SceneLevelProgressItem {
                            scene_slot: scene,
                            status: "error".to_string(),
                            result: None,
                            message: Some(e.clone()),
                        },
                    };
                    let _ = on_result.send(item);
                }
            };
            let cancelled = || SCENE_LEVEL_CANCEL.load(SeqCst);
            // `rebalance` (opt-in) equalizes a path-MERGE scene's two lanes before joint-k;
            // non-mergeable scenes fall through to the same joint-k either way.
            if rebalance {
                leveller::level_scenes_rebalance(
                    slot,
                    &jobs,
                    &stim,
                    target_lufs,
                    save_run,
                    on_scene,
                    cancelled,
                )
            } else {
                leveller::level_scenes_oneshot(
                    slot,
                    &jobs,
                    &stim,
                    target_lufs,
                    save_run,
                    on_scene,
                    cancelled,
                )
            }
        };
        // Per-scene leveling drives ONLY the active amp's `outputLevel`. When a scene
        // can't reach target even at the knob's limit it CLAMPS and reports the achieved
        // loudness — we do NOT raise the global `presetLevel` to compensate. Raising it
        // lifts EVERY other scene off-target (presetLevel is the Base's job, settled once
        // before the scene pass), and HW the old boost-and-rerun drove
        // presetLevel to 1.0 and blew preset 001's loud scenes 5–7 LU over target.
        let outcome = run_batched(save);
        let result = match outcome {
            Ok(outcomes) => Ok(outcomes
                .iter()
                .filter(|o| o.failure.is_none())
                .map(|o| outcome_to_level_result(slot, target_lufs, save, o))
                .collect()),
            Err(e) if e == leveller::CANCELLED => {
                let _ = on_result.send(SceneLevelProgressItem {
                    scene_slot: session::BASE_SCENE_SLOT,
                    status: "cancelled".to_string(),
                    result: None,
                    message: Some(e),
                });
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        };
        match Session::connect().and_then(|mut s| s.set_reamp_mode(false)) {
            Ok(_) => log::info!("level_scenes_apply_batched: final re-amp OFF sent"),
            Err(e) => log::warn!("level_scenes_apply_batched: final re-amp OFF failed ({e})"),
        }
        result
    })
    .await
}

/// Map a [`leveller::BatchedSceneOutcome`] onto the frontend's `LevelResult`
/// contract (the batched runner's outcome is per-scene; `verify_lufs` carries
/// the final measured window).
fn outcome_to_level_result(
    slot: u32,
    target_lufs: f64,
    save: bool,
    o: &leveller::BatchedSceneOutcome,
) -> leveller::LevelResult {
    let lufs = o.final_lufs.unwrap_or(f64::NAN);
    leveller::LevelResult {
        slot,
        ref_level: o.final_level.unwrap_or(0.0),
        measured_lufs: lufs,
        constant_c: f64::NAN,
        final_level: o.final_level.unwrap_or(0.0),
        target_lufs,
        predicted_lufs: lufs,
        clamped: o.clamped,
        saved: save,
        verify_lufs: o.final_lufs,
        iterations: o.windows.max(o.writes),
        dynamic_spread_lu: o.dynamic_spread_lu,
        clamp_reason: o.clamp_reason.clone(),
        verify_by_ear: o.verify_by_ear,
    }
}

/// Headroom (LU) below the quietest-capable preset's ceiling when auto-picking
/// the setlist common target. Small margin so the floor preset isn't clamped.
const SETLIST_HEADROOM_LU: f64 = 1.0;

/// One preset in a setlist leveling job: its slot + the instrument profile's
/// topology (resolved to that instrument's stimulus).
#[derive(serde::Deserialize)]
struct SetlistJobEntry {
    slot: u32,
    topology_id: Option<String>,
    calibration_lufs: Option<f32>,
}

/// Level a whole setlist to one common loudness target so switching presets (and
/// instruments) on stage causes no jump. Measures every preset's ceiling, picks a
/// target just below the quietest, and applies it to all. Like `level_preset`, it
/// releases the app's seize, runs, then re-establishes the UI session.
#[tauri::command]
async fn level_setlist(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entries: Vec<SetlistJobEntry>,
    save: bool,
) -> Result<leveller::SetlistResult, String> {
    if entries.is_empty() {
        return Err("no presets selected to level".to_string());
    }
    // Resolve each entry's stimulus path + playback compensation on the UI
    // thread (needs AppHandle; the store is read ONCE for the whole setlist).
    // The common target stays one loudness; a bass entry's offset rides its own
    // effective target inside the leveller.
    let playback = profiles::load(&app)
        .map(|s| s.playback_level)
        .unwrap_or_default();
    let resolved: Vec<(u32, String, Option<f32>, f64)> = entries
        .into_iter()
        .map(|e| {
            let offset_lu = profiles::playback_offset_lu(
                playback,
                stimulus_instrument(e.topology_id.as_deref()),
            );
            resolve_stimulus(&app, None, e.topology_id)
                .map(|p| (e.slot, p, e.calibration_lufs, offset_lu))
        })
        .collect::<Result<_, _>>()?;
    with_released_seize(state.session.clone(), move || {
        // Own each stimulus (calibrated if the profile has a real-output level),
        // then borrow into entries for the leveller.
        let stims: Vec<(u32, Vec<f32>, f64)> = resolved
            .into_iter()
            .map(|(slot, path, cal, off)| {
                read_stimulus_calibrated(&path, cal).map(|s| (slot, s, off))
            })
            .collect::<Result<_, _>>()?;
        let lvl_entries: Vec<leveller::SetlistEntry> = stims
            .iter()
            .map(|(slot, s, off)| leveller::SetlistEntry {
                slot: *slot,
                stimulus: s,
                offset_lu: *off,
            })
            .collect();
        leveller::level_setlist(&lvl_entries, SETLIST_HEADROOM_LU, 0.5, save)
    })
    .await
}

/// What one Tier-2 calibration measured, plus its two quality caveats.
/// Mirrored in `src/lib/types.ts` (`CalibrateResult`).
#[derive(Debug, Clone, Copy, Serialize)]
struct CalibrateResult {
    /// Measured K-weighted loudness of the dry capture (stored on the profile).
    lufs: f32,
    /// The dry tap (USB-Out 3, no limiter) hit 0 dBFS — the measurement is biased
    /// LOW (clipped transients flatten the brightness K-weighting credits).
    clipped: bool,
    /// The topology stimulus cannot be scaled up to `lufs` without clipping (the
    /// 0.99 peak cap in `read_stimulus_calibrated_with_shortfall`): leveling will
    /// drive the amp this many LU softer than the real instrument. `None` = reachable.
    stimulus_shortfall_lu: Option<f32>,
}

/// Tier-2 calibration: capture the dry instrument (USB-Out 3) for `secs` while
/// the user plays their real guitar, measure its K-weighted loudness (LUFS), store
/// it on the profile's `calibration_lufs`, and return the measured value plus the
/// clip/stimulus-ceiling caveats. The device must be in normal mode with the
/// guitar in the front INSTRUMENT input.
#[tauri::command]
async fn calibrate_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    secs: f32,
) -> Result<CalibrateResult, String> {
    let app2 = app.clone();
    with_released_seize(state.session.clone(), move || {
        // Force normal mode so the dry instrument flows on USB-Out 3.
        if let Ok(mut s) = Session::connect() {
            let _ = s.set_reamp_mode(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let cap = audio::capture_input(secs.clamp(2.0, 30.0), 48_000)?;

        let peak = cap.channel_peak(audio::DRY_INSTRUMENT_IN_CH);
        if peak < 1e-4 {
            return Err("no instrument signal captured — play continuously during \
                        calibration (guitar in the front INSTRUMENT input, volume up)"
                .to_string());
        }
        // K-weighted loudness (perceptual), not flat RMS — see read_stimulus_calibrated.
        let lufs =
            lufs::measure_mono(&cap.channel(audio::DRY_INSTRUMENT_IN_CH), 48_000)?.integrated_lufs;
        if !lufs.is_finite() {
            return Err("captured signal too quiet to measure — play louder/longer".to_string());
        }
        let lufs = lufs as f32;

        let mut store = profiles::load(&app2)?;
        let p = store
            .profiles
            .iter_mut()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("unknown profile '{profile_id}'"))?;
        p.calibration_lufs = Some(lufs);
        let topology_id = p.topology_id.clone();
        profiles::save(&app2, &store)?;

        // Best-effort caveats (calibration is already persisted; a WAV-resolution
        // failure must not fail the command).
        let stimulus_shortfall_lu = resolve_stimulus(&app2, None, Some(topology_id))
            .and_then(|path| read_stimulus_calibrated_with_shortfall(&path, Some(lufs)))
            .map(|(_, shortfall)| shortfall)
            .unwrap_or(None);
        Ok(CalibrateResult {
            lufs,
            clipped: peak >= 0.99,
            stimulus_shortfall_lu,
        })
    })
    .await
}

// ─── OFFLINE library + bulk-run commands ─────────────────────────────────────────

/// Block-category map for facet indexing. TODO: derive amp/cab categories
/// from the firmware `product_profile.json`; until then facets index blocks/IRs/SICs
/// /name/level/template (everything except the amp/cab classification).
fn library_categories() -> search::CategoryMap {
    search::CategoryMap::new()
}

/// Zero-padded epoch-millis id for a bulk run (sortable, collision-free per ms).
fn now_stamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("run-{ms:020}")
}

/// Build `PresetTarget`s for the selected list indices from the imported library.
/// Errors if any index is not a matched, writable record (the destructive-op guard:
/// unmatched/ambiguous records have `list_index = None` and never reach here).
fn targets_from_library(
    lib: &library::Library,
    selection: &[u32],
) -> Result<Vec<bulkrun::PresetTarget>, String> {
    selection
        .iter()
        .map(|&idx| {
            lib.records
                .iter()
                .find(|r| r.list_index == Some(idx))
                .ok_or_else(|| {
                    format!("list index {idx} is not a matched, writable library record")
                })
                .and_then(|r| r.to_target())
        })
        .collect()
}

/// Construct the right `PresetIo` for a run's path (built fresh per run; both are
/// stateless and open their own device connections).
fn io_for_path(path: bulk_cmd::IoPath) -> Box<dyn bulkrun::PresetIo> {
    match path {
        bulk_cmd::IoPath::Live => Box::new(preset_io::LiveIo),
        bulk_cmd::IoPath::Offline => Box::new(preset_io::OfflineIo),
    }
}

/// Import a folder of Pro-Control-exported `.preset` files as the canonical library
/// and reconcile it against the live device slot list. Returns the reconcile report
/// (matched / unmatched / ambiguous) for the user to confirm before any write.
#[tauri::command]
async fn import_library(
    folder: String,
    state: State<'_, AppState>,
) -> Result<library::ReconcileReport, String> {
    let folder_path = std::path::PathBuf::from(&folder);
    let (mut records, errors) =
        library::load_library_from_dir(&folder_path, &library_categories())?;
    if !errors.is_empty() {
        log::warn!(
            "[import_library] {} file(s) skipped: {errors:?}",
            errors.len()
        );
    }
    let device_list: Vec<PresetEntry> = with_released_seize(state.session.clone(), || {
        Session::connect()?.list_my_presets()
    })
    .await
    .unwrap_or_default();
    let report = library::reconcile_with_device(&mut records, &device_list);
    *lock_ok(&state.library) = Some(library::Library {
        folder: folder_path,
        records,
    });
    Ok(report)
}

/// The imported library's records (decoded JSON omitted from the wire — it's large
/// and the UI doesn't need it; bulk ops resolve it backend-side).
#[tauri::command]
fn library_records(state: State<'_, AppState>) -> Result<Vec<library::LibraryRecord>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard
        .as_ref()
        .ok_or("no library imported — import a .preset folder first")?;
    Ok(lib.records.clone())
}

/// Filter args mirroring `search::Filter` (which has no serde derive).
#[derive(serde::Deserialize, Default)]
struct FilterArgs {
    name_substr: Option<String>,
    amp: Option<String>,
    block: Option<String>,
    ir: Option<String>,
    sic: Option<String>,
    level_lt: Option<f64>,
    level_gt: Option<f64>,
}

/// List indices of library records matching `filter` (selection feeder).
/// Only **writable** (matched) records are returned — unmatched/ambiguous ones
/// (sentinel `u32::MAX`) are dropped so they can't be selected for a write.
#[tauri::command]
fn library_filter(filter: FilterArgs, state: State<'_, AppState>) -> Result<Vec<u32>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let f = search::Filter {
        name_substr: filter.name_substr,
        amp: filter.amp,
        block: filter.block,
        ir: filter.ir,
        sic: filter.sic,
        level_lt: filter.level_lt,
        level_gt: filter.level_gt,
    };
    let records: Vec<search::PresetRecord> =
        lib.records.iter().map(|r| r.search_record()).collect();
    Ok(search::filter_records(&records, &f)
        .into_iter()
        .map(|r| r.list_index)
        .filter(|&i| i != u32::MAX)
        .collect())
}

/// Preview a bulk operation over a selection — per-preset change list, writes nothing.
#[tauri::command]
fn bulk_dry_run(
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::DryRunEntry>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    let targets = targets_from_library(lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(bulkrun::dry_run(&targets, operation.as_ref()))
}

// ─── Block templates (persisted in the app config dir, applied via OpSpec::ApplyBlock) ─

/// Path to the persisted block-template library (app config dir).
fn block_lib_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app config dir: {e}"))?
        .join("block_library.json"))
}

/// The saved block templates.
#[tauri::command]
fn list_block_templates(app: tauri::AppHandle) -> Result<Vec<blocklib::BlockTemplate>, String> {
    Ok(blocklib::load_library_from_path(&block_lib_path(&app)?).unwrap_or_default())
}

/// Capture the first block of `model` from a source preset into the library under
/// `name` (replaces a same-named entry). PURE capture + persist to disk; no device.
#[tauri::command]
fn save_block_template(
    app: tauri::AppHandle,
    source_list_index: u32,
    model: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Vec<blocklib::BlockTemplate>, String> {
    let template = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let v: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        blocklib::capture_block(&v, &model, &name)
            .ok_or_else(|| format!("source preset has no block of model {model:?}"))?
    };
    let path = block_lib_path(&app)?;
    let mut lib = blocklib::load_library_from_path(&path).unwrap_or_default();
    lib.retain(|t| t.name != name); // replace a same-named template
    lib.push(template);
    blocklib::save_library_to_path(&path, &lib)?;
    Ok(lib)
}

// ─── Create variants (create = append-import) ────────────────────────────────────

/// Serde-friendly mirror of `variants::VariantEdit`.
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
enum VariantEditArg {
    SetParam {
        model: String,
        param: String,
        value: f64,
    },
    ReplaceBlock {
        from: String,
        to: String,
    },
    SetBpm {
        bpm: f64,
    },
}

#[derive(serde::Deserialize, Clone)]
struct RecipeArg {
    name_suffix: String,
    edits: Vec<VariantEditArg>,
}
impl RecipeArg {
    fn to_recipe(&self) -> variants::Recipe {
        variants::Recipe {
            name_suffix: self.name_suffix.clone(),
            edits: self
                .edits
                .iter()
                .map(|e| match e.clone() {
                    VariantEditArg::SetParam {
                        model,
                        param,
                        value,
                    } => variants::VariantEdit::SetParam {
                        model,
                        param,
                        value,
                    },
                    VariantEditArg::ReplaceBlock { from, to } => {
                        variants::VariantEdit::ReplaceBlock { from, to }
                    }
                    VariantEditArg::SetBpm { bpm } => variants::VariantEdit::SetBpm(bpm),
                })
                .collect(),
        }
    }
}

/// Create a variant on the device (LIVE, append-only): clone + recipe -> .preset bytes
/// -> `import_preset` (the device files it at the next empty slot — variants carry no
/// inherited Song membership, so an append is correct, NOT an in-place overwrite).
/// HW-validation pending.
#[tauri::command]
async fn create_variant(
    source_list_index: u32,
    recipe: RecipeArg,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let variant_bytes = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let rec = lib
            .records
            .iter()
            .find(|r| r.list_index == Some(source_list_index))
            .ok_or_else(|| format!("source preset {source_list_index} not found / not matched"))?;
        let source: serde_json::Value =
            serde_json::from_str(&rec.decoded_json).map_err(|e| e.to_string())?;
        let variant = variants::apply_recipe(&source, &recipe.to_recipe())?;
        backup::xor_jld(
            serde_json::to_string(&variant)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )
    };
    with_released_seize(state.session.clone(), move || {
        let reported = Session::connect()?.import_preset(&variant_bytes)?;
        Ok(match reported {
            Some((le, slot)) => {
                format!("variant imported (device reported listEnum {le}, slot {slot})")
            }
            None => "variant imported (device gave no slot echo — re-list to confirm)".to_string(),
        })
    })
    .await
}

// ─── Bulk rename (apply = LIVE) ──────────────────────────────────────────────────

/// Max preset-name length for the rename validator. Generous pending an exact HW
/// limit (better not to falsely flag than to block a valid name).
const RENAME_MAX: usize = 60;

/// Serde-friendly mirror of `rename::RenameSpec` (which has no serde derive).
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type")]
enum RenameSpecArg {
    FindReplace { from: String, to: String },
    Template { pattern: String },
    Number { width: usize, start: u32 },
}
impl RenameSpecArg {
    fn to_spec(&self) -> rename::RenameSpec {
        match self.clone() {
            RenameSpecArg::FindReplace { from, to } => rename::RenameSpec::FindReplace { from, to },
            RenameSpecArg::Template { pattern } => rename::RenameSpec::Template { pattern },
            RenameSpecArg::Number { width, start } => rename::RenameSpec::Number { width, start },
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct RenameRow {
    list_index: Option<u32>,
    name: String,
    new_name: String,
    /// Set on a no-op rename (new == old) or a validation failure; the row is then skipped on apply.
    note: Option<String>,
}

/// Compute new names for the selection (PURE — no device). The `{n}` token / Number
/// offset use each preset's position within the selection.
fn rename_rows(
    lib: &library::Library,
    selection: &[u32],
    spec: &rename::RenameSpec,
) -> Vec<RenameRow> {
    selection
        .iter()
        .enumerate()
        .filter_map(|(i, idx)| {
            let r = lib.records.iter().find(|r| r.list_index == Some(*idx))?;
            let new_name = rename::apply_rename(&r.display_name, i, spec);
            let note = if new_name == r.display_name {
                Some("unchanged".to_string())
            } else {
                rename::validate_name(&new_name, RENAME_MAX).err()
            };
            Some(RenameRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                new_name,
                note,
            })
        })
        .collect()
}

#[derive(serde::Serialize)]
struct RenameApplyRow {
    list_index: u32,
    new_name: String,
    applied: bool,
    error: Option<String>,
}

/// Apply the rename to each selected preset on the device (LIVE): per preset
/// `load_preset → rename_current_preset → save_current_preset` (the PC "save under a
/// new name" pair). Rows with a validation note / no-op are skipped.
#[tauri::command]
async fn bulk_rename(
    selection: Vec<u32>,
    spec: RenameSpecArg,
    state: State<'_, AppState>,
) -> Result<Vec<RenameApplyRow>, String> {
    let rows: Vec<RenameRow> = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        rename_rows(lib, &selection, &spec.to_spec())
    };
    // Only valid, changed, matched rows are applied.
    let jobs: Vec<(u32, String, String)> = rows
        .into_iter()
        .filter(|r| r.note.is_none())
        .filter_map(|r| r.list_index.map(|i| (i, r.name, r.new_name)))
        .collect();
    if jobs.is_empty() {
        return Err("nothing to rename (all rows unchanged or invalid)".into());
    }
    with_released_seize(state.session.clone(), move || {
        let mut out = Vec::new();
        for (idx, name_before, new_name) in jobs {
            let mut row = RenameApplyRow {
                list_index: idx,
                new_name: new_name.clone(),
                applied: false,
                error: None,
            };
            let res = (|| -> Result<(), String> {
                let mut s = Session::connect()?;
                // Load (accumulating the PresetLoaded echo) then CONFIRM the target is
                // active before renaming+saving — a dropped load would otherwise
                // rename+save the WRONG preset over this slot (same fix as the
                // single-preset rename_save_preset path).
                s.clear_raw();
                s.send_and_collect(&proto::load_preset((idx + 1) as u64, 1), 200)?;
                s.confirm_active(idx, Some(&name_before))?;
                s.rename_current_preset(&new_name)?;
                s.save_current_preset(idx)?; // persist (rename = save-under-new-name)
                Ok(())
            })();
            match res {
                Ok(()) => row.applied = true,
                Err(e) => row.error = Some(e),
            }
            out.push(row);
        }
        Ok(out)
    })
    .await
}

// ─── Audition (MEASURE — re-amp render → WAV data URL for playback) ──────────────

/// Standard-alphabet base64 (no padding omitted) — small + dependency-free, for
/// embedding a rendered WAV as a `data:` URL the webview can play.
fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Encode mono f32 samples as a 32-bit-float WAV (in memory).
fn wav_bytes(samples: &[f32], rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut w =
            hound::WavWriter::new(&mut cursor, spec).map_err(|e| format!("wav writer: {e}"))?;
        for &s in samples {
            w.write_sample(s).map_err(|e| format!("wav write: {e}"))?;
        }
        w.finalize().map_err(|e| format!("wav finalize: {e}"))?;
    }
    Ok(cursor.into_inner())
}

/// Re-amp a preset and return its processed audio as a `data:audio/wav;base64,…` URL
/// the frontend `<audio>` element can play (MEASURE — drives the device,
/// HW-pending). A/B and before/after are a later refinement (render two + compare).
#[tauri::command]
async fn audition_render(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Cache hit → return the already-rendered clip, skipping the re-amp pass.
    let cache_key = audition::clip_key(slot, topology_id.as_deref().unwrap_or("default"));
    if let Some(url) = lock_ok(&state.clip_cache).get(&cache_key) {
        return Ok(url);
    }
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    let url = with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let wav = wav_bytes(&samples, rate)?;
        Ok(format!("data:audio/wav;base64,{}", base64_encode(&wav)))
    })
    .await?;
    state
        .clip_cache
        .lock()
        .unwrap()
        .insert(&cache_key, url.clone());
    Ok(url)
}

// ─── Spectrum report (MEASURE — re-amp capture + band analysis) ──────────────────

/// Per-band energies + tonal flags for one preset.
#[derive(serde::Serialize)]
struct SpectrumResult {
    bands: Vec<f64>,
    flags: Vec<String>,
}

/// Re-amp a preset and analyze its captured spectrum (MEASURE — drives the device;
/// HW-validation pending). Reuses the leveller's validated capture sequence.
#[tauri::command]
async fn spectrum_scan(
    app: tauri::AppHandle,
    slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<SpectrumResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
        let bands = spectrum::band_energies(&samples, rate as f32, &spectrum::default_bands());
        let flags = spectrum::tonal_flags(&bands);
        Ok(SpectrumResult { bands, flags })
    })
    .await
}

/// EQ-match: source vs reference spectra + the per-band gain deltas that move
/// source toward reference, with a preview of the matched spectrum.
#[derive(serde::Serialize)]
struct EqMatchResult {
    source_bands: Vec<f64>,
    reference_bands: Vec<f64>,
    distance: f64,
    deltas: Vec<f64>,
    matched_bands: Vec<f64>,
}

/// Re-amp two presets and compute the EQ-match from `source` toward `reference`
/// (MEASURE — drives the device; HW-validation pending).
#[tauri::command]
async fn eq_match(
    app: tauri::AppHandle,
    source_slot: u32,
    reference_slot: u32,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<EqMatchResult, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (s, sr) = leveller::capture_samples(source_slot, &stim, 0.5)?;
        let source_bands = spectrum::band_energies(&s, sr as f32, &cfg);
        let (r, rr) = leveller::capture_samples(reference_slot, &stim, 0.5)?;
        let reference_bands = spectrum::band_energies(&r, rr as f32, &cfg);
        let distance = spectrum::spectral_distance(&source_bands, &reference_bands);
        let deltas = spectrum::eq_match_deltas(&source_bands, &reference_bands);
        let matched_bands = spectrum::apply_deltas(&source_bands, &deltas);
        Ok(EqMatchResult {
            source_bands,
            reference_bands,
            distance,
            deltas,
            matched_bands,
        })
    })
    .await
}

/// Re-amp a target + candidate presets and rank the candidates by spectral distance
/// to the target, nearest first ("best match" — MEASURE; HW-pending).
#[tauri::command]
async fn rank_candidates(
    app: tauri::AppHandle,
    target_slot: u32,
    candidate_slots: Vec<u32>,
    topology_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<spectrum::SicRank>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cfg = spectrum::default_bands();
        let (t, tr) = leveller::capture_samples(target_slot, &stim, 0.5)?;
        let target = spectrum::band_energies(&t, tr as f32, &cfg);
        let mut cands = Vec::with_capacity(candidate_slots.len());
        for slot in candidate_slots {
            let (c, cr) = leveller::capture_samples(slot, &stim, 0.5)?;
            cands.push((
                format!("slot {slot}"),
                spectrum::band_energies(&c, cr as f32, &cfg),
            ));
        }
        Ok(spectrum::rank_sics(&target, &cands))
    })
    .await
}

// ─── Firmware-migration analysis over the imported library (no device) ────────────

/// One preset affected by a firmware migration: the in-use block models it
/// references that are absent from the target catalog.
#[derive(serde::Serialize)]
struct MigrationRow {
    list_index: Option<u32>,
    name: String,
    affected_blocks: Vec<String>,
}

/// Scan the imported library against a target firmware's `target_catalog` (valid block
/// model ids) and list presets that reference models the target no longer has
/// (OFFLINE read-only). The user resolves them via Block Replace.
#[tauri::command]
fn migration_scan(
    target_catalog: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationRow>, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    // The "old" catalog is every block model actually used across the library.
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, &target_catalog);
    let removed: std::collections::HashSet<String> = diff.removed.into_iter().collect();
    // Index by array position (stable identity), then map results back to records.
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            serde_json::from_str(&r.decoded_json)
                .ok()
                .map(|v| (i as u32, v))
        })
        .collect();
    Ok(migration::scan_affected(&presets, &removed)
        .into_iter()
        .map(|(pos, blocks)| {
            let r = &lib.records[pos as usize];
            MigrationRow {
                list_index: r.list_index,
                name: r.display_name.clone(),
                affected_blocks: blocks,
            }
        })
        .collect())
}

/// The migration plan: how the catalog changed + the concrete block swaps to run.
#[derive(serde::Serialize)]
struct MigrationPlan {
    classified: migration::ClassifiedDiff,
    plan: Vec<migration::Replacement>,
}

/// Build the classified catalog diff + per-preset replacement plan from a target
/// catalog + a user rename map (matched presets only, keyed by real list_index).
fn compute_migration_plan(
    lib: &library::Library,
    target_catalog: &[String],
    rename_map: &std::collections::BTreeMap<String, String>,
) -> MigrationPlan {
    let mut in_use: std::collections::BTreeSet<String> = Default::default();
    for r in &lib.records {
        for b in &r.facets.blocks {
            in_use.insert(b.clone());
        }
    }
    let old: Vec<String> = in_use.into_iter().collect();
    let diff = migration::diff_catalogs(&old, target_catalog);
    let classified = migration::classify(&diff, rename_map);
    let removed: std::collections::HashSet<String> = diff.removed.iter().cloned().collect();
    let presets: Vec<(u32, serde_json::Value)> = lib
        .records
        .iter()
        .filter_map(|r| r.list_index.zip(serde_json::from_str(&r.decoded_json).ok()))
        .collect();
    let affected = migration::scan_affected(&presets, &removed);
    let plan = migration::plan_replacements(&affected, rename_map);
    MigrationPlan { classified, plan }
}

/// Preview the renamed/removed/added block ids + the planned swaps for a
/// target firmware catalog and a rename map (pure, no device).
#[tauri::command]
fn migration_plan(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> Result<MigrationPlan, String> {
    let guard = lock_ok(&state.library);
    let lib = guard.as_ref().ok_or("no library imported")?;
    Ok(compute_migration_plan(lib, &target_catalog, &rename_map))
}

/// One preset's migration-apply outcome.
#[derive(serde::Serialize)]
struct MigrationApplyRow {
    list_index: u32,
    swaps: usize,
    applied: bool,
    error: Option<String>,
}

/// Snapshot a preset's pre-edit JSON before an in-place migration write, so a mid-run
/// failure still leaves it revertible (bulkrun's AC2 discipline, which `migration_apply`
/// bypasses by calling `replace_inplace_core` directly). `Err` ⇒ DO NOT WRITE. `source`
/// is `"offline-file"`: migration edits a complete `.preset` from the offline library.
fn snapshot_before_migrate(
    backup_dir: &std::path::Path,
    list_index: u32,
    display_name: &str,
    before_json: &str,
) -> Result<std::path::PathBuf, String> {
    // list_enum 1 = My Presets; source "offline-file" = migration edits a complete
    // `.preset` from the offline library.
    bulkrun::save_pre_write_snapshot(
        backup_dir,
        1,
        list_index,
        display_name,
        "offline-file",
        before_json,
    )
}

/// Apply the migration plan: per affected preset, swap each renamed block
/// (Block Replace defaults) and re-import OFFLINE in place. `dry_run` previews the swap
/// counts without writing (HW-pending — device write gated by the read-only policy).
#[tauri::command]
async fn migration_apply(
    target_catalog: Vec<String>,
    rename_map: std::collections::BTreeMap<String, String>,
    dry_run: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<MigrationApplyRow>, String> {
    // Build the plan + each affected preset's (original json, edited bytes) under the lock.
    let mut jobs: Vec<(u32, String, usize, String, Vec<u8>)> = Vec::new();
    {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        let plan = compute_migration_plan(lib, &target_catalog, &rename_map).plan;
        // Group the plan by preset, then apply all its swaps to one decoded copy.
        let mut by_preset: std::collections::BTreeMap<u32, Vec<migration::Replacement>> =
            Default::default();
        for r in plan {
            by_preset.entry(r.list_index).or_default().push(r);
        }
        for (li, reps) in by_preset {
            let Some(rec) = lib.records.iter().find(|r| r.list_index == Some(li)) else {
                continue;
            };
            let before_json = rec.decoded_json.clone();
            let mut v: serde_json::Value =
                serde_json::from_str(&before_json).map_err(|e| e.to_string())?;
            let swaps: usize = reps
                .iter()
                .map(|r| migration::apply_replacement(&mut v, r))
                .sum();
            let bytes = backup::xor_jld(
                serde_json::to_string(&v)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            );
            jobs.push((li, rec.display_name.clone(), swaps, before_json, bytes));
        }
    }
    // Dry run — preview the swap counts without touching the device or a backup.
    if dry_run {
        return Ok(jobs
            .into_iter()
            .map(|(li, _name, swaps, _before, _bytes)| MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: None,
            })
            .collect());
    }
    // A write must be revertible: snapshot each preset's pre-edit state before its
    // in-place re-import (bulkrun's AC2, which migration bypasses by calling
    // replace_inplace_core directly). Resolve the backups dir once up front so a whole
    // run refuses rather than writing the first preset unbacked.
    let backup_dir = backup::backups_dir(&app)?;
    let mut rows = Vec::with_capacity(jobs.len());
    for (li, display_name, swaps, before_json, bytes) in jobs {
        // AC2 — snapshot before writing; refuse the write if it fails (no backup = not
        // revertible). Uses the ORIGINAL decoded_json, not the edited bytes.
        if let Err(e) = snapshot_before_migrate(&backup_dir, li, &display_name, &before_json) {
            rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(format!("snapshot: {e} — NOT written (kept revertible)")),
            });
            continue;
        }
        let outcome = with_released_seize(state.session.clone(), move || {
            replace_inplace_core(li, &bytes)
        })
        .await;
        match outcome {
            Ok(o) if o.edit_landed => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: true,
                error: None,
            }),
            Ok(_) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some("edit did not land".into()),
            }),
            Err(e) => rows.push(MigrationApplyRow {
                list_index: li,
                swaps,
                applied: false,
                error: Some(e),
            }),
        }
    }
    Ok(rows)
}

#[derive(serde::Serialize)]
struct BulkApplyResult {
    run_id: String,
    report: bulkrun::RunReport,
}

/// Apply a bulk operation to a selection, snapshotting each preset first, and record
/// the run so it can be reverted. Runs with the app's HID seize released (the io
/// adapters open their own connections).
#[tauri::command]
async fn bulk_apply(
    app: tauri::AppHandle,
    selection: Vec<u32>,
    op: bulk_cmd::OpSpec,
    backup: bool,
    state: State<'_, AppState>,
) -> Result<BulkApplyResult, String> {
    let targets = {
        let guard = lock_ok(&state.library);
        let lib = guard.as_ref().ok_or("no library imported")?;
        targets_from_library(lib, &selection)?
    };
    if targets.is_empty() {
        return Err("selection is empty (no matched, writable presets)".into());
    }
    let io_path = op.io_path();
    let op_label = bulk_cmd::build_operation(&op)?.label();
    let backup_dir = if backup {
        backup::backups_dir(&app)?
    } else {
        std::env::temp_dir().join("tmp-companion-nobackup")
    };
    let run_id = now_stamp_id();

    // Build the operation INSIDE the blocking closure (OpSpec is Send; a boxed
    // Operation is not), so the closure stays Send for spawn_blocking.
    let report = with_released_seize(state.session.clone(), move || {
        let operation = bulk_cmd::build_operation(&op)?;
        let mut io = io_for_path(io_path);
        Ok(bulkrun::apply(
            &targets,
            operation.as_ref(),
            io.as_mut(),
            &backup_dir,
        ))
    })
    .await?;

    lock_ok(&state.runs).insert(
        run_id.clone(),
        bulk_cmd::StoredRun {
            report: report.clone(),
            op_label,
        },
    );
    Ok(BulkApplyResult { run_id, report })
}

/// Revert a recorded run — restore every touched preset to its pre-run snapshot.
#[tauri::command]
async fn bulk_revert(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<bulkrun::RevertEntry>, String> {
    let stored = {
        let guard = lock_ok(&state.runs);
        guard
            .get(&run_id)
            .cloned()
            .ok_or_else(|| format!("no run {run_id} to revert"))?
    };
    let report = stored.report;
    with_released_seize(state.session.clone(), move || {
        // Revert ALWAYS restores via OFFLINE re-import of the captured original
        // `.preset` (the snapshot is a complete offline-file). This is correct for
        // both forward paths: a LIVE (ParamEdit) run can't be reverted by `LiveIo`
        // — its `write` diffs before-vs-after, and on revert both are the snapshot,
        // so the diff is empty and the device keeps the applied change while falsely
        // reporting `restored`. OfflineIo re-imports the original bytes in place
        // (identity-preserving), which undoes either forward path.
        let mut io = io_for_path(bulk_cmd::IoPath::Offline);
        Ok(bulkrun::revert(&report, io.as_mut()))
    })
    .await
}

// ─── probe bulk subcommands (headless HW validation) ─────────────────────────────

fn format_dry_run(entries: &[bulkrun::DryRunEntry]) -> String {
    let mut s = format!("[probe bulk dry-run] {} preset(s)\n", entries.len());
    for e in entries {
        s += &format!(
            "  slot {:>3} {:<24} {:?}{}\n",
            e.list_index,
            e.display_name,
            e.status,
            match &e.error {
                Some(err) => format!("  ERROR: {err}"),
                None if !e.changes.is_empty() => format!("  ({} field(s) change)", e.changes.len()),
                None => String::new(),
            }
        );
    }
    s
}

/// Build + reconcile a library from `folder` against the live device (Pro Control
/// closed). Shared by the dry-run / apply probes.
fn probe_load_reconciled_library(folder: &str) -> Result<library::Library, String> {
    let (mut records, errors) =
        library::load_library_from_dir(std::path::Path::new(folder), &library_categories())?;
    if !errors.is_empty() {
        eprintln!("[probe] {} file(s) skipped: {errors:?}", errors.len());
    }
    let device_list = Session::connect()?.list_my_presets()?;
    library::reconcile_with_device(&mut records, &device_list);
    Ok(library::Library {
        folder: folder.into(),
        records,
    })
}

/// `probe --bulk-dryrun <folder> <opspec.json>` — preview an op over all matched
/// presets; writes nothing.
pub fn probe_bulk_dryrun(folder: &str, opspec_json: &str) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection: Vec<u32> = lib.records.iter().filter_map(|r| r.list_index).collect();
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    Ok(format_dry_run(&bulkrun::dry_run(
        &targets,
        operation.as_ref(),
    )))
}

/// `probe --bulk-apply <folder> <opspec.json> [--slots a,b] [revert]` — apply on the
/// real device (backup-first), optionally reverting immediately for a full round-trip.
pub fn probe_bulk_apply(
    folder: &str,
    opspec_json: &str,
    slots: Option<Vec<u32>>,
    revert: bool,
) -> Result<String, String> {
    let lib = probe_load_reconciled_library(folder)?;
    let op: bulk_cmd::OpSpec =
        serde_json::from_str(opspec_json).map_err(|e| format!("parse opspec: {e}"))?;
    let selection =
        slots.unwrap_or_else(|| lib.records.iter().filter_map(|r| r.list_index).collect());
    let targets = targets_from_library(&lib, &selection)?;
    let operation = bulk_cmd::build_operation(&op)?;
    let mut io = io_for_path(op.io_path());
    let backup_dir = std::env::temp_dir().join(format!(
        "tmp-companion-probe-backups-{}",
        std::process::id()
    ));

    let report = bulkrun::apply(&targets, operation.as_ref(), io.as_mut(), &backup_dir);
    let mut out = format!(
        "[probe bulk apply] {} target(s): {} changed, {} verified, {} error(s)\n",
        report.entries.len(),
        report.changed(),
        report.verified(),
        report.errors(),
    );
    for e in &report.entries {
        out += &format!(
            "  slot {:>3} {:<24} changed={} verified={}{}\n",
            e.list_index,
            e.display_name,
            e.changed,
            e.verified,
            e.error
                .as_ref()
                .map(|x| format!("  ERROR: {x}"))
                .unwrap_or_default(),
        );
    }
    if revert {
        let rev = bulkrun::revert(&report, io.as_mut());
        out += &format!(
            "[probe bulk revert] {} restored\n",
            rev.iter().filter(|r| r.restored).count()
        );
        for r in &rev {
            out += &format!(
                "  slot {:>3} restored={}{}\n",
                r.list_index,
                r.restored,
                r.error
                    .as_ref()
                    .map(|x| format!("  ERROR: {x}"))
                    .unwrap_or_default(),
            );
        }
    }
    Ok(out)
}

// ─── Loudness audit ──────────────────────────────────────────────────────────────

/// Re-amp each selected preset and flag clipping + loudness outliers (vs the
/// median), the gain-stage audit (MEASURE — drives the device; HW-pending).
#[tauri::command]
async fn audit_loudness(
    app: tauri::AppHandle,
    slots: Vec<u32>,
    topology_id: Option<String>,
    outlier_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<lint::Finding>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let mut measures = Vec::with_capacity(slots.len());
        for slot in slots {
            let (samples, rate) = leveller::capture_samples(slot, &stim, 0.5)?;
            let peak = samples.iter().fold(0f32, |m, &s| m.max(s.abs()));
            let peak_dbfs = if peak > 0.0 {
                20.0 * (peak as f64).log10()
            } else {
                -120.0
            };
            let loud = lufs::measure_mono(&samples, rate)?;
            measures.push(lint::AuditMeasure {
                list_index: slot,
                peak_dbfs,
                loudness_lufs: loud.integrated_lufs,
            });
        }
        Ok(lint::audit_measures(&measures, outlier_lu))
    })
    .await
}

// ─── Song-assignment device WRITE (SongMessage 14–17) ────────────────────────────

/// Bind a user preset (+ scene) to a Song row on the device — `assignSongPreset`.
/// `user_list_index` is 0-based (session applies the device +1). DEVICE WRITE.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn song_assign(
    song_slot: u32,
    song_preset_slot: u32,
    user_list_index: u32,
    footswitch_label: String,
    footswitch_color: u32,
    preset_scene_slot: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.assign_song_preset(
            song_slot,
            song_preset_slot,
            user_list_index,
            &footswitch_label,
            footswitch_color,
            preset_scene_slot,
        )
    })
    .await
}

/// Empty a Song row on the device — `clearSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_clear(
    song_slot: u32,
    song_preset_slot: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.clear_song_preset(song_slot, song_preset_slot)
    })
    .await
}

/// Reorder a Song row on the device — `moveSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_move(
    song_slot: u32,
    old: u32,
    new: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.move_song_preset(song_slot, old, new)
    })
    .await
}

/// Swap two Song rows on the device — `swapSongPreset`. DEVICE WRITE.
#[tauri::command]
async fn song_swap(
    song_slot: u32,
    a: u32,
    b: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.swap_song_preset(song_slot, a, b)
    })
    .await
}

/// Measure each scene's ceiling loudness (re-amp + `loadScene` per scene)
/// and return the per-scene gain offsets to a common target (MEASURE — drives the
/// device; HW-pending). Supersedes hand-entered C values when hardware is present.
#[tauri::command]
async fn level_scenes(
    app: tauri::AppHandle,
    slot: u32,
    scene_count: u32,
    topology_id: Option<String>,
    headroom_lu: f64,
    state: State<'_, AppState>,
) -> Result<Vec<f64>, String> {
    let stim_path = resolve_stimulus(&app, None, topology_id)?;
    with_released_seize(state.session.clone(), move || {
        let stim = read_stimulus_calibrated(&stim_path, None)?;
        let cs = leveller::capture_scene_ceilings(slot, scene_count, &stim)?;
        scenes::normalize_scene_targets(&cs, headroom_lu)
            .ok_or_else(|| "no finite scene loudness measured".to_string())
    })
    .await
}

/// List the bulk-run backup snapshots on disk (newest first), as file paths — the
/// audit trail of what `bulk_apply` snapshotted. A missing dir = no backups.
#[tauri::command]
fn list_snapshots(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    Ok(backup::list_snapshots_in_dir(&backup::backups_dir(&app)?)?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        // Frontend (console/error) + backend `log::*` records → OS log dir
        // (~/Library/Logs/dev.tmpcompanion.app/) and stdout. Gives render
        // crashes and device errors an on-disk trace.
        .plugin(
            tauri_plugin_log::Builder::new()
                // `Builder::new()` already ships DEFAULT_LOG_TARGETS = [Stdout, LogDir]
                // and `.target()` APPENDS — so re-adding the same two duplicated every
                // record (each line written 2× to BOTH the log file and stdout). Clear
                // the defaults first, then set exactly the two sinks we want.
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir { file_name: None },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            list_samples,
            list_pickup_topologies,
            get_store,
            save_profiles,
            save_targets,
            set_playback_level,
            calibrate_profile,
            level_preset,
            level_setlist,
            list_level_blocks,
            import_library,
            library_records,
            library_filter,
            bulk_dry_run,
            bulk_apply,
            bulk_revert,
            migration_scan,
            bulk_rename,
            create_variant,
            list_block_templates,
            save_block_template,
            spectrum_scan,
            audition_render,
            eq_match,
            rank_candidates,
            migration_plan,
            migration_apply,
            audit_loudness,
            list_snapshots,
            song_assign,
            song_clear,
            song_move,
            song_swap,
            level_scenes,
            level_scenes_apply,
            level_scenes_apply_batched,
            cancel_scene_leveling,
            cancel_preset_leveling,
            level_footswitches_apply,
            cancel_footswitch_leveling,
            read_active_preset,
            current_graph,
            request_scene_list,
            stop_live_sync,
            list_songs,
            load_preset_on_amp,
            delete_preset,
            move_preset,
            rename_save_preset,
            load_scene_on_amp,
            read_setlists,
            list_setlist_songs,
            add_song,
            rename_song,
            remove_song,
            set_song_notes,
            set_song_bpm,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_song,
            remove_setlist_song,
            move_setlist_song,
            create_song_full,
            update_song_full,
            add_setlist_songs,
            read_preset_scenes,
            scan_preset_scenes,
            cancel_scene_scan,
            read_library_via_backup,
            list_saved_blocks,
            list_user_irs,
            bulk_replace_live,
            cancel_bulk_replace,
            copy_apply,
            cancel_copy_apply
        ])
        // Native macOS menu. Setting a menu replaces the default, so the standard
        // App / Edit / Window submenus are rebuilt explicitly (Edit is load-bearing
        // — copy/paste in the rename fields ride its predefined items). The
        // non-affiliation notice lives in the standard "About TMP Companion" panel
        // via AboutMetadata; the leveling explainer is in-app (Level tab), so there
        // is no custom Help submenu.
        .menu(|handle| {
            use tauri::menu::{AboutMetadataBuilder, MenuBuilder, SubmenuBuilder};
            let about = AboutMetadataBuilder::new()
                .name(Some("TMP Companion"))
                // ponytail: omit `version` (NSAboutPanelOptionApplicationVersion, the
                // parenthetical) — macOS already shows the bundle's short version, so
                // setting it too renders the redundant `Version 0.1.0 (0.1.0)`.
                .short_version(Some(env!("CARGO_PKG_VERSION")))
                // The dev binary has no bundle icon, so the panel would show a
                // generic folder — set it explicitly (same art as the Dock icon).
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/dock.png")).ok())
                // macOS draws `copyright` as the small line and `credits` as the
                // body. Copyright = the real © line; the affiliation + trademark
                // notice is the body.
                .copyright(Some("© 2026 Pedro Cavadas"))
                .credits(Some(
                    "Fender, Tone Master Pro, and other amp, cabinet, and effect \
                     names are trademarks of their respective owners, used \
                     nominatively to describe compatibility and lineage. \
                     Independent project — not affiliated with Fender Musical \
                     Instruments Corporation.",
                ))
                .build();
            let app_menu = SubmenuBuilder::new(handle, "TMP Companion")
                .about_with_text("About TMP Companion", Some(about))
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .quit()
                .build()?;
            let edit = SubmenuBuilder::new(handle, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let window = SubmenuBuilder::new(handle, "Window")
                .minimize()
                .maximize()
                .separator()
                .fullscreen()
                .close_window()
                .build()?;
            MenuBuilder::new(handle)
                .items(&[&app_menu, &edit, &window])
                .build()
        })
        .setup(|app| {
            // Confirms the logger is live (and gives the log file a deterministic
            // first line). Subsequent warn/error from the device + frontend paths
            // append here too.
            log::info!("TMP Companion {} started", env!("CARGO_PKG_VERSION"));
            // Dock icon for `tauri dev` (the raw binary has no bundle .icns).
            #[cfg(target_os = "macos")]
            dock::set_dock_icon();
            // Hotplug watcher: attach/detach events + dead-seize cleanup.
            use tauri::Manager;
            let session = app.state::<AppState>().session.clone();
            watcher::spawn(app.handle().clone(), session.clone());
            // Device monitor: app-level `connect_device` enables it, then the monitor
            // owns the idle seize with a dense ~250 ms heartbeat, publishes the startup
            // snapshot, and mirrors unsolicited unit pushes as tmp://live-preset /
            // live-scene / scene-list / signal-chain / sync. It coexists with commands
            // via the pause-then-ack protocol inside `lock_device_op`, and only opens
            // HID while `AppState.session` is None.
            monitor::spawn(app.handle().clone(), session);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Offline-UI-e2e backend (`--features e2e`): a windowless MockRuntime app whose REAL
/// commands are invoked over HTTP by the Playwright `bridge-client`. The transport
/// factory routes every device open to a shared `SimDevice`, a fixture startup snapshot
/// makes the app appear connected (no monitor thread), and the bulk backup is served
/// from the built fixture blob — so the real React UI in Chromium drives the real Rust
/// backend down to the (faked) unit. No window, no HTTP-framework dependency: a localhost
/// `std::net` server wrapping `tauri::test::get_ipc_response`. Request/response only —
/// the V1 Copy/Level journeys complete on the command's return value, not on Channels.
/// The one source of truth for the e2e mode: `TMP_E2E_ONLINE` set ⇒ drive the REAL device
/// (no SimDevice factory, real re-amp, real device backup); unset ⇒ the offline fake. Read
/// by `run_e2e_server`, the `/sim/reset` guard, and `audio::reamp_capture`.
#[cfg(feature = "e2e")]
pub(crate) fn e2e_online() -> bool {
    std::env::var("TMP_E2E_ONLINE").is_ok()
}

#[cfg(feature = "e2e")]
pub fn run_e2e_server() {
    use std::net::TcpListener;

    let online = e2e_online();
    // OFFLINE only: default the backup fixture so `read_library_via_backup` decodes it
    // through the real backup path. ONLINE must stream the REAL device backup, so the var
    // must be UNSET — affirmatively CLEAR it (don't just skip the default), or a stale
    // `TMP_E2E_BACKUP_FIXTURE` inherited from a prior offline shell would silently divert
    // the online tier to the fixture instead of the plugged-in unit's real library.
    if online {
        std::env::remove_var("TMP_E2E_BACKUP_FIXTURE");
    } else if std::env::var("TMP_E2E_BACKUP_FIXTURE").is_err() {
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
    }
    // The leveling stimulus (MockRuntime can't resolve bundle resources) — a committed WAV.
    if std::env::var("TMP_E2E_STIMULUS").is_err() {
        std::env::set_var(
            "TMP_E2E_STIMULUS",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/samples/guitar-humbucker.wav"
            ),
        );
    }
    // ONLINE (`TMP_E2E_ONLINE=1`): drive the REAL device — no transport factory, so every
    // Session opens real `Hid`. One real handshake seeds the startup snapshot so
    // connect/list serve it (no Wry-typed monitor on the MockRuntime). The default OFFLINE
    // path installs the `SimDevice` factory + fixture snapshot instead. The server keeps
    // serving either way (a device-absent online run surfaces the error to the spec).
    if online {
        match e2e_seed_online_snapshot() {
            Ok(()) => eprintln!("e2e_server: ONLINE — seeded snapshot from the real device"),
            Err(e) => eprintln!("e2e_server: ONLINE — device handshake failed: {e}"),
        }
    } else {
        e2e_install_offline_fake();
    }

    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            connect_device,
            list_presets,
            read_library_via_backup,
            copy_apply,
            cancel_copy_apply,
            get_store,
            level_preset,
            list_songs,
            read_setlists,
            add_song,
            rename_song,
            remove_song,
            create_song_full,
            update_song_full,
            list_setlist_songs,
            add_setlist,
            rename_setlist,
            remove_setlist,
            add_setlist_songs,
            remove_setlist_song,
            move_setlist_song,
            e2e_seed_scenario,
            e2e_clear_preset,
            e2e_load_preset,
            e2e_reamp_off
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build e2e mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build e2e webview");

    let port: u16 = std::env::var("TMP_E2E_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7600);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind e2e port");
    let mode = if online {
        "ONLINE / real device"
    } else {
        "offline / SimDevice"
    };
    eprintln!("e2e_server: listening on http://127.0.0.1:{port} ({mode})");
    // Single-threaded serial accept: Playwright runs `workers:1` (the device is
    // exclusive-seize), and the webview handle stays on this one thread.
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        e2e_handle_conn(&webview, &mut stream);
    }
}

/// Install the offline fake: the shared `SimDevice` transport factory + a fixture startup
/// snapshot (keep its presets in sync with `e2e/fixtures/backup-fixture.bin` — the
/// build script lists them). Re-callable to reset device state between specs (`/sim/reset`).
#[cfg(feature = "e2e")]
fn e2e_install_offline_fake() {
    // SHOWCASE (`TMP_E2E_SHOWCASE=1`, the marketing-screenshot tour): drive the whole app
    // from the curated, non-personal `e2e/fixtures/showcase/` library instead of the
    // 3-preset test scenario. The committed `.bin` (built from `showcase.json` by the
    // `build_showcase_fixture` generator) is the SAME device-backup shape, so `read_*`
    // decode it unchanged; we just point the env there, derive the preset list + hero graph
    // from it, and seed the curated song/setlist names. No test-gate path touches this.
    if std::env::var("TMP_E2E_SHOWCASE").is_ok() {
        e2e_install_showcase();
        return;
    }
    let sim = crate::sim_device::SimDevice::new();
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));
    // The 3 scenario presets at slots 400/401/402 — same slots the online tier seeds by
    // cloning, and the same presets baked into the backup fixture, so one set of specs
    // runs in both modes. `ensureScenario` finds them present offline and skips seeding.
    let presets = vec![
        session::PresetEntry {
            slot: 400,
            name: "E2E Reference".into(),
        },
        session::PresetEntry {
            slot: 401,
            name: "E2E Target 1".into(),
        },
        session::PresetEntry {
            slot: 402,
            name: "E2E Target 2".into(),
        },
    ];
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);
}

/// Install the SHOWCASE offline fake (marketing screenshots). Points the backup-fixture
/// env at the curated `.bin`, decodes it to derive the preset list + the active preset's
/// hero graph (so the Level chain paints), and seeds the SimDevice with the curated
/// song/setlist names read from `showcase.json` (those names aren't in the decoded archive
/// result; the `.bin` carries presets + graph + song↔preset bindings). Best-effort: any
/// read failure falls back to an empty library rather than panicking the server.
#[cfg(feature = "e2e")]
fn e2e_install_showcase() {
    let bin = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase-fixture.bin"
    );
    std::env::set_var("TMP_E2E_BACKUP_FIXTURE", bin);
    let json = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../e2e/fixtures/showcase/showcase.json"
    );

    // The single curated source — parsed once (`firmware`, `activeSlot`, song/setlist names
    // come from here; presets + graph come from the `.bin`). Null on any read/parse error,
    // so the indexing below all yields empties and the server still boots.
    let spec = std::fs::read_to_string(json)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let names = |key: &str| {
        spec[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .or_else(|| x["name"].as_str())
                            .map(str::to_string)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    // Curated song / setlist names for the live read-back (Songs tab main list).
    let sim = crate::sim_device::SimDevice::new().with_songs(names("songs"), names("setlists"));
    crate::session::e2e_transport::set_factory(Box::new(move || Box::new(sim.clone())));

    // Preset list + hero graph, decoded from the same curated `.bin`.
    let (presets, graph) = match std::fs::read(bin)
        .ok()
        .and_then(|b| read_backup_archive(&b).ok())
    {
        Some(res) => {
            // PresetEntry.slot is the 0-based LIST INDEX; the DB `slot` (i64) is index + 1.
            let presets = res
                .presets
                .iter()
                .map(|r| session::PresetEntry {
                    slot: (r.slot - 1).max(0) as u32,
                    name: r.name.clone(),
                })
                .collect();
            // Hero = the `activeSlot` preset's routed graph.
            let active = spec["activeSlot"].as_u64().unwrap_or(0);
            let graph = res
                .presets
                .iter()
                .find(|r| r.slot as u64 == active)
                .map(|r| r.graph.clone());
            (presets, graph)
        }
        None => (Vec::new(), None),
    };

    let firmware = spec["firmware"].as_str().unwrap_or("1.8.45").to_string();
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(Some(firmware), presets, graph);
}

/// e2e ONLINE seam: one real-device handshake → install the startup snapshot (firmware +
/// My Presets) so `connect_device` / `list_presets` serve it WITHOUT a monitor thread; no
/// transport factory is installed, so every command opens the real seized `Hid`. The
/// graph stays `None` (the hero just won't paint a live chain); the journeys don't need
/// it. Requires the device plugged in + Pro Control closed.
#[cfg(feature = "e2e")]
fn e2e_seed_online_snapshot() -> Result<(), String> {
    let mut s = session::Session::connect_with_firmware()?;
    let fw = s.firmware_version();
    let presets = s.list_my_presets()?;
    drop(s); // release the seize; commands reopen via with_released_seize
    MONITOR_ENABLED.store(true, SeqCst);
    monitor::e2e_install_snapshot(fw, presets, None);
    Ok(())
}

/// Patch ONE slot's name in the startup snapshot's preset list so the UI's snapshot-backed
/// list (the Level tab) reflects a scratch-slot clone/clear immediately. Done locally from
/// the KNOWN write rather than a device re-read — `list_my_presets` lags its own writes
/// (read-after-write propagation), so an immediate re-read installs a stale list.
#[cfg(feature = "e2e")]
fn e2e_patch_snapshot_slot(slot: u32, name: &str) {
    let Some(snap) = monitor::startup_snapshot() else {
        return;
    };
    let mut presets = snap.presets;
    if let Some(e) = presets.iter_mut().find(|p| p.slot == slot) {
        e.name = name.to_string();
    }
    monitor::e2e_install_snapshot(snap.firmware, presets, snap.graph);
}

/// ONLINE-e2e DETERMINISTIC scratch setup: import the THREE committed scenario presets
/// (`e2e/fixtures/scenario-presets.json` — the SAME presetJsons baked into the offline
/// backup fixture) into their list indices (400/401/402). So both modes run the identical
/// fixed presets, validated against known blocks, rather than a clone of whatever is on the
/// unit. Each is placed in-place via [`replace_inplace_core`] (import → land → save to slot
/// → clear the scratch landing, guarded). The target slots start EMPTY (the 400+ scratch
/// zone); [`e2e_clear_preset`] returns them to empty. Idempotent at the spec layer
/// (`ensureScenario` skips when they already exist — i.e. offline).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_seed_scenario(state: State<'_, AppState>) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct ScenarioPreset {
        #[serde(rename = "listIndex")]
        list_index: u32,
        name: String,
        #[serde(rename = "presetJson")]
        preset_json: String,
    }
    let path = std::env::var("TMP_E2E_SCENARIO_PRESETS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/scenario-presets.json"
        )
        .into()
    });
    let raw = std::fs::read(&path).map_err(|e| format!("scenario presets {path}: {e}"))?;
    let presets: Vec<ScenarioPreset> =
        serde_json::from_slice(&raw).map_err(|e| format!("parse scenario presets: {e}"))?;
    with_released_seize(state.session.clone(), move || {
        for (i, p) in presets.iter().enumerate() {
            // ponytail: real-TMP HID open-lockout recovery, plus a deliberate fresh-connect
            // import. replace_inplace_core does its post-import "where did it land?" list read
            // on a FRESH Session::connect() — a full handshake forces the device to
            // re-enumerate the just-imported slot. A held single session is faster but its
            // re-arm list read does NOT reflect a fresh import (read-after-write lag; the
            // device only re-enumerates inside the recognized full handshake), so the scratch
            // slot is invisible and seeding fails. So we keep the fresh-connect path and pay
            // the open-lockout tax with an 8 s quiet gap between presets, which lets the
            // device recover (offline SimDevice has no lockout, and the offline specs use the
            // baked fixture, so this only bites online). ~92 s for three presets — fine for
            // one-time e2e setup; correctness over the held-session speedup.
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(8));
            }
            // A `.preset` file is `xor_jld(compact JSON)`; `import_preset` adds the outer LZ4.
            let bytes = backup::xor_jld(p.preset_json.as_bytes());
            replace_inplace_core(p.list_index, &bytes)?;
            e2e_patch_snapshot_slot(p.list_index, &p.name);
        }
        Ok(())
    })
    .await
}

/// ONLINE-e2e scratch teardown: clear scratch slot `slot` (0-based list index), restoring
/// the empty state. SAFETY: refuses unless the slot currently holds `expect_name` (read in
/// the same session) — so a wrong index can never clear a real preset.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_clear_preset(
    state: State<'_, AppState>,
    slot: u32,
    expect_name: String,
) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        let mut s = Session::connect()?;
        let list = s.list_my_presets_strict()?;
        let entry = list
            .get(slot as usize)
            .ok_or_else(|| format!("slot {slot} out of range"))?;
        if entry.name != expect_name {
            return Err(format!(
                "refusing to clear slot {slot}: expected '{expect_name}', found '{}'",
                entry.name
            ));
        }
        s.clear_user_preset(slot)?;
        e2e_patch_snapshot_slot(slot, "Empty");
        Ok(())
    })
    .await
}

/// ONLINE-e2e end-of-scenario state: recall a preset (0-based list index) on the unit so
/// the test leaves it on a known preset (001 = index 0). Non-destructive (a load, no save).
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_load_preset(state: State<'_, AppState>, slot: u32) -> Result<(), String> {
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.load_preset(slot)
    })
    .await
}

/// ONLINE-e2e safety teardown: disengage re-amp on a fresh connection. The re-amp latch is
/// device-side and survives the HID release, so a Level run KILLED mid-capture (a Playwright
/// timeout tearing down the server) would otherwise leave the unit input-muted. The Level
/// flow's own in-session `set_reamp_mode(false)` doesn't run on an abrupt kill — this is the
/// belt-and-braces OFF the scenario teardown calls. No-op offline (the fake never engages
/// re-amp), so it's harmless on the offline path.
#[cfg(feature = "e2e")]
#[tauri::command]
async fn e2e_reamp_off(state: State<'_, AppState>) -> Result<(), String> {
    if !e2e_online() {
        return Ok(());
    }
    with_released_seize(state.session.clone(), move || {
        Session::connect()?.set_reamp_mode(false).map(|_| ())
    })
    .await
}

/// Parse one HTTP/1.1 request and reply. Routes: `POST /invoke` (the command bridge),
/// `POST /sim/reset` (fresh device state), `GET /health`, `OPTIONS` (CORS preflight).
#[cfg(feature = "e2e")]
fn e2e_handle_conn(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    stream: &mut std::net::TcpStream,
) {
    use std::io::{BufRead, BufReader, Read, Write};

    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() {
        return;
    }
    let mut it = req_line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t
            .strip_prefix("Content-Length:")
            .or_else(|| t.strip_prefix("content-length:"))
        {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }

    let (status, payload) = e2e_route(webview, &method, &path, &body);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST,GET,OPTIONS\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&payload);
    let _ = stream.flush();
}

/// Map a request to `(status, json body)`. `/invoke` wraps the command result in an
/// `{ok,data}` / `{ok,error}` envelope the bridge-client unwraps into resolve/reject.
#[cfg(feature = "e2e")]
fn e2e_route(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    method: &str,
    path: &str,
    body: &[u8],
) -> (&'static str, Vec<u8>) {
    use serde_json::json;
    if method == "OPTIONS" {
        return ("200 OK", Vec::new());
    }
    match (method, path) {
        ("GET", "/health") => ("200 OK", b"{\"ok\":true}".to_vec()),
        ("POST", "/sim/reset") => {
            // ONLINE: the real device IS the state — re-installing the offline fake (a
            // SimDevice factory) would clobber it, so the reset is a no-op online.
            if !e2e_online() {
                e2e_install_offline_fake();
            }
            ("200 OK", b"{\"ok\":true}".to_vec())
        }
        ("POST", "/invoke") => {
            let req: serde_json::Value = serde_json::from_slice(body).unwrap_or(json!({}));
            let cmd = req
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = req.get("args").cloned().unwrap_or(json!({}));
            let request = tauri::webview::InvokeRequest {
                cmd,
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            let env = match tauri::test::get_ipc_response(webview, request) {
                Ok(b) => {
                    let data = b
                        .deserialize::<serde_json::Value>()
                        .unwrap_or(serde_json::Value::Null);
                    json!({ "ok": true, "data": data })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            };
            ("200 OK", serde_json::to_vec(&env).unwrap_or_default())
        }
        _ => (
            "404 Not Found",
            b"{\"ok\":false,\"error\":\"not found\"}".to_vec(),
        ),
    }
}

#[cfg(all(test, feature = "e2e"))]
mod e2e_server_spike {
    use super::*;
    use tauri::test::MockRuntime;
    use tauri::WebviewWindow;

    /// The transport factory + startup snapshot are process-GLOBAL; cargo runs tests in
    /// parallel, so the factory-installing tests must hold this for their whole body or
    /// they stomp each other's fake (a hard-to-spot cross-contamination).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        lock_ok(&SERIAL)
    }

    /// Invoke a command through the SAME IPC path the HTTP bridge uses: a JSON body in,
    /// the command's JSON response out (or its error value).
    fn invoke(
        webview: &WebviewWindow<MockRuntime>,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<serde_json::Value>().expect("json body"))
    }

    /// The full OFFLINE Copy journey driven through the real backend exactly as the UI
    /// drives it — connect → list presets → read the library → copy_apply — with the
    /// device replaced by a `SimDevice` (via the transport factory) and the bulk backup
    /// replaced by the built fixture blob. This is "UI to unit" minus the browser: every
    /// command runs for real over the mock IPC; only the USB transport + the snapshot are
    /// faked. The HTTP bridge + Playwright layer reuses this exact wiring.
    #[test]
    fn offline_copy_journey_through_real_backend() {
        use std::sync::atomic::Ordering::SeqCst;
        let _serial = serial();

        // One shared fake: every Session::connect* (command lane) clones it.
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));
        // The library read decodes the fixture blob through the real backup path.
        std::env::set_var(
            "TMP_E2E_BACKUP_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../e2e/fixtures/backup-fixture.bin"
            ),
        );
        // Pre-fill the startup snapshot so connect/list serve it with no monitor thread —
        // the 3 scenario presets at slots 400/401/402 (matching the backup fixture).
        let presets = vec![
            crate::session::PresetEntry {
                slot: 400,
                name: "E2E Reference".into(),
            },
            crate::session::PresetEntry {
                slot: 401,
                name: "E2E Target 1".into(),
            },
            crate::session::PresetEntry {
                slot: 402,
                name: "E2E Target 2".into(),
            },
        ];
        MONITOR_ENABLED.store(true, SeqCst);
        monitor::e2e_install_snapshot(Some("1.8.45".into()), presets, None);

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                connect_device,
                list_presets,
                read_library_via_backup,
                copy_apply
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // 1. connect → the pre-filled snapshot (firmware).
        let conn = invoke(&webview, "connect_device", serde_json::json!({})).expect("connect");
        assert_eq!(
            conn.get("firmware").and_then(|v| v.as_str()),
            Some("1.8.45")
        );

        // 2. list presets → the snapshot's 3 fixture entries.
        let list = invoke(&webview, "list_presets", serde_json::json!({})).expect("list");
        assert_eq!(list.as_array().map(|a| a.len()), Some(3), "presets: {list}");

        // 3. read the library via the fixture backup → 3 rows, decoded graphs.
        let lib =
            invoke(&webview, "read_library_via_backup", serde_json::json!({})).expect("library");
        let rows = lib
            .get("presets")
            .and_then(|p| p.as_array())
            .expect("library presets array");
        assert_eq!(rows.len(), 3, "library rows: {lib}");
        assert!(
            rows.iter()
                .any(|r| r.get("graph").is_some_and(|g| !g.is_null())),
            "at least one library row carries a decoded signal graph: {lib}"
        );

        // 4. copy_apply a dry-run replace on the target → outcome "updated", NOTHING saved.
        // The job is the exact camelCase wire shape `CopyJob`/`CopyOp`/`CopyRepl` accept
        // (the input-only structs the frontend's `diffToOps` produces). The fake confirms
        // any structural edit, so the nodeId need not match a fixture node.
        let jobs = serde_json::json!([{
            "listIndex": 401,
            "name": "E2E Target 1",
            "ops": [{
                "kind": "replace",
                "group": "G1",
                "nodeId": "ACD_PhaserP90",
                "repl": { "kind": "model", "fenderId": "ACD_KingOfTone" }
            }]
        }]);
        let items = invoke(
            &webview,
            "copy_apply",
            serde_json::json!({ "jobs": jobs, "save": false, "onResult": "__CHANNEL__:0" }),
        )
        .expect("copy_apply");
        let items = items.as_array().expect("copy items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get("outcome").and_then(|v| v.as_str()),
            Some("updated"),
            "copy outcome: {items:?}"
        );
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Replace { .. })),
            "the replace reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "dry run must not save: {ev:?}"
        );
    }

    /// The Level journey's measure→solve→apply path runs end-to-end OFFLINE: the device
    /// goes through the `SimDevice` factory and the re-amp capture through the
    /// `--features e2e` audio fake (`audio::reamp_capture` returns the stimulus), so the
    /// leveler produces a finite `C` / final level with no hardware. Proves the audio
    /// seam the UI Level run depends on; loudness fidelity stays the online tier's job.
    #[test]
    fn offline_level_preset_runs_against_the_fake_audio() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        // 0.5 s of a 440 Hz tone at 48 kHz — non-silent so the loudness meter is finite.
        let rate = 48_000usize;
        let stim: Vec<f32> = (0..rate / 2)
            .map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();

        let opts = crate::leveller::LevelOptions {
            save: false,
            verify: true,
            ..Default::default()
        };
        let r = crate::leveller::level_preset(0, &stim, -30.0, opts, &[], || false)
            .expect("level_preset");
        assert!(
            r.final_level.is_finite() && r.final_level > 0.0,
            "solved a finite level: {r:?}"
        );
        assert!(
            r.measured_lufs.is_finite(),
            "measured a finite loudness: {r:?}"
        );
        // Dry run — the fake recorded a level set but no save.
        let ev = sim.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::PresetLevel(_))),
            "the level setter reached the fake: {ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, crate::sim_device::SimEvent::Saved(_))),
            "save:false must not save: {ev:?}"
        );
    }

    /// Songs CRUD through the real backend over the mock IPC: the SimDevice models the
    /// song wire protocol (list / add / rename / remove), so `list_songs` reads the seed,
    /// a write mutates it, and the read-back reflects the change — the Songs tab's
    /// read-after-write contract, with no hardware.
    #[test]
    fn offline_songs_crud_through_real_backend() {
        let _serial = serial();
        let sim = crate::sim_device::SimDevice::new();
        let sim_for_factory = sim.clone();
        crate::session::e2e_transport::set_factory(Box::new(move || {
            Box::new(sim_for_factory.clone())
        }));

        let app = tauri::test::mock_builder()
            .manage(AppState::default())
            .invoke_handler(tauri::generate_handler![
                list_songs,
                read_setlists,
                add_song,
                rename_song,
                remove_song
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("build webview");

        // Seed: 2 songs, 1 setlist.
        let songs = invoke(&webview, "list_songs", serde_json::json!({})).expect("list_songs");
        assert_eq!(
            songs.as_array().map(|a| a.len()),
            Some(2),
            "seed songs: {songs}"
        );
        let setlists =
            invoke(&webview, "read_setlists", serde_json::json!({})).expect("read_setlists");
        assert_eq!(
            setlists.as_array().map(|a| a.len()),
            Some(1),
            "seed setlists: {setlists}"
        );

        // Add → read-back reflects it.
        let after_add = invoke(
            &webview,
            "add_song",
            serde_json::json!({ "name": "Soundcheck" }),
        )
        .expect("add_song");
        assert_eq!(
            after_add.as_array().map(|a| a.len()),
            Some(3),
            "after add: {after_add}"
        );
        assert!(sim.song_names().iter().any(|n| n == "Soundcheck"));

        // Remove the first → back to 2.
        let after_rm = invoke(
            &webview,
            "remove_song",
            serde_json::json!({ "slot": 1, "expectName": "Opening Set" }),
        )
        .expect("remove_song");
        assert_eq!(
            after_rm.as_array().map(|a| a.len()),
            Some(2),
            "after remove: {after_rm}"
        );
        assert!(!sim.song_names().iter().any(|n| n == "Opening Set"));
    }
}

#[cfg(test)]
mod audition_tests {
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

    #[test]
    fn backup_preset_scenes_parse_names_and_fs_tags() {
        // The DB presetJson is the same plaintext shape as the live field-3 doc:
        // scenes[].sceneName slot-ordered + an ftsw map assigning footswitches to
        // scene slots. decode_preset_scenes must yield names + 1-based FS tags.
        let json = br#"{"ftsw":[[{"func":"scene","sceneSlot":1,"isActive":true}],[{"func":"scene","sceneSlot":2,"isActive":true}]],"lastLoadedScene":0,"scenes":[{"sceneName":"Rhythm","uuid":"a"},{"sceneName":"Crunch","uuid":"b"},{"sceneName":"Lead","uuid":"c"}]}"#;
        let ps = decode_preset_scenes(json).expect("parse");
        assert_eq!(ps.scenes, vec!["Rhythm", "Crunch", "Lead"]);
        // ftsw assigns FS1→scene 1, FS2→scene 2 (1-based tag = switch index + 1);
        // scene 0 has no footswitch.
        assert_eq!(ps.fs, vec![None, Some(1), Some(2)]);
    }

    /// Build a real in-memory device-backup archive from one SQL script: run it through
    /// `sqlite3` to a fresh temp `normalDb.db3`, tar it under the firmware's logical
    /// `databaseBackup` entry name, then LZ4-frame compress — the exact shape
    /// `read_backup_archive` decodes. Shared by the two backup-decode tests and the
    /// showcase-fixture generator (a per-call counter keeps parallel temp dirs distinct).
    fn build_backup_archive(sql: &str) -> Vec<u8> {
        use std::io::Write as _;
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let uniq = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "tmp-companion-backup-{}-{uniq}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("normalDb.db3");
        let _ = std::fs::remove_file(&db_path);
        let status = std::process::Command::new("sqlite3")
            .arg(&db_path)
            .arg(sql)
            .status()
            .expect("spawn sqlite3");
        assert!(status.success(), "sqlite3 create failed");
        let db_bytes = std::fs::read(&db_path).expect("read db");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&dir);

        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_size(db_bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    "databaseBackup",
                    std::io::Cursor::new(&db_bytes),
                )
                .expect("tar append");
            builder.finish().expect("tar finish");
        }
        let mut archive = Vec::new();
        {
            let mut enc = lz4_flex::frame::FrameEncoder::new(&mut archive);
            enc.write_all(&tar_bytes).expect("lz4 write");
            enc.finish().expect("lz4 finish");
        }
        archive
    }

    /// A decoded backup row carries the routed signal-chain `graph` (lanes/stages)
    /// for a NON-active preset — the "Copy blocks between presets" frontend reads each
    /// preset's real topology from this without a device round-trip. Builds a real
    /// in-memory backup archive (sqlite → tar `databaseBackup` → LZ4-frame) so the
    /// FULL `read_backup_archive` path is exercised, then asserts the multi-block
    /// `gtrSeries` preset's row has a non-empty `graph.stages` (and `graph.nodes`).
    #[test]
    fn backup_row_carries_routed_graph() {
        // A known multi-block series preset doc (the live field-3 shape): two guitar
        // blocks in G1 under a `gtrSeries` template → extract_active_graph yields one
        // Series stage with both nodes.
        let preset_json = r#"{"info":{"displayName":"Copy Test","userSlot":3},"audioGraph":{"template":"gtrSeries","guitarNodes":{"G1":[{"nodeId":"ACD_Comp","FenderId":"ACD_Comp","dspUnitParameters":{"bypass":false}},{"nodeId":"ACD_TwinReverb","FenderId":"ACD_TwinReverb","dspUnitParameters":{"bypass":false}}]}},"scenes":[{"sceneName":"Rhythm","uuid":"a"}]}"#;

        let archive = build_backup_archive(&format!(
            "CREATE TABLE UserPresets(slot INTEGER, displayName TEXT, presetJson TEXT); \
             INSERT INTO UserPresets VALUES (3, 'Copy Test', '{}');",
            preset_json.replace('\'', "''")
        ));

        let result = read_backup_archive(&archive).expect("decode archive");
        let row = result
            .presets
            .iter()
            .find(|p| p.name == "Copy Test")
            .expect("the row is present");
        // Block roster survives (the pre-existing field) …
        assert_eq!(row.blocks.len(), 2, "two-block roster");
        // … AND the new routed graph carries the topology the frontend renders.
        assert_eq!(row.graph.template.as_deref(), Some("gtrSeries"));
        assert!(
            !row.graph.stages.is_empty(),
            "graph.stages must be non-empty"
        );
        assert!(!row.graph.nodes.is_empty(), "graph.nodes must be non-empty");
        let session::Stage::Series { blocks } = &row.graph.stages[0] else {
            panic!("gtrSeries → a Series stage");
        };
        assert_eq!(blocks.len(), 2, "both blocks in the series stage");
    }

    /// A backup archive carries the song→preset bindings out of the `SongPresets`
    /// table (Songs.slot → UserPresets.slot), so the Songs tab's Presets axis can show
    /// "which songs use this preset" with ZERO new device reads. Builds the full
    /// archive (sqlite → tar → LZ4) with `Songs` + `SongPresets` rows and asserts the
    /// decoded `song_presets` carry the expected `{song_slot, preset_slot}` pairs.
    #[test]
    fn backup_carries_song_preset_bindings() {
        // UserPresets (slot 8, 58) + two Songs + three SongPresets bindings:
        //   Song slot 1 → presets 8, 58 ; Song slot 2 → preset 58.
        let archive = build_backup_archive(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             INSERT INTO UserPresets VALUES (10, 8, 'Plexi Crunch', '{}'); \
             INSERT INTO UserPresets VALUES (11, 58, 'Stadium Lead', '{}'); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             INSERT INTO Songs VALUES (1, 1, 'Song A'); \
             INSERT INTO Songs VALUES (2, 2, 'Song B'); \
             CREATE TABLE SongPresets(id INTEGER PRIMARY KEY, Songs_id INTEGER, UserPresets_id INTEGER, slot INTEGER); \
             INSERT INTO SongPresets VALUES (1, 1, 10, 1); \
             INSERT INTO SongPresets VALUES (2, 1, 11, 2); \
             INSERT INTO SongPresets VALUES (3, 2, 11, 1);",
        );

        let result = read_backup_archive(&archive).expect("decode archive");
        assert_eq!(
            result.song_presets,
            vec![
                SongPresetBinding {
                    song_slot: 1,
                    preset_slot: 8
                },
                SongPresetBinding {
                    song_slot: 1,
                    preset_slot: 58
                },
                SongPresetBinding {
                    song_slot: 2,
                    preset_slot: 58
                },
            ],
        );
    }

    /// The Songs tab is sourced from the startup backup: the archive carries the full
    /// `Songs` (name/notes/bpm) + `Setlists` (name) + `SetlistSongs` (membership) tables,
    /// so the tab can paint with ZERO live device reads. Builds the full archive (sqlite →
    /// tar → LZ4) and asserts the decoded `songs`/`setlists`/`setlist_songs`.
    #[test]
    fn backup_carries_songs_setlists_membership() {
        let archive = build_backup_archive(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT, notes TEXT, bpmActive INTEGER, bpm INTEGER); \
             INSERT INTO Songs VALUES (1, 1, 'Opener', 'capo 2', 1, 128); \
             INSERT INTO Songs VALUES (2, 2, 'Ballad', '', 0, 72); \
             CREATE TABLE Setlists(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             INSERT INTO Setlists VALUES (1, 1, 'Main Set'); \
             CREATE TABLE SetlistSongs(id INTEGER PRIMARY KEY, Setlists_id INTEGER, Songs_id INTEGER, slot INTEGER); \
             INSERT INTO SetlistSongs VALUES (1, 1, 2, 1); \
             INSERT INTO SetlistSongs VALUES (2, 1, 1, 2);",
        );
        let result = read_backup_archive(&archive).expect("decode archive");
        assert_eq!(
            result.songs,
            vec![
                session::SongRecord {
                    slot: 1,
                    name: "Opener".into(),
                    notes: "capo 2".into(),
                    bpm: 128,
                    bpm_active: true
                },
                session::SongRecord {
                    slot: 2,
                    name: "Ballad".into(),
                    notes: String::new(),
                    bpm: 72,
                    bpm_active: false
                },
            ],
        );
        assert_eq!(
            result.setlists,
            vec![session::SetlistRecord {
                slot: 1,
                name: "Main Set".into()
            }],
        );
        // Setlist 1 holds song slot 2 at position 1, song slot 1 at position 2.
        assert_eq!(
            result.setlist_songs,
            vec![
                BackupSetlistSong {
                    setlist_slot: 1,
                    song_slot: 2,
                    position: 1
                },
                BackupSetlistSong {
                    setlist_slot: 1,
                    song_slot: 1,
                    position: 2
                },
            ],
        );
    }

    /// GENERATOR (not a gate — `#[ignore]`): expand the curated, non-personal
    /// `e2e/fixtures/showcase/showcase.json` into a real device-backup archive
    /// (`showcase-fixture.bin`) the marketing-screenshot tour decodes through the SAME
    /// `read_backup_archive` path the app uses. Reuses the archive-building recipe of
    /// `backup_row_carries_routed_graph` / `backup_carries_song_preset_bindings`
    /// (sqlite → tar `databaseBackup` → LZ4-frame). Run with:
    ///   `cargo test --features e2e build_showcase_fixture -- --ignored`
    /// (also chained by `bun run screenshots`). Committed output, regenerated only when
    /// `showcase.json` changes.
    #[test]
    #[ignore = "generator: writes showcase-fixture.bin from showcase.json"]
    fn build_showcase_fixture() {
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/showcase/showcase.json"
        );
        let spec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(src).expect("read showcase.json"))
                .expect("parse showcase.json");

        // Expand one compact preset spec → a full `.preset` doc (the live field-3 shape).
        let build_preset_json = |p: &serde_json::Value| -> String {
            let slot = p["slot"].as_u64().expect("slot");
            let name = p["name"].as_str().expect("name");
            let template = p["template"].as_str().unwrap_or("gtrSeries");
            let mk_node = |fender: &str| {
                serde_json::json!({
                    "FenderId": fender,
                    "nodeId": fender,
                    "nodeType": "dspUnit",
                    "dspUnitParameters": { "bypass": false }
                })
            };
            // G1..G7 (+ M1..M4 empty) — empty for any group the spec omits.
            let mut guitar = serde_json::Map::new();
            for g in ["G1", "G2", "G3", "G4", "G5", "G6", "G7"] {
                let arr: Vec<_> = p["groups"][g]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str()).map(mk_node).collect())
                    .unwrap_or_default();
                guitar.insert(g.to_string(), serde_json::Value::Array(arr));
            }
            let mut mic = serde_json::Map::new();
            for m in ["M1", "M2", "M3", "M4"] {
                mic.insert(m.to_string(), serde_json::json!([]));
            }
            // Named scenes (count + names drive the Level list's "N scenes" breakdown).
            let scenes: Vec<_> = p["scenes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .enumerate()
                        .map(|(i, s)| {
                            serde_json::json!({
                                "sceneName": s,
                                "uuid": format!("5cffe000-0000-0000-0000-{:012}", slot * 100 + i as u64),
                                "guitarNodes": {}
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let doc = serde_json::json!({
                "audioGraph": {
                    "guitarNodes": guitar,
                    "micNodes": mic,
                    "presetLevel": 0.5,
                    "template": template
                },
                "scenes": scenes,
                "info": { "displayName": name, "userSlot": slot, "version": "5.0" }
            });
            doc.to_string()
        };

        // Build the SQL: UserPresets (id = slot) + Songs (id = ordinal) + SongPresets.
        let mut sql = String::from(
            "CREATE TABLE UserPresets(id INTEGER PRIMARY KEY, slot INTEGER, displayName TEXT, presetJson TEXT); \
             CREATE TABLE Songs(id INTEGER PRIMARY KEY, slot INTEGER, name TEXT); \
             CREATE TABLE SongPresets(id INTEGER PRIMARY KEY, Songs_id INTEGER, UserPresets_id INTEGER, slot INTEGER);",
        );
        for p in spec["presets"].as_array().expect("presets") {
            let slot = p["slot"].as_u64().expect("slot");
            let name = p["name"].as_str().expect("name").replace('\'', "''");
            let json = build_preset_json(p).replace('\'', "''");
            sql.push_str(&format!(
                " INSERT INTO UserPresets VALUES ({slot}, {slot}, '{name}', '{json}');"
            ));
        }
        let mut sp_id = 0u64;
        for (si, song) in spec["songs"].as_array().expect("songs").iter().enumerate() {
            let song_id = si as u64 + 1; // Songs.id; slot = positional (1-based)
            let name = song["name"]
                .as_str()
                .expect("song name")
                .replace('\'', "''");
            sql.push_str(&format!(
                " INSERT INTO Songs VALUES ({song_id}, {song_id}, '{name}');"
            ));
            for (pi, ps) in song["presets"].as_array().into_iter().flatten().enumerate() {
                sp_id += 1;
                let preset_slot = ps.as_u64().expect("preset slot"); // = UserPresets.id
                let ord = pi as u64 + 1;
                sql.push_str(&format!(
                    " INSERT INTO SongPresets VALUES ({sp_id}, {song_id}, {preset_slot}, {ord});"
                ));
            }
        }

        // sqlite → tar `databaseBackup` → LZ4-frame, the exact device-backup shape.
        let archive = build_backup_archive(&sql);

        // Self-check: the archive round-trips through the real decoder before we commit it.
        let decoded = read_backup_archive(&archive).expect("decode generated archive");
        let n_presets = spec["presets"].as_array().unwrap().len();
        assert_eq!(decoded.presets.len(), n_presets, "all presets decode");
        let active = spec["activeSlot"].as_u64().unwrap();
        let hero = decoded
            .presets
            .iter()
            .find(|r| r.slot as u64 == active)
            .expect("active preset present");
        assert!(
            !hero.graph.stages.is_empty(),
            "hero graph has routed stages"
        );

        let out = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../e2e/fixtures/showcase/showcase-fixture.bin"
        );
        std::fs::write(out, &archive).expect("write showcase-fixture.bin");
        eprintln!(
            "build_showcase_fixture: wrote {} bytes ({} presets, {} songs) → {out}",
            archive.len(),
            n_presets,
            spec["songs"].as_array().unwrap().len(),
        );
    }

    /// The exact JSON the "Copy blocks between presets" frontend sends for a
    /// `copy_apply` job deserializes into the [`CopyJob`]/[`CopyOp`]/[`CopyRepl`]
    /// shapes — replace (all three repl variants), insert (append + before-anchor), remove.
    #[test]
    fn copy_job_round_trips_from_frontend_json() {
        let json = r#"{
            "listIndex": 7,
            "name": "Lead Tone",
            "ops": [
                { "kind": "replace", "group": "G1", "nodeId": "ACD_TwinReverb",
                  "repl": { "kind": "model", "fenderId": "ACD_HiwattDR103CanMod" } },
                { "kind": "replace", "group": "M1", "nodeId": "ACD_CabSimTMS",
                  "repl": { "kind": "saved", "fenderId": "ACD_CabSimTMS", "index": 3 } },
                { "kind": "insert", "group": "G1", "beforeFenderId": "ACD_Comp",
                  "repl": { "kind": "ir", "fenderId": "ACD_UserIRTMS", "file": "Oversize.wav" } },
                { "kind": "insert", "group": "G2", "beforeFenderId": null,
                  "repl": { "kind": "model", "fenderId": "ACD_Klon" } },
                { "kind": "remove", "group": "G1", "nodeId": "ACD_Comp" }
            ]
        }"#;
        let job: CopyJob = serde_json::from_str(json).expect("CopyJob deserializes");
        assert_eq!(job.list_index, 7);
        assert_eq!(job.name, "Lead Tone");
        assert_eq!(job.ops.len(), 5);

        // op0: replace → model
        let CopyOp::Replace {
            group,
            node_id,
            repl,
        } = &job.ops[0]
        else {
            panic!("replace")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_TwinReverb");
        let CopyRepl::Model { fender_id } = repl else {
            panic!("model repl")
        };
        assert_eq!(fender_id, "ACD_HiwattDR103CanMod");

        // op1: replace → saved (index)
        let CopyOp::Replace { repl, .. } = &job.ops[1] else {
            panic!("replace")
        };
        let CopyRepl::Saved { fender_id, index } = repl else {
            panic!("saved repl")
        };
        assert_eq!(fender_id, "ACD_CabSimTMS");
        assert_eq!(*index, 3);

        // op2: insert BEFORE a FenderId → ir (file)
        let CopyOp::Insert {
            group,
            before_fender_id,
            repl,
        } = &job.ops[2]
        else {
            panic!("insert")
        };
        assert_eq!(group, "G1");
        assert_eq!(before_fender_id.as_deref(), Some("ACD_Comp"));
        let CopyRepl::Ir { fender_id, file } = repl else {
            panic!("ir repl")
        };
        assert_eq!(fender_id, "ACD_UserIRTMS");
        assert_eq!(file, "Oversize.wav");
        // The IR's insert id resolves to the placeholder, not the catalog id.
        assert_eq!(repl.insert_fender_id(), "ACD_UserIRTMS");

        // op3: insert APPEND (before = None) → model
        let CopyOp::Insert {
            before_fender_id,
            repl,
            ..
        } = &job.ops[3]
        else {
            panic!("insert")
        };
        assert!(before_fender_id.is_none(), "null beforeFenderId → append");
        assert_eq!(repl.insert_fender_id(), "ACD_Klon");

        // op4: remove
        let CopyOp::Remove { group, node_id } = &job.ops[4] else {
            panic!("remove")
        };
        assert_eq!(group, "G1");
        assert_eq!(node_id, "ACD_Comp");
    }

    /// A `copy_apply` Channel item carries the `BulkReplaceItem` fields plus an OPTIONAL
    /// `graph` that is OMITTED from the wire when `None` (`skip_serializing_if`), so the
    /// no-graph case mirrors `BulkReplaceItem` exactly and the TS `graph?` field stays
    /// optional.
    #[test]
    fn copy_apply_item_serializes_like_bulk_replace_item() {
        let item = CopyApplyItem {
            slot: 7,
            name: "Lead Tone".to_string(),
            outcome: "updated".to_string(),
            detail: "5 op(s)".to_string(),
            graph: None,
        };
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["slot"], 7);
        assert_eq!(v["name"], "Lead Tone");
        assert_eq!(v["outcome"], "updated");
        assert_eq!(v["detail"], "5 op(s)");
        assert!(
            v.get("graph").is_none(),
            "None graph is omitted from the wire"
        );
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn wav_bytes_has_riff_header_and_round_trips() {
        let samples = [0.0f32, 0.5, -0.5, 1.0];
        let bytes = wav_bytes(&samples, 48000).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // Decodes back to the same samples via hound.
        let mut rdr = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let got: Vec<f32> = rdr.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(got, samples);
    }

    #[test]
    fn amp_output_level_param_is_output_level_only() {
        assert!(is_amp_output_level_param("outputLevel"));
        assert!(!is_amp_output_level_param("output"));
        assert!(!is_amp_output_level_param("outputlevel"));
        assert!(!is_amp_output_level_param("level"));
        assert!(!is_amp_output_level_param("brightvolume"));
        assert!(!is_amp_output_level_param("mastervolume"));
        assert!(!is_amp_output_level_param("normalvolume"));
        assert!(!is_amp_output_level_param("volume"));
    }

    #[test]
    fn amp_model_id_matches_merged_cab_ir_variant() {
        // Bare amp bid (separate cab block).
        assert!(is_amp_model_id("ACD_HiwattDR103CanMod"));
        // Amp+cab combo block carries a merged "CabIR" suffix the catalog bid lacks
        // → stripped to the bare bid.
        assert!(is_amp_model_id("ACD_HiwattDR103CanModCabIR"));
        // Reverb amps are catalogued WITH the suffix → must match directly (check-first),
        // NOT be over-stripped to a non-existent bare bid.
        assert!(is_amp_model_id("ACD_PrincetonReverb68CabIRConvRvb"));
        // Wet amp id whose base is catalogued ONLY with the NoFx token: strips to the
        // bare id (which misses), then the +NoFx bridge matches …BlondeVibratoNoFx.
        assert!(is_amp_model_id(
            "ACD_DeluxeReverb65BlondeVibratoCabIRConvRvb"
        ));
        // A non-amp block is still rejected (and +NoFx must not conjure a false match).
        assert!(!is_amp_model_id("ACD_TMReverse"));
    }

    #[test]
    fn scene_jobs_prefer_active_amp_output_level_over_preamp_volume() {
        let doc = serde_json::json!({
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                {
                    "nodeId": "ACD_HiwattDR103CanMod",
                    "FenderId": "ACD_HiwattDR103CanMod",
                    "dspUnitParameters": {
                        "bypass": false,
                        "brightvolume": 0.5,
                        "outputLevel": 1.0
                    }
                }
            ] } }
        });
        let candidates = vec![
            LevelBlockArg {
                group_id: "G1".to_string(),
                node_id: "ACD_HiwattDR103CanMod".to_string(),
                parameter_id: "brightvolume".to_string(),
                value: 0.5,
            },
            LevelBlockArg {
                group_id: "G1".to_string(),
                node_id: "ACD_HiwattDR103CanMod".to_string(),
                parameter_id: "outputLevel".to_string(),
                value: 0.34,
            },
        ];

        let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
        let leveller::LevelKnob::Block { parameter_id, .. } = &jobs[0].knobs[0].knob else {
            panic!("expected block knob");
        };
        assert_eq!(parameter_id, "outputLevel");
        assert_eq!(jobs[0].knobs[0].current, 1.0);
    }

    #[test]
    fn scene_jobs_reject_preamp_volume_as_level_control() {
        let doc = serde_json::json!({
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                {
                    "nodeId": "ACD_HiwattDR103CanMod",
                    "FenderId": "ACD_HiwattDR103CanMod",
                    "dspUnitParameters": {
                        "bypass": false,
                        "mastervolume": 1.0
                    }
                }
            ] } }
        });
        let candidates = vec![LevelBlockArg {
            group_id: "G1".to_string(),
            node_id: "ACD_HiwattDR103CanMod".to_string(),
            parameter_id: "mastervolume".to_string(),
            value: 1.0,
        }];

        // The Hiwatt is an active amp but its only candidate is a preamp volume, not
        // outputLevel → the scene is skipped with a reason, not leveled on the wrong knob.
        let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap_err();
        assert!(err.contains("outputLevel"), "got: {err}");
    }

    // Parallel-merged (gtrParallel1): an amp in each split lane (G2 | G3), no post-merge
    // amp → BOTH amps become the joint-k knob set (not just the first).
    #[test]
    fn scene_jobs_parallel_merged_picks_both_lane_amps() {
        let amp = |fid: &str| {
            serde_json::json!({
                "nodeId": fid, "FenderId": fid,
                "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
            })
        };
        let doc = serde_json::json!({
            "audioGraph": { "template": "gtrParallel1", "guitarNodes": {
                "G1": [],
                "G2": [ amp("ACD_TM59Bassman") ],
                "G3": [ amp("ACD_HiwattDR103CanMod") ]
            } }
        });
        let candidates = vec![
            LevelBlockArg {
                group_id: "G2".into(),
                node_id: "ACD_TM59Bassman".into(),
                parameter_id: "outputLevel".into(),
                value: 0.5,
            },
            LevelBlockArg {
                group_id: "G3".into(),
                node_id: "ACD_HiwattDR103CanMod".into(),
                parameter_id: "outputLevel".into(),
                value: 0.5,
            },
        ];
        let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
        assert_eq!(
            jobs[0].knobs.len(),
            2,
            "both lane amps drive together (joint-k)"
        );
        let groups: std::collections::HashSet<_> = jobs[0]
            .knobs
            .iter()
            .map(|kt| match &kt.knob {
                leveller::LevelKnob::Block { group_id, .. } => group_id.clone(),
                _ => panic!("block knob"),
            })
            .collect();
        assert!(groups.contains("G2") && groups.contains("G3"));
    }

    // gtrParallel1 with a post-merge amp (G4, after the G2|G3 split) → that single amp is
    // the series master, NOT a 2-knob joint-k.
    #[test]
    fn scene_jobs_post_merge_amp_is_single_master() {
        let amp = |fid: &str| {
            serde_json::json!({
                "nodeId": fid, "FenderId": fid,
                "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
            })
        };
        let doc = serde_json::json!({
            "audioGraph": { "template": "gtrParallel1", "guitarNodes": {
                "G1": [], "G2": [], "G3": [],
                "G4": [ amp("ACD_HiwattDR103CanMod") ]
            } }
        });
        let candidates = vec![LevelBlockArg {
            group_id: "G4".into(),
            node_id: "ACD_HiwattDR103CanMod".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }];
        let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
        assert_eq!(jobs[0].knobs.len(), 1);
    }

    // No known template (truncated read) → skip with a reason, NEVER a silent
    // single-amp series fallback.
    #[test]
    fn scene_jobs_skip_when_template_unknown() {
        let doc = serde_json::json!({
            "audioGraph": { "guitarNodes": { "G1": [
                { "nodeId": "ACD_TwinReverb", "FenderId": "ACD_TwinReverb",
                  "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
            ] } }
        });
        let candidates = vec![LevelBlockArg {
            group_id: "G1".into(),
            node_id: "ACD_TwinReverb".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }];
        let err = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap_err();
        assert!(err.contains("routing"), "got: {err}");
    }

    // Mic-only routing has no guitar amp the instrument re-amp can drive → the scene is
    // SKIPPED (per-scene, not a hard error); we level only what reaches USB 1/2.
    #[test]
    fn scene_jobs_skip_mic_only_no_guitar_amp() {
        let doc = serde_json::json!({
            "audioGraph": { "template": "micSeries", "guitarNodes": { "G1": [] },
                "micNodes": { "M1": [
                    { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
                      "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
                ] } }
        });
        let candidates = vec![LevelBlockArg {
            group_id: "M1".into(),
            node_id: "ACD_HiwattDR103CanMod".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }];
        let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
        assert!(
            jobs[0].skip.as_deref().unwrap_or("").contains("guitar amp"),
            "got: {:?}",
            jobs[0].skip
        );
    }

    // Split-output (gtrSplit): an amp in each output lane (OUT 1 / OUT 2) → both join the
    // joint-k set, measured at USB 1/2. No routing read; the user controls what's on USB.
    #[test]
    fn scene_jobs_split_output_joint_ks_both_output_lanes() {
        let amp = |fid: &str| {
            serde_json::json!({
                "nodeId": fid, "FenderId": fid,
                "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 }
            })
        };
        // gtrSplit: stages=[Series{G1}], outputs={a: G2-G4, b: G5-G7}.
        let doc = serde_json::json!({
            "audioGraph": { "template": "gtrSplit", "guitarNodes": {
                "G1": [], "G2": [ amp("ACD_TM59Bassman") ], "G3": [], "G4": [],
                "G5": [ amp("ACD_HiwattDR103CanMod") ], "G6": [], "G7": []
            } }
        });
        let candidates = vec![
            LevelBlockArg {
                group_id: "G2".into(),
                node_id: "ACD_TM59Bassman".into(),
                parameter_id: "outputLevel".into(),
                value: 0.5,
            },
            LevelBlockArg {
                group_id: "G5".into(),
                node_id: "ACD_HiwattDR103CanMod".into(),
                parameter_id: "outputLevel".into(),
                value: 0.5,
            },
        ];
        let jobs = build_scene_jobs(&[7], &candidates, &[(7, Some(doc))]).unwrap();
        assert_eq!(
            jobs[0].knobs.len(),
            2,
            "both output-lane amps drive together"
        );
    }

    // A per-SCENE issue (this scene bypasses its only amp) becomes a SKIP job, NOT a hard
    // error — one bad scene must not abort the batch (the runner reports it skipped).
    #[test]
    fn scene_jobs_per_scene_skip_does_not_abort() {
        let bypassed = serde_json::json!({
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
                  "dspUnitParameters": { "bypass": true, "outputLevel": 0.5 } }
            ] } }
        });
        let active = serde_json::json!({
            "audioGraph": { "template": "gtrSeries", "guitarNodes": { "G1": [
                { "nodeId": "ACD_HiwattDR103CanMod", "FenderId": "ACD_HiwattDR103CanMod",
                  "dspUnitParameters": { "bypass": false, "outputLevel": 0.5 } }
            ] } }
        });
        let candidates = vec![LevelBlockArg {
            group_id: "G1".into(),
            node_id: "ACD_HiwattDR103CanMod".into(),
            parameter_id: "outputLevel".into(),
            value: 0.5,
        }];
        let jobs = build_scene_jobs(
            &[0, 1],
            &candidates,
            &[(0, Some(bypassed)), (1, Some(active))],
        )
        .unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs[0].skip.is_some(), "bypassed-amp scene is skipped");
        assert!(jobs[0].knobs.is_empty());
        assert!(jobs[1].skip.is_none(), "active-amp scene levels normally");
        assert_eq!(jobs[1].knobs.len(), 1);
    }
}

/// End-to-end tests of the held-session Copy/Level orchestration over the in-memory
/// `sim_device::SimDevice` fake — NO hardware. These exercise the real command-layer
/// state machine (`copy_apply_one`'s load → re-arm → confirm-gated edits → identity-
/// preserving save) and the two safety invariants that previously only ever ran on the
/// device: a `presetError` is NEVER followed by a save, and the cold first edit's silent
/// drop is retried. The pure edit→op diff is unit-tested elsewhere (`copyModel` in
/// Vitest, `bulk_cmd` in Rust); this is the wire-driving the diff feeds.
#[cfg(test)]
mod copy_level_e2e_tests {
    use super::*;
    use crate::session::Session;
    use crate::sim_device::{SimDevice, SimEvent};

    fn model_replace(group: &str, node: &str, fender: &str) -> CopyOp {
        CopyOp::Replace {
            group: group.into(),
            node_id: node.into(),
            repl: CopyRepl::Model {
                fender_id: fender.into(),
            },
        }
    }

    fn one_replace_job(slot: u32, name: &str) -> CopyJob {
        CopyJob {
            list_index: slot,
            name: name.into(),
            ops: vec![model_replace("G1", "n2", "ACD_DeluxeReverb65")],
        }
    }

    /// Drive `copy_apply_one` over a configured fake device, returning the outcome item
    /// plus the ordered device actions the fake recorded.
    fn run_copy(sim: SimDevice, job: &CopyJob, save: bool) -> (CopyApplyItem, Vec<SimEvent>) {
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let item = copy_apply_one(&mut s, job, save).unwrap();
        (item, sim.events())
    }

    #[test]
    fn copy_replace_happy_path_loads_confirms_renames_and_saves_in_order() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(5),
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_DeluxeReverb65".into(),
                },
                SimEvent::Renamed("Stadium Lead".into()),
                SimEvent::Saved(5),
            ]
        );
    }

    #[test]
    fn copy_multi_op_applies_remove_replace_insert_in_order_then_saves() {
        let job = CopyJob {
            list_index: 3,
            name: "Clean Verse".into(),
            ops: vec![
                CopyOp::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                model_replace("G1", "n2", "ACD_Klon"),
                CopyOp::Insert {
                    group: "G1".into(),
                    before_fender_id: Some("ACD_Klon".into()),
                    repl: CopyRepl::Model {
                        fender_id: "ACD_TapeEcho".into(),
                    },
                },
            ],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "updated");
        assert_eq!(
            ev,
            vec![
                SimEvent::Loaded(3),
                SimEvent::Remove {
                    group: "G1".into(),
                    node_id: "n1".into(),
                },
                SimEvent::Replace {
                    group: "G1".into(),
                    node_id: "n2".into(),
                    fender_id: "ACD_Klon".into(),
                },
                SimEvent::Insert {
                    group: "G1".into(),
                    before: Some("ACD_Klon".into()),
                    fender_id: "ACD_TapeEcho".into(),
                },
                SimEvent::Renamed("Clean Verse".into()),
                SimEvent::Saved(3),
            ]
        );
    }

    #[test]
    fn copy_preset_error_is_never_followed_by_a_save() {
        // The device REJECTS the edit (presetError 53) — copy must report an error and
        // MUST NOT rename or save (an unconfirmed save corrupted a real preset).
        let (item, ev) = run_copy(
            SimDevice::new().with_reject_at(1),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "error");
        assert!(item.detail.contains("NOT saved"), "detail: {}", item.detail);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "a rejected edit must not save/rename: {ev:?}"
        );
    }

    #[test]
    fn copy_cold_first_edit_silent_drop_is_retried_then_saved() {
        // The first structural edit after a fresh load is silently DROPPED; the held
        // path retries it once and then confirms + saves.
        let (item, ev) = run_copy(
            SimDevice::new().with_drop_first(),
            &one_replace_job(5, "Stadium Lead"),
            true,
        );
        assert_eq!(item.outcome, "updated");
        let replaces = ev
            .iter()
            .filter(|e| matches!(e, SimEvent::Replace { .. }))
            .count();
        assert_eq!(replaces, 2, "the dropped edit + its retry: {ev:?}");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn copy_dry_run_applies_edits_but_does_not_save() {
        let (item, ev) = run_copy(SimDevice::new(), &one_replace_job(5, "Stadium Lead"), false);
        assert_eq!(item.outcome, "updated");
        assert!(ev.iter().any(|e| matches!(e, SimEvent::Replace { .. })));
        assert!(
            !ev.iter()
                .any(|e| matches!(e, SimEvent::Saved(_) | SimEvent::Renamed(_))),
            "dry run must not persist: {ev:?}"
        );
    }

    #[test]
    fn copy_empty_op_list_is_skipped_without_touching_the_device() {
        let job = CopyJob {
            list_index: 2,
            name: "Untouched".into(),
            ops: vec![],
        };
        let (item, ev) = run_copy(SimDevice::new(), &job, true);
        assert_eq!(item.outcome, "skipped");
        assert!(ev.is_empty(), "no ops → no device traffic");
    }

    #[test]
    fn level_setter_roundtrip_echoes_and_records() {
        // The Level seam over the fake: set_preset_level's wire encoding round-trips and
        // the device's PresetLevelChanged(77) echo parses back to the value sent.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let echo = s.set_preset_level(0.5).unwrap();
        assert!((echo.expect("level echo") - 0.5).abs() < 1e-6);
        s.save_current_preset(7).unwrap();
        assert_eq!(
            sim.events(),
            vec![SimEvent::PresetLevel(0.5), SimEvent::Saved(7)]
        );
    }

    #[test]
    fn copy_partial_failure_saves_only_the_confirmed_targets() {
        // The multi-target copy loop runs every job on ONE held session. With the SECOND
        // target's edit rejected (presetError), the FIRST must still confirm + save and the
        // rejected one must NOT save — a batch is partial-success, never all-or-nothing, and
        // a rejected target never gets a wrong-content save. (The frontend then patches the
        // cache for the "updated" target only — CopyView.tsx's `outcome === "updated"` gate.)
        let sim = SimDevice::new().with_reject_at(2);
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let a = copy_apply_one(&mut s, &one_replace_job(5, "Target A"), true).unwrap();
        let b = copy_apply_one(&mut s, &one_replace_job(6, "Target B"), true).unwrap();
        assert_eq!(a.outcome, "updated");
        assert_eq!(b.outcome, "error");
        let ev = sim.events();
        assert!(
            ev.contains(&SimEvent::Saved(5)),
            "confirmed target A must save: {ev:?}"
        );
        assert!(
            !ev.contains(&SimEvent::Saved(6)),
            "rejected target B must NOT save: {ev:?}"
        );
    }

    #[test]
    fn copy_apply_refuses_an_over_cap_insert_end_to_end() {
        // The device does NOT enforce the 5 firmware block-count caps (C-1, see
        // `blockcaps.rs`'s module docs) — this is the guard's REAL test: an over-cap
        // edit must be refused HERE, before it ever reaches the device, not by any
        // device response (the fake would happily confirm it, like the real firmware).
        let sim = SimDevice::new().with_preset_json(
            r#"{"audioGraph":{"guitarNodes":{"G1":[
                {"FenderId":"ACD_AC30BrilliantCabIR","nodeId":"n1"},
                {"FenderId":"ACD_AC30NormalCabIR","nodeId":"n2"}
            ]}}}"#,
        );
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let job = CopyJob {
            list_index: 5,
            name: "Stadium Lead".into(),
            ops: vec![CopyOp::Insert {
                group: "G1".into(),
                before_fender_id: None,
                repl: CopyRepl::Model {
                    fender_id: "ACD_Ampeg66B15CabIR".into(), // a 3rd cabinet member
                },
            }],
        };
        let item = copy_apply_one(&mut s, &job, true).unwrap();
        assert_eq!(item.outcome, "error");
        assert!(
            item.detail.contains("ComboHalfStackCabinetsLimit"),
            "detail: {}",
            item.detail
        );
        let ev = sim.events();
        assert!(
            !ev.iter().any(|e| matches!(
                e,
                SimEvent::Insert { .. } | SimEvent::Saved(_) | SimEvent::Renamed(_)
            )),
            "an over-cap insert must never reach the device, let alone save: {ev:?}"
        );
    }
    // ── PR2: confirm-before-save write safety (Session::confirm_active) ──

    #[test]
    fn confirm_active_ok_when_slot_echo_matches() {
        // A load the device echoed (PresetLoaded) confirms via the SLOT identity.
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(6, 1), 50)
            .unwrap(); // dev slot 6 = 0-based list index 5
        assert!(
            s.confirm_active(5, None).is_ok(),
            "slot echo should confirm: loaded={:?}",
            s.loaded_slot()
        );
        s.save_current_preset(5).unwrap();
        assert!(sim.events().iter().any(|e| matches!(e, SimEvent::Saved(5))));
    }

    #[test]
    fn confirm_active_errs_and_blocks_save_when_load_dropped() {
        // No PresetLoaded echo and no matching active name (a dropped load) ⇒ confirm
        // errs, and a caller using `?` never reaches the save (no wrong-content write).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        let attempt = |s: &mut Session| -> Result<(), String> {
            s.confirm_active(7, Some("Target"))?;
            s.save_current_preset(7)?; // MUST NOT run
            Ok(())
        };
        assert!(attempt(&mut s).is_err(), "unconfirmed load must not save");
        assert!(
            !sim.events().iter().any(|e| matches!(e, SimEvent::Saved(_))),
            "no save on an unconfirmed load: {:?}",
            sim.events()
        );
    }

    #[test]
    fn confirm_active_errs_when_a_different_preset_is_active() {
        // The device says a DIFFERENT slot is active — a possibly-duplicate name must not
        // override the contradicting slot echo, so confirm errs (never edit the wrong one).
        let sim = SimDevice::new();
        let mut s = Session::from_transport(Box::new(sim.clone()));
        s.send_and_collect(&crate::proto::load_preset(4, 1), 50)
            .unwrap(); // dev slot 4 = 0-based list index 3
        assert!(
            s.confirm_active(5, Some("Target")).is_err(),
            "slot 3 active but target 5 — must not confirm"
        );
    }

    // ── PR2: migration snapshot-before-write (AC2) ──

    #[test]
    fn migration_refuses_to_snapshot_into_an_unwritable_dir() {
        // A path whose parent is not a directory ⇒ the snapshot fails; migration_apply
        // then skips the write and keeps the preset revertible.
        let bad = std::path::Path::new("/dev/null/tmp-companion-cannot-mkdir");
        let r = super::snapshot_before_migrate(bad, 3, "Cliff", r#"{"info":{"preset_id":"x"}}"#);
        assert!(r.is_err(), "unwritable backup dir must refuse: {r:?}");
    }

    #[test]
    fn migration_snapshot_captures_the_pre_edit_json() {
        let dir = std::env::temp_dir().join(format!(
            "tmp-companion-migsnap-{}",
            crate::bulkrun::now_stamp()
        ));
        let before = r#"{"info":{"preset_id":"abc","displayName":"Cliff"}}"#;
        let p = super::snapshot_before_migrate(&dir, 3, "Cliff", before).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(
            content.contains("abc"),
            "snapshot must carry the pre-edit json: {content}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
