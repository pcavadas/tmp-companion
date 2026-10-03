#!/usr/bin/env bash
# Build signed repositories without Git/network publication. Bash 3.2 compatible.
set -euo pipefail
[ "$#" -eq 4 ] || { echo "usage: build-linux-repos.sh <deb-dir> <rpm-dir> <output-dir> <fingerprint>" >&2; exit 2; }
DEB_DIR=$1 RPM_DIR=$2 OUTPUT=$3 FPR=$4
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
have_deb=0 have_rpm=0
for package in "$DEB_DIR"/*.deb; do [ ! -f "$package" ] || have_deb=1; done
for package in "$RPM_DIR"/*.rpm; do [ ! -f "$package" ] || have_rpm=1; done
if [ "$have_deb$have_rpm" = 00 ]; then
  echo "build-linux-repos: no artifacts; leaving repositories unchanged"
  exit 0
fi
case "$FPR" in ''|*[!0-9A-Fa-f]*) echo "invalid signing fingerprint" >&2; exit 2 ;; esac
[ "${#FPR}" -eq 40 ] || [ "${#FPR}" -eq 64 ] || { echo "invalid signing fingerprint length" >&2; exit 2; }
case "$OUTPUT" in ''|/|.) echo "refusing an unsafe output directory" >&2; exit 2 ;; esac
work="$(mktemp -d "${TMPDIR:-/tmp}/tmp-companion-linux-repos.XXXXXX")"
trap 'rm -rf "$work"' EXIT
if [ "$have_deb" -eq 1 ]; then
  # Both the rendered configuration and Berkeley DB stay outside tracked output.
  # A completely new pool/dists tree cannot retain blobs from a missing old DB.
  mkdir -p "$work/apt-conf" "$work/apt"
  sed "s/PLACEHOLDER_FPR/$FPR/" "$REPO/packaging/apt/conf/distributions" > "$work/apt-conf/distributions"
  for package in "$DEB_DIR"/*.deb; do
    [ -f "$package" ] || continue
    # Tauri bundles omit Section; supply it and preserve their optional priority.
    reprepro --section sound --priority optional --confdir "$work/apt-conf" --dbdir "$work/apt-db" --outdir "$work/apt" includedeb stable "$package"
  done
  gpg --batch --verify "$work/apt/dists/stable/Release.gpg" "$work/apt/dists/stable/Release"
  gpg --batch --verify "$work/apt/dists/stable/InRelease"
  gpg --batch --export "$FPR" > "$work/apt/pubkey.gpg"
fi
if [ "$have_rpm" -eq 1 ]; then
  mkdir -p "$work/rpm"
  for package in "$RPM_DIR"/*.rpm; do
    [ -f "$package" ] || continue
    rpm --checksig --nosignature "$package"
    cp "$package" "$work/rpm/"
  done
  createrepo_c --checksum sha256 "$work/rpm"
  gpg --batch --yes --local-user "$FPR" --detach-sign --armor \
    -o "$work/rpm/repodata/repomd.xml.asc" "$work/rpm/repodata/repomd.xml"
  gpg --batch --verify "$work/rpm/repodata/repomd.xml.asc" "$work/rpm/repodata/repomd.xml"
  gpg --batch --export --armor "$FPR" > "$work/rpm/RPM-GPG-KEY-tmp-companion"
  printf '%s\n' '[tmp-companion]' 'name=TMP Companion' \
    'baseurl=https://pcavadas.github.io/tmp-companion/rpm' 'enabled=1' \
    'gpgcheck=0' 'repo_gpgcheck=1' \
    'gpgkey=https://pcavadas.github.io/tmp-companion/rpm/RPM-GPG-KEY-tmp-companion' \
    > "$work/rpm/tmp-companion.repo"
fi
# Verify both replacement builds before changing either format. Missing formats
# are untouched. Publication happens later, as one Git commit and Pages artifact.
mkdir -p "$OUTPUT"
for format in apt rpm; do
  [ -d "$work/$format" ] || continue
  rm -rf "${OUTPUT:?}/$format"
  mv "$work/$format" "$OUTPUT/$format"
done
