import QtQuick
import Quickshell.Widgets
import qs.core

// Home's big banner for one game: its hero art (else its grid art, cropped),
// the logo or the name, when it was last played and for how long, Play,
// favorite and the game page.
// Picking another game crossfades the art.
ClippingRectangle {
    id: root

    property var game: null
    property string label: ""               // a small heading over the name
    readonly property alias play: play

    signal details                          // the info button: open the game page

    radius: Theme.radius.xl
    color: Theme.surfaceHigh

    // Two layers: the new art fades in over the old one once it is decoded
    readonly property string artSource: Library.art(game, "hero") || Library.art(game, "grid")
    property bool _aFront: true
    onArtSourceChanged: {
        (_aFront ? artB : artA).source = artSource;
        _aFront = !_aFront;
    }
    HeroArt {
        id: artA
        front: root._aFront
        Component.onCompleted: source = root.artSource
    }
    HeroArt {
        id: artB
        front: !root._aFront
    }
    component HeroArt: Image {
        property bool front
        anchors.fill: parent
        sourceSize.width: Math.ceil(root.width * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectCrop
        verticalAlignment: Image.AlignTop
        asynchronous: true
        opacity: front && status === Image.Ready ? 1 : 0
        z: front ? 1 : 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.slow; easing.type: Easing.OutCubic }
        }
    }

    // Without art: the theme's colors and the game's icon (or kind glyph), large on the right
    Rectangle {
        anchors.fill: parent
        visible: root.artSource === ""
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Theme.surfaceHigh }
            GradientStop { position: 1; color: Theme.primaryContainer }
        }
        Item {
            readonly property string icon: Library.art(root.game, "icon")
            anchors.right: parent.right
            anchors.rightMargin: parent.width * 0.08
            anchors.verticalCenter: parent.verticalCenter
            width: parent.height * 0.5
            height: width
            Image {
                anchors.fill: parent
                visible: parent.icon !== ""
                source: parent.icon
                sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                fillMode: Image.PreserveAspectFit
                asynchronous: true
            }
            Icon {
                anchors.centerIn: parent
                visible: parent.icon === ""
                name: Library.kindIcon(root.game ? root.game.kind : "")
                size: parent.width * 0.8
                color: Qt.alpha(Theme.fgPrimaryContainer, 0.5)
            }
        }
    }

    // Text stays readable over any art: dark from the bottom and the left
    Rectangle {
        anchors.fill: parent
        z: 2
        gradient: Gradient {
            GradientStop { position: 0.35; color: "transparent" }
            GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.78) }
        }
    }
    Rectangle {
        anchors.fill: parent
        z: 2
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Qt.rgba(0, 0, 0, 0.5) }
            GradientStop { position: 0.6; color: "transparent" }
        }
    }

    Column {
        id: info
        z: 3
        x: Theme.space.xxl
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space.xxl
        width: Math.min(parent.width - Theme.space.xxl * 2, 560)
        spacing: Theme.space.l

        SText {
            text: root.label
            visible: text !== ""
            color: Theme.fgOnArtDim
            font.weight: Theme.font.weightTitle
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1.2
            font.pixelSize: Theme.font.small
        }

        Item {
            width: parent.width
            height: logo.visible ? Math.min(140, root.height * 0.3) : name.implicitHeight
            Image {
                id: logo
                anchors.fill: parent
                visible: status === Image.Ready
                source: Library.art(root.game, "logo")
                sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))
                fillMode: Image.PreserveAspectFit
                horizontalAlignment: Image.AlignLeft
                verticalAlignment: Image.AlignBottom
                asynchronous: true
            }
            SText {
                id: name
                width: parent.width
                anchors.bottom: parent.bottom
                visible: !logo.visible
                text: root.game ? root.game.name : ""
                color: Theme.fgOnArt
                wrapMode: Text.Wrap
                maximumLineCount: 2
                font.pixelSize: Theme.font.display
                font.weight: Theme.font.weightDisplay
            }
        }

        SText {
            width: parent.width
            color: Theme.fgOnArtDim
            text: {
                const g = root.game;
                if (!g)
                    return "";
                const parts = [Library.ago(g.lastPlayed)];
                if (g.playtime)
                    parts.push(Library.playtime(g.playtime) + " played");
                if (g.build && g.build.updateAvailable)
                    parts.push("Update available");
                return parts.join("  ·  ");
            }
        }

        Row {
            spacing: Theme.space.m
            GamePlayButton {
                id: play
                game: root.game
            }
            HeroButton {
                icon: "favorite"
                active: !!root.game && root.game.favorite
                onClicked: if (root.game) Library.setFavorite(root.game.id, !root.game.favorite)
            }
            HeroButton {
                icon: "info"
                onClicked: root.details()
            }
        }

        SText {
            width: parent.width
            visible: play.error !== ""
            text: play.error
            color: Theme.error
            wrapMode: Text.Wrap
            maximumLineCount: 3
        }
    }

    component HeroButton: IconButton {
        anchors.verticalCenter: parent.verticalCenter
        width: 56
        height: 56
        activeColor: Theme.primaryContainer
        iconColor: active ? Theme.fgPrimaryContainer : Theme.fgOnArt
        color: Qt.rgba(1, 1, 1, 0.12)
        hoverColor: Qt.rgba(1, 1, 1, 0.2)
    }

    // Rim
    Rectangle {
        anchors.fill: parent
        z: 4
        radius: root.radius
        color: "transparent"
        border.width: 1
        border.color: Theme.glassRim
    }
}
