import QtQuick
import Quickshell.Widgets
import qs.core
import qs.components

// Online: one HoYoverse game at a time, laid out like HoYoPlay. Its launcher
// background fills the page (animated when HoYoPlay has a loop, which plays
// only while the window is focused and nothing pauses motion); the logo top
// left (HoYoPlay draws it into its pictures; ours offline); news and events bottom left;
// version, playtime and Play (or Install) bottom right; a dock on the right
// edge switches between Genshin Impact and Zenless Zone Zero. Settings slide
// over the page as on the game page. Offline, the library's Hoyo games are
// shown with their cached art.
Item {
    id: root

    property var apiGames: []                // hoyo.games
    property string apiError: ""
    property bool apiLoaded: false
    property var content: null               // hoyo.content of the current game
    property var updatePlan: null            // hoyo.updatePlan of its library game
    property string error: ""

    // The API's games, else (offline) one per Hoyo game in the library
    readonly property var hoyoGames: {
        if (apiGames.length)
            return apiGames;
        const seen = {};
        const out = [];
        for (const g of Library.all) {
            const biz = Library.hoyoBiz(g);
            if (biz && !seen[biz]) {
                seen[biz] = true;
                out.push({ biz, name: g.name, logo: "", icon: "", backgrounds: [], main: null, preload: null });
            }
        }
        return out;
    }
    readonly property var current: hoyoGames.find(g => g.biz === Config.ui.onlineGame) ?? hoyoGames[0] ?? null
    readonly property string biz: current ? current.biz : ""

    // The library game for it: the one being installed or updated, else the last played
    readonly property var game: {
        const mine = Library.all.filter(g => Library.hoyoBiz(g) === biz && !g.hidden);
        mine.sort((a, b) => (Library.job(b.id) ? 1 : 0) - (Library.job(a.id) ? 1 : 0) || (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
        return mine[0] ?? null;
    }
    readonly property var hoyo: Library.hoyo(game)
    readonly property bool installed: !!game && !!game.installDir && !(hoyo && hoyo.installing)
    readonly property var job: game ? Library.job(game.id) : null

    function pick(biz) {
        Config.ui.onlineGame = biz;
    }

    function loadGames(force) {
        Daemon.call("hoyo.games", { force }, (r, e) => {
            root.apiLoaded = true;
            root.apiError = e ? e.message : "";
            if (r)
                root.apiGames = r;
        });
    }
    function loadContent() {
        content = null;
        const b = biz;
        if (!b)
            return;
        Daemon.call("hoyo.content", { biz: b }, r => {
            if (root.biz === b)
                root.content = r;
        });
    }
    function loadUpdatePlan() {
        updatePlan = null;
        if (!installed || !current || !current.preload)
            return;
        const id = game.id;
        Daemon.call("hoyo.updatePlan", { id }, r => {
            if (root.game && root.game.id === id)
                root.updatePlan = r;
        });
    }
    Component.onCompleted: if (Daemon.ready) loadGames(false)
    Connections {
        target: Daemon
        function onReadyChanged() {
            if (Daemon.ready)
                root.loadGames(false);
        }
    }
    onBizChanged: {
        error = "";
        loadContent();
    }
    // A job ending changes what a preload or an update needs
    readonly property string planKey: (game ? game.id : -1) + "/" + (hoyo ? hoyo.version : "") + "/" + !!job + "/" + (current && current.preload ? current.preload.tag : "")
    onPlanKeyChanged: loadUpdatePlan()

    // The background, crossfading between two layers when the game changes;
    // the loop plays only while someone can see it
    readonly property var background: current && current.backgrounds.length ? current.backgrounds[0] : null
    readonly property bool live: Config.ui.onlineVideo && !Motion.reduced && Window.active && Nav.gameId < 0 && !settings.open
    property bool _aFront: true
    onBackgroundChanged: {
        const back = _aFront ? layerB : layerA;
        back.background = background;
        _aFront = !_aFront;
    }

    ClippingRectangle {
        id: frame
        anchors.fill: parent
        anchors.margins: Theme.space.m
        radius: Theme.radius.xl
        color: Theme.surfaceHigh

        // Before anything loads, and for a game without art
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.surfaceHigh }
                GradientStop { position: 1; color: Theme.primaryContainer }
            }
        }

        // Offline, or until HoYoPlay's picture is in: the library's copy
        Image {
            anchors.fill: parent
            visible: !frame.front.ready
            source: visible ? Library.art(root.game, "hero") : ""
            sourceSize.width: Math.ceil(width * Screen.devicePixelRatio)
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
        }
        HoyoBackdrop {
            id: layerA
            anchors.fill: parent
            live: root.live && root._aFront
            opacity: root._aFront ? 1 : 0
            visible: opacity > 0
            Behavior on opacity {
                NumberAnimation { duration: Motion.slow; easing.type: Easing.OutCubic }
            }
            Component.onCompleted: background = root.background
        }
        HoyoBackdrop {
            id: layerB
            anchors.fill: parent
            live: root.live && !root._aFront
            opacity: root._aFront ? 0 : 1
            visible: opacity > 0
            Behavior on opacity {
                NumberAnimation { duration: Motion.slow; easing.type: Easing.OutCubic }
            }
        }
        readonly property HoyoBackdrop front: root._aFront ? layerA : layerB

        // Readable text: darker at the bottom, and on the left where the theme has none
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.45; color: "transparent" }
                GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.7) }
            }
        }
        Rectangle {
            anchors.fill: parent
            opacity: frame.front.hasTheme ? 0 : 1
            Behavior on opacity {
                NumberAnimation { duration: Motion.slow }
            }
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Qt.rgba(0, 0, 0, 0.5) }
                GradientStop { position: 0.45; color: "transparent" }
            }
        }

        // The logo, top left, only offline: HoYoPlay's pictures (or their theme) carry their own
        Image {
            id: logo
            x: Theme.space.xxl
            y: Theme.space.xxl
            width: Math.min(300, parent.width * 0.3)
            height: width / 2
            visible: !frame.front.ready
            source: Library.art(root.game, "logo") || (root.current ? root.current.logo : "")
            sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))
            fillMode: Image.PreserveAspectFit
            horizontalAlignment: Image.AlignLeft
            verticalAlignment: Image.AlignTop
            asynchronous: true
        }

        // Offline and nothing in the library, or the daemon is away
        Column {
            anchors.centerIn: parent
            visible: root.hoyoGames.length === 0
            spacing: Theme.space.m
            width: Math.min(parent.width - Theme.space.xxl * 2, 440)
            Icon {
                anchors.horizontalCenter: parent.horizontalCenter
                name: "public"
                size: 48
                color: Theme.fgSurfaceVariant
            }
            SText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: !Daemon.ready ? "Connecting…" : root.apiLoaded ? "HoYoverse can't be reached" : "Loading…"
                font.pixelSize: Theme.font.headline
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                visible: root.apiError !== ""
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                maximumLineCount: 3
                text: root.apiError
                dim: true
            }
            TextButton {
                anchors.horizontalCenter: parent.horizontalCenter
                visible: root.apiLoaded && Daemon.ready
                icon: "refresh"
                text: "Try again"
                onClicked: root.loadGames(true)
            }
        }

        // News and events, bottom left
        HoyoNews {
            id: news
            x: Theme.space.xl
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space.xl
            width: Math.min(400, parent.width - playBlock.width - dock.width - Theme.space.xl * 4)
            visible: !!root.content && width >= 280 && (banners.length > 0 || tabs.length > 0)
            content: root.content
        }

        // Version, playtime, Play, bottom right
        Column {
            id: playBlock
            anchors.right: dock.left
            anchors.rightMargin: Theme.space.xl
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space.xl
            width: Math.max(playRow.implicitWidth, 300)
            visible: !!root.current
            spacing: Theme.space.m

            SText {
                anchors.right: parent.right
                visible: !root.installed && !!root.current
                text: root.current ? root.current.name : ""
                color: Theme.fgOnArt
                font.pixelSize: Theme.font.title
                font.weight: Theme.font.weightTitle
            }
            SText {
                anchors.right: parent.right
                width: parent.width
                horizontalAlignment: Text.AlignRight
                color: Theme.fgOnArtDim
                text: root.statusLine()
                wrapMode: Text.Wrap
                maximumLineCount: 2
            }
            Row {
                anchors.right: parent.right
                spacing: Theme.space.s
                visible: root.installed
                Stat {
                    label: "Playtime"
                    value: root.game && root.game.playtime ? Library.playtime(root.game.playtime) : "None yet"
                }
                Stat {
                    label: "Last played"
                    value: root.game ? (root.game.id in Library.running ? "Playing now" : Library.ago(root.game.lastPlayed)) : ""
                }
            }

            Row {
                id: playRow
                anchors.right: parent.right
                spacing: Theme.space.m
                PageButton {
                    icon: "forum"
                    visible: !!root.content && !!root.content.hoyolab
                    onClicked: Qt.openUrlExternally(root.content.hoyolab)
                }
                PageButton {
                    icon: "info"
                    visible: !!root.game
                    onClicked: Nav.openGame(root.game.id, null)
                }
                PageButton {
                    icon: "settings"
                    visible: !!root.game
                    onClicked: settings.open = true
                }
                // Stops an install (kept to resume), an update or a preload
                PageButton {
                    icon: "close"
                    visible: !!root.job
                    onClicked: Daemon.call("downloads.cancel", { id: root.job.id }, undefined)
                }
                GamePlayButton {
                    id: play
                    visible: !!root.game
                    game: root.game
                }
                PlayButton {
                    visible: !root.game
                    enabled: !!root.current && !!root.current.main
                    label: "Install"
                    icon: "download"
                    onClicked: install.open = true
                }
            }

            // A preload while HoYoPlay offers one
            TextButton {
                id: preload
                readonly property var p: root.updatePlan ? root.updatePlan.preload ?? null : null
                anchors.right: parent.right
                visible: root.installed && !!p
                enabled: !!p && !p.done && !root.job
                icon: p && p.done ? "check" : "download_for_offline"
                text: !p ? "" : p.done ? "Preloaded " + p.version : "Preload " + p.version + (p.downloadSize ? " · " + Library.bytes(p.downloadSize) : "")
                onClicked: Daemon.call("hoyo.preload", { id: root.game.id }, (r, e) => root.error = e ? e.message : "")
            }

            SText {
                anchors.right: parent.right
                width: parent.width
                horizontalAlignment: Text.AlignRight
                visible: text !== ""
                text: play.error || root.error
                color: Theme.error
                wrapMode: Text.Wrap
                maximumLineCount: 4
            }
        }

        // The games, on the right edge
        Rectangle {
            id: dock
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.l
            anchors.verticalCenter: parent.verticalCenter
            width: 72
            height: dockCol.implicitHeight + Theme.space.m * 2
            radius: Theme.radius.large
            visible: root.hoyoGames.length > 0
            color: Qt.rgba(0, 0, 0, 0.38)
            border.width: 1
            border.color: Theme.glassRim

            Column {
                id: dockCol
                anchors.centerIn: parent
                spacing: Theme.space.m
                Repeater {
                    model: root.hoyoGames
                    Item {
                        id: entry
                        required property var modelData
                        readonly property bool on: modelData.biz === root.biz
                        readonly property var mine: Library.all.find(g => Library.hoyoBiz(g) === modelData.biz && !g.hidden) ?? null
                        width: 48
                        height: 48
                        scale: entryMouse.pressed ? 0.92 : on ? 1 : entryMouse.containsMouse ? 0.96 : 0.88
                        Behavior on scale {
                            Spring { preset: "snappy" }
                        }
                        ClippingRectangle {
                            anchors.fill: parent
                            radius: Theme.radius.normal
                            color: Theme.surfaceHighest
                            opacity: entry.on || entryMouse.containsMouse ? 1 : 0.6
                            Image {
                                anchors.fill: parent
                                source: entry.modelData.icon || Library.art(entry.mine, "icon")
                                sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                            }
                        }
                        Rectangle {
                            anchors.fill: parent
                            anchors.margins: -4
                            radius: Theme.radius.normal + 4
                            color: "transparent"
                            border.width: 2
                            border.color: Theme.primary
                            visible: entry.on
                        }
                        // Something runs for it: an install, an update
                        Rectangle {
                            anchors.right: parent.right
                            anchors.bottom: parent.bottom
                            anchors.margins: -3
                            width: 12
                            height: 12
                            radius: 6
                            visible: !!entry.mine && (!!Library.job(entry.mine.id) || entry.mine.id in Library.running)
                            color: Theme.primary
                            border.width: 2
                            border.color: Theme.surface
                        }
                        MouseArea {
                            id: entryMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.pick(entry.modelData.biz)
                        }
                    }
                }
            }
        }

        Rectangle {
            anchors.fill: parent
            radius: frame.radius
            color: "transparent"
            border.width: 1
            border.color: Theme.glassRim
        }
    }

    // "Version 7.1.0 · Up to date", "Version 3.2.0 · not installed", …
    function statusLine() {
        const c = current;
        if (!c)
            return "";
        const latest = c.main ? c.main.tag : "";
        if (!game)
            return latest ? "Version " + latest + " · not installed" : "Not installed";
        if (job && job.total > 0)
            return Library.bytes(job.done) + " of " + Library.bytes(job.total);
        if (hoyo && hoyo.installing)
            return "Version " + hoyo.installing.version + "  ·  install paused";
        const v = Library.version(game);
        const parts = ["Version " + (v || "unknown")];
        if (game.build && game.build.updateAvailable)
            parts.push(game.build.latest + " is out");
        else if (latest && v === latest)
            parts.push("Up to date");
        return parts.join("  ·  ");
    }

    HoyoInstall {
        id: install
        anchors.fill: parent
        hoyoGame: root.current
    }

    GameSettings {
        id: settings
        anchors.fill: parent
        game: root.game
        onCloseRequested: open = false
        onOpenChanged: if (open) forceActiveFocus()
    }

    component PageButton: IconButton {
        anchors.verticalCenter: parent.verticalCenter
        width: 56
        height: 56
        iconColor: Theme.fgOnArt
        color: Qt.rgba(0, 0, 0, 0.35)
        hoverColor: Qt.rgba(1, 1, 1, 0.2)
    }

    component Stat: Rectangle {
        id: stat
        property string label
        property string value
        width: Math.max(120, statCol.implicitWidth + Theme.space.l * 2)
        height: statCol.implicitHeight + Theme.space.s * 2
        radius: Theme.radius.normal
        color: Qt.rgba(0, 0, 0, 0.38)
        border.width: 1
        border.color: Theme.glassRim
        Column {
            id: statCol
            x: Theme.space.l
            y: Theme.space.s
            spacing: 1
            SText {
                text: stat.label
                color: Theme.fgOnArtDim
                font.pixelSize: Theme.font.small
                font.weight: Theme.font.weightTitle
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 1
            }
            SText {
                text: stat.value
                color: Theme.fgOnArt
                font.pixelSize: Theme.font.normal + 1
                font.weight: Theme.font.weightTitle
            }
        }
    }
}
