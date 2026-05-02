import QtQuick
import Quickshell
import Quickshell.Wayland
import "../core"

// Reference: ~/jarvis-orbe.jpg (2D, flat).
// Single-Canvas implementation. Layers from outside in:
//   - Soft radial center glow.
//   - Outer ring: glowCircle + segmentedRing (8 segs) + 64 ticks.
//   - Middle ring: segmentedRing (20 segs) + thick 270° arc (gap up).
//   - Progress accent (amber arc) reflecting active-tool load.
//   - Status dots (4 amber points up-right) with shadowBlur glow.
//   - Clock-hand field: 60 uniform ticks rotating as one.
//   - Inner variable-thickness ring (phase-shifted, no bitmap rotation).
//   - Five overlapping audio bands at the same interior radius.
//   - Core disc + J.A.R.V.I.S. text.
//   - Status text below.
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

    // 0..1 — fraction of "active tool slots" in use. Drives the amber
    // progress arc on the middle ring. Saturates at three concurrent
    // tools.
    readonly property real toolLoad: Math.min(1.0,
        Object.keys(EventBus.activeTools).length / 3.0)

    // Hex palette hardcoded (Palette singleton sometimes resolves to
    // undefined in Quickshell setups; we cache the values here).
    readonly property color colorPrimary: ring.offline ? "#f5c56a" : "#5dcbff"
    readonly property color colorDeep:    ring.offline ? "#7a5a2a" : "#3aa8e8"
    readonly property color colorSoft:    ring.offline ? "#f7d59b" : "#9fe6ff"
    readonly property color colorAccent:  "#f5c56a"
    readonly property color colorDot:     "#e8dd43"

    Item {
        id: stage
        anchors.centerIn: parent
        width: 540
        height: 540

        // Single Canvas paints every animated graphic. Everything that
        // would benefit from QML's text rendering or hit testing
        // (J.A.R.V.I.S. label, status text, core disc) lives outside.
        Canvas {
            id: orb
            anchors.fill: parent
            antialiasing: true
            renderStrategy: Canvas.Threaded

            // Animated phases. `spin` drives ring rotations; `phase`
            // drives the variable-thickness lobe and the breathing
            // audio bands.
            property real spin: 0
            property real phase: 0
            property real audioAmp: ring.agentActive ? 1.0 : 0.35
            Behavior on audioAmp { NumberAnimation { duration: 350 } }

            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                if (ring.offline) {
                    return;
                }

                var w = width;
                var h = height;
                var cx = w / 2;
                var cy = h / 2;
                var base = Math.min(w, h) / 2;

                var outerR = base * 0.42;
                var midR   = base * 0.33;
                var innerR = base * 0.26;
                var coreR  = base * 0.18;

                // ─── Helpers ──────────────────────────────────────────
                function degToRad(deg) {
                    return (deg - 90) * Math.PI / 180.0;
                }
                function arc(r, a1, a2, lw, color, alpha) {
                    ctx.save();
                    ctx.globalAlpha = alpha === undefined ? 1 : alpha;
                    ctx.beginPath();
                    ctx.lineWidth = lw;
                    ctx.strokeStyle = color;
                    ctx.arc(cx, cy, r, degToRad(a1), degToRad(a2), false);
                    ctx.stroke();
                    ctx.restore();
                }
                function circle(r, lw, color, alpha) {
                    arc(r, 0, 360, lw, color, alpha);
                }
                function glowCircle(r, lw, color, alpha, blur) {
                    ctx.save();
                    ctx.shadowBlur = blur;
                    ctx.shadowColor = color;
                    circle(r, lw, color, alpha);
                    ctx.restore();
                }
                function ticks(r, count, len, lw, color, alpha, rotOffset, skipEvery) {
                    ctx.save();
                    ctx.translate(cx, cy);
                    ctx.rotate((rotOffset || 0) * Math.PI / 180.0);
                    ctx.strokeStyle = color;
                    ctx.lineWidth = lw;
                    ctx.globalAlpha = alpha === undefined ? 1 : alpha;
                    for (var i = 0; i < count; ++i) {
                        if (skipEvery > 0 && i % skipEvery === 0) {
                            continue;
                        }
                        var a = i * Math.PI * 2 / count;
                        var x1 = Math.cos(a - Math.PI / 2) * (r - len);
                        var y1 = Math.sin(a - Math.PI / 2) * (r - len);
                        var x2 = Math.cos(a - Math.PI / 2) * r;
                        var y2 = Math.sin(a - Math.PI / 2) * r;
                        ctx.beginPath();
                        ctx.moveTo(x1, y1);
                        ctx.lineTo(x2, y2);
                        ctx.stroke();
                    }
                    ctx.restore();
                }
                function segmentedRing(r, count, gapDeg, lw, color, alpha, rotOffset) {
                    var span = 360 / count - gapDeg;
                    for (var i = 0; i < count; ++i) {
                        var start = (rotOffset || 0) + i * (360 / count);
                        var end = start + span;
                        arc(r, start, end, lw, color, alpha);
                    }
                }

                // ─── Soft center glow ─────────────────────────────────
                var grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, outerR);
                grad.addColorStop(0.0, "rgba(70, 210, 255, 0.18)");
                grad.addColorStop(0.4, "rgba(40, 160, 220, 0.10)");
                grad.addColorStop(1.0, "rgba(0, 0, 0, 0.0)");
                ctx.fillStyle = grad;
                ctx.beginPath();
                ctx.arc(cx, cy, outerR, 0, Math.PI * 2);
                ctx.fill();

                // ─── Outer ring: glow + segmented + 64 ticks ─────────
                glowCircle(outerR, 5, ring.colorPrimary, 0.95, 10);
                segmentedRing(outerR, 8, 18, 7, ring.colorPrimary, 0.9, orb.spin);
                ticks(outerR + 18, 64, 14, 1.6, ring.colorSoft, 0.7, orb.spin / 2, 0);

                // ─── Middle ring ─────────────────────────────────────
                segmentedRing(midR, 20, 5, 12, "rgba(120,220,255,0.55)",
                              1.0, -orb.spin * 0.7);
                circle(midR, 1.5, "rgba(120,220,255,0.35)", 0.85);

                // Thick 270° arc with gap centered at the top.
                // PathAngleArc convention here: degToRad already
                // subtracts 90, so a1=45 and a2=315 traces clockwise
                // leaving the top 90° open.
                ctx.save();
                ctx.shadowBlur = 8;
                ctx.shadowColor = ring.colorPrimary;
                arc(midR + 8, 45, 315, 4, ring.colorPrimary, 0.95);
                ctx.restore();

                // ─── Progress accent (amber, tool load) ──────────────
                if (ring.toolLoad > 0.001) {
                    var progStart = 230;
                    var progEnd = progStart + ring.toolLoad * 120;
                    arc(midR - 6, progStart, progEnd, 8, ring.colorAccent, 1.0);
                    arc(midR - 6, progStart - 12, progStart + 4, 10,
                        ring.colorAccent, 0.9);
                    arc(midR - 6, progEnd - 4, progEnd + 12, 10,
                        ring.colorAccent, 0.9);
                }

                // ─── Status dots (4 amber points, top right) ─────────
                for (var d = 0; d < 4; ++d) {
                    var da = degToRad(332 + d * 11);
                    var dx = Math.cos(da) * (midR - 2);
                    var dy = Math.sin(da) * (midR - 2);
                    ctx.save();
                    ctx.beginPath();
                    ctx.fillStyle = ring.colorDot;
                    ctx.shadowBlur = 8;
                    ctx.shadowColor = ring.colorDot;
                    ctx.arc(cx + dx, cy + dy, 3.4, 0, Math.PI * 2);
                    ctx.fill();
                    ctx.restore();
                }

                // ─── Clock hand field: 60 uniform ticks rotating ─────
                ticks(innerR + 12, 60, 5, 1, ring.colorPrimary, 0.85, orb.spin, 0);

                // ─── Inner variable-thickness ring (phase-shifted) ───
                ctx.save();
                ctx.shadowBlur = 12;
                ctx.shadowColor = ring.colorSoft;
                ctx.strokeStyle = ring.colorSoft;
                ctx.lineCap = "round";
                ctx.globalAlpha = 0.95;
                var step = 2;
                var stepRad = step * Math.PI / 180;
                for (var deg = 0; deg < 360; deg += step) {
                    var theta = deg * Math.PI / 180;
                    var lobeW = 1.2 + 4.0 * Math.abs(Math.sin(theta + orb.phase));
                    ctx.lineWidth = lobeW;
                    ctx.beginPath();
                    ctx.arc(cx, cy, innerR, theta, theta + stepRad);
                    ctx.stroke();
                }
                ctx.restore();

                // Faint helper rings around the inner area.
                circle(innerR - 16, 1.5, "rgba(140,235,255,0.22)", 0.8);

                // ─── Five overlapping audio bands (same radius) ──────
                // All bands at the same rBase so they overlap; n /
                // speed / color give each band its visual identity.
                var bandRBase = coreR + 8;
                var bandConfigs = [
                    { color: ring.colorAccent,  ampMul: 6,  n: 4,  speed: 0.4 },
                    { color: ring.colorDeep,    ampMul: 5,  n: 6,  speed: 0.7 },
                    { color: ring.colorPrimary, ampMul: 6,  n: 8,  speed: 1.0 },
                    { color: ring.colorSoft,    ampMul: 4,  n: 11, speed: 1.3 },
                    { color: "#f7d59b",         ampMul: 3,  n: 14, speed: 1.7 }
                ];
                var t = orb.phase;
                var amp = orb.audioAmp;
                for (var c = 0; c < bandConfigs.length; c++) {
                    var cfg = bandConfigs[c];
                    ctx.save();
                    ctx.beginPath();
                    ctx.strokeStyle = cfg.color;
                    ctx.lineWidth = 1.2;
                    ctx.globalAlpha = (cfg.color === ring.colorAccent ? 0.6 : 0.7) * amp;
                    var first = true;
                    for (var bdeg = 0; bdeg <= 360; bdeg += 1) {
                        var btheta = bdeg * Math.PI / 180;
                        var br = bandRBase
                                 + cfg.ampMul * amp
                                   * Math.sin(cfg.n * btheta + t * cfg.speed);
                        var bx = cx + br * Math.cos(btheta);
                        var by = cy + br * Math.sin(btheta);
                        if (first) {
                            ctx.moveTo(bx, by);
                            first = false;
                        } else {
                            ctx.lineTo(bx, by);
                        }
                    }
                    ctx.closePath();
                    ctx.stroke();
                    ctx.restore();
                }

                // ─── Core decoration ─────────────────────────────────
                circle(coreR, 1.5, "rgba(111,234,255,0.20)", 0.7);
                circle(coreR + 14, 1, "rgba(111,234,255,0.10)", 0.6);
            }

            Timer {
                interval: 33                    // ~30 fps
                running: !ring.offline
                repeat: true
                onTriggered: {
                    orb.spin = (orb.spin + 0.6) % 360;
                    orb.phase += 0.025;
                    orb.requestPaint();
                }
            }

            Component.onCompleted: requestPaint()
        }

        // ─── Core disc + J.A.R.V.I.S. text (outside Canvas) ───────────
        // Crisp Text rendering rather than fillText. The disc is a
        // Rectangle so the shape is hit-test-clean even though the
        // surface is overall pointer-pass-through.
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
                font.pixelSize: 11
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
