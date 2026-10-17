import QtQuick
import QtQuick.Effects
import Quickshell.Widgets
import qs.core

// The Minecraft page's banner for one instance: a picture (`image`: a picked
// one or the newest screenshot), else `wash` (the last played world's icon)
// or the instance's icon blurred into a wash of its colors; the icon, name,
// version and loader chips, playtime and last played, and Play. The page puts
// the account menu over its top right.
ClippingRectangle {
    id: root

    property var game: null
    property string image: ""                 // a picked picture or a screenshot (URL), when there is one
    property string wash: ""                  // a small picture to blur when there is no `image`
    readonly property var prism: Library.prism(game)
    readonly property alias play: playButton
    // Too narrow for Play beside the title: it goes under it
    readonly property bool narrow: width < 620
    // The least height that fits it all under the account menu (`topInset`)
    property real topInset: 64
    readonly property real neededHeight: topInset + title.height + (narrow ? playButton.height + Theme.space.l : 0) + Theme.space.xl

    radius: Theme.radius.xl
    color: Theme.surfaceHigh

    // The icon blown up and blurred: the instance's own colors behind it
    Image {
        id: wash
        anchors.fill: parent
        source: root.image === "" ? root.wash || Library.art(root.game, "icon") : ""
        sourceSize: Qt.size(64, 64)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        visible: false
    }
    MultiEffect {
        anchors.fill: parent
        visible: root.image === "" && wash.status === Image.Ready
        source: wash
        blurEnabled: true
        blur: 1
        blurMax: 64
        saturation: 0.2
        brightness: -0.1
    }
    Rectangle {
        anchors.fill: parent
        visible: root.image === "" && wash.status !== Image.Ready
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Theme.surfaceHigh }
            GradientStop { position: 1; color: Theme.primaryContainer }
        }
    }
    Image {
        id: picture
        anchors.fill: parent
        source: root.image
        sourceSize.width: Math.ceil(width * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: status === Image.Ready ? 1 : 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.normal }
        }
    }
    // Text stays readable over any picture
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.25; color: "transparent" }
            GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.78) }
        }
    }
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Qt.rgba(0, 0, 0, 0.45) }
            GradientStop { position: 0.6; color: "transparent" }
        }
    }

    Row {
        id: title
        anchors.left: parent.left
        anchors.right: root.narrow ? parent.right : playButton.left
        anchors.bottom: root.narrow ? playButton.top : parent.bottom
        anchors.margins: Theme.space.xl
        anchors.rightMargin: root.narrow ? Theme.space.xl : Theme.space.l
        anchors.bottomMargin: root.narrow ? Theme.space.l : Theme.space.xl
        spacing: Theme.space.l

        InstanceIcon {
            anchors.bottom: parent.bottom
            width: 76
            height: 76
            radius: Theme.radius.normal
            game: root.game
        }
        Column {
            anchors.bottom: parent.bottom
            width: parent.width - 76 - parent.spacing
            spacing: Theme.space.s
            SText {
                width: parent.width
                text: root.game ? root.game.name : ""
                color: Theme.fgOnArt
                wrapMode: Text.Wrap
                maximumLineCount: 2
                font.pixelSize: root.narrow ? Theme.font.title + 2 : Theme.font.headline
                font.weight: Theme.font.weightDisplay
            }
            Flow {
                width: parent.width
                spacing: Theme.space.s
                Repeater {
                    model: root.chips()
                    Rectangle {
                        required property string modelData
                        width: chipText.implicitWidth + Theme.space.m * 2
                        height: 26
                        radius: 13
                        color: Qt.rgba(1, 1, 1, 0.12)
                        border.width: 1
                        border.color: Theme.glassRim
                        SText {
                            id: chipText
                            anchors.centerIn: parent
                            text: parent.modelData
                            color: Theme.fgOnArt
                            font.pixelSize: Theme.font.small
                            font.weight: Theme.font.weightTitle
                        }
                    }
                }
            }
            SText {
                width: parent.width
                color: Theme.fgOnArtDim
                text: {
                    const g = root.game;
                    if (!g)
                        return "";
                    const when = g.id in Library.running ? "Playing now" : Library.ago(g.lastPlayed);
                    return g.playtime ? Library.playtime(g.playtime) + " played  ·  " + when : when;
                }
            }
        }
    }

    GamePlayButton {
        id: playButton
        x: root.narrow ? Theme.space.xl : parent.width - width - Theme.space.xl
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space.xl
        game: root.game
    }

    // Version, loader with its version, the modpack, the group
    function chips() {
        const p = prism;
        if (!p)
            return [];
        const c = [];
        if (p.minecraft)
            c.push(p.minecraft);
        c.push(p.loader ? p.loader + (p.loaderVersion ? " " + p.loaderVersion : "") : "Vanilla");
        if (p.pack && p.pack !== (game ? game.name : ""))
            c.push(p.pack);
        return c;
    }
}
