import QtQuick
import qs.core

// A row of choices, one picked: a pill slides to the current one on a spring.
// `options`: [{id, name}]; `picked(id)` fires on a click.
Rectangle {
    id: root

    property var options: []
    property var current
    signal picked(var id)

    readonly property int index: options.findIndex(o => o.id === current)

    implicitWidth: row.implicitWidth + 8
    implicitHeight: 40
    radius: height / 2
    color: Theme.glassRaised
    border.width: 1
    border.color: Theme.glassRim

    Rectangle {
        // rep.count: itemAt is not a binding, so look again once the items exist
        readonly property Item at: rep.count > 0 && root.index >= 0 ? rep.itemAt(root.index) : null
        x: at ? row.x + at.x : 0
        y: 4
        width: at ? at.width : 0
        height: parent.height - 8
        radius: height / 2
        color: Theme.secondaryContainer
        visible: at !== null
        Behavior on x {
            Spring { preset: "snappy" }
        }
        Behavior on width {
            Spring { preset: "snappy" }
        }
    }

    Row {
        id: row
        x: 4
        height: parent.height
        Repeater {
            id: rep
            model: root.options
            Item {
                required property var modelData
                required property int index
                width: label.implicitWidth + Theme.space.l * 2
                height: row.height
                SText {
                    id: label
                    anchors.centerIn: parent
                    text: parent.modelData.name
                    color: root.index === parent.index ? Theme.fgSecondaryContainer : Theme.fgSurface
                    font.weight: root.index === parent.index ? Theme.font.weightTitle : Theme.font.weight
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.picked(parent.modelData.id)
                }
            }
        }
    }
}
