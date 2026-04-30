import QtQuick

Rectangle {
    id: frame
    color: Qt.rgba(0.008, 0.047, 0.078, 0.88)  // #02060e at 88% alpha
    border.color: "#1f4256"
    border.width: 1
    radius: 4

    property string title: "panel"

    Text {
        id: titleText
        text: "▸ " + frame.title
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 10
        color: "#5dcbff"
        font.family: "monospace"
        font.pixelSize: 9
        font.letterSpacing: 1.6
        font.capitalization: Font.AllUppercase
    }

    Rectangle {
        anchors.top: titleText.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 10
        height: 1
        color: "#1f4256"
    }
}
