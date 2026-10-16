pragma Singleton

import QtQuick
import Quickshell

// Every file the launcher UI reads or writes, in one place. The daemon's own
// files (library.db, daemon.log) are listed in core/crates/proto `paths`.
Singleton {
    readonly property string home: Quickshell.env("HOME")
    readonly property string configHome: Quickshell.env("XDG_CONFIG_HOME") || home + "/.config"
    readonly property string stateHome: Quickshell.env("XDG_STATE_HOME") || home + "/.local/state"
    readonly property string runtime: Quickshell.env("XDG_RUNTIME_DIR") || "/tmp"

    readonly property string config: configHome + "/somelauncher/config.json"   // UI preferences (core/Config.qml)
    readonly property string colors: stateHome + "/somelauncher/colors.json"     // our own matugen output, used without SomeShell
    readonly property string socket: Quickshell.env("SOMELAUNCHER_SOCKET") || runtime + "/somelauncher.sock"
    // The daemon binary; `scripts/dev.sh` points it at the debug build
    readonly property string daemon: Quickshell.env("SOMELAUNCHER_DAEMON") || "somelauncherd"

    // SomeShell, read only and only when it is installed (core/Shell.qml)
    readonly property string shellConfig: configHome + "/someshell/config.json"
    readonly property string shellColors: stateHome + "/someshell/colors.json"

    function url(path) {
        return path.startsWith("file://") ? path : "file://" + path;
    }
}
