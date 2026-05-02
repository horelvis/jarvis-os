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
                //   ANILLO 2  Two-layer ring (A2A 220° arc + A2B circle)
                //   ANILLO 3  Clock-hand field (60 uniform ticks, rotates)
                //   ANILLO 4  Two-layer ring (A4A 220° arc + A4B circle)
                //   ANILLO 5  Clock-hand field — mirrors A3 (60 ticks, rotates)
                //   ANILLO 6  Outermost frame ring (thick uniform)
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

                // ─── ANILLO 5 + 6: outer stack ────────────────────────
                // Drawn first so the inner layers paint on top of them.
                //
                // ANILLO 5 — clock-hand field, counter-rotating mirror
                // of ANILLO 3. Same 60 ticks / line 1 / colorPrimary /
                // α=0.85, but length 20 (4× A3) and `-orb.spin` so it
                // rotates in the opposite direction. The two tick
                // fields read as a coupled pair: A3 inside spinning CW,
                // A5 outside spinning CCW.
                //   • Outer edge at outerR + 11 ≈ 124.
                //   • Inner edge at outerR - 9 ≈ 104.
                ticks(outerR + 11, 60, 20, 1, ring.colorPrimary, 0.85, -orb.spin, 0);

                // ANILLO 6 — outermost frame ring.
                //   • Width 18 px. Center at outerR + 34.5 so the
                //     inner edge sits at outerR + 25.5 (well clear of
                //     A4 ticks at outerR + 11).
                //   • Alpha 0.55 — perceptual match to ANILLO 2 on a
                //     thick stroke + glow.
                //   • shadowBlur=12. Static.
                glowCircle(outerR + 34.5, 18, ring.colorPrimary, 0.55, 12);

                // ─── ANILLO 4: two-layer ring (A4A + A4B), same A2 pattern.
                //   A4A — wide base, 220° arc only (gap centered at
                //         12 o'clock). Center midR, width 8 → outer
                //         edge midR + 4.
                //   A4B — bright hairline, full circle. Center
                //         midR + 3.25, width 1.5 → outer edge midR + 4
                //         (matches A4A's outer edge).
                //   Both at colorPrimary, alpha 0.85, shadowBlur=8 on
                //   A4A only.
                //
                // (The previous middle stack — segmentedRing 20 segs,
                // helper circle, thick 270° arc — was replaced at user
                // request to unify A4 with the A2 design language.)
                var a4R = midR;
                ctx.save();
                ctx.shadowBlur = 8;
                ctx.shadowColor = ring.colorPrimary;
                arc(a4R, 70, 290, 8, ring.colorPrimary, 0.85);   // A4A
                ctx.restore();
                circle(a4R + 3.25, 1.5, ring.colorPrimary, 0.85); // A4B

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

                // ─── ANILLO 3: clock-hand field (restored) ────────────
                // 60 uniform ticks at radius 65, length 5, line 1 px,
                // rotating with orb.spin. Was lost when the original
                // ANILLO 2 (clock-hand at innerR+18) was rebuilt as a
                // two-layer ring; restored here at radius 65 so it sits
                // in the empty band between the new A2 (radius 44) and
                // the middle ring stack (midR ≈ 89).
                ticks(65, 60, 5, 1, ring.colorPrimary, 0.85, orb.spin, 0);

                // ─── ANILLO 2: two-layer ring (A2A + A2B) ─────────────
                // Two strokes that share color and alpha but differ in
                // width and radius so their *outer edges align* — the
                // layered-strokes design rule (see
                // feedback_layered_strokes_for_thickness memory).
                //
                //   A2A — wide base, **220° arc only**.
                //         Center a2R, width 8 → outer edge a2R + 4.
                //         Gap centered at 12 o'clock (70°..290°).
                //   A2B — hairline, full circle.
                //         Center a2R + 3.25, width 1.5 → outer edge
                //         a2R + 4 (matches A2A's outer edge).
                //
                // Both at colorPrimary, alpha 0.85. shadowBlur=8 on
                // A2A only (the base carries the halo; A2B reads as a
                // hard highlight on top).
                //
                // a2R = 44 (was innerR + 18 = 88 — halved to clear
                // ANILLO 4's middle ring at midR ≈ 89).
                var a2R = (innerR + 18) / 2;
                ctx.save();
                ctx.shadowBlur = 8;
                ctx.shadowColor = ring.colorPrimary;
                arc(a2R, 70, 290, 8, ring.colorPrimary, 0.85);   // A2A
                ctx.restore();
                circle(a2R + 3.25, 1.5, ring.colorPrimary, 0.85); // A2B

                // (The original ANILLO 2 — inner two-piece ring at
                // innerR, plus its faint helper at innerR-16 — was
                // removed at user request. The audio bands of ANILLO 1
                // now have the inner ground entirely to themselves.)

                // ─── ANILLO 1: five overlapping audio bands ───────────
                // The innermost layer. Five closed undulating curves
                // at the SAME radius (no staircase): each band is
                //   r(θ,t) = bandRBase + ampMul · audioAmp · sin(n·θ + ω·t)
                // so they overlap continuously, never drawing the same
                // shape. Bands differ by n (petal count) / ω (phase
                // speed) / color, simulating bass / low-mid / mid /
                // high-mid / treble. F3a uses synthetic phases; F3b
                // will swap them for the voice daemon's per-band level
                // data so the bands react to actual audio.
                var bandRBase = (coreR + 8) / 2;
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
