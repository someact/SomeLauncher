import QtQuick
import qs.core

// A filter chip: a label (and an optional glyph), filled when `active`.
PressButton {
    id: root

    property string label
    property string icon: ""

    implicitWidth: row.implicitWidth + Theme.space.l * 2
    implicitHeight: 36
    color: Theme.glassRaised
    activeColor: Theme.secondaryContainer

    Row {
        id: row
        anchors.centerIn: parent
        spacing: Theme.space.s
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.icon !== ""
            name: root.icon
            size: 18
            fill: root.active ? 1 : 0
            color: root.active ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        }
        SText {
            anchors.verticalCenter: parent.verticalCenter
            text: root.label
            color: root.active ? Theme.fgSecondaryContainer : Theme.fgSurface
            font.weight: root.active ? Theme.font.weightTitle : Theme.font.weight
        }
    }
}
