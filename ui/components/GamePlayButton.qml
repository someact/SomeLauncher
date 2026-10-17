import QtQuick
import qs.core

// Play for one game, following its state: Play, Waiting for Steam / Starting,
// Stop while it runs. A launch that fails sets `error` (cleared on the next try
// and when the game changes).
PlayButton {
    id: root

    property var game: null
    property string error: ""
    readonly property var state: game ? Library.running[game.id] ?? null : null
    readonly property string st: state ? state.state : ""

    label: st === "running" ? "Stop" : st === "starting" ? (state.step === "steam" ? "Waiting for Steam" : "Starting") : "Play"
    icon: st === "running" ? "stop" : st === "starting" ? "hourglass_top" : "play_arrow"
    onGameChanged: error = ""

    function trigger() {
        if (!game)
            return;
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
}
