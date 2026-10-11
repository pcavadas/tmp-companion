#!/usr/bin/env bash
#
# pre-pr.sh — run the offline gates and the online lane SIDE BY SIDE in this worktree, so a
# device-facing PR waits for the longer of the two instead of their sum. One verdict.
#
#   scripts/pre-pr.sh
#
# The online lane is `scripts/e2e.sh online auto`: exactly the specs this diff requires
# (`scripts/gates.sh --required-online`), on its own port band, so its stale-server sweep
# and its Vite never touch the offline lane's. Each lane writes its own stamp for the same
# tree key. gates.sh itself is unchanged: the pre-push hook still calls it alone.
#
# Order: preflight → one prebuild → online lane in the background → wait until it is seeded
# and serving (its build and seed finish before the host gets loaded) → `nice gates.sh` with
# two Playwright workers in the foreground. If gates.sh fails, the online run gets SIGINT —
# never SIGKILL, which skips its trap and strands the unit input-muted — and its trap
# recovers the unit (re-amp OFF, recall 001), kills its server and releases the device lock.
#
# The online lane runs with TMP_E2E_REQUIRE_STOCK=1: a unit without its stock USB interfaces
# is refused before anything is seeded. Its host-silence count (OUT gaps above 500 ms and
# TrackedHid auto-reopens) lands on the stamp's #run line, for comparison with quiet runs.
#
# Test seam: TMP_PRE_PR_GATES_CMD replaces the gates command, to prove the failing-gate path
# on the unit without a tracked edit (which would re-key the tree): `false` stops the online
# lane before its first spec, `sleep 150; false` stops it inside one.
#
# Edit nothing tracked while it runs: a non-prose change moves the key and the online lane
# then refuses to stamp. bash 3.2-safe (macOS /bin/bash).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

# shellcheck source=scripts/device-lock.sh disable=SC1091
. "$REPO/scripts/device-lock.sh"
# shellcheck source=scripts/lib/e2e-lane.sh disable=SC1091
. "$REPO/scripts/lib/e2e-lane.sh"

log() { printf '\033[36m▸ pre-pr: %s\033[0m\n' "$*"; }
err() { printf '\033[31m✗ pre-pr: %s\033[0m\n' "$*" >&2; }

LOG_DIR="${TMPDIR:-/tmp}/tmp-companion-e2e"
ONLINE_LOG="$LOG_DIR/pre-pr-online.log"
mkdir -p "$LOG_DIR"
t0=$(date +%s)

# ── 1. the key both lanes must stamp ──
KEY="$(bash scripts/gates.sh --key)"
log "tree key $KEY"

# ── 2. preflight ──
REQ="$(bash scripts/gates.sh --required-online)"
log "online specs this diff requires: $(printf '%s' "${REQ:-none}" | tr '\n' ' ')"
if [ -n "$REQ" ]; then
  case " $(printf '%s' "$REQ" | tr '\n' ' ') " in
    *" level.online "*)
      if ! command -v ffmpeg >/dev/null 2>&1; then
        err "ffmpeg is not on PATH — level.online cannot stamp without the external judge"
        exit 1
      fi ;;
  esac
  if [ -d "$DEVICE_LOCK_DIR" ]; then
    err "the device lock is held ($(_device_lock_field owner)) — stop that run first"
    exit 1
  fi
  UNIT="$(lane_unit_state)"
  if [ "$UNIT" != stock ]; then
    err "unit=$UNIT — the unit must present its stock USB interfaces"
    exit 1
  fi
fi
[ -f dist/index.html ] || { err "dist/ is missing — run bun run build first"; exit 1; }

# ── 3. one prebuild, so neither lane compiles while the other is timed ──
log "building e2e_server + probe (incremental)…"
cargo build -q --manifest-path src-tauri/Cargo.toml --features e2e --bin e2e_server --bin probe

# ── 4. the online lane, in the background ──
# Job control on: an async job of a non-interactive shell otherwise starts with SIGINT
# IGNORED, and a signal ignored on entry cannot be trapped — e2e.sh's recovery trap would
# never run. Signal the e2e.sh PID alone, never its process group: the group holds the
# e2e_server its trap needs alive to switch re-amp OFF.
ONLINE_PID=""
online_rc=0
if [ -n "$REQ" ]; then
  set -m
  TMP_E2E_REQUIRE_STOCK=1 \
    /bin/bash scripts/e2e.sh online auto >"$ONLINE_LOG" 2>&1 &
  ONLINE_PID=$!
  set +m
  log "online lane started (pid $ONLINE_PID, log $ONLINE_LOG)"
fi

# shellcheck disable=SC2329  # invoked via trap
stop_online() {
  if [ -n "$ONLINE_PID" ] && kill -0 "$ONLINE_PID" 2>/dev/null; then
    log "sending SIGINT to the online lane; waiting for it to recover the unit…"
    kill -INT "$ONLINE_PID" 2>/dev/null || true
    wait "$ONLINE_PID" 2>/dev/null || true
  fi
}
trap 'stop_online; exit 130' INT TERM

# ── 5. wait until it is seeded and serving, or finished ──
if [ -n "$ONLINE_PID" ]; then
  while kill -0 "$ONLINE_PID" 2>/dev/null; do
    if grep -q 'device connected — snapshot includes the seeded presets' "$ONLINE_LOG" 2>/dev/null; then
      log "online lane is seeded and serving — starting the offline gates"
      break
    fi
    sleep 2
  done
fi
t_online_ready=$(date +%s)

# ── 6. the offline gates, niced, in the foreground ──
gates_rc=0
if [ -n "${TMP_PRE_PR_GATES_CMD:-}" ]; then
  log "TMP_PRE_PR_GATES_CMD set — running '$TMP_PRE_PR_GATES_CMD' instead of the gates"
  sh -c "$TMP_PRE_PR_GATES_CMD" || gates_rc=$?
else
  nice -n 10 env TMP_E2E_WORKERS=2 /bin/bash scripts/gates.sh || gates_rc=$?
fi
t_gates=$(date +%s)

# ── 7./8. one verdict ──
if [ "$gates_rc" -ne 0 ]; then
  err "offline gates failed (exit $gates_rc)"
  stop_online
  trap - INT TERM
  exit 1
fi
if [ -n "$ONLINE_PID" ]; then
  wait "$ONLINE_PID" || online_rc=$?
fi
trap - INT TERM
t_end=$(date +%s)
log "wall clock: gates $((t_gates - t_online_ready))s after the online lane was ready; total $((t_end - t0))s"
if [ -n "$ONLINE_PID" ]; then
  grep -E 'wall time:|headroom:|NOT stamped|recording the online stamp|online stamp recorded' "$ONLINE_LOG" || true
fi
if [ "$online_rc" -ne 0 ]; then
  err "online lane failed (exit $online_rc) — see $ONLINE_LOG"
  exit 1
fi
if [ -n "$REQ" ] && ! bash scripts/gates.sh --check-online-diff; then
  err "the online lane passed but its stamp does not cover this diff — see $ONLINE_LOG"
  exit 1
fi
log "both lanes green for tree $KEY"
