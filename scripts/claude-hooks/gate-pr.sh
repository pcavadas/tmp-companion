#!/bin/bash
#
# gate-pr.sh — PreToolUse(Bash) hook: a PR must never open/merge red.
#
# On `gh pr create` / `gh pr merge`:
#   - require scripts/gates.sh --check green (fresh green stamp for this tree)
#   - if the diff vs origin/main touches device-facing paths, ALSO require a
#     fresh ONLINE stamp (the online e2e lane must have run)
# Exit 2 = block + show stderr to Claude.
#
# Reads the tool-call JSON on stdin. bash 3.2-safe. Resolves the repo via
# CLAUDE_PROJECT_DIR (set by Claude Code) so it works from root or a worktree.

set -euo pipefail

# Fires on EVERY Bash tool call — slurp stdin once and cheaply pre-filter before
# forking python3. Only `gh` commands can be a PR create/merge.
input="$(cat)"
case "$input" in *gh*) ;; *) exit 0 ;; esac

cmd="$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null || true)"

if [ -z "$cmd" ]; then exit 0; fi

if ! printf '%s' "$cmd" | grep -Eq 'gh[[:space:]]+pr[[:space:]]+(create|merge)'; then
  exit 0
fi

repo="${CLAUDE_PROJECT_DIR:-.}"
gates="$repo/scripts/gates.sh"
if [ ! -x "$gates" ]; then
  # Not our repo / gates not installed — don't block.
  exit 0
fi

if ! /bin/bash "$gates" --check >/dev/null 2>&1; then
  printf 'BLOCKED: gates are red/stale — run scripts/gates.sh; a PR must never open red.\n' >&2
  exit 2
fi

# Device-facing diff → also require a fresh online stamp. Scope mirrors
# gates.sh: working tree vs merge-base (origin/main, else local main) + untracked
# — so a NEW untracked device-facing file can't dodge the online requirement.
device_re='src-tauri/src/leveller\.rs|src-tauri/src/session\.rs|src-tauri/src/audio\.rs|src-tauri/src/commands/level_|src-tauri/src/commands/doctor'
# shellcheck disable=SC2015  # deliberate best-effort: any failure here must fall through to `true`, never abort the hook under set -e
changed="$(cd "$repo" && {
  b="$(git merge-base HEAD origin/main 2>/dev/null || git merge-base HEAD main 2>/dev/null || true)"
  if [ -n "$b" ]; then git diff --name-only "$b"; fi
  git ls-files --others --exclude-standard
} 2>/dev/null || true)"

if printf '%s\n' "$changed" | grep -Eq "$device_re"; then
  if ! /bin/bash "$gates" --check-online >/dev/null 2>&1; then
    printf 'BLOCKED: device-facing change — run the online e2e lane first (scripts/e2e.sh online … then scripts/gates.sh --record-online).\n' >&2
    exit 2
  fi
fi

exit 0
