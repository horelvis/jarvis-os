import QtQuick

Item {
    id: bar
    height: 4

    property real progress: 0  // 0.0 – 1.0
    property color fillColor: "#5dcbff"

    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0.12, 0.26, 0.34, 0.4)
        radius: 2
    }
    Rectangle {
        height: parent.height
        width: parent.width * Math.max(0, Math.min(1, bar.progress))
        color: bar.fillColor
        radius: 2
        Behavior on width { NumberAnimation { duration: 200 } }
    }
}
