#!/usr/bin/env bash
#
# validate-hbe.sh — P5's attended, standalone external-validation run (the Friedman HBE
# case). An ORCHESTRATION WRAPPER over exactly the same contract the online e2e lane
# uses: it drives `probe`, the probe's measurement seams emit the JSON-lines expectation
# log (`src-tauri/src/validate_log.rs`), and `scripts/level-validate.sh --expectations`
# judges those WAVs with ffmpeg. The operator never re-types a scene→target mapping —
# the log already carries it.
#
# Given a `.preset` FILE and a SCRATCH slot it:
#   1. refuses to run without the device present (a read-only `probe --fw` connect check);
#   2. imports the file into the slot (`probe --import-file` — refuses an OCCUPIED target
#      on its own, so this is safe even if the slot still holds an e2e fixture);
#   3. confirms the import with a NON-DESTRUCTIVE read (`probe --slot-json`) and keeps the
#      preset's OWN name — the guard the final clear step uses, in the SAME 0-based
#      list-index address space `--clear` acts in (`.claude/rules/danger.md`'s "same
#      address space" rule: the read and the destructive op must agree on which space
#      they're addressing, not just which number);
#   4. levels the base (`probe --levelpreset … save`), optionally the FS scenes
#      (`probe --level-preset-scenes`) and optionally block-acting footswitches
#      (`probe --level-footswitch … --commit`);
#   5. WAITS OUT THE COMMIT WINDOW (see COMMIT_WINDOW_WAIT below) after every save
#      and before the next load — mandatory, not politeness;
#   6. re-measures each leveled foundation, EMITTING one expectation row + WAV per sound:
#      base/scenes via `probe --measure-scene … --target … --dump-wav`, footswitches via
#      `probe --measure-footswitch … --target … --dump-wav` (P5 closed that hole; FS rows
#      are now externally validated like every other row);
#   7. hands the whole log to `scripts/level-validate.sh --expectations` and branches its
#      0/1/3 exit explicitly — a SKIP is reported as SKIPPED, never as a target miss;
#   8. clears the scratch slot back to empty (`probe --clear`, name-guarded — skip with
#      `--no-clear` to leave the imported preset for inspection);
#   9. GUARANTEED re-amp OFF on a fresh connection, via a trap that runs on every exit
#      path (success, a failed step, Ctrl-C) — `.claude/rules/danger.md`: "a dropped OFF
#      strands the unit input-muted".
#
# WHY THE COMMIT-WINDOW WAIT IS NOT OPTIONAL: `saveCurrentPreset` commits LAZILY on the
# real TMP — the write materializes T+45–100 s later, and a same-slot `loadPreset` inside
# that window materializes the PRE-save preset (`.claude/rules/danger.md`, HW-reproduced
# fw 1.8.45). The app's in-process `ensure_fresh_load` barrier cannot help here: each
# `probe` invocation is a FRESH PROCESS whose `SLOT_SAVE_REGISTRY` is empty, so it has
# nothing to wait on and would happily load stale bytes. Every re-measure in step 6
# begins with a load, so without this wait the whole validation reads the PRE-leveling
# preset and fails a perfectly correct run. The script waits before EVERY load that
# follows a save — each leveling step below loads the slot — and once more before the
# re-measures.
#
# Which probe flags this script uses: `--fw` (connect check), `--import-file`
# (occupied-target-safe import), `--slot-json` (non-destructive confirm read), `--fs-list`
# (read-only footswitch census, informational — logged so the operator can pick
# `--footswitch` handles from real device output rather than guessing), `--levelpreset`
# (base), `--level-preset-scenes` (FS scenes, Name=Target overrides), `--level-footswitch
# … --commit` (one block-acting footswitch's engaged state per call), `--measure-scene …
# --target … --dump-wav` and `--measure-footswitch … --target … --dump-wav` (the
# expectation-emitting re-captures), `--clear` (name-guarded restore), `--reamp-off` (the
# guaranteed-OFF trap).
#
# Usage:
#   scripts/validate-hbe.sh <preset-file> [options]
#
# Options:
#   --slot N                 0-based scratch list index (default 400). MUST be a member of
#                             `SCRATCH_SLOTS` (`probe_api/mod.rs`) — checked before anything
#                             device-touching runs. If it currently holds an e2e fixture,
#                             the import step refuses (safely) rather than clobbering it.
#   --target LUFS             base leveling target (default -23, one of the shipped defaults)
#   --topology ID              stimulus topology id (default guitar-humbucker) — resolves to
#                             src-tauri/resources/samples/<ID>.wav
#   --scene-target LUFS        default target for `--level-preset-scenes`; presence of this
#                             flag is what turns scene leveling ON at all
#   --scene-override NAME=T    (repeatable) per-scene-name target override, passed verbatim
#                             into `--level-preset-scenes`
#   --scene-verify N=T          (repeatable) after scene leveling, externally re-measure wire
#                             scene slot N (0-based) against target T. YOU supply the
#                             name→index mapping (from the `--fs-list`/scene doc info step);
#                             this script does not guess it from the preset JSON.
#   --footswitch SW:G:N:P:T    (repeatable) level footswitch SW's engaged state by driving
#                             group G / node N / param P to hit target T. Each one is also
#                             externally re-measured (`--measure-footswitch --lev G:N:P`).
#   --tol LU                  validation tolerance (default $TMP_E2E_LEVEL_TOL_LU or 1.0 —
#                             see level-validate.sh's TOLERANCE note; it must exceed the
#                             solver's own acceptance band plus recapture noise)
#   --out DIR                 artifact dir for the log, WAVs and probe logs (default mktemp -d)
#   --no-clear                leave the imported preset in the slot instead of clearing it
#
# Exit: 0 = every row PASSed or SKIPped, with at least one row actually MEASURED · 1 = a
#       leveling step failed, a row FAILed or CLAMPED, or an expectation row was never
#       emitted (MISSING) · 2 = usage/precondition error · 3 = validation SKIPPED (ffmpeg absent —
#       nothing was independently checked; the leveling itself still ran) · 4 = validation
#       VACUOUS (every emitted row was clamped/persist-mismatched — nothing was
#       independently verified; the leveling itself still ran and is saved).
#
# BSD userland only (macOS system /bin/bash 3.2): no `timeout(1)`, no GNU flags, no
# `mapfile`/associative arrays — plain indexed arrays only, guarded per `.claude/rules/
# shell-scripts.md`.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_TAURI="$REPO/src-tauri"
cd "$REPO"

log()  { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
err()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }

# shellcheck source=scripts/device-lock.sh disable=SC1091
. "$REPO/scripts/device-lock.sh"

# ── SCRATCH_SLOTS mirror (`src-tauri/src/probe_api/mod.rs`) — the ONE declaration every
# destructive/working-copy-writing probe guard checks; this script must never touch a slot
# outside it. If that Rust constant ever widens, update this line in the SAME commit. ──
SCRATCH_SLOTS="400 401 402 403 404 405 406 407 408 409 410"
BASE_SCENE_SLOT=8   # session::BASE_SCENE_SLOT — the wire scene-slot sentinel for "base"

# The lazy-save commit window, in seconds. Mirrors `leveller::COMMIT_WINDOW_SECS` (150),
# itself the HW-observed 45–100 s worst case plus margin — see the WHY note in this
# script's header and `.claude/rules/danger.md`'s lazy-commit entry. A fresh `probe`
# process cannot consult the in-process save registry, so this wait IS the barrier.
COMMIT_WINDOW_WAIT=150
PENDING_SAVE=0
settle_commit() {
  [ "$PENDING_SAVE" -eq 1 ] || return 0
  log "waiting ${COMMIT_WINDOW_WAIT}s for the device's LAZY save commit before the next load…"
  sleep "$COMMIT_WINDOW_WAIT"
  PENDING_SAVE=0
}

# Print the file's own comment header as the help text. The range ends at the last line
# before `set -euo pipefail`, found at runtime so an edit to the header can never leave
# `--help` truncating mid-sentence.
usage() {
  local last
  last="$(grep -n '^set -euo pipefail' "${BASH_SOURCE[0]}" | head -1 | cut -d: -f1)"
  sed -n "3,$((last - 1))p" "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit 2
}

# ── arg parsing ───────────────────────────────────────────────────────────────────
PRESET_FILE=""
SLOT=400
TARGET="-23"
TOPOLOGY="guitar-humbucker"
SCENE_TARGET=""
SCENE_OVERRIDES=()
SCENE_VERIFY=()
FOOTSWITCHES=()
TOL="${TMP_E2E_LEVEL_TOL_LU:-1.0}"
OUT_DIR=""
NO_CLEAR=0

if [ $# -eq 0 ]; then usage; fi
PRESET_FILE="$1"; shift
case "$PRESET_FILE" in -h|--help) usage ;; esac

while [ $# -gt 0 ]; do
  case "$1" in
    --slot) SLOT="${2:-}"; shift 2 ;;
    --target) TARGET="${2:-}"; shift 2 ;;
    --topology) TOPOLOGY="${2:-}"; shift 2 ;;
    --scene-target) SCENE_TARGET="${2:-}"; shift 2 ;;
    --scene-override) SCENE_OVERRIDES+=("${2:-}"); shift 2 ;;
    --scene-verify) SCENE_VERIFY+=("${2:-}"); shift 2 ;;
    --footswitch) FOOTSWITCHES+=("${2:-}"); shift 2 ;;
    --tol) TOL="${2:-}"; shift 2 ;;
    --out) OUT_DIR="${2:-}"; shift 2 ;;
    --no-clear) NO_CLEAR=1; shift ;;
    -h|--help) usage ;;
    *) err "unrecognized argument: $1"; usage ;;
  esac
done

[ -f "$PRESET_FILE" ] || { err "no such preset file: $PRESET_FILE"; exit 2; }
case " $SCRATCH_SLOTS " in
  *" $SLOT "*) ;;
  *) err "--slot $SLOT is not in SCRATCH_SLOTS ($SCRATCH_SLOTS) — refusing (danger.md: never touch a slot outside the scratch zone)"; exit 2 ;;
esac

STIM_PATH="$SRC_TAURI/resources/samples/$TOPOLOGY.wav"
[ -f "$STIM_PATH" ] || { err "no stimulus WAV for topology '$TOPOLOGY' at $STIM_PATH"; exit 2; }

if [ -z "$OUT_DIR" ]; then
  OUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/validate-hbe.XXXXXX")"
fi
mkdir -p "$OUT_DIR"
log "artifacts: $OUT_DIR"

# The ONE contract shared with the online lane: the probe measurement seams append here.
VALIDATE_LOG="$OUT_DIR/expectations.jsonl"
VALIDATE_WAV_DIR="$OUT_DIR/wavs"
: > "$VALIDATE_LOG"
mkdir -p "$VALIDATE_WAV_DIR"
export TMP_E2E_VALIDATE_LOG="$VALIDATE_LOG"
export TMP_E2E_VALIDATE_WAV_DIR="$VALIDATE_WAV_DIR"

PROBE_BIN="$SRC_TAURI/target/debug/probe"

# Build the probe binary ONCE up front (repro instrumentation build, no e2e feature — an
# `--features e2e` probe FABRICATES every capture) so per-step runs don't interleave a
# compile with HID open timing.
log "[build] cargo build --bin probe…"
( cd "$SRC_TAURI" && cargo build --bin probe ) >"$OUT_DIR/build.log" 2>&1 \
  || { err "probe build failed (see $OUT_DIR/build.log)"; tail -20 "$OUT_DIR/build.log" >&2; exit 1; }
[ -x "$PROBE_BIN" ] || { err "probe binary not found at $PROBE_BIN after build"; exit 1; }

# Run probe FROM src-tauri — `probe_stimulus_path` resolves resources/samples/<id>.wav
# relative to CWD (a documented gotcha: a probe run from elsewhere "can't find its
# stimulus" and reads like a device fault, not a CWD one).
run_probe() {
  ( cd "$SRC_TAURI" && "$PROBE_BIN" "$@" )
}

# HID open-lockout hygiene: leave the line quiet between probe invocations (each is its
# own fresh connection/open). Conservative on purpose — `danger.md`: "every failed open
# attempt appears to RESET the lockout", so a script that hammers retries never recovers.
gap() { sleep 5; }

SCRIPT_LABEL="$REPO (validate-hbe: $PRESET_FILE → slot $SLOT)"
device_lock_acquire "$SCRIPT_LABEL" || exit 1

CONFIRMED_NAME=""
FAILED=0

# ── ONE cleanup path for every exit — success, a failed step, or Ctrl-C. Order matters:
# reamp-off FIRST (the danger.md guarantee — never let a slower step delay it), then the
# name-guarded clear (best-effort: skip if we never got a confirmed name), then release
# the device lock LAST (other waiters should not start until recovery is done). ──
# shellcheck disable=SC2329  # invoked via `trap cleanup EXIT INT TERM`, which shellcheck doesn't count as a use
cleanup() {
  local code=$?
  trap - EXIT INT TERM
  # A gap BEFORE the OFF: the last step may have closed a session moments ago, and a
  # fresh open inside the lockout window fails (and each failed open re-arms it —
  # danger.md). One retry after a LONGER quiet, then a banner; never a retry loop.
  log "cleanup — guaranteed re-amp OFF…"
  sleep 3
  if ! run_probe --reamp-off >"$OUT_DIR/reamp-off.log" 2>&1; then
    err "reamp-off did not confirm — waiting out the HID lockout and retrying ONCE…"
    sleep 30
    if ! run_probe --reamp-off >>"$OUT_DIR/reamp-off.log" 2>&1; then
      err "########################################################################"
      err "#  RE-AMP MAY STILL BE ENGAGED — the unit is INPUT-MUTED until it is"
      err "#  turned off. Run this yourself once the device is quiet:"
      err "#      cargo run --bin probe -- --reamp-off"
      err "#  (log: $OUT_DIR/reamp-off.log)"
      err "########################################################################"
      code=1
    else
      ok "re-amp OFF confirmed on the retry"
    fi
  fi
  if [ "$NO_CLEAR" -eq 0 ] && [ -n "$CONFIRMED_NAME" ]; then
    log "clearing scratch slot $SLOT (expect name: $CONFIRMED_NAME)…"
    run_probe --clear "$SLOT" "$CONFIRMED_NAME" >"$OUT_DIR/clear.log" 2>&1 \
      || err "clear did not confirm (see $OUT_DIR/clear.log) — slot $SLOT may still hold the imported preset"
  elif [ "$NO_CLEAR" -eq 1 ]; then
    log "--no-clear given — leaving slot $SLOT as-is"
  else
    log "no confirmed preset name — skipping clear (nothing was confirmed-imported, or the confirm step never ran)"
  fi
  device_lock_release
  log "artifacts kept at $OUT_DIR"
  exit "$code"
}
trap cleanup EXIT INT TERM

# ── 1. connection check — refuse to run without the device present ──────────────────
log "[1] connection check (probe --fw)…"
FW="$(run_probe --fw 2>"$OUT_DIR/fw.log" || true)"
case "$FW" in
  [0-9]*.[0-9]*.[0-9]*) ok "device connected — firmware $FW" ;;
  *) err "no device responded to --fw (see $OUT_DIR/fw.log) — plug in the TMP and close Pro Control"; exit 1 ;;
esac
gap

# ── 2. import ─────────────────────────────────────────────────────────────────────
log "[2] importing $PRESET_FILE → list index ${SLOT}…"
run_probe --import-file "$PRESET_FILE" "$SLOT" >"$OUT_DIR/import.log" 2>&1 \
  || { err "import failed (see $OUT_DIR/import.log) — the slot may be occupied; pick a free scratch index or clear it first"; exit 1; }
ok "imported (see $OUT_DIR/import.log)"
gap

# ── 3. confirm read — SAME address space (0-based list index → device slot SLOT+1) the
# final --clear acts in, per danger.md's same-address-space guard ──
log "[3] confirm read (probe --slot-json $((SLOT + 1)))…"
run_probe --slot-json "$((SLOT + 1))" >"$OUT_DIR/slot.json" 2>"$OUT_DIR/slot-json.log" \
  || { err "confirm read failed (see $OUT_DIR/slot-json.log)"; exit 1; }
# Prefer `jq -r .info.displayName` (the preset's user-visible name — `info` carries no
# bare "name" field; fw 1.8.45's info keys are author/created_at/displayName/preset_id/
# product_id/source_id/timestamp/version). Fall back to a first-match sed scan if jq
# isn't present; a WRONG name here only makes the final `--clear` guard refuse (safe), it
# can never make it clear the wrong thing — `--clear`'s own expect-name check is the
# actual safety backstop, this is just aiming it correctly.
if command -v jq >/dev/null 2>&1; then
  CONFIRMED_NAME="$(jq -r '.info.displayName // empty' "$OUT_DIR/slot.json" 2>/dev/null)"
else
  CONFIRMED_NAME="$(sed -n 's/.*"displayName" *: *"\([^"]*\)".*/\1/p' "$OUT_DIR/slot.json" | head -1)"
fi
[ -n "$CONFIRMED_NAME" ] || { err "could not read the imported preset's name from $OUT_DIR/slot.json — refusing to proceed (no safe name to guard the later clear with)"; exit 1; }
ok "confirmed slot $SLOT holds \"$CONFIRMED_NAME\""
gap

# ── informational: read-only footswitch census (helps pick --footswitch handles) ──
log "[info] probe --fs-list $SLOT (read-only)…"
run_probe --fs-list "$SLOT" >"$OUT_DIR/fs-list.log" 2>&1 || log "fs-list failed (non-fatal, see $OUT_DIR/fs-list.log)"
gap

# ── 4a. level base ────────────────────────────────────────────────────────────────
log "[4a] leveling base → $TARGET LUFS…"
if TMP_LEVELLER_STIMULUS="$STIM_PATH" run_probe --levelpreset "$SLOT" "$TARGET" save \
  >"$OUT_DIR/level-base.log" 2>&1; then
  PENDING_SAVE=1
else
  err "base leveling failed (see $OUT_DIR/level-base.log)"; FAILED=1
fi
cat "$OUT_DIR/level-base.log"
gap

# ── 4b. level scenes (opt-in via --scene-target) ─────────────────────────────────
# Gated on 4a's $FAILED like steps 5/6: after a failed base level the preset is in an
# unknown state, and every scene/footswitch pass below is a real re-amp engage + save.
if [ "$FAILED" -ne 0 ]; then
  log "[4b] skipping scene leveling — the base leveling step already failed above"
elif [ -n "$SCENE_TARGET" ]; then
  settle_commit
  log "[4b] leveling FS scenes → default $SCENE_TARGET LUFS (${#SCENE_OVERRIDES[@]} override(s))…"
  set -- --level-preset-scenes "$SLOT" "$SCENE_TARGET" "$TOPOLOGY" 1
  for ov in "${SCENE_OVERRIDES[@]:-}"; do
    [ -n "$ov" ] && set -- "$@" "$ov"
  done
  if TMP_LEVELLER_STIMULUS="$STIM_PATH" run_probe "$@" >"$OUT_DIR/level-scenes.log" 2>&1; then
    PENDING_SAVE=1
  else
    err "scene leveling failed (see $OUT_DIR/level-scenes.log)"; FAILED=1
  fi
  cat "$OUT_DIR/level-scenes.log"
  gap
  # Honest-count gate: the enumeration must see every scene the SOURCE preset
  # carries. A field-8 partial read that cuts the scene tail levels fewer scenes
  # than exist — and a missed scene can still PASS its re-measure by coincidence
  # of base leveling — so the count mismatch is itself a failure, independent of
  # the per-row tolerance verdicts. (.preset = XOR-JLD compact JSON; the key is
  # the committed PRESET_XOR_KEY constant in src-tauri/src/backup.rs.)
  if [ "$FAILED" -eq 0 ]; then
    SRC_SCENES="$(python3 - "$PRESET_FILE" <<'PY'
import json, sys
raw = open(sys.argv[1], 'rb').read()
key = b'JLD'
doc = json.loads(bytes(b ^ key[i % 3] for i, b in enumerate(raw)))
print(len(doc.get('scenes', [])))
PY
)" || SRC_SCENES=""
    ENUM_SCENES="$(sed -n 's/^scenes (\([0-9][0-9]*\)).*/\1/p' "$OUT_DIR/level-scenes.log" | head -1)"
    if [ -z "$SRC_SCENES" ] || [ -z "$ENUM_SCENES" ]; then
      err "could not compare scene counts (source='$SRC_SCENES' enumerated='$ENUM_SCENES')"; FAILED=1
    elif [ "$ENUM_SCENES" -ne "$SRC_SCENES" ]; then
      err "scene enumeration saw $ENUM_SCENES scene(s) but the source preset has $SRC_SCENES - an incomplete read leveled fewer scenes than exist"; FAILED=1
    else
      log "[4b] scene count check: enumerated $ENUM_SCENES == source $SRC_SCENES"
    fi
  fi
  # A count match only proves enumeration; a SKIPped row was enumerated and never leveled,
  # and its re-measure can still land in tolerance by luck (HW: Friedman scene 3, 2026-09-02).
  if [ "$FAILED" -eq 0 ] && grep -q '\[SKIP:' "$OUT_DIR/level-scenes.log"; then
    err "a scene row was SKIPped — enumerated but never leveled:"
    grep '\[SKIP:' "$OUT_DIR/level-scenes.log" >&2
    FAILED=1
  fi
else
  log "[4b] no --scene-target given — skipping scene leveling"
fi

# ── 4c. level footswitches (opt-in, repeatable --footswitch SW:G:N:P:T) ─────────────
if [ "$FAILED" -ne 0 ]; then
  log "[4c] skipping footswitch leveling — a leveling step already failed above"
elif [ "${#FOOTSWITCHES[@]}" -gt 0 ]; then
  for fs in "${FOOTSWITCHES[@]}"; do
    sw="${fs%%:*}"; rest="${fs#*:}"
    grp="${rest%%:*}"; rest="${rest#*:}"
    node="${rest%%:*}"; rest="${rest#*:}"
    param="${rest%%:*}"; fstarget="${rest#*:}"
    settle_commit
    log "[4c] leveling footswitch $sw ($grp/$node/$param) → $fstarget LUFS…"
    if TMP_LEVELLER_STIMULUS="$STIM_PATH" run_probe \
      --level-footswitch "$SLOT" "$sw" "$grp" "$node" "$param" "$fstarget" --commit \
      >"$OUT_DIR/level-fs-$sw.log" 2>&1; then
      PENDING_SAVE=1
    else
      err "footswitch $sw leveling failed (see $OUT_DIR/level-fs-$sw.log)"; FAILED=1
    fi
    cat "$OUT_DIR/level-fs-$sw.log"
    gap
  done
else
  log "[4c] no --footswitch given — skipping footswitch leveling"
fi

# ── 5. THE COMMIT-WINDOW WAIT — see the WHY note in this script's header ────────────
if [ "$FAILED" -eq 0 ]; then
  log "[5] (danger.md: a same-slot load inside T+45–100s materializes the PRE-save preset,"
  log "     and a fresh probe process has no in-process save registry to wait on)"
  settle_commit
  ok "commit window elapsed"
fi

# ── 6. re-measure every leveled foundation, emitting expectation rows + WAVs ────────
# Every row this pass ASKS for is recorded here, because a re-measure that dies before
# its capture (device read error, stimulus resolution, a floor-guard `require_live` Err)
# emits NO row at all — and a shorter log is invisible to the judge, which can only grade
# the rows it is handed. Without this ledger a run whose measurements all failed would
# reach step 7 with an empty-but-not-missing log and exit 0. Labels are built to match
# `validate_log::ValidationRow`'s constructors exactly (base/scene/footswitch).
#
# TWO PINS make that prediction safe, and both must hold or every label mismatches and this
# ledger becomes a false red (the danger.md "guard in the wrong address space" shape):
#   · SLOT SPACE — `probe --measure-scene/--measure-footswitch` take a 0-BASED LIST INDEX and
#     hand it to the row verbatim (no +1 anywhere in bin/probe.rs), which is the same space
#     this script's $SLOT is in.
#   · BASE SENTINEL — $BASE_SCENE_SLOT above must equal `session::BASE_SCENE_SLOT` (8); the
#     probe branches on exactly that constant to pick base() over scene(), so a drift would
#     emit `scene:…:scene8` while this predicts `base:…`.
EXPECTED_LABELS=""
expect_row() { EXPECTED_LABELS="$EXPECTED_LABELS$1
"; }

# The label the EMITTER will write for wire scene slot $1 on $SLOT. Not a formatting
# nicety: `validate_log::ValidationRow` branches on `BASE_SCENE_SLOT` and writes
# `base:slotN` for it, `scene:slotN:sceneS` for everything else (`probe --measure-scene`
# hands the slot straight through). The completeness ledger below greps for these strings
# verbatim, so an operator's `--scene-verify 8=T` predicting `scene:…:scene8` produced a
# guaranteed MISSING row — plus a second measure of the base sound writing over the base
# row's own WAV. One function, so the prediction cannot drift from the emitter again.
row_label_for_scene() {
  if [ "$1" -ge "$BASE_SCENE_SLOT" ]; then
    printf 'base:slot%s' "$SLOT"
  else
    printf 'scene:slot%s:scene%s' "$SLOT" "$1"
  fi
}

measure_scene_row() {
  local label="$1" scene="$2" target="$3"
  local mlog="$OUT_DIR/measure-scene-$scene.log"
  expect_row "$label"
  log "[6] re-measuring $label (wire scene $scene) → expectation row…"
  run_probe --measure-scene "$SLOT" "$scene" "$TOPOLOGY" \
    --target "$target" --dump-wav "$VALIDATE_WAV_DIR" \
    >"$mlog" 2>&1 || err "re-measure of $label reported an error (see $mlog) — the row, if emitted, still carries its own engage verdict"
  cat "$mlog"
  gap
}

measure_fs_row() {
  local sw="$1" grp="$2" node="$3" param="$4" target="$5"
  local mlog="$OUT_DIR/measure-fs-$sw.log"
  expect_row "footswitch:slot${SLOT}:switch${sw}"
  log "[6] re-measuring footswitch $sw ($grp/$node/$param) → expectation row…"
  run_probe --measure-footswitch "$SLOT" "$sw" "$TOPOLOGY" \
    --lev "$grp:$node:$param" --target "$target" --dump-wav "$VALIDATE_WAV_DIR" \
    >"$mlog" 2>&1 || err "re-measure of footswitch $sw reported an error (see $mlog)"
  cat "$mlog"
  gap
}

if [ "$FAILED" -eq 0 ]; then
  measure_scene_row "$(row_label_for_scene "$BASE_SCENE_SLOT")" "$BASE_SCENE_SLOT" "$TARGET"
  for sv in "${SCENE_VERIFY[@]:-}"; do
    [ -n "$sv" ] || continue
    sidx="${sv%%=*}"; starget="${sv#*=}"
    case "$sidx" in
      ''|*[!0-9]*)
        err "--scene-verify '$sv' — the scene index must be a non-negative integer"
        exit 2 ;;
    esac
    # `--scene-verify 8=T` names the BASE sound, which the base row above already measured.
    # Re-measuring it would duplicate the label (the ledger greps by identity, so a
    # duplicate masks nothing, but the WAV would be re-dumped) — and worse, it costs a
    # whole extra device capture for a sound already covered. Say so and move on.
    if [ "$sidx" -ge "$BASE_SCENE_SLOT" ]; then
      log "[6] --scene-verify $sidx names the BASE sound (BASE_SCENE_SLOT=$BASE_SCENE_SLOT), already re-measured above — skipping the duplicate"
      continue
    fi
    measure_scene_row "$(row_label_for_scene "$sidx")" "$sidx" "$starget"
  done
  for fs in "${FOOTSWITCHES[@]:-}"; do
    [ -n "$fs" ] || continue
    sw="${fs%%:*}"; rest="${fs#*:}"
    grp="${rest%%:*}"; rest="${rest#*:}"
    node="${rest%%:*}"; rest="${rest#*:}"
    param="${rest%%:*}"; fstarget="${rest#*:}"
    measure_fs_row "$sw" "$grp" "$node" "$param" "$fstarget"
  done
else
  log "[6] skipping the re-measure pass — a leveling step already failed above"
fi

# ── 7. external validation over the emitted log ─────────────────────────────────────
# 7a. COMPLETENESS FIRST. The judge grades the rows it is given and cannot know one is
# absent; a missing row is a re-measure that never produced audio, which is a failure of
# this run, not a clean sheet. Check identity, not just a count — a duplicate label would
# otherwise mask a missing one.
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

EXT_RC=0
if [ "$FAILED" -eq 0 ]; then
  if [ ! -s "$VALIDATE_LOG" ]; then
    err "no expectation rows were emitted — nothing was independently measured"
    EXT_RC=1
  else
    set +e
    bash "$REPO/scripts/level-validate.sh" --expectations "$VALIDATE_LOG" --tol "$TOL"
    EXT_RC=$?
    set -e
  fi
fi

echo
if [ "$FAILED" -ne 0 ]; then
  err "validate-hbe: FAIL — a leveling step failed before validation could run (logs in $OUT_DIR)"
  exit 1
fi
# A missing row is a failure of THIS run whatever the judge said — including when the judge
# skipped for want of ffmpeg. It means a sound was never re-measured at all.
if [ "$MISSING" -ne 0 ]; then
  err "validate-hbe: FAIL — $MISSING expectation row(s) were never emitted; that many sounds"
  err "                     went unvalidated. Named above; per-measure logs in $OUT_DIR"
  exit 1
fi
# A clamped row sits at its knob's end stop, not on a solved value: first-pass leveling means
# no clamps, so it fails the run even when its re-measure lands in tolerance.
CLAMPED="$(grep -h 'CLAMPED' "$OUT_DIR"/level-*.log || true)"
if [ -n "$CLAMPED" ]; then
  err "validate-hbe: FAIL — clamped row(s), leveled at a knob end stop, not a solved value:"
  printf '%s\n' "$CLAMPED" >&2
  exit 1
fi
# Branch every known code explicitly — a SKIP or a VACUOUS pass must not read as a miss,
# and (the point of naming 4 here) a VACUOUS pass must not silently fall into the `*)`
# failure arm either, which would misreport "nothing was verified" as "a row FAILed".
case "$EXT_RC" in
  0)
    ok "validate-hbe: PASS — every measured row landed within ${TOL} LU of its target"
    exit 0
    ;;
  3)
    err "validate-hbe: SKIPPED — ffmpeg is not on PATH, so NOTHING was independently"
    err "                         measured. The leveling itself ran and is saved."
    exit 3
    ;;
  4)
    err "validate-hbe: VACUOUS — every emitted row was SKIPped (clamped or persist-"
    err "                        mismatched), so NOTHING was independently verified —"
    err "                        this is the shape a lazy-commit regression takes. The"
    err "                        leveling itself ran and is saved; see the per-row"
    err "                        verdicts above and the logs in $OUT_DIR"
    exit 4
    ;;
  *)
    err "validate-hbe: FAIL — see the per-row verdicts above and the logs in $OUT_DIR"
    exit 1
    ;;
esac
