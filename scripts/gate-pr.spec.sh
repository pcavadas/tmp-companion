#!/usr/bin/env bash
# Offline spec for per-diff online spec selection: `gates.sh --required-online` on fixed
# diffs, and the PR hook (scripts/claude-hooks/gate-pr.sh) fed real hook payloads.
#
# Fixture: a scratch git repo holding this checkout's gates.sh, gate TSVs and hook, with a
# `main` branch as the merge base. Each case checks out a fresh branch, adds or edits the
# named paths, and asserts the output. Stamps are written straight into the scratch repo's
# git dir (a green run of the real gates is not the point here). Payloads live in
# scripts/gate-pr.spec/, written as files rather than shell strings: the hook matches
# command TEXT, so the PR words never appear in a command this spec runs.
#
# Bash 3.2-compatible (.claude/rules/shell-scripts.md); gates.sh runs it under /bin/bash.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
payloads="$script_dir/gate-pr.spec"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
r="$work/repo"
mkdir -p "$r/scripts/claude-hooks"
cp "$script_dir/gates.sh" "$script_dir/gate-map.tsv" "$script_dir/gate-policy.tsv" "$r/scripts/"
cp "$script_dir/claude-hooks/gate-pr.sh" "$r/scripts/claude-hooks/"
chmod +x "$r/scripts/gates.sh"
(
  cd "$r"
  git init -q
  git config user.email spec@example.invalid
  git config user.name spec
  git checkout -q -b main
  git add -A
  git commit -qm base
) >/dev/null

fail=0
check() { # <label> <expected> <actual>
  if [ "$2" = "$3" ]; then
    printf 'ok   %s\n' "$1"
  else
    printf 'FAIL %s: expected [%s], got [%s]\n' "$1" "$2" "$3"
    fail=1
  fi
}

case_n=0
# <paths...> — a fresh branch off main whose diff is exactly these paths (created or edited).
diff_of() {
  case_n=$((case_n + 1))
  (
    cd "$r"
    git checkout -q -f main
    git clean -qfdx
    git checkout -q -b "case-$case_n"
    for p in "$@"; do
      mkdir -p "$(dirname "$p")"
      printf 'edit %s\n' "$case_n" >> "$p"
    done
  )
}
required() { (cd "$r" && /bin/bash scripts/gates.sh --required-online 2>/dev/null) | tr '\n' ' ' | sed 's/ $//'; }
required_err() { (cd "$r" && /bin/bash scripts/gates.sh --required-online 3>&1 1>/dev/null 2>&3); }

ALL4="doctor.online level.online songs copy"

diff_of src-tauri/src/commands/songs.rs
check "songs.rs → songs" "songs" "$(required)"
for f in src-tauri/src/leveller.rs src-tauri/src/commands/doctor.rs src-tauri/src/probe_api/scene_jobs.rs; do
  diff_of "$f"
  check "$f → doctor.online level.online" "doctor.online level.online" "$(required)"
done
for f in src-tauri/src/session.rs src-tauri/src/blockcaps.rs src-tauri/src/hid/write_completion.rs \
  e2e/fixtures/scenario.ts scripts/e2e.sh scripts/lib/e2e-lane.sh; do
  diff_of "$f"
  check "$f → all four" "$ALL4" "$(required)"
done
diff_of src-tauri/src/bin/probe.rs
check "bin/probe.rs → ANY" "ANY" "$(required)"
for f in notes/e2e-test-plan.md scripts/gate-map.tsv scripts/gates.sh .github/workflows/ci.yml \
  src/App.tsx src-tauri/src/sim_device.rs e2e/specs/level.spec.ts src-tauri/src/probe_api/inspect.rs; do
  diff_of "$f"
  check "$f → nothing" "" "$(required)"
done
diff_of src-tauri/src/foo.rs
check "unmapped src-tauri/src/foo.rs → all four" "$ALL4" "$(required)"
check "unmapped file is named as unclassified" 1 "$(required_err | grep -c 'unclassified: src-tauri/src/foo.rs')"
diff_of src-tauri/src/commands/songs.rs src-tauri/src/leveller.rs
check "songs.rs + leveller.rs → canonical order" "doctor.online level.online songs" "$(required)"
diff_of src-tauri/src/commands/bulk_replace.rs
check "unexercised file → ANY" "ANY" "$(required)"
check "unexercised file prints an advisory" 1 "$(required_err | grep -c 'advisory (unexercised)')"
diff_of e2e/specs/doctor-apply.online.spec.ts
check "on-demand spec → nothing, with an advisory" "|1" "$(required)|$(required_err | grep -c 'advisory (on-demand)')"

# ── the PR hook, fed payloads ────────────────────────────────────────────────────
stamp_dir() { (cd "$r" && printf '%s/tmp-gates' "$(git rev-parse --absolute-git-dir)"); }
key() { (cd "$r" && /bin/bash scripts/gates.sh --key); }
green() { mkdir -p "$(stamp_dir)"; : > "$(stamp_dir)/green-$(key)"; }
online() { (cd "$r" && /bin/bash scripts/gates.sh --record-online "$@" >/dev/null); }
hook() { # <payload> — print the hook's exit code
  sed "s|__CWD__|$r|" "$payloads/$1.json" > "$work/payload.json"
  rc=0
  (cd "$work" && /bin/bash "$r/scripts/claude-hooks/gate-pr.sh" < "$work/payload.json" >/dev/null 2>&1) || rc=$?
  printf '%s' "$rc"
}

diff_of notes/x.md
check "hook: not a create or merge → allowed" 0 "$(hook other)"
check "hook: no green stamp → blocked" 2 "$(hook create)"
green
check "hook: green, docs-only → allowed" 0 "$(hook create)"

diff_of src-tauri/src/commands/songs.rs
green
check "hook: green, songs.rs, no online stamp → blocked" 2 "$(hook create)"
online copy
check "hook: online stamp lacks songs → blocked" 2 "$(hook create)"
online songs
check "hook: merged stamp covers songs → allowed" 0 "$(hook create)"
check "hook: merge command, same tree → allowed" 0 "$(hook merge)"

diff_of src-tauri/src/session.rs
green
online songs copy
check "hook: session.rs with a partial stamp → blocked" 2 "$(hook create)"
online doctor.online level.online
check "hook: session.rs with all four merged → allowed" 0 "$(hook create)"

diff_of src-tauri/src/bin/probe.rs
green
check "hook: probe.rs (ANY) with no online stamp → blocked" 2 "$(hook create)"
online songs
check "hook: probe.rs (ANY) with any spec stamped → allowed" 0 "$(hook create)"

if [ "$fail" -ne 0 ]; then
  printf 'gate-pr spec: FAILED\n' >&2
  exit 1
fi
printf 'gate-pr spec: all checks passed\n'
