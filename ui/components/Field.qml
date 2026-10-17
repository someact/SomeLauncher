import QtQuick
import qs.core

// A one-line text field. It shows `value` until edited, and `committed(text)`
// fires on Enter or when the focus leaves, only if the text changed. Escape
// puts `value` back.
Rectangle {
    id: root

    property string value
    property string placeholder
    property bool mono: false
    property bool secret: false              // an API key: shown only while edited
    property alias text: input.text
    readonly property bool active: input.activeFocus
    signal committed(string text)

    function focusField() {
        input.forceActiveFocus();
    }

    implicitWidth: 260
    implicitHeight: 40
    radius: Theme.radius.small + 2
    color: active ? Theme.glassRaisedHover : Theme.glassRaised
    border.width: 1
    border.color: active ? Theme.primary : Theme.glassRim
    Behavior on border.color {
        ColorAnimation { duration: Motion.fast }
    }

    onValueChanged: if (!input.activeFocus) input.text = value
    Component.onCompleted: input.text = value

    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: Theme.space.m
        anchors.rightMargin: Theme.space.m
        verticalAlignment: TextInput.AlignVCenter
        color: Theme.fgSurface
        selectionColor: Theme.primaryContainer
        selectedTextColor: Theme.fgPrimaryContainer
        selectByMouse: true
        echoMode: root.secret ? TextInput.PasswordEchoOnEdit : TextInput.Normal
        font.family: root.mono ? Theme.font.mono : Theme.font.ui
        font.pixelSize: root.mono ? Theme.font.normal - 1 : Theme.font.normal
        font.weight: Theme.font.weight
        clip: true
        onEditingFinished: if (text !== root.value) root.committed(text)
        // Enter commits here and goes no further (the game page plays on Enter)
        Keys.onReturnPressed: focus = false
        Keys.onEnterPressed: focus = false
        Keys.onEscapePressed: event => {
            if (text !== root.value) {
                text = root.value;
                event.accepted = true;
            } else {
                event.accepted = false;
            }
            focus = false;
        }

        SText {
            anchors.fill: parent
            visible: input.text === ""
            text: root.placeholder
            dim: true
            opacity: 0.7
            font.family: input.font.family
            font.pixelSize: input.font.pixelSize
        }
    }
}
