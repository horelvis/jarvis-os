import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import "../core"
import "../theme"

// Reference: ~/jarvis-orbe.jpg (2D, flat).
// Layers from inside out:
//   1. J.A.R.V.I.S. text + flat inner disc
//   2. Variable-thickness ring (rotates slowly)
//   3. 60 clock-hand ticks rotating as a single field
//   4. Thick arc ring (270°, gap at top, rotates)
//   5. Outer variable-thickness ring (rotates)
//   6. Five concentric breathing audio bands (bass → treble)
// Idle = slow rotation; ACTIVE = ~2.5× faster.
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

    readonly property color colorPrimary: ring.offline ? Palette.amber : Palette.cyanPrimary
    readonly property color colorDeep:    ring.offline ? "#7a5a2a" : Palette.cyanDeep
    readonly property color colorSoft:    ring.offline ? "#f7d59b" : Palette.cyanSoft
    readonly property color colorAccent:  Palette.amber

    Item {
        id: stage
        anchors.centerIn: parent
        width: 600
        height: 540

        // ─── Layer 6 — five concentric breathing audio bands ──────────
        // Outside everything else. One closed undulating curve per
        // band: r(θ,t) = R_base + A · sin(n·θ + φ·t). F3a uses
        // synthetic phases; F3b will swap them for the voice daemon's
        // per-band level data.
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
                // The bands sit outside the static ring stack (whose
                // outermost element reaches ~118 px radius). First band
                // at 140; outer band at 240.
                var configs = [
                    { color: ring.colorAccent,  width: 1.4, rBase: 140, ampMul: 7,  n: 4,  speed: 0.4 },  // bass
                    { color: ring.colorDeep,    width: 1.2, rBase: 165, ampMul: 7,  n: 6,  speed: 0.7 },  // low-mid
                    { color: ring.colorPrimary, width: 1.5, rBase: 190, ampMul: 8,  n: 8,  speed: 1.0 },  // mid
                    { color: ring.colorSoft,    width: 1.0, rBase: 215, ampMul: 6,  n: 11, speed: 1.3 },  // high-mid
                    { color: "#f7d59b",         width: 0.9, rBase: 240, ampMul: 5,  n: 14, speed: 1.7 }   // treble
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

        // ─── Layer 5 — outer variable-thickness ring ──────────────────
        // Drawn as 90 short arc segments whose lineWidth modulates with
        // the angle. Rotates slowly clockwise.
        Item {
            id: outerRingHolder
            anchors.centerIn: parent
            width: 240
            height: 240
            Canvas {
                id: outerVarRing
                anchors.fill: parent
                renderStrategy: Canvas.Threaded
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
                    var step = 4;
                    var stepRad = step * Math.PI / 180;
                    for (var deg = 0; deg < 360; deg += step) {
                        var theta = deg * Math.PI / 180;
                        // Two thick lobes at left + right (sin of 2θ
                        // peaks at 0/π), thinner top/bottom.
                        var w = 1.0 + 4.0 * Math.abs(Math.sin(theta));
                        ctx.lineWidth = w;
                        ctx.beginPath();
                        ctx.arc(cx, cy, r, theta, theta + stepRad);
                        ctx.stroke();
                    }
                }
            }
            RotationAnimation on rotation {
                running: !ring.offline
                from: 0; to: 360
                duration: ring.agentActive ? 28000 : 70000
                loops: Animation.Infinite
            }
        }

        // ─── Layer 4 — thick arc ring with 90° gap at the top ─────────
        // 270° solid stroke. Gap centered at 12 o'clock. Rotates so the
        // gap orbits the orb.
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
                        // PathAngleArc 0° = 3 o'clock, sweep is CCW.
                        // Gap centered at 12 o'clock means arc covers
                        // from 225° to -45° going CCW = 270°.
                        // Equivalently: start 225, sweep 270.
                        startAngle: 225
                        sweepAngle: 270
                    }
                }
            }
            RotationAnimation on rotation {
                running: !ring.offline
                from: 0; to: -360       // counter-rotate vs the outer ring
                duration: ring.agentActive ? 18000 : 45000
                loops: Animation.Infinite
            }
        }

        // ─── Layer 3 — clock-hand tick field (60 marks) ───────────────
        // 60 ticks every 6°; cardinals (every 5th) longer. Rotates as
        // a single field.
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
                        height: index % 5 === 0 ? 11 : 4
                        color: index % 5 === 0
                            ? ring.colorPrimary
                            : ring.colorDeep
                        opacity: index % 5 === 0 ? 0.95 : 0.6
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

        // ─── Layer 2 — inner variable-thickness ring ──────────────────
        // Same technique as the outer variable ring but smaller and
        // with thicker lobes for visual continuity. Rotates the
        // opposite direction so the variable thickness pattern is not
        // identical to the outer ring's.
        Item {
            id: innerRingHolder
            anchors.centerIn: parent
            width: 130
            height: 130
            Canvas {
                anchors.fill: parent
                renderStrategy: Canvas.Threaded
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
                    var step = 4;
                    var stepRad = step * Math.PI / 180;
                    for (var deg = 0; deg < 360; deg += step) {
                        var theta = deg * Math.PI / 180;
                        var w = 0.8 + 3.0 * Math.abs(Math.sin(theta));
                        ctx.lineWidth = w;
                        ctx.beginPath();
                        ctx.arc(cx, cy, r, theta, theta + stepRad);
                        ctx.stroke();
                    }
                }
            }
            RotationAnimation on rotation {
                running: !ring.offline
                from: 0; to: -360
                duration: ring.agentActive ? 25000 : 60000
                loops: Animation.Infinite
            }
        }

        // ─── Layer 1 — central disc + J.A.R.V.I.S. label ──────────────
        Item {
            id: core
            anchors.centerIn: parent
            width: 96
            height: 96

            // Flat fill — deep cyan, low opacity. No gradient.
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: ring.colorDeep
                opacity: 0.35
            }

            // Subtle border so the disc has a hard edge against the
            // surrounding rings.
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.color: ring.colorPrimary
                border.width: 1
                opacity: 0.6
            }

            Text {
                anchors.centerIn: parent
                text: "J.A.R.V.I.S."
                color: ring.colorSoft
                font.family: "monospace"
                font.pixelSize: 11
                font.bold: true
                font.letterSpacing: 0.8
            }
        }

        // ─── Status text under the orb ────────────────────────────────
        Text {
            anchors.top: core.bottom
            anchors.topMargin: 178
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
