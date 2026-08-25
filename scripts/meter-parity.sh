#!/usr/bin/env bash
#
# meter-parity.sh — DEV-ONLY, OPTIONAL. Ground-truths this repo's own BS.1770
# loudness meter (`src-tauri/src/lufs.rs`) against an independent BS.1770
# implementation (ffmpeg's `ebur128` filter) on deterministic synthetic
# fixtures generated PER-RUN via ffmpeg lavfi — nothing binary is committed.
#
# NOT part of `scripts/gates.sh` and NOT a CI dependency: ffmpeg is never
# linked into or invoked by the shipped app, only by this developer tool. If
# ffmpeg is not on PATH, this script prints a skip notice and exits 0.
#
# Usage:
#   bash scripts/meter-parity.sh                 # generate fixtures + compare
#   bash scripts/meter-parity.sh <dir-of-wavs>    # compare an existing directory
#                                                 # of WAVs instead (e.g. a
#                                                 # `probe --dump-wav` capture dir)
#
# What a FAIL means: `lufs.rs` disagrees with the independent BS.1770 read by
# more than the tolerance on at least one fixture. Since ffmpeg's `ebur128` is
# a mature, widely-used implementation, treat that as a lufs.rs regression
# (channel/hop handling) first, not a bad fixture — unless the fixture came
# from a real device capture (`--dump-wav`), where device-side noise/DC offset
# is also a plausible explanation.
#
# Precision: ffmpeg's end-of-run ebur128 SUMMARY prints only one decimal, which
# would cap this comparison at ~0.1 LU and hide the real agreement. So we read the
# full-precision integrated value out of the filter's own metadata instead
# (`ebur128=metadata=1` + `ametadata=print:key=lavfi.r128.I`, last value emitted =
# the final integrated loudness, ~0.001 resolution). That let the tolerance drop
# from 0.1 to 0.02 LU. Measured residual vs ffmpeg on real device captures is
# 0.004-0.006 LU; two conformant BS.1770 implementations still differ slightly in
# how the final partial gating block is handled, so do NOT expect bit-exact
# equality — 0.02 is a deliberately tight but achievable bar.
#
# Written to run under macOS system /bin/bash (3.2): no associative arrays,
# no `mapfile`. Plain indexed arrays (`WAVS=()`/`+=()`) ARE fine on 3.2 — only
# `declare -A` and `mapfile` are the 4.x-only features — but "${arr[@]}" on a
# TRULY EMPTY array is an unbound-variable abort pre-4.4, so any expansion of
# WAVS is guarded by an `${#WAVS[@]}` length check first.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

log()  { printf '\033[36m▸ %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$*"; }
err()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; }

TOL_LU="0.02"

if ! command -v ffmpeg >/dev/null 2>&1; then
  log "skipped: ffmpeg not found on PATH (this is a dev-only harness — ffmpeg is never a build or runtime dependency of the app)"
  exit 0
fi

INPUT_DIR="${1:-}"
WORKDIR=""
# shellcheck disable=SC2329  # invoked via `trap cleanup EXIT`, which shellcheck doesn't count as a use
cleanup() {
  if [ -n "$WORKDIR" ] && [ -d "$WORKDIR" ]; then
    rm -rf "$WORKDIR"
  fi
}
trap cleanup EXIT

if [ -n "$INPUT_DIR" ]; then
  if [ ! -d "$INPUT_DIR" ]; then
    err "not a directory: $INPUT_DIR"
    exit 2
  fi
  FIXTURE_DIR="$INPUT_DIR"
  log "comparing an EXISTING WAV directory (real captures, e.g. from --dump-wav): $FIXTURE_DIR"
else
  WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/meter-parity.XXXXXX")"
  FIXTURE_DIR="$WORKDIR"
  log "generating deterministic synthetic fixtures in $FIXTURE_DIR"

  # Fixed seed on every noise source — unseeded `anoisesrc` is not reproducible
  # run-to-run, which would make a FAIL non-actionable (did lufs.rs regress, or
  # did the fixture just change?).
  SEED=42

  # sine mono — the INPUT-side convention control (measure_mono).
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=1000:duration=5:sample_rate=48000,volume=0.5" \
    -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/sine_mono.wav"

  # sine dual-mono — same tone duplicated onto both channels: the TMP's own
  # mirrored USB-Out shape. dual-mono minus mono should read +3.0103 dB exactly
  # (10*log10(2)) — the whole PR2 re-baseline in one comparison.
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=1000:duration=5:sample_rate=48000,volume=0.5,pan=stereo|c0=c0|c1=c0" \
    -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/sine_dual_mono.wav"

  # true stereo — DIFFERENT tones on L/R (energy-sum convention, not a
  # dual-mono shortcut in disguise).
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=440:duration=5:sample_rate=48000,volume=0.5[a];sine=frequency=660:duration=5:sample_rate=48000,volume=0.2[b];[a][b]amerge=inputs=2" \
    -ac 2 -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/true_stereo.wav"

  # pink noise dual-mono — a non-tonal, non-stationary-spectrum fixture.
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "anoisesrc=color=pink:duration=5:sample_rate=48000:seed=$SEED,volume=0.5,pan=stereo|c0=c0|c1=c0" \
    -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/pink_dual_mono.wav"

  # dynamic two-level clip — a loud passage then a long quieter one, so the
  # relative gate has real dynamics to discard (the clean-vs-distorted class
  # `lufs.rs`'s short-term-max exists for).
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "anoisesrc=color=pink:duration=3:sample_rate=48000:seed=$SEED,volume=0.8[a];anoisesrc=color=pink:duration=5:sample_rate=48000:seed=$((SEED + 1)),volume=0.25[b];[a][b]concat=n=2:v=0:a=1[mono];[mono]pan=stereo|c0=c0|c1=c0" \
    -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/dynamic.wav"

  # tremolo — amplitude-modulated tone (a stationary spectrum but a
  # non-stationary envelope, distinct from the dynamic clip's two flat levels).
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=1000:duration=5:sample_rate=48000,volume=0.6,tremolo=f=200:d=0.8,pan=stereo|c0=c0|c1=c0" \
    -ar 48000 -c:a pcm_f32le "$FIXTURE_DIR/tremolo.wav"
fi

log "building probe (production build, no e2e feature)…"
( cd "$REPO/src-tauri" && cargo build --quiet --bin probe )
PROBE_BIN="$REPO/src-tauri/target/debug/probe"
if [ ! -x "$PROBE_BIN" ]; then
  err "probe binary not found at $PROBE_BIN after build"
  exit 1
fi

# This repo's own meter, via the PRODUCTION output-side convention (stereo for
# a ≥2-ch file, mono for 1-ch — the same path `--measure-wav` documents).
# `|| true`: under `set -euo pipefail` an unparseable/failed measurement (grep
# finds nothing, or `probe` itself errors) would otherwise abort the WHOLE
# script mid-loop instead of surfacing as one FAIL row in the table below.
companion_lufs() {
  "$PROBE_BIN" --measure-wav "$1" 2>/dev/null \
    | grep -o 'integrated_lufs=-\{0,1\}[0-9.]*' \
    | cut -d= -f2 || true
}

# The independent BS.1770 read (`ffmpeg_lufs`) lives in the shared lib — ONE home for
# the full-precision `lavfi.r128.I` extraction, shared with `scripts/level-validate.sh`.
# shellcheck source=scripts/lib/ebur128.sh disable=SC1091
. "$REPO/scripts/lib/ebur128.sh"

WAVS=()
if [ -n "$INPUT_DIR" ]; then
  for f in "$FIXTURE_DIR"/*.wav; do
    [ -e "$f" ] || continue
    WAVS+=("$f")
  done
  if [ "${#WAVS[@]}" -eq 0 ]; then
    err "no .wav files found in $FIXTURE_DIR"
    exit 2
  fi
else
  WAVS+=(
    "$FIXTURE_DIR/sine_mono.wav"
    "$FIXTURE_DIR/sine_dual_mono.wav"
    "$FIXTURE_DIR/true_stereo.wav"
    "$FIXTURE_DIR/pink_dual_mono.wav"
    "$FIXTURE_DIR/dynamic.wav"
    "$FIXTURE_DIR/tremolo.wav"
  )
fi

log "comparing lufs.rs (probe --measure-wav) vs ffmpeg ebur128 — tolerance ${TOL_LU} LU"
printf '\n%-22s %14s %14s %10s   %s\n' "fixture" "companion" "ffmpeg" "delta" "verdict"
printf '%s\n' "-------------------------------------------------------------------------"

FAILED=0
# Guard the array expansion: under `set -u`, "${WAVS[@]}" on a TRULY EMPTY
# array is safe on bash 4.4+ but an "unbound variable" abort on macOS's
# bash 3.2 (this script's target shell — see the header). Unreachable in
# practice (the INPUT_DIR branch above already exits if empty, and the
# fixture branch always populates six entries), but cheap insurance against
# a future edit reintroducing an empty-array path.
if [ "${#WAVS[@]}" -gt 0 ]; then
  for wav in "${WAVS[@]}"; do
    name="$(basename "$wav" .wav)"
    companion="$(companion_lufs "$wav")"
    ffmpeg_val="$(ffmpeg_lufs "$wav")"

    if [ -z "$companion" ] || [ -z "$ffmpeg_val" ]; then
      printf '%-22s %14s %14s %10s   %s\n' "$name" "${companion:-N/A}" "${ffmpeg_val:-N/A}" "N/A" "FAIL (unparseable)"
      FAILED=1
      continue
    fi

    delta="$(awk -v a="$companion" -v b="$ffmpeg_val" 'BEGIN { d = a - b; if (d < 0) d = -d; printf "%.3f", d }')"
    verdict="$(awk -v d="$delta" -v tol="$TOL_LU" 'BEGIN { print (d <= tol) ? "PASS" : "FAIL" }')"
    printf '%-22s %14s %14s %10s   %s\n' "$name" "$companion" "$ffmpeg_val" "$delta" "$verdict"
    if [ "$verdict" = "FAIL" ]; then
      FAILED=1
    fi
  done
fi

echo
if [ "$FAILED" -eq 0 ]; then
  ok "PASS — every fixture agrees with the independent BS.1770 read within ${TOL_LU} LU"
  exit 0
else
  err "FAIL — at least one fixture exceeded ${TOL_LU} LU against the independent BS.1770 read"
  exit 1
fi
