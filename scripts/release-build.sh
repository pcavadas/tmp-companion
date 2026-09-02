#!/usr/bin/env bash
# Release build → notarize → updater manifest. Invoked by semantic-release's
# @semantic-release/exec `prepareCmd` (see .releaserc.json). Runs in the `prepare`
# step, BEFORE @semantic-release/github's `publish`, so any failure here aborts the
# release with nothing half-published.
#
# Args: $1 = release version (e.g. 1.6.0); $2 = base64-encoded release notes.
# Apple creds come from the release-job env: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID.
#
# bash-3.2-safe: /bin/bash on macos-14 is 3.2.57 (see .claude/rules/shell-scripts.md).
# No arrays / mapfile —
# positional params only.
set -euo pipefail

VERSION="${1:?release version required}"
NOTES_B64="${2:-}"
# Where release.yml's download-artifact steps stage the Linux build-deb/build-rpm
# jobs' output, ahead of this script running as semantic-release's prepareCmd.
LINUX_DIR="release-linux"

echo "release-build: bumping src-tauri/tauri.conf.json to $VERSION"
node scripts/bump-version.mjs "$VERSION"

# The Linux jobs resolve their own version from an earlier `semantic-release
# --dry-run` (resolve-version job) and stamp it into version.txt beside their
# bundle. It SHOULD always agree with $VERSION — both analyze the same commits
# at the same SHA — but a disagreement would publish a Linux package labelled
# with the wrong version, which is worse than publishing none. Drop rather than
# fail: a version mismatch is exactly the "Linux never blocks macOS" case.
if [ -f "$LINUX_DIR/version.txt" ]; then
  LINUX_VERSION="$(cat "$LINUX_DIR/version.txt")"
  if [ "$LINUX_VERSION" != "$VERSION" ]; then
    echo "release-build: WARNING Linux build version ($LINUX_VERSION) disagrees with this release ($VERSION) — dropping the Linux artifacts" >&2
    rm -f "$LINUX_DIR"/*.deb "$LINUX_DIR"/*.rpm "$LINUX_DIR/version.txt"
  fi
fi

echo "release-build: building the universal-apple-darwin bundle"
bun run tauri build --target universal-apple-darwin

echo "release-build: locating the DMG"
# Guard the glob: exactly one DMG, or fail loudly (mirrors the .sig count-guard in
# scripts/latest-json.mjs). Unmatched glob stays literal and the -f check catches it.
set -- src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "release-build: expected exactly one DMG, found $#: $*" >&2
  exit 1
fi
DMG="$1"
echo "release-build: DMG = $DMG"

echo "release-build: notarizing the DMG"
xcrun notarytool submit "$DMG" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait

echo "release-build: stapling the DMG"
xcrun stapler staple "$DMG"

echo "release-build: writing latest.json"
node scripts/latest-json.mjs "$VERSION" "$NOTES_B64"
