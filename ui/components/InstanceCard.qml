import QtQuick
import qs.core

// One Minecraft instance in the Minecraft page's column: its icon, name and
// `26.2 · Fabric`; filled when `selected`, a dot while it runs. A click picks
// it (`picked`), a double click plays it (`play`).
PressButton {
    id: root

    property var game: null
    property bool selected: false
    readonly property var state: game ? Library.running[game.id] ?? null : null
    signal picked
    signal play

    implicitHeight: 60
    radius: Theme.radius.normal
    color: "transparent"
    activeColor: Theme.secondaryContainer
    active: selected

    InstanceIcon {
        id: icon
        x: Theme.space.s
        anchors.verticalCenter: parent.verticalCenter
        width: 44
        height: 44
        game: root.game
    }
    Column {
        anchors.left: icon.right
        anchors.leftMargin: Theme.space.m
        anchors.right: dot.left
        anchors.rightMargin: Theme.space.s
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2
        SText {
            width: parent.width
            text: root.game ? root.game.name : ""
            color: root.selected ? Theme.fgSecondaryContainer : Theme.fgSurface
            font.weight: Theme.font.weightTitle
        }
        SText {
            width: parent.width
            text: Library.mcVersion(root.game)
            color: root.selected ? Qt.alpha(Theme.fgSecondaryContainer, 0.75) : Theme.fgSurfaceVariant
            font.pixelSize: Theme.font.small
        }
    }
    // Running (filled) or starting (a ring)
    Rectangle {
        id: dot
        anchors.right: parent.right
        anchors.rightMargin: Theme.space.m
        anchors.verticalCenter: parent.verticalCenter
        width: root.state ? 8 : 0
        height: 8
        radius: 4
        visible: !!root.state
        color: root.state && root.state.state === "running" ? Theme.primary : "transparent"
        border.width: 1.5
        border.color: Theme.primary
    }

    // The second click of a double click plays
    property double lastClick: 0
    onClicked: {
        const now = Date.now();
        if (now - lastClick < Qt.styleHints.mouseDoubleClickInterval)
            play();
        lastClick = now;
        picked();
    }
}
