#!/usr/bin/env bash
# Select optional Linux artifacts; an API failure or expired artifact is an error.
set -euo pipefail
inventory="$(mktemp "${TMPDIR:-/tmp}/tmp-companion-artifacts.XXXXXX")"
trap 'rm -f "$inventory"' EXIT
gh api "repos/${GITHUB_REPOSITORY:?}/actions/runs/${GITHUB_RUN_ID:?}/artifacts" \
  --paginate --jq '.artifacts[] | select(.name == "linux-deb" or .name == "linux-rpm") | [.name, .expired] | @tsv' > "$inventory"
deb=false rpm=false
while IFS="$(printf '\t')" read -r name expired; do
  [ -n "$name" ] || continue
  case "$expired" in
    false) ;;
    true) echo "expected artifact $name has expired" >&2; exit 1 ;;
    *) echo "invalid artifact inventory entry for $name" >&2; exit 1 ;;
  esac
  case "$name" in
    linux-deb) deb=true ;;
    linux-rpm) rpm=true ;;
    *) echo "unexpected artifact name: $name" >&2; exit 1 ;;
  esac
done < "$inventory"
printf 'deb=%s\nrpm=%s\n' "$deb" "$rpm"
