import QtQuick
import qs.core
import qs.components

// Library, Netflix style: a row of wide cards for recent and favorite games,
// then a row per category (the user's, else the store's), then the games
// without one by kind, and the games that are not installed last. Searching or
// picking a filter shows a grid of the matches instead. Typing anywhere
// searches; Ctrl+F or / focuses the box, Escape clears it.
// A click picks a card, a double click plays it.
Item {
    id: root

    focus: true

    readonly property string query: search.text.trim().toLowerCase()
    property string filter: "all"
    readonly property bool browsing: query === "" && filter === "all"
    property int selectedId: -1

    readonly property var kinds: [
        { id: "proton", name: "Windows" },
        { id: "native", name: "Linux" },
        { id: "steam", name: "Steam" },
        { id: "hoyo", name: "HoYoverse" },
        { id: "minecraft", name: "Minecraft" }
    ]
    // Kind and state filters only show when they would match something
    readonly property var filters: {
        const all = Library.all;
        const f = [{ id: "all", name: "All" }, { id: "favorites", name: "Favorites", icon: "favorite" }];
        for (const k of kinds)
            if (Library.games.some(g => g.kind === k.id))
                f.push(k);
        if (Library.games.some(g => !installed(g)))
            f.push({ id: "notInstalled", name: "Not installed" });
        if (all.some(g => g.hidden))
            f.push({ id: "hidden", name: "Hidden", icon: "visibility_off" });
        return f;
    }
    onFiltersChanged: if (!filters.some(f => f.id === filter)) filter = "all"

    function installed(g) {
        return !!g.installDir;
    }
    function categories(g) {
        return g.categories.length > 0 ? g.categories : (g.meta && g.meta.categories ? g.meta.categories : []);
    }
    function matches(g) {
        if (filter === "hidden" ? !g.hidden : g.hidden)
            return false;
        if (filter === "favorites" && !g.favorite || filter === "notInstalled" && installed(g))
            return false;
        if (kinds.some(k => k.id === filter) && g.kind !== filter)
            return false;
        if (query === "")
            return true;
        return g.name.toLowerCase().includes(query) || g.tags.some(t => t.includes(query)) || categories(g).some(c => c.toLowerCase().includes(query));
    }

    // Rows: keys in order, and the ids of each. A row whose games did not
    // change keeps its array, so its scroll position stays.
    property var rowKeys: []
    property var rowTitles: ({})
    property var rowIds: ({})
    property var resultIds: []

    function rebuild() {
        const games = Library.games;
        const ids = {};
        const titles = {};
        const keys = [];
        const add = (key, title, g) => {
            if (!(key in ids)) {
                ids[key] = [];
                titles[key] = title;
                keys.push(key);
            }
            ids[key].push(g.id);
        };

        const days = Config.home.recentDays;
        const since = Library.now - days * 86400;
        const top = games.filter(g => g.favorite || g.lastPlayed && (days <= 0 || g.lastPlayed >= since)).sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0)).slice(0, 16);
        for (const g of top)
            add("top", "Recent and favorites", g);

        // Categories, the biggest rows first
        const cats = {};
        for (const g of games)
            if (installed(g))
                for (const c of categories(g))
                    (cats[c] = cats[c] ?? []).push(g);
        for (const c of Object.keys(cats).sort((a, b) => cats[b].length - cats[a].length || a.localeCompare(b)))
            for (const g of cats[c])
                add("cat:" + c, c, g);

        for (const k of kinds)
            for (const g of games)
                if (g.kind === k.id && installed(g) && categories(g).length === 0)
                    add("kind:" + k.id, k.id === "steam" ? "Steam" : k.id === "hoyo" || k.id === "minecraft" ? k.name : k.name + " games", g);
        for (const g of games)
            if (!installed(g))
                add("notInstalled", "Not installed", g);

        for (const k of keys)
            if (rowIds[k] && Library.sameIds(rowIds[k], ids[k]))
                ids[k] = rowIds[k];
        rowTitles = titles;
        rowIds = ids;
        if (!Library.sameIds(keys, rowKeys))
            rowKeys = keys;

        const res = Library.all.filter(matches).map(g => g.id);
        if (!Library.sameIds(res, resultIds))
            resultIds = res;
    }
    Component.onCompleted: rebuild()
    onQueryChanged: rebuild()
    onFilterChanged: rebuild()
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

    function play(id) {
        selectedId = id;
        if (!(id in Library.running))
            Library.play(id, error => {
                if (error)
                    toast.show(error.message);
            });
    }

    // Typing anywhere starts a search
    Keys.onPressed: event => {
        if (event.text.length === 1 && event.text >= " " && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) {
            search.text += event.text;
            search.focusField();
            event.accepted = true;
        }
    }
    Shortcut {
        sequences: ["Ctrl+F", "/"]
        onActivated: search.focusField()
    }

    // The bar: title, search and the filters
    Column {
        id: bar
        x: Theme.space.xl
        y: Theme.space.xl
        width: parent.width - Theme.space.xl * 2
        spacing: Theme.space.l

        Item {
            width: parent.width
            height: search.height
            Row {
                anchors.verticalCenter: parent.verticalCenter
                spacing: Theme.space.m
                SText {
                    id: heading
                    text: "Library"
                    font.pixelSize: Theme.font.headline
                    font.weight: Theme.font.weightTitle
                }
                SText {
                    anchors.baseline: heading.baseline
                    text: Library.games.length + (Library.games.length === 1 ? " game" : " games")
                    dim: true
                    visible: Library.loaded
                }
            }
            SearchField {
                id: search
                anchors.right: parent.right
                width: Math.min(360, parent.width * 0.45)
                placeholder: "Search by name, tag or category"
                onEscaped: root.forceActiveFocus()
            }
        }

        Flow {
            width: parent.width
            spacing: Theme.space.s
            Repeater {
                model: root.filters
                Chip {
                    required property var modelData
                    label: modelData.name
                    icon: modelData.icon ?? ""
                    active: root.filter === modelData.id
                    onClicked: root.filter = modelData.id
                }
            }
        }
    }

    Item {
        id: body
        anchors.top: bar.bottom
        anchors.topMargin: Theme.space.m
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        clip: true

        // Browsing: rows
        Flickable {
            id: rows
            anchors.fill: parent
            visible: root.browsing && Library.games.length > 0
            contentHeight: rowCol.implicitHeight + Theme.space.xxl
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: rowCol
                width: rows.width
                spacing: Theme.space.l
                Repeater {
                    model: root.rowKeys
                    GameRow {
                        required property string modelData
                        width: rowCol.width
                        inset: Theme.space.xl
                        wide: modelData === "top"
                        title: root.rowTitles[modelData] ?? ""
                        note: modelData === "top" ? "" : String((root.rowIds[modelData] ?? []).length)
                        ids: root.rowIds[modelData] ?? []
                        selectedId: root.selectedId
                        onPicked: id => root.selectedId = id
                        onActivated: id => root.play(id)
                    }
                }
            }
        }

        // Searching or filtering: a grid of the matches
        GridView {
            id: grid
            anchors.fill: parent
            anchors.leftMargin: Theme.space.xl
            anchors.rightMargin: Theme.space.xl
            visible: !root.browsing
            cellWidth: 156 + Theme.space.l
            cellHeight: 234 + Theme.space.l * 2
            topMargin: Theme.space.xs
            bottomMargin: Theme.space.xxl
            boundsBehavior: Flickable.StopAtBounds
            cacheBuffer: 600
            model: root.resultIds
            header: SText {
                width: grid.width
                height: 28
                text: root.resultIds.length === 0 ? "" : root.resultIds.length + (root.resultIds.length === 1 ? " match" : " matches")
                dim: true
            }
            delegate: Item {
                required property int modelData
                width: grid.cellWidth
                height: grid.cellHeight
                GameCard {
                    anchors.verticalCenter: parent.verticalCenter
                    gameId: parent.modelData
                    selected: parent.modelData === root.selectedId
                    onClicked: root.selectedId = parent.modelData
                    onDoubleClicked: root.play(parent.modelData)
                }
            }
        }
        Column {
            anchors.centerIn: parent
            visible: !root.browsing && root.resultIds.length === 0
            spacing: Theme.space.m
            Icon {
                anchors.horizontalCenter: parent.horizontalCenter
                name: "search_off"
                size: 40
                color: Theme.fgSurfaceVariant
            }
            SText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.query !== "" ? `Nothing matches “${search.text.trim()}”` : "No games here"
                dim: true
            }
        }

        // An empty library (or no daemon)
        Column {
            anchors.centerIn: parent
            visible: root.browsing && Library.games.length === 0
            spacing: Theme.space.m
            Icon {
                anchors.horizontalCenter: parent.horizontalCenter
                name: "stadia_controller"
                size: 48
                color: Theme.fgSurfaceVariant
            }
            SText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: !Daemon.ready ? "Connecting…" : Library.loaded ? "No games yet. Home can import them." : "Loading…"
                dim: true
            }
        }
    }

    // A short message at the bottom (a launch that failed)
    Rectangle {
        id: toast
        property string message
        function show(m) {
            message = m;
            hide.restart();
        }
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space.xl
        width: Math.min(parent.width - Theme.space.xxl * 2, msg.implicitWidth + Theme.space.xl * 2)
        height: msg.implicitHeight + Theme.space.m * 2
        radius: Theme.radius.normal
        color: Theme.surfaceHighest
        opacity: hide.running ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.normal }
        }
        Timer {
            id: hide
            interval: 5000
        }
        SText {
            id: msg
            anchors.centerIn: parent
            width: parent.width - Theme.space.xl * 2
            text: toast.message
            color: Theme.error
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
        }
    }
}
