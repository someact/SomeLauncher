import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Widgets
import qs.core

// The unlock popup (7d), PS5 style: a small dark card at the top of the
// focused monitor, over the game, for a few seconds, one unlock after the
// other. Only for gbe_fork/Goldberg games while they run (Steam shows its
// own). A layer-shell surface on the overlay layer, so it shows over windowed
// and fullscreen games, also while the launcher's window is hidden; not inside
// gamescope, which draws its own screen. It takes no clicks and no keys, and
// exists only while it shows.
Scope {
    id: root

    property var queue: []
    property var current: null
    readonly property int showFor: 6000

    Connections {
        target: Daemon
        function onEvent(name, data) {
            if (name !== "achievement.unlocked" || data.source !== "gbe")
                return;
            // Unlocks found after the game ended (the last read of its save) are old news
            if (!(data.id in Library.running))
                return;
            root.queue = root.queue.concat([data]);
            if (!root.current)
                root.next();
        }
    }
    function next() {
        current = queue.length ? queue[0] : null;
        queue = queue.slice(1);
        if (current)
            hold.restart();
    }
    // Shows, then leaves (the card slides out before the next comes)
    Timer {
        id: hold
        interval: root.showFor
        onTriggered: leave.restart()
    }
    Timer {
        id: leave
        interval: Motion.normal
        onTriggered: root.next()
    }

    LazyLoader {
        active: root.current !== null

        PanelWindow {
            id: win

            readonly property var a: root.current ? root.current.achievement : null

            screen: Quickshell.screens.find(s => s.name === Hyprland.focusedMonitor?.name) ?? Quickshell.screens[0]
            WlrLayershell.namespace: "somelauncher:achievement"
            WlrLayershell.layer: WlrLayer.Overlay
            exclusionMode: ExclusionMode.Ignore
            anchors.top: true
            margins.top: 28
            implicitWidth: 420
            implicitHeight: card.height + 8
            color: "transparent"
            // Clicks go through to the game
            mask: Region {}

            Rectangle {
                id: card
                width: parent.width
                height: 76
                radius: Theme.radius.large
                color: Qt.rgba(0.06, 0.06, 0.08, 0.92)
                border.width: 1
                border.color: Qt.rgba(1, 1, 1, 0.1)

                // In from above, out the same way when its time is up
                readonly property bool leaving: leave.running
                property real shown: 0
                Component.onCompleted: shown = 1
                onLeavingChanged: shown = leaving ? 0 : 1
                opacity: shown
                transform: Translate {
                    y: Motion.reduced ? 0 : (1 - card.shown) * -16
                }
                Behavior on shown {
                    NumberAnimation { duration: Motion.normal; easing.type: Easing.OutCubic }
                }

                ClippingRectangle {
                    id: pic
                    x: 14
                    anchors.verticalCenter: parent.verticalCenter
                    width: 52
                    height: 52
                    radius: Theme.radius.small
                    color: Qt.rgba(1, 1, 1, 0.08)
                    Image {
                        anchors.fill: parent
                        source: win.a ? Library.media(win.a.icon) : ""
                        sourceSize: Qt.size(104, 104)
                        fillMode: Image.PreserveAspectCrop
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: !win.a || !win.a.icon
                        name: "trophy"
                        size: 28
                        fill: 1
                        color: Theme.fgOnArt
                    }
                }
                Column {
                    anchors.left: pic.right
                    anchors.leftMargin: 14
                    anchors.right: parent.right
                    anchors.rightMargin: 16
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Row {
                        spacing: 6
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "trophy"
                            size: 14
                            fill: 1
                            color: Theme.primary
                        }
                        SText {
                            anchors.verticalCenter: parent.verticalCenter
                            text: "Achievement unlocked"
                            color: Theme.primary
                            font.pixelSize: Theme.font.small
                            font.weight: Theme.font.weightTitle
                        }
                    }
                    SText {
                        width: parent.width
                        text: win.a ? win.a.name : ""
                        color: Theme.fgOnArt
                        font.pixelSize: Theme.font.large
                        font.weight: Theme.font.weightTitle
                    }
                    SText {
                        width: parent.width
                        text: root.current ? root.current.game : ""
                        color: Theme.fgOnArtDim
                        font.pixelSize: Theme.font.small
                    }
                }
            }
        }
    }
}
