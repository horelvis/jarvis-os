import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: ring

    // Layer-shell anchor: bottom-right with 32px gap from screen edge.
    anchors {
        bottom: true
        right: true
    }
    margins {
        right: 32
        bottom: 32
    }

    width: 100
    height: 100

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    Rectangle {
        id: outerRing
        anchors.fill: parent
        radius: width / 2
        color: "transparent"
        border.color: "#5dcbff"
        border.width: 2

        // Inner gradient glow.
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 30
            height: parent.height - 30
            radius: width / 2
            color: Qt.rgba(0.36, 0.79, 1, 0.18)
        }

        // Spinning dashed inner ring.
        Rectangle {
            id: innerRing
            anchors.centerIn: parent
            width: parent.width - 16
            height: parent.height - 16
            radius: width / 2
            color: "transparent"
            border.color: "#3aa8e8"
            border.width: 1
            opacity: 0.7

            RotationAnimation on rotation {
                from: 0
                to: 360
                duration: 14000
                loops: Animation.Infinite
                running: true
            }
        }

        // Center core.
        Rectangle {
            anchors.centerIn: parent
            width: 16
            height: 16
            radius: 8
            color: "#5dcbff"
        }
    }
}
