#!/usr/bin/env bash
#
# pre-release.sh — the release-time online run. PRs run the offline gates only; the online
# suite runs here, on the unit, once per release, over every device-facing change merged
# since the last release tag.
#
#   scripts/pre-release.sh          the specs the range requires (`e2e.sh online release`)
#   scripts/pre-release.sh all      the full online set, whatever the range requires
#
# Run it on an up-to-date `main` checkout with the unit plugged in and Pro Control closed,
# before starting the release workflow. It first prints the release report
# (`scripts/gates.sh --device-changes`): each merged commit since the last tag that touched
# device-facing code, with its files and their gate-map classes, so a red spec points
# straight at its likely cause. Then it runs the online lane with TMP_E2E_REQUIRE_STOCK=1 (a
# unit without its stock USB interfaces is refused before anything is seeded).
#
# Interrupt with Ctrl-C (SIGINT), never SIGKILL: e2e.sh's trap recovers the unit (re-amp
# OFF, recall 001), kills its server and releases the device lock. bash 3.2-safe.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

# shellcheck source=scripts/device-lock.sh disable=SC1091
. "$REPO/scripts/device-lock.sh"
# shellcheck source=scripts/lib/e2e-lane.sh disable=SC1091
. "$REPO/scripts/lib/e2e-lane.sh"

log() { printf '\033[36m▸ pre-release: %s\033[0m\n' "$*"; }
err() { printf '\033[31m✗ pre-release: %s\033[0m\n' "$*" >&2; }

SET="release"
case "${1:-}" in
  '') ;;
  all) SET="all" ;;
  *) err "usage: scripts/pre-release.sh [all]"; exit 2 ;;
esac

if ! BASE="$(bash scripts/gates.sh --release-base)"; then
  err "no release tag (v*) behind HEAD"
  exit 1
fi

# ── 1. the release report ──
log "device-facing changes merged since $BASE:"
bash scripts/gates.sh --device-changes --since "$BASE"

# ── 2. preflight ──
if [ "$SET" = all ]; then
  REQ="$(bash scripts/gates.sh --online-spec-set)"
else
  REQ="$(bash scripts/gates.sh --required-online --since "$BASE" 2>/dev/null)"
fi
if [ -z "$REQ" ]; then
  log "nothing merged since $BASE reaches the device — no online run needed"
  exit 0
fi
case " $(printf '%s' "$REQ" | tr '\n' ' ') " in
  *" level.online "*)
    if ! command -v ffmpeg >/dev/null 2>&1; then
      err "ffmpeg is not on PATH — level.online cannot pass without the external judge"
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
[ -f dist/index.html ] || { err "dist/ is missing — run bun run build first"; exit 1; }

# ── 3. the online lane, in the foreground (Ctrl-C reaches its recovery trap) ──
log "running the online lane: e2e.sh online $SET"
TMP_E2E_REQUIRE_STOCK=1 /bin/bash scripts/e2e.sh online "$SET"
log "online lane green since $BASE — start the release workflow"
