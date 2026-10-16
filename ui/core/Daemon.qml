pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The somelauncherd client (docs/protocol.md). Connects on start and starts the
// daemon itself when nobody answers; reconnects whenever the socket drops.
// No optional calls (`f?.()`) anywhere in the UI: Qt 6.12's compiler segfaults on them.
//   Daemon.call("settings.get", { key: "gameDirs" }, (result, error) => …)
//   Connections { target: Daemon; function onEvent(name, data) { … } }
Singleton {
    id: root

    readonly property int protocol: 1
    readonly property bool connected: sock.connected
    property bool ready: false          // connected and the daemon speaks our protocol
    property string version: ""
    property string problem: ""         // why the daemon is unusable, for the UI

    signal event(string name, var data)

    property int _next: 1
    property var _pending: ({})
    property bool _spawned: false

    function call(method, params, callback) {
        if (!ready) {
            if (callback)
                callback(null, { code: "offline", message: "somelauncherd is not connected" });
            return;
        }
        const id = _next++;
        if (callback)
            _pending[id] = callback;
        sock.write(JSON.stringify(params === undefined ? { id, method } : { id, method, params }) + "\n");
        sock.flush();
    }

    function _receive(line) {
        let msg;
        try {
            msg = JSON.parse(line);
        } catch (e) {
            console.warn("Daemon: bad line", line);
            return;
        }
        if (msg.id !== undefined) {
            const cb = _pending[msg.id];
            delete _pending[msg.id];
            if (cb)
                cb(msg.result ?? null, msg.error ?? null);
        } else if (msg.event === "hello") {
            version = msg.data.version;
            ready = msg.data.protocol === protocol;
            problem = ready ? "" : `somelauncherd ${version} speaks protocol ${msg.data.protocol}, this UI needs ${protocol}`;
        } else {
            event(msg.event, msg.data);
        }
    }

    function _dropped() {
        ready = false;
        // Callers learn their request is gone
        const pending = _pending;
        _pending = {};
        for (const id in pending)
            if (pending[id])
                pending[id](null, { code: "offline", message: "somelauncherd went away" });
        retry.start();
    }

    Socket {
        id: sock
        path: Paths.socket
        Component.onCompleted: connected = true
        onConnectionStateChanged: {
            if (connected) {
                // Each outage may start the daemon once more
                retry.attempt = 0;
                root._spawned = false;
            } else {
                root._dropped();
            }
        }
        onError: if (!connected) root._dropped()
        parser: SplitParser {
            onRead: line => root._receive(line)
        }
    }

    // The first failure of an outage starts the daemon; then back off to 2 s between tries
    Timer {
        id: retry
        property int attempt: 0
        interval: Math.min(2000, 200 * Math.pow(2, attempt))
        onTriggered: {
            if (!root._spawned) {
                root._spawned = true;
                console.info("Daemon: starting", Paths.daemon);
                Quickshell.execDetached([Paths.daemon]);
            }
            attempt++;
            if (attempt > 6 && root.problem === "")
                root.problem = "somelauncherd does not start (" + Paths.daemon + ")";
            sock.connected = true;
        }
    }
}
