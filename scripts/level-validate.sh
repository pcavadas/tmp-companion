#!/usr/bin/env bash
#
# level-validate.sh — P5 external validation. ffmpeg's `ebur128` is the ONLY meter here:
# Companion code plays/solves/captures, this script independently MEASURES the captured
# WAV and compares it against the TARGET the leveling run recorded — never against that
# run's own achieved/predicted number. A green run means "the physical output actually
# reads at target through a meter this repo did not write", the one thing no Rust unit
# test can prove (a `lufs.rs` regression fools every self-consistency check equally).
#
# ── THE CONTRACT, stated once ──────────────────────────────────────────────────────
# INPUT (batch mode) is the JSON-lines log the Rust measurement seam appends to when
# `TMP_E2E_VALIDATE_LOG` is set (`src-tauri/src/validate_log.rs`). One line per sound
# re-measured from the SAVED state, flat and un-nested:
#
#   {"label":"scene:slot404:scene2","slot":404,"scene_slot":2,"switch":null,
#    "target_lufs":-17.0000,"tol_lu":null,"clamped":false,"persist_mismatch":null,
#    "wav":"/path/scene_slot404_scene2.wav","wav_error":null,"engaged":"engaged"}
#
# This script does NOT drive the device and does NOT re-capture: the WAV named by each
# row already exists, written by the same capture that produced the run's own number.
#
# PER-ROW VERDICTS
#   FAIL  — the independent read missed `target_lufs` by more than the tolerance.
#   FAIL  — `engaged` is "silent" or "floor" ⇒ a FAILED INJECT, not a level miss.
#           `danger.md`: a silent/failed re-amp inject reads as the device's stationary
#           output floor, which ebur128 happily measures as a plausible-looking wrong
#           number.
#   WARN  — `engaged` is "floor_suspect": the capture is FLAT but LOUD. That is the known
#           false positive of `leveller::is_engaged`'s spread>0.5 arm on a compressed
#           chain (CLAUDE.md), not a failed inject, and failing it hard would sink a
#           correct 45-minute online run. The row is still MEASURED and still PASS/FAILs
#           on ffmpeg's own number — only the engage proof is downgraded to advisory, and
#           the verdict column carries `[WARN engage floor_suspect]`.
#   FAIL  — `wav` is null / the file is missing (named distinctly from a target miss).
#   FAIL  — the WAV is MONO. `dump_processed_capture` writes mono only when the capture
#           carried no processed PAIR, refusing to fake dual-mono; the leveler solved
#           against a 2-ch BS.1770 target, so a mono read is ~3 LU adrift by convention.
#           A convention mismatch, not a level miss.
#   SKIP  — `clamped` (the target was never reachable, so asserting it is a false red).
#   SKIP  — `persist_mismatch` (the saved preset does not hold the value the run
#           reported, so the number under test is meaningless).
#
# EXIT CODES — both callers (`scripts/e2e.sh`, `scripts/validate-hbe.sh`) branch on all
# four explicitly, so a mid-run skip — or a vacuous pass — can never be reported as a
# target miss, and a vacuous pass can never be reported as a real one:
#   0  every row PASSed, and at least one row was actually MEASURED (not just skipped).
#   1  at least one row FAILed.
#   2  usage error.
#   3  ffmpeg is not on PATH — nothing was checked. A DISTINCT code so "skipped" is
#      never mistaken for "checked and passed"; a silent skip that looks like a pass is
#      worse than a red failure.
#   4  VACUOUS — every row was SKIPped (clamped / persist_mismatch), so ZERO rows were
#      independently measured. Distinct from 0 on purpose: this is precisely the shape a
#      lazy-commit regression takes (every row persist-mismatches, none FAILs), so a
#      caller that treats it as a plain pass would certify a run that verified nothing.
#
# TOLERANCE defaults to 1.0 LU, NOT the solve's own acceptance band. `level.online.spec.
# ts` documents the arithmetic: the footswitch lane accepts a solve within KNOB_TOL_LU
# (0.3 LU, `leveller.rs`) and the re-measure adds its own run-to-run noise on top (~0.12
# LU base/scene, more on a modulated preset), so a 0.3 tolerance here would fail
# perfectly correct runs. This validates the LEVELING, not the solver's last digit.
#
# ── MODES ──────────────────────────────────────────────────────────────────────────
#   level-validate.sh --expectations <file.jsonl>
#       Batch: judge every row of the log against the WAV the row itself names.
#   level-validate.sh --wav <path> --label L --target T [--tol T] [--probe-log <f>]
#       Single file. `--probe-log` is the captured stdout of the `probe --measure-*` run
#       that produced <path>; a FLOOR/SILENT stamp in it fails the row without ever
#       trusting ffmpeg's number (the same engage-proof the batch rows carry inline).
#   level-validate.sh --live <seconds> --label L --target T [--tol T] [--device ID]
#       Bare avfoundation capture, front pair (USB-Out 1/2) → stereo. ATTENDED ONLY: it
#       has NO engage-proof — the CALLER must be driving engagement and verifying it by
#       other means. No automated caller uses this; `--wav`/`--expectations` are the
#       paths that carry the proof.
#
# Env:
#   TMP_E2E_LEVEL_TOL_LU   default tolerance in LU (1.0 — see the TOLERANCE note above)
#   TMP_E2E_AVF_DEVICE     default avfoundation device id for --live (":0")
#
# BSD userland only (macOS system /bin/bash 3.2): no `timeout(1)`, no GNU flags, no
# `mapfile`/associative arrays. jq is USED when present but every jq call has an
# awk/sed fallback, so a jq-less box still runs — see `have_jq`.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

log()  { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
err()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }
warn() { printf '\033[33m⚠ %s\033[0m\n' "$*"; }

# The independent BS.1770 read + the channel-count check: ONE home, shared with
# `scripts/meter-parity.sh` (see that file's header for the precision rationale).
# shellcheck source=scripts/lib/ebur128.sh disable=SC1091
. "$REPO/scripts/lib/ebur128.sh"

DEFAULT_TOL="${TMP_E2E_LEVEL_TOL_LU:-1.0}"
DEFAULT_DEVICE="${TMP_E2E_AVF_DEVICE:-:0}"

usage() {
  cat >&2 <<'USAGE'
usage:
  level-validate.sh --expectations <file.jsonl>
  level-validate.sh --wav <path> --label L --target T [--tol T] [--probe-log <file>]
  level-validate.sh --live <seconds> --label L --target T [--tol T] [--device ID]

exit: 0 pass · 1 fail · 2 usage · 3 ffmpeg absent (nothing checked) · 4 vacuous (0 rows measured)
USAGE
  exit 2
}

have_jq() { command -v jq >/dev/null 2>&1; }

# Capture SECONDS from avfoundation DEVICE_ID, front pair → stereo WAV at OUT_PATH.
# avfoundation enumerates the TMP's 4 channels; channels 0/1 are USB-Out 1/2 (the
# processed pair Companion measures) — `-map_channel` pins exactly those two, dropping
# 2/3 regardless of how many the device exposes.
avf_capture() {
  local secs="$1" device="$2" out="$3"
  ffmpeg -hide_banner -loglevel error -y \
    -f avfoundation -i "$device" -t "$secs" \
    -map_channel 0.0.0 -map_channel 0.0.1 \
    -ar 48000 -c:a pcm_f32le "$out"
}

ROW_FAILED=0
ROWS_SEEN=0
print_header_once_done=0
row_header() {
  if [ "$print_header_once_done" -eq 0 ]; then
    printf '\n%-34s %12s %12s %8s %8s   %s\n' "label" "measured" "target" "tol" "delta" "verdict"
    printf '%s\n' "-------------------------------------------------------------------------------------"
    print_header_once_done=1
  fi
}
row_out() { # label measured target tol delta verdict
  row_header
  printf '%-34s %12s %12s %8s %8s   %s\n' "$1" "$2" "$3" "$4" "$5" "$6"
}
row_fail() { # label target tol verdict
  row_out "$1" "N/A" "$2" "$3" "N/A" "$4"
  ROW_FAILED=1
}
row_skip() { # label target tol verdict
  row_out "$1" "N/A" "$2" "$3" "N/A" "$4"
}

# Measure one WAV and print its verdict row. `engaged` (optional) is the emitter's own
# engage verdict — checked BEFORE ffmpeg is consulted, because a floor capture's
# ebur128 read is a real number for the WRONG signal, not noise a threshold can catch.
# `probe_log` (optional, single mode) is the same proof read out of a probe headline.
measure_row() { # label wav target tol [engaged] [probe_log]
  local label="$1" wav="$2" target="$3" tol="$4" engaged="${5:-}" probe_log="${6:-}"
  local warn_note=""
  ROWS_SEEN=$((ROWS_SEEN + 1))
  # `floor_suspect` is a WARN, not a FAIL: the emitter stamps it when the capture is flat
  # (is_engaged's spread>0.5 arm) but LOUD — the documented false positive of that
  # criterion on a compressed chain, not a failed inject (`validate_log.rs`'s
  # `engaged_verdict`). ffmpeg stays the authority: the row still gets measured and still
  # PASSes/FAILs on its own number, with the doubt carried into the verdict string.
  # `silent` and a genuine near-silence `floor` stay hard FAILs.
  if [ "$engaged" = "floor_suspect" ]; then
    warn "$label: engage verdict 'floor_suspect' — flat but loud (is_engaged's spread arm, not a failed inject); grading it on the ffmpeg number anyway"
    warn_note=" [WARN engage floor_suspect]"
  elif [ -n "$engaged" ] && [ "$engaged" != "engaged" ]; then
    row_fail "$label" "$target" "$tol" \
      "FAIL (engage verdict '$engaged' — failed inject, not a level miss)"
    return
  fi
  if [ -n "$probe_log" ] && [ -f "$probe_log" ] && grep -q 'FLOOR/SILENT' "$probe_log"; then
    row_fail "$label" "$target" "$tol" \
      "FAIL (FLOOR/SILENT in the probe headline — failed inject, not a level miss)"
    return
  fi
  if [ -z "$wav" ] || [ "$wav" = "null" ]; then
    row_fail "$label" "$target" "$tol" "FAIL (no WAV was dumped for this row)"
    return
  fi
  if [ ! -f "$wav" ]; then
    row_fail "$label" "$target" "$tol" "FAIL (missing WAV: $wav)"
    return
  fi
  local ch
  ch="$(ffmpeg_channels "$wav")"
  if [ "$ch" = "1" ]; then
    row_fail "$label" "$target" "$tol" \
      "FAIL (mono capture — convention mismatch, not a level miss)"
    return
  fi
  local measured
  measured="$(ffmpeg_lufs "$wav")"
  # Guard non-numeric/degenerate reads (-inf, nan, an empty extraction) — a raw awk
  # comparison against those does not do what it looks like it does.
  case "$measured" in
    ''|*[!0-9.+-]*|*inf*|*nan*|*NaN*)
      row_out "$label" "${measured:-N/A}" "$target" "$tol" "N/A" \
        "FAIL (unparseable ebur128 read: '${measured:-empty}')"
      ROW_FAILED=1
      return
      ;;
  esac
  local delta verdict
  delta="$(awk -v a="$measured" -v b="$target" 'BEGIN { d = a - b; if (d < 0) d = -d; printf "%.3f", d }')"
  verdict="$(awk -v d="$delta" -v tol="$tol" 'BEGIN { print (d <= tol) ? "PASS" : "FAIL" }')"
  row_out "$label" "$measured" "$target" "$tol" "$delta" "$verdict$warn_note"
  if [ "$verdict" = "FAIL" ]; then
    ROW_FAILED=1
  fi
}

# Pull one field out of a FLAT single-line JSON object. `$2` is the key; prints the
# value with surrounding quotes stripped, or an empty string when absent/null. Not a
# general JSON parser — good enough for the one committed shape the emitter writes.
json_field() {
  local line="$1" key="$2" v=""
  if have_jq; then
    v="$(printf '%s' "$line" | jq -r --arg k "$key" '.[$k] // empty' 2>/dev/null || true)"
  else
    v="$(printf '%s' "$line" | sed -n "s/.*\"$key\" *: *\"\([^\"]*\)\".*/\1/p")"
    if [ -z "$v" ]; then
      v="$(printf '%s' "$line" | sed -n "s/.*\"$key\" *: *\(-\{0,1\}[0-9][0-9.]*\).*/\1/p")"
    fi
    if [ -z "$v" ]; then
      case "$line" in *"\"$key\":true"*|*"\"$key\": true"*) v="true" ;; esac
    fi
  fi
  printf '%s' "$v"
}

# ── arg parsing (BEFORE the ffmpeg presence check, so --help always works) ─────────
MODE=""
EXPECTATIONS=""
WAV_PATH=""
PROBE_LOG=""
LIVE_SECS=""
LABEL=""
TARGET=""
TOL="$DEFAULT_TOL"
DEVICE="$DEFAULT_DEVICE"

while [ $# -gt 0 ]; do
  case "$1" in
    --expectations) EXPECTATIONS="${2:-}"; MODE="batch"; shift 2 ;;
    --wav) WAV_PATH="${2:-}"; MODE="${MODE:-single}"; shift 2 ;;
    --probe-log) PROBE_LOG="${2:-}"; shift 2 ;;
    --live) LIVE_SECS="${2:-}"; MODE="${MODE:-live}"; shift 2 ;;
    --label) LABEL="${2:-}"; shift 2 ;;
    --target) TARGET="${2:-}"; shift 2 ;;
    --tol) TOL="${2:-}"; shift 2 ;;
    --device) DEVICE="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) err "unrecognized argument: $1"; usage ;;
  esac
done

if ! command -v ffmpeg >/dev/null 2>&1; then
  printf '\033[31m\n' >&2
  printf '########################################################################\n' >&2
  printf '#  level-validate.sh: ffmpeg NOT FOUND on PATH — EXTERNAL VALIDATION   #\n' >&2
  printf '#  WAS SKIPPED. Nothing was checked. This is NOT a pass.               #\n' >&2
  printf '########################################################################\n' >&2
  printf '\033[0m\n' >&2
  exit 3
fi

case "$MODE" in
  batch)
    [ -n "$EXPECTATIONS" ] || usage
    [ -f "$EXPECTATIONS" ] || { err "no such expectations file: $EXPECTATIONS"; exit 2; }
    log "batch mode — $EXPECTATIONS (tolerance ${TOL} LU)"
    while IFS= read -r line || [ -n "$line" ]; do
      [ -n "$line" ] || continue
      line_label="$(json_field "$line" label)"
      line_target="$(json_field "$line" target_lufs)"
      line_tol="$(json_field "$line" tol_lu)"
      line_wav="$(json_field "$line" wav)"
      line_wav_err="$(json_field "$line" wav_error)"
      line_engaged="$(json_field "$line" engaged)"
      line_clamped="$(json_field "$line" clamped)"
      line_persist="$(json_field "$line" persist_mismatch)"
      if [ -z "$line_label" ] || [ -z "$line_target" ]; then
        err "unparseable expectation line: $line"
        ROW_FAILED=1
        continue
      fi
      [ -n "$line_tol" ] || line_tol="$TOL"
      # SKIP before FAIL: a clamped row was never able to reach its target, and a row
      # whose save did not verify is describing a value the device does not hold —
      # asserting either against the target is a guaranteed false red.
      if [ "$line_clamped" = "true" ]; then
        row_skip "$line_label" "$line_target" "$line_tol" \
          "SKIP (clamped — target unreachable, nothing to validate against)"
        continue
      fi
      if [ "$line_persist" = "true" ]; then
        row_skip "$line_label" "$line_target" "$line_tol" \
          "SKIP (persist_mismatch — the saved preset does not hold this value)"
        continue
      fi
      if [ -n "$line_wav_err" ]; then
        row_fail "$line_label" "$line_target" "$line_tol" \
          "FAIL (WAV dump failed: $line_wav_err)"
        continue
      fi
      measure_row "$line_label" "$line_wav" "$line_target" "$line_tol" "$line_engaged"
    done < "$EXPECTATIONS"
    ;;
  single)
    [ -n "$WAV_PATH" ] && [ -n "$LABEL" ] && [ -n "$TARGET" ] || usage
    measure_row "$LABEL" "$WAV_PATH" "$TARGET" "$TOL" "" "$PROBE_LOG"
    ;;
  live)
    [ -n "$LIVE_SECS" ] && [ -n "$LABEL" ] && [ -n "$TARGET" ] || usage
    TMP_WAV="$(mktemp "${TMPDIR:-/tmp}/level-validate-live.XXXXXX").wav"
    trap 'rm -f "$TMP_WAV"' EXIT
    log "capturing ${LIVE_SECS}s live from avfoundation $DEVICE (ATTENDED — no engage-proof)…"
    avf_capture "$LIVE_SECS" "$DEVICE" "$TMP_WAV"
    measure_row "$LABEL" "$TMP_WAV" "$TARGET" "$TOL"
    ;;
  *)
    usage
    ;;
esac

echo
if [ "$ROW_FAILED" -eq 0 ]; then
  # A pass over zero measured rows is vacuous — every row skipped (clamped / persist
  # mismatch) means ffmpeg graded NOTHING. Legitimate (a skip is a real verdict, not an
  # error) but it must never read — or exit — as "the levels were verified": this is
  # exactly the shape a lazy-commit regression takes (every row persist-mismatches), so
  # it gets its OWN exit code rather than folding into the plain-pass 0.
  if [ "$ROWS_SEEN" -eq 0 ]; then
    warn "VACUOUS (exit 4) — no row was measurable, so NOTHING was independently verified"
    exit 4
  fi
  ok "PASS — every measured row landed within tolerance of its target ($ROWS_SEEN measured)"
  exit 0
else
  err "FAIL — at least one row missed its target or was unmeasurable"
  exit 1
fi
