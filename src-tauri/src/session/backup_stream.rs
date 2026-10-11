//! The device backup stream (`device_backup`) and its progress and stats types. It runs
//! its own wall-clock loop. Transport set.

use super::*;

/// Timing + integrity metadata from a [`Session::device_backup`] run.
#[derive(Debug, Clone, Serialize)]
pub struct BackupStats {
    /// Wall-clock from request to last chunk.
    pub elapsed_secs: f64,
    /// `numBytes` the device declared (uncompressed-on-wire archive size).
    pub num_bytes: u32,
    /// `numChunks` the device declared.
    pub num_chunks: u32,
    /// Distinct `chunkNum`s actually received.
    pub chunks_received: u32,
    /// `crc` the device stamped on every chunk (over the whole archive).
    pub crc: u32,
    /// Bytes we reassembled (should equal `num_bytes` on a clean transfer).
    pub bytes_assembled: usize,
    /// Time to the first chunk (handshake + device-side archive build).
    pub first_chunk_secs: f64,
    /// `(elapsed_secs, BackupRestoreState.State)` transitions, in order.
    pub state_log: Vec<(f64, i64)>,
    /// Max `BackupRestoreState.progressSize` (field 2) seen — non-zero iff the
    /// device reports build-phase progress (lets the "Preparing…" phase be
    /// determinate too). 0 means the device doesn't populate it.
    pub build_size: u32,
    /// Max `BackupRestoreState.progressTicks` (field 3) seen.
    pub build_ticks: u32,
}

/// Live progress for a [`Session::device_backup`] run (drives a progress bar).
#[derive(Debug, Clone, Serialize)]
pub struct BackupProgress {
    /// `"building"` (device assembling the archive, before chunks) or
    /// `"streaming"` (chunks arriving — `percent` is exact).
    pub phase: &'static str,
    /// Chunks received so far.
    pub received: u32,
    /// Total chunks (`numChunks`, known from the first chunk; 0 while building).
    pub total: u32,
    /// Archive bytes assembled so far.
    pub bytes: u64,
    /// Total archive bytes (`numBytes`; 0 while building).
    pub total_bytes: u64,
    /// 0..100, exact during streaming (`received/total`); 0 while building unless
    /// the device reports `build_size`/`build_ticks`.
    pub percent: f32,
    /// `BackupRestoreState.progressSize` (device build progress; 0 if unreported).
    pub build_size: u32,
    /// `BackupRestoreState.progressTicks`.
    pub build_ticks: u32,
}

impl Session {
    /// Trigger and capture the device's bulk backup archive (`BackupMessage`).
    /// Returns the raw archive bytes (a GNU-tar + LZ4-frame blob — caller decodes)
    /// plus [`BackupStats`]. This is the ONLY bulk file-egress in the protocol and
    /// the fast path to the full preset/scene library: one request streams the whole
    /// `/data` store, vs ~500 per-preset round-trips.
    ///
    /// Read-only on the device: the archive is built in tmpfs and streamed; nothing
    /// is persisted on flash (only the `SdBackupRequest` variant writes a file).
    ///
    /// We MUST heartbeat (~250 ms `ConnectionHeartbeat`) throughout: Pro Control's
    /// captured backup runs inside its dense heartbeat stream (the request is just
    /// slotted between heartbeats), and without it the session lapses during the
    /// device's multi-second archive build/stream and the device drops the backup
    /// (observed: a heartbeat-less request gets zero response, not even
    /// `BACKUP_STARTED`). Heartbeat replies are tiny single `0x35` frames that land
    /// between chunks (current=None), which the reassembler emits standalone without
    /// disturbing an open chunk stream.
    ///
    /// `on_progress` is called as the transfer advances (a `BackupProgress` per
    /// device state-change and per chunk) so a caller can drive a progress bar; the
    /// chunk percentage is exact because `numChunks` is known from the first chunk.
    ///
    /// `max_secs` bounds the time WITHOUT a progress event (new chunk, state change,
    /// build tick), not the whole transfer: the stream runs at ~2.7 chunks/s, so a
    /// 230-chunk (2.2 MiB) library needs ~85 s and a whole-transfer cap of 60 s cut it
    /// at 163/230 (HW, 2026-09-02). Every refresh source is finite, so the stall timer
    /// alone bounds the loop.
    pub fn device_backup<F: FnMut(BackupProgress)>(
        &mut self,
        max_secs: u64,
        mut on_progress: F,
    ) -> Result<(Vec<u8>, BackupStats), String> {
        use std::collections::BTreeMap;
        use std::time::Instant;

        let debug = std::env::var("TMP_BACKUP_DEBUG").is_ok();
        let mut debugged_first = false;
        let hexn = |b: &[u8], n: usize| -> String {
            b.iter()
                .take(n)
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Establish PC's live-controller cadence, THEN request inside it. PC sends
        // a ~250 ms ConnectionHeartbeat continuously; the backup rides that stream.
        self.raw.clear();
        let _ = self.hid.send(&proto::heartbeat());
        let start = Instant::now();
        self.send_and_collect(&proto::backup_request(), BACKUP_REQUEST_MS)?;
        // Inline reassembler (mirrors proto::reassemble_streams_final, but decodes
        // each completed stream as it closes so we never hold the whole frame set).
        let mut current: Option<Vec<u8>> = None;
        let mut chunks: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        let mut num_chunks: u32 = 0;
        let mut num_bytes: u32 = 0;
        let mut crc: u32 = 0;
        let mut last_state: i64 = -1;
        let mut state_log: Vec<(f64, i64)> = Vec::new();
        let mut build_size: u32 = 0;
        let mut build_ticks: u32 = 0;
        let mut first_chunk_secs: f64 = 0.0;
        let mut assembled_bytes: u64 = 0;
        let mut last_progress = Instant::now();
        let mut all_chunks_at: Option<Instant> = None;
        // First pass consumes the frames captured during the backup-send window;
        // subsequent passes pump fresh.
        let mut pending: Vec<Vec<u8>> = std::mem::take(&mut self.raw);

        loop {
            if last_progress.elapsed().as_secs() >= max_secs {
                return Err(format!(
                    "device backup stalled for {max_secs}s: {}/{} chunks, last_state={last_state}",
                    chunks.len(),
                    num_chunks
                ));
            }
            // Keep the live-controller session alive (PC's ~250 ms cadence) or the
            // device aborts the backup mid-build/stream.
            if self.hid.silent_ms() >= 250 {
                let _ = self.hid.send(&proto::heartbeat());
            }
            let reports = if pending.is_empty() {
                self.hid.pump(120)?
            } else {
                std::mem::take(&mut pending)
            };
            for data in &reports {
                if data.len() < 4 || data[0] != 0x00 {
                    continue;
                }
                let magic = data[1];
                let body_len = data[3] as usize;
                let end = (4 + body_len).min(data.len());
                let body = &data[4..end];
                // 0x33 start · 0x34 continue · 0x35 final (proto framing).
                let completed: Option<Vec<u8>> = match magic {
                    0x33 => {
                        let prev = current.take();
                        current = Some(body.to_vec());
                        prev
                    }
                    0x34 => {
                        match current {
                            Some(ref mut c) => c.extend_from_slice(body),
                            None => current = Some(body.to_vec()),
                        }
                        None
                    }
                    0x35 => match current.take() {
                        Some(mut c) => {
                            c.extend_from_slice(body);
                            Some(c)
                        }
                        None => Some(body.to_vec()),
                    },
                    _ => None,
                };
                let Some(stream) = completed else { continue };
                // TMS → backupMessage(8) → { backupRestoreData(2), backupRestoreState(3) }
                let top = proto::parse(&stream);
                let Some(bm) = proto::first_bytes(&top, 8) else {
                    continue;
                };
                if debug && !debugged_first {
                    debugged_first = true;
                    let fields: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
                    eprintln!(
                        "[backup-dbg] first stream: {}B, top fields {fields:?}, head {}",
                        stream.len(),
                        hexn(&stream, 24)
                    );
                    let bmp = proto::parse(bm);
                    if let Some(d) = proto::first_bytes(&bmp, 2) {
                        let dp = proto::parse(d);
                        let dfields: Vec<u32> = dp.iter().map(|(f, _)| *f).collect();
                        eprintln!(
                            "[backup-dbg]   backupRestoreData fields {dfields:?}; field6(chunkData) head {}",
                            proto::first_bytes(&dp, 6).map(|c| hexn(c, 24)).unwrap_or_else(|| "<none>".into())
                        );
                    }
                }
                let bm = proto::parse(bm);
                if let Some(st) = proto::first_bytes(&bm, 3) {
                    let stp = proto::parse(st);
                    if let Some(sz) = proto::first_varint(&stp, 2) {
                        build_size = build_size.max(sz as u32);
                    }
                    if let Some(tk) = proto::first_varint(&stp, 3) {
                        if tk as u32 > build_ticks {
                            build_ticks = tk as u32;
                            last_progress = Instant::now();
                        }
                    }
                    if let Some(s) = proto::first_varint(&stp, 1) {
                        let s = s as i64;
                        if s != last_state {
                            state_log.push((start.elapsed().as_secs_f64(), s));
                            last_state = s;
                            last_progress = Instant::now();
                            // Pre-stream "building" progress (determinate iff the
                            // device populates build_size).
                            if chunks.is_empty() && matches!(s, 1 | 2) {
                                let pct = if build_size > 0 {
                                    (build_ticks as f32 / build_size as f32 * 100.0).min(100.0)
                                } else {
                                    0.0
                                };
                                on_progress(BackupProgress {
                                    phase: "building",
                                    received: 0,
                                    total: 0,
                                    bytes: 0,
                                    total_bytes: 0,
                                    percent: pct,
                                    build_size,
                                    build_ticks,
                                });
                            }
                        }
                    }
                }
                if let Some(d) = proto::first_bytes(&bm, 2) {
                    let d = proto::parse(d);
                    if let Some(v) = proto::first_varint(&d, 2) {
                        crc = v as u32;
                    }
                    if let Some(v) = proto::first_varint(&d, 3) {
                        num_bytes = v as u32;
                    }
                    if let Some(v) = proto::first_varint(&d, 4) {
                        num_chunks = v as u32;
                    }
                    let cn = proto::first_varint(&d, 5).map(|v| v as u32);
                    let cd = proto::first_bytes(&d, 6).map(|b| b.to_vec());
                    if let (Some(cn), Some(cd)) = (cn, cd) {
                        if chunks.is_empty() {
                            first_chunk_secs = start.elapsed().as_secs_f64();
                        }
                        // Incremental byte tally (avoids re-summing the whole map per
                        // chunk); only a NEW chunkNum advances it / resets the watchdog.
                        let cd_len = cd.len() as u64;
                        let is_new = !chunks.contains_key(&cn);
                        chunks.entry(cn).or_insert(cd); // keep-first on a dup chunkNum
                        if is_new {
                            assembled_bytes += cd_len;
                            last_progress = Instant::now();
                        }
                        let pct = if num_chunks > 0 {
                            (chunks.len() as f32 / num_chunks as f32 * 100.0).min(100.0)
                        } else {
                            0.0
                        };
                        on_progress(BackupProgress {
                            phase: "streaming",
                            received: chunks.len() as u32,
                            total: num_chunks,
                            bytes: assembled_bytes,
                            total_bytes: num_bytes as u64,
                            percent: pct,
                            build_size,
                            build_ticks,
                        });
                    }
                }
            }
            let all = num_chunks > 0 && chunks.len() as u32 >= num_chunks;
            let terminal = matches!(last_state, 4 | 10); // BACKUP_COMPLETE | BACKUP_SUCCESS
                                                         // Terminal state = clean finish (the device's BackupManager has reset);
                                                         // exit immediately so a later backup isn't ignored.
            if terminal && !chunks.is_empty() {
                break;
            }
            // All chunks in but no terminal state yet: keep draining briefly to let
            // BACKUP_COMPLETE land (a hard break here left the device non-idle and
            // the NEXT BackupRequest got no reply — HW-observed).
            if all {
                let waited = all_chunks_at.get_or_insert_with(Instant::now);
                if waited.elapsed().as_secs() >= 2 {
                    break;
                }
            }
            // Stall watchdog: stop if the stream goes silent after it began.
            if !chunks.is_empty() && last_progress.elapsed().as_secs() >= 5 {
                break;
            }
        }

        let mut blob = Vec::with_capacity(num_bytes as usize);
        for cd in chunks.values() {
            blob.extend_from_slice(cd);
        }
        let stats = BackupStats {
            elapsed_secs: start.elapsed().as_secs_f64(),
            num_bytes,
            num_chunks,
            chunks_received: chunks.len() as u32,
            crc,
            bytes_assembled: blob.len(),
            first_chunk_secs,
            state_log,
            build_size,
            build_ticks,
        };
        Ok((blob, stats))
    }
}
