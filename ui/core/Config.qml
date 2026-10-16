pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// UI preferences in ~/.config/somelauncher/config.json. Missing keys take the
// defaults below; edits write back, and edits to the file reload live.
// Daemon settings (game folders, API keys) are not here: they go through
// Daemon.call("settings.set").
Singleton {
    id: root

    readonly property alias glass: adapter.glass
    readonly property alias motion: adapter.motion
    readonly property alias ui: adapter.ui
    readonly property alias home: adapter.home
    property bool ready: false

    FileView {
        id: file
        path: Paths.config
        watchChanges: true
        printErrors: false
        // Ignore the change notification for our own write (SomeShell lost
        // several quick `Config.x = …` writes to the reload without this)
        property double lastWrite: 0
        onFileChanged: if (Date.now() - lastWrite > 600) reload()
        onAdapterUpdated: {
            lastWrite = Date.now();
            writeAdapter();
        }
        onLoaded: root.ready = true
        onLoadFailed: error => {
            // First start: write the defaults
            if (error === FileViewError.FileNotFound) {
                lastWrite = Date.now();
                writeAdapter();
            }
            root.ready = true;
        }

        JsonAdapter {
            id: adapter

            property JsonObject glass: JsonObject {
                property bool followShell: true    // use SomeShell's glass tint and rim when it is installed
                property real tint: 0.45           // surface tint over the compositor frost
                property bool rim: true            // 1 px light rim and top highlight
            }
            property JsonObject motion: JsonObject {
                property bool reduce: false
                property real speed: 1.0
            }
            property JsonObject ui: JsonObject {
                property string page: "home"       // the page shown at start (the last one open)
            }
            property JsonObject home: JsonObject {
                property int recentDays: 30        // games not played for this long drop off Home (0: never); Library keeps them
            }
        }
    }
}
