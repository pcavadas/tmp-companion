#!/bin/bash
#
# gates.sh — scoped local verification-gate runner + green-stamp writer.
#
# The forcing function behind the "a PR is never opened / pushed red" policy.
# It mirrors CI's build-test gates LOCALLY, scoped to what the change actually
# touches, and records a per-tree "green stamp" so repeat checks are instant.
#
#   scripts/gates.sh                 run the gates for the current change scope,
#                                    write a green stamp on full pass
#   scripts/gates.sh --check         exit 0 iff a green stamp matches this tree,
#                                    else non-zero (used by the pre-push hook +
#                                    the Claude gh-pr PreToolUse hook)
#   scripts/gates.sh --check-online  exit 0 iff an ONLINE stamp matches this tree
#   scripts/gates.sh --record-online  write an ONLINE stamp for this tree (called
#                                    by the online e2e runner AFTER it passes —
#                                    this script never runs the device itself)
#
# ── Scope detection ────────────────────────────────────────────────────────
# Changed files = the working tree vs origin/main (merge-base) + untracked. The
# union is classified:
#   docs-only (*.md, docs/, notes/)     → no gates (near-instant)
#   frontend  (src/, e2e/, ts/js/json)  → bun lint + typecheck + test + fmt-check
#   rust      (src-tauri/)              → cargo clippy (e2e feat) + fmt + test --lib
#   e2e-relev (any code side, e2e/,      → offline e2e (bun run e2e)
#              scripts/e2e.sh)
#   *.sh touched                        → shellcheck (only if installed)
#
# ── Stamp model ────────────────────────────────────────────────────────────
# The stamp KEY is the git tree-object id of the whole working tree (tracked +
# untracked non-ignored), computed via a throwaway index (compute_key). Because
# it hashes CONTENT, it is COMMIT-INVARIANT: a green run before `git commit`
# stays a hit on the pre-push --check (commit changes no bytes), and any real
# edit changes the key and orphans the old stamp. Stamps live under
# $(git rev-parse --git-dir)/tmp-gates (per-worktree, OUTSIDE the work tree; note
# .git is a FILE in a worktree, so --git-dir is required).
#
# ── Relationship to CI ─────────────────────────────────────────────────────
# CI (.github/workflows/ci.yml) stays the AUTHORITATIVE remote gate; this is the
# local/agent enforcement layer that keeps a red tree from ever reaching a push
# or a PR. It is intentionally a subset+mirror, not a replacement.
#
# Portable to macOS system /bin/bash 3.2.57 + BSD tools: empty-array-safe under
# `set -u` (no bare "${arr[@]}"), no GNU-only flags. Test under /bin/bash.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

MODE="${1:-run}"

git_dir="$(git rev-parse --git-dir)"
stamp_dir="$git_dir/tmp-gates"

# Merge-base against the upstream default branch; fall back to local main, then
# to HEAD (only uncommitted changes visible). Shared by the key + scope.
base=""
if git rev-parse --verify --quiet origin/main >/dev/null; then
  base="$(git merge-base HEAD origin/main 2>/dev/null || true)"
elif git rev-parse --verify --quiet main >/dev/null; then
  base="$(git merge-base HEAD main 2>/dev/null || true)"
fi
[ -n "$base" ] || base="$(git rev-parse HEAD 2>/dev/null || true)"

compute_key() {
  # Content hash of the whole working tree (tracked + untracked non-ignored),
  # via a THROWAWAY index so the real index/HEAD are untouched. This is the
  # git tree object id of "everything that would be committed", so it is
  # COMMIT-INVARIANT: staging + committing the same bytes yields the same key,
  # and it hashes an untracked file identically to its committed self — so a
  # green run before `git commit` is still a hit on the pre-push --check.
  # Ignored files (dist/, node_modules/) are excluded by git's ignore rules.
  local tmp_index tree
  tmp_index="$(mktemp)"
  cp -f "$git_dir/index" "$tmp_index" 2>/dev/null || true   # warm the stat cache → fast add
  GIT_INDEX_FILE="$tmp_index" git add -A >/dev/null 2>&1 || true
  tree="$(GIT_INDEX_FILE="$tmp_index" git write-tree 2>/dev/null || true)"
  rm -f "$tmp_index"
  printf '%s' "$tree"
}

KEY="$(compute_key)"
green_stamp="$stamp_dir/green-$KEY"
online_stamp="$stamp_dir/online-$KEY"

write_stamp() { # <stamp-path> <prefix>  — one stamp per tree; orphan the stale ones
  mkdir -p "$stamp_dir"
  rm -f "$stamp_dir/$2"-*
  : > "$1"
}

case "$MODE" in
  --check)
    if [ -f "$green_stamp" ]; then exit 0; fi
    printf 'gates: no fresh green stamp for this tree — run scripts/gates.sh\n' >&2
    exit 1
    ;;
  --check-online)
    if [ -f "$online_stamp" ]; then exit 0; fi
    printf 'gates: no fresh ONLINE stamp for this tree — run the online e2e lane\n' >&2
    exit 1
    ;;
  --record-online)
    write_stamp "$online_stamp" online
    printf 'gates: online stamp recorded for this tree\n'
    exit 0
    ;;
  run) ;;
  *)
    printf 'gates: unknown mode %s (use: --check | --check-online | --record-online | no arg)\n' "$MODE" >&2
    exit 2
    ;;
esac

# ── run mode ───────────────────────────────────────────────────────────────
if [ -f "$green_stamp" ]; then
  printf 'gates: already green for this tree\n'
  exit 0
fi

# Union of changed files: everything in the working tree that differs from base,
# plus untracked non-ignored files.
changed="$(
  {
    if [ -n "$base" ]; then git diff --name-only "$base"; fi
    git ls-files --others --exclude-standard
  } | LC_ALL=C sort -u
)"

if [ -z "$changed" ]; then
  printf 'gates: no changes vs origin/main — nothing to check\n'
  write_stamp "$green_stamp" green
  exit 0
fi

docs_only=1
want_frontend=0
want_rust=0
want_e2e=0
want_shell=0

while IFS= read -r f; do
  if [ -z "$f" ]; then continue; fi
  case "$f" in
    *.md | docs/* | notes/*) ;;                # docs — no gate on their own
    *) docs_only=0 ;;
  esac
  case "$f" in
    src-tauri/*)                 want_rust=1; want_e2e=1 ;;
    src/* | e2e/*)               want_frontend=1; want_e2e=1 ;;
    scripts/e2e.sh)              want_e2e=1 ;;
    # Root TS/JS config + entry that affects lint/tsc/test/build. Deliberately
    # NOT a bare *.json (so .claude/settings.json / .github/*.json don't trip it).
    *.ts | *.tsx | *.config.* | tsconfig*.json | package.json | index.html)
                                 want_frontend=1; want_e2e=1 ;;
  esac
  case "$f" in
    *.sh) want_shell=1 ;;
  esac
done <<EOF
$changed
EOF

if [ "$docs_only" -eq 1 ]; then
  printf 'gates: docs-only change — no gates required\n'
  write_stamp "$green_stamp" green
  exit 0
fi

# tauri-build's generate_context! panics without dist/index.html (gitignored,
# absent in a fresh worktree). A stub satisfies it for the cargo gates.
ensure_dist() {
  if [ ! -f dist/index.html ]; then
    mkdir -p dist
    printf '<!doctype html><title>stub</title>\n' > dist/index.html
  fi
}

run_gate() { # <label> <cmd...>
  local label="$1"
  shift
  printf '\ngates: ── %s ──\n' "$label"
  if ! "$@"; then
    printf '\ngates: FAILED at %s — fix it, then re-run scripts/gates.sh\n' "$label" >&2
    exit 1
  fi
}

if [ "$want_shell" -eq 1 ] && command -v shellcheck >/dev/null 2>&1; then
  # Filter out deleted files so shellcheck doesn't fail on "No such file or directory"
  sh_files=""
  for f in $(printf '%s\n' "$changed" | grep -E '\.sh$' || true); do
    if [ -f "$f" ]; then
      sh_files="$sh_files $f"
    fi
  done
  if [ -n "$sh_files" ]; then
    # shellcheck disable=SC2086
    run_gate "shellcheck" shellcheck $sh_files
  fi
fi

if [ "$want_frontend" -eq 1 ]; then
  run_gate "lint (eslint)"       bun run lint
  run_gate "typecheck (tsc)"     bun run typecheck
  run_gate "frontend tests"      bun run test
  run_gate "format (prettier)"   bun run format:check
fi

if [ "$want_rust" -eq 1 ]; then
  ensure_dist
  run_gate "clippy (e2e feature)" sh -c 'cd src-tauri && cargo clippy --all-targets --features e2e -- -D warnings'
  run_gate "rustfmt --check"      sh -c 'cd src-tauri && cargo fmt --check'
  run_gate "rust tests"           sh -c 'cd src-tauri && cargo test --lib'
fi

if [ "$want_e2e" -eq 1 ]; then
  ensure_dist
  run_gate "offline e2e" bun run e2e
fi

write_stamp "$green_stamp" green
printf '\ngates: all gates green — stamp written (%s)\n' "green-$KEY"
