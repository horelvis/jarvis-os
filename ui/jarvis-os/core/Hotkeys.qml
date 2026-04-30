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

    // IPC entry point for hotkey scripts. Invoked from Hyprland keybinds via:
    //   qs ipc call jarvis-ui toggleShow
    //   qs ipc call jarvis-ui toggleHide
    property var ipc: IpcHandler {
        target: "jarvis-ui"

        function toggleShow(): void {
            hotkeys.overrideShowAll = !hotkeys.overrideShowAll;
            if (hotkeys.overrideShowAll) hotkeys.autoHideTimer.restart();
            else hotkeys.autoHideTimer.stop();
        }

        function toggleHide(): void {
            hotkeys.overrideHideAll = !hotkeys.overrideHideAll;
        }
    }
}
