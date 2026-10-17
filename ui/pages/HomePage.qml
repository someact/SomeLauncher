import QtQuick
import qs.core
import qs.components

// Home: a banner for one game (the last played until another card is picked),
// "Continue playing" (games played in the last `Config.home.recentDays` days,
// newest first), favorites, and the Hoyo games with their installed version.
// Games that drop off Home stay in Library. A click picks a card for the
// banner, a double click plays it; the banner's info button opens the game page.
Item {
    id: root

    property var recentIds: []
    property var favoriteIds: []
    property var hoyoIds: []
    property int selectedId: -1
    readonly property var selected: Library.byId[selectedId] ?? Library.byId[recentIds[0] ?? favoriteIds[0] ?? hoyoIds[0]] ?? Library.games[0] ?? null

    function rebuild() {
        const days = Config.home.recentDays;
        const since = Library.now - days * 86400;
        const byRecent = (a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0);
        const pick = (filter, list) => {
            const ids = Library.games.filter(filter).sort(byRecent).map(g => g.id);
            return Library.sameIds(ids, list) ? list : ids;
        };
        recentIds = pick(g => g.lastPlayed && (days <= 0 || g.lastPlayed >= since), recentIds).slice(0, 24);
        favoriteIds = pick(g => g.favorite, favoriteIds);
        hoyoIds = pick(g => g.kind === "hoyo", hoyoIds);
    }
    Component.onCompleted: rebuild()
    Connections {
        target: Library
        function onByIdChanged() {
            root.rebuild();
        }
    }
    Connections {
        target: Config.home
        function onRecentDaysChanged() {
            root.rebuild();
        }
    }

    function hoyoVersion(g) {
        const v = Library.version(g);
        return v ? "Version " + v : "Version unknown";
    }

    function play(id) {
        selectedId = id;
        if (!(id in Library.running))
            hero.play.trigger();
    }

    Flickable {
        id: flick
        anchors.fill: parent
        visible: Library.games.length > 0
        contentHeight: col.implicitHeight + Theme.space.xxl
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: col
            width: flick.width
            spacing: Theme.space.xl
            topPadding: Theme.space.m

            HomeHero {
                id: hero
                x: Theme.space.m
                width: parent.width - Theme.space.m * 2
                height: Math.max(360, root.height * 0.58)
                game: root.selected
                label: root.selected && root.selected.id === root.recentIds[0] ? "Continue playing" : ""
                onDetails: if (root.selected) Nav.openGame(root.selected.id, hero)
            }

            GameRow {
                width: parent.width
                inset: Theme.space.xl
                visible: root.recentIds.length > 0
                title: "Continue playing"
                ids: root.recentIds
                selectedId: root.selected ? root.selected.id : -1
                onPicked: id => root.selectedId = id
                onActivated: id => root.play(id)
            }
            SText {
                x: Theme.space.xl
                visible: root.recentIds.length === 0 && Config.home.recentDays > 0
                text: `Nothing played in the last ${Config.home.recentDays} days. Every game is in Library.`
                dim: true
            }

            GameRow {
                width: parent.width
                inset: Theme.space.xl
                visible: root.favoriteIds.length > 0
                title: "Favorites"
                ids: root.favoriteIds
                selectedId: root.selected ? root.selected.id : -1
                onPicked: id => root.selectedId = id
                onActivated: id => root.play(id)
            }

            Column {
                x: Theme.space.xl
                width: parent.width - Theme.space.xl * 2
                visible: root.hoyoIds.length > 0
                spacing: Theme.space.m
                SText {
                    text: "HoYoverse"
                    font.pixelSize: Theme.font.title
                    font.weight: Theme.font.weightTitle
                }
                Flow {
                    width: parent.width
                    spacing: Theme.space.l
                    Repeater {
                        model: root.hoyoIds
                        WideCard {
                            required property int modelData
                            gameId: modelData
                            subtitle: root.hoyoVersion(game)
                            selected: !!root.selected && root.selected.id === modelData
                            onClicked: root.selectedId = modelData
                            onDoubleClicked: root.play(modelData)
                        }
                    }
                }
            }
        }
    }

    // No games yet, or no daemon
    Column {
        anchors.centerIn: parent
        visible: Library.games.length === 0
        spacing: Theme.space.l
        width: Math.min(parent.width - Theme.space.xxl * 2, 460)

        Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            name: "home"
            size: 48
            color: Theme.fgSurfaceVariant
        }
        SText {
            anchors.horizontalCenter: parent.horizontalCenter
            text: !Daemon.ready ? "Connecting…" : Library.loaded ? "No games yet" : "Loading…"
            font.pixelSize: Theme.font.headline
            font.weight: Theme.font.weightTitle
        }
        SText {
            width: parent.width
            visible: Daemon.ready && Library.loaded
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            text: importing.running ? "Looking through Steam, Lutris, Twintail and Prism…" : importing.error || "Bring in what Steam, Lutris, Twintail and Prism already have installed."
            color: importing.error ? Theme.error : Theme.fgSurfaceVariant
        }
        PlayButton {
            id: importing
            property bool running: false
            property string error: ""
            anchors.horizontalCenter: parent.horizontalCenter
            visible: Daemon.ready && Library.loaded
            enabled: !running
            label: running ? "Importing" : "Import games"
            icon: "download"
            onClicked: {
                running = true;
                error = "";
                Daemon.call("library.import", undefined, (r, e) => {
                    running = false;
                    if (e)
                        error = e.message;
                    else if (r.added.length === 0)
                        error = "Nothing found to import." + Object.values(r.errors).map(m => " " + m).join("");
                });
            }
        }
        TextButton {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: Daemon.ready && Library.loaded && !importing.running
            icon: "add"
            text: "Add a game by hand"
            onClicked: Nav.addGame()
        }
    }
}
