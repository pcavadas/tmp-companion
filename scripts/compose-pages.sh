#!/usr/bin/env bash
# Overlay Git-backed package repositories onto the current marketing website.
set -euo pipefail
[ "$#" -eq 3 ] || { echo "usage: compose-pages.sh <website-dir> <repo-remote> <output-dir>" >&2; exit 2; }
SOURCE=$1 REMOTE=$2 OUTPUT=$3
case "$OUTPUT" in ''|/|.) echo "refusing an unsafe output directory" >&2; exit 2 ;; esac
mkdir -p "$OUTPUT"
cp -R "$SOURCE/." "$OUTPUT/"
work="$(mktemp -d "${TMPDIR:-/tmp}/tmp-companion-pages.XXXXXX")"
trap 'rm -rf "$work"' EXIT
git_auth() { git -c credential.helper='!gh auth git-credential' "$@"; }
BRANCH=codex/linux-package-repos
if git_auth ls-remote --exit-code --heads "$REMOTE" "$BRANCH" >/dev/null; then
  git_auth clone --quiet --single-branch --depth 1 --branch "$BRANCH" "$REMOTE" "$work/repos"
  for format in apt rpm; do
    [ -d "$work/repos/$format" ] || continue
    rm -rf "${OUTPUT:?}/$format"
    cp -R "$work/repos/$format" "$OUTPUT/$format"
  done
else
  code=$?
  [ "$code" -eq 2 ] || exit "$code"
  echo "compose-pages: repository branch is not published yet; serving website"
fi
