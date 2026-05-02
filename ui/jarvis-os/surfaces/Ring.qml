import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import "../core"
import "../theme"

PanelWindow {
    id: ring

    // Span the full screen so the orb can sit at exact screen center.
    // The panel surface is transparent and pointer events pass through
    // (`mask: Region {}`), so the rest of the desktop is unaffected.
    anchors { top: true; bottom: true; left: true; right: true }

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // Empty mask region → all pointer events fall through. Without this,
    // the fullscreen layer would swallow every click.
    mask: Region {}

    readonly property bool agentActive: Object.keys(EventBus.activeTools).length > 0
    readonly property bool offline: !EventBus.connected

    readonly property color colorPrimary: ring.offline ? Palette.amber : Palette.cyanPrimary
    readonly property color colorDeep:    ring.offline ? "#7a5a2a" : Palette.cyanDeep
    readonly property color colorSoft:    ring.offline ? "#f7d59b" : Palette.cyanSoft
    readonly property color colorAccent:  Palette.amber

    // ──────────────────────────────────────────────────────────────────
    // Composition root: a wide horizontal stage centered on the screen.
    // The mecha core sits at the middle; sonar half-rings emanate to its
    // left and right (radial waves spreading outward along the horizontal
    // axis); a synthetic waveform crosses the entire stage. F3a uses
    // synthetic data; F3b will swap the source for the voice daemon.
    // ──────────────────────────────────────────────────────────────────
    Item {
        id: stage
        anchors.centerIn: parent
        width: 760
        height: 280

        // ─── Sonar half-rings on the left side of the core ────────────
        // Each ring is a half-ellipse (right side, opening toward the
        // core). Stroke width and color cycle to give the field its
        // varying "depth"; every 4th ring is amber as accent.
        Repeater {
            model: 7
            delegate: Shape {
                id: leftRing
                required property int index
                width: 70 + index * 18
                height: 220 - index * 10
                x: stage.width / 2 - 90 - index * 36 - width / 2
                y: stage.height / 2 - height / 2
                opacity: ring.offline
                    ? 0.18
                    : (ring.agentActive ? 0.85 : 0.55) * (1.0 - index * 0.09)
                Behavior on opacity { NumberAnimation { duration: 300 } }
                ShapePath {
                    strokeColor: index % 4 === 3 ? ring.colorAccent : ring.colorPrimary
                    strokeWidth: index % 4 === 3 ? 2.5 : (index % 2 === 0 ? 1.4 : 0.8)
                    fillColor: "transparent"
                    PathAngleArc {
                        centerX: leftRing.width / 2
                        centerY: leftRing.height / 2
                        radiusX: leftRing.width / 2
                        radiusY: leftRing.height / 2
                        startAngle: 90
                        sweepAngle: 180
                    }
                }
            }
        }

        // ─── Sonar half-rings on the right side of the core ───────────
        Repeater {
            model: 7
            delegate: Shape {
                id: rightRing
                required property int index
                width: 70 + index * 18
                height: 220 - index * 10
                x: stage.width / 2 + 90 + index * 36 - width / 2
                y: stage.height / 2 - height / 2
                opacity: ring.offline
                    ? 0.18
                    : (ring.agentActive ? 0.85 : 0.55) * (1.0 - index * 0.09)
                Behavior on opacity { NumberAnimation { duration: 300 } }
                ShapePath {
                    strokeColor: index % 4 === 3 ? ring.colorAccent : ring.colorPrimary
                    strokeWidth: index % 4 === 3 ? 2.5 : (index % 2 === 0 ? 1.4 : 0.8)
                    fillColor: "transparent"
                    PathAngleArc {
                        centerX: rightRing.width / 2
                        centerY: rightRing.height / 2
                        radiusX: rightRing.width / 2
                        radiusY: rightRing.height / 2
                        startAngle: 270
                        sweepAngle: 180
                    }
                }
            }
        }

        // ─── Synthetic waveform crossing the stage horizontally ───────
        // Three superimposed sines at different frequencies and phases,
        // refreshed at ~30 fps. An envelope dips the amplitude near the
        // core so the waveform looks like it's emerging *from* the core,
        // not overlapping it. F3b swaps `phase`/`amplitude` for the
        // voice daemon's running level + dominant-band data.
        Canvas {
            id: waveform
            anchors.fill: parent
            renderStrategy: Canvas.Threaded
            property real phase: 0.0
            property real amplitude: ring.agentActive ? 1.0 : 0.35
            Behavior on amplitude { NumberAnimation { duration: 350 } }
            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                if (ring.offline) {
                    return;
                }
                var w = width;
                var h = height;
                var cy = h / 2;
                var amp = waveform.amplitude;
                var configs = [
                    { color: ring.colorSoft,    width: 1.0, freq: 0.030, ampMul: 18, phase: phase * 1.7 },
                    { color: ring.colorPrimary, width: 1.6, freq: 0.018, ampMul: 32, phase: phase * 1.0 },
                    { color: ring.colorAccent,  width: 1.2, freq: 0.011, ampMul: 24, phase: phase * 0.6 + 1.2 }
                ];
                for (var c = 0; c < configs.length; c++) {
                    var cfg = configs[c];
                    ctx.beginPath();
                    ctx.strokeStyle = cfg.color;
                    ctx.lineWidth = cfg.width;
                    ctx.globalAlpha = (cfg.color === ring.colorAccent ? 0.55 : 0.7) * amp;
                    for (var x = 0; x <= w; x += 2) {
                        var distFromCore = Math.abs(x - w / 2);
                        var envelope = Math.min(1.0, distFromCore / 110);
                        var y = cy + Math.sin(x * cfg.freq + cfg.phase)
                                       * cfg.ampMul * amp * envelope;
                        if (x === 0) ctx.moveTo(x, y);
                        else ctx.lineTo(x, y);
                    }
                    ctx.stroke();
                }
                ctx.globalAlpha = 1.0;
            }
        }
        Timer {
            interval: 33
            running: !ring.offline
            repeat: true
            onTriggered: {
                waveform.phase += 0.12;
                waveform.requestPaint();
            }
        }

        // ─── Mecha core (center) ──────────────────────────────────────
        Item {
            id: core
            anchors.centerIn: parent
            width: 160
            height: 160

            // Glow halo
            Rectangle {
                anchors.centerIn: parent
                width: parent.width + 22
                height: parent.height + 22
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1
                opacity: ring.agentActive ? 0.55 : 0.25
                Behavior on opacity { NumberAnimation { duration: 250 } }
            }

            // Outer ring — solid
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 2
            }

            // Outer ring tick marks (12 cardinals + intercardinals)
            Repeater {
                model: 12
                Item {
                    width: core.width
                    height: core.height
                    anchors.centerIn: parent
                    rotation: index * 30
                    Rectangle {
                        width: 1
                        height: index % 3 === 0 ? 9 : 5
                        color: ring.colorPrimary
                        opacity: index % 3 === 0 ? 0.95 : 0.55
                        anchors.top: parent.top
                        anchors.horizontalCenter: parent.horizontalCenter
                    }
                }
            }

            // Mid ring with rotating dashes (replaces the old 4-arc)
            Item {
                anchors.centerIn: parent
                width: parent.width - 28
                height: parent.height - 28
                Rectangle {
                    anchors.fill: parent
                    radius: width / 2
                    color: "transparent"
                    border.color: ring.colorDeep
                    border.width: 1
                    opacity: ring.offline ? 0.3 : 0.7
                }
                Repeater {
                    model: 8
                    Item {
                        width: parent.width
                        height: parent.height
                        anchors.centerIn: parent
                        rotation: index * 45
                        Rectangle {
                            width: 12
                            height: 2
                            color: index % 2 === 0 ? ring.colorPrimary : ring.colorAccent
                            opacity: ring.offline ? 0.25 : 0.85
                            anchors.top: parent.top
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.topMargin: -1
                        }
                    }
                }
                RotationAnimation on rotation {
                    from: 0; to: 360
                    duration: ring.agentActive ? 4500 : 14000
                    loops: Animation.Infinite
                    running: !ring.offline
                }
            }

            // Hexagonal segment overlay — six small hexagons placed
            // around the inner ring. This is what gives the core its
            // "mecha" feel without doing 3D geometry.
            Item {
                id: hexCage
                anchors.centerIn: parent
                width: core.width - 60
                height: core.height - 60
                Repeater {
                    model: 6
                    Item {
                        width: hexCage.width
                        height: hexCage.height
                        anchors.centerIn: parent
                        rotation: index * 60
                        Shape {
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.top: parent.top
                            width: 14
                            height: 14
                            ShapePath {
                                strokeColor: ring.colorPrimary
                                strokeWidth: 1
                                fillColor: ring.agentActive
                                    ? Qt.rgba(0.36, 0.79, 1, 0.18)
                                    : Qt.rgba(0.36, 0.79, 1, 0.06)
                                startX: 7; startY: 0
                                PathLine { x: 14; y: 4 }
                                PathLine { x: 14; y: 10 }
                                PathLine { x: 7; y: 14 }
                                PathLine { x: 0; y: 10 }
                                PathLine { x: 0; y: 4 }
                                PathLine { x: 7; y: 0 }
                            }
                        }
                    }
                }
                RotationAnimation on rotation {
                    from: 360; to: 0
                    duration: ring.agentActive ? 9000 : 22000
                    loops: Animation.Infinite
                    running: !ring.offline
                }
            }

            // Inner ring — pulse-scaled when active
            Rectangle {
                id: innerRing
                anchors.centerIn: parent
                width: 56
                height: 56
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1.5
                opacity: 0.85
                SequentialAnimation on scale {
                    running: ring.agentActive
                    loops: Animation.Infinite
                    NumberAnimation { from: 1.0; to: 1.12; duration: 700 }
                    NumberAnimation { from: 1.12; to: 1.0; duration: 700 }
                }
            }

            // Core fill — gradient circle suggesting depth without 3D
            Rectangle {
                anchors.centerIn: parent
                width: 46
                height: 46
                radius: width / 2
                gradient: Gradient {
                    GradientStop { position: 0.0; color: ring.colorSoft }
                    GradientStop { position: 0.55; color: ring.colorPrimary }
                    GradientStop { position: 1.0; color: ring.colorDeep }
                }
                opacity: 0.95
            }

            // Crosshairs through the core (subtle)
            Rectangle {
                anchors.centerIn: parent
                width: core.width - 40
                height: 1
                color: ring.colorDeep
                opacity: 0.25
            }
            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: core.height - 40
                color: ring.colorDeep
                opacity: 0.25
            }

            // Center label
            Text {
                anchors.centerIn: parent
                text: "JARVIS"
                color: ring.offline ? "#150c02" : "#02060e"
                font.family: "monospace"
                font.pixelSize: 8
                font.bold: true
                font.letterSpacing: 1.5
            }
        }

        // ─── HUD mini displays at the top corners ─────────────────────
        // Decorative; matches the SVG mockup language. Synthetic
        // numbers in F3a — F3b plugs the voice daemon's running level
        // and dominant-band frequency.
        Column {
            anchors.top: parent.top
            anchors.left: parent.left
            spacing: 1
            Text {
                text: "AMPLITUDE"
                color: ring.colorPrimary
                font.family: "monospace"
                font.pixelSize: 8
                font.letterSpacing: 1.2
                opacity: 0.85
            }
            Text {
                text: ring.offline ? "—— dB" : (ring.agentActive ? "−12 dB" : "−42 dB")
                color: ring.colorSoft
                font.family: "monospace"
                font.pixelSize: 9
                opacity: 0.7
            }
        }
        Column {
            anchors.top: parent.top
            anchors.right: parent.right
            spacing: 1
            Text {
                text: "FREQUENCY HZ"
                color: ring.colorPrimary
                font.family: "monospace"
                font.pixelSize: 8
                font.letterSpacing: 1.2
                opacity: 0.85
                anchors.right: parent.right
            }
            Text {
                text: ring.offline ? "————" : (ring.agentActive ? "440 — 8k" : "—")
                color: ring.colorSoft
                font.family: "monospace"
                font.pixelSize: 9
                opacity: 0.7
                anchors.right: parent.right
            }
        }

        // ─── Status text directly under the core ──────────────────────
        Text {
            anchors.top: core.bottom
            anchors.topMargin: 18
            anchors.horizontalCenter: core.horizontalCenter
            color: ring.colorPrimary
            font.family: "monospace"
            font.pixelSize: 10
            font.letterSpacing: 2.5
            text: ring.offline ? "OFFLINE"
                : ring.agentActive ? "ACTIVE"
                : "IDLE"
            opacity: 0.95
        }
    }
}
