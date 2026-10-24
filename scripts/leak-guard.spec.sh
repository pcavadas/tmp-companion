#!/usr/bin/env bash
# Offline spec for scripts/leak-guard.sh in its staged (pre-commit) mode: a staged binary
# bigger than a pipe buffer passes, and a staged public marker is refused.
#
# Fixture: a scratch git repo holding this checkout's leak-guard.sh, with no private
# pattern source, so only the public markers apply. The marker is assembled at run time,
# because this file is scanned like any other.
#
# Bash 3.2-compatible (.claude/rules/shell-scripts.md); gates.sh runs it under /bin/bash.
set -euo pipefail
# A git hook exports GIT_DIR and friends; left set, the scratch repo's git commands act
# on the CALLER's repository.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY GIT_PREFIX
# shellcheck disable=SC2046
unset $(git rev-parse --local-env-vars 2>/dev/null)

script_dir="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
r="$work/repo"
mkdir -p "$r/scripts"
cp "$script_dir/leak-guard.sh" "$r/scripts/"
git -C "$r" init -q
export LEAK_GUARD_PRIVATE=''
export LEAK_GUARD_PRIVATE_FILE="$work/no-private-patterns"

fail=0
check() { # <name> <expected exit> — runs the guard on the scratch repo's index
  local rc=0
  (cd "$r" && /bin/bash scripts/leak-guard.sh >"$work/out" 2>&1) || rc=$?
  if [ "$rc" -eq "$2" ]; then
    printf 'ok   %s\n' "$1"
  else
    printf 'FAIL %s: exit %s, expected %s\n' "$1" "$rc" "$2"
    sed 's/^/     /' "$work/out"
    fail=1
  fi
}

# 1 MiB of random bytes: binary, and far bigger than a pipe buffer.
head -c 1048576 /dev/urandom >"$r/big.png"
git -C "$r" add big.png
check "a staged binary bigger than a pipe buffer passes" 0

printf 'calls FUN_%s here\n' 0040a1b2 >"$r/leak.txt"
git -C "$r" add leak.txt
check "a staged public marker is refused" 1

exit "$fail"
