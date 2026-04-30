pragma Singleton
import QtQuick
import QtWebSockets

QtObject {
    id: bus

    // Connection state
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

    property var ws: WebSocket {
        url: "ws://127.0.0.1:3000/api/chat/ws"
        active: true
        // Qt 6 requires explicit signal-handler parameters; auto-injection
        // is deprecated since Qt 5.15.
        onStatusChanged: (status) => {
            if (status === WebSocket.Open) {
                bus.connected = true;
                bus.lastError = "";
                console.log("[EventBus] WS open");
            } else if (status === WebSocket.Closed || status === WebSocket.Error) {
                bus.connected = false;
                if (errorString) bus.lastError = errorString;
                console.warn("[EventBus] WS closed/error:", errorString);
                reconnectTimer.start();
            }
        }
        onTextMessageReceived: (message) => bus.handleEvent(message)
    }

    property var reconnectTimer: Timer {
        interval: 1000
        repeat: false
        onTriggered: {
            ws.active = false;
            ws.active = true;
            // Future: exponential backoff up to 30s; v1 = fixed 1s.
        }
    }

    function handleEvent(text) {
        var ev;
        try {
            ev = JSON.parse(text);
        } catch (e) {
            console.warn("[EventBus] non-JSON message:", text);
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
            // Heuristic: 1 chunk ≈ 1 token for the v1 estimate.
            tokenEstimate += 1;
            break;
        }
    }
}
