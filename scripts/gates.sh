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
#                                    AND covers every spec in the canonical online
#                                    set (see --online-spec-set)
#   scripts/gates.sh --record-online <spec>...
#                                    write an ONLINE stamp for this tree naming the
#                                    specs that passed (called by the online e2e
#                                    runner AFTER it passes — this script never
#                                    runs the device itself)
#   scripts/gates.sh --online-spec-set  print the canonical online spec set, one
#                                    name per line (read-only; the single source
#                                    of truth for the full online tier)
#   scripts/gates.sh --key            print the tree KEY for the current working tree
#                                    (read-only; lets a caller — e.g. the online e2e
#                                    runner — snapshot the key before a long run and
#                                    compare it after, to detect a mid-run edit)
#
# ── Scope detection ────────────────────────────────────────────────────────
# Changed files = the working tree vs origin/main (merge-base) + untracked. The
# union is classified:
#   docs-only (*.md, docs/, notes/)     → no gates (near-instant)
#   frontend  (src/, e2e/, ts/js/json)  → bun lint + typecheck + test + fmt-check
#   rust      (src-tauri/)              → cargo clippy (e2e feat) + fmt + test --lib
#   e2e-relev (any code side, e2e/,      → offline e2e (bun run e2e)
#              scripts/e2e.sh)
#   *.sh touched                        → shellcheck (only if installed) +
#                                          tauri-dev-env.spec.sh
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
  # Prose (*.md, docs/) is excluded from the key so documentation edits don't
  # orphan a green/online stamp; code and config still re-key. '*.md' must stay
  # quoted (git pathspec, not shell glob) — an unquoted glob would expand against
  # the CWD instead of being passed through to git. The `|| true` guard covers a
  # tree with no md files (git rm errors on a pathspec that matches nothing).
  # --ignore-unmatch: without it, ONE unmatched pathspec aborts the whole `git rm`
  # atomically (nothing is removed) — e.g. a tree with no docs/ dir at all would
  # silently leave prose IN the key, quietly undoing the exclusion above.
  GIT_INDEX_FILE="$tmp_index" git rm -r --cached -q --ignore-unmatch -- '*.md' docs/ >/dev/null 2>&1 || true
  # COVERAGE.md IS CODE, not prose: `fixture_gates` (src-tauri/src/lib.rs) parses it at
  # TEST TIME to cross-check e2e spec coverage, so an edit to it must still re-key the
  # stamps like any other code change — re-add it after the blanket *.md exclusion above.
  GIT_INDEX_FILE="$tmp_index" git add -- e2e/fixtures/COVERAGE.md 2>/dev/null || true
  tree="$(GIT_INDEX_FILE="$tmp_index" git write-tree 2>/dev/null || true)"
  rm -f "$tmp_index"
  printf '%s' "$tree"
}

# Canonical online spec set — the SINGLE SOURCE OF TRUTH for the full online
# tier, kept on ONE greppable line so a Rust gate (src-tauri/src/lib.rs) can
# parse it verbatim and assert it against scripts/e2e.sh's own hand-maintained
# default-spec literal (the two are separate mirrors on purpose — see that
# gate's doc comment — this line is what it diffs against). Consumed here by
# --check-online (which specs a stamp must cover) and --online-spec-set.
ONLINE_SPEC_SET="doctor.online level.online songs copy"
online_spec_set() {
  # shellcheck disable=SC2086  # deliberate word-split: one spec name per printf line
  printf '%s\n' $ONLINE_SPEC_SET
}

KEY="$(compute_key)"
green_stamp="$stamp_dir/green-$KEY"
online_stamp="$stamp_dir/online-$KEY"

write_stamp() { # <stamp-path> <prefix> [content]  — one stamp per tree; orphan the
                 # stale ones. No content → empty file (green stamp path, unchanged).
  mkdir -p "$stamp_dir"
  rm -f "$stamp_dir/$2"-*
  if [ -n "${3:-}" ]; then
    printf '%s\n' "$3" > "$1"
  else
    : > "$1"
  fi
}

case "$MODE" in
  --check)
    if [ -f "$green_stamp" ]; then exit 0; fi
    printf 'gates: no fresh green stamp for this tree — run scripts/gates.sh\n' >&2
    exit 1
    ;;
  --check-online)
    if [ ! -f "$online_stamp" ]; then
      printf 'gates: no fresh ONLINE stamp for this tree — run the online e2e lane\n' >&2
      exit 1
    fi
    covered=1
    while IFS= read -r spec; do
      [ -z "$spec" ] && continue
      grep -qxF "$spec" "$online_stamp" || covered=0
    done < <(online_spec_set)
    if [ "$covered" -eq 1 ]; then exit 0; fi
    printf 'gates: ONLINE stamp does not cover the full online tier (found: %s)\n' \
      "$(tr '\n' ' ' < "$online_stamp")" >&2
    exit 1
    ;;
  --record-online)
    shift
    if [ "$#" -eq 0 ]; then
      printf 'gates: --record-online requires the spec names that passed, e.g.\n' >&2
      printf '  gates.sh --record-online doctor.online level.online songs copy\n' >&2
      printf 'the runner records this automatically on a full passing lane — manual\n' >&2
      printf 'recording must name the specs that actually passed.\n' >&2
      exit 1
    fi
    online_content="$(printf '%s\n' "$@")"
    write_stamp "$online_stamp" online "$online_content"
    printf 'gates: online stamp recorded for this tree (%s)\n' "$*"
    exit 0
    ;;
  --online-spec-set)
    online_spec_set
    exit 0
    ;;
  --key)
    printf '%s\n' "$KEY"
    exit 0
    ;;
  run) ;;
  *)
    printf 'gates: unknown mode %s (use: --check | --check-online | --record-online <spec>... | --online-spec-set | --key | no arg)\n' "$MODE" >&2
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

# <newline-separated paths> — run one file per line through shellcheck; a
# `while read` loop (not `for f in $(...)`) so a path with a space or glob
# char is preserved verbatim, not word-split/globbed.
shellcheck_files() {
  local rc=0 f
  while IFS= read -r f; do
    if [ -z "$f" ] || [ ! -f "$f" ]; then
      continue
    fi
    shellcheck "$f" || rc=1
  done <<SHFILES
$1
SHFILES
  return "$rc"
}

if [ "$want_shell" -eq 1 ] && command -v shellcheck >/dev/null 2>&1; then
  # Filter out deleted files (below) so shellcheck doesn't fail on "No such file or directory"
  sh_files_list="$(printf '%s\n' "$changed" | grep -E '\.sh$' || true)"
  if [ -n "$sh_files_list" ]; then
    run_gate "shellcheck" shellcheck_files "$sh_files_list"
  fi
fi

if [ "$want_shell" -eq 1 ] && [ -f scripts/tauri-dev-env.spec.sh ]; then
  run_gate "tauri-dev-env spec" /bin/bash scripts/tauri-dev-env.spec.sh
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
  # ALSO under `--features e2e`: some guards only COMPILE there. `settle_ms`'s offline-collapse
  # branch is the one that matters — regress its predicate to `!e2e_online()` and the settles
  # would silently zero on real hardware, yet the default-feature run above still passes (it
  # compiles the identity path) and the offline suite stays green (it wants zeros). Without
  # this lane the guard exists but nothing ever executes it.
  run_gate "rust tests (e2e feature)" sh -c 'cd src-tauri && cargo test --lib --features e2e'
fi

if [ "$want_e2e" -eq 1 ]; then
  ensure_dist
  run_gate "offline e2e" bun run e2e
fi

write_stamp "$green_stamp" green
printf '\ngates: all gates green — stamp written (%s)\n' "green-$KEY"
