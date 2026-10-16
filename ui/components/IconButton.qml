import QtQuick
import qs.core

// A round button with one glyph (from SomeShell components/IconButton.qml,
// 2e12a96). Round when off, a squircle when `active`.
PressButton {
    id: root
    property string icon
    property real iconSize: 22
    property color iconColor: active ? Theme.fgPrimary : Theme.fgSurface

    implicitWidth: 48
    implicitHeight: 48
    color: Theme.glassRaised
    activeRadius: Math.round(Math.min(width, height) * 0.32)

    Icon {
        anchors.centerIn: parent
        name: root.icon
        size: root.iconSize
        fill: root.active ? 1 : 0
        color: root.iconColor
    }
}
