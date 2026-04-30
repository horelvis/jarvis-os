import QtQuick
import Quickshell
import Quickshell.Wayland
import "../core"

PanelWindow {
    id: ring

    anchors { bottom: true; right: true }
    margins { right: 32; bottom: 32 }

    width: 100
    height: 120  // extra room for status label below the orb

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    readonly property bool agentActive: Object.keys(EventBus.activeTools).length > 0
    readonly property bool offline: !EventBus.connected

    Rectangle {
        id: outerRing
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        width: 100; height: 100
        radius: width / 2
        color: "transparent"
        border.color: ring.offline ? "#f5c56a" : "#5dcbff"
        border.width: 2

        // Glow intensifies with activity.
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 30
            height: parent.height - 30
            radius: width / 2
            color: ring.agentActive
                ? Qt.rgba(0.36, 0.79, 1, 0.35)
                : Qt.rgba(0.36, 0.79, 1, 0.12)
            Behavior on color { ColorAnimation { duration: 250 } }
        }

        // Spinning dashed inner ring — speed depends on agent state.
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 16
            height: parent.height - 16
            radius: width / 2
            color: "transparent"
            border.color: "#3aa8e8"
            border.width: 1
            opacity: ring.offline ? 0.3 : 0.7

            RotationAnimation on rotation {
                from: 0
                to: 360
                duration: ring.agentActive ? 4000 : 14000
                loops: Animation.Infinite
                running: !ring.offline
            }
        }

        // Center core pulses on activity.
        Rectangle {
            anchors.centerIn: parent
            width: 16; height: 16
            radius: 8
            color: ring.offline ? "#f5c56a" : "#5dcbff"
            opacity: 1.0

            SequentialAnimation on scale {
                running: ring.agentActive
                loops: Animation.Infinite
                NumberAnimation { from: 1.0; to: 1.3; duration: 600 }
                NumberAnimation { from: 1.3; to: 1.0; duration: 600 }
            }
        }
    }

    // Status label below the orb.
    Text {
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        color: ring.offline ? "#f5c56a" : "#5dcbff"
        font.family: "monospace"
        font.pixelSize: 8
        font.letterSpacing: 1
        text: ring.offline ? "OFFLINE"
            : ring.agentActive ? "ACTIVE"
            : "IDLE"
        opacity: 0.85
    }
}
