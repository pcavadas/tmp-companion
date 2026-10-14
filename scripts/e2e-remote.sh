#!/bin/bash
# e2e-remote.sh — the REMOTE e2e lane: the online-shaped specs against a remote device
# reached over a socket (`hid::remote::RemoteHid`). Never the unit, and never a stamp.
#
#   TMP_E2E_REMOTE=tcp:127.0.0.1:11600 scripts/e2e-remote.sh [spec …]    (default: songs copy)
#   TMP_E2E_REMOTE=… scripts/e2e-remote.sh --seed-only    stop after step 3, before any spec
#   TMP_E2E_REMOTE=… TMP_E2E_REMOTE_AUDIO=tcp:127.0.0.1:11700 scripts/e2e-remote.sh level.online
#                                       with the remote device's re-amp audio: offline leveling
#
# The remote device counts as online, so pick the `*.online` specs for anything they cover: a dual-mode
# spec that skips online (level.spec.ts, for one) skips here too, and a run in which no test
# ran fails rather than passing green.
#
# --seed-only leaves the remote device holding the pristine scenario: snapshot it there. A snapshot
# taken after a run holds what the specs saved (copy.spec saves edits into 401/402), so
# every later seed would re-import those slots.
#
# The remote device itself runs outside this repository; this script only needs its address. It:
#   1. builds e2e_server + probe with `--features e2e` into a SEPARATE target dir (under the
#      git common dir, so neither the worktree's stamps nor the production `probe` in
#      src-tauri/target are touched; override with TMP_E2E_REMOTE_TARGET);
#   2. empties the remote device's scratch zone: a fresh remote device's bank holds the factory presets,
#      so the scenario slots hold presets the seed must never overwrite on a unit and
#      refuses. Each is cleared with the name-guarded `probe --clear`, and only after that
#      probe confirmed it is talking to the remote device (its `[probe] REMOTE:` banner);
#   3. seeds the scenario presets (`probe --seed-scenario`, a fresh process, as online);
#   4. starts the e2e_server and waits for its REMOTE phrase, never the online one;
#   5. runs Playwright with e2e/playwright.remote.config.ts, which only adopts that server;
#   6. with the remote device's audio, prints the capture counters (`/remote/audio`), whatever the specs
#      did, and fails the run if any capture job failed: one the remote device ran without re-amp
#      engaged, or one it could not finish;
#   7. tears down: re-amp OFF, stray sweep, recall preset 001, server stopped.
#
# Re-amp captures go to the remote device's own audio when TMP_E2E_REMOTE_AUDIO is set beside the
# remote's address (`remote_audio`: the remote device's DSP, resampled to and from its 44.1 kHz). Without
# it a spec that captures fails with "the remote device has no re-amp audio here", never a faked
# number. Either way it is not device
# evidence (remote levels are not calibrated against the unit) and writes no stamp.
#
# Ports: bridge 12000 + offset*8, Vite 1821 + offset — disjoint from offline (7800-9399 /
# 1421-1620), online (9400-10999 / 1621-1820) and the remote device harness's default 11600.
#
# Written to run under macOS system /bin/bash (3.2).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

log() { printf '\033[36m▸ %s\033[0m\n' "$*"; }
err() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }

if [ -z "${TMP_E2E_REMOTE:-}" ]; then
  err "TMP_E2E_REMOTE is not set: give the remote device's address (tcp:<host>:<port> or unix:<path>)"
  exit 2
fi
if [ "${TMP_E2E_ONLINE:-}" = 1 ]; then
  err "TMP_E2E_ONLINE=1 is set: this lane drives the remote device, never the unit — unset it"
  exit 2
fi
export TMP_E2E_ONLINE=0

SPECS=()
SEED_ONLY=0
for a in "$@"; do
  case "$a" in
    -h|--help) sed -n '2,42p' "$0"; exit 0 ;;
    --seed-only) SEED_ONLY=1 ;;
    *) SPECS+=("$a") ;;
  esac
done
if [ "${#SPECS[@]}" -eq 0 ]; then SPECS=(songs copy); fi
SPEC_FILES=()
for s in "${SPECS[@]}"; do
  f="e2e/specs/${s%.spec.ts}.spec.ts"
  [ -f "$f" ] || { err "no spec $f"; exit 2; }
  case "$s" in
    level*|doctor*)
      if [ -z "${TMP_E2E_REMOTE_AUDIO:-}" ]; then
        log "note: $s captures audio and TMP_E2E_REMOTE_AUDIO is unset — the remote device refuses captures, expect it to fail"
      fi ;;
  esac
  SPEC_FILES+=("$f")
done

PORT_STRIDE=8
PORT_OFFSET=$(( $(printf '%s' "$REPO" | cksum | cut -d' ' -f1) % 200 ))
PORT="${TMP_E2E_PORT:-$((12000 + PORT_OFFSET * PORT_STRIDE))}"
VITE_PORT="${TMP_E2E_VITE_PORT:-$((1821 + PORT_OFFSET))}"
export TMP_E2E_PORT="$PORT" TMP_E2E_VITE_PORT="$VITE_PORT" TMP_E2E_WORKERS=1

COMMON="$(cd "$(git rev-parse --git-common-dir)" && pwd)"
TARGET="${TMP_E2E_REMOTE_TARGET:-$COMMON/tmp-remote-target}"
PROBE="$TARGET/debug/probe"
SERVER="$TARGET/debug/e2e_server"
LOG_DIR="${TMPDIR:-/tmp}/tmp-companion-e2e/remote"
mkdir -p "$LOG_DIR"
SERVER_LOG="$LOG_DIR/e2e_server.log"
PROBE_LOG="$LOG_DIR/probe.log"
: > "$PROBE_LOG"
export TMP_E2E_REMOTE_REPORT="$LOG_DIR/report.json"
rm -f "$TMP_E2E_REMOTE_REPORT"
SERVER_PID=""

[ -f dist/index.html ] || { mkdir -p dist; printf '<!doctype html><title>e2e</title>' > dist/index.html; }
# A stale node_modules surfaces only as Vite's error overlay, so every spec would sit out
# its full timeout on the first click. Install from the lockfile first (a no-op when current).
bun install --frozen-lockfile >/dev/null 2>&1 || {
  err "bun install --frozen-lockfile failed — fix the frontend deps before the remote lane"; exit 1; }

kill_port() {
  for pid in $(lsof -ti "tcp:$1" 2>/dev/null || true); do
    case "$(ps -o comm= -p "$pid" 2>/dev/null || true)" in
      *e2e_server) kill "$pid" 2>/dev/null || true ;;
    esac
  done
}
bridge_post() { # $1 = JSON body, $2 = timeout (s, default 60)
  curl -fsS -m "${2:-60}" -X POST "http://127.0.0.1:$PORT/invoke" \
    -H 'content-type: application/json' -d "$1" 2>/dev/null
}

# Run probe against the remote device, appending its output to $PROBE_LOG and echoing stdout. Fails
# the lane unless probe printed its remote banner: no device op below may run unconfirmed.
remote_probe() {
  local out rc=0
  out="$("$PROBE" "$@" 2>&1)" || rc=$?
  printf '%s\n' "$out" >> "$PROBE_LOG"
  case "$out" in
    *"[probe] REMOTE: "*) ;;
    *) err "probe $1 did not confirm it is talking to the remote device — refusing (see $PROBE_LOG)"; exit 1 ;;
  esac
  printf '%s\n' "$out"
  return "$rc"
}

# shellcheck disable=SC2329,SC2317  # invoked via the EXIT/INT/TERM traps, which shellcheck doesn't count as a use
cleanup() {
  local code=$?
  [ -z "${1:-}" ] || code="$1"
  trap - EXIT INT TERM
  if curl -fsS -m 5 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    log "teardown: re-amp OFF, stray sweep, recall 001 (on the remote device)"
    bridge_post '{"cmd":"e2e_reamp_off","args":{}}' >/dev/null || true
    bridge_post '{"cmd":"e2e_clear_strays","args":{}}' 300 >/dev/null || true
    bridge_post '{"cmd":"e2e_load_preset","args":{"slot":0}}' >/dev/null || true
  fi
  if [ -n "$SERVER_PID" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  kill_port "$PORT"
  if [ "$code" -eq 0 ] && [ "$SEED_ONLY" = 1 ]; then
    log "remote seeded (--seed-only): the scenario is pristine on the remote device — snapshot it now"
  elif [ "$code" -eq 0 ]; then
    log "remote lane PASSED (${SPECS[*]}) — $PASS_NOTE; no stamp written"
  else
    err "remote lane FAILED (exit $code) — server log: $SERVER_LOG, probe log: $PROBE_LOG"
  fi
  exit "$code"
}
# What a passing run covered: the control plane, unless step 6 counts captures.
PASS_NOTE="control plane only"
trap cleanup EXIT
trap 'cleanup 130' INT
trap 'cleanup 143' TERM

log "remote lane: $TMP_E2E_REMOTE · audio ${TMP_E2E_REMOTE_AUDIO:-none} · bridge :$PORT · vite :$VITE_PORT · target $TARGET"
log "building e2e_server + probe (--features e2e, separate target dir)…"
CARGO_TARGET_DIR="$TARGET" cargo build -q --manifest-path src-tauri/Cargo.toml \
  --features e2e --bin e2e_server --bin probe

# ── 2. empty the remote device's scratch zone of presets the seed does not own ──
# The scenario's slots and names, from the fixture file itself (the same parse as
# scripts/e2e.sh's recovery: `"listIndex": N,` then `"name": "…",`).
scenario_pairs="$(awk '
  /"listIndex"/ { if (match($0, /[0-9]+/)) idx = substr($0, RSTART, RLENGTH); next }
  /"name"/ { if (idx != "" && match($0, /"name"[ \t]*:[ \t]*"[^"]*"/)) {
      s = substr($0, RSTART, RLENGTH); sub(/^"name"[ \t]*:[ \t]*"/, "", s); sub(/"$/, "", s)
      print idx "\t" s; idx = "" } }
' e2e/fixtures/scenario-presets.json)"
[ -n "$scenario_pairs" ] || { err "no slot/name pairs in e2e/fixtures/scenario-presets.json"; exit 1; }

log "reading the remote device's bank…"
listing="$(remote_probe --listall)" || {
  err "reading the remote device's bank failed: $(grep "FAILED" "$PROBE_LOG" | tail -1)"; exit 1; }
cleared=0
TAB="$(printf '\t')"
while IFS="$TAB" read -r sidx sname; do
  [ -n "$sidx" ] || continue
  # `probe --listall` prints "  idx NNN · slot NNN  <name>"; the name is everything after
  # the slot number's two-space gap.
  cur="$(printf '%s\n' "$listing" | awk -v want="$sidx" '
    $1 == "idx" && $2 == want { if (match($0, /slot +[0-9]+  /)) print substr($0, RSTART + RLENGTH); exit }')"
  case "$(printf '%s' "$cur" | tr '[:upper:]' '[:lower:]')" in
    ""|"--"|"—"|"empty") continue ;;
  esac
  if [ "$cur" = "$sname" ]; then continue; fi   # a fixture in place: the seed classifies it
  log "remote scratch slot $sidx holds a preset the seed does not own — clearing it (remote only)"
  out="$(remote_probe --clear "$sidx" "$cur")"
  case "$out" in
    *"cleared"*) cleared=$((cleared + 1)) ;;
    *) err "clearing remote slot $sidx did not report a clear — refusing to go on"; exit 1 ;;
  esac
done <<EOF
$scenario_pairs
EOF
log "scratch zone ready ($cleared slot(s) cleared)"

# ── 3. seed, in a fresh probe process ──
seeded=0
for attempt in 1 2 3; do
  log "seeding the scenario presets on the remote device (attempt $attempt)…"
  if remote_probe --seed-scenario >/dev/null; then seeded=1; break; fi
  sleep 2
done
[ "$seeded" = 1 ] || {
  err "scenario seed failed 3 times: $(grep "FAILED" "$PROBE_LOG" | tail -1)"; exit 1; }
if [ "$SEED_ONLY" = 1 ]; then exit 0; fi

# ── 4. the server ──
for i in 0 1 2 3 4 5 6 7; do kill_port $((PORT + i)); done
log "starting the remote device e2e_server on :$PORT"
"$SERVER" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true
ready=0
for _ in $(seq 1 240); do
  if grep -q "remote handshake failed\|REFUSING TO START" "$SERVER_LOG" 2>/dev/null; then
    err "$(grep "remote handshake failed\|REFUSING TO START" "$SERVER_LOG" | tail -1)"; exit 1
  fi
  if grep -q "REMOTE — seeded snapshot from the remote device" "$SERVER_LOG" 2>/dev/null; then
    health="$(curl -fsS -m 5 "http://127.0.0.1:$PORT/health" 2>/dev/null || true)"
    case "$health" in *'"remote":true'*) ;; *) sleep 1; continue ;; esac
    if [ -n "${TMP_E2E_REMOTE_AUDIO:-}" ]; then
      case "$health" in
        *'"remoteAudio":true'*) ;;
        *) err "the remote device e2e_server is up without the remote device's audio: $health"; exit 1 ;;
      esac
    fi
    ready=1; break
  fi
  sleep 1
done
[ "$ready" = 1 ] || { err "remote e2e_server not ready after 240 s"; tail -20 "$SERVER_LOG" >&2; exit 1; }
if ! bridge_post '{"cmd":"e2e_mark_seeded","args":{}}' 30 | grep -q '"ok":true'; then
  err "failed to patch the seeded presets into the startup snapshot"; exit 1
fi
log "remote connected — snapshot includes the seeded presets"

# ── 5. the specs ──
# Their exit status is kept, not acted on at once: the capture counters below matter most
# exactly when a spec failed, so they print first and the run exits with the worse verdict.
spec_rc=0
bunx playwright test --config e2e/playwright.remote.config.ts "${SPEC_FILES[@]}" || spec_rc=$?
verdict=$spec_rc

# ── 6. the remote device's captures: any failed job fails the lane, whatever the specs made of it ──
if [ -n "${TMP_E2E_REMOTE_AUDIO:-}" ]; then
  stats="$(curl -fsS -m 10 "http://127.0.0.1:$PORT/remote/audio" 2>/dev/null || true)"
  log "remote captures: ${stats:-unreadable}"
  jobs="$(printf '%s' "$stats" | sed -n 's/.*"jobs":\([0-9][0-9]*\).*/\1/p')"
  failed="$(printf '%s' "$stats" | sed -n 's/.*"failed":\([0-9][0-9]*\).*/\1/p')"
  if [ -z "$jobs" ] || [ -z "$failed" ]; then
    err "could not read the remote device's capture counters"; verdict=1
  elif [ "$failed" != 0 ]; then
    err "$failed remote capture job(s) failed (not engaged, mixed, stalled or refused): see lastError above"
    verdict=1
  elif [ "$jobs" != 0 ]; then
    PASS_NOTE="$jobs re-amp capture(s), every one engaged"
  fi
fi

# Playwright exits 0 when every selected test skipped: count the tests that actually ran.
ran="$(bun -e '
  const s = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).stats ?? {};
  console.log((s.expected ?? 0) + (s.flaky ?? 0) + " " + (s.skipped ?? 0));
' "$TMP_E2E_REMOTE_REPORT" 2>/dev/null || true)"
if [ "$spec_rc" -eq 0 ]; then
  case "$ran" in
    ""|"0 "*)
      err "no test ran on the remote device (${ran#* } skipped, or no report): the selected specs skip in this mode — pick *.online specs"
      verdict=1 ;;
    *) log "remote specs: ${ran% *} ran, ${ran#* } skipped" ;;
  esac
fi
exit "$verdict"
