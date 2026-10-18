#!/bin/bash
#
# check-docs.sh — the docs gate: markdown links, anchors and `.md` path mentions.
#
#   scripts/check-docs.sh            fail on a broken link, anchor or .md mention
#   scripts/check-docs.sh --strict   also report stale wording (report-only)
#
# The checks live in scripts/check-docs.mjs (see its header). This wrapper picks a
# JavaScript runtime: node first, then bun. No network; a few seconds on the whole tree.
# Runs from the pre-commit hook, scripts/gates.sh (every mode, docs-only diffs too) and CI.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if command -v node >/dev/null 2>&1; then
  exec node "$here/check-docs.mjs" "$@"
elif command -v bun >/dev/null 2>&1; then
  exec bun "$here/check-docs.mjs" "$@"
fi
printf 'check-docs: needs node or bun on PATH\n' >&2
exit 2
