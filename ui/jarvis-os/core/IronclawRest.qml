pragma Singleton
import QtQuick

QtObject {
    id: rest

    readonly property string baseUrl: "http://127.0.0.1:8080"

    function getThreads(callback) {
        var req = new XMLHttpRequest();
        req.open("GET", baseUrl + "/api/threads", true);
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

    function getThread(id, callback) {
        var req = new XMLHttpRequest();
        req.open("GET", baseUrl + "/api/threads/" + encodeURIComponent(id), true);
        req.onreadystatechange = function() {
            if (req.readyState === 4) {
                if (req.status === 200) {
                    callback(null, JSON.parse(req.responseText));
                } else {
                    callback("HTTP " + req.status, null);
                }
            }
        };
        req.send();
    }
}
