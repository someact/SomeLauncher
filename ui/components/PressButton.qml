import QtQuick
import qs.core

// Rounded button with a hover tint and a springy squash on press (from
// SomeShell components/PressButton.qml, 2e12a96, without the hold-to-confirm
// fill). Put content inside; corners morph between `radius` and `activeRadius`.
Item {
    id: root

    default property alias content: inner.data
    property real radius: Math.min(width, height) / 2
    property real activeRadius: radius
    property color color: "transparent"
    property color hoverColor: Theme.glassRaisedHover
    property color activeColor: Theme.primary
    property bool active: false
    property bool enabled: true
    readonly property bool hovered: mouse.containsMouse
    readonly property bool pressed: mouse.pressed

    signal clicked
    signal rightClicked

    implicitWidth: 40
    implicitHeight: 40
    opacity: enabled ? 1 : 0.4
    // A quick click still shows the squash: it holds for a moment
    readonly property bool squashed: pressed || squashHold.running
    scale: squashed ? 0.94 : 1
    Timer {
        id: squashHold
        interval: 90
    }
    onPressedChanged: if (pressed) squashHold.restart()
    Behavior on scale {
        Spring { preset: "bouncy" }
    }

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: {
            const r = root.active ? root.activeRadius : root.radius;
            return root.squashed ? r * 0.8 : r;
        }
        Behavior on radius {
            Spring { preset: "snappy" }
        }
        color: root.active ? root.activeColor : root.hovered ? root.hoverColor : root.color
        Behavior on color {
            ColorAnimation { duration: Motion.fast }
        }
    }

    Item {
        id: inner
        anchors.fill: parent
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        enabled: root.enabled
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: event => event.button === Qt.RightButton ? root.rightClicked() : root.clicked()
    }
}
