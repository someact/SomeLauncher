import QtQuick
import qs.core

// A pill with a label (and an optional glyph): `primary` fills it with the
// accent, else it is a quiet glass pill.
PressButton {
    id: root

    property string text
    property string icon: ""
    property bool primary: false

    implicitWidth: row.implicitWidth + Theme.space.xl * 2
    implicitHeight: 40
    color: primary ? Theme.primary : Theme.glassRaised
    hoverColor: primary ? Qt.lighter(Theme.primary, 1.08) : Theme.glassRaisedHover

    Row {
        id: row
        anchors.centerIn: parent
        spacing: Theme.space.s
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.icon !== ""
            name: root.icon
            size: 18
            fill: root.primary ? 1 : 0
            color: root.primary ? Theme.fgPrimary : Theme.fgSurface
        }
        SText {
            anchors.verticalCenter: parent.verticalCenter
            text: root.text
            color: root.primary ? Theme.fgPrimary : Theme.fgSurface
            font.weight: Theme.font.weightTitle
        }
    }
}
