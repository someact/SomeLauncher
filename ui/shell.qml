//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000

// SomeLauncher UI: `qs -c somelauncher` (installed) or `scripts/dev.sh ui`.
// A normal window. The work lives in somelauncherd (core/Daemon.qml connects
// and starts it), so closing this window never stops a game or a download.
// Pages load one at a time; a game page opens over them.

import QtQuick
import Quickshell
import Quickshell.Io
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
        _lastIndex = indexOf(page);
    }
    // `SOMELAUNCHER_GAME=<id>` opens that game's page once the library is in, `<id>/simple`
    // or `<id>/advanced` its settings too (screenshots, testing)
    Connections {
        target: Library
        enabled: Quickshell.env("SOMELAUNCHER_GAME") !== null && Quickshell.env("SOMELAUNCHER_GAME") !== ""
        function onLoadedChanged() {
            if (Library.loaded)
                Nav.openGame(parseInt(Quickshell.env("SOMELAUNCHER_GAME")), null);
        }
    }
    Connections {
        target: Nav
        function onPageRequested(id) {
            app.page = id;
        }
    }
    // Pages enter from the side of the rail they come from: lower ones rise, higher ones drop
    property int enterFrom: 1
    property int _lastIndex: 0
    function indexOf(id) {
        const i = pages.findIndex(p => p.id === id);
        return i < 0 ? pages.length : i;
    }
    onPageChanged: {
        const i = indexOf(page);
        enterFrom = i >= _lastIndex ? 1 : -1;
        _lastIndex = i;
        if (Config.ready)
            Config.ui.page = page;
        Nav.closeGame();
    }

    function fileFor(id) {
        return "pages/" + id.charAt(0).toUpperCase() + id.slice(1) + "Page.qml";
    }

    // When a game starts (Config.ui.onPlay): hide the window until the last
    // game ends, or quit (the daemon keeps the game). Hidden, nothing renders.
    property bool hiddenForGame: false
    Connections {
        target: Library
        function onGameStarted(id) {
            if (Config.ui.onPlay === "close") {
                Qt.quit();
            } else if (Config.ui.onPlay === "hide") {
                app.hiddenForGame = true;
                win.visible = false;
            }
        }
        function onPlayingChanged() {
            if (!Library.playing && app.hiddenForGame)
                app.show();
        }
    }
    function show() {
        hiddenForGame = false;
        win.visible = true;
    }

    // `qs -c somelauncher ipc call launcher <function>` (`qs -p ui/shell.qml ipc call …` from the repo).
    // Not `show`: `qs ipc call x show` is read as `qs ipc show`.
    IpcHandler {
        target: "launcher"
        function open(): void {
            app.show();
        }
        function toggle(): void {
            if (win.visible)
                win.visible = false;
            else
                app.show();
        }
        function hideWindow(): void {
            win.visible = false;
        }
        // home online library minecraft settings
        function page(id: string): void {
            app.page = id;
            app.show();
        }
        function game(id: int): void {
            app.show();
            Nav.openGame(id, null);
        }
        function add(): void {
            app.show();
            Nav.addGame();
        }
        function play(id: int): void {
            Library.play(id, undefined);
        }
        function state(): string {
            return JSON.stringify({
                visible: win.visible,
                page: app.page,
                game: Nav.gameId,
                daemon: Daemon.ready,
                running: Object.keys(Library.running).map(id => ({ id: Number(id), name: Library.byId[id] ? Library.byId[id].name : "", state: Library.running[id].state, since: Library.running[id].since ?? null }))
            });
        }
    }

    // gbe_fork unlocks while a game runs, over the game (its own layer surface)
    AchievementToast {}

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
                focus: true   // the page gets the keys (Library: type to search)
                onLoaded: enter.restart()
            }
            // New page: fades in while it slides the last bit from its side
            // and settles from a hair smaller; with reduced motion only the fade
            ParallelAnimation {
                id: enter
                NumberAnimation { target: pageLoader; property: "opacity"; from: 0; to: 1; duration: Motion.normal; easing.type: Easing.OutCubic }
                NumberAnimation { target: pageLoader; property: "y"; from: Motion.reduced ? 0 : 28 * app.enterFrom; to: 0; duration: Motion.slow; easing.type: Easing.OutQuint }
                NumberAnimation { target: pageLoader; property: "scale"; from: Motion.reduced ? 1 : 0.985; to: 1; duration: Motion.slow; easing.type: Easing.OutQuint }
            }

            // The game page, over whichever page opened it (core/Nav.qml)
            Loader {
                id: gameLoader
                anchors.fill: parent
                active: Nav.gameId >= 0
                source: "pages/GamePage.qml"
                onLoaded: item.forceActiveFocus()
                onActiveChanged: if (!active) pageLoader.forceActiveFocus()
            }

            // Adding a game by hand; the new game's page opens after
            AddGame {
                anchors.fill: parent
                open: Nav.adding
                onCloseRequested: Nav.adding = false
                onAdded: id => {
                    Nav.adding = false;
                    Nav.openGame(id, null);
                }
            }

            // The first start: the import dialog, once
            Loader {
                anchors.fill: parent
                active: Config.ready && !Config.ui.welcomed && Daemon.ready && Library.loaded
                sourceComponent: Welcome {
                    open: true
                    onCloseRequested: Config.ui.welcomed = true
                    onAddRequested: {
                        Config.ui.welcomed = true;
                        Nav.addGame();
                    }
                }
            }
        }
    }
}
