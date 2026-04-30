import QtQuick
import Quickshell
import Quickshell.Wayland
import "../components"
import "../core"

PanelWindow {
    id: widget

    anchors {
        top: true
        right: true
    }
    margins {
        top: 32
        right: 32
    }

    width: 280
    height: contentColumn.implicitHeight + 44

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // Smart visibility: appear while any tool is running, unless force-hidden.
    visible: !Hotkeys.overrideHideAll && (visibleByActivity || forceShown)
    property bool visibleByActivity: Object.keys(EventBus.activeTools).length > 0
    property bool forceShown: Hotkeys.overrideShowAll

    PanelFrame {
        id: frame
        anchors.fill: parent
        title: "tools active"
        opacity: widget.visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }

        Column {
            id: contentColumn
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.topMargin: 32
            anchors.margins: 12
            spacing: 6

            Repeater {
                model: Object.keys(EventBus.activeTools)
                Item {
                    width: parent.width
                    height: 22
                    property var tool: EventBus.activeTools[modelData]
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        text: tool ? tool.name : "?"
                        color: "#9fe6ff"
                        font.family: "monospace"
                        font.pixelSize: 10
                    }
                    ProgressBar {
                        anchors.bottom: parent.bottom
                        anchors.left: parent.left
                        anchors.right: parent.right
                        progress: 0.6  // running indicator (no progress signal in v1)
                        fillColor: "#f5c56a"
                    }
                }
            }
        }
    }
}
