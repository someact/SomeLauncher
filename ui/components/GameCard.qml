import QtQuick
import qs.core

// A portrait game card: grid art (or its fallback), lifting on hover, with a
// ring when it is the selected one, and badges: Steam, running, update out, favorite.
Item {
    id: root

    property int gameId
    property bool selected: false
    readonly property var game: Library.byId[gameId] ?? null
    readonly property bool running: gameId in Library.running
    readonly property bool hovered: mouse.containsMouse
    readonly property Item artItem: art     // where the game page grows from

    // Fades and slides in after `enterDelay` ms when made (GameRow staggers its first cards)
    property int enterDelay: -1

    signal clicked
    signal doubleClicked

    implicitWidth: 156
    implicitHeight: 234

    Item {
        id: lift
        anchors.fill: parent
        scale: mouse.pressed ? 0.97 : root.hovered ? 1.04 : 1
        // Hovered, it rises a little too (a transform: the anchors own y)
        transform: Translate {
            id: enterShift
            y: root.hovered && !mouse.pressed ? -6 : 0
            Behavior on y {
                Spring { preset: "snappy" }
            }
        }
        Behavior on scale {
            Spring { preset: "bouncy" }
        }

        Rectangle {
            // The selection ring sits just outside the art
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
            game: root.game
            sources: ["grid", "hero"]
        }

        // Rim, brighter on hover
        Rectangle {
            anchors.fill: art
            radius: art.radius
            color: "transparent"
            border.width: 1
            border.color: root.hovered ? Qt.rgba(1, 1, 1, 0.3) : Theme.glassRim
        }

        SteamBadge {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.margins: Theme.space.s
            game: root.game
        }

        Row {
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.margins: Theme.space.s
            spacing: Theme.space.xs
            Badge {
                visible: root.running
                icon: "play_arrow"
                color: Theme.primary
                iconColor: Theme.fgPrimary
            }
            Badge {
                visible: !!root.game && root.game.build && root.game.build.updateAvailable
                icon: "download"
            }
            Badge {
                visible: !!root.game && root.game.favorite
                icon: "favorite"
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

    component Badge: Rectangle {
        property string icon
        property color iconColor: Theme.fgOnArt
        width: 26
        height: 26
        radius: 13
        color: Theme.scrim
        Icon {
            anchors.centerIn: parent
            name: parent.icon
            size: 16
            fill: 1
            color: parent.iconColor
        }
    }
}
