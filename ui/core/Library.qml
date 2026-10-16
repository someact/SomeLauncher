pragma Singleton

import QtQuick
import Quickshell

// The daemon's game library, kept current from its events (never polled):
// `games` (visible games by name), `byId`, and `running` (id → the latest
// `game.state`). Pages derive their rows as lists of ids and bind each card to
// `Library.byId[id]`, so a changed game updates its card without rebuilding the row.
//   Library.play(id, error => …)   Library.stop(id)
Singleton {
    id: root

    property var games: []
    property var byId: ({})
    property var running: ({})
    property bool loaded: false
    // Unix seconds, renewed with each change: what "2 days ago" is counted from
    property real now: Date.now() / 1000

    function refresh() {
        Daemon.call("games.list", undefined, (list, error) => {
            if (error)
                return;
            root._set(list);
            root.loaded = true;
        });
        Daemon.call("games.running", undefined, (list, error) => {
            if (error)
                return;
            const r = {};
            for (const g of list)
                r[g.id] = { state: g.since === null ? "starting" : "running", unit: g.unit, since: g.since };
            root.running = r;
        });
    }

    function _set(list) {
        const m = {};
        for (const g of list)
            m[g.id] = g;
        now = Date.now() / 1000;
        games = list;
        byId = m;
    }

    // `library.changed` names the games to fetch again; changes come in bursts
    // (an art job touches one game after another), so they are gathered first
    property var _changed: ({})
    Timer {
        id: gather
        interval: 150
        onTriggered: {
            const ids = Object.keys(root._changed);
            root._changed = {};
            let left = ids.length;
            const got = {};
            for (const id of ids)
                Daemon.call("games.get", { id: Number(id) }, (g, error) => {
                    got[id] = error ? null : g;
                    if (--left === 0)
                        root._merge(got);
                });
        }
    }
    function _merge(got) {
        const list = games.filter(g => !(g.id in got));
        for (const id in got)
            if (got[id] && !got[id].hidden)
                list.push(got[id]);
        list.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
        _set(list);
    }

    Connections {
        target: Daemon
        function onReadyChanged() {
            if (Daemon.ready)
                root.refresh();
        }
        function onEvent(name, data) {
            if (name === "library.changed") {
                for (const id of data.ids)
                    root._changed[id] = true;
                gather.restart();
            } else if (name === "game.state") {
                const r = Object.assign({}, root.running);
                if (data.state === "stopped")
                    delete r[data.id];
                else
                    r[data.id] = data;
                root.running = r;
            }
        }
    }
    Component.onCompleted: if (Daemon.ready) refresh()

    function play(id, done) {
        Daemon.call("games.launch", { id }, (r, error) => {
            if (done)
                done(error);
        });
    }
    function stop(id) {
        Daemon.call("games.stop", { id }, undefined);
    }
    function setFavorite(id, on) {
        Daemon.call("games.update", { id, favorite: on }, undefined);
    }

    // The same list of ids? Rows keep their model (and scroll) when nothing moved
    function sameIds(a, b) {
        return a.length === b.length && a.every((x, i) => x === b[i]);
    }

    function art(game, kind) {
        const p = game && game.art ? game.art[kind] : null;
        return p ? Paths.url(p) : "";
    }
    function kindIcon(kind) {
        return { steam: "sports_esports", proton: "wine_bar", native: "terminal", hoyo: "auto_awesome", minecraft: "deployed_code" }[kind] ?? "stadia_controller";
    }
    function playtime(seconds) {
        if (!seconds)
            return "";
        const h = seconds / 3600;
        return h >= 10 ? Math.round(h) + " h" : h >= 1 ? h.toFixed(1).replace(/\.0$/, "") + " h" : Math.max(1, Math.round(seconds / 60)) + " min";
    }
    function ago(t) {
        if (!t)
            return "Never played";
        const days = Math.floor((new Date(now * 1000).setHours(24, 0, 0, 0) / 1000 - t) / 86400);
        if (days <= 0)
            return "Today";
        if (days === 1)
            return "Yesterday";
        if (days < 7)
            return days + " days ago";
        if (days < 30)
            return Math.round(days / 7) === 1 ? "Last week" : Math.round(days / 7) + " weeks ago";
        return new Date(t * 1000).toLocaleDateString(Qt.locale(), "d MMM yyyy");
    }
}
