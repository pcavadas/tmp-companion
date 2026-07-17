#!/usr/bin/env bash
#
# Turn-key dual-mode Playwright e2e runner for tmp-companion.
#
#   scripts/e2e.sh                  # OFFLINE (SimDevice) — fast, default, no hardware (~1.5 min)
#   scripts/e2e.sh offline copy     # OFFLINE, only the copy spec
#   scripts/e2e.sh online           # ONLINE (real device) — songs, copy, level in turn (~9 min)
#   scripts/e2e.sh online level     # ONLINE, only the level spec
#   scripts/e2e.sh online all       # ONLINE, the full set (= the default online set)
#
# OFFLINE is a near-passthrough to `playwright test` (Playwright starts/stops its own
# SimDevice e2e_server + vite). ONLINE is fully managed: it pre-flights the device via a
# handshake-verified server start, runs each heavy spec in its own invocation (the device is
# exclusive-seize and the level spec is two ~3-min tests), and ALWAYS recovers the unit on
# exit — reamp-off + guarded scratch-clear (400/401/402) + recall 001 — even on Ctrl-C or a
# failed/killed run (a killed level run otherwise strands the unit re-amp-engaged / input-muted).
#
# Both modes first kill any stale e2e_server on :7600, so neither can silently reuse a
# wrong-mode server (`reuseExistingServer: true` would otherwise make an "online" run hit
# SimDevice — a false-green pass — or vice-versa), and pre-build the binary so the cold compile
# is out of the timed path (it would otherwise blow the config's 180 s webServer timeout).
#
# Preconditions for ONLINE: the unit plugged in + RESTED, and Pro Control CLOSED (it holds the
# exclusive HID seize). A handshake failure is reported with that hint.
#
# Written to run under macOS system /bin/bash (3.2) — note the empty-array `set -u` guards.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

PORT=7600
OFFLINE_CFG="e2e/playwright.config.ts"
ONLINE_CFG="e2e/playwright.online.config.ts"
MANIFEST="src-tauri/Cargo.toml"
LOG_DIR="${TMPDIR:-/tmp}/tmp-companion-e2e"
SERVER_LOG="$LOG_DIR/e2e_server.log"
mkdir -p "$LOG_DIR"

# ── parse args: a mode token (online|offline) + zero or more spec names (copy|level|songs|all) ──
MODE="offline"
SPECS=()
for a in "$@"; do
  case "$a" in
    online|offline) MODE="$a" ;;
    -h|--help)
      cat >&2 <<'USAGE'
Usage: scripts/e2e.sh [online|offline] [copy|level|songs|doctor|all ...]
  (no args)        OFFLINE — all specs vs SimDevice (fast, ~1.5 min, no hardware)
  offline copy     OFFLINE — only the copy spec
  online           ONLINE  — songs, copy, level vs the real unit (~9 min; Pro Control closed)
  online level     ONLINE  — only the level spec
USAGE
      exit 0 ;;
    *) SPECS+=("$a") ;;
  esac
done

SERVER_PID=""

# ── helpers ───────────────────────────────────────────────────────────────────
log() { printf '\033[36m▸ %s\033[0m\n' "$*"; }
err() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }

# A real build (cargo for e2e_server) panics in tauri-build if ./dist is absent (gitignored,
# missing in a fresh worktree). A stub index.html is enough — the live UI is served by vite.
ensure_dist() { [ -f dist/index.html ] || { mkdir -p dist; printf '<!doctype html><title>e2e</title>' > dist/index.html; }; }

# Build the e2e_server up front so the (potentially minutes-long, cold) compile is out of the
# timed server-start path; its exit code is the build check.
prebuild() {
  log "building e2e_server (incremental)…"
  cargo build -q --manifest-path "$MANIFEST" --features e2e --bin e2e_server \
    || { err "e2e_server build failed"; exit 1; }
}

kill_port() { lsof -ti "tcp:$1" 2>/dev/null | xargs kill 2>/dev/null || true; }

bridge_post() { # $1 = JSON body, $2 = timeout (s, default 60); echoes the response body
  curl -fsS -m "${2:-60}" -X POST "http://127.0.0.1:$PORT/invoke" \
    -H 'content-type: application/json' -d "$1" 2>/dev/null
}

post() { # POST one /invoke command, best-effort (recovery must never fail the script)
  bridge_post "$1" "${2:-60}" >/dev/null 2>&1 || true
}

recover_device() {
  curl -fsS -m 5 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 || return 0  # server gone, nothing to do
  log "recovering device — reamp-off + guarded scratch-clear + recall 001"
  post '{"cmd":"e2e_reamp_off","args":{}}'
  post '{"cmd":"e2e_clear_preset","args":{"slot":400,"expectName":"E2E Reference"}}'
  post '{"cmd":"e2e_clear_preset","args":{"slot":401,"expectName":"E2E Target 1"}}'
  post '{"cmd":"e2e_clear_preset","args":{"slot":402,"expectName":"E2E Target 2"}}'
  # Sweep stray scenario imports an aborted seed stranded elsewhere in the bank
  # (imports land at the first EMPTY slot anywhere; guarded, fail-closed). Long
  # timeout: N strays × clear can exceed the default 60 s cap.
  post '{"cmd":"e2e_clear_strays","args":{}}' 300
  post '{"cmd":"e2e_load_preset","args":{"slot":0}}'
}

cleanup() { # ONLINE only (offline execs Playwright, which owns teardown + has no device)
  local code=$?
  trap - EXIT INT TERM
  recover_device                          # guarded-clear is fail-closed: never touches a real preset
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  kill_port "$PORT"                       # cargo run spawns the binary as a child — kill by port too
  exit "$code"
}

# Wait until the managed ONLINE server prints its ready line AND answers /health, surfacing a
# device-handshake failure with an actionable hint. (Build failures are already caught by prebuild.)
wait_server_ready() {
  local i
  for i in $(seq 1 240); do
    handshake_err=$(grep "device handshake failed" "$SERVER_LOG" 2>/dev/null | tail -1 || true)
    if [ -n "$handshake_err" ]; then
      err "$handshake_err"
      err "  → is the unit plugged in and Pro Control closed? (Pro Control holds the exclusive HID seize)"
      exit 1
    fi
    if grep -q "seeded snapshot from the real device" "$SERVER_LOG" 2>/dev/null \
       && curl -fsS -m 5 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  err "e2e_server not ready after 240s:"; tail -20 "$SERVER_LOG" >&2; exit 1
}

# ── always clear a stale :7600 first (the fake-mode-reuse guard), then warm the binary ──
kill_port "$PORT"
ensure_dist
prebuild

# ── OFFLINE: fast path — let Playwright own the (SimDevice) server + vite lifecycle ──
if [ "$MODE" = offline ]; then
  log "OFFLINE e2e (SimDevice) — Playwright manages servers"
  # Build the spec-file args in the positional params ("$@" is empty-safe under `set -u`,
  # unlike an empty array — avoids a two-arm exec).
  set --
  for s in "${SPECS[@]:-}"; do [ -n "$s" ] && [ "$s" != all ] && set -- "$@" "specs/$s.spec.ts"; done
  exec bunx playwright test --config "$OFFLINE_CFG" "$@"
fi

# ── ONLINE: managed — handshake-verified start, per-spec runs, guaranteed recovery ──
trap cleanup EXIT INT TERM

# Resolve the spec set: empty (→ "  ") OR `all` → the full ordered set (light → heavy).
case " ${SPECS[*]:-} " in *" all "*|"  ") SPECS=(songs copy level) ;; esac

log "ONLINE e2e (real device) — starting handshake-verified server on :$PORT"
: > "$SERVER_LOG"
TMP_E2E_ONLINE=1 TMP_E2E_PORT="$PORT" \
  cargo run -q --manifest-path "$MANIFEST" --features e2e --bin e2e_server \
  >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true  # silence the shell's "Terminated" notice when cleanup kills it
wait_server_ready
log "device connected — snapshot seeded"

# Seed the scenario presets from the RUNNER (not the spec): the seed is the first fresh
# HID open after the server handshake and can hit the device's capricious open lockout —
# a retry with a long quiet rest cures it, but Playwright's per-test budget (300 s) can't
# absorb seed (~90–150 s) + retries. The seed self-repairs (sweeps stray imports from any
# earlier aborted run) so retrying is pollution-safe; the specs' `ensureScenario` stays as
# the idempotent fallback (it finds the presets present and skips).
seed_scenario() {
  bridge_post '{"cmd":"e2e_seed_scenario","args":{}}' 420 | grep -q '"ok":true'
}

fail=0
first=1
for s in "${SPECS[@]}"; do
  if [ "$first" -eq 1 ]; then
    # The server-start handshake arms the device's post-close open LOCKOUT (tens of
    # seconds; hid.rs's own retries reset it instead of riding it out — HW-measured:
    # a seed ~5 s after server-ready fails through 41 s of ladder retries, while a
    # 60 s true quiet then opens in seconds). Rest BEFORE the first spec's seed.
    log "resting the unit before the first spec (post-handshake open lockout)…"
    sleep 60
  else
    log "resting the unit between specs…"
    sleep 12
  fi
  first=0
  seeded=0
  for attempt in 1 2 3; do
    log "seeding the scenario presets (attempt $attempt)…"
    if seed_scenario; then seeded=1; break; fi
    log "seed attempt $attempt failed — resting 60 s (open lockout) before retry"
    sleep 60
  done
  if [ "$seeded" -ne 1 ]; then
    err "scenario seed failed after 3 attempts — aborting (device recovery runs on exit)"
    fail=1
    break
  fi
  log "running specs/$s.spec.ts (online)"
  # No outer timeout: Playwright's own 300 s/test governs; a short wrapper would kill it mid-run.
  if bunx playwright test --config "$ONLINE_CFG" "specs/$s.spec.ts"; then
    log "specs/$s.spec.ts PASSED"
  else
    err "specs/$s.spec.ts FAILED"; fail=1
  fi
done

[ "$fail" -eq 0 ] && log "all online specs passed" || err "one or more online specs failed"
exit "$fail"
