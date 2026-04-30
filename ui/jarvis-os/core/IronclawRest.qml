pragma Singleton
import QtQuick

QtObject {
    id: rest

    readonly property string baseUrl: "http://127.0.0.1:8080"
    // Loopback-only token. Mirror in ~/.ironclaw/.env as GATEWAY_AUTH_TOKEN.
    readonly property string authToken: "jarvis-os-local-token"

    function _send(method, path, callback) {
        var req = new XMLHttpRequest();
        req.open(method, baseUrl + path, true);
        req.setRequestHeader("Authorization", "Bearer " + authToken);
        req.onreadystatechange = function() {
            if (req.readyState === 4) {
                if (req.status === 200) {
                    try {
                        callback(null, JSON.parse(req.responseText));
                    } catch (e) {
                        callback(e.toString(), null);
                    }
                } else {
                    callback("HTTP " + req.status, null);
                }
            }
        };
        req.send();
    }

    function getThreads(callback) {
        _send("GET", "/api/chat/threads", callback);
    }

    function getThread(id, callback) {
        _send("GET", "/api/chat/threads/" + encodeURIComponent(id), callback);
    }
}
