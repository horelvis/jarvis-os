pragma Singleton
import QtQuick
import Quickshell.Io

QtObject {
    id: bus

    // Connection state — driven by `bridge_online` / `bridge_offline`
    // synthetic events from jarvis-ui-bridge.
    property bool connected: false
    property string lastError: ""

    // Last-known state (for widget triggers)
    property var activeTools: ({})        // call_id -> { name, started_at }
    property string currentThreadId: ""
    property int turnCount: 0
    property int tokenEstimate: 0
    property var recentEvents: []          // tail bounded to 50

    // Signals dispatched to widgets
    signal toolStarted(string name, string callId)
    signal toolCompleted(string name, string callId, bool success, int durationMs)
    signal thinkingStarted(string threadId)
    signal responseEmitted(string threadId)
    signal eventLogged(var ev)

    // Bridge socket. systemd-user puts XDG_RUNTIME_DIR=/run/user/<uid>;
    // for the loopback case the UID is the user's (typically 1000 on
    // single-user Arch). The bridge service writes the socket here.
    readonly property string socketPath: "/run/user/1000/jarvis-ui-bridge.sock"

    // Pipe NDJSON from the bridge socket via `socat`. The bridge writes
    // one JSON event per line; SplitParser slices on `\n`.
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

        // Bridge synthetic events update connection state.
        if (ev.type === "bridge_online") {
            bus.connected = true;
            bus.lastError = "";
            return;
        }
        if (ev.type === "bridge_offline") {
            bus.connected = false;
            bus.lastError = "bridge offline";
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
                var t = activeTools;
                t[ev.call_id] = { name: ev.name, started_at: Date.now() };
                activeTools = t;
            }
            toolStarted(ev.name || "?", ev.call_id || "");
            break;
        case "tool_completed":
            if (ev.call_id && activeTools[ev.call_id]) {
                var t = activeTools;
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
