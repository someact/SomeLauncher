import QtQuick
import qs.core

// A titled, horizontally scrolling row of game cards (portrait, or landscape
// with `wide`). The list's model only changes when `ids` really does, so the
// row keeps its scroll position while its games update.
Column {
    id: root

    property string title
    property string note                     // dim text after the title
    property var ids: []
    property int selectedId: -1
    property real inset: 0                   // left padding of the cards, scrolled under
    property bool wide: false

    property var _model: []
    onIdsChanged: if (!Library.sameIds(ids, _model)) _model = ids
    Component.onCompleted: _model = ids

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

    Item {
        width: root.width
        height: (root.wide ? 200 : 234) + Theme.space.l * 2

        ListView {
            id: list
            anchors.fill: parent
            orientation: ListView.Horizontal
            spacing: Theme.space.l
            leftMargin: root.inset
            rightMargin: root.inset
            // Room for the hover lift and the selection ring
            topMargin: Theme.space.l
            bottomMargin: Theme.space.l
            boundsBehavior: Flickable.StopAtBounds
            cacheBuffer: 600
            // Not flicked itself: an interactive Flickable eats vertical wheels,
            // which belong to the page. The handler below scrolls it sideways.
            interactive: false
            model: root._model
            delegate: root.wide ? wideCard : card

            readonly property real minX: originX - leftMargin
            readonly property real maxX: Math.max(minX, originX + contentWidth + rightMargin - width)
            function page(dir) {
                pageAnim.to = Math.max(minX, Math.min(maxX, contentX + dir * (width - root.inset * 2) * 0.8));
                pageAnim.restart();
            }
            NumberAnimation {
                id: pageAnim
                target: list
                property: "contentX"
                duration: Motion.slow
                easing.type: Easing.OutCubic
            }

            // Sideways wheels and swipes scroll the row (Alt turns a wheel
            // sideways); vertical ones are not wanted here and go on to the page
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                orientation: Qt.Horizontal
                onWheel: event => {
                    pageAnim.stop();
                    list.contentX = Math.max(list.minX, Math.min(list.maxX, list.contentX - event.angleDelta.x));
                }
            }
        }

        // Arrows at the edges while the pointer is over the row, for mice
        // without a sideways wheel
        HoverHandler {
            id: hover
        }
        RowArrow {
            anchors.left: parent.left
            icon: "chevron_left"
            shown: hover.hovered && list.contentX > list.minX + 1
            onClicked: list.page(-1)
        }
        RowArrow {
            anchors.right: parent.right
            icon: "chevron_right"
            shown: hover.hovered && list.contentX < list.maxX - 1
            onClicked: list.page(1)
        }
    }

    component RowArrow: IconButton {
        property bool shown
        anchors.verticalCenter: parent.verticalCenter
        anchors.margins: Theme.space.s
        iconSize: 28
        color: Theme.surfaceHighest
        hoverColor: Theme.surfaceHigh
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.fast }
        }
    }

    Component {
        id: card
        GameCard {
            required property int modelData
            gameId: modelData
            selected: modelData === root.selectedId
            onClicked: root.picked(modelData)
            onDoubleClicked: root.activated(modelData)
        }
    }
    Component {
        id: wideCard
        WideCard {
            required property int modelData
            width: 356
            gameId: modelData
            selected: modelData === root.selectedId
            onClicked: root.picked(modelData)
            onDoubleClicked: root.activated(modelData)
        }
    }
}
