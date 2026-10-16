//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000

// SomeLauncher UI: `qs -c somelauncher` (installed) or `scripts/dev.sh ui`.
// A normal window. The work lives in somelauncherd (core/Daemon.qml connects
// and starts it), so closing this window never stops a game or a download.
// Pages load one at a time.

import QtQuick
import Quickshell
import qs.core
import qs.components

ShellRoot {
    id: app

    readonly property var pages: [
        { id: "home", name: "Home", icon: "home" },
        { id: "online", name: "Online", icon: "public" },
        { id: "library", name: "Library", icon: "stadia_controller" },
        { id: "minecraft", name: "Minecraft", icon: "deployed_code" }
    ]
    readonly property var settingsPage: ({ id: "settings", name: "Settings", icon: "settings" })
    property string page: "home"
    Component.onCompleted: {
        const start = Quickshell.env("SOMELAUNCHER_PAGE") || Config.ui.page;
        page = [...pages, settingsPage].some(p => p.id === start) ? start : "home";
    }
    onPageChanged: if (Config.ready) Config.ui.page = page

    function fileFor(id) {
        return "pages/" + id.charAt(0).toUpperCase() + id.slice(1) + "Page.qml";
    }

    FloatingWindow {
        id: win
        title: "SomeLauncher"
        implicitWidth: 1280
        implicitHeight: 800
        minimumSize: Qt.size(900, 560)
        // Hyprland blurs what shows through; Glass tints it
        color: "transparent"
        onClosed: Qt.quit()

        Glass {
            anchors.fill: parent
            radius: 0
            border.width: 0
            highlight: false
        }

        NavRail {
            id: rail
            width: 232
            height: parent.height
            pages: app.pages
            settingsPage: app.settingsPage
            current: app.page
            onPicked: id => app.page = id
        }

        Item {
            id: content
            anchors.left: rail.right
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            clip: true

            Loader {
                id: pageLoader
                width: parent.width
                height: parent.height
                source: app.fileFor(app.page)
                asynchronous: false
                onLoaded: enter.restart()
            }
            // New page: fade in while rising a little
            ParallelAnimation {
                id: enter
                NumberAnimation { target: pageLoader; property: "opacity"; from: 0; to: 1; duration: Motion.normal; easing.type: Easing.OutCubic }
                NumberAnimation { target: pageLoader; property: "y"; from: 18; to: 0; duration: Motion.slow; easing.type: Easing.OutQuint }
            }
        }
    }
}
