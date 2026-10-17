import QtQuick
import Quickshell.Widgets
import qs.core

// A game's achievements, a glass sheet sliding over the game page from the
// right (like its settings): progress, then the list with All, Unlocked and
// Locked. Unlocked ones come first, newest first, each with its date; locked
// ones keep their source's order, their icon gray. A hidden achievement keeps
// its description to itself until unlocked, or while hovered.
// Read when the sheet opens and whenever the game's progress changes.
Item {
    id: root

    property var game: null
    property bool open: false
    signal closeRequested

    property var list: []
    property string filter: "all"
    property bool loading: false
    property string error: ""
    readonly property var progress: game ? game.achievements ?? null : null

    readonly property var shown: {
        const unlocked = list.filter(a => a.unlockedAt !== null).sort((a, b) => (b.unlockedAt ?? 0) - (a.unlockedAt ?? 0));
        const locked = list.filter(a => a.unlockedAt === null);
        return filter === "unlocked" ? unlocked : filter === "locked" ? locked : unlocked.concat(locked);
    }

    function load() {
        if (!game)
            return;
        const id = game.id;
        Daemon.call("achievements.list", { id }, (r, e) => {
            if (root.game && root.game.id === id) {
                root.list = r ?? [];
                root.error = e ? e.message : "";
            }
        });
    }
    function refresh() {
        if (!game)
            return;
        loading = true;
        const id = game.id;
        Daemon.call("achievements.refresh", { id }, (r, e) => {
            root.loading = false;
            root.error = e ? e.message : "";
            if (r)
                Library._merge({ [id]: r });
            root.load();
        });
    }
    onOpenChanged: if (open) load()
    // A refresh (after a session, or the button) changed the counts
    readonly property string progressKey: progress ? progress.unlocked + "/" + progress.total + "/" + (progress.last ?? 0) : ""
    onProgressKeyChanged: if (open) load()

    function icon(a) {
        const src = a.unlockedAt !== null ? a.icon : a.iconGray || a.icon;
        return Library.media(src);
    }
    function date(t) {
        if (t === null || t === undefined)
            return "";
        if (t === 0)
            return "Unlocked";
        return new Date(t * 1000).toLocaleDateString(Qt.locale(), "d MMM yyyy");
    }
    function sourceNote(s) {
        return { steam: "From Steam's cache on this PC. Add a Steam Web API key in Settings to see games played elsewhere.", steamApi: "From the Steam Web API", gbe: "From the game's gbe_fork save" }[s] ?? "";
    }

    Keys.onEscapePressed: closeRequested()

    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.35)
        opacity: root.open ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation { duration: Motion.normal }
        }
        MouseArea {
            anchors.fill: parent
            onClicked: root.closeRequested()
        }
    }

    Rectangle {
        id: sheet
        width: Math.min(560, root.width - Theme.space.xxl * 2)
        height: root.height - Theme.space.m * 2
        y: Theme.space.m
        x: root.open ? root.width - width - Theme.space.m : root.width + Theme.space.xl
        visible: x < root.width
        Behavior on x {
            Spring { preset: "snappy" }
        }
        radius: Theme.radius.xl
        color: Qt.rgba(Theme.surface.r, Theme.surface.g, Theme.surface.b, 0.94)
        border.width: 1
        border.color: Theme.glassRim

        MouseArea {
            anchors.fill: parent
        }

        Column {
            id: head
            x: Theme.space.xl
            y: Theme.space.xl
            width: parent.width - Theme.space.xl * 2
            spacing: Theme.space.l

            Item {
                width: parent.width
                height: 44
                Column {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - close.width - refreshButton.width - Theme.space.m * 2
                    SText {
                        width: parent.width
                        text: "Achievements"
                        font.pixelSize: Theme.font.title
                        font.weight: Theme.font.weightTitle
                    }
                    SText {
                        width: parent.width
                        text: root.game ? root.game.name : ""
                        dim: true
                    }
                }
                IconButton {
                    id: refreshButton
                    anchors.right: close.left
                    anchors.rightMargin: Theme.space.s
                    anchors.verticalCenter: parent.verticalCenter
                    icon: root.loading ? "hourglass_top" : "refresh"
                    enabled: !root.loading
                    onClicked: root.refresh()
                }
                IconButton {
                    id: close
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    icon: "close"
                    onClicked: root.closeRequested()
                }
            }

            // Progress
            Column {
                width: parent.width
                spacing: Theme.space.s
                visible: !!root.progress
                Row {
                    spacing: Theme.space.m
                    SText {
                        anchors.baseline: pct.baseline
                        text: root.progress ? `${root.progress.unlocked} of ${root.progress.total}` : ""
                        font.pixelSize: Theme.font.large
                        font.weight: Theme.font.weightTitle
                    }
                    SText {
                        id: pct
                        text: root.progress && root.progress.total ? Math.floor(root.progress.unlocked * 100 / root.progress.total) + " %" : ""
                        dim: true
                    }
                    SText {
                        anchors.baseline: pct.baseline
                        visible: !!root.progress && !!root.progress.last
                        text: root.progress && root.progress.last ? "· last " + root.date(root.progress.last) : ""
                        dim: true
                    }
                }
                Rectangle {
                    width: parent.width
                    height: 6
                    radius: 3
                    color: Theme.glassRaisedHover
                    Rectangle {
                        width: parent.width * (root.progress && root.progress.total ? root.progress.unlocked / root.progress.total : 0)
                        height: parent.height
                        radius: 3
                        color: Theme.primary
                        Behavior on width {
                            Spring { preset: "smooth" }
                        }
                    }
                }
            }

            Segmented {
                options: [{ id: "all", name: "All" }, { id: "unlocked", name: "Unlocked" }, { id: "locked", name: "Locked" }]
                current: root.filter
                onPicked: id => root.filter = id
            }
            SText {
                width: parent.width
                visible: text !== ""
                text: root.error
                color: Theme.error
                wrapMode: Text.Wrap
                maximumLineCount: 3
            }
        }

        ListView {
            id: listView
            anchors.top: head.bottom
            anchors.topMargin: Theme.space.l
            anchors.bottom: foot.top
            anchors.bottomMargin: Theme.space.s
            x: Theme.space.xl
            width: parent.width - Theme.space.xl * 2
            clip: true
            spacing: Theme.space.s
            boundsBehavior: Flickable.StopAtBounds
            model: root.shown
            delegate: Rectangle {
                id: row
                required property var modelData
                readonly property bool unlocked: modelData.unlockedAt !== null
                readonly property bool secret: modelData.hidden && !unlocked && !rowHover.hovered
                width: listView.width
                height: Math.max(68, text.implicitHeight + Theme.space.m * 2)
                radius: Theme.radius.normal
                color: unlocked ? Theme.glassRaised : "transparent"
                border.width: unlocked ? 0 : 1
                border.color: Theme.glassRim
                HoverHandler {
                    id: rowHover
                }
                ClippingRectangle {
                    id: pic
                    x: Theme.space.m
                    anchors.verticalCenter: parent.verticalCenter
                    width: 48
                    height: 48
                    radius: Theme.radius.small
                    color: Theme.surfaceHighest
                    opacity: row.unlocked ? 1 : 0.55
                    Image {
                        anchors.fill: parent
                        source: root.icon(row.modelData)
                        sourceSize: Qt.size(96, 96)
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: root.icon(row.modelData) === ""
                        name: "trophy"
                        color: Theme.fgSurfaceVariant
                    }
                }
                Column {
                    id: text
                    anchors.left: pic.right
                    anchors.leftMargin: Theme.space.m
                    anchors.right: when.left
                    anchors.rightMargin: Theme.space.m
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    SText {
                        width: parent.width
                        text: row.modelData.name
                        font.weight: Theme.font.weightTitle
                        opacity: row.unlocked ? 1 : 0.8
                    }
                    SText {
                        width: parent.width
                        visible: text !== ""
                        text: row.secret ? "Hidden achievement" : row.modelData.description
                        font.italic: row.secret
                        dim: true
                        font.pixelSize: Theme.font.small
                        wrapMode: Text.Wrap
                        maximumLineCount: 3
                    }
                }
                Item {
                    id: when
                    anchors.right: parent.right
                    anchors.rightMargin: Theme.space.m
                    anchors.verticalCenter: parent.verticalCenter
                    width: row.unlocked ? whenText.implicitWidth : 20
                    height: 20
                    SText {
                        id: whenText
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        visible: row.unlocked
                        text: root.date(row.modelData.unlockedAt)
                        dim: true
                        font.pixelSize: Theme.font.small
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: !row.unlocked
                        name: "lock"
                        size: 18
                        color: Theme.fgSurfaceVariant
                    }
                }
            }
        }

        SText {
            anchors.centerIn: listView
            visible: root.shown.length === 0
            text: root.list.length === 0 ? "No achievements known for this game" : root.filter === "unlocked" ? "Nothing unlocked yet" : "Everything is unlocked"
            dim: true
        }

        SText {
            id: foot
            x: Theme.space.xl
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space.l
            width: parent.width - Theme.space.xl * 2
            text: root.progress ? root.sourceNote(root.progress.source) : ""
            dim: true
            font.pixelSize: Theme.font.small
            wrapMode: Text.Wrap
            maximumLineCount: 2
        }
    }
}
