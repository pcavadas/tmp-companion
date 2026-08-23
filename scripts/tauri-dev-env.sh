#!/usr/bin/env bash
set -euo pipefail

# On KDE/GNOME Wayland, WebKitGTK's native-Wayland window sometimes maps but
# never paints (taskbar entry, no visible content) with some GPU drivers.
# Forcing XWayland works around it. See notes/gotchas.md.
if [[ "${1:-}" == "dev" && "$(uname)" == "Linux" && "${XDG_SESSION_TYPE:-}" == "wayland" && -z "${GDK_BACKEND:-}" ]]; then
  export GDK_BACKEND=x11
fi

exec tauri "$@"
