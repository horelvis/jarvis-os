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
    // The orb sits at the middle, with concentric rings (varied
    // thickness, two of them rotating) wrapping the core. A synthetic
    // waveform crosses the entire stage. F3a uses synthetic data; F3b
    // will swap the source for the voice daemon.
    // ──────────────────────────────────────────────────────────────────
    Item {
        id: stage
        anchors.centerIn: parent
        width: 600
        height: 240

        // ─── Synthetic circular waveforms wrapping the core ───────────
        // Three concentric undulating rings — closed curves whose radius
        // varies with the angle: r(θ,t) = R_base + A·sin(n·θ + φ·t).
        // Each curve has its own n (petal count), phase speed, and
        // amplitude, so the field breathes asymmetrically without any
        // single frequency dominating. Spec sec. 5.4.3.
        //
        // F3a uses synthetic phase animation; F3b swaps `phase` and
        // `amplitude` for the voice daemon's running level + dominant-
        // band data so the rings react to actual audio.
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
                var cx = width / 2;
                var cy = height / 2;
                var amp = waveform.amplitude;
                var t = waveform.phase;
                // rBase: average radius. ampMul: how much the radius
                // wobbles. n: number of petals. speed: phase ω.
                var configs = [
                    { color: ring.colorSoft,    width: 0.9, rBase: 78,  ampMul: 5,  n: 11, speed: 1.3 },
                    { color: ring.colorPrimary, width: 1.5, rBase: 100, ampMul: 8,  n: 7,  speed: 1.0 },
                    { color: ring.colorAccent,  width: 1.2, rBase: 128, ampMul: 6,  n: 5,  speed: 0.6 }
                ];
                for (var c = 0; c < configs.length; c++) {
                    var cfg = configs[c];
                    ctx.beginPath();
                    ctx.strokeStyle = cfg.color;
                    ctx.lineWidth = cfg.width;
                    ctx.globalAlpha = (cfg.color === ring.colorAccent ? 0.55 : 0.7) * amp;
                    var firstPoint = true;
                    for (var deg = 0; deg <= 360; deg += 1) {
                        var theta = deg * Math.PI / 180;
                        var r = cfg.rBase
                                + cfg.ampMul * amp
                                  * Math.sin(cfg.n * theta + t * cfg.speed);
                        var px = cx + r * Math.cos(theta);
                        var py = cy + r * Math.sin(theta);
                        if (firstPoint) {
                            ctx.moveTo(px, py);
                            firstPoint = false;
                        } else {
                            ctx.lineTo(px, py);
                        }
                    }
                    ctx.closePath();
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

        // ─── Concentric rings wrapping the core ───────────────────────
        // Eight rings of varying thickness around the central orb.
        // Two of them (indices 0 and 5) rotate, giving life to the
        // composition. Every 3rd ring is amber as accent. Stroke width
        // alternates so the ring field has visual rhythm rather than a
        // uniform comb-line look.
        //
        // The model array carries radius offset, stroke width, color
        // selector, dash spec, and rotation flag per ring.
        Item {
            id: ringField
            anchors.centerIn: parent
            width: 320
            height: 320

            Repeater {
                model: [
                    { r: 0,   w: 1.0,  c: 0, dash: false, rotate: true,  rotMs: 18000 },
                    { r: 14,  w: 0.5,  c: 0, dash: false, rotate: false, rotMs: 0 },
                    { r: 22,  w: 1.8,  c: 0, dash: false, rotate: false, rotMs: 0 },
                    { r: 36,  w: 0.5,  c: 1, dash: false, rotate: false, rotMs: 0 },
                    { r: 44,  w: 1.0,  c: 0, dash: false, rotate: false, rotMs: 0 },
                    { r: 58,  w: 2.5,  c: 2, dash: false, rotate: true,  rotMs: 11000 },
                    { r: 72,  w: 0.5,  c: 0, dash: false, rotate: false, rotMs: 0 },
                    { r: 84,  w: 1.4,  c: 1, dash: true,  rotate: false, rotMs: 0 }
                ]
                delegate: Item {
                    id: concentricRing
                    required property var modelData
                    required property int index
                    anchors.centerIn: parent
                    width: 140 + modelData.r * 2
                    height: 140 + modelData.r * 2

                    rotation: 0
                    RotationAnimation on rotation {
                        running: modelData.rotate && !ring.offline
                        from: 0; to: 360
                        duration: ring.agentActive
                            ? modelData.rotMs / 2.5
                            : modelData.rotMs
                        loops: Animation.Infinite
                    }

                    Shape {
                        anchors.fill: parent
                        ShapePath {
                            strokeColor: modelData.c === 2
                                ? ring.colorAccent
                                : (modelData.c === 1
                                    ? ring.colorDeep
                                    : ring.colorPrimary)
                            strokeWidth: modelData.w
                            fillColor: "transparent"
                            strokeStyle: modelData.dash
                                ? ShapePath.DashLine
                                : ShapePath.SolidLine
                            dashPattern: [3, 4]
                            PathAngleArc {
                                centerX: concentricRing.width / 2
                                centerY: concentricRing.height / 2
                                radiusX: concentricRing.width / 2
                                radiusY: concentricRing.height / 2
                                startAngle: 0
                                sweepAngle: 360
                            }
                        }
                    }

                    // Cardinal tick marks on the outermost solid ring
                    Repeater {
                        model: modelData.r === 22 ? 12 : 0
                        Item {
                            width: concentricRing.width
                            height: concentricRing.height
                            anchors.centerIn: parent
                            rotation: index * 30
                            Rectangle {
                                width: 1
                                height: index % 3 === 0 ? 8 : 4
                                color: ring.colorPrimary
                                opacity: index % 3 === 0 ? 0.95 : 0.55
                                anchors.top: parent.top
                                anchors.horizontalCenter: parent.horizontalCenter
                            }
                        }
                    }

                    // Cardinal letters N/E/S/W just inside the second
                    // ring, kept upright via counter-rotation.
                    Repeater {
                        model: modelData.r === 22
                            ? [
                                { label: "N", angle: 0   },
                                { label: "E", angle: 90  },
                                { label: "S", angle: 180 },
                                { label: "W", angle: 270 }
                              ]
                            : []
                        Item {
                            width: concentricRing.width
                            height: concentricRing.height
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
                                rotation: -modelData.angle
                            }
                        }
                    }

                    // Slash dashes on the rotating mid ring (r=58).
                    // Four short bars at 0/90/180/270 degrees that
                    // travel with the rotation animation, giving the
                    // ring a "scanner" feel.
                    Repeater {
                        model: modelData.r === 58 ? 4 : 0
                        Item {
                            width: concentricRing.width
                            height: concentricRing.height
                            anchors.centerIn: parent
                            rotation: index * 90
                            Rectangle {
                                width: 14
                                height: 2
                                color: index % 2 === 0
                                    ? ring.colorPrimary
                                    : ring.colorAccent
                                opacity: ring.offline ? 0.3 : 0.9
                                anchors.top: parent.top
                                anchors.horizontalCenter: parent.horizontalCenter
                                anchors.topMargin: -1
                            }
                        }
                    }
                }
            }
        }

        // ─── Mecha core (center) ──────────────────────────────────────
        // Flat 2D — no radial gradient, no depth shading. The "mecha"
        // feel comes from layered solid shapes (hexagonal cage + tick
        // marks + cross hairs) over a flat fill, not from 3D illusions.
        Item {
            id: core
            anchors.centerIn: parent
            width: 110
            height: 110

            // Solid flat fill — the deep cyan disc that anchors the orb.
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: ring.colorDeep
                opacity: 0.35
            }

            // Outer ring border
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1.5
            }

            // Inner secondary ring
            Rectangle {
                anchors.centerIn: parent
                width: parent.width - 18
                height: parent.height - 18
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 0.8
                opacity: 0.65
            }

            // Hexagonal segment overlay — six small hexagons placed
            // around the inner ring. Counter-rotating cage gives the
            // core its "mecha" look without 3D geometry.
            Item {
                id: hexCage
                anchors.centerIn: parent
                width: parent.width - 30
                height: parent.height - 30
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
                            width: 12
                            height: 12
                            ShapePath {
                                strokeColor: ring.colorPrimary
                                strokeWidth: 1
                                fillColor: ring.colorDeep
                                startX: 6; startY: 0
                                PathLine { x: 12; y: 3 }
                                PathLine { x: 12; y: 9 }
                                PathLine { x: 6;  y: 12 }
                                PathLine { x: 0;  y: 9 }
                                PathLine { x: 0;  y: 3 }
                                PathLine { x: 6;  y: 0 }
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

            // Crosshair lines through the core (subtle alignment marks)
            Rectangle {
                anchors.centerIn: parent
                width: core.width - 24
                height: 1
                color: ring.colorDeep
                opacity: 0.4
            }
            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: core.height - 24
                color: ring.colorDeep
                opacity: 0.4
            }

            // Center dot — flat, no gradient. Pulse-scaled when active.
            Rectangle {
                id: centerDot
                anchors.centerIn: parent
                width: 14
                height: 14
                radius: width / 2
                color: ring.colorPrimary
                opacity: ring.offline ? 0.5 : 0.95
                SequentialAnimation on scale {
                    running: ring.agentActive
                    loops: Animation.Infinite
                    NumberAnimation { from: 1.0; to: 1.4; duration: 600 }
                    NumberAnimation { from: 1.4; to: 1.0; duration: 600 }
                }
            }
        }

        // ─── HUD mini displays at the top corners ─────────────────────
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
            anchors.topMargin: 110
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
