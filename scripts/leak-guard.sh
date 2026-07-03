#!/usr/bin/env bash
#
# leak-guard.sh — keep PRIVATE firmware-emulator content out of this PUBLIC repo.
#
# tmp-companion is public; a separate private project reverse-engineers and
# emulates the device firmware. Their Claude skills folder is shared locally via
# a symlink, so it is easy to accidentally stage private content. This guard
# scans for unambiguous markers of that private project — firmware-binary names,
# RE tooling, emulator infrastructure, private skill names, and local-only Claude
# paths — and refuses the commit / fails CI if any appears in tracked content.
#
#   scripts/leak-guard.sh          scan STAGED content    (git pre-commit hook)
#   scripts/leak-guard.sh --all    scan ALL tracked files (CI, belt-and-braces)
#
# Exits non-zero and prints the offending file:line on any match.
# Portable to macOS /bin/bash 3.2.57 + BSD grep: no arrays, no GNU-only flags,
# no `&& cmd` statements (set -e unsafe); here-docs (not pipes) keep `status` in
# scope. Test under /bin/bash explicitly, not a newer PATH bash.

set -euo pipefail

self="scripts/leak-guard.sh"

# Fingerprints of the PRIVATE emulator project (POSIX extended regex, '|').
# Targets that project's own infrastructure, skill names, and repo path — NOT the
# device firmware-binary names (`tm-stomp-server`, `tone-master-stomp-client`) or
# the Ghidra RE provenance, which are this repo's ESTABLISHED public data-lineage
# (see Cargo.toml, proto.rs, model-cpu.json, block-classification.md, …). Matching
# those would false-positive on ~12 legitimately-public files.
pattern='LD_PRELOAD|qt_metacall|vtable\+0x|populator_stub|populator-gap|harness/stubs|nam-patcher|unsquashfs|microVM|firmware/[0-9][A-Za-z0-9._-]*/recon|Documents/Personal/tmp|tmp-binary-re|tmp-firmware-pipeline|tmp-vm-runtime|tmp-stub-catalog|tmp-device-platform|tmp-emulator-touchscreen-driving|\.claude/plans|\.claude/projects/.+/memory'

mode="${1:-staged}"
status=0

report() { # <file-label> <grep-output>
  printf 'LEAK-GUARD: private-project marker in %s\n' "$1" >&2
  printf '%s\n' "$2" | sed 's/^/    /' >&2
  status=1
}

if [ "$mode" = "--all" ] || [ "$mode" = "all" ]; then
  # All tracked files, scanned in the working tree.
  while IFS= read -r f; do
    if [ -z "$f" ]; then continue; fi
    if [ "$f" = "$self" ]; then continue; fi
    if [ ! -f "$f" ]; then continue; fi
    out="$(grep -nIE "$pattern" -- "$f" 2>/dev/null || true)"
    if [ -n "$out" ]; then report "$f" "$out"; fi
  done <<EOF
$(git ls-files)
EOF
else
  # Staged additions/modifications, scanned from the index (the exact bytes that
  # would be committed), not the working tree.
  while IFS= read -r f; do
    if [ -z "$f" ]; then continue; fi
    if [ "$f" = "$self" ]; then continue; fi
    out="$(git show ":$f" 2>/dev/null | grep -nIE "$pattern" 2>/dev/null || true)"
    if [ -n "$out" ]; then report "$f (staged)" "$out"; fi
  done <<EOF
$(git diff --cached --name-only --diff-filter=ACM)
EOF
fi

if [ "$status" -ne 0 ]; then
  printf '\nRefusing: private firmware-emulator content must not enter the public repo.\n' >&2
  printf 'If this is a genuine false positive, tighten the pattern in %s.\n' "$self" >&2
  exit 1
fi

printf 'leak-guard: clean\n'
