import QtQuick
import qs.core

// A landscape card for a Hoyo game on Home: its art, name, installed version
// and update state. The version is Twintail's until our own Hoyo support
// (Phase 6) reads it from the game files and checks for updates.
Item {
    id: root

    property int gameId
    property bool selected: false
    readonly property var game: Library.byId[gameId] ?? null
    readonly property bool running: gameId in Library.running
    readonly property bool hovered: mouse.containsMouse
    readonly property string version: {
        if (!game)
            return "";
        if (game.build && (game.build.installed || game.build.recorded))
            return game.build.installed || game.build.recorded;
        const s = game.sources.find(s => s.data && s.data.version);
        return s ? s.data.version : "";
    }
    readonly property bool update: !!game && !!game.build && game.build.updateAvailable

    signal clicked
    signal doubleClicked

    implicitWidth: 380
    implicitHeight: 200

    Item {
        anchors.fill: parent
        scale: mouse.pressed ? 0.98 : root.hovered ? 1.02 : 1
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
                    text: root.running ? "Playing now" : root.version ? "Version " + root.version : "Version unknown"
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

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
        onDoubleClicked: root.doubleClicked()
    }
}
