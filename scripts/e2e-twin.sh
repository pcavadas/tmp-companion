#!/bin/bash
# e2e-twin.sh — the TWIN e2e lane: the online-shaped specs against an offline twin of the
# unit's firmware, reached over `hid::twin::TwinHid`. Never the unit, and never a stamp.
#
#   TMP_E2E_TWIN=tcp:127.0.0.1:11600 scripts/e2e-twin.sh [spec …]    (default: songs copy)
#   TMP_E2E_TWIN=… scripts/e2e-twin.sh --seed-only    stop after step 3, before any spec
#   TMP_E2E_TWIN=… TMP_E2E_TWIN_AUDIO=tcp:127.0.0.1:11700 scripts/e2e-twin.sh level
#                                       with the twin's re-amp audio: offline leveling
#
# --seed-only leaves the twin holding the pristine scenario: snapshot it there. A snapshot
# taken after a run holds what the specs saved (copy.spec saves edits into 401/402), so
# every later seed would re-import those slots.
#
# The twin itself runs outside this repository; this script only needs its address. It:
#   1. builds e2e_server + probe with `--features e2e` into a SEPARATE target dir (under the
#      git common dir, so neither the worktree's stamps nor the production `probe` in
#      src-tauri/target are touched; override with TMP_E2E_TWIN_TARGET);
#   2. empties the twin's scratch zone: a fresh twin's bank is the firmware's factory copy,
#      so the scenario slots hold presets the seed must never overwrite on a unit and
#      refuses. Each is cleared with the name-guarded `probe --clear`, and only after that
#      probe confirmed it is talking to the twin (its `[probe] TWIN:` banner);
#   3. seeds the scenario presets (`probe --seed-scenario`, a fresh process, as online);
#   4. starts the e2e_server and waits for its TWIN phrase, never the online one;
#   5. runs Playwright with e2e/playwright.twin.config.ts, which only adopts that server;
#   6. tears down: re-amp OFF, stray sweep, recall preset 001, server stopped.
#
# Re-amp captures go to the twin's own audio when TMP_E2E_TWIN_AUDIO is set beside the
# twin's address (`twin_audio`: the twin's DSP, resampled to and from its 44.1 kHz). Without
# it a spec that captures fails with "the twin has no re-amp audio here", never a faked
# number. Either way the lane runs against the real firmware offline; it is not device
# evidence (twin levels are not calibrated against the unit) and writes no stamp.
#
# Ports: bridge 12000 + offset*8, Vite 1821 + offset — disjoint from offline (7800-9399 /
# 1421-1620), online (9400-10999 / 1621-1820) and the twin harness's default 11600.
#
# Written to run under macOS system /bin/bash (3.2).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

log() { printf '\033[36m▸ %s\033[0m\n' "$*"; }
err() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }

if [ -z "${TMP_E2E_TWIN:-}" ]; then
  err "TMP_E2E_TWIN is not set: give the twin's address (tcp:<host>:<port> or unix:<path>)"
  exit 2
fi
if [ "${TMP_E2E_ONLINE:-}" = 1 ]; then
  err "TMP_E2E_ONLINE=1 is set: this lane drives the twin, never the unit — unset it"
  exit 2
fi
export TMP_E2E_ONLINE=0

SPECS=()
SEED_ONLY=0
for a in "$@"; do
  case "$a" in
    -h|--help) sed -n '2,37p' "$0"; exit 0 ;;
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
      if [ -z "${TMP_E2E_TWIN_AUDIO:-}" ]; then
        log "note: $s captures audio and TMP_E2E_TWIN_AUDIO is unset — the twin refuses captures, expect it to fail"
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
TARGET="${TMP_E2E_TWIN_TARGET:-$COMMON/tmp-twin-target}"
PROBE="$TARGET/debug/probe"
SERVER="$TARGET/debug/e2e_server"
LOG_DIR="${TMPDIR:-/tmp}/tmp-companion-e2e/twin"
mkdir -p "$LOG_DIR"
SERVER_LOG="$LOG_DIR/e2e_server.log"
PROBE_LOG="$LOG_DIR/probe.log"
: > "$PROBE_LOG"
SERVER_PID=""

[ -f dist/index.html ] || { mkdir -p dist; printf '<!doctype html><title>e2e</title>' > dist/index.html; }
# A stale node_modules surfaces only as Vite's error overlay, so every spec would sit out
# its full timeout on the first click. Install from the lockfile first (a no-op when current).
bun install --frozen-lockfile >/dev/null 2>&1 || {
  err "bun install --frozen-lockfile failed — fix the frontend deps before the twin lane"; exit 1; }

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

# Run probe against the twin, appending its output to $PROBE_LOG and echoing stdout. Fails
# the lane unless probe printed its twin banner: no device op below may run unconfirmed.
twin_probe() {
  local out rc=0
  out="$("$PROBE" "$@" 2>&1)" || rc=$?
  printf '%s\n' "$out" >> "$PROBE_LOG"
  case "$out" in
    *"[probe] TWIN: "*) ;;
    *) err "probe $1 did not confirm it is talking to the twin — refusing (see $PROBE_LOG)"; exit 1 ;;
  esac
  printf '%s\n' "$out"
  return "$rc"
}

cleanup() {
  local code=$?
  [ -z "${1:-}" ] || code="$1"
  trap - EXIT INT TERM
  if curl -fsS -m 5 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    log "teardown: re-amp OFF, stray sweep, recall 001 (on the twin)"
    bridge_post '{"cmd":"e2e_reamp_off","args":{}}' >/dev/null || true
    bridge_post '{"cmd":"e2e_clear_strays","args":{}}' 300 >/dev/null || true
    bridge_post '{"cmd":"e2e_load_preset","args":{"slot":0}}' >/dev/null || true
  fi
  if [ -n "$SERVER_PID" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  kill_port "$PORT"
  if [ "$code" -eq 0 ] && [ "$SEED_ONLY" = 1 ]; then
    log "twin seeded (--seed-only): the scenario is pristine on the twin — snapshot it now"
  elif [ "$code" -eq 0 ]; then
    log "twin lane PASSED (${SPECS[*]}) — control plane only; no stamp written"
  else
    err "twin lane FAILED (exit $code) — server log: $SERVER_LOG, probe log: $PROBE_LOG"
  fi
  exit "$code"
}
trap cleanup EXIT
trap 'cleanup 130' INT
trap 'cleanup 143' TERM

log "twin lane: $TMP_E2E_TWIN · audio ${TMP_E2E_TWIN_AUDIO:-none} · bridge :$PORT · vite :$VITE_PORT · target $TARGET"
log "building e2e_server + probe (--features e2e, separate target dir)…"
CARGO_TARGET_DIR="$TARGET" cargo build -q --manifest-path src-tauri/Cargo.toml \
  --features e2e --bin e2e_server --bin probe

# ── 2. empty the twin's scratch zone of presets the seed does not own ──
# The scenario's slots and names, from the fixture file itself (the same parse as
# scripts/e2e.sh's recovery: `"listIndex": N,` then `"name": "…",`).
scenario_pairs="$(awk '
  /"listIndex"/ { if (match($0, /[0-9]+/)) idx = substr($0, RSTART, RLENGTH); next }
  /"name"/ { if (idx != "" && match($0, /"name"[ \t]*:[ \t]*"[^"]*"/)) {
      s = substr($0, RSTART, RLENGTH); sub(/^"name"[ \t]*:[ \t]*"/, "", s); sub(/"$/, "", s)
      print idx "\t" s; idx = "" } }
' e2e/fixtures/scenario-presets.json)"
[ -n "$scenario_pairs" ] || { err "no slot/name pairs in e2e/fixtures/scenario-presets.json"; exit 1; }

log "reading the twin's bank…"
listing="$(twin_probe --listall)" || {
  err "reading the twin's bank failed: $(grep "FAILED" "$PROBE_LOG" | tail -1)"; exit 1; }
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
  log "twin scratch slot $sidx holds a preset the seed does not own — clearing it (twin only)"
  out="$(twin_probe --clear "$sidx" "$cur")"
  case "$out" in
    *"cleared"*) cleared=$((cleared + 1)) ;;
    *) err "clearing twin slot $sidx did not report a clear — refusing to go on"; exit 1 ;;
  esac
done <<EOF
$scenario_pairs
EOF
log "scratch zone ready ($cleared slot(s) cleared)"

# ── 3. seed, in a fresh probe process ──
seeded=0
for attempt in 1 2 3; do
  log "seeding the scenario presets on the twin (attempt $attempt)…"
  if twin_probe --seed-scenario >/dev/null; then seeded=1; break; fi
  sleep 2
done
[ "$seeded" = 1 ] || {
  err "scenario seed failed 3 times: $(grep "FAILED" "$PROBE_LOG" | tail -1)"; exit 1; }
if [ "$SEED_ONLY" = 1 ]; then exit 0; fi

# ── 4. the server ──
for i in 0 1 2 3 4 5 6 7; do kill_port $((PORT + i)); done
log "starting the twin e2e_server on :$PORT"
"$SERVER" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true
ready=0
for _ in $(seq 1 240); do
  if grep -q "twin handshake failed\|REFUSING TO START" "$SERVER_LOG" 2>/dev/null; then
    err "$(grep "twin handshake failed\|REFUSING TO START" "$SERVER_LOG" | tail -1)"; exit 1
  fi
  if grep -q "TWIN — seeded snapshot from the twin" "$SERVER_LOG" 2>/dev/null; then
    health="$(curl -fsS -m 5 "http://127.0.0.1:$PORT/health" 2>/dev/null || true)"
    case "$health" in *'"twin":true'*) ;; *) sleep 1; continue ;; esac
    if [ -n "${TMP_E2E_TWIN_AUDIO:-}" ]; then
      case "$health" in
        *'"twinAudio":true'*) ;;
        *) err "the twin e2e_server is up without the twin's audio: $health"; exit 1 ;;
      esac
    fi
    ready=1; break
  fi
  sleep 1
done
[ "$ready" = 1 ] || { err "twin e2e_server not ready after 240 s"; tail -20 "$SERVER_LOG" >&2; exit 1; }
if ! bridge_post '{"cmd":"e2e_mark_seeded","args":{}}' 30 | grep -q '"ok":true'; then
  err "failed to patch the seeded presets into the startup snapshot"; exit 1
fi
log "twin connected — snapshot includes the seeded presets"

# ── 5. the specs ──
bunx playwright test --config e2e/playwright.twin.config.ts "${SPEC_FILES[@]}"
