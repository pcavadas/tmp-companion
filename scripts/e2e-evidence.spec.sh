#!/usr/bin/env bash
# Offline spec for scripts/lib/e2e-evidence.sh: the online lane's run folders.
#
# Proves, with no unit attached: two runs leave two folders and the first is untouched; an
# evidence override inside the worktree that git does not ignore is refused; writing
# evidence under the default root leaves `gates.sh --key` unchanged; a run stopped with
# SIGINT still writes MANIFEST.json with its non-zero exit code; retention drops only the
# WAVs of older runs; and the trace checks read synthetic TCT v1 lines correctly.
#
# Bash 3.2-compatible (.claude/rules/shell-scripts.md); gates.sh runs it under /bin/bash.
set -euo pipefail
# A git hook (e.g. pre-push from a linked worktree) exports GIT_DIR and friends; left set,
# the scratch repo's `git init` / `git config` / `checkout` act on the CALLER's repository.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY GIT_PREFIX
# ...and the rest of the repo-local set git itself clears for a subprocess (GIT_CONFIG,
# GIT_CONFIG_PARAMETERS, GIT_ALTERNATE_OBJECT_DIRECTORIES, ...). Word-split on purpose.
# shellcheck disable=SC2046
unset $(git rev-parse --local-env-vars 2>/dev/null)

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/lib/e2e-evidence.sh disable=SC1091
. "$script_dir/lib/e2e-evidence.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail=0
check() { # <label> <expected> <actual>
  if [ "$2" = "$3" ]; then
    printf 'ok   %s\n' "$1"
  else
    printf 'FAIL %s: expected [%s], got [%s]\n' "$1" "$2" "$3"
    fail=1
  fi
}

# A scratch repo with this checkout's gates.sh, so `--key` is the real one.
r="$work/repo"
mkdir -p "$r/scripts"
cp "$script_dir/gates.sh" "$script_dir/gate-map.tsv" "$script_dir/gate-policy.tsv" "$r/scripts/"
printf 'out/\n' > "$r/.gitignore"
(
  cd "$r"
  git init -q
  git config user.email spec@example.invalid
  git config user.name spec
  git add -A
  git commit -qm base
) >/dev/null
common="$(cd "$r/.git" && pwd -P)"

# ── the root ────────────────────────────────────────────────────────────────────
check "default root is in the git common dir" "$common/tmp-evidence" \
  "$(unset TMP_E2E_EVIDENCE_DIR; evidence_root "$r")"
inside_rc=0
(TMP_E2E_EVIDENCE_DIR="$r/evidence" evidence_root "$r" >/dev/null 2>&1) || inside_rc=$?
check "an un-ignored override inside the worktree is refused" 1 "$inside_rc"
check "an ignored override inside the worktree is accepted" "$(cd "$r" && pwd -P)/out/ev" \
  "$(TMP_E2E_EVIDENCE_DIR="$r/out/ev" evidence_root "$r")"
check "an override outside the worktree is accepted" "$(mkdir -p "$work/ext" && cd "$work/ext" && pwd -P)" \
  "$(TMP_E2E_EVIDENCE_DIR="$work/ext" evidence_root "$r")"

# ── two runs, two folders; the first is left alone ──────────────────────────────
root="$(unset TMP_E2E_EVIDENCE_DIR; evidence_root "$r")"
key_before="$(cd "$r" && /bin/bash scripts/gates.sh --key)"
run1="$(evidence_new_run "$root" online abc1234 0)"
printf 'first\n' > "$run1/e2e_server.log"
run2="$(evidence_new_run "$root" online abc1234 1)"
check "two runs make two folders" 2 "$(find "$root/runs" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
check "the second run is a new folder" yes "$([ "$run1" != "$run2" ] && echo yes || echo no)"
check "the first run's log is unchanged" first "$(cat "$run1/e2e_server.log")"
check "a dirty tree is named in the folder" 1 "$(basename "$run2" | grep -c -- '-online-abc1234-dirty$')"
run3="$(evidence_new_run "$root" online abc1234 0)"
check "a same-second run gets its own folder" yes "$([ -d "$run3" ] && [ "$run3" != "$run1" ] && echo yes || echo no)"

# Evidence under the default root never re-keys the tree.
mkdir -p "$run1/trace" "$run1/wav"
printf 'x\n' > "$run1/wav/a.wav"
check "gates.sh --key is unchanged by evidence" "$key_before" "$(cd "$r" && /bin/bash scripts/gates.sh --key)"

# ── synthetic traces ────────────────────────────────────────────────────────────
t="$run1/trace"
cat > "$t/e2e_server-100.tct.jsonl" <<'TCT'
{"seq":0,"sid":null,"t_us":0,"t_nom_ms":0,"ev":"header","tct":1}
{"seq":1,"sid":1,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"device","ok":true,"err":null,"ms":3}
{"seq":2,"sid":1,"t_us":9,"t_nom_ms":0,"ev":"fw","fw":"1.8.58"}
{"seq":3,"sid":1,"t_us":9,"t_nom_ms":0,"ev":"drop","what":"wire","count":2}
{"seq":4,"sid":1,"t_us":9,"t_nom_ms":0,"ev":"drop","what":"capture","count":1}
TCT
cat > "$t/probe-101.tct.jsonl" <<'TCT'
{"seq":0,"sid":null,"t_us":0,"t_nom_ms":0,"ev":"header","tct":1}
{"seq":1,"sid":1,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"device","ok":true,"err":null,"ms":3}
{"seq":2,"sid":2,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"device","ok":true,"err":null,"ms":3}
TCT
check "a server trace with a device open proves the device" 0 \
  "$(evidence_server_touched_device "$t"; echo $?)"
check "the firmware comes from the trace" 1.8.58 "$(evidence_trace_fw "$t")"
traces="$(evidence_traces_json "$t")"
check "traces list both files" 2 "$(printf '%s' "$traces" | grep -o '"file"' | wc -l | tr -d ' ')"
check "drops are summed" 1 "$(printf '%s' "$traces" | grep -c '"file":"trace/e2e_server-100.tct.jsonl","bytes":[0-9]*,"device_sessions":1,"drops":3')"
check "device sessions are counted" 1 "$(printf '%s' "$traces" | grep -c '"device_sessions":2,"drops":0')"

s="$work/stale"
mkdir -p "$s"
printf '%s\n' '{"seq":1,"sid":1,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"sim","ok":true,"err":null,"ms":0}' \
  > "$s/e2e_server-7.tct.jsonl"
printf '%s\n' '{"seq":1,"sid":1,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"device","ok":true,"err":null,"ms":0}' \
  > "$s/probe-8.tct.jsonl"
check "a server trace with only sim opens does not prove the device" 1 \
  "$(evidence_server_touched_device "$s"; echo $?)"
printf '%s\n' '{"seq":1,"sid":1,"t_us":5,"t_nom_ms":0,"ev":"open","backend":"device","ok":false,"err":"seized","ms":0}' \
  >> "$s/e2e_server-7.tct.jsonl"
check "a failed device open does not prove the device" 1 \
  "$(evidence_server_touched_device "$s"; echo $?)"
check "no trace at all does not prove the device" 1 \
  "$(evidence_server_touched_device "$work/none"; echo $?)"

# ── a run stopped with SIGINT still writes its MANIFEST ─────────────────────────
cat > "$work/lane.sh" <<LANE
#!/bin/bash
set -euo pipefail
. "$script_dir/lib/e2e-evidence.sh"
RUN_DIR="\$(evidence_new_run "$root" online abc1234 0)"
printf '%s\n' "\$RUN_DIR" > "$work/run-dir"
cleanup() {
  local code=\$?
  [ -z "\${1:-}" ] || code="\$1"
  trap - EXIT INT TERM
  ( set +e; EV_ROOT="$root" EV_RUN_DIR="\$RUN_DIR" EV_MODE=online EV_EXIT="\$code" \\
      EV_UNIT=stock evidence_finalize >/dev/null ) || true
  exit "\$code"
}
trap cleanup EXIT
trap 'cleanup 130' INT
trap 'cleanup 143' TERM
printf 'ready\n' > "$work/ready"
while :; do sleep 1; done
LANE
# Job control on, so the background lane keeps SIGINT (a non-interactive shell starts
# its background jobs with SIGINT ignored), as a caller that backgrounds the lane must.
set -m
/bin/bash "$work/lane.sh" &
lane_pid=$!
set +m
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -f "$work/ready" ] && break; sleep 1; done
kill -INT "$lane_pid"
lane_rc=0
wait "$lane_pid" || lane_rc=$?
check "the stopped lane exits non-zero" yes "$([ "$lane_rc" -ne 0 ] && echo yes || echo no)"
int_run="$(cat "$work/run-dir")"
check "a SIGINT run has a MANIFEST" yes "$([ -f "$int_run/MANIFEST.json" ] && echo yes || echo no)"
check "the MANIFEST records a non-zero exit code" 1 \
  "$(grep -Ec '"exit_code": [1-9][0-9]*,' "$int_run/MANIFEST.json" 2>/dev/null || true)"
check "the MANIFEST names the unit" 1 "$(grep -c '"unit_config": "stock",' "$int_run/MANIFEST.json" 2>/dev/null || true)"
check "latest points at the newest run" "$int_run" "$(cd "$root/latest" && pwd -P)"
if command -v python3 >/dev/null 2>&1; then
  check "the MANIFEST is valid JSON" 0 \
    "$(python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$int_run/MANIFEST.json" >/dev/null 2>&1; echo $?)"
fi

if command -v python3 >/dev/null 2>&1; then
  ctl="$(printf 'a\033[31mb\tc\001d"e\\f')"
  check "a value with control bytes stays a valid JSON string" 'a[31mb cd"e\f' \
    "$(evidence_json_str "$ctl" | python3 -c 'import json,sys; print(json.load(sys.stdin))' 2>&1)"
fi

# ── retention: WAVs only for the newest N runs, everything else kept ────────────
rr="$work/ret"
mkdir -p "$rr/runs"
for n in 1 2 3 4; do
  d="$rr/runs/20260101T00000${n}Z-online-abc1234"
  mkdir -p "$d/wav" "$d/level-validate-wavs"
  printf 'x\n' > "$d/wav/c.wav"
  printf 'x\n' > "$d/MANIFEST.json"
done
evidence_retention "$rr" 2
check "the oldest runs lose their WAVs" no "$([ -d "$rr/runs/20260101T000001Z-online-abc1234/wav" ] && echo yes || echo no)"
check "the oldest runs lose their validation WAVs" no \
  "$([ -d "$rr/runs/20260101T000002Z-online-abc1234/level-validate-wavs" ] && echo yes || echo no)"
check "the oldest runs keep their MANIFEST" yes "$([ -f "$rr/runs/20260101T000001Z-online-abc1234/MANIFEST.json" ] && echo yes || echo no)"
check "the newest runs keep their WAVs" yes "$([ -f "$rr/runs/20260101T000004Z-online-abc1234/wav/c.wav" ] && echo yes || echo no)"
check "the second newest keeps its WAVs" yes "$([ -f "$rr/runs/20260101T000003Z-online-abc1234/wav/c.wav" ] && echo yes || echo no)"

if [ "$fail" -ne 0 ]; then
  printf 'e2e-evidence spec: FAILED\n' >&2
  exit 1
fi
printf 'e2e-evidence spec: all checks passed\n'
