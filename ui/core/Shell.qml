pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland

// SomeShell, when it is installed: its matugen colors, its glass settings and
// its game mode. Everything here is optional; without SomeShell `present` is
// false and the launcher uses its own defaults.
Singleton {
    id: root

    readonly property bool present: shellConfig.loaded
    property var glass: null        // SomeShell's `glass` object from its config.json
    property bool gameMode: false

    FileView {
        id: shellConfig
        property bool loaded: false
        path: Paths.shellConfig
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: {
            try {
                root.glass = JSON.parse(text()).glass ?? null;
                loaded = true;
            } catch (e) {
                console.warn("Shell: bad SomeShell config", e);
            }
        }
    }

    // hypr/modes/gamemode.lua emits `custom>>someshell_gamemode,<0|1>`; a reload
    // resets its Lua state without an event, so read it again then
    Connections {
        target: Hyprland
        function onRawEvent(event) {
            if (event.name === "custom" && event.data.startsWith("someshell_gamemode,"))
                root.gameMode = event.data.endsWith(",1");
            else if (event.name === "configreloaded")
                gameModeRead.running = true;
        }
    }
    Process {
        id: gameModeRead
        running: true
        command: ["hyprctl", "repl", "return GameMode and GameMode.active or false"]
        stdout: StdioCollector {
            onStreamFinished: root.gameMode = text.trim() === "true"
        }
    }
}
