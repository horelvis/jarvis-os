import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import "../core"

PanelWindow {
    id: ring

    anchors { bottom: true; right: true }
    margins { right: 32; bottom: 32 }

    implicitWidth: 140
    implicitHeight: 156  // extra room for status label below the orb

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    readonly property bool agentActive: Object.keys(EventBus.activeTools).length > 0
    readonly property bool offline: !EventBus.connected

    readonly property color colorPrimary: ring.offline ? "#f5c56a" : "#5dcbff"
    readonly property color colorDeep:    ring.offline ? "#7a5a2a" : "#3aa8e8"
    readonly property color colorSoft:    ring.offline ? "#f7d59b" : "#9fe6ff"

    Item {
        id: orb
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        width: 140
        height: 140

        // ──────────────── Outer scanline / glow ────────────────
        Rectangle {
            anchors.centerIn: parent
            width: parent.width
            height: parent.height
            radius: width / 2
            color: "transparent"
            border.color: ring.colorPrimary
            border.width: 1.5
        }

        // Tick marks every 30° on the outer ring.
        Repeater {
            model: 12
            Item {
                width: orb.width
                height: orb.height
                anchors.centerIn: parent
                rotation: index * 30
                Rectangle {
                    width: 1
                    height: index % 3 === 0 ? 8 : 5  // longer ticks at cardinals
                    color: ring.colorPrimary
                    opacity: index % 3 === 0 ? 0.95 : 0.55
                    anchors.top: parent.top
                    anchors.horizontalCenter: parent.horizontalCenter
                }
            }
        }

        // Cardinal letters N / E / S / W just inside the outer ring.
        Repeater {
            model: [
                {label: "N", angle: 0},
                {label: "E", angle: 90},
                {label: "S", angle: 180},
                {label: "W", angle: 270}
            ]
            Item {
                width: orb.width; height: orb.height
                anchors.centerIn: parent
                rotation: modelData.angle
                Text {
                    text: modelData.label
                    color: ring.colorPrimary
                    opacity: 0.6
                    font.family: "monospace"
                    font.pixelSize: 7
                    font.letterSpacing: 1
                    anchors.top: parent.top
                    anchors.topMargin: 12
                    anchors.horizontalCenter: parent.horizontalCenter
                    rotation: -modelData.angle  // counter-rotate so always upright
                }
            }
        }

        // ──────────────── Layer 1 — outer glow halo ────────────────
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 22
            height: parent.height - 22
            radius: width / 2
            color: "transparent"
            border.color: ring.colorPrimary
            border.width: 1
            opacity: ring.agentActive ? 0.85 : 0.45
            Behavior on opacity { NumberAnimation { duration: 250 } }

            // Soft gradient fill that intensifies with activity.
            Rectangle {
                anchors.centerIn: parent
                width: parent.width - 4
                height: parent.height - 4
                radius: width / 2
                color: ring.agentActive
                    ? Qt.rgba(0.36, 0.79, 1, 0.22)
                    : Qt.rgba(0.36, 0.79, 1, 0.08)
                Behavior on color { ColorAnimation { duration: 250 } }
            }
        }

        // ──────────────── Layer 2 — dashed ring rotating CW ────────────────
        Item {
            anchors.centerIn: parent
            width: parent.width - 38
            height: parent.height - 38

            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorDeep
                border.width: 1
                opacity: ring.offline ? 0.25 : 0.7
            }

            // Slash arcs as a dashed effect: 4 short arcs rotating.
            Repeater {
                model: 4
                Item {
                    width: parent.width; height: parent.height
                    anchors.centerIn: parent
                    rotation: index * 90
                    Rectangle {
                        width: 14
                        height: 2
                        color: ring.colorPrimary
                        opacity: ring.offline ? 0.3 : 0.85
                        anchors.top: parent.top
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.topMargin: -1
                    }
                }
            }

            RotationAnimation on rotation {
                from: 0; to: 360
                duration: ring.agentActive ? 4000 : 12000
                loops: Animation.Infinite
                running: !ring.offline
            }
        }

        // ──────────────── Layer 3 — inner ring rotating CCW ────────────────
        Item {
            anchors.centerIn: parent
            width: parent.width - 60
            height: parent.height - 60

            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1
                opacity: ring.offline ? 0.3 : 0.65
            }

            // Tick marks every 45° on this inner ring.
            Repeater {
                model: 8
                Item {
                    width: parent.width; height: parent.height
                    anchors.centerIn: parent
                    rotation: index * 45
                    Rectangle {
                        width: 1
                        height: 4
                        color: ring.colorSoft
                        opacity: 0.7
                        anchors.top: parent.top
                        anchors.horizontalCenter: parent.horizontalCenter
                    }
                }
            }

            RotationAnimation on rotation {
                from: 360; to: 0
                duration: ring.agentActive ? 8000 : 20000
                loops: Animation.Infinite
                running: !ring.offline
            }
        }

        // ──────────────── Core — pulsing dot with halo ────────────────
        Rectangle {
            anchors.centerIn: parent
            width: 26; height: 26
            radius: 13
            color: "transparent"
            border.color: ring.colorPrimary
            border.width: 1
            opacity: 0.7
        }

        Rectangle {
            id: core
            anchors.centerIn: parent
            width: 14; height: 14
            radius: 7
            color: ring.colorPrimary
            opacity: 1.0

            SequentialAnimation on scale {
                running: ring.agentActive
                loops: Animation.Infinite
                NumberAnimation { from: 1.0; to: 1.4; duration: 600 }
                NumberAnimation { from: 1.4; to: 1.0; duration: 600 }
            }
        }

        // Two perpendicular crosshair lines through the core (subtle).
        Rectangle {
            anchors.centerIn: parent
            width: orb.width - 70
            height: 1
            color: ring.colorDeep
            opacity: 0.25
        }
        Rectangle {
            anchors.centerIn: parent
            width: 1
            height: orb.height - 70
            color: ring.colorDeep
            opacity: 0.25
        }
    }

    // Status label below the orb.
    Text {
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 2
        anchors.horizontalCenter: parent.horizontalCenter
        color: ring.colorPrimary
        font.family: "monospace"
        font.pixelSize: 9
        font.letterSpacing: 2
        text: ring.offline ? "OFFLINE"
            : ring.agentActive ? "ACTIVE"
            : "IDLE"
        opacity: 0.95
    }
}
