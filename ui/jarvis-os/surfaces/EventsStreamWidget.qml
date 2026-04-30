import QtQuick
import Quickshell
import Quickshell.Wayland
import "../components"
import "../core"

PanelWindow {
    id: widget

    anchors { bottom: true; left: true }
    margins { bottom: 32; left: 32 }

    width: 280
    height: 200

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // Debug-only: visible only when forceShown (Super+J override) and not force-hidden.
    visible: !Hotkeys.overrideHideAll && forceShown
    property bool forceShown: Hotkeys.overrideShowAll

    PanelFrame {
        anchors.fill: parent
        title: "events"
        opacity: widget.visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }

        ListView {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.topMargin: 32
            anchors.margins: 12
            clip: true

            model: EventBus.recentEvents.slice(-15)  // last 15 entries fit
            delegate: EventLine {
                ev: modelData
                width: parent ? parent.width : 0
            }
            verticalLayoutDirection: ListView.BottomToTop
        }
    }
}
