import QtQuick
import qs.core

// Play for one game, following its state: Play, Waiting for Steam / Starting,
// Stop while it runs. A Hoyo game also shows its job (Installing 42 %,
// Updating, …), Resume install, and Update when a new version is out (Play is
// refused then). A launch that fails sets `error` (cleared on the next try and
// when the game changes).
PlayButton {
    id: root

    property var game: null
    property string error: ""
    readonly property var state: game ? Library.running[game.id] ?? null : null
    readonly property string st: state ? state.state : ""
    readonly property var job: game ? Library.job(game.id) : null
    readonly property var hoyo: Library.hoyo(game)
    readonly property bool installing: !!hoyo && !!hoyo.installing
    readonly property bool outdated: !!hoyo && !!game.build && game.build.updateAvailable
    // What a click does
    readonly property string action: job ? "" : st !== "" ? "stop" : installing ? "install" : outdated ? "update" : "play"

    label: {
        if (job) {
            // A decimal while it is small: 85 GB moves slowly at first
            const p = job.total > 0 ? job.done * 100 / job.total : -1;
            const pct = p < 0 ? "" : (p < 10 ? p.toFixed(1) : Math.floor(p)) + " %";
            return job.label.split(" ")[0] + (pct ? " " + pct : "");
        }
        if (st === "running")
            return "Stop";
        if (st === "starting")
            return state.step === "steam" ? "Waiting for Steam" : "Starting";
        if (installing)
            return "Resume install";
        if (outdated)
            return "Update to " + game.build.latest;
        return "Play";
    }
    icon: job ? "downloading" : st === "running" ? "stop" : st === "starting" ? "hourglass_top" : installing || outdated ? "download" : "play_arrow"
    progress: job && job.total > 0 ? job.done / job.total : 0
    onGameChanged: error = ""

    function trigger() {
        if (!game || action === "")
            return;
        if (action === "install" || action === "update") {
            error = "";
            const id = game.id;
            Daemon.call(action === "install" ? "hoyo.install" : "hoyo.update", { id }, (r, e) => {
                if (root.game && root.game.id === id)
                    root.error = e ? e.message : "";
            });
            return;
        }
        if (st === "") {
            error = "";
            const id = game.id;
            Library.play(id, e => {
                if (root.game && root.game.id === id)
                    root.error = e ? e.message : "";
            });
        } else {
            Library.stop(game.id);
        }
    }
    onClicked: trigger()
    Connections {
        target: Library
        function onJobFailed(id, error) {
            if (root.game && id === "hoyo-" + root.game.id)
                root.error = error;
        }
    }
}
