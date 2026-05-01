#!/usr/bin/env bash
#
# arch/install-qml.sh — fast iteration: copy only the QML/UI files into
# /usr/share/jarvis-os/qml/ and restart jarvis-ui.service. Does NOT
# rebuild any Rust binary, does NOT touch systemd units, does NOT
# install dependencies.
#
# Use this when you've changed `ui/jarvis-os/**` in the repo and want
# the running UI to pick up the change without sitting through a full
# cargo build. For a from-scratch / production install use install.sh.
#
# Usage:
#   bash arch/install-qml.sh
#
# Equivalent to the QML-copy step of install.sh + the service restart.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JARVIS_OS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

log()  { printf '\033[36m[install-qml]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[install-qml] WARN:\033[0m %s\n' "$*"; }
fail() { printf '\033[31m[install-qml] FAIL:\033[0m %s\n' "$*" >&2; exit 1; }

# ─── Sanity checks ───────────────────────────────────────────────────
SRC_DIR="$JARVIS_OS_DIR/ui/jarvis-os"
DST_DIR="/usr/share/jarvis-os/qml"

[[ -d "$SRC_DIR" ]] || fail "source dir missing: $SRC_DIR"
[[ -f "$SRC_DIR/shell.qml" ]] || fail "$SRC_DIR/shell.qml not found — wrong tree?"

# Need sudo for /usr/share writes.
if ! sudo -v; then
    fail "sudo authentication failed"
fi

# ─── Copy QML files ──────────────────────────────────────────────────
log "Copying $SRC_DIR/* → $DST_DIR/"
sudo install -d "$DST_DIR"
sudo cp -r "$SRC_DIR"/* "$DST_DIR/"

# Mirror install.sh's chmod for the toggle script.
TOGGLE="$DST_DIR/scripts/jarvis-ui-toggle.sh"
if [[ -f "$TOGGLE" ]]; then
    sudo chmod +x "$TOGGLE"
fi

# ─── Restart jarvis-ui.service if it's enabled ───────────────────────
if systemctl --user is-enabled jarvis-ui.service &>/dev/null; then
    log "Restarting jarvis-ui.service…"
    systemctl --user restart jarvis-ui.service
    sleep 1
    if systemctl --user is-active jarvis-ui.service &>/dev/null; then
        log "jarvis-ui.service is active"
    else
        warn "jarvis-ui.service did not come back active — check 'journalctl --user -u jarvis-ui.service -n 30'"
    fi
else
    warn "jarvis-ui.service not enabled — skipping restart. Run quickshell directly to test:"
    warn "  quickshell -p $DST_DIR"
fi

log "Done."
