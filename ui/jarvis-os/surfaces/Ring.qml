import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import "../core"

// Reference: ~/jarvis-orbe.jpg (2D, flat).
// Layers from inside out:
//   1. J.A.R.V.I.S. text + flat inner disc
//   2. Five overlapping audio bands at the SAME radius (interior).
//      Each band has its own n / phase speed / color so the same ring
//      breathes with multiple frequencies at once.
//   3. Variable-thickness ring (pattern shifts via phase, no bitmap
//      rotation — avoids canvas pixelation).
//   4. 60 uniform-height clock-hand ticks rotating as a single field.
//   5. Thick arc ring (270°, gap at top, rotates).
//   6. Outer variable-thickness ring (phase-shifted, not rotated).
PanelWindow {
    id: ring

    anchors { top: true; bottom: true; left: true; right: true }

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    mask: Region {}

    readonly property bool agentActive: Object.keys(EventBus.activeTools).length > 0
    readonly property bool offline: !EventBus.connected

    // Hardcoded palette — reading from a separate Palette singleton
    // was fragile (the import resolved to undefined in some Quickshell
    // setups, leaving strokeStyle at its default of black). The hex
    // codes here match `theme/Palette.qml`.
    readonly property color colorPrimary: ring.offline ? "#f5c56a" : "#5dcbff"
    readonly property color colorDeep:    ring.offline ? "#7a5a2a" : "#3aa8e8"
    readonly property color colorSoft:    ring.offline ? "#f7d59b" : "#9fe6ff"
    readonly property color colorAccent:  "#f5c56a"

    Item {
        id: stage
        anchors.centerIn: parent
        width: 600
        height: 540

        // ─── Layer 6 — outer variable-thickness ring ──────────────────
        // Drawn as 90 short arc segments whose lineWidth modulates with
        // angle + a phase that animates over time. We do NOT rotate
        // the Item — rotating a Canvas as a bitmap caused the
        // pixelation the user reported. Instead the pattern shifts via
        // `phase`, repainting each frame.
        Canvas {
            id: outerVarRing
            anchors.centerIn: parent
            width: 240
            height: 240
            renderStrategy: Canvas.Threaded
            antialiasing: true
            property real phase: 0.0
            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                if (ring.offline) {
                    return;
                }
                var cx = width / 2;
                var cy = height / 2;
                var r = width / 2 - 6;
                ctx.strokeStyle = ring.colorPrimary;
                ctx.lineCap = "round";
                var step = 2;                // tighter step → smoother
                var stepRad = step * Math.PI / 180;
                for (var deg = 0; deg < 360; deg += step) {
                    var theta = deg * Math.PI / 180;
                    // Two thick lobes orbiting around the ring as
                    // `phase` advances.
                    var w = 1.0 + 4.0 * Math.abs(Math.sin(theta + outerVarRing.phase));
                    ctx.lineWidth = w;
                    ctx.beginPath();
                    ctx.arc(cx, cy, r, theta, theta + stepRad);
                    ctx.stroke();
                }
            }
        }

        // ─── Layer 5 — thick arc ring with 90° gap at the top ─────────
        // Vector Shape rotates without pixelation (it's not a bitmap).
        Item {
            id: arcHolder
            anchors.centerIn: parent
            width: 200
            height: 200
            Shape {
                anchors.fill: parent
                ShapePath {
                    strokeColor: ring.colorPrimary
                    strokeWidth: 5
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    PathAngleArc {
                        centerX: arcHolder.width / 2
                        centerY: arcHolder.height / 2
                        radiusX: arcHolder.width / 2
                        radiusY: arcHolder.height / 2
                        startAngle: 225
                        sweepAngle: 270
                    }
                }
            }
            RotationAnimation on rotation {
                running: !ring.offline
                from: 0; to: -360
                duration: ring.agentActive ? 18000 : 45000
                loops: Animation.Infinite
            }
        }

        // ─── Layer 4 — clock-hand tick field (60 marks, all equal) ────
        // 60 ticks every 6°, all the same length and color. Rotates as
        // a single field. (Cardinals were taller before; the user
        // wants them uniform.)
        Item {
            id: clockHands
            anchors.centerIn: parent
            width: 170
            height: 170
            Repeater {
                model: 60
                Item {
                    width: 170
                    height: 170
                    anchors.centerIn: parent
                    rotation: index * 6
                    Rectangle {
                        width: 1
                        height: 5
                        color: ring.colorPrimary
                        opacity: 0.85
                        anchors.top: parent.top
                        anchors.horizontalCenter: parent.horizontalCenter
                    }
                }
            }
            RotationAnimation on rotation {
                running: !ring.offline
                from: 0; to: 360
                duration: ring.agentActive ? 22000 : 55000
                loops: Animation.Infinite
            }
        }

        // ─── Layer 3 — inner variable-thickness ring ──────────────────
        // Same phase-shift technique as the outer ring; smaller and
        // with the lobe orbiting in the opposite direction so the two
        // variable rings don't beat in lockstep.
        Canvas {
            id: innerVarRing
            anchors.centerIn: parent
            width: 130
            height: 130
            renderStrategy: Canvas.Threaded
            antialiasing: true
            property real phase: 0.0
            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                if (ring.offline) {
                    return;
                }
                var cx = width / 2;
                var cy = height / 2;
                var r = width / 2 - 4;
                ctx.strokeStyle = ring.colorPrimary;
                ctx.lineCap = "round";
                var step = 2;
                var stepRad = step * Math.PI / 180;
                for (var deg = 0; deg < 360; deg += step) {
                    var theta = deg * Math.PI / 180;
                    // Lobe orbits opposite to the outer ring.
                    var w = 0.8 + 3.0 * Math.abs(Math.sin(theta - innerVarRing.phase));
                    ctx.lineWidth = w;
                    ctx.beginPath();
                    ctx.arc(cx, cy, r, theta, theta + stepRad);
                    ctx.stroke();
                }
            }
        }

        // ─── Layer 2 — five overlapping audio bands at the same radius ─
        // All five closed undulating curves sit at the same rBase, just
        // inside the inner variable-thickness ring (so the bands feel
        // like the orb's "voice"). Each band has its own n (petal
        // count), phase speed (ω), color and amplitude — so they
        // overlap continuously without ever drawing the same shape.
        // F3a uses synthetic phases; F3b will swap them for the voice
        // daemon's per-band level data.
        Canvas {
            id: waveform
            anchors.centerIn: parent
            width: 130
            height: 130
            renderStrategy: Canvas.Threaded
            antialiasing: true
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
                // Same rBase for all five — they overlap in the same
                // ring. Differentiation comes from n / speed / color.
                var rBase = 52;
                var configs = [
                    { color: ring.colorAccent,  ampMul: 6,  n: 4,  speed: 0.4 },  // bass
                    { color: ring.colorDeep,    ampMul: 5,  n: 6,  speed: 0.7 },  // low-mid
                    { color: ring.colorPrimary, ampMul: 6,  n: 8,  speed: 1.0 },  // mid
                    { color: ring.colorSoft,    ampMul: 4,  n: 11, speed: 1.3 },  // high-mid
                    { color: "#f7d59b",         ampMul: 3,  n: 14, speed: 1.7 }   // treble
                ];
                for (var c = 0; c < configs.length; c++) {
                    var cfg = configs[c];
                    ctx.beginPath();
                    ctx.strokeStyle = cfg.color;
                    ctx.lineWidth = 1.2;
                    ctx.globalAlpha = (cfg.color === ring.colorAccent ? 0.6 : 0.7) * amp;
                    var firstPoint = true;
                    for (var deg = 0; deg <= 360; deg += 1) {
                        var theta = deg * Math.PI / 180;
                        var r = rBase
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

        // Single timer drives every animated phase so all canvases stay
        // in sync at ~30 fps.
        Timer {
            interval: 33
            running: !ring.offline
            repeat: true
            onTriggered: {
                waveform.phase += 0.12;
                outerVarRing.phase += 0.025;
                innerVarRing.phase += 0.025;
                waveform.requestPaint();
                outerVarRing.requestPaint();
                innerVarRing.requestPaint();
            }
        }

        // ─── Layer 1 — central disc + J.A.R.V.I.S. label ──────────────
        Item {
            id: core
            anchors.centerIn: parent
            width: 80
            height: 80

            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: ring.colorDeep
                opacity: 0.4
            }
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1
                opacity: 0.7
            }
            Text {
                anchors.centerIn: parent
                text: "J.A.R.V.I.S."
                color: ring.colorSoft
                font.family: "monospace"
                font.pixelSize: 10
                font.bold: true
                font.letterSpacing: 0.8
            }
        }

        // ─── Status text under the orb ────────────────────────────────
        Text {
            anchors.top: stage.verticalCenter
            anchors.topMargin: 130
            anchors.horizontalCenter: stage.horizontalCenter
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
