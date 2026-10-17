import QtQuick
import qs.core

// On/off switch: a pill track with a springing knob (from SomeShell
// components/PillSwitch.qml, 2e12a96, in the launcher's glass colors).
Item {
    id: root

    property bool checked: false
    property bool enabled: true
    signal toggled(bool on)

    implicitWidth: 44
    implicitHeight: 26
    opacity: enabled ? 1 : 0.4

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: root.checked ? Theme.primary : Theme.glassRaisedHover
        border.width: root.checked ? 0 : 1
        border.color: Theme.glassRim
        Behavior on color {
            ColorAnimation { duration: Motion.fast }
        }
    }
    Rectangle {
        property real size: root.checked ? root.height - 6 : root.height - 10
        width: size
        height: size
        radius: size / 2
        anchors.verticalCenter: parent.verticalCenter
        x: root.checked ? root.width - size - 3 : 5
        color: root.checked ? Theme.fgPrimary : Theme.fgSurfaceVariant
        Behavior on x {
            Spring { preset: "snappy" }
        }
        Behavior on size {
            Spring { preset: "snappy" }
        }
    }
    MouseArea {
        anchors.fill: parent
        enabled: root.enabled
        cursorShape: Qt.PointingHandCursor
        onClicked: root.toggled(!root.checked)
    }
}
