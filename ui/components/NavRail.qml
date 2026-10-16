import QtQuick
import qs.core

// The left panel: the logo slot on top, the pages, Settings at the bottom with
// the daemon's state. One highlight pill slides between the items on a spring.
Item {
    id: root

    property var pages: []
    property var settingsPage
    property string current
    signal picked(string id)

    readonly property int itemHeight: 46

    Glass {
        anchors.fill: parent
        anchors.margins: Theme.space.m
        anchors.rightMargin: 0
        radius: Theme.radius.xl
    }

    // Logo slot: empty until there is a logo
    Item {
        id: logo
        x: Theme.space.xl
        y: Theme.space.xl
        width: parent.width - Theme.space.xl * 2
        height: 64
        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: Theme.space.m
            Rectangle {
                width: 40
                height: 40
                radius: Theme.radius.normal
                color: "transparent"
                border.width: 1
                border.color: Theme.outlineVariant
            }
            SText {
                anchors.verticalCenter: parent.verticalCenter
                text: "SomeLauncher"
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
        }
    }

    Item {
        id: list
        x: Theme.space.m + Theme.space.s
        anchors.top: logo.bottom
        anchors.topMargin: Theme.space.l
        anchors.bottom: bottom.top
        width: parent.width - Theme.space.m - Theme.space.s * 2

        readonly property int index: root.pages.findIndex(p => p.id === root.current)

        Rectangle {
            id: highlight
            width: list.width
            height: root.itemHeight
            radius: height / 2
            color: Theme.secondaryContainer
            opacity: list.index >= 0 ? 1 : 0
            y: Math.max(0, list.index) * (root.itemHeight + 2)
            Behavior on y {
                Spring { preset: "snappy" }
            }
            Behavior on opacity {
                NumberAnimation { duration: Motion.fast }
            }
        }

        Column {
            width: parent.width
            spacing: 2
            Repeater {
                model: root.pages
                NavItem {
                    required property var modelData
                    width: parent.width
                    height: root.itemHeight
                    page: modelData
                    current: root.current === modelData.id
                    onClicked: root.picked(modelData.id)
                }
            }
        }
    }

    Column {
        id: bottom
        x: list.x
        width: list.width
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space.xl
        spacing: Theme.space.s

        NavItem {
            width: parent.width
            height: root.itemHeight
            page: root.settingsPage
            current: root.current === root.settingsPage.id
            filled: true
            onClicked: root.picked(root.settingsPage.id)
        }

        // Daemon state: only says something when it is not fine
        SText {
            x: Theme.space.l
            width: parent.width - Theme.space.l * 2
            visible: !Daemon.ready
            text: Daemon.problem || "Connecting to somelauncherd…"
            dim: true
            font.pixelSize: Theme.font.small
            wrapMode: Text.Wrap
            elide: Text.ElideNone
        }
    }
}
