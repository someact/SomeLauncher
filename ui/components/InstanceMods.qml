import QtQuick
import qs.core

// A Minecraft instance's Mods tab (`minecraft.mods`): search, All/On/Off, a
// switch per mod (Prism's `.disabled` rename, `minecraft.setMod`), its page on
// Modrinth or CurseForge, the folder. Read when the tab shows and when the
// instance changes. Only the rows on screen are built (packs have hundreds).
// A modpack's mods may come back on when Prism updates the pack: a note says so.
Item {
    id: root

    property var game: null
    property bool active: false
    readonly property bool running: !!game && game.id in Library.running

    property var mods: []                 // every mod, as the daemon sent them
    property var pack: null
    property string dir: ""
    property bool loading: false
    property string error: ""
    property string filter: "all"
    readonly property string query: search.text.trim().toLowerCase()
    readonly property int offCount: mods.filter(m => !m.enabled).length

    function load() {
        if (!game)
            return;
        const id = game.id;
        loading = true;
        Daemon.call("minecraft.mods", { id }, (r, e) => {
            if (!root.game || root.game.id !== id)
                return;
            root.loading = false;
            root.error = e ? e.message : "";
            root.mods = r ? r.mods : [];
            root.pack = r ? r.pack : null;
            root.dir = r ? r.dir : "";
            root.rebuild();
        });
    }
    onActiveChanged: if (active) load()
    onGameChanged: {
        mods = [];
        model.clear();
        search.text = "";
        if (active)
            load();
    }
    onQueryChanged: rebuild()
    onFilterChanged: rebuild()

    ListModel {
        id: model
    }
    function rebuild() {
        model.clear();
        for (const m of mods) {
            if (filter === "on" && !m.enabled || filter === "off" && m.enabled)
                continue;
            if (query !== "" && !(m.name.toLowerCase().includes(query) || m.file.toLowerCase().includes(query) || (m.description ?? "").toLowerCase().includes(query)))
                continue;
            model.append({
                file: m.file,
                name: m.name,
                version: m.version ?? "",
                description: m.description ?? "",
                authors: (m.authors ?? []).join(", "),
                icon: m.icon ?? "",
                url: m.url ?? "",
                enabled: m.enabled
            });
        }
    }

    // Switched at once; put back if the daemon refuses
    function setMod(row, on) {
        const was = model.get(row).file;
        const apply = (file, enabled) => {
            const m = mods.find(m => m.file === was || m.file === file);
            if (m) {
                m.file = file;
                m.enabled = enabled;
            }
            if (row < model.count) {
                model.setProperty(row, "file", file);
                model.setProperty(row, "enabled", enabled);
            }
            mods = mods.slice();     // offCount follows
        };
        apply(was, on);
        Daemon.call("minecraft.setMod", { id: game.id, file: was, enabled: on }, (r, e) => {
            if (e) {
                root.error = e.message;
                apply(was, !on);
            } else {
                root.error = "";
                apply(r.file, on);
            }
        });
    }

    // --- the bar

    Item {
        id: bar
        width: parent.width
        height: 40
        SearchField {
            id: search
            width: Math.min(300, parent.width * 0.4)
            height: 40
            placeholder: "Search mods"
        }
        Segmented {
            id: filterBar
            anchors.left: search.right
            anchors.leftMargin: Theme.space.m
            options: [{ id: "all", name: "All" }, { id: "on", name: "On" }, { id: "off", name: "Off" }]
            current: root.filter
            onPicked: id => root.filter = id
        }
        SText {
            anchors.left: filterBar.right
            anchors.leftMargin: Theme.space.l
            anchors.right: folder.left
            anchors.rightMargin: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            dim: true
            text: root.loading && root.mods.length === 0 ? "Reading mods…" : root.mods.length + (root.mods.length === 1 ? " mod" : " mods") + (root.offCount ? " · " + root.offCount + " off" : "")
        }
        IconButton {
            id: folder
            anchors.right: parent.right
            width: 40
            height: 40
            icon: "folder_open"
            iconSize: 20
            onClicked: if (root.dir) Qt.openUrlExternally(Paths.url(root.dir))
        }
    }

    // A modpack, the game running, or an error: one line under the bar
    Rectangle {
        id: note
        anchors.top: bar.bottom
        anchors.topMargin: visible ? Theme.space.m : 0
        width: parent.width
        height: visible ? noteText.implicitHeight + Theme.space.m * 2 : 0
        visible: noteText.text !== ""
        radius: Theme.radius.normal
        color: root.error !== "" ? Qt.alpha(Theme.error, 0.12) : Theme.glassRaised
        Icon {
            id: noteIcon
            x: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            name: root.error !== "" ? "error" : root.running ? "sports_esports" : "inventory_2"
            size: 20
            color: root.error !== "" ? Theme.error : Theme.primary
        }
        SText {
            id: noteText
            anchors.left: noteIcon.right
            anchors.leftMargin: Theme.space.m
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            wrapMode: Text.Wrap
            maximumLineCount: 3
            text: {
                if (root.error !== "")
                    return root.error;
                if (root.running)
                    return "The game is running: mods can change once it is closed.";
                if (root.pack)
                    return (root.pack.name ?? "This instance") + " is a " + (root.pack.type === "flame" ? "CurseForge" : root.pack.type === "modrinth" ? "Modrinth" : "") + " modpack: updating it in Prism may switch mods you turned off back on.";
                return "";
            }
        }
    }

    ListView {
        id: list
        anchors.top: note.bottom
        anchors.topMargin: Theme.space.m
        anchors.bottom: parent.bottom
        width: parent.width
        clip: true
        model: model
        spacing: Theme.space.xs
        boundsBehavior: Flickable.StopAtBounds
        reuseItems: true
        cacheBuffer: 400

        delegate: Rectangle {
            id: row
            required property int index
            required property string file
            required property string name
            required property string version
            required property string description
            required property string authors
            required property string icon
            required property string url
            required property bool enabled
            width: list.width
            height: 60
            radius: Theme.radius.normal
            color: hover.hovered ? Theme.glassRaisedHover : Theme.glassRaised
            opacity: enabled ? 1 : 0.6
            HoverHandler {
                id: hover
            }

            Rectangle {
                id: pic
                x: Theme.space.s
                anchors.verticalCenter: parent.verticalCenter
                width: 40
                height: 40
                radius: Theme.radius.small
                color: picImg.status === Image.Ready ? "transparent" : Theme.surfaceHigh
                Image {
                    id: picImg
                    anchors.fill: parent
                    source: row.icon !== "" ? Paths.url(row.icon) : ""
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    // Most mod icons are pixel art: hard pixels when drawn larger
                    smooth: implicitWidth >= width * Screen.devicePixelRatio
                }
                Icon {
                    anchors.centerIn: parent
                    visible: picImg.status !== Image.Ready
                    name: "extension"
                    color: Theme.fgSurfaceVariant
                }
            }
            Column {
                anchors.left: pic.right
                anchors.leftMargin: Theme.space.m
                anchors.right: link.left
                anchors.rightMargin: Theme.space.s
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                Row {
                    width: parent.width
                    spacing: Theme.space.s
                    SText {
                        id: modName
                        width: Math.min(implicitWidth, parent.width - (modVersion.visible ? Math.min(modVersion.implicitWidth, parent.width * 0.4) + parent.spacing : 0))
                        text: row.name
                        font.weight: Theme.font.weightTitle
                    }
                    SText {
                        id: modVersion
                        anchors.baseline: modName.baseline
                        width: Math.min(implicitWidth, parent.width * 0.4)
                        visible: row.version !== ""
                        text: row.version
                        dim: true
                        font.pixelSize: Theme.font.small
                    }
                }
                SText {
                    width: parent.width
                    text: row.description || row.authors || row.file
                    dim: true
                    font.pixelSize: Theme.font.small
                }
            }
            IconButton {
                id: link
                anchors.right: modSwitch.left
                anchors.rightMargin: Theme.space.m
                anchors.verticalCenter: parent.verticalCenter
                width: 36
                height: 36
                icon: "open_in_new"
                iconSize: 18
                color: "transparent"
                visible: row.url !== "" && hover.hovered
                onClicked: Qt.openUrlExternally(row.url)
            }
            Switch {
                id: modSwitch
                anchors.right: parent.right
                anchors.rightMargin: Theme.space.m
                anchors.verticalCenter: parent.verticalCenter
                checked: row.enabled
                enabled: !root.running
                onToggled: on => root.setMod(row.index, on)
            }
        }
    }

    SText {
        anchors.centerIn: list
        visible: !root.loading && model.count === 0
        dim: true
        text: root.mods.length === 0 ? "No mods in this instance" : "No mod matches"
    }
}
