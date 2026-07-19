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

# One key=value field from the owner file. Kept sed-based (not `source`) on purpose: the
# owner label is an unquoted path that may contain spaces/parens (hw-e2e.sh), which would
# break/inject under `. "$file"`.
_device_lock_field() { sed -n "s/^$1=//p" "$DEVICE_LOCK_OWNER" 2>/dev/null; }

# device_lock_acquire "<label>" — block until the lock is ours (or ~30 min elapse → 1).
device_lock_acquire() {
  local label="${1:-?}" waited=0 max=1800 opid
  while :; do
    if mkdir "$DEVICE_LOCK_DIR" 2>/dev/null; then
      printf 'pid=%s\nowner=%s\nsince=%s\n' \
        "$$" "$label" "$(date '+%Y-%m-%d %H:%M:%S')" > "$DEVICE_LOCK_OWNER"
      return 0
    fi
    # Held. Stale? (owner pid gone → reclaim and retry immediately.)
    opid="$(_device_lock_field pid)"
    if [ -n "$opid" ] && ! kill -0 "$opid" 2>/dev/null; then
      printf '\033[33mdevice lock held by dead pid %s — reclaiming\033[0m\n' "$opid" >&2
      rm -rf "$DEVICE_LOCK_DIR"
      continue
    fi
    if [ "$waited" -ge "$max" ]; then
      printf '\033[31mdevice still busy after %ss — aborting\033[0m\n' "$waited" >&2
      return 1
    fi
    printf '\033[33mdevice busy — held by %s (pid %s) since %s; waiting…\033[0m\n' \
      "$(_device_lock_field owner)" "${opid:-?}" "$(_device_lock_field since)" >&2
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
