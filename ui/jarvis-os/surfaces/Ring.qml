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
            // Cooperative renderer syncs with the main render loop;
            // Threaded was producing visible stutter on the rotating
            // rings — frames could be dropped or arrive late on this
            // hardware.
            renderStrategy: Canvas.Cooperative

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

                // ═════════════════════════════════════════════════════════
                // ORB LAYER MAP — drawn back-to-front (later draws on top).
                // Numbered from the center outward so the design reads as
                // a stack of rings, not a sequence of paint calls.
                //
                //   [bg]      Soft radial center glow (cyan halo)
                //   ANILLO 1  Five overlapping audio bands (FFT bands)
                //   ANILLO 2  Clock-hand field (60 uniform ticks, rotates CW)
                //   ANILLO 3  Two-layer ring (A3A 280° arc + A3B circle)
                //   ANILLO 4  Clock-hand field — mirrors A2 (60 ticks, rotates CCW)
                //   ANILLO 5  Two-layer frame ring — inverse of A3
                //   [deco a]  Status dots — DISABLED (commented out)
                //   [deco b]  Progress accent — DISABLED (commented out)
                //
                // History (current state):
                //  • Original outer ring at outerR (glow + 8-segment) was
                //    removed at user request (commit de73456c).
                //  • Original inner two-piece ring at innerR (270°+90°)
                //    was removed at user request (commit f954c4d8) — the
                //    new ANILLO 2 above is its replacement.
                //  • The clock-hand field was accidentally lost when A2
                //    was rebuilt as two-layer; restored here at radius
                //    65 (between A2 and the middle ring stack).
                //
                // The on-paint order below is *outside in* so the inner
                // elements visually win when they overlap. The numbering
                // above is *inside out* for readability.
                // ═════════════════════════════════════════════════════════

                // ─── [bg] Soft center glow ────────────────────────────
                // Radial gradient cyan that fades out at outerR. Anchors
                // the orb on transparent backgrounds.
                var grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, outerR);
                grad.addColorStop(0.0, "rgba(70, 210, 255, 0.18)");
                grad.addColorStop(0.4, "rgba(40, 160, 220, 0.10)");
                grad.addColorStop(1.0, "rgba(0, 0, 0, 0.0)");
                ctx.fillStyle = grad;
                ctx.beginPath();
                ctx.arc(cx, cy, outerR, 0, Math.PI * 2);
                ctx.fill();

                // ─── ANILLO 4 + 5: outer stack ────────────────────────
                // Drawn first so the inner layers paint on top of them.
                //
                // ANILLO 4 — clock-hand field, counter-rotating mirror
                // of ANILLO 2. Same 60 ticks / line 1 / colorPrimary /
                // α=0.85, length 20 (4× A2) and `-orb.spin` so it
                // rotates CCW. The two tick fields A2 (CW) + A4 (CCW)
                // read as a coupled counter-rotating pair.
                //   • Outer edge at outerR + 11 ≈ 124.
                //   • Inner edge at outerR - 9 ≈ 104.
                ticks(outerR + 11, 60, 20, 1, ring.colorPrimary, 0.85, -orb.spin, 0);

                // ANILLO 5 — two-layer frame ring (A5A + A5B), inverse of A3.
                //   A5A — wide base, 280° arc with gap at 12 o'clock
                //         (inverse of A3A whose gap is at 6 o'clock).
                //         Sweep 40°..320° leaves an 80° gap on top.
                //         Center outerR + 34.5, width 18 → inner edge
                //         outerR + 25.5, outer edge outerR + 43.5.
                //   A5B — bright hairline, full circle. EXTERIOR edges
                //         align (inverse of A3 which has interior
                //         edges aligned). Center outerR + 42.75,
                //         width 1.5 → outer edge outerR + 43.5
                //         (matches A5A's outer edge).
                //   Both at colorPrimary, α=0.5, shadowBlur=8 on A5A.
                var a5R = outerR + 34.5;
                ctx.save();
                ctx.shadowBlur = 8;
                ctx.shadowColor = ring.colorPrimary;
                arc(a5R, 40, 320, 18, ring.colorPrimary, 0.5);   // A5A — gap up
                ctx.restore();
                circle(a5R + 8.25, 1.5, ring.colorPrimary, 0.5); // A5B — exterior aligned

                // ─── ANILLO 3: two-layer ring (A3A + A3B) ─────────────
                //   A3A — wide base, 280° arc with gap at 6 o'clock.
                //         Sweep 220°..500° (wraps past 0°). Center
                //         midR, width 16 → inner midR-8, outer midR+8.
                //   A3B — bright hairline, full circle. Interior edges
                //         align with A3A. Center midR-7.25, width 1.5
                //         → inner edge midR-8.
                //   Both at colorPrimary, α=0.3 (translúcido — A3 is
                //   the lightest of the structural rings now), with
                //   shadowBlur=8 on A3A.
                var a3R = midR;
                ctx.save();
                ctx.shadowBlur = 8;
                ctx.shadowColor = ring.colorPrimary;
                arc(a3R, 220, 500, 16, ring.colorPrimary, 0.3);    // A3A
                ctx.restore();
                circle(a3R - 7.25, 1.5, ring.colorPrimary, 0.3);   // A3B

                // ─── [deco a + deco b] DISABLED ───────────────────────
                // User: "no logro identificarlos" — the status dots and
                // the progress accent were not visually distinguishable
                // in context, so both blocks are commented out below.
                // Re-enable by uncommenting; the code is otherwise
                // intact.
                //
                // [deco b] — progress accent (amber arc, tool load):
                //
                // if (ring.toolLoad > 0.001) {
                //     var progStart = 230;
                //     var progEnd = progStart + ring.toolLoad * 120;
                //     arc(midR - 6, progStart, progEnd, 8, ring.colorAccent, 1.0);
                //     arc(midR - 6, progStart - 12, progStart + 4, 10,
                //         ring.colorAccent, 0.9);
                //     arc(midR - 6, progEnd - 4, progEnd + 12, 10,
                //         ring.colorAccent, 0.9);
                // }
                //
                // [deco a] — 4 amber status dots, top-right arc:
                //
                // for (var d = 0; d < 4; ++d) {
                //     var da = degToRad(332 + d * 11);
                //     var dx = Math.cos(da) * (midR - 2);
                //     var dy = Math.sin(da) * (midR - 2);
                //     ctx.save();
                //     ctx.beginPath();
                //     ctx.fillStyle = ring.colorDot;
                //     ctx.shadowBlur = 8;
                //     ctx.shadowColor = ring.colorDot;
                //     ctx.arc(cx + dx, cy + dy, 3.4, 0, Math.PI * 2);
                //     ctx.fill();
                //     ctx.restore();
                // }

                // ─── ANILLO 2: inner clock-hand field ─────────────────
                // 60 uniform ticks at radius 65, length 5, line 1 px,
                // rotating CW with orb.spin. Forms a coupled CW/CCW
                // pair with ANILLO 4 (the outer clock-hand field).
                ticks(65, 60, 5, 1, ring.colorPrimary, 0.85, orb.spin, 0);

                // (Original ANILLO 2 — interior two-layer ring at radius
                // 44 — was removed at user request to give the audio
                // bands more room: "apenas se ven". Its slot was the
                // band 36..48; the audio bands now expand into that
                // region with a higher bandRBase below.)

                // (The original ANILLO 2 — inner two-piece ring at
                // innerR, plus its faint helper at innerR-16 — was
                // removed at user request. The audio bands of ANILLO 1
                // now have the inner ground entirely to themselves.)

                // ─── ANILLO 1: five overlapping audio bands ───────────
                // bandRBase pulled back to 30 after the user reduced
                // the band radius again ("reducir radio de A1"); line
                // width bumped 1.2 → 2.5 ("añadir más grosor") so the
                // strokes are visible on the smaller circumference.
                // Five closed undulating curves with a small radial
                // staircase (rOffset 0/2/4/6/8) so the bands separate
                // a bit instead of fully overlapping. Each band is
                //   r(θ,t) = bandRBase + rOffset
                //          + ampMul · audioAmp · sin(n·θ + ω·t)
                // Bands differ by n (petal count) / ω (phase speed) /
                // color, simulating bass / low-mid / mid / high-mid /
                // treble. F3a uses synthetic phases; F3b will swap them
                // for the voice daemon's per-band level data.
                var bandRBase = 30;
                var bandConfigs = [
                    { color: ring.colorAccent,  ampMul: 6,  n: 4,  speed: 0.4, rOffset: 0 },  // bass
                    { color: ring.colorDeep,    ampMul: 5,  n: 6,  speed: 0.7, rOffset: 2 },  // low-mid
                    { color: ring.colorPrimary, ampMul: 6,  n: 8,  speed: 1.0, rOffset: 4 },  // mid
                    { color: ring.colorSoft,    ampMul: 4,  n: 11, speed: 1.3, rOffset: 6 },  // high-mid
                    { color: "#f7d59b",         ampMul: 3,  n: 14, speed: 1.7, rOffset: 8 }   // treble
                ];
                var t = orb.phase;
                var amp = orb.audioAmp;
                for (var c = 0; c < bandConfigs.length; c++) {
                    var cfg = bandConfigs[c];
                    ctx.save();
                    ctx.beginPath();
                    ctx.strokeStyle = cfg.color;
                    ctx.lineWidth = 2.5;
                    ctx.globalAlpha = (cfg.color === ring.colorAccent ? 0.6 : 0.7) * amp;
                    var first = true;
                    for (var bdeg = 0; bdeg <= 360; bdeg += 1) {
                        var btheta = bdeg * Math.PI / 180;
                        var br = bandRBase + cfg.rOffset
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

                // (Core decoration intentionally omitted — user removed
                // the central disc + label, leaving the audio bands
                // free to breathe in the empty center.)
            }

            Timer {
                // 60 fps — 30 fps was producing visible stutter on
                // the rotating outer / middle / clock rings. Per-frame
                // increments halved so the perceived rotation speed
                // stays constant.
                interval: 16
                running: !ring.offline
                repeat: true
                onTriggered: {
                    orb.spin = (orb.spin + 0.3) % 360;
                    orb.phase += 0.012;
                    orb.requestPaint();
                }
            }

            Component.onCompleted: requestPaint()
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
