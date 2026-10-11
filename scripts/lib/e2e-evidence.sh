# shellcheck shell=bash
#
# scripts/lib/e2e-evidence.sh — run folders for the ONLINE e2e lane (and soak): each run
# writes into its own dated directory instead of truncating the last one, and gets a
# MANIFEST.json when it ends. Sourced by scripts/e2e.sh; tested offline by
# scripts/e2e-evidence.spec.sh. Other device scripts reuse it with their own <mode>.
#
# Layout and the trace format: notes/e2e-test-plan.md, "Recording". The root defaults to
# `$(git rev-parse --git-common-dir)/tmp-evidence`: inside .git, so it is never tracked,
# never part of `gates.sh --key`, shared by every worktree and kept when a worktree is
# removed. Raw evidence never leaves this machine.
#
# Everything here is best-effort bookkeeping: finalizing must never change a run's exit
# code, so callers run `evidence_finalize` in a subshell with `|| true`.
#
# bash 3.2-safe (macOS /bin/bash): no associative arrays, no mapfile, no ${x^^}.

# $1 = repo (worktree) root. Print the absolute evidence root, or fail (exit 1, reason on
# stderr) when TMP_E2E_EVIDENCE_DIR points inside the worktree at a path git does not
# ignore — writing there would change the tree key mid-run and cost the stamp.
evidence_root() {
  local repo="$1" common dir
  common="$(cd "$repo" && cd "$(git rev-parse --git-common-dir)" && pwd -P)" || return 1
  if [ -z "${TMP_E2E_EVIDENCE_DIR:-}" ]; then
    printf '%s/tmp-evidence\n' "$common"
    return 0
  fi
  mkdir -p "$TMP_E2E_EVIDENCE_DIR" || return 1
  dir="$(cd "$TMP_E2E_EVIDENCE_DIR" && pwd -P)" || return 1
  local top
  top="$(cd "$repo" && pwd -P)"
  case "$dir/" in
    "$common"/*) ;;                     # inside the git dir: never tracked
    "$top"/*)
      if ! git -C "$repo" check-ignore -q "$dir"; then
        printf 'TMP_E2E_EVIDENCE_DIR=%s is inside the worktree and not ignored by git — evidence there would change the tree key mid-run; pick a path outside the worktree or an ignored one\n' \
          "$TMP_E2E_EVIDENCE_DIR" >&2
        return 1
      fi
      ;;
  esac
  printf '%s\n' "$dir"
}

# $1 = root, $2 = mode, $3 = short sha, $4 = 1 when the tree is dirty. Create and print a
# fresh run directory `<root>/runs/<yyyymmddThhmmssZ>-<mode>-<sha>[-dirty]`; a second run
# in the same second gets a `-2`, `-3`… suffix rather than reusing the first one's folder.
evidence_new_run() {
  local root="$1" mode="$2" sha="$3" dirty="$4" name base n=2
  name="$(date -u +%Y%m%dT%H%M%SZ)-$mode-$sha"
  [ "$dirty" = 1 ] && name="$name-dirty"
  base="$name"
  mkdir -p "$root/runs" || return 1
  while ! mkdir "$root/runs/$name" 2>/dev/null; do
    [ -d "$root/runs/$name" ] || return 1
    name="$base-$n"
    n=$((n + 1))
  done
  printf '%s/runs/%s\n' "$root" "$name"
}

# A JSON string literal for $1: quotes and backslashes escaped, tab/CR/LF folded to spaces,
# and every other control byte (and DEL) dropped, so no value can make MANIFEST.json invalid.
evidence_json_str() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | tr '\t\r\n' '   ' | LC_ALL=C tr -d '\000-\037\177')"
}

# $1 = value. A JSON string, or null when empty.
evidence_json_opt() {
  if [ -z "$1" ]; then printf 'null'; else evidence_json_str "$1"; fi
}

# $1 = 0/1 (or empty). A JSON boolean, or null when empty.
evidence_json_bool() {
  case "$1" in 1) printf 'true' ;; 0) printf 'false' ;; *) printf 'null' ;; esac
}

# $1 = a trace directory. Print a JSON array with one object per trace file: file, bytes,
# device sessions (successful opens on the real device) and dropped items. TCT v1 writes
# its fields in a fixed order, so grep reads them.
evidence_traces_json() {
  local dir="$1" f first=1 bytes sessions drops
  printf '['
  for f in "$dir"/*.tct.jsonl; do
    [ -f "$f" ] || continue
    bytes="$(wc -c < "$f" | tr -d ' ')"
    sessions="$(grep -c '"ev":"open","backend":"device","ok":true' "$f" 2>/dev/null || true)"
    drops="$(grep -o '"ev":"drop","what":"[a-z]*","count":[0-9]*' "$f" 2>/dev/null \
      | sed 's/.*"count"://' | awk '{s += $1} END {print s + 0}')"
    [ "$first" = 1 ] || printf ','
    first=0
    printf '{"file":%s,"bytes":%s,"device_sessions":%s,"drops":%s}' \
      "$(evidence_json_str "trace/$(basename "$f")")" "${bytes:-0}" "${sessions:-0}" "${drops:-0}"
  done
  printf ']'
}

# $1 = a trace directory. Print the firmware the last device trace reported, or nothing.
evidence_trace_fw() {
  cat "$1"/*.tct.jsonl 2>/dev/null | sed -n 's/.*"ev":"fw","fw":"\([^"]*\)".*/\1/p' | tail -1
}

# $1 = a trace directory. Exit 0 when the e2e server's trace holds a successful open on the
# real device: the proof an online run's server touched the unit, rather than a stale
# offline server answering on the port.
evidence_server_touched_device() {
  local f
  for f in "$1"/e2e_server-*.tct.jsonl; do
    [ -f "$f" ] || continue
    grep -q '"ev":"open","backend":"device","ok":true' "$f" && return 0
  done
  return 1
}

# $1 = root, $2 = how many of the newest runs keep their WAVs. Every older run loses its
# WAV folders (wav/, level-validate-wavs/); everything else is kept forever. Run folder
# names start with their UTC time, so a lexical sort is chronological.
evidence_retention() {
  local root="$1" keep="$2" total n=0 run
  case "$keep" in ''|*[!0-9]*) keep=10 ;; esac
  total="$(find "$root/runs" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')"
  [ "${total:-0}" -gt "$keep" ] || return 0
  find "$root/runs" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | LC_ALL=C sort | while read -r run; do
    n=$((n + 1))
    [ "$n" -le $((total - keep)) ] || break
    rm -rf "$run/wav" "$run/level-validate-wavs"
  done
}

# Write <run>/MANIFEST.json, point <root>/latest at the run, apply retention and print the
# run's path and size. Reads its inputs from EV_* variables (all optional except
# EV_ROOT and EV_RUN_DIR), so a caller sets what it knows and leaves the rest empty:
#   EV_RUN_ID EV_MODE EV_SPECS EV_FULL_SET EV_STARTED EV_EXIT
#   EV_VALIDATED EV_STAMPED EV_KEY_START EV_KEY_END
#   EV_GIT_SHA EV_DIRTY EV_FFMPEG EV_DEVICE_FW EV_UNIT EV_SCENE_CHANGE EV_FIXTURE_STAMP
#   EV_WAV_POLICY EV_WAV_RUNS
evidence_finalize() {
  local run="$EV_RUN_DIR" root="$EV_ROOT" fw os
  [ -d "$run" ] || return 1
  fw="${EV_DEVICE_FW:-}"
  [ -n "$fw" ] || fw="$(evidence_trace_fw "$run/trace")"
  os="$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  {
    printf '{\n'
    printf '  "run_id": %s,\n' "$(evidence_json_opt "${EV_RUN_ID:-$(basename "$run")}")"
    printf '  "mode": %s,\n' "$(evidence_json_opt "${EV_MODE:-}")"
    printf '  "specs": %s,\n' "$(evidence_json_opt "${EV_SPECS:-}")"
    printf '  "full_set": %s,\n' "$(evidence_json_bool "${EV_FULL_SET:-}")"
    printf '  "started_utc": %s,\n' "$(evidence_json_opt "${EV_STARTED:-}")"
    printf '  "ended_utc": %s,\n' "$(evidence_json_str "$(date -u +%Y-%m-%dT%H:%M:%SZ)")"
    printf '  "exit_code": %s,\n' "${EV_EXIT:-null}"
    printf '  "validated": %s,\n' "$(evidence_json_bool "${EV_VALIDATED:-}")"
    printf '  "stamped": %s,\n' "$(evidence_json_bool "${EV_STAMPED:-}")"
    printf '  "key_start": %s,\n' "$(evidence_json_opt "${EV_KEY_START:-}")"
    printf '  "key_end": %s,\n' "$(evidence_json_opt "${EV_KEY_END:-}")"
    printf '  "git_sha": %s,\n' "$(evidence_json_opt "${EV_GIT_SHA:-}")"
    printf '  "dirty": %s,\n' "$(evidence_json_bool "${EV_DIRTY:-}")"
    printf '  "os": %s,\n' "$(evidence_json_opt "$os")"
    printf '  "ffmpeg": %s,\n' "$(evidence_json_bool "${EV_FFMPEG:-}")"
    printf '  "device_fw": %s,\n' "$(evidence_json_opt "$fw")"
    printf '  "unit_config": %s,\n' "$(evidence_json_opt "${EV_UNIT:-}")"
    printf '  "scene_change_behavior": %s,\n' "$(evidence_json_opt "${EV_SCENE_CHANGE:-}")"
    printf '  "fixture_stamp": %s,\n' "$(evidence_json_opt "${EV_FIXTURE_STAMP:-}")"
    printf '  "traces": %s,\n' "$(evidence_traces_json "$run/trace")"
    printf '  "wav_policy": %s\n' "$(evidence_json_str "${EV_WAV_POLICY:-pair}, newest ${EV_WAV_RUNS:-10} runs")"
    printf '}\n'
  } > "$run/MANIFEST.json.tmp" && mv "$run/MANIFEST.json.tmp" "$run/MANIFEST.json"
  ln -sfn "$run" "$root/latest" 2>/dev/null || true
  evidence_retention "$root" "${EV_WAV_RUNS:-10}"
  printf 'run evidence: %s (%s)\n' "$run" "$(du -sh "$run" 2>/dev/null | cut -f1)"
}
