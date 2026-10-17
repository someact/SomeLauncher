import QtQuick
import qs.core

// Picks what a game is (3a): a search box (the game's name to start with) and
// the candidates from `meta.search` as covers, best first; a click matches the
// game (`meta.match`) and fires `matched(game)`. Without a game yet (the add
// wizard, `gameId` -1) it searches `query` and a click only fires `chosen(c)`,
// for the wizard to match once the game is added.
Column {
    id: root

    property int gameId: -1
    property string query: ""
    property var results: []
    property bool searching: false
    property string error: ""
    property string matching: ""             // the candidate being matched: source:id
    readonly property var game: Library.byId[gameId] ?? null
    property string chosenKey: ""
    readonly property string matchedKey: game && game.meta ? game.meta.source + ":" + game.meta.sourceId : chosenKey

    signal matched(var game)
    signal chosen(var candidate)

    spacing: Theme.space.m

    function search() {
        searching = true;
        error = "";
        const q = field.text.trim();
        const params = gameId < 0 ? { query: q } : q !== "" && (!game || q !== game.name) ? { id: gameId, query: q } : { id: gameId };
        Daemon.call("meta.search", params, (r, e) => {
            searching = false;
            if (e)
                error = e.message;
            results = r ?? [];
        });
    }
    function pick(c) {
        if (gameId < 0) {
            const key = c.source + ":" + c.id;
            chosenKey = chosenKey === key ? "" : key;
            root.chosen(chosenKey ? c : null);
            return;
        }
        matching = c.source + ":" + c.id;
        Daemon.call("meta.match", { id: gameId, source: c.source, sourceId: c.id }, (g, e) => {
            matching = "";
            if (e)
                error = e.message;
            else
                root.matched(g);
        });
    }
    Component.onCompleted: if (gameId >= 0 || query !== "") search()

    Row {
        width: parent.width
        spacing: Theme.space.s
        Field {
            id: field
            width: parent.width - go.width - Theme.space.s
            value: root.query || (root.game ? root.game.name : "")
            placeholder: "Game name"
            onCommitted: root.search()
        }
        PressButton {
            id: go
            width: 96
            height: 40
            radius: Theme.radius.small + 2
            color: Theme.secondaryContainer
            enabled: !root.searching
            onClicked: root.search()
            SText {
                anchors.centerIn: parent
                text: root.searching ? "Searching" : "Search"
                color: Theme.fgSecondaryContainer
                font.weight: Theme.font.weightTitle
            }
        }
    }

    SText {
        width: parent.width
        visible: root.error !== "" || (!root.searching && root.results.length === 0)
        text: root.error || "Nothing found. Try another name, or add a SteamGridDB key in Settings."
        color: root.error ? Theme.error : Theme.fgSurfaceVariant
        wrapMode: Text.Wrap
    }

    Flow {
        width: parent.width
        spacing: Theme.space.m
        Repeater {
            model: root.results.slice(0, 12)
            Item {
                id: cand
                required property var modelData
                readonly property string key: modelData.source + ":" + modelData.id
                readonly property bool current: key === root.matchedKey
                width: 112
                height: 168 + label.height + Theme.space.xs
                scale: mouse.pressed ? 0.96 : mouse.containsMouse ? 1.03 : 1
                Behavior on scale {
                    Spring { preset: "bouncy" }
                }

                Rectangle {
                    id: cover
                    width: parent.width
                    height: 168
                    radius: Theme.radius.small + 2
                    color: Theme.surfaceHighest
                    clip: true
                    border.width: cand.current ? 2 : 1
                    border.color: cand.current ? Theme.primary : Theme.glassRim
                    Image {
                        id: img
                        anchors.fill: parent
                        anchors.margins: cover.border.width
                        source: cand.modelData.cover || cand.modelData.thumb || ""
                        sourceSize: Qt.size(224, 336)
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                    }
                    // A Steam app without portrait art: its wide capsule instead
                    Image {
                        anchors.centerIn: parent
                        width: parent.width - 8
                        height: width * 87 / 231
                        visible: img.status === Image.Error && !!cand.modelData.thumb
                        source: visible ? cand.modelData.thumb : ""
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                    }
                    Rectangle {
                        anchors.left: parent.left
                        anchors.bottom: parent.bottom
                        anchors.margins: Theme.space.xs
                        width: src.implicitWidth + Theme.space.s * 2
                        height: 20
                        radius: 10
                        color: Theme.scrim
                        SText {
                            id: src
                            anchors.centerIn: parent
                            text: cand.modelData.reason || (cand.modelData.source === "steamgriddb" ? "SteamGridDB" : "Steam")
                            color: Theme.fgOnArt
                            font.pixelSize: Theme.font.small - 1
                        }
                    }
                    Rectangle {
                        anchors.fill: parent
                        visible: root.matching === cand.key
                        color: Theme.scrim
                        Icon {
                            anchors.centerIn: parent
                            name: "hourglass_top"
                            color: Theme.fgOnArt
                        }
                    }
                }
                SText {
                    id: label
                    y: cover.height + Theme.space.xs
                    width: parent.width
                    text: cand.modelData.name + (cand.modelData.year ? ` (${cand.modelData.year})` : "")
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    font.pixelSize: Theme.font.small
                    font.weight: cand.current ? Theme.font.weightTitle : Theme.font.weight
                    color: cand.current ? Theme.primary : Theme.fgSurface
                }
                MouseArea {
                    id: mouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.pick(cand.modelData)
                }
            }
        }
    }
}
