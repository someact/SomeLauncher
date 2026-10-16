import QtQuick
import qs.core

// One row of the nav rail. The rail draws the sliding highlight; an item draws
// its own only when `filled` (Settings, outside the sliding list).
PressButton {
    id: root

    property var page
    property bool current: false
    property bool filled: false

    color: filled && current ? Theme.secondaryContainer : "transparent"
    hoverColor: current ? (filled ? Theme.secondaryContainer : "transparent") : Theme.glassRaisedHover

    Row {
        anchors.verticalCenter: parent.verticalCenter
        x: 18
        spacing: 14
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            name: root.page.icon
            size: 22
            fill: root.current ? 1 : 0
            color: root.current ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        }
        SText {
            anchors.verticalCenter: parent.verticalCenter
            text: root.page.name
            color: root.current ? Theme.fgSecondaryContainer : Theme.fgSurface
            font.weight: root.current ? Theme.font.weightTitle : Theme.font.weight
        }
    }
}
