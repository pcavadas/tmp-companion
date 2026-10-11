#!/usr/bin/env bash
# Offline spec for scripts/lib/e2e-lane.sh and the merging `gates.sh --record-online`.
#
# Proves, with no unit attached, every rule that decides whether an online run may stamp:
# the stock-interface check against synthetic USB listings (macOS ioreg text and a fake
# Linux sysfs tree; never a capture from a real unit), the stamp verdict's clauses, the
# firmware parse, and that stamps of one tree merge across runs.
#
# Bash 3.2-compatible (.claude/rules/shell-scripts.md); gates.sh runs it under /bin/bash.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/lib/e2e-lane.sh disable=SC1091
. "$script_dir/lib/e2e-lane.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail=0
check() { # <label> <expected> <actual>
  if [ "$2" = "$3" ]; then
    printf 'ok   %s\n' "$1"
  else
    printf 'FAIL %s: expected [%s], got [%s]\n' "$1" "$2" "$3"
    fail=1
  fi
}

# ── macOS: synthetic ioreg listings ──────────────────────────────────────────────
# One interface entry per `+-o` line. Stock: HID, audio control, MIDI on the unit's
# vendor, plus a CDC interface on ANOTHER vendor (which must not count).
cat > "$work/ioreg-stock.txt" <<'IOREG'
+-o IOUSBHostInterface@0  <class IOUSBHostInterface, id 0x1, registered, matched, active>
    {
      "idVendor" = 7896
      "bInterfaceClass" = 3
    }
+-o IOUSBHostInterface@1  <class IOUSBHostInterface, id 0x2, registered, matched, active>
    {
      "idVendor" = 7896
      "bInterfaceClass" = 1
    }
+-o IOUSBHostInterface@0  <class IOUSBHostInterface, id 0x3, registered, matched, active>
    {
      "idVendor" = 1234
      "bInterfaceClass" = 2
    }
IOREG
# Non-stock: the same, plus CDC control (2) and CDC data (10) on the unit's vendor.
cp "$work/ioreg-stock.txt" "$work/ioreg-cdc.txt"
cat >> "$work/ioreg-cdc.txt" <<'IOREG'
+-o IOUSBHostInterface@3  <class IOUSBHostInterface, id 0x4, registered, matched, active>
    {
      "idVendor" = 7896
      "bInterfaceClass" = 2
    }
+-o IOUSBHostInterface@4  <class IOUSBHostInterface, id 0x5, registered, matched, active>
    {
      "idVendor" = 7896
      "bInterfaceClass" = 10
    }
IOREG
# Only the CDC data interface (class 10), and as the LAST entry (the END clause).
cp "$work/ioreg-stock.txt" "$work/ioreg-data-last.txt"
cat >> "$work/ioreg-data-last.txt" <<'IOREG'
+-o IOUSBHostInterface@4  <class IOUSBHostInterface, id 0x6, registered, matched, active>
    {
      "bInterfaceClass" = 10
      "idVendor" = 7896
    }
IOREG
# Class 12 must not read as class 2 (the pattern is anchored at end of line).
cat > "$work/ioreg-class12.txt" <<'IOREG'
+-o IOUSBHostInterface@5  <class IOUSBHostInterface, id 0x7, registered, matched, active>
    {
      "idVendor" = 7896
      "bInterfaceClass" = 12
    }
IOREG

unit_on_mac() { TMP_E2E_UNIT_OS=Darwin TMP_E2E_IOREG_FILE="$1" lane_unit_state; }
check "macOS stock listing → stock" stock "$(unit_on_mac "$work/ioreg-stock.txt")"
check "macOS CDC on the unit's vendor → non-stock" non-stock "$(unit_on_mac "$work/ioreg-cdc.txt")"
check "macOS CDC data as the last entry → non-stock" non-stock "$(unit_on_mac "$work/ioreg-data-last.txt")"
check "macOS class 12 is not CDC → stock" stock "$(unit_on_mac "$work/ioreg-class12.txt")"

# ── Linux: a fake sysfs devices tree ─────────────────────────────────────────────
mkiface() { # <root> <device> <iface> <class-hex>
  mkdir -p "$1/$2:$3"
  printf '%s\n' "$4" > "$1/$2:$3/bInterfaceClass"
}
mkdev() { # <root> <device> <vendor-hex>
  mkdir -p "$1/$2"
  printf '%s\n' "$3" > "$1/$2/idVendor"
}
s="$work/sysfs-stock"
mkdev "$s" 1-1 1ed8; mkiface "$s" 1-1 1.0 03; mkiface "$s" 1-1 1.1 01; mkiface "$s" 1-1 1.2 01
mkdev "$s" 1-2 abcd; mkiface "$s" 1-2 1.0 02
n="$work/sysfs-cdc"
mkdev "$n" 1-1 1ed8; mkiface "$n" 1-1 1.0 03; mkiface "$n" 1-1 1.3 02; mkiface "$n" 1-1 1.4 0a
a="$work/sysfs-absent"
mkdev "$a" 1-2 abcd; mkiface "$a" 1-2 1.0 02

unit_on_linux() { TMP_E2E_UNIT_OS=Linux TMP_E2E_SYSFS_ROOT="$1" lane_unit_state; }
check "Linux stock tree → stock" stock "$(unit_on_linux "$s")"
check "Linux CDC on the unit's vendor → non-stock" non-stock "$(unit_on_linux "$n")"
check "Linux unit absent → unknown" unknown "$(unit_on_linux "$a")"
check "other OS → unknown" unknown "$(TMP_E2E_UNIT_OS=Windows_NT lane_unit_state)"

# ── the stamp verdict ───────────────────────────────────────────────────────────
# args: fail key_start key_end mode ran_level validated unit
v() { lane_stamp_verdict "$@" | cut -d: -f1; }
check "copy, no ffmpeg, stock → stamp" stamp "$(v 0 k k online 0 0 stock)"
check "level.online validated → stamp" stamp "$(v 0 k k online 1 1 stock)"
check "level.online without ffmpeg → no stamp" no-stamp "$(v 0 k k online 1 0 stock)"
check "soak never stamps" no-stamp "$(v 0 k k soak 1 1 stock)"
check "a failed spec → no stamp" no-stamp "$(v 1 k k online 0 0 stock)"
check "the key moved → no stamp" no-stamp "$(v 0 k1 k2 online 0 0 stock)"
check "an empty key → no stamp" no-stamp "$(v 0 '' '' online 0 0 stock)"
check "non-stock unit → no stamp" no-stamp "$(v 0 k k online 0 0 non-stock)"
check "unknown unit → no stamp" no-stamp "$(v 0 k k online 0 0 unknown)"
check "non-stock reason names the stock interfaces" \
  "no-stamp: unit=non-stock — the unit must present its stock USB interfaces" \
  "$(lane_stamp_verdict 0 k k online 0 0 non-stock)"

# ── the firmware parse ──────────────────────────────────────────────────────────
check "fw from the ready line" 1.8.58 \
  "$(printf 'x\ne2e_server: ONLINE — seeded snapshot from the real device fw=1.8.58\n' | lane_fw_from_log)"
check "fw unknown without it" unknown \
  "$(printf 'e2e_server: ONLINE — seeded snapshot from the real device\n' | lane_fw_from_log)"

# ── merging stamps: a scratch repo with this checkout's gates.sh ─────────────────
r="$work/repo"
mkdir -p "$r/scripts"
cp "$script_dir/gates.sh" "$script_dir/gate-map.tsv" "$script_dir/gate-policy.tsv" "$r/scripts/"
(
  cd "$r"
  git init -q
  git config user.email spec@example.invalid
  git config user.name spec
  git add -A
  git commit -qm base
) >/dev/null
gates() { (cd "$r" && /bin/bash scripts/gates.sh "$@"); }
stamp_file() { printf '%s/.git/tmp-gates/online-%s' "$r" "$(gates --key)"; }

gates --record-online --run "os=darwin fw=1.8.58 unit=stock validated=na set=partial" copy >/dev/null
gates --record-online --run "os=darwin fw=1.8.58 unit=stock validated=na set=partial" songs >/dev/null
check "merged stamp keeps both specs, canonical order" "songs copy" \
  "$(grep -v '^#' "$(stamp_file)" | tr '\n' ' ' | sed 's/ $//')"
check "merged stamp keeps both #run lines" 2 "$(grep -c '^#run ' "$(stamp_file)")"
check "#run line carries specs= and unit=" 1 \
  "$(grep -c '^#run .* specs=copy os=darwin fw=1.8.58 unit=stock validated=na set=partial$' "$(stamp_file)")"
check "--check-online copy songs passes" 0 "$(gates --check-online copy songs >/dev/null 2>&1; echo $?)"
check "--check-online (full set) fails" 1 "$(gates --check-online >/dev/null 2>&1; echo $?)"
gates --record-online songs doctor.online >/dev/null
check "a repeated spec is not duplicated" "doctor.online songs copy" \
  "$(grep -v '^#' "$(stamp_file)" | tr '\n' ' ' | sed 's/ $//')"

# A non-prose edit re-keys the tree: the old stamp no longer counts.
printf 'x\n' > "$r/scripts/new.sh"
check "a non-prose edit orphans the stamp" 1 "$(gates --check-online copy >/dev/null 2>&1; echo $?)"

# ── `e2e.sh online auto` on a docs-only diff: exit 0 before any lock, build or seed ──
a="$work/auto"
mkdir -p "$a/scripts/lib" "$a/e2e/specs"
cp "$script_dir/e2e.sh" "$script_dir/gates.sh" "$script_dir/gate-map.tsv" \
  "$script_dir/gate-policy.tsv" "$script_dir/device-lock.sh" "$a/scripts/"
cp "$script_dir/lib/e2e-lane.sh" "$a/scripts/lib/"
cp "$script_dir/../e2e/specs/level.online.spec.ts" "$a/e2e/specs/"
(
  cd "$a"
  git init -q
  git config user.email spec@example.invalid
  git config user.name spec
  git checkout -q -b main
  git add -A
  git commit -qm base
  git checkout -q -b docs
  mkdir -p notes
  printf 'prose\n' > notes/x.md
) >/dev/null
mkdir -p "$work/tmpdir"
auto_rc=0
auto_out="$(cd "$a" && TMPDIR="$work/tmpdir" TMP_DEVICE_LOCK_DIR="$work/lock" \
  /bin/bash scripts/e2e.sh online auto 2>&1)" || auto_rc=$?
check "online auto, docs-only diff → exit 0" 0 "$auto_rc"
check "online auto says nothing is required" 1 "$(printf '%s\n' "$auto_out" | grep -c 'requires no online spec')"
check "online auto never seeded" no "$([ -e "$work/tmpdir/tmp-companion-e2e/seed.log" ] && echo yes || echo no)"
check "online auto never took the device lock" no "$([ -e "$work/lock" ] && echo yes || echo no)"

if [ "$fail" -ne 0 ]; then
  printf 'e2e-lane spec: FAILED\n' >&2
  exit 1
fi
printf 'e2e-lane spec: all checks passed\n'
