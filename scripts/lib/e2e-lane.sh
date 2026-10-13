# shellcheck shell=bash
#
# scripts/lib/e2e-lane.sh — pure helpers for the ONLINE e2e lane, sourced by scripts/e2e.sh
# and scripts/pre-release.sh, and tested offline by scripts/e2e-lane.spec.sh. Nothing here
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
#   $8 on_device   1 when the e2e server's own trace holds a successful open on the real
#                  device (`evidence_server_touched_device`): an online run answered by a
#                  stale offline server never touched the unit and must not stamp
lane_stamp_verdict() {
  if [ "$1" != 0 ]; then
    printf 'no-stamp: a spec or the external validation failed\n'
  elif [ "$4" != online ]; then
    printf 'no-stamp: %s runs never stamp\n' "$4"
  elif [ "$7" != stock ]; then
    printf 'no-stamp: unit=%s — the unit must present its stock USB interfaces\n' "$7"
  elif [ "${8:-}" != 1 ]; then
    printf "no-stamp: the e2e server's trace holds no session on the real device (a stale or offline server answered this run)\n"
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

# The ONLINE lane's external-validation verdict: judges the expectation log the e2e server
# wrote with scripts/level-validate.sh and decides whether this run counts as VALIDATED.
# The judge's own report goes to stderr; stdout is ONE verdict line:
#   none                nothing was emitted and nothing was required
#   validated           every row was judged (none dropped by the cap), the judge passed,
#                       and every required sound was MEASURED and PASSed, by identity
#   unverified: <why>   the judge failed nothing, but the run cannot certify: the cap
#                       dropped rows, every row was skipped, or a required sound was
#                       skipped (clamped) rather than measured
#   skipped: <why>      ffmpeg vanished mid-run (judge exit 3); nothing was checked
#   fail: <why>         a required sound was never emitted, the row count is wrong, or the
#                       judge failed a row (a target miss, a failed inject, a failed save)
# A passing PREFIX never certifies: the identities are checked against what the judge
# actually MEASURED (its --summary), after the cap, not against the emitted line count.
# Args:
#   $1 judge       path to level-validate.sh
#   $2 log         the expectation jsonl (may be absent or empty)
#   $3 work        a scratch dir for the capped feed and the judge's summary
#   $4 max_rows    the row cap (TMP_E2E_VALIDATE_MAX_ROWS)
#   $5 want_rows   the exact row count required, or '' when no strict spec ran
#   $6 labels      the space-separated identities that must be measured, or ''
lane_external_validation() {
  local judge="$1" log="$2" work="$3" max="$4" want_rows="$5" labels="$6"
  local rows=0 feed="$2" summary="$3/level-validate-summary.tsv" vrc=0 tab
  local fail="" unverified="" l
  tab="$(printf '\t')"
  if [ -s "$log" ]; then
    rows="$(grep -c . "$log" 2>/dev/null || true)"
  fi
  rows="${rows:-0}"
  if [ -n "$want_rows" ] && [ "$rows" -ne "$want_rows" ]; then
    fail="$rows expectation row(s) were emitted, expected exactly $want_rows"
  fi
  for l in $labels; do
    if ! grep -qF "\"label\":\"$l\"" "$log" 2>/dev/null; then
      fail="${fail:+$fail; }no expectation row for $l (its re-measure emitted nothing)"
    fi
  done
  if [ "$rows" -eq 0 ]; then
    if [ -n "$fail" ]; then printf 'fail: %s\n' "$fail"; else printf 'none\n'; fi
    return 0
  fi
  mkdir -p "$work"
  : > "$summary"
  if [ "$rows" -gt "$max" ]; then
    feed="$work/level-validate-expectations.capped.jsonl"
    head -n "$max" "$log" > "$feed"
    unverified="the row cap ($max) dropped $((rows - max)) of $rows row(s), never judged"
  fi
  bash "$judge" --expectations "$feed" --summary "$summary" >&2 || vrc=$?
  case "$vrc" in
    0) ;;
    3)
      if [ -z "$fail" ]; then
        printf 'skipped: ffmpeg vanished mid-run (judge exit 3); nothing was checked\n'
        return 0
      fi ;;
    4) unverified="${unverified:+$unverified; }every judged row was skipped (0 measured)" ;;
    *) fail="${fail:+$fail; }the judge failed a row (exit $vrc)" ;;
  esac
  if [ -n "$fail" ]; then
    printf 'fail: %s\n' "$fail"
    return 0
  fi
  for l in $labels; do
    if ! grep -q "^PASS$tab$l\$" "$summary"; then
      unverified="${unverified:+$unverified; }$l was not measured and passed"
    fi
  done
  if [ -n "$unverified" ]; then
    printf 'unverified: %s\n' "$unverified"
  else
    printf 'validated\n'
  fi
}
