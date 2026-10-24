#!/bin/bash
#
# leak-guard.sh — keep PRIVATE content out of this PUBLIC repo (local pre-commit hook).
#
# tmp-companion is public; private tooling shares its Claude skills folder with it
# locally, so private content is easy to stage by accident. This guard refuses a
# commit that adds markers of that content. It runs LOCALLY only (the pre-commit
# hook): a CI check would fire after the push, when the content is already public.
#
#   scripts/leak-guard.sh          scan STAGED content    (git pre-commit hook)
#   scripts/leak-guard.sh --all    scan ALL tracked files (manual audit), from any directory
#
# Patterns (POSIX ERE, combined with '|'):
#   - PUBLIC, below: generic markers that name nothing private.
#   - PRIVATE: every project-specific marker, kept out of this public file. Read from
#     the untracked file $(git rev-parse --git-common-dir)/info/leak-guard-private
#     (one ERE per line; '#' comments and blank lines ignored; LEAK_GUARD_PRIVATE_FILE
#     overrides the path) and from the env var LEAK_GUARD_PRIVATE (one alternation).
#     Without either, only the public markers are checked and a warning is printed.
#
# The deployed report endpoint host (ALLOW_HOST) is functional public config: it is
# stripped from each line before matching, so no pattern can trip on it.
# This file is scanned like any other, so its own patterns are written not to match
# themselves (`[.]` for a literal dot; ALLOW_HOST unescaped, so its line strips itself).
#
# Exits non-zero and prints the offending file:line on any match; exits 2 if the
# combined pattern is not a valid ERE (a broken pattern must never pass silently).
# Portable to macOS /bin/bash 3.2.57 + BSD grep: no arrays, no GNU-only flags,
# no `&& cmd` statements (set -e unsafe); process substitution (not a pipe) keeps
# the loops, and so `status`, in this shell. Test under /bin/bash explicitly, not a
# newer PATH bash.

set -euo pipefail

self="scripts/leak-guard.sh"
cd "$(git rev-parse --show-toplevel)"

# PUBLIC markers: Ghidra function labels and local-only Claude paths.
pattern='FUN_[0-9a-f]{8}|[.]claude/plans|[.]claude/projects/.+/memory'

# PRIVATE patterns: env var, then the untracked local file.
private="${LEAK_GUARD_PRIVATE:-}"
private_file="${LEAK_GUARD_PRIVATE_FILE:-$(git rev-parse --git-common-dir)/info/leak-guard-private}"
if [ -f "$private_file" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    line="$(printf '%s' "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    case "$line" in '' | '#'*) continue ;; esac
    if [ -n "$private" ]; then private="$private|$line"; else private="$line"; fi
  done <"$private_file"
fi
if [ -n "$private" ]; then
  pattern="$pattern|$private"
else
  printf 'leak-guard: WARNING no private pattern file (%s); only generic markers checked\n' "$private_file" >&2
fi

rc=0
printf '' | grep -E "$pattern" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then
  printf 'LEAK-GUARD: the combined pattern is not a valid ERE — fix the private pattern file or LEAK_GUARD_PRIVATE\n' >&2
  exit 2
fi

ALLOW_HOST='tmp-companion-reports.pcavadas.workers.dev'

# <grep-input on stdin> — matching lines (numbered) once ALLOW_HOST is stripped.
scan() { LC_ALL=C sed -e "s/$ALLOW_HOST//g" | grep -nIE "$pattern" 2>/dev/null || true; }

mode="${1:-staged}"
status=0

report() { # <file-label> <grep-output>
  printf 'LEAK-GUARD: private-project marker in %s\n' "$1" >&2
  printf '%s\n' "$2" | sed 's/^/    /' >&2
  status=1
}

if [ "$mode" = "--all" ] || [ "$mode" = "all" ]; then
  # All tracked files, scanned in the working tree. NUL-separated (-z), so a name git
  # would C-quote (a tab, newline, quote or backslash in it) is read verbatim.
  while IFS= read -r -d '' f; do
    if [ ! -f "$f" ]; then continue; fi
    # Plain grep first (fast, skips binaries); re-scan a hit with the allowlist applied.
    out="$(grep -lIE "$pattern" -- "$f" 2>/dev/null || true)"
    if [ -n "$out" ]; then out="$(scan <"$f")"; fi
    if [ -n "$out" ]; then report "$f" "$out"; fi
  done < <(git ls-files -z)
else
  # Staged additions, modifications, renames and type changes, scanned from the index
  # (the exact bytes that would be committed), not the working tree; NUL-separated (-z).
  while IFS= read -r -d '' f; do
    if ! git cat-file -e ":$f" 2>/dev/null; then
      printf 'LEAK-GUARD: cannot read the staged copy of %s — refusing rather than skipping it\n' "$f" >&2
      exit 2
    fi
    # `grep -I` stops reading a binary at its first block, so `git show` of one bigger
    # than a pipe buffer dies of SIGPIPE (141); under pipefail that alone would abort.
    out="$( { git show ":$f" || [ $? -eq 141 ]; } | scan)"
    if [ -n "$out" ]; then report "$f (staged)" "$out"; fi
  done < <(git diff --cached --name-only -z --diff-filter=ACMRT)
fi

if [ "$status" -ne 0 ]; then
  printf '\nRefusing: private tooling content must not enter the public repo.\n' >&2
  printf 'If this is a genuine false positive, tighten the public pattern in %s or the private pattern source.\n' "$self" >&2
  exit 1
fi

printf 'leak-guard: clean\n'
