#!/bin/bash
# check-doc-anchors.sh — WARN-ONLY staleness guard for doc-cited symbols.
#
# Extracts backtick-quoted, identifier-shaped tokens from CLAUDE.md and the
# skills' SKILL.md files and warns about any that no longer resolve against
# the git tree (exact path, code/reference grep, basename, or head-split
# grep). Renamed a doc-cited symbol? Run this. Always exits 0 — promote to a
# blocking gate only once the warn list is stably empty.
#
# Scope is deliberately SKILL.md-level: the skills' references/*.md are the
# depth layer and legitimately cite device-side vocabulary that never appears
# in this tree; they also serve as resolution EVIDENCE for SKILL.md citations.
#
# bash-3.2-safe (macOS /bin/bash): no mapfile, no associative arrays.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Tokens that legitimately never resolve in this tree:
# - external tools + lint-rule/preset vocabulary
# - device/firmware-side vocabulary (preset-JSON keys, firmware rejection
#   strings, on-device UI labels)
# - deliberately OFF-REPO catalog-generator sources (they live on the
#   maintainer's machine; only their names are cited)
# - symbols cited as DELETED on purpose ("do not reintroduce")
# - placeholder patterns used by the docs themselves
ALLOW='cliclick
osascript
pgrep
pkill
lsof
screencapture
xattr
notarytool
stapler
python3
rustfmt
commitlint
semantic-release
prettier
eslint
vitest
playwright
pyloudnorm
ebur128
coreaudiod
eslint-disable
@ts-ignore
@ts-expect-error
@ts-nocheck
strictTypeChecked
stylisticTypeChecked
parserOptions.projectService
react-hooks/refs
react-hooks/set-state-in-effect
react-refresh/only-export-components
react/prop-types
react-in-jsx-scope
no-unnecessary-condition
no-confusing-void-expression
no-misused-promises
restrict-template-expressions
ftswStates
BlockPresetLimitReached
Thru
Pre-Roll
UNDO
PLAY/STOP
RECORD/OVERDUB
Move/Delete
ACD_FxLoop3/4/3_4
ACD_TMSpring63/65
pipeline.py
expand_catalog.py
colorcheck.py
BASS_FORM
tm-stomp-server
Assets.car
derive_key
learn_key
models/halfStack.ts
nodeXxx
XxxProps
cavpedro@gmail.com
mapfile'

DOCS="CLAUDE.md"
for f in .claude/skills/*/SKILL.md; do
  [ -f "$f" ] && DOCS="$DOCS $f"
done

TREE=$(git ls-files)

# Backticked spans >=4 chars, filtered to identifier/path-shaped tokens
# (letters/digits/_ . / : - @ only — prose, commands with spaces, and
# wire-byte literals drop out here).
TOKENS=$(grep -ho '`[^`]\{3,\}`' $DOCS \
  | sed 's/^`//; s/`$//' \
  | grep -E '^[A-Za-z_@][A-Za-z0-9_./:@-]*$' \
  | grep -vE '^[0-9a-fA-F]+$' \
  | sort -u)

# Evidence = every tracked file EXCEPT the docs under check, so a stale
# symbol cannot self-confirm (references/, notes/, configs all count).
grep_evidence() {
  git grep -qF "$1" -- . ':(exclude)CLAUDE.md' ':(exclude)*/SKILL.md' 2>/dev/null
}

resolves() {
  tok="$1"
  # 1. allowlist
  printf '%s\n' "$ALLOW" | grep -qxF "$tok" && return 0
  # 2. exact tracked path, or a directory prefix (skill/dir names)
  printf '%s\n' "$TREE" | grep -qxF "$tok" && return 0
  printf '%s\n' "$TREE" | grep -qF "$tok/" && return 0
  # 3. literal appearance in the evidence tree
  grep_evidence "$tok" && return 0
  # 4. path-ish token: basename lookup (catches `audio.rs`, `ui/Menu`,
  #    files cited without their full path)
  case "$tok" in
    */* | *.*)
      base="${tok##*/}"
      printf '%s\n' "$TREE" | grep -qF "/$base" && return 0
      printf '%s\n' "$TREE" | grep -qF "$base" && return 0
      ;;
  esac
  # 5. dotted/member/scoped token: resolve the head (`copyModel.diffToOps`,
  #    `leveller::CANCELLED`, `ACD_FxLoop3/4`)
  head=$(printf '%s' "$tok" | sed 's/[.:#/].*//')
  if [ -n "$head" ] && [ "$head" != "$tok" ]; then
    grep_evidence "$head" && return 0
    printf '%s\n' "$TREE" | grep -qF "/$head" && return 0
  fi
  return 1
}

warn_count=0
warns=""
for tok in $TOKENS; do
  if ! resolves "$tok"; then
    warns="$warns$tok
"
    warn_count=$((warn_count + 1))
  fi
done

total=$(printf '%s\n' "$TOKENS" | grep -c . || true)
if [ "$warn_count" -eq 0 ]; then
  echo "check-doc-anchors: all $total doc-cited tokens resolve."
else
  echo "check-doc-anchors: $warn_count of $total doc-cited tokens do NOT resolve in the tree:"
  printf '%s' "$warns" | sed 's/^/  ? /'
  echo "(warn-only — fix the doc, rename back, or add a deliberate external name to the ALLOW list)"
fi
exit 0
