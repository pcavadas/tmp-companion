#!/usr/bin/env bash
#
# replay-user-leveling.sh — headless replay of a real user's leveling session against
# THREE presets already resident on the device (slots 26/27/28 — 0-based list indices,
# the app's own slot space), followed by an external ffmpeg verdict on every leveled row.
# ORCHESTRATION WRAPPER, same shape as `scripts/validate-hbe.sh`, but driven over the
# ONLINE `e2e_server` HTTP bridge (the same one `scripts/e2e.sh online …` starts) instead
# of the `probe` CLI — because every command here (`level_preset`,
# `level_scenes_apply_batched`, `level_footswitches_apply`, `e2e_measure_sound`,
# `read_library_via_backup`) is a real Tauri command with no `probe` CLI surface.
#
# It:
#   1. builds the e2e_server (`--features e2e`) + a PLAIN (non-e2e) `probe` for the
#      guaranteed reamp-off fallback, and acquires the machine-global device lock;
#   2. starts `e2e_server` in ONLINE mode itself (`TMP_E2E_ONLINE=1`, this worktree's
#      derived port — see `scripts/e2e.sh`'s port-stride comment, mirrored below) with
#      `TMP_E2E_VALIDATE_LOG`/`TMP_E2E_VALIDATE_WAV_DIR` exported INTO its environment —
#      that process is what runs the strict re-measures and emits the P5 expectation log
#      (`.claude/rules/e2e.md`'s "External validation (P5)");
#   3. confirms the server is GENUINELY online from the SERVER LOG's own
#      "seeded snapshot from the real device" line (CLAUDE.md: the runner's stdout never
#      carries it) AND `/health`'s `online:true` — refuses to continue otherwise;
#   4. reads the whole library ONCE (`read_library_via_backup`, non-destructive) and
#      refuses to run unless every target slot's NAME matches the table below (danger.md:
#      "a slot is a position, not an identity") — resolving scene NAMES to their wire
#      `scenes[]` index and footswitch NAMES to their `group_id`/`node_id`/`parameter_id`
#      triple from that SAME read, never guessed;
#   5. per preset, in the order the app used (base → scenes → footswitches), issues ONE
#      SAVING leveling call per step (batched for scenes/footswitches — one command levels
#      every row of that step), then WAITS OUT THE LAZY-SAVE COMMIT WINDOW (150 s) before
#      re-measuring that preset — mandatory, not politeness, and NOT redundant with the
#      leveling commands' own in-process `ensure_fresh_load` barrier: the measurement seam
#      (`leveller::measure_sound_asis_strict`, behind `e2e_measure_sound`) opens its OWN
#      fresh session with NO registry check (`.claude/rules/danger.md`'s lazy-commit entry;
#      `src-tauri/src/leveller.rs`'s `capture_full_at_params`/`capture_fs_at`);
#   6. re-measures every leveled row with `e2e_measure_sound` (the strict re-measure
#      command), which appends one expectation row + WAV to the log the server already
#      has armed;
#   7. AFTER all three presets: a completeness check (every row this script ASKED for is
#      present in the log BY IDENTITY, not count — a re-measure that dies before its
#      capture emits nothing) then `scripts/level-validate.sh --expectations <log>` ONCE;
#   8. GUARANTEED re-amp OFF on every exit path via a trap — success, a failed step, or
#      Ctrl-C — in-band first (`e2e_reamp_off` over the bridge), falling back to a fresh
#      `probe --reamp-off` process with a loud banner if the bridge is unreachable.
#
# THE STIMULUS PRECEDENCE TRAP — READ BEFORE RUNNING: `run_e2e_server` unconditionally
# DEFAULTS `TMP_E2E_STIMULUS` to the bundled `guitar-humbucker.wav` the instant it starts,
# UNLESS the var is already SET (even to "") in its environment — and
# `resolve_stimulus_with_capture`'s precedence chain (`src-tauri/src/commands/
# level_preset.rs`, "ORDER IS LOAD-BEARING") checks `TMP_E2E_STIMULUS` FIRST, before the
# profile's stored Tier-2 DI capture, the topology WAV, or `TMP_LEVELLER_STIMULUS` (in
# that order). Left alone (the harness's default), EVERY command below — leveling AND
# re-measure alike — would silently use guitar-humbucker.wav regardless of the
# `profileId`/`topologyId` we pass. That is NOT faithful to the real session, so this
# script exports `TMP_E2E_STIMULUS=""` (empty, not unset) into the server's env below:
# `run_e2e_server`'s default-set is skipped (`var().is_err()` is false for an empty
# string), and the resolver's own `!p.is_empty()` guard then falls through to the
# `profileId` branch, giving `level_preset`/`level_scenes_apply_batched`/
# `level_footswitches_apply` the REAL "Maverick bridge" DI capture, verbatim, exactly as
# the UI would for this profile.
#
# CORRECTION (HW, 2026-08-19): the last clause above was WRONG, and it cost a whole
# acceptance run. The `profileId` branch cannot win under the MockRuntime — see
# `LEVEL_TOPOLOGY_ID` below for why and for the routing that replaces it. The leveling
# commands are no longer passed `$TOPOLOGY_ID` at all.
#
# `e2e_measure_sound` (the P5 re-measure step) has NO `profileId` param at all — only
# `topologyId`, whose normal resolution (`topology_wav_path`) goes through Tauri's
# `app.path().resolve(_, BaseDirectory::Resource)`, which a maintainer's own comment in
# `e2e_server.rs` states "can't resolve bundle resources" under the MockRuntime this
# harness runs on. Rather than accept that as a hard limit on P5 validation, this script
# closes the gap with the ONE remaining rung of the same precedence chain:
# `TMP_LEVELLER_STIMULUS`, which sits after `topology_id` but is NOT feature-gated, so it
# is live for `e2e_measure_sound` too. The server is started with
# `TMP_LEVELLER_STIMULUS=<profile capture WAV path>` (see `PROFILE_CAPTURE_WAV` below —
# `profiles::capture_wav_path_in`'s exact on-disk path for this profile id, existence
# checked before anything else runs), and every `e2e_measure_sound` call in `measure_row`
# passes `topologyId:""` — empty, not the real "guitar-filtertron" — so
# `.filter(|t| !t.is_empty())` drops it and resolution falls through past the broken
# `topology_wav_path` call straight to `TMP_LEVELLER_STIMULUS`. `e2e_measure_sound`'s own
# capture call passes `calibration_lufs: None` unconditionally (not wired to the profile
# at all), so this path never risks the double-rescale a `TMP_E2E_STIMULUS`-hijack
# workaround would have caused — the SAME DI-capture bytes leveling used are injected
# verbatim for the re-measure too, by construction, not by trusting `topology_wav_path`.
# This is a deliberate, one-line deviation from "pass topologyId on every command that
# accepts it" for `e2e_measure_sound` specifically; every OTHER command still gets the
# real `$TOPOLOGY_ID`.
#
# THE TABLE — the whole replay at a glance; change a target here, nowhere else. Columns:
#   slot|kind|id1|id2|id3|id4|target
#     kind=base   id1..id4 unused ("-")
#     kind=scene  id1=scene NAME (resolved to its wire index via the library read)
#     kind=fs     id1=switch id2=groupId id3=fenderId id4=parameterId
# Order within a slot mirrors the app's own Level flow: base, then scenes, then
# footswitches — and this table lists them in exactly that order per slot.
PRESET_NAMES=(
  "26|Plumes+BD2+OCD"
  "27|Friedman HBE"
  "28|JFF LP  Hiwatt 3 scenes"
)
ROWS=(
  "26|base|-|-|-|-|-23"
  "26|fs|5|G1|ACD_Plumes|level|-23"
  "26|fs|6|G1|ACD_BluesDriver|level|-23"
  "26|fs|7|G1|ACD_ObsessiveDrive|blend|-21"
  "26|fs|8|G1|ACD_Rat|volume|-21"
  "27|base|-|-|-|-|-23"
  "27|scene|Dirt|-|-|-|-23"
  "27|scene|Crunch|-|-|-|-21"
  "27|scene|Solo|-|-|-|-19"
  "27|scene|Clean|-|-|-|-23"
  "27|fs|0|G1|ACD_Chorus_CE5Mono|mix|-23"
  "27|fs|2|G1|ACD_TubeScreamer|blend|-23"
  "27|fs|3|G1|ACD_MicroPitch|mix|-23"
  "28|base|-|-|-|-|-23"
  "28|scene|Clean|-|-|-|-23"
  "28|scene|Rhythm|-|-|-|-21"
  "28|scene|Dirty|-|-|-|-19"
  "28|fs|2|G1|ACD_MythicDrive|output|-23"
  "28|fs|3|G1|ACD_Lightspeed|loudness|-23"
  "28|fs|11|G1|ACD_TremoloBias|level|-23"
  "28|fs|12|G4|ACD_UniVibe|volume|-23"
)
# sw1 ACD_PhaserP90 on slot 27 has no level-class parameter and is deliberately absent
# above; "Base Scene" on slot 28 was never individually leveled in the recorded session
# and is deliberately absent too (only Clean/Rhythm/Dirty were).

# Usage:
#   scripts/replay-user-leveling.sh [--tol LU] [--out DIR]
#
# Options:
#   --tol LU    validation tolerance (default $TMP_E2E_LEVEL_TOL_LU or 1.0 — see
#               level-validate.sh's TOLERANCE note)
#   --out DIR   artifact dir for the server log, expectations log, WAVs and per-step
#               responses (default mktemp -d)
#
# Exit: 0 = every measured row PASSed or SKIPped · 1 = a leveling/resolution step failed,
#       a row was never emitted, or a row FAILed judging · 2 = usage/precondition error ·
#       3 = validation SKIPPED (ffmpeg absent — nothing was independently checked; the
#       leveling itself ran and saved).
#
# BSD userland only (macOS system /bin/bash 3.2): no `timeout(1)`, no GNU flags, no
# `mapfile`/associative arrays — plain indexed arrays + process substitution only.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_TAURI="$REPO/src-tauri"
cd "$REPO"

log()  { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
err()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }
# shellcheck disable=SC2329  # only called from cleanup(), itself invoked via `trap`
warn() { printf '\033[33m⚠ %s\033[0m\n' "$*"; }
gap()  { sleep 5; }  # HID open-lockout hygiene between device-touching calls — danger.md

usage() {
  local last
  last="$(grep -n '^set -euo pipefail' "${BASH_SOURCE[0]}" | head -1 | cut -d: -f1)"
  sed -n "3,$((last - 1))p" "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit 2
}

# ── arg parsing ────────────────────────────────────────────────────────────────────
TOL="${TMP_E2E_LEVEL_TOL_LU:-1.0}"
OUT_DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --tol) TOL="${2:-}"; shift 2 ;;
    --out) OUT_DIR="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) err "unrecognized argument: $1"; usage ;;
  esac
done

command -v jq >/dev/null 2>&1 || {
  err "jq is required — this script resolves scene/footswitch identity from a nested"
  err "device-library read, and a hand-rolled sed parser for that shape risks writing"
  err "to the WRONG block on real hardware (danger.md). Install jq and rerun."
  exit 2
}

if [ -z "$OUT_DIR" ]; then
  OUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/replay-user-leveling.XXXXXX")"
fi
mkdir -p "$OUT_DIR"
log "artifacts: $OUT_DIR"

SERVER_LOG="$OUT_DIR/e2e_server.log"
VALIDATE_LOG="$OUT_DIR/expectations.jsonl"
VALIDATE_WAV_DIR="$OUT_DIR/wavs"
LIB_JSON="$OUT_DIR/library.json"
: > "$VALIDATE_LOG"
mkdir -p "$VALIDATE_WAV_DIR"

# The lazy-save commit window, in seconds — mirrors `leveller::COMMIT_WINDOW_SECS` (150)
# and `scripts/validate-hbe.sh`'s own constant. See the header note above for WHY this is
# not covered by the leveling commands' in-process barrier.
COMMIT_WINDOW_WAIT=150

# The stimulus profile the real session used ("Maverick bridge") — passed through on
# every command that accepts a profileId/topologyId/calibrationLufs triple, exactly as
# the UI does. Treated as given (recovered from the user's app log + profiles.json).
PROFILE_ID="9a57daa8-b8b3-4a29-a39b-0759eb468d28"
TOPOLOGY_ID="guitar-filtertron"
CALIBRATION_LUFS="-22.182825"

# THE LEVELING COMMANDS' OWN STIMULUS ROUTING — deliberately NOT `$TOPOLOGY_ID`.
#
# `resolve_stimulus_with_capture`'s profile_id branch calls
# `profiles::existing_capture_for`, which resolves `app.path().app_config_dir()`. Under the
# MockRuntime this harness runs on, that is NOT the real `dev.tmpcompanion.app` directory,
# so the branch returns None even though the capture file is present on disk (the existence
# check below passes and proves nothing about it). Resolution then reaches
# `if let Some(tid) = topology_id.filter(|t| !t.is_empty()) { return topology_wav_path(..) }`
# — an unconditional return — and "guitar-filtertron" canonicalises to "guitar-singlecoil",
# which DOES resolve. The result: every leveling command solved against a bundled SYNTHETIC
# sample while `e2e_measure_sound` judged the outcome against the real DI capture. Two
# different stimuli through a nonlinear amp, and the whole verdict was an artefact
# (HW, 2026-08-19 — every one of 13 rows landed 0.7-4.4 dB quiet, in one direction).
#
# Passing an EMPTY topology id drops that branch (`.filter(|t| !t.is_empty())`) and lets
# resolution fall through to `TMP_LEVELLER_STIMULUS` — the profile capture exported into the
# server's env — which is the SAME file `measure_row` already routes to. Leveling and
# re-measure then share one stimulus by construction.
#
# `calibrationLufs` must go null with it. `resolve_stimulus_for_leveling` returns
# `if from_capture { None } else { calibration_lufs }`, so the real app NULLS calibration
# whenever the profile capture wins — the capture is already at its calibrated level.
# Reaching the same bytes via `TMP_LEVELLER_STIMULUS` reports `from_capture=false`, so a
# non-null calibration here would rescale a capture the app injects verbatim.
#
# Nothing else changes: `playback_offset_for` maps an absent/unknown topology to the guitar
# default, whose offset is 0.0 at every playback level — the same 0.0 "guitar-filtertron"
# yields (`profiles::playback_offset_lu`).
LEVEL_TOPOLOGY_ID=""
LEVEL_CALIBRATION_LUFS="null"

# `profiles::capture_wav_path_in` = `<app_config_dir>/captures/<sanitized-id>.wav`;
# the profile id above is a plain UUID so sanitize_id is the identity map. app_config_dir
# is `app.path().app_config_dir()` (identifier `dev.tmpcompanion.app`, per tauri.conf.json —
# NOT `dev.cavadas.tmp-companion`, a stale sibling with plausible-looking placeholder data).
# This file must exist BEFORE anything below runs: without it, `profiles::existing_capture_for`
# returns None, `resolve_stimulus_with_capture`'s profile_id branch falls through to a
# SYNTHETIC stimulus with `from_capture=false` — so `calibration_lufs` is NOT nulled and the
# whole replay silently levels every row against a rescaled synthetic sample instead of the
# real "Maverick bridge" DI capture, with nothing in this script able to notice. Fail loud,
# before touching the device at all, rather than let that happen quietly.
PROFILE_CAPTURE_WAV="$HOME/Library/Application Support/dev.tmpcompanion.app/captures/$PROFILE_ID.wav"
if [ ! -f "$PROFILE_CAPTURE_WAV" ]; then
  err "profile capture WAV not found at:"
  err "  $PROFILE_CAPTURE_WAV"
  err "Without it, leveling falls back to a SYNTHETIC stimulus with calibration_lufs NOT"
  err "nulled — not the real DI capture this replay is supposed to reproduce. Refusing to"
  err "start (no device was touched)."
  exit 2
fi
ok "profile capture WAV confirmed: $PROFILE_CAPTURE_WAV"
# State the SESSION BEING REPLAYED next to the routing this script actually uses for it, so
# a run's own log answers "were these really the user's settings?" without reading the
# source. `$TOPOLOGY_ID`/`$CALIBRATION_LUFS` are the recorded session's values and are
# deliberately NOT what the leveling commands receive — see `LEVEL_TOPOLOGY_ID` above for
# why passing them would silently level against a synthetic sample under this harness.
log "replaying profile $PROFILE_ID (topology $TOPOLOGY_ID, calibration $CALIBRATION_LUFS)"
log "  leveling commands are passed topologyId='' + calibrationLufs=null so resolution lands"
log "  on TMP_LEVELLER_STIMULUS — the SAME capture bytes the re-measure judges against"

# shellcheck source=scripts/device-lock.sh disable=SC1091
. "$REPO/scripts/device-lock.sh"

# ── per-worktree bridge port — MIRRORS scripts/e2e.sh's derivation exactly (same
# formula, same constants) so this script lands on the SAME port e2e.sh would use for
# this worktree; the device lock (below) is what actually serializes the one real unit,
# this just avoids gratuitously claiming a different port. Update alongside e2e.sh if its
# constants ever change. ──
PORT_STRIDE=8
PORT_BASE=7800
PORT_OFFSET=$(( $(printf '%s' "$REPO" | cksum | cut -d' ' -f1) % 200 ))
PORT="${TMP_E2E_PORT:-$((PORT_BASE + PORT_OFFSET * PORT_STRIDE))}"
export TMP_E2E_PORT="$PORT"

MANIFEST="src-tauri/Cargo.toml"
PROBE_BIN="$SRC_TAURI/target/debug/probe"
SERVER_PID=""

# Kill ONLY an `e2e_server` on a port — never a bystander (mirrors scripts/e2e.sh).
kill_port() {
  for pid in $(lsof -ti "tcp:$1" 2>/dev/null || true); do
    case "$(ps -o comm= -p "$pid" 2>/dev/null || true)" in
      *e2e_server) kill "$pid" 2>/dev/null || true ;;
    esac
  done
}

# A real build (tauri-build's generate_context!) panics if ./dist is absent — gitignored,
# missing in a fresh worktree. A stub is enough; the live UI is never rendered here.
ensure_dist() { [ -f dist/index.html ] || { mkdir -p dist; printf '<!doctype html><title>e2e</title>' > dist/index.html; }; }

# POST one /invoke command; echoes the raw response body. $1=json body $2=timeout(s).
bridge_post() {
  curl -fsS -m "${2:-60}" -X POST "http://127.0.0.1:$PORT/invoke" \
    -H 'content-type: application/json' -d "$1"
}

# POST one /invoke command and unwrap {"ok":true,"data":...} → echoes compact `data`
# JSON on stdout; on {"ok":false,...} (or a transport failure) prints the error and
# returns 1. Every device-touching call in this script goes through this ONE seam.
invoke_cmd() { # $1=json body $2=timeout(s, default 60)
  local resp ok_flag
  if ! resp="$(bridge_post "$1" "${2:-60}")"; then
    err "bridge POST failed (transport) for: $(printf '%s' "$1" | jq -r '.cmd' 2>/dev/null || echo '?')"
    return 1
  fi
  ok_flag="$(printf '%s' "$resp" | jq -r '.ok' 2>/dev/null || echo false)"
  if [ "$ok_flag" != "true" ]; then
    err "$(printf '%s' "$1" | jq -r '.cmd' 2>/dev/null || echo '?') failed: $(printf '%s' "$resp" | jq -r '.error // "unknown error"' 2>/dev/null)"
    return 1
  fi
  printf '%s' "$resp" | jq -c '.data'
}

# ── prebuild: the e2e_server (feature-gated, ONLINE-mode) + a PLAIN probe for the
# reamp-off fallback — a plain, non-`--features e2e` probe. An e2e-built probe would be
# fine for --reamp-off (it does no measurement), but a stray binary in the same target dir
# is the exact class of mixup `gates.sh`/`e2e.sh` guard against; a dedicated plain build
# keeps this script clean of that hazard entirely. ──
ensure_dist
log "[build] cargo build --features e2e --bin e2e_server…"
cargo build -q --manifest-path "$MANIFEST" --features e2e --bin e2e_server \
  >"$OUT_DIR/build-e2e-server.log" 2>&1 \
  || { err "e2e_server build failed (see $OUT_DIR/build-e2e-server.log)"; exit 1; }
log "[build] cargo build --bin probe (plain, reamp-off fallback only)…"
( cd "$SRC_TAURI" && cargo build --bin probe ) >"$OUT_DIR/build-probe.log" 2>&1 \
  || { err "probe build failed (see $OUT_DIR/build-probe.log)"; exit 1; }
[ -x "$PROBE_BIN" ] || { err "probe binary not found at $PROBE_BIN after build"; exit 1; }

SCRIPT_LABEL="$REPO (replay-user-leveling: slots 26/27/28)"

# ── ONE cleanup path for every exit — success, a failed step, or Ctrl-C. Order: re-amp
# OFF first (danger.md's guarantee, never delayed by a slower step), then kill the server,
# then release the device lock LAST. ──
# shellcheck disable=SC2329  # invoked via `trap cleanup EXIT INT TERM`
cleanup() {
  local code=$? reamp_ok=0
  trap - EXIT INT TERM
  log "cleanup — guaranteed re-amp OFF…"
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null \
     && invoke_cmd '{"cmd":"e2e_reamp_off","args":{}}' 30 >"$OUT_DIR/reamp-off.log" 2>&1; then
    ok "re-amp OFF confirmed over the bridge"
    reamp_ok=1
  fi
  # Release the HID (kill the server) BEFORE any probe fallback, whether or not the
  # in-band attempt above succeeded — a still-live server holding the exclusive HID seize
  # makes probe's own open hit the lockout (0xe00002c5), and each failed open re-arms it
  # (danger.md's HID open-lockout model). Fresh-quiet-then-open needs the seize released
  # first.
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  kill_port "$PORT"
  if [ "$reamp_ok" -ne 1 ]; then
    warn "in-band reamp-off unavailable or failed — falling back to a fresh probe process"
    sleep 3
    if ! ( cd "$SRC_TAURI" && "$PROBE_BIN" --reamp-off ) >"$OUT_DIR/reamp-off-fallback.log" 2>&1; then
      err "reamp-off did not confirm — waiting out the HID lockout and retrying ONCE…"
      sleep 30
      if ! ( cd "$SRC_TAURI" && "$PROBE_BIN" --reamp-off ) >>"$OUT_DIR/reamp-off-fallback.log" 2>&1; then
        err "########################################################################"
        err "#  RE-AMP MAY STILL BE ENGAGED — the unit is INPUT-MUTED until it is"
        err "#  turned off. Run this yourself once the device is quiet:"
        err "#      cargo run --bin probe -- --reamp-off"
        err "#  (log: $OUT_DIR/reamp-off-fallback.log)"
        err "########################################################################"
        code=1
      else
        ok "re-amp OFF confirmed on the fallback retry"
      fi
    else
      ok "re-amp OFF confirmed via the fallback probe"
    fi
  fi
  device_lock_release
  log "artifacts kept at $OUT_DIR"
  exit "$code"
}
trap cleanup EXIT INT TERM

device_lock_acquire "$SCRIPT_LABEL" || exit 1
kill_port "$PORT"

# ── start the ONLINE e2e_server ourselves ────────────────────────────────────────────
: > "$SERVER_LOG"
log "starting e2e_server ONLINE on :${PORT}"
# TMP_E2E_STIMULUS="" (empty, NOT unset) — see the header's "STIMULUS PRECEDENCE TRAP"
# note: this defeats run_e2e_server's unconditional guitar-humbucker.wav default while
# also defeating resolve_stimulus_with_capture's own e2e-first short-circuit, letting the
# real profile/topology precedence chain run exactly as the UI would drive it for the
# leveling commands (level_preset/level_scenes_apply_batched/level_footswitches_apply).
#
# TMP_LEVELLER_STIMULUS=<profile capture WAV> — this is what makes the RE-MEASURE step
# (e2e_measure_sound, which has no profileId param at all) inject the SAME bytes leveling
# used, without ever calling topology_wav_path (broken under MockRuntime — see header).
# It sits AFTER profile_id/topology_id in the precedence chain, so it changes nothing for
# leveling; measure_row (below) pairs this with topologyId:"" so the empty topology_id is
# filtered out by resolve_stimulus_with_capture's `.filter(|t| !t.is_empty())` and
# resolution falls through past topology_wav_path to this var. And because
# e2e_measure_sound's own capture call passes `calibration_lufs: None` unconditionally
# (never wired to the profile's calibration at all), the capture is injected verbatim
# either way — no rescale risk, unlike the rejected TMP_E2E_STIMULUS-hijack workaround.
TMP_E2E_ONLINE=1 TMP_E2E_PORT="$PORT" TMP_E2E_STIMULUS="" \
  TMP_LEVELLER_STIMULUS="$PROFILE_CAPTURE_WAV" \
  TMP_E2E_VALIDATE_LOG="$VALIDATE_LOG" TMP_E2E_VALIDATE_WAV_DIR="$VALIDATE_WAV_DIR" \
  cargo run -q --manifest-path "$MANIFEST" --features e2e --bin e2e_server \
  >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true

# Ready-gate: the SERVER LOG's own seed line (never the runner's stdout — CLAUDE.md's
# stale-server false-green trap) AND /health's online:true. Refuses to continue otherwise
# — a silent fall back to an offline/fake server would fabricate every LUFS.
log "waiting for the real-device handshake…"
SERVER_READY=0
for _ in $(seq 1 240); do
  if grep -q "device handshake failed" "$SERVER_LOG" 2>/dev/null; then
    err "$(grep 'device handshake failed' "$SERVER_LOG" | tail -1)"
    err "  → is the unit plugged in and Pro Control closed? (Pro Control holds the exclusive HID seize)"
    exit 1
  fi
  if grep -q "seeded snapshot from the real device" "$SERVER_LOG" 2>/dev/null; then
    if curl -fsS -m 5 "http://127.0.0.1:$PORT/health" 2>/dev/null \
       | jq -e '.ok == true and .online == true' >/dev/null 2>&1; then
      SERVER_READY=1
      break
    fi
  fi
  sleep 1
done
if [ "$SERVER_READY" -ne 1 ]; then
  err "e2e_server not confirmed ONLINE after 240s — refusing to proceed (see $SERVER_LOG)"
  tail -40 "$SERVER_LOG" >&2
  exit 1
fi
ok "e2e_server ONLINE and seeded from the real device"

log "resting the unit before the first device-touching call (post-handshake settle)…"
sleep 60

# ── read the whole library ONCE (non-destructive) and confirm every target slot's name ──
log "[1] read_library_via_backup (non-destructive)…"
invoke_cmd '{"cmd":"read_library_via_backup","args":{}}' 180 > "$LIB_JSON" \
  || { err "read_library_via_backup failed — see $OUT_DIR"; exit 1; }
gap

FAILED=0
for row in "${PRESET_NAMES[@]:-}"; do
  [ -n "$row" ] || continue
  slot="${row%%|*}"; expect_name="${row#*|}"
  # ADDRESS SPACES — the two differ by one and danger.md's first rule is to put the guard in
  # the SAME space as the mutation. `$slot` is the 0-BASED LIST INDEX the leveling commands
  # take (`level_preset`'s `slot`, and what the user's own run logged as `slot=26` for
  # "Plumes+BD2+OCD"). `read_library_via_backup` reports `.slot` as the 1-BASED DEVICE slot —
  # the number the UI shows (027/028/029) and the one `preset_json_from_backup` addresses by.
  # So the lookup is `$slot + 1`; comparing raw would check the NEIGHBOUR preset's name and
  # wave through a run that then wrote to a different preset than it verified.
  dev_slot=$((slot + 1))
  actual_name="$(jq -r --argjson slot "$dev_slot" '.presets[]? | select(.slot == $slot) | .name' "$LIB_JSON")"
  if [ -z "$actual_name" ]; then
    err "list index $slot (device slot $dev_slot) not found in the library read — refusing (a slot is a position, not an identity)"
    FAILED=1
  elif [ "$actual_name" != "$expect_name" ]; then
    err "list index $slot (device slot $dev_slot) holds \"$actual_name\", expected \"$expect_name\" — refusing to run (danger.md)"
    FAILED=1
  else
    ok "list index $slot (device slot $dev_slot) confirmed: \"$actual_name\""
    jq --argjson slot "$dev_slot" '.presets[] | select(.slot == $slot)' "$LIB_JSON" \
      > "$OUT_DIR/preset-$slot.json"
  fi
done
[ "$FAILED" -eq 0 ] || { err "identity guard failed — no device write was attempted"; exit 1; }

# ── row-table lookup helper: prints "id1|id2|id3|id4|target" lines for $1=slot $2=kind ──
rows_for() {
  local want_slot="$1" want_kind="$2" r rs rk r1 r2 r3 r4 rt
  for r in "${ROWS[@]:-}"; do
    [ -n "$r" ] || continue
    IFS='|' read -r rs rk r1 r2 r3 r4 rt <<<"$r"
    if [ "$rs" = "$want_slot" ] && [ "$rk" = "$want_kind" ]; then
      printf '%s|%s|%s|%s|%s\n' "$r1" "$r2" "$r3" "$r4" "$rt"
    fi
  done
}

# Resolve a scene NAME to its wire `scenes[]` index for $1=slot $2=name. Empty = not found.
scene_index_for() {
  jq -r --arg n "$2" \
    '[.scenes | to_entries[] | select(.value.name == $n) | .key] | .[0] // empty' \
    "$OUT_DIR/preset-$1.json"
}

# Resolve a footswitch NAME (switch+group+fenderId+parameterId) to its `node_id` for
# $1=slot $2=switch $3=group $4=fenderId $5=parameterId. Empty = not found.
fs_node_for() {
  jq -r --argjson sw "$2" --arg grp "$3" --arg fid "$4" --arg p "$5" \
    '(.footswitches[]? | select(.switch == $sw) | .level_params[]?
       | select(.group_id == $grp and .fender_id == $fid and .parameter_id == $p)
       | .node_id) // empty' \
    "$OUT_DIR/preset-$1.json"
}

# ── resolution pre-flight: prove every row in the table resolves BEFORE any device
# write, across ALL THREE presets — not discovered mid-run after slots 26/27 are already
# leveled and saved (danger.md: a save cannot be undone from the app). ──
log "[2] resolution pre-flight — scenes, footswitches, amp candidates…"
for row in "${PRESET_NAMES[@]:-}"; do
  [ -n "$row" ] || continue
  slot="${row%%|*}"
  need_amp_candidates=0
  while IFS='|' read -r name _ _ _ _; do
    need_amp_candidates=1
    idx="$(scene_index_for "$slot" "$name")"
    if [ -z "$idx" ]; then
      err "scene \"$name\" does not resolve on slot $slot — refusing to run (have: $(jq -r '.scenes[]?.name' "$OUT_DIR/preset-$slot.json" | tr '\n' '/'))"
      FAILED=1
    fi
  done < <(rows_for "$slot" scene)
  if [ "$need_amp_candidates" -eq 1 ]; then
    n_cand="$(jq -r '.amp_candidates | length' "$OUT_DIR/preset-$slot.json")"
    if [ "$n_cand" -eq 0 ]; then
      err "slot $slot has scene rows but .amp_candidates is EMPTY — the batched scene call would have nothing to drive and every row would silently filter out; refusing to run"
      FAILED=1
    fi
  fi
  while IFS='|' read -r sw grp fid param _; do
    node="$(fs_node_for "$slot" "$sw" "$grp" "$fid" "$param")"
    if [ -z "$node" ]; then
      err "footswitch $sw ($grp/$fid.$param) does not resolve on slot $slot — refusing to run (available level_params: $(jq -c --argjson sw "$sw" '.footswitches[]? | select(.switch == $sw) | .level_params' "$OUT_DIR/preset-$slot.json"))"
      FAILED=1
    fi
  done < <(rows_for "$slot" fs)
done
[ "$FAILED" -eq 0 ] || { err "resolution pre-flight failed — no device write was attempted"; exit 1; }
ok "[2] every scene/footswitch row in the table resolves on its slot"

EXPECTED_LABELS=""
expect_row() { EXPECTED_LABELS="$EXPECTED_LABELS$1
"; }

# One re-measure + expectation-emission call. $1=slot $2=scene(or -) $3=switch(or -)
# $4=group(or -) $5=node(or -) $6=param(or -) $7=target $8=clamped $9=persist_mismatch $10=label
measure_row() {
  local slot="$1" scene="$2" sw="$3" grp="$4" node="$5" param="$6" target="$7" \
        clamped="$8" pm="$9" label="${10}" scene_json sw_json lev_json body data lufs
  scene_json="null"; [ "$scene" = "-" ] || scene_json="$scene"
  sw_json="null"; [ "$sw" = "-" ] || sw_json="$sw"
  lev_json="null"
  [ "$grp" = "-" ] || lev_json="$(jq -nc --arg g "$grp" --arg n "$node" --arg p "$param" \
    '{groupId:$g, nodeId:$n, parameterId:$p}')"
  # topologyId:"" (deliberate deviation from "pass topologyId on every command that
  # accepts it" — see the header's STIMULUS PRECEDENCE TRAP note): e2e_measure_sound has
  # no profileId param, so an empty topology_id is what routes its OWN resolve_stimulus
  # call past the broken-under-MockRuntime topology_wav_path and onto TMP_LEVELLER_STIMULUS
  # (the profile capture WAV, exported into the server's env above) instead.
  body="$(jq -nc \
    --argjson slot "$slot" --argjson scene "$scene_json" --argjson sw "$sw_json" \
    --argjson lev "$lev_json" \
    --argjson target "$target" --argjson clamped "$clamped" --argjson pm "$pm" \
    '{cmd:"e2e_measure_sound", args:{slot:$slot, scene:$scene, footswitch:$sw,
      topologyId:"", lev:$lev,
      validate:{targetLufs:$target, clamped:$clamped, persistMismatch:$pm}}}')"
  # NON-FATAL on purpose (mirrors scripts/validate-hbe.sh's step 6): the label was
  # already recorded above, so a dead re-measure here is caught by the completeness
  # check after the whole run instead of aborting every OTHER row still to be measured.
  expect_row "$label"
  if data="$(invoke_cmd "$body" 90)"; then
    lufs="$(printf '%s' "$data" | jq -r '.')"
    log "  [remeasure] $label — self-reported ${lufs} LUFS vs target ${target} LUFS (authoritative verdict is the ffmpeg judge, run once at the end)"
  else
    err "re-measure of $label FAILED — the completeness check below will catch its missing row"
  fi
  gap
}

process_preset() {
  local slot="$1" base_target base_row body data clamped pm \
        scene_jobs_ndjson fs_jobs_ndjson jobs_json cand_json result \
        idx name grp fid param sw node target has_scenes has_fs

  log "════ preset slot $slot ════"

  # ── base ──────────────────────────────────────────────────────────────────────
  base_row="$(rows_for "$slot" base)"
  base_target="$(printf '%s' "$base_row" | cut -d'|' -f5)"
  log "[base] leveling slot $slot → $base_target LUFS…"
  body="$(jq -nc \
    --argjson slot "$slot" --argjson t "$base_target" \
    --arg topo "$LEVEL_TOPOLOGY_ID" --argjson cal "$LEVEL_CALIBRATION_LUFS" --arg prof "$PROFILE_ID" \
    '{cmd:"level_preset", args:{job:{
        slot:$slot, target_lufs:$t, save:true,
        topology_id:$topo, calibration_lufs:$cal, profile_id:$prof,
        stimulus_path:null,
        block_group_id:null, block_node_id:null, block_parameter_id:null, block_value:null
    }}}')"
  data="$(invoke_cmd "$body" 900)" || return 1
  printf '%s' "$data" > "$OUT_DIR/level-base-$slot.json"
  ok "[base] slot $slot leveled (clamped=$(printf '%s' "$data" | jq -r '.clamped'))"
  gap

  # ── scenes (batched — one call levels every scene row of this slot) ─────────────
  has_scenes=0
  scene_jobs_ndjson=""
  while IFS='|' read -r name _ _ _ target; do
    has_scenes=1
    idx="$(scene_index_for "$slot" "$name")"
    if [ -z "$idx" ]; then
      err "scene \"$name\" not found on slot $slot (have: $(jq -r '.scenes[].name' "$OUT_DIR/preset-$slot.json" | tr '\n' '/'))"
      return 1
    fi
    scene_jobs_ndjson="$scene_jobs_ndjson$(jq -nc --argjson i "$idx" --argjson t "$target" '{sceneSlot:$i, targetLufs:$t}')
"
  done < <(rows_for "$slot" scene)
  if [ "$has_scenes" -eq 1 ]; then
    jobs_json="$(printf '%s' "$scene_jobs_ndjson" | jq -s -c '.')"
    cand_json="$(jq -c '.amp_candidates' "$OUT_DIR/preset-$slot.json")"
    log "[scenes] leveling slot $slot's scene rows…"
    body="$(jq -nc \
      --argjson slot "$slot" --argjson jobs "$jobs_json" --argjson candidates "$cand_json" \
      --arg topo "$LEVEL_TOPOLOGY_ID" --argjson cal "$LEVEL_CALIBRATION_LUFS" --arg prof "$PROFILE_ID" \
      '{cmd:"level_scenes_apply_batched", args:{
          slot:$slot, jobs:$jobs, candidates:$candidates, save:true, rebalance:false,
          topologyId:$topo, calibrationLufs:$cal, profileId:$prof, onResult:"__CHANNEL__:0"
      }}')"
    data="$(invoke_cmd "$body" 1800)" || return 1
    printf '%s' "$data" > "$OUT_DIR/level-scenes-$slot.json"
    ok "[scenes] slot $slot: $(printf '%s' "$data" | jq 'length') scene row(s) leveled"
    gap
  else
    log "[scenes] no scene rows for slot $slot — skipping"
  fi

  # ── footswitches (batched — one call levels every fs row of this slot) ──────────
  has_fs=0
  fs_jobs_ndjson=""
  while IFS='|' read -r sw grp fid param target; do
    has_fs=1
    node="$(fs_node_for "$slot" "$sw" "$grp" "$fid" "$param")"
    if [ -z "$node" ]; then
      err "footswitch $sw ($grp/$fid.$param) not found on slot $slot — available level_params: $(jq -c --argjson sw "$sw" '.footswitches[]? | select(.switch == $sw) | .level_params' "$OUT_DIR/preset-$slot.json")"
      return 1
    fi
    fs_jobs_ndjson="$fs_jobs_ndjson$(jq -nc --argjson sw "$sw" --arg g "$grp" --arg n "$node" --arg p "$param" --argjson t "$target" \
      '{switch:$sw, levGroupId:$g, levNodeId:$n, levParameterId:$p, targetLufs:$t}')
"
  done < <(rows_for "$slot" fs)
  if [ "$has_fs" -eq 1 ]; then
    jobs_json="$(printf '%s' "$fs_jobs_ndjson" | jq -s -c '.')"
    log "[fs] leveling slot $slot's footswitch rows…"
    body="$(jq -nc \
      --argjson slot "$slot" --argjson jobs "$jobs_json" \
      --arg topo "$LEVEL_TOPOLOGY_ID" --argjson cal "$LEVEL_CALIBRATION_LUFS" --arg prof "$PROFILE_ID" \
      '{cmd:"level_footswitches_apply", args:{
          slot:$slot, jobs:$jobs, save:true,
          topologyId:$topo, calibrationLufs:$cal, profileId:$prof, onResult:"__CHANNEL__:0"
      }}')"
    data="$(invoke_cmd "$body" 1800)" || return 1
    printf '%s' "$data" > "$OUT_DIR/level-fs-$slot.json"
    ok "[fs] slot $slot: $(printf '%s' "$data" | jq 'length') footswitch row(s) leveled"
    gap
  else
    log "[fs] no footswitch rows for slot $slot — skipping"
  fi

  # ── THE COMMIT-WINDOW WAIT — see this file's header for why it is not optional ──
  log "[wait] waiting ${COMMIT_WINDOW_WAIT}s for slot $slot's LAZY save commit before any re-measure…"
  log "       (this is not a hang — danger.md: a same-slot load inside T+45-100s materializes"
  log "        the PRE-save preset, and e2e_measure_sound's capture path is not registry-guarded)"
  sleep "$COMMIT_WINDOW_WAIT"
  ok "[wait] commit window elapsed for slot $slot"

  # ── re-measure every leveled row of this slot ────────────────────────────────────
  clamped="$(jq -r '.clamped' "$OUT_DIR/level-base-$slot.json")"
  pm="$(jq -r '.persist_mismatch' "$OUT_DIR/level-base-$slot.json")"
  measure_row "$slot" - - - - - "$base_target" "$clamped" "$pm" "base:slot$slot"

  if [ "$has_scenes" -eq 1 ]; then
    while IFS='|' read -r name _ _ _ target; do
      idx="$(scene_index_for "$slot" "$name")"
      result="$(jq -c --argjson i "$idx" '[.[] | select(.scene_slot == $i)] | .[0] // empty' "$OUT_DIR/level-scenes-$slot.json")"
      if [ -z "$result" ]; then
        err "scene \"$name\" (index $idx) was requested but the leveling call silently dropped it — a filtered failure, not a level miss"
        return 1
      fi
      clamped="$(printf '%s' "$result" | jq -r '.clamped')"
      pm="$(printf '%s' "$result" | jq -r '.persist_mismatch')"
      measure_row "$slot" "$idx" - - - - "$target" "$clamped" "$pm" "scene:slot$slot:scene$idx"
    done < <(rows_for "$slot" scene)
  fi

  if [ "$has_fs" -eq 1 ]; then
    while IFS='|' read -r sw grp fid param target; do
      node="$(fs_node_for "$slot" "$sw" "$grp" "$fid" "$param")"
      result="$(jq -c --argjson sw "$sw" '[.[] | select(.switch == $sw)] | .[0] // empty' "$OUT_DIR/level-fs-$slot.json")"
      if [ -z "$result" ]; then
        err "footswitch $sw ($grp/$fid.$param) was requested but the leveling call silently dropped it — a filtered failure, not a level miss"
        return 1
      fi
      clamped="$(printf '%s' "$result" | jq -r '.clamped')"
      pm="$(printf '%s' "$result" | jq -r '.persist_mismatch')"
      measure_row "$slot" - "$sw" "$grp" "$node" "$param" "$target" "$clamped" "$pm" "footswitch:slot$slot:switch$sw"
    done < <(rows_for "$slot" fs)
  fi

  ok "════ preset slot $slot done ════"
  return 0
}

SLOTS="26 27 28"
first=1
for slot in $SLOTS; do
  if [ "$first" -eq 0 ]; then
    log "resting the unit between presets…"
    sleep 10
  fi
  first=0
  if ! process_preset "$slot"; then
    err "preset slot $slot FAILED — stopping the replay (no further presets will be touched)"
    FAILED=1
    break
  fi
done

# ── completeness check + the ONE final judge pass ───────────────────────────────────
MISSING=0
if [ "$FAILED" -eq 0 ]; then
  while IFS= read -r want; do
    [ -n "$want" ] || continue
    if ! grep -q "\"label\":\"$want\"" "$VALIDATE_LOG" 2>/dev/null; then
      err "expectation row MISSING for $want — its re-measure emitted nothing (see $OUT_DIR)"
      MISSING=$((MISSING + 1))
    fi
  done <<EOF
$EXPECTED_LABELS
EOF
fi

echo
if [ "$FAILED" -ne 0 ]; then
  err "replay-user-leveling: FAIL — a leveling/resolution step failed before validation could complete (logs in $OUT_DIR)"
  exit 1
fi
if [ "$MISSING" -ne 0 ]; then
  err "replay-user-leveling: FAIL — $MISSING expectation row(s) were never emitted; that many"
  err "                       sounds went unvalidated. Named above; artifacts in $OUT_DIR"
  exit 1
fi

log "final summary (independent ffmpeg ebur128 judge over every re-measured row):"
EXT_RC=0
if [ ! -s "$VALIDATE_LOG" ]; then
  err "no expectation rows were emitted — nothing was independently measured"
  EXT_RC=1
else
  set +e
  bash "$REPO/scripts/level-validate.sh" --expectations "$VALIDATE_LOG" --tol "$TOL"
  EXT_RC=$?
  set -e
fi

case "$EXT_RC" in
  0)
    ok "replay-user-leveling: PASS — every measured row landed within ${TOL} LU of its target"
    exit 0
    ;;
  3)
    err "replay-user-leveling: SKIPPED — ffmpeg is not on PATH, so NOTHING was independently"
    err "                       measured. The leveling itself ran and is saved."
    exit 3
    ;;
  *)
    err "replay-user-leveling: FAIL — see the per-row verdicts above and the logs in $OUT_DIR"
    exit 1
    ;;
esac
