#!/usr/bin/env bash
# Release build → notarize → updater manifest. Invoked by semantic-release's
# @semantic-release/exec `prepareCmd` (see .releaserc.json). Runs in the `prepare`
# step, BEFORE @semantic-release/github's `publish`, so any failure here aborts the
# release with nothing half-published.
#
# Args: $1 = release version (e.g. 1.6.0); $2 = base64-encoded release notes.
# Apple creds come from the release-job env: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID.
#
# bash-3.2-safe: /bin/bash on macos-14 is 3.2.57 (see CLAUDE.md). No arrays / mapfile —
# positional params only.
set -euo pipefail

VERSION="${1:?release version required}"
NOTES_B64="${2:-}"

echo "release-build: bumping src-tauri/tauri.conf.json to $VERSION"
VERSION="$VERSION" node -e 'const f="src-tauri/tauri.conf.json",fs=require("fs");const j=JSON.parse(fs.readFileSync(f));j.version=process.env.VERSION;fs.writeFileSync(f,JSON.stringify(j,null,2)+"\n")'

echo "release-build: building the aarch64-apple-darwin bundle"
bun run tauri build --target aarch64-apple-darwin

echo "release-build: locating the DMG"
# Guard the glob: exactly one DMG, or fail loudly (mirrors the .sig count-guard in
# scripts/latest-json.mjs). Unmatched glob stays literal and the -f check catches it.
set -- src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg
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
