import QtQuick
import qs.core

// A choice from a list that opens in place (no popup window): the current
// label and a chevron; a click unfolds the options under it, up to
// `maxRows` before they scroll. `options`: [{id, name, note?}]; `picked(id)`.
Column {
    id: root

    property var options: []
    property var current
    property string placeholder: "Choose"
    property int maxRows: 6
    property bool open: false
    signal picked(var id)

    readonly property var currentOption: options.find(o => o.id === current) ?? null

    width: 280
    spacing: Theme.space.xs

    PressButton {
        id: button
        width: parent.width
        height: 40
        radius: Theme.radius.small + 2
        color: Theme.glassRaised
        Rectangle {
            anchors.fill: parent
            radius: button.radius
            color: "transparent"
            border.width: 1
            border.color: root.open ? Theme.primary : Theme.glassRim
        }
        SText {
            x: Theme.space.m
            width: parent.width - Theme.space.m - 36
            anchors.verticalCenter: parent.verticalCenter
            text: root.currentOption ? root.currentOption.name : root.placeholder
            dim: !root.currentOption
        }
        Icon {
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.s
            anchors.verticalCenter: parent.verticalCenter
            name: "expand_more"
            color: Theme.fgSurfaceVariant
            rotation: root.open ? 180 : 0
            Behavior on rotation {
                Spring { preset: "snappy" }
            }
        }
        onClicked: root.open = !root.open
    }

    Rectangle {
        width: parent.width
        height: root.open ? Math.min(list.contentHeight, root.maxRows * 40) + 8 : 0
        visible: height > 0
        clip: true
        radius: Theme.radius.small + 2
        color: Theme.surfaceHigh
        border.width: 1
        border.color: Theme.glassRim
        Behavior on height {
            Spring { preset: "snappy" }
        }
        ListView {
            id: list
            anchors.fill: parent
            anchors.margins: 4
            model: root.options
            boundsBehavior: Flickable.StopAtBounds
            delegate: PressButton {
                id: option
                required property var modelData
                width: list.width
                height: 40
                radius: Theme.radius.small
                active: modelData.id === root.current
                activeColor: Theme.secondaryContainer
                Column {
                    x: Theme.space.m
                    width: parent.width - Theme.space.m * 2
                    anchors.verticalCenter: parent.verticalCenter
                    SText {
                        width: parent.width
                        text: option.modelData.name
                        color: option.active ? Theme.fgSecondaryContainer : Theme.fgSurface
                    }
                    SText {
                        width: parent.width
                        visible: !!option.modelData.note
                        text: option.modelData.note ?? ""
                        dim: true
                        font.pixelSize: Theme.font.small
                    }
                }
                onClicked: {
                    root.open = false;
                    root.picked(modelData.id);
                }
            }
        }
    }
}
