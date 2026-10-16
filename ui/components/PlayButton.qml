import QtQuick
import qs.core

// The big pill-shaped Play button of the game pages. `label` and `icon` change
// with the state (Play, Update, Installing 42 %); `progress` fills it from the left.
PressButton {
    id: root

    property string label: "Play"
    property string icon: "play_arrow"
    property real progress: 0

    implicitWidth: row.implicitWidth + 56
    implicitHeight: 56
    color: Theme.primary
    hoverColor: Qt.lighter(Theme.primary, 1.08)

    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: parent.width * root.progress
        visible: root.progress > 0
        radius: root.radius
        color: Qt.alpha(Theme.fgPrimary, 0.18)
        Behavior on width {
            Spring { preset: "smooth" }
        }
    }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 10
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            name: root.icon
            size: 28
            fill: 1
            color: Theme.fgPrimary
        }
        SText {
            anchors.verticalCenter: parent.verticalCenter
            text: root.label
            color: Theme.fgPrimary
            font.pixelSize: Theme.font.large + 2
            font.weight: Theme.font.weightTitle
        }
    }
}
