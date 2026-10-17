import QtQuick
import Quickshell
import qs.core
import qs.components

// Minecraft: Prism's instances, our layout (docs/plan.md, Minecraft UX). Left,
// a column of instance cards in Prism's groups (folding, remembered) with a
// search (typing anywhere starts it); right, the picked instance: its banner
// with Play, then its tabs. Up and Down move through the column, Enter plays.
// New instances and accounts are Prism's until Phase 10: + opens it.
Item {
    id: root

    focus: true

    readonly property string query: search.text.trim().toLowerCase()
    readonly property var instances: Library.games.filter(g => g.kind === "minecraft")
    readonly property var collapsed: Config.minecraft.collapsed ?? []

    // [{name, ids}]: Prism's groups by name, the instances without one first.
    // While searching, every group is open and empty ones go.
    readonly property var groups: {
        const by = {};
        for (const g of instances) {
            if (query !== "" && !matches(g))
                continue;
            const p = Library.prism(g) ?? {};
            const name = p.group ?? "";
            (by[name] = by[name] ?? []).push(g.id);
        }
        return Object.keys(by).sort((a, b) => a === "" ? -1 : b === "" ? 1 : a.localeCompare(b, undefined, { sensitivity: "base" })).map(name => ({ name, ids: by[name] }));
    }
    // What Up and Down walk through: the cards that are showing
    readonly property var shownIds: groups.reduce((all, g) => isOpen(g.name) ? all.concat(g.ids) : all, [])

    function matches(g) {
        const p = Library.prism(g) ?? {};
        return [g.name, p.minecraft, p.loader, p.group, p.pack].some(s => !!s && s.toLowerCase().includes(query));
    }
    function isOpen(group) {
        return query !== "" || group === "" || !collapsed.includes(group);
    }
    function toggle(group) {
        Config.minecraft.collapsed = collapsed.includes(group) ? collapsed.filter(c => c !== group) : [...collapsed, group];
    }

    // The picked instance: the remembered one, else the last played
    readonly property int selectedId: {
        const want = Config.minecraft.selected;
        if (instances.some(g => g.id === want))
            return want;
        const recent = instances.slice().sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
        return recent.length ? recent[0].id : -1;
    }
    readonly property var game: Library.byId[selectedId] ?? null
    function pick(id) {
        Config.minecraft.selected = id;
    }
    function step(by) {
        const ids = shownIds;
        if (ids.length === 0)
            return;
        const i = ids.indexOf(selectedId);
        pick(ids[Math.max(0, Math.min(ids.length - 1, i < 0 ? 0 : i + by))]);
    }
    function play(id) {
        pick(id);
        header.play.trigger();
    }

    Keys.onUpPressed: step(-1)
    Keys.onDownPressed: step(1)
    Keys.onReturnPressed: if (game) header.play.trigger()
    Keys.onEnterPressed: if (game) header.play.trigger()
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

    // --- the column

    Rectangle {
        id: column
        x: Theme.space.l
        y: Theme.space.l
        width: root.width < 900 ? 236 : 280
        height: parent.height - Theme.space.l * 2
        radius: Theme.radius.large
        color: Theme.glassRaised
        border.width: 1
        border.color: Theme.glassRim

        Item {
            id: top
            x: Theme.space.l
            y: Theme.space.l
            width: parent.width - Theme.space.l * 2
            height: 40
            SText {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                text: "Minecraft"
                font.pixelSize: Theme.font.title
                font.weight: Theme.font.weightTitle
            }
            IconButton {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                width: 40
                height: 40
                icon: "add"
                // A new instance is made in Prism until Phase 10
                onClicked: Daemon.call("minecraft.prism", {}, undefined)
            }
        }
        SearchField {
            id: search
            x: Theme.space.m
            anchors.top: top.bottom
            anchors.topMargin: Theme.space.m
            width: parent.width - Theme.space.m * 2
            height: 40
            placeholder: "Search instances"
            onEscaped: root.forceActiveFocus()
        }

        Flickable {
            id: list
            anchors.top: search.bottom
            anchors.topMargin: Theme.space.m
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space.s
            width: parent.width
            clip: true
            contentHeight: groupsCol.height
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: groupsCol
                x: Theme.space.s
                width: list.width - Theme.space.s * 2
                spacing: Theme.space.xs

                Repeater {
                    model: root.groups
                    Column {
                        id: group
                        required property var modelData
                        readonly property bool open: root.isOpen(modelData.name)
                        width: groupsCol.width

                        // The group's name folds it; the instances without one have none
                        PressButton {
                            width: parent.width
                            height: 32
                            visible: group.modelData.name !== ""
                            radius: Theme.radius.small
                            enabled: root.query === ""
                            opacity: 1
                            onClicked: root.toggle(group.modelData.name)
                            Icon {
                                id: chevron
                                x: Theme.space.xs
                                anchors.verticalCenter: parent.verticalCenter
                                name: "expand_more"
                                size: 20
                                color: Theme.fgSurfaceVariant
                                rotation: group.open ? 0 : -90
                                Behavior on rotation {
                                    Spring { preset: "snappy" }
                                }
                            }
                            SText {
                                anchors.left: chevron.right
                                anchors.leftMargin: Theme.space.xs
                                anchors.right: count.left
                                anchors.verticalCenter: parent.verticalCenter
                                text: group.modelData.name
                                dim: true
                                font.pixelSize: Theme.font.small
                                font.weight: Theme.font.weightTitle
                                font.capitalization: Font.AllUppercase
                                font.letterSpacing: 1
                            }
                            SText {
                                id: count
                                anchors.right: parent.right
                                anchors.rightMargin: Theme.space.m
                                anchors.verticalCenter: parent.verticalCenter
                                text: group.modelData.ids.length
                                dim: true
                                font.pixelSize: Theme.font.small
                            }
                        }

                        // Folds on a spring; folded, its cards are not built
                        Item {
                            id: foldBox
                            width: parent.width
                            height: cards.height * fold
                            clip: fold < 1
                            property real fold: group.open ? 1 : 0
                            Behavior on fold {
                                Spring { preset: "snappy"; epsilon: 0.005 }
                            }
                            Column {
                                id: cards
                                width: parent.width
                                visible: foldBox.fold > 0
                                Repeater {
                                    model: group.open || foldBox.fold > 0 ? group.modelData.ids : []
                                    InstanceCard {
                                        required property int modelData
                                        width: cards.width
                                        game: Library.byId[modelData] ?? null
                                        selected: modelData === root.selectedId
                                        onPicked: {
                                            root.pick(modelData);
                                            root.forceActiveFocus();
                                        }
                                        onPlay: root.play(modelData)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        SText {
            anchors.centerIn: list
            width: list.width - Theme.space.xl * 2
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            visible: Library.loaded && root.groups.length === 0
            dim: true
            text: root.query !== "" ? "No instance matches" : "No Prism instances yet. Import them in Settings, or make one with +."
        }
    }

    // --- the picked instance

    // What its files hold (`minecraft.instance`): read when it is picked and
    // after each session (new worlds and screenshots)
    property var info: null
    property string tab: "overview"
    function load() {
        const id = selectedId;
        if (id < 0) {
            info = null;
            return;
        }
        Daemon.call("minecraft.instance", { id }, (r, e) => {
            if (id === root.selectedId)
                root.info = r ?? null;
        });
    }
    onSelectedIdChanged: {
        info = null;
        load();
    }
    Component.onCompleted: load()
    readonly property bool selectedRuns: selectedId in Library.running
    onSelectedRunsChanged: if (!selectedRuns) load()
    Connections {
        target: Daemon
        function onReadyChanged() {
            if (Daemon.ready)
                root.load();
        }
    }

    Item {
        id: detail
        anchors.left: column.right
        anchors.leftMargin: Theme.space.l
        anchors.right: parent.right
        anchors.rightMargin: Theme.space.l
        y: Theme.space.l
        height: parent.height - Theme.space.l * 2
        visible: root.game !== null

        InstanceHeader {
            id: header
            width: parent.width
            height: Math.max(neededHeight, Math.min(340, parent.height * 0.4))
            game: root.game
            // A picture the user picked (the game's hero art), else the newest screenshot
            image: Library.art(root.game, "hero") || (root.info && root.info.screenshots.length ? Paths.url(root.info.screenshots[0].path) : "")
            wash: root.info && root.info.worlds.length && root.info.worlds[0].icon ? Paths.url(root.info.worlds[0].icon) : ""
        }

        Item {
            id: bar
            anchors.top: header.bottom
            anchors.topMargin: Theme.space.l
            width: parent.width
            height: tabs.height
            Segmented {
                id: tabs
                options: [{ id: "overview", name: "Overview" }, { id: "mods", name: "Mods" }, { id: "settings", name: "Settings" }, { id: "files", name: "Files" }]
                current: root.tab
                onPicked: id => root.tab = id
            }
            SText {
                anchors.left: tabs.right
                anchors.leftMargin: Theme.space.l
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                visible: header.play.error !== ""
                text: header.play.error
                color: Theme.error
                elide: Text.ElideRight
            }
        }

        Item {
            id: body
            anchors.top: bar.bottom
            anchors.topMargin: Theme.space.l
            anchors.bottom: parent.bottom
            width: parent.width
            clip: true

            InstanceOverview {
                anchors.fill: parent
                visible: root.tab === "overview"
                game: root.game
                info: root.info
                onPlay: extra => Library.play(root.selectedId, e => header.play.error = e ? e.message : "", extra)
                onShowShot: i => viewer.index = i
            }
            InstanceFiles {
                anchors.fill: parent
                visible: root.tab === "files"
                game: root.game
                info: root.info
            }
            InstanceMods {
                anchors.fill: parent
                visible: root.tab === "mods"
                active: visible
                game: root.game
            }
            Placeholder {
                anchors.fill: parent
                visible: root.tab === "settings"
                icon: "tune"
                title: "Settings"
                note: "Coming next (Phase 8e)"
            }
        }

        // Over the banner's top right; it unfolds over what is below
        AccountMenu {
            anchors.right: header.right
            anchors.top: header.top
            anchors.margins: Theme.space.l
            gameId: root.selectedId
        }
    }

    ScreenshotViewer {
        id: viewer
        anchors.fill: parent
        shots: root.info ? root.info.screenshots : []
        onCloseRequested: {
            index = -1;
            root.forceActiveFocus();
        }
    }

    Placeholder {
        anchors.fill: detail
        z: -1
        visible: Library.loaded && root.instances.length === 0
        icon: "deployed_code"
        title: "Minecraft"
        note: "Prism Launcher's instances show here"
    }
}
