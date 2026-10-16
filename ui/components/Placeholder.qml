import QtQuick
import qs.core

// A page that is not built yet: its icon, name and the phase that builds it.
Item {
    id: root
    property string icon
    property string title
    property string note

    Column {
        anchors.centerIn: parent
        spacing: Theme.space.m
        Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            name: root.icon
            size: 48
            color: Theme.fgSurfaceVariant
        }
        SText {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.title
            font.pixelSize: Theme.font.headline
            font.weight: Theme.font.weightTitle
        }
        SText {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.note
            dim: true
        }
    }
}
