#!/bin/bash
#
# gate-pr.sh — PreToolUse(Bash) hook: a PR must never open/merge red.
#
# On `gh pr create` / `gh pr merge`:
#   - require scripts/gates.sh --check green (fresh green stamp for this tree)
# PRs run the offline gates only (see scripts/pre-release.sh for the online run).
# Exit 2 = block + show stderr to Claude.
#
# Reads the tool-call JSON on stdin. bash 3.2-safe. Resolves the repo from the
# hook payload's own `cwd` (falling back to the process cwd, then
# CLAUDE_PROJECT_DIR), because gates.sh stamps are per-worktree and in a worktree
# session CLAUDE_PROJECT_DIR points at the MAIN checkout.

set -euo pipefail

# Fires on EVERY Bash tool call — slurp stdin once and cheaply pre-filter before
# forking python3. Only `gh` commands can be a PR create/merge.
input="$(cat)"
case "$input" in *gh*) ;; *) exit 0 ;; esac

cmd="$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null || true)"

if [ -z "$cmd" ]; then exit 0; fi

# Match `gh … pr create|merge` even with global options between `gh` and `pr`
# (e.g. `gh --repo owner/repo pr create`) — the `([^[:space:]]+[[:space:]]+)*`
# consumes any intervening flag tokens before the `pr` subcommand.
if ! printf '%s' "$cmd" | grep -Eq 'gh[[:space:]]+([^[:space:]]+[[:space:]]+)*pr[[:space:]]+(create|merge)'; then
  exit 0
fi

# Prefer the hook payload's own `cwd` (the session dir = the worktree actually being
# PR'd) over the process cwd, which may be the project dir. Both fall back to
# CLAUDE_PROJECT_DIR when neither is inside a repo.
hook_cwd="$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("cwd",""))' 2>/dev/null || true)"
repo=""
for cand in "$hook_cwd" "$PWD"; do
  if [ -n "$cand" ]; then
    # shellcheck disable=SC2015  # best-effort: any failure must fall through to `true`
    repo="$(cd "$cand" 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null || true)"
    if [ -n "$repo" ]; then break; fi
  fi
done
[ -n "$repo" ] || repo="${CLAUDE_PROJECT_DIR:-.}"
gates="$repo/scripts/gates.sh"
if [ ! -x "$gates" ]; then
  # Not our repo / gates not installed — don't block.
  exit 0
fi

if ! /bin/bash "$gates" --check >/dev/null 2>&1; then
  printf 'BLOCKED: gates are red/stale — run scripts/gates.sh; a PR must never open red.\n' >&2
  exit 2
fi

exit 0
