import QtQuick
import Quickshell.Widgets
import qs.core

// A game's art, rounded (ClippingRectangle clips in the shader: no offscreen
// layer). The first of `sources` that exists is shown, decoded at the size it
// is drawn; without any, the game's icon art or kind glyph and its name on a
// tinted panel (from SomeShell components/Cover.qml, 2e12a96).
ClippingRectangle {
    id: root

    property var game: null
    property var sources: ["grid"]          // art kinds, best first
    property bool showName: true            // the fallback names the game
    readonly property string source: {
        for (const k of sources) {
            const u = Library.art(game, k);
            if (u)
                return u;
        }
        return "";
    }
    readonly property bool ready: img.status === Image.Ready

    radius: Theme.radius.normal
    color: Theme.surfaceHigh

    Rectangle {
        anchors.fill: parent
        visible: !root.ready
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.primaryContainer }
            GradientStop { position: 1; color: Theme.surfaceHigh }
        }
    }

    Image {
        id: img
        anchors.fill: parent
        source: root.source
        sourceSize: Qt.size(Math.ceil(root.width * Screen.devicePixelRatio), Math.ceil(root.height * Screen.devicePixelRatio))
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: root.ready ? 1 : 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.normal }
        }
    }

    Column {
        anchors.centerIn: parent
        width: parent.width - Theme.space.l * 2
        spacing: Theme.space.m
        visible: root.source === "" || img.status === Image.Error
        Item {
            readonly property string icon: Library.art(root.game, "icon")
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(root.width, root.height) * 0.4
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
                size: parent.width * 0.75
                color: Theme.fgPrimaryContainer
            }
        }
        SText {
            visible: root.showName
            width: parent.width
            text: root.game ? root.game.name : ""
            color: Theme.fgPrimaryContainer
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            maximumLineCount: 3
            font.weight: Theme.font.weightTitle
        }
    }
}
