#!/usr/bin/env bash
#
# Machine-global device lock — serializes access to the ONE Tone Master Pro (exclusive
# HID seize) across parallel Claude sessions / worktrees. Only device-touching runs need
# it (online e2e, hw-e2e); offline (SimDevice) never sources it.
#
# macOS ships no flock(1), so use the portable ATOMIC `mkdir` lock: mkdir succeeds for
# exactly one racer. The winner drops an owner file (pid + label + timestamp); waiters
# poll, honour a STALE lock (owner pid dead → reclaim), and give up after ~30 min.
#
# Usage (source it, don't exec):
#   . scripts/device-lock.sh
#   device_lock_acquire "<label>" || exit 1
#   trap device_lock_release EXIT INT TERM   # or fold into an existing cleanup handler
#
# bash-3.2-safe: no arrays, no bashisms beyond arithmetic + [ ].

DEVICE_LOCK_DIR="${TMP_DEVICE_LOCK_DIR:-${TMPDIR:-/tmp}/tmp-companion-device.lock}"
DEVICE_LOCK_OWNER="$DEVICE_LOCK_DIR/owner"

# One key=value field ($1) from an owner file ($2, default the live lock's). Kept sed-based
# (not `source`) on purpose: the owner label is an unquoted path that may contain
# spaces/parens (hw-e2e.sh), which would break/inject under `. "$file"`. NULL-SAFE
# (|| true): the sourcing script runs under `set -e`, and a read in the window between a
# rival's mkdir and its owner-file write hits a missing/partial file — that must yield an
# empty field, not abort the whole script.
_device_lock_field() { sed -n "s/^$1=//p" "${2:-$DEVICE_LOCK_OWNER}" 2>/dev/null || true; }

# device_lock_acquire "<label>" — block until the lock is ours (or ~30 min elapse → 1).
device_lock_acquire() {
  local label="${1:-?}" waited=0 max=1800 opid howner hsince stale
  while :; do
    if mkdir "$DEVICE_LOCK_DIR" 2>/dev/null; then
      printf 'pid=%s\nowner=%s\nsince=%s\n' \
        "$$" "$label" "$(date '+%Y-%m-%d %H:%M:%S')" > "$DEVICE_LOCK_OWNER"
      return 0
    fi
    # Held. Stale? An EMPTY pid = owner file not written yet (rival mid-acquire) →
    # held-by-unknown, so fall through to waiting, never reclaim. A dead pid → try reclaim.
    opid="$(_device_lock_field pid)"
    if [ -n "$opid" ] && ! kill -0 "$opid" 2>/dev/null; then
      # Single-winner reclaim via atomic rename: exactly one racer's mv wins and then
      # EXCLUSIVELY owns the moved-aside dir. The dead-check above is only a cheap filter —
      # the AUTHORITY is re-reading the pid FROM the moved copy: if a live holder acquired
      # between our dead-check and the mv, the pid no longer matches the dead one we saw, so
      # we restore it and keep waiting instead of stealing a live lock (the TOCTOU CodeRabbit
      # flagged). A failed mv (another waiter already won) just loops back to waiting.
      # ponytail: a 3-way reclaim/acquire interleave can still momentarily dual-own; the
      # device's own exclusive-HID open (0xe00002c5) is the loud backstop, and contention is
      # a handful of sessions, not a hot path — a full CAS lock isn't worth it here.
      stale="$DEVICE_LOCK_DIR.stale.$$"
      if mv "$DEVICE_LOCK_DIR" "$stale" 2>/dev/null; then
        if [ "$(_device_lock_field pid "$stale/owner")" = "$opid" ]; then
          printf '\033[33mdevice lock held by dead pid %s — reclaiming\033[0m\n' "$opid" >&2
          rm -rf "$stale" 2>/dev/null || true
        else
          # Grabbed a still-live lock (someone acquired in the gap) — put it back and wait.
          mv "$stale" "$DEVICE_LOCK_DIR" 2>/dev/null || rm -rf "$stale" 2>/dev/null || true
        fi
      fi
      continue
    fi
    if [ "$waited" -ge "$max" ]; then
      printf '\033[31mdevice still busy after %ss — aborting\033[0m\n' "$waited" >&2
      return 1
    fi
    howner="$(_device_lock_field owner)"
    hsince="$(_device_lock_field since)"
    printf '\033[33mdevice busy — held by %s (pid %s) since %s; waiting…\033[0m\n' \
      "${howner:-?}" "${opid:-?}" "${hsince:-?}" >&2
    sleep 10
    waited=$((waited + 10))
  done
}

# device_lock_release — drop the lock, but ONLY if we still own it (never yank a lock a
# reclaim handed to someone else).
device_lock_release() {
  [ -d "$DEVICE_LOCK_DIR" ] || return 0
  if [ "$(_device_lock_field pid)" = "$$" ]; then rm -rf "$DEVICE_LOCK_DIR"; fi
  return 0
}
