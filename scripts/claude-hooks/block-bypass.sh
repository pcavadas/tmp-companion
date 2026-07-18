#!/bin/bash
#
# block-bypass.sh — PreToolUse(Bash) hook: refuse gate-bypass on commit/push.
#
# Blocks any Bash command that both mutates git history/remote (git commit /
# git push) AND disables the local hooks (--no-verify, HUSKY=0, or a
# core.hooksPath override). Exit 2 = block + show stderr to Claude.
#
# Reads the tool-call JSON on stdin; parses tool_input.command with python3
# (always present on macOS). bash 3.2-safe.

set -euo pipefail

# Fires on EVERY Bash tool call — slurp stdin once and cheaply pre-filter before
# forking python3. Only git commands can carry the bypasses we block.
input="$(cat)"
case "$input" in *git*) ;; *) exit 0 ;; esac

cmd="$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null || true)"

if [ -z "$cmd" ]; then exit 0; fi

# Only care about git commit/push commands (flags may sit between `git` and the
# subcommand, e.g. `git -c core.hooksPath=… push`), so match order-independently.
if ! printf '%s' "$cmd" | grep -Eq '(^|[^[:alnum:]])git([^[:alnum:]]|$)'; then
  exit 0
fi
if ! printf '%s' "$cmd" | grep -Eq '(^|[^[:alnum:]])(commit|push)([^[:alnum:]]|$)'; then
  exit 0
fi

if printf '%s' "$cmd" | grep -Eq -- '--no-verify|HUSKY=0|core\.hooksPath'; then
  printf 'BLOCKED: hooks-bypass on a git commit/push is against policy — the verification gates must run.\n' >&2
  printf 'Do not use --no-verify / HUSKY=0 / core.hooksPath. Run scripts/gates.sh and let the hooks pass.\n' >&2
  exit 2
fi

exit 0
