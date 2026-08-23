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
# Reads the tool-call JSON on stdin. bash 3.2-safe. Resolves the repo from the
# hook payload's own `cwd` (falling back to the process cwd, then
# CLAUDE_PROJECT_DIR), because gates.sh stamps are per-worktree: in a worktree
# session CLAUDE_PROJECT_DIR still points at the MAIN checkout, so using it
# checked a tree that wasn't the one being PR'd — a green worktree got blocked
# by the main checkout's missing stamp.

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
# PR'd) over the process cwd, so this doesn't rest on an unverified assumption about
# which directory Claude Code spawns PreToolUse hooks in — if it spawns them in the
# project dir instead, resolving via process cwd alone would be a silent no-op that
# still gates the wrong tree while looking correct. Both fall back to
# CLAUDE_PROJECT_DIR when neither is inside a repo.
hook_cwd="$(printf '%s' "$input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("cwd",""))' 2>/dev/null || true)"
repo=""
for cand in "$hook_cwd" "$PWD"; do
  if [ -n "$cand" ]; then
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

# Device-facing diff → also require a fresh online stamp. Scope mirrors
# gates.sh: working tree vs merge-base (origin/main, else local main) + untracked
# — so a NEW untracked device-facing file can't dodge the online requirement.
# audiograph/doctor were previously missed near-misses: `audio\.rs` doesn't match
# `audiograph.rs` (different file), and only the `commands/doctor` wrapper matched,
# not the `doctor.rs` engine it calls into. Online specs + the runner script are
# included too — merging a broken oracle (a spec or the e2e.sh runner itself) would
# silently defeat the online lane's whole purpose. e2e_server.rs/scenario.ts/probe.rs
# are the lane's own bridge/seeder; presets.rs/preset_io.rs/edit_tools.rs are the
# save/clear/import destructive-write class. The online-spec alternatives name ONLY
# the two files in gates.sh's canonical online set (doctor.online, level.online) —
# NOT every `*.online.spec.ts`: the on-demand specs (lib.rs's ON_DEMAND_ONLINE_SPECS,
# e.g. doctor-apply.online) never run in the default lane, so requiring a fresh
# online stamp on their account would block on a run that proves nothing about them.
device_re='src-tauri/src/leveller\.rs|src-tauri/src/footswitch\.rs|src-tauri/src/session\.rs|src-tauri/src/audio\.rs|src-tauri/src/commands/level_|src-tauri/src/commands/doctor|src-tauri/src/hid\.rs|src-tauri/src/proto\.rs|src-tauri/src/blockcaps\.rs|src-tauri/src/backup\.rs|src-tauri/src/backup_read\.rs|src-tauri/src/audiograph\.rs|src-tauri/src/doctor\.rs|src-tauri/src/replace_inplace\.rs|src-tauri/src/probe_api/|src-tauri/src/commands/copy_apply\.rs|src-tauri/src/commands/held_edit\.rs|src-tauri/src/commands/bulk_replace\.rs|src-tauri/src/e2e_server\.rs|e2e/fixtures/scenario\.ts|src-tauri/src/commands/presets\.rs|src-tauri/src/preset_io\.rs|src-tauri/src/commands/edit_tools\.rs|src-tauri/src/bin/probe\.rs|e2e/specs/doctor\.online\.spec\.ts|e2e/specs/level\.online\.spec\.ts|scripts/e2e\.sh'
# shellcheck disable=SC2015  # deliberate best-effort: any failure here must fall through to `true`, never abort the hook under set -e
changed="$(cd "$repo" && {
  b="$(git merge-base HEAD origin/main 2>/dev/null || git merge-base HEAD main 2>/dev/null || true)"
  if [ -n "$b" ]; then git diff --name-only "$b"; fi
  git ls-files --others --exclude-standard
} 2>/dev/null || true)"

if printf '%s\n' "$changed" | grep -Eq "$device_re"; then
  if ! /bin/bash "$gates" --check-online >/dev/null 2>&1; then
    printf 'BLOCKED: device-facing change — run scripts/e2e.sh online — a full passing lane records the online stamp automatically.\n' >&2
    exit 2
  fi
fi

exit 0
