#!/bin/sh
# Post-install hook for the .deb/.rpm bundle — reloads udev so the rule installed
# alongside it (packaging/udev/70-fender-tone-master-pro.rules) applies to a unit
# that is already plugged in, with no unplug/replug required.
#
# Non-fatal by design: a container or chroot install has no running udev, and
# that must not fail the package install. POSIX sh — deb and rpm both invoke
# this with /bin/sh, not bash.
set -e

if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
  udevadm trigger --subsystem-match=hidraw || true
fi
