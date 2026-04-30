pragma Singleton
import QtQuick
import Quickshell.Io

QtObject {
    id: hotkeys

    property bool overrideShowAll: false
    property bool overrideHideAll: false

    property var autoHideTimer: Timer {
        interval: 10000  // 10s auto-hide for Super+J override
        onTriggered: hotkeys.overrideShowAll = false
    }

    // Listen on UNIX socket for TOGGLE messages from jarvis-ui-toggle.sh.
    // Quickshell.Io.Socket is the standard way to consume per-line text.
    property var socket: Socket {
        path: "/tmp/jarvis-ui.sock"
        listening: true
        onLineReceived: function(line) {
            switch (line.trim()) {
            case "TOGGLE_SHOW":
                hotkeys.overrideShowAll = !hotkeys.overrideShowAll;
                if (hotkeys.overrideShowAll) hotkeys.autoHideTimer.restart();
                else hotkeys.autoHideTimer.stop();
                break;
            case "TOGGLE_HIDE":
                hotkeys.overrideHideAll = !hotkeys.overrideHideAll;
                break;
            }
        }
    }
}
