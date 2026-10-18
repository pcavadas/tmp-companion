# E2E recording

Every online and soak run keeps its evidence in its own run folder instead of overwriting the last run's, and every process that opens the device can record the wire.
The recorder is `src-tauri/src/trace/` (its `//!` headers are the authority); the run folders belong to `scripts/e2e.sh` and `scripts/lib/e2e-evidence.sh`.

## Run folders

- `<root>` is `$(git rev-parse --git-common-dir)/tmp-evidence`, made absolute.
  It sits inside `.git`, so it is never tracked, never hashed into a gate stamp, shared by every worktree and kept when a worktree is removed.
- `TMP_E2E_EVIDENCE_DIR` overrides the root; an override inside the worktree must be gitignored, or the run refuses it.
- `<root>/runs/<yyyymmddThhmmssZ>-<mode>-<sha7>[-dirty]/` is one run.
- `MANIFEST.json` holds the run metadata: mode, specs, start and end, exit code, whether it validated and stamped, the tree key before and after, git sha, OS, ffmpeg, device firmware, `unit_config` (`stock` or `non-stock`), Scene Change Behavior, the fixture stamp, the traces (file, bytes, device sessions, drops) and the WAV policy.
- `e2e_server.log`, `seed.log`, `online-spec-<spec>.log` and `soak-run-<n>.log` are the lane's logs, one set per run; `$LOG_DIR/e2e_server.log` stays a symlink to the current server log.
- `level-validate-expectations.jsonl` and `level-validate-wavs/` hold the external-validation rows and their WAVs; `doctor-validate.jsonl` holds the Doctor validation rows.
- `trace/<process>-<pid>.tct.jsonl` is one trace per process that opened the device (the seed probe, the e2e server).
- `wav/<cap_id>.wav` is each capture's processed pair (`TMP_TRACE_WAV=pair`).
- `field8/<list_index>.json` is the canonical field-8 body of each fixture slot the seed read.
- `<root>/LAST_DEVICE_FW` is the firmware a device session last reported, and `<root>/latest` links to the newest run.

Everything except WAVs is kept; WAVs are kept for the newest `TMP_E2E_EVIDENCE_WAV_RUNS` runs (default 10).
A run stopped with SIGINT still gets its MANIFEST, after the device recovery.

## Arming the recorder

Nothing records unless `TMP_TRACE_DIR` is set: no thread, no allocation, no file.
The lane sets these for the seed probe and the e2e server, and any probe or script arms it the same way:

- `TMP_TRACE_DIR`: the run's `trace/` directory; `wav/` and `field8/` go next to it.
- `TMP_TRACE_PROCESS`: the process name in the trace file name (default: the executable's).
- `TMP_TRACE_RUN_ID`, `TMP_TRACE_GIT_SHA`, `TMP_TRACE_TREE_KEY`: copied into the header.
- `TMP_E2E_UNIT_CONFIG`: `stock` or `non-stock`, copied into the header.
- `TMP_TRACE_WAV=pair`: write each capture's processed pair as a WAV.
- `TMP_TRACE_FW_FILE`: where a device session's firmware version is written.

## TCT v1

A trace is JSON Lines, header first.
A cut last line is legal (the server is killed, not stopped), and readers drop it.
Every line starts with the same five fields in this order, and each event's own fields follow in a fixed order, so plain `grep` works:

- `seq`: per process, gap-free.
- `sid`: one per opened transport (`null` when no transport is open).
- `t_us`: monotonic microseconds since the recorder started.
- `t_nom_ms`: the nominal clock, the sum of pump windows, nominal settles and nominal capture lengths up to this event.
- `ev`: the event type.

Events:

- `header`: `tct` (1), `recorder`, `process`, `pid`, `os`, `wall_anchor_utc`, `run_id`, `git_sha`, `tree_key`, `fixture_stamp`, `unit_config`.
  `wall_anchor_utc` is the wall time at `t_us` 0.
- `open`: `backend`, `ok`, `err`, `ms`.
  `backend` is `device` when the real device was opened, `sim` for the e2e factory's fake, and `remote` for the `remote` tier's `RemoteHid` (e2e builds only); it comes from the branch that ran, never from the environment.
  A failed open is recorded too.
- `out`: `call`, `pump_ms`, `body`, `k`.
  `call` is `send`, `transact`, `transact_chunked`, `pump` or `transact_eager`.
  `body` is the full hex body (`null` for `pump`).
  `k` is `<tms>.<inner>`, the first two field numbers.
- `in`: `of`, `t_end_us`, `reports`, `k`, `err`.
  `of` is the `seq` of the matching `out`, and `t_us` is when the call started.
  `reports` are the raw input reports in hex; each `k` is the kind on a single frame or stream start, and `cont` on a continuation.
- `close`: the transport was dropped, a connection boundary.
- `settle`: `nominal_ms`, `applied_ms`, from `device_gate::settle_ms`.
  Raw `thread::sleep` sites are not counted.
- `fw`: `fw`, parsed from a `currentFwResponse` report.
- `capture`: `cap_id`, `backend`, `t_start_us`, `t_end_us`, `stim_sha256`, `stim_frames`, `rate`, `tail_ms`, `channels`, `frames`, `lufs_i`, `true_peak_dbtp`, `spread_lu`, `peak`, `rms`, `engage`, `ctx`, `wav`, `err`.
  Loudness is the processed pair's; `peak` and `rms` cover every returned channel.
  `engage` is the last re-amp toggle seen on the wire, its `seq` and whether an echo arrived: context, not proof (proof is a Mixer `reAmpModeRequest` (4) → field 9 read, or a capture that passes `leveller::measure_floor_guarded`).
  `ctx` names the sound (`label`, `slot`, `scene_slot`, `switch`) when the caller knows it.
- `cmd`: `name`, `args_sha256`, `ok`, `ms`, from the e2e server's `/invoke` route only.
- `state`: `kind` (`field8`), `slot`, `bytes`, `complete`, `canon_sha256`, `pristine`, `file`.
  `pristine` is `ok`, the reason the body is not pristine, or `null` when not checked.
  A body with `complete: false` did not parse and is never the stored truth.
- `drop`: `what`, `count`.
  The recorder shed work (`wire`, `capture` or `other`); a trace with any `drop` is not a clean conformance source.

Per-report arrival times are not recorded; reports arrive one per 2.143 ms, which is enough to estimate them.
`LiveReamp` and `reamp_measure` captures are not tapped.

**Canonical field-8 bodies** (`trace::canonical_json`, shared with any replay normalizer): keys sorted at every depth, arrays in order, every non-integer number through `f32` and then six significant digits, integers and strings (node ids included) verbatim.
The device's own re-rounding moves a value by a few `f32` steps: an imported `0.49` reads back as `0.489999920129776`.

## Timing

The caller's thread only clones bytes and hands them to a bounded queue.
One writer thread per process does the hex and file work and flushes at least every 250 ms; a second thread does each capture's hashing, metering and WAV, so a capture line lands when that finishes (with its own `t_us` and `t_nom_ms`) and nothing waits behind it.
A transport drop waits at most 50 ms for the writer to catch up on the cheap lines, so a short-lived probe still leaves a complete trace and a mid-session reopen is not held up.
A full queue drops and counts, never blocks, so the recorder cannot push an OUT write toward the 750 ms session lapse ([device/session.md#session-lapse](device/session.md#session-lapse)).

## Privacy

Traces carry preset names and bodies from the unit they were recorded on, and captures are the device's DSP output.
Raw evidence never leaves that machine: never commit it, attach it to an issue or PR, or upload it as a CI artifact.
Only derived, redacted data is ever committed.
