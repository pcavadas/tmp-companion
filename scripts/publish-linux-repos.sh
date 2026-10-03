#!/usr/bin/env bash
# Commit generated repositories to their own branch, never to protected main.
set -euo pipefail
[ "$#" -eq 4 ] || { echo "usage: publish-linux-repos.sh <deb-dir> <rpm-dir> <fingerprint> <version>" >&2; exit 2; }
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEB_DIR=$1 RPM_DIR=$2 FPR=$3 VERSION=$4
found=0
for package in "$DEB_DIR"/*.deb "$RPM_DIR"/*.rpm; do [ ! -f "$package" ] || found=1; done
changed() { printf 'changed=%s\n' "$1" >> "${GITHUB_OUTPUT:-/dev/null}"; }
if [ "$found" -eq 0 ]; then
  echo "publish-linux-repos: no artifacts; no signing or Git operations"
  changed false
  exit 0
fi
BRANCH=codex/linux-package-repos
REMOTE="${LINUX_REPO_REMOTE:-https://github.com/${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}.git}"
work="$(mktemp -d "${TMPDIR:-/tmp}/tmp-companion-publish.XXXXXX")"
trap 'rm -rf "$work"' EXIT
git_auth() { git -c credential.helper='!gh auth git-credential' "$@"; }
if git_auth ls-remote --exit-code --heads "$REMOTE" "$BRANCH" >/dev/null; then
  git_auth clone --quiet --single-branch --depth 1 --branch "$BRANCH" "$REMOTE" "$work/repos"
else
  code=$?
  [ "$code" -eq 2 ] || exit "$code"
  git init --quiet -b "$BRANCH" "$work/repos"
  git -C "$work/repos" remote add origin "$REMOTE"
fi
bash "$REPO/scripts/build-linux-repos.sh" "$DEB_DIR" "$RPM_DIR" "$work/repos" "$FPR"
cd "$work/repos"
git config user.name "github-actions[bot]"
git config user.email "github-actions[bot]@users.noreply.github.com"
# git add -A handles either format and pruned blobs without missing pathspecs.
git add -A
if git diff --cached --quiet; then changed false; exit 0; fi
git commit --quiet -m "chore(release): publish Linux repositories for $VERSION"
git_auth push origin "HEAD:refs/heads/$BRANCH"
changed true
