#!/usr/bin/env bash
# jarvis-ui-toggle: send a TOGGLE command to the running jarvis_ui
# via UNIX socket. Invoked from Hyprland keybind config.
#
# Usage:
#   jarvis-ui-toggle.sh show    # toggle Super+J override
#   jarvis-ui-toggle.sh hide    # toggle Super+Shift+J force-hide

set -euo pipefail
CMD="${1:-show}"
SOCKET="/tmp/jarvis-ui.sock"

case "$CMD" in
    show) MSG="TOGGLE_SHOW" ;;
    hide) MSG="TOGGLE_HIDE" ;;
    *) echo "usage: $0 {show|hide}" >&2; exit 2 ;;
esac

if [ ! -S "$SOCKET" ]; then
    echo "jarvis-ui not running (no socket at $SOCKET)" >&2
    exit 1
fi

echo "$MSG" | socat - UNIX-CONNECT:"$SOCKET"
