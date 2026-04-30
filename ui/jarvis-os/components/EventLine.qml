import QtQuick

Row {
    id: line
    spacing: 6
    property var ev: ({})

    Text {
        text: Qt.formatTime(new Date(), "HH:mm:ss")
        color: "#2a5670"
        font.family: "monospace"
        font.pixelSize: 9
    }
    Text {
        text: line.ev.type === "tool_started" ? "▸"
            : line.ev.type === "tool_completed" ? "✓"
            : line.ev.type === "thinking" ? "…"
            : "·"
        color: line.ev.type === "tool_completed" ? "#5dcbff" : "#f5c56a"
        font.family: "monospace"
        font.pixelSize: 9
    }
    Text {
        text: line.ev.type + (line.ev.name ? " " + line.ev.name : "")
        color: "#3aa8e8"
        font.family: "monospace"
        font.pixelSize: 9
        elide: Text.ElideRight
    }
}
