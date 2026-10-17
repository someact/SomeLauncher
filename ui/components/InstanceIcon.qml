import QtQuick
import Quickshell.Widgets
import qs.core

// A Minecraft instance's icon (its icon art, else the kind glyph). Prism's
// icons are pixel art, often 50 or 128 px: drawn larger they keep hard pixels,
// drawn smaller they are smoothed.
ClippingRectangle {
    id: root

    property var game: null
    readonly property string source: Library.art(game, "icon")

    implicitWidth: 44
    implicitHeight: 44
    radius: Theme.radius.small
    color: source !== "" && img.status === Image.Ready ? "transparent" : Theme.primaryContainer

    Image {
        id: img
        anchors.fill: parent
        source: root.source
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        // The art is at most 256 px, so its natural size is cheap to keep
        smooth: implicitWidth >= width * Screen.devicePixelRatio
        mipmap: smooth
    }
    Icon {
        anchors.centerIn: parent
        visible: root.source === "" || img.status === Image.Error
        name: "deployed_code"
        size: Math.round(root.width * 0.55)
        color: Theme.fgPrimaryContainer
    }
}
