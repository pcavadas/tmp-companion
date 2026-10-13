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
        use std::time::Instant;

        let debug = std::env::var("TMP_BACKUP_DEBUG").is_ok();
        let mut debugged_first = false;

        // Establish PC's live-controller cadence, THEN request inside it. PC sends
        // a ~250 ms ConnectionHeartbeat continuously; the backup rides that stream.
        self.raw.clear();
        let _ = self.hid.send(&proto::heartbeat());
        let start = Instant::now();
        self.send_and_collect(&proto::backup_request(), BACKUP_REQUEST_MS)?;
        // The final-frame reassembler, fed one report at a time, so each completed
        // stream decodes as it closes and we never hold the whole frame set.
        let mut reassembler = proto::Reassembler::new(proto::SingleFrame::ClosesStream);
        let mut completed: Vec<proto::Stream> = Vec::new();
        let mut tally = BackupTally::new(start);
        let mut all_chunks_at: Option<Instant> = None;
        // First pass consumes the frames captured during the backup-send window;
        // subsequent passes pump fresh.
        let mut pending: Vec<Vec<u8>> = std::mem::take(&mut self.raw);

        loop {
            if tally.last_progress.elapsed().as_secs() >= max_secs {
                return Err(format!(
                    "device backup stalled for {max_secs}s: {}/{} chunks, last_state={}",
                    tally.chunks.len(),
                    tally.num_chunks,
                    tally.last_state
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
            completed.clear();
            for data in &reports {
                reassembler.push(data, &mut completed);
            }
            for stream in completed.drain(..) {
                let stream = stream.body;
                // TMS → backupMessage(8) → { backupRestoreData(2), backupRestoreState(3) }
                let top = proto::parse(&stream);
                let Some(bm) = proto::first_bytes(&top, 8) else {
                    continue;
                };
                if debug && !debugged_first {
                    debugged_first = true;
                    debug_first_backup_stream(&stream, &top, bm);
                }
                tally.absorb(&proto::parse(bm), &mut on_progress);
            }
            let all = tally.num_chunks > 0 && tally.chunks.len() as u32 >= tally.num_chunks;
            // BACKUP_COMPLETE | BACKUP_SUCCESS
            let terminal = matches!(tally.last_state, 4 | 10);
            // Terminal state = clean finish (the device's BackupManager has reset);
            // exit immediately so a later backup isn't ignored.
            if terminal && !tally.chunks.is_empty() {
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
            if !tally.chunks.is_empty() && tally.last_progress.elapsed().as_secs() >= 5 {
                break;
            }
        }

        Ok(tally.finish())
    }
}

/// `TMP_BACKUP_DEBUG`: dump the first backup stream's field layout to stderr.
fn debug_first_backup_stream(stream: &[u8], top: &[(u32, proto::Val)], bm: &[u8]) {
    let hexn = |b: &[u8], n: usize| -> String {
        b.iter()
            .take(n)
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let fields: Vec<u32> = top.iter().map(|(f, _)| *f).collect();
    eprintln!(
        "[backup-dbg] first stream: {}B, top fields {fields:?}, head {}",
        stream.len(),
        hexn(stream, 24)
    );
    let bmp = proto::parse(bm);
    if let Some(d) = proto::first_bytes(&bmp, 2) {
        let dp = proto::parse(d);
        let dfields: Vec<u32> = dp.iter().map(|(f, _)| *f).collect();
        eprintln!(
            "[backup-dbg]   backupRestoreData fields {dfields:?}; field6(chunkData) head {}",
            proto::first_bytes(&dp, 6)
                .map(|c| hexn(c, 24))
                .unwrap_or_else(|| "<none>".into())
        );
    }
}

/// Everything [`Session::device_backup`] learns from the stream: the chunks, the
/// device's declared totals, its state transitions, and the progress watchdog.
struct BackupTally {
    start: std::time::Instant,
    chunks: std::collections::BTreeMap<u32, Vec<u8>>,
    num_chunks: u32,
    num_bytes: u32,
    crc: u32,
    last_state: i64,
    state_log: Vec<(f64, i64)>,
    build_size: u32,
    build_ticks: u32,
    first_chunk_secs: f64,
    assembled_bytes: u64,
    last_progress: std::time::Instant,
}

impl BackupTally {
    fn new(start: std::time::Instant) -> Self {
        Self {
            start,
            chunks: std::collections::BTreeMap::new(),
            num_chunks: 0,
            num_bytes: 0,
            crc: 0,
            last_state: -1,
            state_log: Vec::new(),
            build_size: 0,
            build_ticks: 0,
            first_chunk_secs: 0.0,
            assembled_bytes: 0,
            last_progress: std::time::Instant::now(),
        }
    }

    /// One decoded `backupMessage`: its state (field 3), then its data (field 2).
    fn absorb(&mut self, bm: &[(u32, proto::Val)], on_progress: &mut impl FnMut(BackupProgress)) {
        if let Some(st) = proto::first_bytes(bm, 3) {
            self.absorb_state(&proto::parse(st), on_progress);
        }
        if let Some(d) = proto::first_bytes(bm, 2) {
            self.absorb_data(&proto::parse(d), on_progress);
        }
    }

    /// `BackupRestoreState`: build progress (fields 2/3) and the state (field 1).
    fn absorb_state(
        &mut self,
        stp: &[(u32, proto::Val)],
        on_progress: &mut impl FnMut(BackupProgress),
    ) {
        if let Some(sz) = proto::first_varint(stp, 2) {
            self.build_size = self.build_size.max(sz as u32);
        }
        if let Some(tk) = proto::first_varint(stp, 3) {
            if tk as u32 > self.build_ticks {
                self.build_ticks = tk as u32;
                self.last_progress = std::time::Instant::now();
            }
        }
        let Some(s) = proto::first_varint(stp, 1) else {
            return;
        };
        let s = s as i64;
        if s == self.last_state {
            return;
        }
        self.state_log.push((self.start.elapsed().as_secs_f64(), s));
        self.last_state = s;
        self.last_progress = std::time::Instant::now();
        // Pre-stream "building" progress (determinate iff the
        // device populates build_size).
        if self.chunks.is_empty() && matches!(s, 1 | 2) {
            let pct = if self.build_size > 0 {
                (self.build_ticks as f32 / self.build_size as f32 * 100.0).min(100.0)
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
                build_size: self.build_size,
                build_ticks: self.build_ticks,
            });
        }
    }

    /// `BackupRestoreData`: the declared totals (fields 2–4) and one chunk (5/6).
    fn absorb_data(
        &mut self,
        d: &[(u32, proto::Val)],
        on_progress: &mut impl FnMut(BackupProgress),
    ) {
        if let Some(v) = proto::first_varint(d, 2) {
            self.crc = v as u32;
        }
        if let Some(v) = proto::first_varint(d, 3) {
            self.num_bytes = v as u32;
        }
        if let Some(v) = proto::first_varint(d, 4) {
            self.num_chunks = v as u32;
        }
        let cn = proto::first_varint(d, 5).map(|v| v as u32);
        let cd = proto::first_bytes(d, 6).map(|b| b.to_vec());
        let (Some(cn), Some(cd)) = (cn, cd) else {
            return;
        };
        if self.chunks.is_empty() {
            self.first_chunk_secs = self.start.elapsed().as_secs_f64();
        }
        // Incremental byte tally (avoids re-summing the whole map per
        // chunk); only a NEW chunkNum advances it / resets the watchdog.
        let cd_len = cd.len() as u64;
        let is_new = !self.chunks.contains_key(&cn);
        self.chunks.entry(cn).or_insert(cd); // keep-first on a dup chunkNum
        if is_new {
            self.assembled_bytes += cd_len;
            self.last_progress = std::time::Instant::now();
        }
        let pct = if self.num_chunks > 0 {
            (self.chunks.len() as f32 / self.num_chunks as f32 * 100.0).min(100.0)
        } else {
            0.0
        };
        on_progress(BackupProgress {
            phase: "streaming",
            received: self.chunks.len() as u32,
            total: self.num_chunks,
            bytes: self.assembled_bytes,
            total_bytes: self.num_bytes as u64,
            percent: pct,
            build_size: self.build_size,
            build_ticks: self.build_ticks,
        });
    }

    /// The archive (chunks in `chunkNum` order) and the run's stats.
    fn finish(self) -> (Vec<u8>, BackupStats) {
        let mut blob = Vec::with_capacity(self.num_bytes as usize);
        for cd in self.chunks.values() {
            blob.extend_from_slice(cd);
        }
        let stats = BackupStats {
            elapsed_secs: self.start.elapsed().as_secs_f64(),
            num_bytes: self.num_bytes,
            num_chunks: self.num_chunks,
            chunks_received: self.chunks.len() as u32,
            crc: self.crc,
            bytes_assembled: blob.len(),
            first_chunk_secs: self.first_chunk_secs,
            state_log: self.state_log,
            build_size: self.build_size,
            build_ticks: self.build_ticks,
        };
        (blob, stats)
    }
}
