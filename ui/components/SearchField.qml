import QtQuick
import qs.core

// A pill-shaped search box: a glyph, the text, and a clear button once there
// is text. Escape clears it and gives the focus back (`escaped`).
Rectangle {
    id: root

    property alias text: input.text
    property string placeholder: "Search"
    readonly property bool active: input.activeFocus
    signal escaped

    function focusField() {
        input.forceActiveFocus();
    }

    implicitWidth: 320
    implicitHeight: 44
    radius: height / 2
    color: active ? Theme.glassRaisedHover : Theme.glassRaised
    border.width: 1
    border.color: active ? Theme.primary : Theme.glassRim
    Behavior on border.color {
        ColorAnimation { duration: Motion.fast }
    }

    Icon {
        id: glyph
        x: Theme.space.l
        anchors.verticalCenter: parent.verticalCenter
        name: "search"
        color: Theme.fgSurfaceVariant
    }

    TextInput {
        id: input
        anchors.left: glyph.right
        anchors.leftMargin: Theme.space.m
        anchors.right: clear.left
        anchors.rightMargin: Theme.space.s
        anchors.verticalCenter: parent.verticalCenter
        color: Theme.fgSurface
        selectionColor: Theme.primaryContainer
        selectedTextColor: Theme.fgPrimaryContainer
        font.family: Theme.font.ui
        font.pixelSize: Theme.font.normal
        font.weight: Theme.font.weight
        clip: true
        Keys.onEscapePressed: {
            text = "";
            focus = false;
            root.escaped();
        }

        SText {
            anchors.fill: parent
            visible: input.text === ""
            text: root.placeholder
            dim: true
        }
    }

    IconButton {
        id: clear
        anchors.right: parent.right
        anchors.rightMargin: Theme.space.xs
        anchors.verticalCenter: parent.verticalCenter
        width: 36
        height: 36
        icon: "close"
        iconSize: 18
        color: "transparent"
        visible: input.text !== ""
        onClicked: {
            input.text = "";
            input.forceActiveFocus();
        }
    }

    // A click anywhere on the pill puts the cursor in it
    MouseArea {
        anchors.fill: parent
        z: -1
        cursorShape: Qt.IBeamCursor
        onClicked: input.forceActiveFocus()
    }
}
