pragma Singleton
import QtQuick
import Quickshell.Io

QtObject {
    id: bus

    // Connection state — driven by the `ipc_hello` handshake the
    // IronClaw core sends once on connect over the local IPC socket.
    property bool connected: false
    property string lastError: ""

    // Last-known state (for widget triggers)
    property var activeTools: ({})        // call_id -> { name, started_at }
    property string currentThreadId: ""
    property int turnCount: 0
    property int tokenEstimate: 0
    property var recentEvents: []          // tail bounded to 50

    // Real-time TTS audio levels (driven by AppEvent::AudioLevel from
    // IronClaw's TtsAudioPipeline). The orb's audio bands subscribe to
    // these properties; the decay timer below drives them back to
    // silence when no events arrive.
    property real audioLevel: 0.0          // RMS, normalised to [0, 1]
    property var audioBands: [0.0, 0.0, 0.0, 0.0, 0.0]
    // Wall-clock millisecond of the last audio_level event. Used by
    // the decay timer to detect "no audio for a while" and fade the
    // levels back to 0 — without this, ElevenLabs going silent at the
    // end of a turn leaves the orb stuck at the last band magnitudes.
    property int _lastAudioMs: 0

    property var _audioDecayTimer: Timer {
        // 20 Hz check is plenty: audio frames arrive at ~30 Hz, so
        // detecting stale state within ~50 ms is well under the
        // human eye's perception of "stuck".
        interval: 50
        running: true
        repeat: true
        onTriggered: {
            if (bus._lastAudioMs === 0) return;
            var dt = Date.now() - bus._lastAudioMs;
            if (dt < 100) return;  // still active, leave alone
            // Multiplicative decay → reaches ~0 in ~10 ticks (500 ms)
            // after the agent goes silent. Faster than NumberAnimation
            // because we don't want a visible tail.
            bus.audioLevel *= 0.7;
            if (bus.audioLevel < 0.001) bus.audioLevel = 0;
            var decayed = [];
            for (var i = 0; i < bus.audioBands.length; i++) {
                var v = bus.audioBands[i] * 0.7;
                decayed.push(v < 0.001 ? 0 : v);
            }
            bus.audioBands = decayed;
        }
    }

    // Signals dispatched to widgets
    signal toolStarted(string name, string callId)
    signal toolCompleted(string name, string callId, bool success, int durationMs)
    signal thinkingStarted(string threadId)
    signal responseEmitted(string threadId)
    signal eventLogged(var ev)

    // Bus socket. systemd-user puts XDG_RUNTIME_DIR=/run/user/<uid>;
    // for the loopback case the UID is the user's (typically 1000 on
    // single-user Arch). The IronClaw core writes the socket here as
    // part of its startup (src/channels/local_ipc/).
    readonly property string socketPath: "/run/user/1000/ironclaw.sock"

    // Pipe NDJSON from the IronClaw local_ipc socket via `socat`. The
    // core writes one JSON event per line; SplitParser slices on `\n`.
    property var reader: Process {
        command: ["socat", "-u", "UNIX-CONNECT:" + bus.socketPath, "-"]
        running: true

        onExited: (exitCode, exitStatus) => {
            console.warn("[EventBus] socat exited code=" + exitCode + ", reconnect in 1s");
            bus.connected = false;
            reconnectTimer.start();
        }

        stdout: SplitParser {
            onRead: (line) => bus._handleLine(line)
        }
        stderr: SplitParser {
            onRead: (line) => console.warn("[EventBus] socat stderr:", line)
        }
    }

    property var reconnectTimer: Timer {
        interval: 1000
        repeat: false
        onTriggered: {
            reader.running = false;
            reader.running = true;
        }
    }

    function _handleLine(text) {
        if (!text || text.length === 0) return;
        var ev;
        try {
            ev = JSON.parse(text);
        } catch (e) {
            console.warn("[EventBus] non-JSON line:", text);
            return;
        }

        // Local IPC handshake: ipc_hello fires once on connect.
        if (ev.type === "ipc_hello") {
            bus.connected = true;
            bus.lastError = "";
            return;
        }
        if (ev.type === "error") {
            console.warn("[EventBus] ipc error:", ev.kind, ev.detail);
            return;
        }
        // High-rate audio frames bypass recentEvents — at ~30 fps they
        // would saturate the bounded tail and drown out useful events.
        // Update properties and return early so QML bindings on the
        // orb fire without growing the log.
        if (ev.type === "audio_level") {
            if (typeof ev.rms === "number") bus.audioLevel = ev.rms;
            if (Array.isArray(ev.bands)) bus.audioBands = ev.bands;
            bus._lastAudioMs = Date.now();
            return;
        }

        // Append to bounded tail.
        recentEvents.push(ev);
        if (recentEvents.length > 50) recentEvents.shift();
        recentEventsChanged();
        eventLogged(ev);

        switch (ev.type) {
        case "tool_started":
            if (ev.call_id) {
                // Object.assign to a NEW object: QML's `property var` only
                // fires *Changed on reference change, not deep mutation.
                // Reassigning the same reference left bindings like
                // Ring.agentActive (Object.keys(activeTools).length > 0)
                // and ToolsActiveWidget.visibleByActivity stale — that was
                // the orb-stays-IDLE bug.
                var t = Object.assign({}, activeTools);
                t[ev.call_id] = { name: ev.name, started_at: Date.now() };
                activeTools = t;
            }
            toolStarted(ev.name || "?", ev.call_id || "");
            break;
        case "tool_completed":
            if (ev.call_id && activeTools[ev.call_id]) {
                var t = Object.assign({}, activeTools);
                delete t[ev.call_id];
                activeTools = t;
            }
            toolCompleted(ev.name || "?", ev.call_id || "", ev.success !== false, ev.duration_ms || 0);
            break;
        case "thinking":
            if (ev.thread_id) currentThreadId = ev.thread_id;
            thinkingStarted(ev.thread_id || "");
            break;
        case "response":
            if (ev.thread_id) currentThreadId = ev.thread_id;
            turnCount += 1;
            responseEmitted(ev.thread_id || "");
            break;
        case "stream_chunk":
            tokenEstimate += 1;
            break;
        }
    }
}
