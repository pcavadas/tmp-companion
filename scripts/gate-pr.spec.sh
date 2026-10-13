#!/usr/bin/env bash
# Offline spec for online spec selection: `gates.sh --required-online` on fixed diffs, the
# release report (`--release-base`, `--device-changes`, `--required-online --since`), and the
# PR hook (scripts/claude-hooks/gate-pr.sh) fed real hook payloads: it checks the green
# stamp alone, since PRs run the offline gates only.
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
# A git hook (e.g. pre-push from a linked worktree) exports GIT_DIR and friends; left set,
# the scratch repo's `git init` / `git config` / `checkout` act on the CALLER's repository.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY GIT_PREFIX
# ...and the rest of the repo-local set git itself clears for a subprocess (GIT_CONFIG,
# GIT_CONFIG_PARAMETERS, GIT_ALTERNATE_OBJECT_DIRECTORIES, ...). Word-split on purpose.
# shellcheck disable=SC2046
unset $(git rev-parse --local-env-vars 2>/dev/null)

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
  # Name the unborn branch directly: `checkout -b main` fails where init.defaultBranch is
  # already main, so the fixture must not depend on the caller's global default.
  git symbolic-ref HEAD refs/heads/main
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

ALL="doctor.online level.online songs copy doctor-apply.online"

diff_of src-tauri/src/commands/songs.rs
check "songs.rs → songs" "songs" "$(required)"
for f in src-tauri/src/doctor.rs src-tauri/src/probe_api/scene_jobs.rs; do
  diff_of "$f"
  check "$f → doctor.online level.online" "doctor.online level.online" "$(required)"
done
# Doctor persistence (apply/save/discard, and the restore they share) also needs the one
# spec that reads a save and a discard back from the device.
for f in src-tauri/src/commands/doctor.rs src-tauri/src/leveller.rs e2e/specs/doctor-apply.online.spec.ts; do
  diff_of "$f"
  check "$f → doctor.online level.online doctor-apply.online" \
    "doctor.online level.online doctor-apply.online" "$(required)"
done
for f in src-tauri/src/session/mod.rs src-tauri/src/session/link.rs src-tauri/src/session/timing.rs \
  src-tauri/src/session/preset.rs src-tauri/src/session/songs.rs src-tauri/src/blockcaps.rs src-tauri/src/hid/write_completion.rs \
  e2e/fixtures/scenario.ts scripts/e2e.sh scripts/lib/e2e-lane.sh; do
  diff_of "$f"
  check "$f → all five" "$ALL" "$(required)"
done
diff_of src-tauri/src/session/structural.rs
check "session/structural.rs → copy" "copy" "$(required)"
diff_of src-tauri/src/session/ftsw.rs
check "session/ftsw.rs → doctor.online level.online" "doctor.online level.online" "$(required)"
# A session command module that touches a wait primitive or a window const is transport.
diff_of src-tauri/src/session/ftsw.rs
printf '        self.pump_silent(LIVE_BEAT_MS)?;\n' >> "$r/src-tauri/src/session/ftsw.rs"
check "session/ftsw.rs touching a pump → all five" "$ALL" "$(required)"
check "the timing escalation names the file" 1 "$(required_err | grep -c 'advisory: src-tauri/src/session/ftsw.rs touches session timing')"
for f in src-tauri/src/session/tests.rs src-tauri/src/session/boundary_tests.rs src-tauri/src/session/wire_golden.txt; do
  diff_of "$f"
  check "$f → nothing" "" "$(required)"
done
diff_of src-tauri/src/bin/probe.rs
check "bin/probe.rs → ANY" "ANY" "$(required)"
for f in notes/e2e-test-plan.md scripts/gate-map.tsv scripts/gates.sh .github/workflows/ci.yml \
  src/App.tsx src-tauri/src/sim_device.rs e2e/specs/level.spec.ts src-tauri/src/probe_api/inspect.rs; do
  diff_of "$f"
  check "$f → nothing" "" "$(required)"
done
diff_of src-tauri/src/foo.rs
check "unmapped src-tauri/src/foo.rs → all five" "$ALL" "$(required)"
check "unmapped file is named as unclassified" 1 "$(required_err | grep -c 'unclassified: src-tauri/src/foo.rs')"
diff_of src-tauri/src/commands/songs.rs src-tauri/src/leveller.rs
check "songs.rs + leveller.rs → canonical order" "doctor.online level.online songs doctor-apply.online" "$(required)"
diff_of src-tauri/src/commands/bulk_replace.rs
check "unexercised file → ANY" "ANY" "$(required)"
check "unexercised file prints an advisory" 1 "$(required_err | grep -c 'advisory (unexercised)')"
diff_of scripts/hw-e2e.sh
check "on-demand script → nothing, with an advisory" "|1" "$(required)|$(required_err | grep -c 'advisory (on-demand)')"

# ── the PR hook, fed payloads ────────────────────────────────────────────────────
stamp_dir() { (cd "$r" && printf '%s/tmp-gates' "$(git rev-parse --absolute-git-dir)"); }
key() { (cd "$r" && /bin/bash scripts/gates.sh --key); }
green() { mkdir -p "$(stamp_dir)"; : > "$(stamp_dir)/green-$(key)"; }
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

# PRs run the offline gates only: a device-facing diff still needs its green stamp, and
# nothing more. The online suite runs at release (scripts/pre-release.sh).
diff_of src-tauri/src/session/link.rs
check "hook: device-facing diff, no green stamp → blocked" 2 "$(hook create)"
green
check "hook: device-facing diff, green → allowed" 0 "$(hook create)"
check "hook: merge command, same tree → allowed" 0 "$(hook merge)"
printf 'x\n' >> "$r/src-tauri/src/session/link.rs"
check "hook: an edit after the green run → blocked" 2 "$(hook create)"

# ── the release report: everything since the last release tag ─────────────────────
rel() { (cd "$r" && /bin/bash scripts/gates.sh "$@" 2>/dev/null); }
commit_paths() { # <subject> <paths...>
  (
    cd "$r"
    msg="$1"
    shift
    for p in "$@"; do
      mkdir -p "$(dirname "$p")"
      printf 'release %s\n' "$msg" >> "$p"
    done
    git add -A
    git commit -qm "$msg"
  ) >/dev/null
}
(
  cd "$r"
  git checkout -q -f main
  git clean -qfdx
  git checkout -q -b release
) >/dev/null
check "no release tag → --release-base fails" 1 "$(rel --release-base >/dev/null; echo $?)"
check "no release tag → --device-changes fails" 1 "$(rel --device-changes >/dev/null; echo $?)"
(cd "$r" && git tag v1.0.0) >/dev/null
commit_paths "docs only" notes/x.md
commit_paths "songs change" src-tauri/src/commands/songs.rs notes/y.md
commit_paths "fake change" src-tauri/src/sim_device.rs
check "--release-base names the tag" "v1.0.0" "$(rel --release-base)"
dc="$(rel --device-changes)"
check "--device-changes lists the device-facing commit" 1 "$(printf '%s\n' "$dc" | grep -c ' songs change$')"
check "--device-changes names its file and class" 1 "$(printf '%s\n' "$dc" | grep -c 'songs.src-tauri/src/commands/songs.rs$')"
check "--device-changes skips prose and offline-oracle commits" 0 "$(printf '%s\n' "$dc" | grep -c -e ' docs only$' -e ' fake change$')"
check "--device-changes counts one change" 1 "$(printf '%s\n' "$dc" | grep -c '^1 device-facing change(s) since v1.0.0$')"
check "--device-changes ends with the specs the range requires" 1 "$(printf '%s\n' "$dc" | grep -c '^online specs this range requires: songs$')"
check "--required-online --since the tag → songs" "songs" "$(rel --required-online --since v1.0.0 | tr '\n' ' ' | sed 's/ $//')"
check "--since with no ref → exit 2" 2 "$(rel --required-online --since >/dev/null; echo $?)"
check "--since an unknown ref → exit 2" 2 "$(rel --required-online --since nope >/dev/null; echo $?)"
(cd "$r" && git tag v1.1.0) >/dev/null
check "a newer tag moves the base" "v1.1.0" "$(rel --release-base)"
check "nothing since the newer tag" 1 "$(rel --device-changes | grep -c '^0 device-facing change(s) since v1.1.0$')"

if [ "$fail" -ne 0 ]; then
  printf 'gate-pr spec: FAILED\n' >&2
  exit 1
fi
printf 'gate-pr spec: all checks passed\n'
