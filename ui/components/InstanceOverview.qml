import QtQuick
import qs.core

// A Minecraft instance's Overview tab, from `minecraft.instance` (`info`):
// its worlds (last played first, each one playable straight into it), its
// servers (joined the same way) and its newest screenshots (a click opens the
// viewer). On a wide pane the screenshots sit beside the lists.
Flickable {
    id: root

    property var game: null
    property var info: null
    signal play(var extra)                 // {world} or {server}
    signal showShot(int index)

    readonly property bool wide: width > 860
    readonly property var worlds: info ? info.worlds : []
    readonly property var servers: info ? info.servers : []
    readonly property var shots: info ? info.screenshots : []
    property bool allWorlds: false
    property bool allShots: false
    onGameChanged: {
        allWorlds = false;
        allShots = false;
        contentY = 0;
    }

    contentHeight: layout.height + Theme.space.xl
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    Grid {
        id: layout
        width: root.width
        columns: root.wide ? 2 : 1
        columnSpacing: Theme.space.xl
        rowSpacing: Theme.space.xl
        readonly property real colWidth: root.wide ? (width - columnSpacing) / 2 : width

        Column {
            width: layout.colWidth
            spacing: Theme.space.xl

            Section {
                title: "Worlds"
                count: root.worlds.length
                empty: "No worlds yet. Play to make one."
                Repeater {
                    model: root.allWorlds ? root.worlds : root.worlds.slice(0, 5)
                    Line {
                        required property var modelData
                        image: modelData.icon ? Paths.url(modelData.icon) : ""
                        glyph: "public"
                        title: modelData.name
                        detail: [modelData.hardcore ? "Hardcore" : root.mode(modelData.mode), modelData.version, Library.ago(modelData.lastPlayed)].filter(x => !!x).join("  ·  ")
                        action: "play_arrow"
                        actionTip: "Play this world"
                        onAct: root.play({ world: modelData.folder })
                    }
                }
                More {
                    total: root.worlds.length
                    shown: 5
                    expanded: root.allWorlds
                    onToggled: root.allWorlds = !root.allWorlds
                }
            }

            Section {
                title: "Servers"
                count: root.servers.length
                visible: root.servers.length > 0
                Repeater {
                    model: root.servers
                    Line {
                        required property var modelData
                        image: modelData.icon ?? ""
                        glyph: "dns"
                        title: modelData.name
                        detail: modelData.address
                        action: "login"
                        actionTip: "Join"
                        onAct: root.play({ server: modelData.address })
                    }
                }
            }
        }

        Section {
            width: layout.colWidth
            title: "Screenshots"
            count: root.shots.length
            empty: "No screenshots yet. F2 in the game takes one."
            Grid {
                id: shotGrid
                width: parent.width
                columns: width > 520 ? 3 : 2
                spacing: Theme.space.s
                readonly property real cell: (width - spacing * (columns - 1)) / columns
                Repeater {
                    model: root.allShots ? root.shots : root.shots.slice(0, shotGrid.columns * 2)
                    PressButton {
                        id: shot
                        required property var modelData
                        required property int index
                        width: shotGrid.cell
                        height: width * 9 / 16
                        radius: Theme.radius.small
                        color: Theme.glassRaised
                        Image {
                            anchors.fill: parent
                            source: Paths.url(shot.modelData.path)
                            sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            opacity: status === Image.Ready ? 1 : 0
                            Behavior on opacity {
                                NumberAnimation { duration: Motion.normal }
                            }
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: shot.radius
                            color: "transparent"
                            border.width: 1
                            border.color: shot.hovered ? Qt.rgba(1, 1, 1, 0.4) : Theme.glassRim
                        }
                        onClicked: root.showShot(index)
                    }
                }
            }
            More {
                total: root.shots.length
                shown: shotGrid.columns * 2
                expanded: root.allShots
                onToggled: root.allShots = !root.allShots
            }
        }
    }

    function mode(m) {
        return m ? m.charAt(0).toUpperCase() + m.slice(1) : "";
    }

    // A titled block: the title and count, then its rows (or a line when it has none)
    component Section: Column {
        id: section
        property string title
        property int count: 0
        property string empty: ""
        default property alias rows: body.data
        width: parent ? parent.width : 0
        spacing: Theme.space.m
        Row {
            spacing: Theme.space.s
            SText {
                id: sectionTitle
                text: section.title
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
            SText {
                anchors.baseline: sectionTitle.baseline
                text: section.count > 0 ? section.count : ""
                dim: true
            }
        }
        SText {
            visible: section.count === 0 && section.empty !== "" && root.info !== null
            text: section.empty
            dim: true
        }
        Column {
            id: body
            width: parent.width
            spacing: Theme.space.xs
        }
    }

    // A world or server: its picture, name, a detail line and one round action
    component Line: Rectangle {
        id: line
        property string image
        property string glyph
        property string title
        property string detail
        property string action
        property string actionTip
        signal act
        width: parent ? parent.width : 0
        height: 64
        radius: Theme.radius.normal
        color: hover.hovered ? Theme.glassRaisedHover : Theme.glassRaised
        HoverHandler {
            id: hover
        }
        Rectangle {
            id: pic
            x: Theme.space.s
            anchors.verticalCenter: parent.verticalCenter
            width: 48
            height: 48
            radius: Theme.radius.small
            color: Theme.surfaceHigh
            clip: true
            Image {
                id: picImg
                anchors.fill: parent
                source: line.image
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
                smooth: false         // Minecraft's 64 px icons stay crisp
            }
            Icon {
                anchors.centerIn: parent
                visible: picImg.status !== Image.Ready
                name: line.glyph
                color: Theme.fgSurfaceVariant
            }
        }
        Column {
            anchors.left: pic.right
            anchors.leftMargin: Theme.space.m
            anchors.right: go.left
            anchors.rightMargin: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            SText {
                width: parent.width
                text: line.title
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                text: line.detail
                dim: true
                font.pixelSize: Theme.font.small
            }
        }
        IconButton {
            id: go
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.s
            anchors.verticalCenter: parent.verticalCenter
            width: 40
            height: 40
            icon: line.action
            iconSize: 20
            // Only one game at a time per instance
            enabled: !!root.game && !(root.game.id in Library.running)
            color: hover.hovered ? Theme.primary : Theme.glassRaised
            iconColor: hover.hovered ? Theme.fgPrimary : Theme.fgSurface
            onClicked: line.act()
        }
    }

    // "Show all 44" / "Show fewer"
    component More: PressButton {
        id: more
        property int total
        property int shown
        property bool expanded
        signal toggled
        visible: total > shown
        width: moreText.implicitWidth + Theme.space.l * 2
        height: 32
        color: Theme.glassRaised
        SText {
            id: moreText
            anchors.centerIn: parent
            text: more.expanded ? "Show fewer" : "Show all " + more.total
        }
        onClicked: toggled()
    }
}
