#!/usr/bin/env bash
# Tier-3 hardware end-to-end smoke for the TMP Companion — drives the REAL Tone Master
# Pro over USB and asserts the Level and Copy happy paths work end-to-end.
#
# ATTENDED + NON-DESTRUCTIVE BY DEFAULT. Every phase is read-only or a DRY run (the Level
# pass measures + solves but does NOT save; the Copy pass confirms the live block replace
# but does NOT commit), so a green run writes NOTHING to the device. Run it with the unit
# plugged in and Pro Control CLOSED (exclusive HID seize). It is a manual re-validation
# harness, NOT a CI gate — the software tiers (Vitest + `cargo test --lib`) are the gate.
#
# The Copy phase references a specific preset by device slot + the two block ids to swap.
# The defaults below match the development unit ("Bassmans Comparison" at device slot 12,
# an amp→amp swap between two Bassman variants already present in that preset). Override
# them for a different unit via the env vars; pick a preset that exists on YOUR device and
# a same-category swap (amp→amp / pedal→pedal) so the device confirms instead of rejecting.
# Discover candidates from a backup: `cargo run --bin probe -- --device-backup`.
#
# Usage:
#   bash scripts/hw-e2e.sh
#   LEVEL_SLOT=3 COPY_SLOT=15 COPY_FROM=... COPY_TO=... bash .../hw-e2e.sh
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_TAURI="$(cd "$SCRIPT_DIR/../src-tauri" && pwd)"
cd "$SRC_TAURI" || exit 1

# Serialize the ONE Tone Master Pro across sessions/worktrees (same machine-global lock the
# online e2e runner uses) — wait if a sibling run holds it, release on exit.
# shellcheck source=scripts/device-lock.sh disable=SC1091
. "$SCRIPT_DIR/device-lock.sh"
device_lock_acquire "$SCRIPT_DIR (hw-e2e)" || exit 1
trap device_lock_release EXIT INT TERM

# Tunables (override via env). Defaults are the dev unit's; see header.
LEVEL_SLOT="${LEVEL_SLOT:-0}"
LEVEL_TARGET="${LEVEL_TARGET:--30}"
TOPOLOGY="${TOPOLOGY:-guitar-humbucker}"
LEVEL_TOL_LU="${LEVEL_TOL_LU:-1.0}"
COPY_SLOT="${COPY_SLOT:-12}"
COPY_FROM="${COPY_FROM:-ACD_TM59Bassman}"
COPY_TO="${COPY_TO:-ACD_TMCust59Bassman}"

OUT="${OUT_DIR:-$(mktemp -d)}"
mkdir -p "$OUT"
PASS=0
FAIL=0
say() { printf '%s\n' "$*"; }
ok()   { PASS=$((PASS+1)); say "  ✓ $1"; }
bad()  { FAIL=$((FAIL+1)); say "  ✗ $1"; }
gap()  { sleep 2; }  # leave the HID line quiet between opens (open-lockout hygiene)

say "=== TMP Companion — Tier-3 hardware e2e (non-destructive) ==="
say "src-tauri: $SRC_TAURI"
say "artifacts: $OUT"

# Build once so the per-phase `cargo run` lines don't interleave a compile with HID timing.
say "[build] cargo build --bin probe…"
if ! cargo build --bin probe >"$OUT/build.txt" 2>&1; then
  say "  ✗ probe build failed:"; tail -15 "$OUT/build.txt"; exit 1
fi

# ── Phase 0: connect + firmware (read-only) ──────────────────────────────────
say "[0] connect + firmware"
cargo run --quiet --bin probe -- --fw >"$OUT/fw.txt" 2>&1
FW="$(grep -Eo '^[0-9]+\.[0-9]+\.[0-9]+' "$OUT/fw.txt" | head -1)"
if [ -n "$FW" ]; then ok "device connected — firmware $FW"; else bad "no device / no firmware (see $OUT/fw.txt)"; say "ABORT"; exit 1; fi
gap

# ── Phase 1: full-library read via device backup (read-only) ─────────────────
say "[1] read path — device backup"
cargo run --quiet --bin probe -- --device-backup >"$OUT/backup.txt" 2>&1
ROWS="$(grep -Eo 'UserPresets rows: [0-9]+' "$OUT/backup.txt" | grep -Eo '[0-9]+' | head -1)"
if [ -n "${ROWS:-}" ] && [ "$ROWS" -gt 0 ]; then ok "library read — $ROWS preset rows"; else bad "backup read failed (see $OUT/backup.txt)"; fi
gap

# ── Phase 2: Level happy path — full re-amp pipeline, NOT saved ───────────────
say "[2] Level — measure → solve → verify (save=false)"
TMP_LEVELLER_STIMULUS="$SRC_TAURI/resources/samples/${TOPOLOGY}.wav" \
  cargo run --quiet --bin probe -- --levelpreset "$LEVEL_SLOT" "$LEVEL_TARGET" >"$OUT/level.txt" 2>&1
ERR="$(grep -Eo 'err [+-][0-9.]+ LU' "$OUT/level.txt" | grep -Eo '[+-][0-9.]+' | head -1)"
if [ -n "${ERR:-}" ]; then
  ABS="$(awk -v e="$ERR" 'BEGIN{printf "%.3f", (e<0?-e:e)}')"
  WITHIN="$(awk -v a="$ABS" -v t="$LEVEL_TOL_LU" 'BEGIN{print (a<=t)?1:0}')"
  if [ "$WITHIN" = "1" ]; then ok "leveled to ${LEVEL_TARGET} LUFS (verify err ${ERR} LU ≤ ${LEVEL_TOL_LU})";
  else bad "verify err ${ERR} LU exceeds ${LEVEL_TOL_LU} (see $OUT/level.txt)"; fi
else bad "Level pipeline produced no verify line (see $OUT/level.txt)"; fi
gap

# ── Phase 3: Copy happy path — live block replace confirmed, NOT committed ────
say "[3] Copy — held-session replaceNode (DRY, not saved)"
cargo run --quiet --bin probe -- --replace-held "$COPY_FROM" "$COPY_TO" "$COPY_SLOT" >"$OUT/copy.txt" 2>&1
if grep -q "DRY RUN" "$OUT/copy.txt" && grep -Eq 'updated \([0-9]+ block' "$OUT/copy.txt"; then
  ok "live replace ${COPY_FROM} → ${COPY_TO} confirmed on slot ${COPY_SLOT} (nodeReplaced, not saved)"
else bad "Copy live-edit not confirmed (see $OUT/copy.txt)"; fi
gap

# ── Phase 4: safety — force re-amp off ───────────────────────────────────────
say "[4] safety — re-amp off"
cargo run --quiet --bin probe -- --reamp-off >"$OUT/reamp.txt" 2>&1
if grep -q "re-amp OFF sent OK" "$OUT/reamp.txt"; then ok "re-amp disengaged"; else bad "re-amp-off not confirmed (see $OUT/reamp.txt)"; fi

say ""
say "=== RESULT: ${PASS} passed, ${FAIL} failed ==="
[ "$FAIL" -eq 0 ]
