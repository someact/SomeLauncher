import QtQuick
import qs.core

// One setting: a title and a dim note on the left, its control on the right
// (the children). With `below`, the control goes under the text at full width.
Item {
    id: root

    property string title
    property string note
    property bool below: false
    default property alias control: slot.data

    implicitHeight: below ? text.implicitHeight + Theme.space.s + slot.childrenRect.height : Math.max(text.implicitHeight, slot.childrenRect.height)
    width: parent ? parent.width : 0

    Column {
        id: text
        width: root.below ? root.width : root.width - slot.childrenRect.width - Theme.space.l
        anchors.verticalCenter: root.below ? undefined : parent.verticalCenter
        spacing: 2
        SText {
            width: parent.width
            text: root.title
            font.weight: Theme.font.weightTitle
        }
        SText {
            width: parent.width
            visible: root.note !== ""
            text: root.note
            dim: true
            font.pixelSize: Theme.font.small
            wrapMode: Text.Wrap
            maximumLineCount: 3
        }
    }
    Item {
        id: slot
        x: root.below ? 0 : root.width - childrenRect.width
        y: root.below ? text.implicitHeight + Theme.space.s : (root.height - childrenRect.height) / 2
        width: root.below ? root.width : childrenRect.width
        height: childrenRect.height
    }
}
