import QtQuick
import qs.core

// A landscape game card: hero art (else grid, cropped), the name and a
// `subtitle` (Home: a Hoyo game's version), "Playing now" while it runs, an
// update chip and the Steam badge.
Item {
    id: root

    property int gameId
    property bool selected: false
    property string subtitle: game ? Library.ago(game.lastPlayed) : ""
    readonly property var game: Library.byId[gameId] ?? null
    readonly property bool running: gameId in Library.running
    readonly property bool hovered: mouse.containsMouse
    readonly property Item artItem: art     // where the game page grows from
    readonly property bool update: !!game && !!game.build && game.build.updateAvailable

    // Fades and slides in after `enterDelay` ms when made (GameRow staggers its first cards)
    property int enterDelay: -1

    signal clicked
    signal doubleClicked

    implicitWidth: 380
    implicitHeight: 200

    Item {
        id: lift
        anchors.fill: parent
        scale: mouse.pressed ? 0.98 : root.hovered ? 1.02 : 1
        // Hovered, it rises a little too (a transform: the anchors own y)
        transform: Translate {
            id: enterShift
            y: root.hovered && !mouse.pressed ? -4 : 0
            Behavior on y {
                Spring { preset: "snappy" }
            }
        }
        Behavior on scale {
            Spring { preset: "bouncy" }
        }

        Rectangle {
            anchors.fill: art
            anchors.margins: -4
            radius: art.radius + 4
            color: "transparent"
            border.width: 2
            border.color: Theme.primary
            opacity: root.selected ? 1 : 0
            Behavior on opacity {
                NumberAnimation { duration: Motion.fast }
            }
        }

        Art {
            id: art
            anchors.fill: parent
            radius: Theme.radius.large
            game: root.game
            sources: ["hero", "grid"]
            showName: false
            fallbackInset: 64
        }
        Rectangle {
            anchors.fill: art
            radius: art.radius
            gradient: Gradient {
                GradientStop { position: 0.4; color: "transparent" }
                GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.75) }
            }
            border.width: 1
            border.color: root.hovered ? Qt.rgba(1, 1, 1, 0.3) : Theme.glassRim
        }

        SteamBadge {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.margins: Theme.space.m
            game: root.game
        }

        Column {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: Theme.space.l
            spacing: Theme.space.xs
            SText {
                width: parent.width
                text: root.game ? root.game.name : ""
                color: Theme.fgOnArt
                font.pixelSize: Theme.font.title
                font.weight: Theme.font.weightTitle
            }
            Row {
                spacing: Theme.space.s
                SText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.running ? "Playing now" : root.subtitle
                    color: Theme.fgOnArtDim
                }
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.update
                    width: chip.implicitWidth + Theme.space.m * 2
                    height: 24
                    radius: 12
                    color: Theme.primary
                    SText {
                        id: chip
                        anchors.centerIn: parent
                        text: "Update available"
                        color: Theme.fgPrimary
                        font.pixelSize: Theme.font.small
                        font.weight: Theme.font.weightTitle
                    }
                }
            }
        }
    }

    Component.onCompleted: if (enterDelay >= 0 && !Motion.reduced) enter.start()
    SequentialAnimation {
        id: enter
        PropertyAction { target: lift; property: "opacity"; value: 0 }
        PauseAnimation { duration: Math.max(0, root.enterDelay) }
        ParallelAnimation {
            NumberAnimation { target: lift; property: "opacity"; to: 1; duration: Motion.normal; easing.type: Easing.OutCubic }
            NumberAnimation { target: enterShift; property: "x"; from: 28; to: 0; duration: Motion.slow; easing.type: Easing.OutQuint }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
        onDoubleClicked: root.doubleClicked()
    }
}
