import QtQuick
import QtQuick.Effects
import Quickshell
import Quickshell.Widgets
import qs.core
import qs.components

// The game page (PS5 style), shown over the current page by shell.qml while
// Nav.gameId is set: the hero art at full size, the logo, Play, settings and
// favorite, then playtime, last played, achievements (their list slides in
// like the settings) and the build. It grows
// out of the card that opened it (Nav.from) and shrinks back into it; without
// a card it fades. Escape, the back arrow or the mouse's back button close it,
// Enter plays. The settings button slides the game's settings sheet over it.
FocusScope {
    id: root

    readonly property var game: Library.byId[Nav.gameId] ?? null
    readonly property real dpr: Screen.devicePixelRatio

    // 0: the card, 1: the page. One spring drives the whole move, so closing
    // while it opens turns it around smoothly.
    property real t: 0
    Behavior on t {
        Spring { preset: "smooth"; epsilon: 0.002 }
    }
    Component.onCompleted: {
        Qt.callLater(() => t = Qt.binding(() => Nav.open ? 1 : 0));
        // Testing: SOMELAUNCHER_GAME=<id>/simple, /advanced or /mods opens the settings too
        const tab = (Quickshell.env("SOMELAUNCHER_GAME") ?? "").split("/")[1];
        if (tab) {
            settings.tab = tab;
            settings.open = true;
        }
    }
    onTChanged: if (!Nav.open && t < 0.01) Nav.closed()
    readonly property real shown: Math.max(0, Math.min(1, (t - 0.45) / 0.55))    // the controls come in late

    // Where the move starts (the card, in our coordinates) and ends
    readonly property bool fromCard: Nav.from.width > 0
    readonly property rect start: {
        const p = mapFromItem(null, Nav.from.x, Nav.from.y);
        return Qt.rect(p.x, p.y, Nav.from.width, Nav.from.height);
    }
    readonly property rect full: Qt.rect(Theme.space.m, Theme.space.m, width - Theme.space.m * 2, height - Theme.space.m * 2)
    function mix(a, b) {
        return a + (b - a) * t;
    }

    readonly property string heroSource: Library.art(game, "hero")
    readonly property string gridSource: Library.art(game, "grid")
    readonly property bool wide: width > 1050

    // A double click on a card plays: its second click lands here, on the page
    property double openedAt: Date.now()

    Keys.onEscapePressed: {
        if (settings.open)
            settings.open = false;
        else if (achievements.open)
            achievements.open = false;
        else
            Nav.closeGame();
    }
    Keys.onReturnPressed: if (!settings.open && !achievements.open) play.trigger()
    Keys.onEnterPressed: if (!settings.open && !achievements.open) play.trigger()

    // The page under it darkens and takes no clicks; a click beside the page closes it
    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.4 * Math.max(0, Math.min(1, root.t)))
    }
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
        onClicked: Nav.closeGame()
    }

    ClippingRectangle {
        id: frame
        x: root.fromCard ? root.mix(root.start.x, root.full.x) : root.full.x
        y: root.fromCard ? root.mix(root.start.y, root.full.y) : root.full.y
        width: root.fromCard ? root.mix(root.start.width, root.full.width) : root.full.width
        height: root.fromCard ? root.mix(root.start.height, root.full.height) : root.full.height
        radius: root.fromCard ? Math.max(0, root.mix(Nav.fromRadius, Theme.radius.xl)) : Theme.radius.xl
        opacity: root.fromCard ? 1 : Math.max(0, Math.min(1, root.t))
        scale: root.fromCard ? 1 : 0.96 + 0.04 * root.t
        color: Theme.surfaceHigh

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.BackButton
            onPressed: event => {
                if (event.button === Qt.BackButton)
                    Nav.closeGame();
                else if (Date.now() - root.openedAt < Qt.styleHints.mouseDoubleClickInterval)
                    play.trigger();
            }
        }

        // Without art: the theme's colors and the game's glyph
        Rectangle {
            anchors.fill: parent
            visible: root.heroSource === "" && root.gridSource === ""
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.surfaceHigh }
                GradientStop { position: 1; color: Theme.primaryContainer }
            }
            Icon {
                anchors.right: parent.right
                anchors.rightMargin: parent.width * 0.1
                anchors.verticalCenter: parent.verticalCenter
                name: Library.kindIcon(root.game ? root.game.kind : "")
                size: Math.min(parent.width, parent.height) * 0.4
                color: Qt.alpha(Theme.fgPrimaryContainer, 0.4)
            }
        }

        // Only a portrait grid: blurred to fill the page, the poster itself on the right
        Item {
            anchors.fill: parent
            visible: root.heroSource === "" && root.gridSource !== ""
            Image {
                id: gridBg
                anchors.fill: parent
                source: parent.visible ? root.gridSource : ""
                sourceSize: Qt.size(240, 360)
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
                visible: false
            }
            MultiEffect {
                anchors.fill: parent
                source: gridBg
                blurEnabled: true
                blur: 1
                blurMax: 64
                brightness: -0.15
                saturation: 0.1
            }
            Image {
                anchors.right: parent.right
                anchors.rightMargin: Theme.space.xxl * 2
                anchors.verticalCenter: parent.verticalCenter
                height: root.full.height * 0.62
                width: height * 2 / 3
                opacity: root.shown
                visible: root.wide
                source: parent.visible ? root.gridSource : ""
                sourceSize: Qt.size(Math.ceil(width * root.dpr), Math.ceil(height * root.dpr))
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
            }
        }

        // The card's own art first, so the move starts from exactly what was clicked
        Image {
            id: cardArt
            anchors.fill: parent
            visible: root.fromCard && opacity > 0
            opacity: 1 - hero.opacity
            source: root.fromCard ? (Nav.from.width > Nav.from.height ? root.heroSource || root.gridSource : root.gridSource || root.heroSource) : ""
            sourceSize: Qt.size(Math.ceil(Nav.from.width * root.dpr), Math.ceil(Nav.from.height * root.dpr))
            fillMode: Image.PreserveAspectCrop
            verticalAlignment: Image.AlignTop
            asynchronous: false
        }
        Image {
            id: hero
            anchors.fill: parent
            source: root.heroSource
            sourceSize.width: Math.ceil(root.full.width * root.dpr)
            fillMode: Image.PreserveAspectCrop
            verticalAlignment: Image.AlignTop
            asynchronous: true
            opacity: status === Image.Ready ? Math.max(0, Math.min(1, root.t * 1.6)) : 0
        }

        // Text stays readable over any art
        Rectangle {
            anchors.fill: parent
            opacity: Math.max(0, Math.min(1, root.t))
            gradient: Gradient {
                GradientStop { position: 0.3; color: "transparent" }
                GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.82) }
            }
        }
        Rectangle {
            anchors.fill: parent
            opacity: Math.max(0, Math.min(1, root.t))
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Qt.rgba(0, 0, 0, 0.55) }
                GradientStop { position: 0.65; color: "transparent" }
            }
        }

        // The controls: laid out for the full page and riding the frame's bottom
        // edge, clipped by it; they come in once the art has nearly landed
        Item {
            id: page
            anchors.bottom: parent.bottom
            width: root.full.width
            height: root.full.height
            opacity: root.shown
            transform: Translate { y: (1 - root.shown) * 28 }
            visible: opacity > 0

            HeroButton {
                x: Theme.space.xl
                y: Theme.space.xl
                icon: "arrow_back"
                onClicked: Nav.closeGame()
            }

            // How it runs, top right
            Row {
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Theme.space.xl
                spacing: Theme.space.s
                Repeater {
                    model: root.runnerChips(root.game)
                    Rectangle {
                        required property string modelData
                        width: chipText.implicitWidth + Theme.space.l * 2
                        height: 32
                        radius: 16
                        color: Theme.scrim
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

            Column {
                id: main
                x: Theme.space.xxl
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Theme.space.xxl
                width: Math.min(640, parent.width - Theme.space.xxl * 2 - (side.visible ? side.width + Theme.space.xxl : 0))
                spacing: Theme.space.l

                Item {
                    width: parent.width
                    // Sized before the logo loads, so its size never waits on its own status
                    height: logo.source != "" && logo.status !== Image.Error ? Math.min(160, root.height * 0.22) : name.implicitHeight
                    Image {
                        id: logo
                        anchors.fill: parent
                        source: Library.art(root.game, "logo")
                        // The box's largest size, never its current one (that would loop through the box's height)
                        sourceSize: Qt.size(Math.ceil(640 * root.dpr), Math.ceil(160 * root.dpr))
                        fillMode: Image.PreserveAspectFit
                        horizontalAlignment: Image.AlignLeft
                        verticalAlignment: Image.AlignBottom
                        asynchronous: true
                    }
                    SText {
                        id: name
                        width: parent.width
                        anchors.bottom: parent.bottom
                        visible: logo.source == "" || logo.status === Image.Error
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
                    visible: text !== ""
                    color: Theme.fgOnArtDim
                    text: root.byline(root.game)
                }

                Row {
                    spacing: Theme.space.m
                    GamePlayButton {
                        id: play
                        game: root.game
                    }
                    HeroButton {
                        anchors.verticalCenter: parent.verticalCenter
                        icon: "settings"
                        onClicked: root.openSettings()
                    }
                    HeroButton {
                        anchors.verticalCenter: parent.verticalCenter
                        icon: "favorite"
                        active: !!root.game && root.game.favorite
                        onClicked: if (root.game) Library.setFavorite(root.game.id, !root.game.favorite)
                    }
                }
                SText {
                    width: parent.width
                    visible: play.error !== ""
                    text: play.error
                    color: Theme.error
                    wrapMode: Text.Wrap
                    maximumLineCount: 4
                }

                Flow {
                    width: parent.width
                    spacing: Theme.space.m
                    topPadding: Theme.space.s
                    Stat {
                        label: "Playtime"
                        value: root.game && root.game.playtime ? Library.playtime(root.game.playtime) : "None yet"
                    }
                    Stat {
                        label: "Last played"
                        value: root.game ? (root.game.id in Library.running ? "Playing now" : Library.ago(root.game.lastPlayed)) : ""
                    }
                    Stat {
                        // `achievements: {unlocked, total}`; the list opens beside the page
                        readonly property var a: root.game ? root.game.achievements ?? null : null
                        visible: !!a && a.total > 0
                        label: "Achievements"
                        value: a ? `${a.unlocked} / ${a.total}` : ""
                        progress: a && a.total ? a.unlocked / a.total : -1
                        note: "See all"
                        onClicked: achievements.open = true
                    }
                    Stat {
                        readonly property var b: root.game ? root.game.build ?? {} : {}
                        readonly property string v: Library.version(root.game)
                        visible: v !== "" || !!b.latest
                        label: root.game && root.game.kind === "hoyo" ? "Version" : "Build"
                        value: v || "Unknown"
                        note: b.updateAvailable ? "Update available" : b.latest && b.latest === v ? "Up to date" : ""
                        highlight: !!b.updateAvailable
                    }
                }
            }

            // The store's description and the tags, beside the controls on a wide window
            Column {
                id: side
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: Theme.space.xxl
                width: 360
                visible: root.wide && (desc.text !== "" || tags.count > 0)
                spacing: Theme.space.l
                SText {
                    id: desc
                    width: parent.width
                    visible: text !== ""
                    text: root.game && root.game.meta && root.game.meta.description ? root.game.meta.description : ""
                    textFormat: Text.PlainText
                    color: Theme.fgOnArtDim
                    wrapMode: Text.Wrap
                    maximumLineCount: 6
                    lineHeight: 1.15
                }
                Flow {
                    width: parent.width
                    spacing: Theme.space.s
                    Repeater {
                        id: tags
                        model: root.game ? root.game.tags.slice(0, 8) : []
                        Rectangle {
                            required property string modelData
                            width: tagText.implicitWidth + Theme.space.m * 2
                            height: 26
                            radius: 13
                            color: Qt.rgba(1, 1, 1, 0.1)
                            SText {
                                id: tagText
                                anchors.centerIn: parent
                                text: parent.modelData
                                color: Theme.fgOnArt
                                font.pixelSize: Theme.font.small
                            }
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

    function openSettings() {
        settings.open = true;
    }
    GameSettings {
        id: settings
        anchors.fill: parent
        game: root.game
        visible: root.t > 0.5
        onCloseRequested: {
            open = false;
            root.forceActiveFocus();
        }
        onOpenChanged: if (open) forceActiveFocus()
    }

    AchievementsSheet {
        id: achievements
        anchors.fill: parent
        game: root.game
        visible: root.t > 0.5
        onCloseRequested: {
            open = false;
            root.forceActiveFocus();
        }
        onOpenChanged: if (open) forceActiveFocus()
    }

    // "semiwork · 2025 · Action, Early Access"
    function byline(g) {
        if (!g || !g.meta)
            return "";
        const m = g.meta;
        const parts = [];
        if (m.developers && m.developers.length)
            parts.push(m.developers[0]);
        const year = (m.release ?? "").match(/\d{4}/);
        if (year)
            parts.push(year[0]);
        if (m.genres && m.genres.length)
            parts.push(m.genres.slice(0, 3).join(", "));
        return parts.join("  ·  ");
    }
    // What runs it: "Steam", "Proton · GE-Proton10-20", "Needs Steam", …
    function runnerChips(g) {
        if (!g)
            return [];
        const r = g.runner ?? {};
        const chips = [];
        if (g.kind === "steam")
            chips.push("Steam");
        else if (g.kind === "proton" || g.kind === "hoyo")
            chips.push(r.proton ? "Proton · " + r.proton.split("/").pop() : "Proton");
        else if (g.kind === "native")
            chips.push("Linux");
        else if (g.kind === "minecraft")
            chips.push("Prism");
        if (r.needsSteam)
            chips.push("Needs Steam");
        if (g.kind === "hoyo" && r.xxmi === true)
            chips.push("XXMI");
        if (!g.installDir)
            chips.push("Not installed");
        return chips;
    }

    component HeroButton: IconButton {
        width: 56
        height: 56
        activeColor: Theme.primaryContainer
        iconColor: active ? Theme.fgPrimaryContainer : Theme.fgOnArt
        color: Qt.rgba(1, 1, 1, 0.12)
        hoverColor: Qt.rgba(1, 1, 1, 0.2)
    }

    // A glass tile: a small label over a value, an optional note and progress bar
    component Stat: Rectangle {
        id: stat
        property string label
        property string value
        property string note: ""
        property bool highlight: false
        property real progress: -1
        signal clicked
        width: Math.max(132, statCol.implicitWidth + Theme.space.l * 2)
        height: statCol.implicitHeight + Theme.space.m * 2
        radius: Theme.radius.normal
        color: Qt.rgba(0, 0, 0, 0.32)
        border.width: 1
        border.color: highlight ? Theme.primary : statMouse.containsMouse ? Qt.rgba(1, 1, 1, 0.3) : Theme.glassRim
        // Only a tile with somewhere to go takes clicks
        MouseArea {
            id: statMouse
            anchors.fill: parent
            enabled: stat.note === "See all"
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: stat.clicked()
        }
        Column {
            id: statCol
            x: Theme.space.l
            y: Theme.space.m
            spacing: 2
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
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
            SText {
                visible: stat.note !== ""
                text: stat.note
                color: stat.highlight ? Theme.primary : Theme.fgOnArtDim
                font.pixelSize: Theme.font.small
            }
            Rectangle {
                visible: stat.progress >= 0
                width: 100
                height: 4
                radius: 2
                color: Qt.rgba(1, 1, 1, 0.15)
                Rectangle {
                    width: parent.width * Math.max(0, Math.min(1, stat.progress))
                    height: parent.height
                    radius: 2
                    color: Theme.primary
                }
            }
        }
    }
}
