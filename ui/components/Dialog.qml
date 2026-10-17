import QtQuick
import qs.core

// A modal card over the content: the rest dims and takes no clicks, the card
// springs in from slightly small. Put the content inside; `closeRequested` on
// Escape or a click beside it (unless `modal`). Show it with `open`; it hides
// itself once faded out.
FocusScope {
    id: root

    property bool open: false
    property bool modal: false               // true: only the dialog's own buttons close it
    property real cardWidth: 640
    property real cardHeight: Math.min(parent ? parent.height - Theme.space.xxl * 2 : 600, 640)
    default property alias content: inner.data
    signal closeRequested

    visible: scrim.opacity > 0
    onOpenChanged: if (open) forceActiveFocus()
    Keys.onEscapePressed: if (!modal) closeRequested()

    Rectangle {
        id: scrim
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.45)
        opacity: root.open ? 1 : 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.normal }
        }
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onClicked: if (!root.modal) root.closeRequested()
        }
    }

    Rectangle {
        id: card
        anchors.centerIn: parent
        width: Math.min(root.cardWidth, root.width - Theme.space.xl * 2)
        height: Math.min(root.cardHeight, root.height - Theme.space.l * 2)
        radius: Theme.radius.xl
        color: Qt.rgba(Theme.surface.r, Theme.surface.g, Theme.surface.b, 0.96)
        border.width: 1
        border.color: Theme.glassRim
        opacity: scrim.opacity
        scale: root.open ? 1 : 0.94
        Behavior on scale {
            Spring { preset: "snappy" }
        }
        MouseArea {
            anchors.fill: parent    // clicks on the card stay on it
        }
        Item {
            id: inner
            anchors.fill: parent
            anchors.margins: Theme.space.xl
        }
    }
}
