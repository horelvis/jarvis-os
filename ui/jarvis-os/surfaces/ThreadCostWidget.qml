import QtQuick
import Quickshell
import Quickshell.Wayland
import "../components"
import "../core"

PanelWindow {
    id: widget

    anchors { top: true; left: true }
    margins { top: 32; left: 32 }

    width: 240
    height: 130

    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.exclusiveZone: 0
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // Smart visibility: appear when a thread is active, unless force-hidden.
    visible: !Hotkeys.overrideHideAll && (visibleByActivity || forceShown)
    property bool visibleByActivity: EventBus.currentThreadId !== ""
    property bool forceShown: Hotkeys.overrideShowAll

    PanelFrame {
        anchors.fill: parent
        title: EventBus.currentThreadId.length >= 6
            ? "thread #" + EventBus.currentThreadId.substr(0, 6)
            : "thread"
        opacity: widget.visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }

        Column {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.topMargin: 32
            anchors.margins: 12
            spacing: 6

            Row { width: parent.width
                Text { text: "turn"; color: "#3aa8e8"; font.family: "monospace"; font.pixelSize: 10; width: 60 }
                Text { text: EventBus.turnCount; color: "#9fe6ff"; font.family: "monospace"; font.pixelSize: 10 }
            }
            Row { width: parent.width
                Text { text: "tokens"; color: "#3aa8e8"; font.family: "monospace"; font.pixelSize: 10; width: 60 }
                Text { text: "~" + EventBus.tokenEstimate; color: "#9fe6ff"; font.family: "monospace"; font.pixelSize: 10 }
            }
            Row { width: parent.width
                Text { text: "cost"; color: "#3aa8e8"; font.family: "monospace"; font.pixelSize: 10; width: 60 }
                // Spec caveat: cost($) not in AppEvent. v1 placeholder; REST poll v1.x.
                Text { text: "TBD via REST"; color: "#f5c56a"; font.family: "monospace"; font.pixelSize: 10 }
            }
        }
    }
}
