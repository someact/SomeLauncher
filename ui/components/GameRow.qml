import QtQuick
import qs.core

// A titled, horizontally scrolling row of game cards. `ids` should only be
// reassigned when the list really changed (Library.sameIds), so the row keeps
// its scroll position while games update.
Column {
    id: root

    property string title
    property string note                     // dim text after the title
    property var ids: []
    property int selectedId: -1
    property real inset: 0                   // left padding of the cards, scrolled under

    signal picked(int id)
    signal activated(int id)

    spacing: Theme.space.m

    Row {
        x: root.inset
        spacing: Theme.space.m
        SText {
            text: root.title
            font.pixelSize: Theme.font.title
            font.weight: Theme.font.weightTitle
        }
        SText {
            anchors.baseline: parent.children[0].baseline
            visible: root.note !== ""
            text: root.note
            dim: true
        }
    }

    ListView {
        id: list
        width: root.width
        height: 234 + Theme.space.l * 2
        orientation: ListView.Horizontal
        spacing: Theme.space.l
        leftMargin: root.inset
        rightMargin: root.inset
        // Room for the hover lift and the selection ring
        topMargin: Theme.space.l
        bottomMargin: Theme.space.l
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: 600
        model: root.ids
        delegate: GameCard {
            required property int modelData
            gameId: modelData
            selected: modelData === root.selectedId
            onClicked: root.picked(modelData)
            onDoubleClicked: root.activated(modelData)
        }

        // A vertical wheel scrolls the page, a horizontal one (or shift) the row
        WheelHandler {
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: event => {
                const dx = event.angleDelta.x !== 0 ? event.angleDelta.x : (event.modifiers & Qt.ShiftModifier ? event.angleDelta.y : 0);
                if (dx === 0) {
                    event.accepted = false;
                    return;
                }
                list.contentX = Math.max(list.originX - list.leftMargin, Math.min(list.contentX - dx, list.originX + list.contentWidth + list.rightMargin - list.width));
            }
        }
    }
}
