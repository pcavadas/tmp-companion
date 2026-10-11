# shellcheck shell=bash
#
# scripts/lib/e2e-lane.sh — pure helpers for the ONLINE e2e lane, sourced by scripts/e2e.sh
# and scripts/pre-pr.sh, and tested offline by scripts/e2e-lane.spec.sh. Nothing here
# touches the device: each function reads text (or a listing it is pointed at) and prints a
# verdict, so every rule that decides whether a run may stamp is provable without a unit.
#
# bash 3.2-safe (macOS /bin/bash): no associative arrays, no mapfile, no ${x^^}.

# The unit's USB vendor id as sysfs spells it (ioreg spells it 7896, in the awk below).
LANE_VENDOR_HEX=1ed8

# stdin: `ioreg -r -c IOUSBHostInterface -l -w0` text. Exit 0 when a CDC interface
# (bInterfaceClass 2 or 10) sits on the unit's vendor id, 1 otherwise. The awk is the
# hardware-proven check, kept verbatim: each `+-o` line starts a new interface entry.
lane_ioreg_has_cdc() {
  awk '/\+-o /{if(v&&k)f=1; v=0; k=0} /"idVendor" = 7896/{v=1} /"bInterfaceClass" = (2|10)$/{k=1} END{if(v&&k)f=1; exit f?0:1}'
}

# $1 = a sysfs `bus/usb/devices` directory. Exit 0 when an interface of a device on the
# unit's vendor id has bInterfaceClass 02 or 0a (CDC), 1 when the unit is present without
# one, 2 when no device on the unit's vendor id is listed at all.
lane_sysfs_has_cdc() {
  local root="$1" seen=0 dev iface vid cls
  for dev in "$root"/*; do
    [ -f "$dev/idVendor" ] || continue
    vid="$(cat "$dev/idVendor" 2>/dev/null || true)"
    [ "$vid" = "$LANE_VENDOR_HEX" ] || continue
    seen=1
    for iface in "$root/$(basename "$dev")":*; do
      [ -f "$iface/bInterfaceClass" ] || continue
      cls="$(cat "$iface/bInterfaceClass" 2>/dev/null || true)"
      case "$cls" in 02|0a) return 0 ;; esac
    done
  done
  [ "$seen" -eq 1 ] && return 1
  return 2
}

# Print the unit's boot state: `stock` (it presents only its stock USB interfaces),
# `non-stock` (it presents extra interfaces), or `unknown` (this host cannot tell).
# Overrides, for the offline spec only: TMP_E2E_UNIT_OS (Darwin|Linux|other),
# TMP_E2E_IOREG_FILE (a saved ioreg listing), TMP_E2E_SYSFS_ROOT (a fake devices dir).
lane_unit_state() {
  local os="${TMP_E2E_UNIT_OS:-$(uname -s 2>/dev/null || echo unknown)}" rc
  case "$os" in
    Darwin)
      if [ -n "${TMP_E2E_IOREG_FILE:-}" ]; then
        lane_ioreg_has_cdc < "$TMP_E2E_IOREG_FILE" && rc=0 || rc=$?
      elif command -v ioreg >/dev/null 2>&1; then
        ioreg -r -c IOUSBHostInterface -l -w0 2>/dev/null | lane_ioreg_has_cdc && rc=0 || rc=$?
      else
        rc=2
      fi
      ;;
    Linux)
      lane_sysfs_has_cdc "${TMP_E2E_SYSFS_ROOT:-/sys/bus/usb/devices}" && rc=0 || rc=$?
      ;;
    *) rc=2 ;;
  esac
  case "$rc" in
    0) printf 'non-stock\n' ;;
    1) printf 'stock\n' ;;
    *) printf 'unknown\n' ;;
  esac
}

# The online stamp decision. Prints `stamp`, or `no-stamp: <reason>` for the FIRST clause
# that fails. Args (all strings):
#   $1 fail        0 when every spec (and the external judge, if it gated) passed
#   $2 key_start   the tree key taken before the specs ran
#   $3 key_end     the tree key taken after
#   $4 mode        online | soak
#   $5 ran_level   1 when level.online was in the spec set
#   $6 validated   1 when the ffmpeg judge PASSED (exit 0)
#   $7 unit        stock | non-stock | unknown
lane_stamp_verdict() {
  if [ "$1" != 0 ]; then
    printf 'no-stamp: a spec or the external validation failed\n'
  elif [ "$4" != online ]; then
    printf 'no-stamp: %s runs never stamp\n' "$4"
  elif [ "$7" != stock ]; then
    printf 'no-stamp: unit=%s — the unit must present its stock USB interfaces\n' "$7"
  elif [ "$5" = 1 ] && [ "$6" != 1 ]; then
    printf 'no-stamp: level.online ran but external validation did not confirm anything\n'
  elif [ -z "$2" ] || [ "$2" != "$3" ]; then
    printf 'no-stamp: the tree changed during the run (key %s -> %s); re-run the lane on the final tree\n' "$2" "$3"
  else
    printf 'stamp\n'
  fi
}

# stdin: an e2e_server log. Print the firmware version its ready line names, or `unknown`.
lane_fw_from_log() {
  sed -n 's/.*seeded snapshot from the real device fw=\([^ ]*\).*/\1/p' | tail -1 | grep . || printf 'unknown\n'
}
