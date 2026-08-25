#!/usr/bin/env bash
#
# ebur128.sh — the shared independent-BS.1770 read. Sourced by BOTH
# `scripts/meter-parity.sh` (which ground-truths `src-tauri/src/lufs.rs` against it) and
# `scripts/level-validate.sh` (which judges a leveled device capture with it). ONE home,
# because the extraction below is not obvious and both callers depend on the same
# precision property.
#
# PRECISION — why not just read ffmpeg's summary: ffmpeg's end-of-run `ebur128` SUMMARY
# line prints only ONE decimal, which caps any comparison at ~0.1 LU and hides the real
# agreement. The full-precision integrated value comes out of the filter's own metadata
# instead (`ebur128=metadata=1` + `ametadata=print:key=lavfi.r128.I`); the LAST value
# emitted is the final integrated loudness, at ~0.001 LU resolution. That is what let
# meter-parity's tolerance drop from 0.1 to 0.02 LU.
#
# This file defines functions only — it runs nothing and sets no options. It must stay
# safe to source under the callers' `set -euo pipefail`, so every function that can fail
# on bad input returns an EMPTY string rather than a non-zero status (the caller decides
# whether an unparseable read is a skip or a failure).
#
# BSD userland / macOS system /bin/bash 3.2 only: no GNU flags, no `mapfile`, no
# associative arrays.

# Integrated loudness (LUFS) of a WAV, full precision. Prints the bare number, or an
# EMPTY string when ffmpeg fails or emits no metadata. The trailing `|| true` keeps a
# failed measurement from aborting a `set -e` caller mid-loop.
ffmpeg_lufs() {
  ffmpeg -hide_banner -nostats -i "$1" \
    -af ebur128=metadata=1,ametadata=print:key=lavfi.r128.I:file=- -f null - 2>&1 \
    | grep -o 'lavfi\.r128\.I=.*' \
    | tail -1 \
    | cut -d= -f2 || true
}

# Channel count of a WAV's first audio stream. Prints an integer, or an EMPTY string
# when it cannot be determined.
#
# WHY IT MATTERS: `dump_processed_capture` (`probe_api/stimulus.rs`) writes a MONO file
# when the capture had no processed PAIR, deliberately refusing to duplicate one channel
# into fake dual-mono (that would invent the +3.01 dB the hardware never produced). So a
# mono validation WAV means the capture never carried USB-Out 1/2 — a convention
# mismatch, ~3 LU adrift of the stereo BS.1770 target the leveler solved against, and NOT
# a level miss. The caller must report it as its own named failure.
#
# ffprobe ships with ffmpeg but is not guaranteed present in every install, so this falls
# back to parsing ffmpeg's own stream banner.
ffmpeg_channels() {
  local n=""
  if command -v ffprobe >/dev/null 2>&1; then
    n="$(ffprobe -v error -select_streams a:0 -show_entries stream=channels \
      -of default=nokey=1:noprint_wrappers=1 "$1" 2>/dev/null | head -1 || true)"
  fi
  if [ -z "$n" ]; then
    # "Stream #0:0: Audio: pcm_f32le ([3][0][0][0] / 0x0003), 48000 Hz, stereo, flt, …"
    local layout
    layout="$(ffmpeg -hide_banner -nostats -i "$1" -f null - 2>&1 \
      | sed -n 's/.*Audio: .*, [0-9][0-9]* Hz, \([a-z0-9.()]*\),.*/\1/p' | head -1 || true)"
    case "$layout" in
      mono) n=1 ;;
      stereo) n=2 ;;
      *) n="" ;;
    esac
  fi
  printf '%s' "$n"
}
