#!/usr/bin/env bash
# jarvis-ui-toggle: invoke a TOGGLE function on the running jarvis_ui
# Quickshell instance via the qs IPC bridge. Called from Hyprland keybinds.
#
# Usage:
#   jarvis-ui-toggle.sh show    # toggle Super+J override (auto-hide 10s)
#   jarvis-ui-toggle.sh hide    # toggle Super+Shift+J force-hide

set -euo pipefail
CMD="${1:-show}"

case "$CMD" in
    show) FN="toggleShow" ;;
    hide) FN="toggleHide" ;;
    *) echo "usage: $0 {show|hide}" >&2; exit 2 ;;
esac

if ! command -v qs >/dev/null 2>&1; then
    echo "qs CLI not found in PATH (Quickshell not installed?)" >&2
    exit 1
fi

qs ipc call jarvis-ui "$FN"
