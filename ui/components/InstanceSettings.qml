import QtQuick
import qs.core

// A Minecraft instance's Settings tab: Prism's own settings for it, in its
// `instance.cfg` (`minecraft.settings`/`setSettings`). As in Prism each group
// either follows Prism's defaults or is the instance's own: changing a value
// makes the group its own, the reset button hands it back. The common ones
// first (memory, Java, a few switches); Advanced holds JVM arguments, the
// window size and Prism's launch commands.
// Prism keeps these in memory while it runs, so a running Prism is pointed out
// (and can be closed: it reads them again on its next start).
Flickable {
    id: root

    property var game: null
    property bool active: false

    property var values: ({})        // what instance.cfg sets
    property var defaults: ({})      // Prism's (prismlauncher.cfg)
    property var javas: []
    property int ramMb: 8192
    property bool prismRunning: false
    property string error: ""
    property bool advanced: false

    readonly property var groups: ({
        memory: "OverrideMemory",
        java: "OverrideJavaLocation",
        args: "OverrideJavaArgs",
        window: "OverrideWindow",
        performance: "OverridePerformance",
        commands: "OverrideCommands"
    })
    function own(group) {
        return values[groups[group]] === true;
    }
    // What applies: the instance's value when its group is its own, else Prism's
    function get(group, key) {
        const v = own(group) ? values[key] : undefined;
        return v !== undefined ? v : defaults[key];
    }

    function apply(r, e) {
        root.error = e ? e.message : "";
        if (!r)
            return;
        root.values = r.values;
        root.defaults = r.defaults;
        root.javas = r.javas;
        root.ramMb = r.ramMb ?? 8192;
        root.prismRunning = r.prismRunning;
    }
    function load() {
        if (!game)
            return;
        const id = game.id;
        Daemon.call("minecraft.settings", { id }, (r, e) => {
            if (root.game && root.game.id === id)
                root.apply(r, e);
        });
    }
    // Sets keys of one group, making it the instance's own
    function set(group, changes) {
        const id = game.id;
        const v = Object.assign({ [groups[group]]: true }, changes);
        Daemon.call("minecraft.setSettings", { id, values: v }, (r, e) => {
            if (root.game && root.game.id === id)
                root.apply(r, e);
        });
    }
    function reset(group) {
        const id = game.id;
        Daemon.call("minecraft.setSettings", { id, values: { [groups[group]]: false } }, (r, e) => {
            if (root.game && root.game.id === id)
                root.apply(r, e);
        });
    }
    onActiveChanged: if (active) load()
    onGameChanged: {
        values = {};
        contentY = 0;
        if (active)
            load();
    }

    function gb(mb) {
        return mb >= 1024 ? (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1) + " GB" : mb + " MB";
    }

    contentHeight: col.height + Theme.space.xl
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    Column {
        id: col
        width: Math.min(root.width, 860)
        spacing: Theme.space.xl

        // Prism open, or an error
        Rectangle {
            width: parent.width
            height: visible ? noteRow.height + Theme.space.m * 2 : 0
            visible: root.prismRunning || root.error !== ""
            radius: Theme.radius.normal
            color: root.error !== "" ? Qt.alpha(Theme.error, 0.12) : Theme.glassRaised
            Row {
                id: noteRow
                x: Theme.space.m
                y: Theme.space.m
                width: parent.width - Theme.space.m * 2
                spacing: Theme.space.m
                Icon {
                    id: noteIcon
                    anchors.verticalCenter: parent.verticalCenter
                    name: root.error !== "" ? "error" : "info"
                    size: 20
                    color: root.error !== "" ? Theme.error : Theme.primary
                }
                SText {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - noteIcon.width - (closePrism.visible ? closePrism.width + parent.spacing : 0) - parent.spacing
                    wrapMode: Text.Wrap
                    maximumLineCount: 3
                    text: root.error !== "" ? root.error : "Prism is open: it keeps its own copy of these until it restarts, and a change made in it can write over them."
                }
                TextButton {
                    id: closePrism
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.error === "" && root.prismRunning
                    text: "Close Prism"
                    onClicked: Daemon.call("minecraft.closePrism", {}, (r, e) => {
                        root.error = e ? e.message : "";
                        closeCheck.restart();
                    })
                }
            }
            Timer {
                id: closeCheck
                interval: 1000
                onTriggered: root.load()
            }
        }

        Section {
            title: "Memory"
            group: "memory"
            SettingRow {
                id: memRow
                below: true
                title: "Maximum memory"
                readonly property int max: root.get("memory", "MaxMemAlloc") ?? 4096
                // While dragging: the slider's value, written a moment after it stops
                property int preview: -1
                readonly property int shown: preview >= 0 ? preview : max
                note: (root.own("memory") ? "Set for this instance" : "Prism's default") + (shown > root.ramMb * 0.75 ? ". More than ¾ of this PC's " + root.gb(root.ramMb) + ": the system may run short" : "")
                Slider {
                    width: parent.width
                    from: 1024
                    to: Math.max(2048, Math.floor(root.ramMb / 512) * 512)
                    step: 512
                    value: memRow.shown
                    label: root.gb(memRow.shown)
                    onMoved: v => {
                        memRow.preview = v;
                        memCommit.restart();
                    }
                }
                Timer {
                    id: memCommit
                    interval: 450
                    onTriggered: {
                        const v = memRow.preview;
                        root.set("memory", { MaxMemAlloc: v, MinMemAlloc: Math.min(root.get("memory", "MinMemAlloc") ?? 512, v) });
                        memRow.preview = -1;
                    }
                }
            }
        }

        Section {
            title: "Java"
            group: "java"
            SettingRow {
                below: true
                title: "Java runtime"
                note: root.javaAuto ? "Prism picks the Java this Minecraft version needs and downloads it when missing" + (root.values.JavaPath ? ". Now: " + root.javaName(root.values.JavaPath) : "") : "Always this one"
                Select {
                    width: Math.min(parent.width, 420)
                    maxRows: 7
                    options: [{ id: "auto", name: "Automatic", note: "Prism's choice per version" }].concat(root.javas.map(j => ({ id: j.path, name: "Java " + (j.version ?? "?"), note: (j.source === "prism" ? "Prism's · " : "") + j.path })))
                    current: root.javaAuto ? "auto" : root.get("java", "JavaPath")
                    onPicked: id => root.set("java", id === "auto" ? { AutomaticJava: true } : { AutomaticJava: false, JavaPath: id })
                }
            }
        }

        Section {
            title: "Performance"
            group: "performance"
            Toggle {
                group: "performance"
                key: "EnableFeralGamemode"
                title: "GameMode"
                note: "Feral GameMode while playing (SomeShell's game mode follows it)"
            }
            Toggle {
                group: "performance"
                key: "EnableMangoHud"
                title: "MangoHud"
                note: "FPS and frame time overlay"
            }
            Toggle {
                group: "performance"
                key: "UseDiscreteGpu"
                title: "Discrete GPU"
                note: "On a laptop with two GPUs, run on the faster one"
            }
            Toggle {
                group: "performance"
                key: "UseZink"
                visible: root.advanced
                title: "Zink"
                note: "OpenGL over Vulkan; can help on some drivers"
            }
        }

        Section {
            title: "Window"
            group: "window"
            Toggle {
                group: "window"
                key: "LaunchMaximized"
                title: "Start maximized"
                note: "Else the window size under Advanced"
            }
            SettingRow {
                visible: root.advanced
                title: "Window size"
                note: "Width × height when not maximized"
                Row {
                    spacing: Theme.space.s
                    Field {
                        width: 84
                        value: String(root.get("window", "MinecraftWinWidth") ?? 854)
                        onCommitted: t => {
                            const n = parseInt(t);
                            if (n > 0)
                                root.set("window", { MinecraftWinWidth: n });
                        }
                    }
                    SText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: "×"
                        dim: true
                    }
                    Field {
                        width: 84
                        value: String(root.get("window", "MinecraftWinHeight") ?? 480)
                        onCommitted: t => {
                            const n = parseInt(t);
                            if (n > 0)
                                root.set("window", { MinecraftWinHeight: n });
                        }
                    }
                }
            }
        }

        PressButton {
            width: advText.implicitWidth + Theme.space.xl * 2
            height: 36
            color: Theme.glassRaised
            Row {
                anchors.centerIn: parent
                spacing: Theme.space.s
                SText {
                    id: advText
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.advanced ? "Fewer settings" : "Advanced"
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "expand_more"
                    size: 18
                    rotation: root.advanced ? 180 : 0
                }
            }
            onClicked: root.advanced = !root.advanced
        }

        Section {
            visible: root.advanced
            title: "Advanced"
            group: ""
            SettingRow {
                below: true
                title: "Minimum memory"
                note: "What Java takes at the start (-Xms)"
                Field {
                    width: 120
                    value: String(root.get("memory", "MinMemAlloc") ?? 512)
                    onCommitted: t => {
                        const n = parseInt(t);
                        if (n > 0)
                            root.set("memory", { MinMemAlloc: n, MaxMemAlloc: Math.max(n, root.get("memory", "MaxMemAlloc") ?? n) });
                    }
                }
            }
            Line {
                group: "args"
                key: "JvmArgs"
                title: "JVM arguments"
                note: "Added to Java's command line, e.g. -XX:+UseZGC"
            }
            Line {
                group: "commands"
                key: "WrapperCommand"
                title: "Wrapper command"
                note: "Runs the game through it, e.g. prime-run"
            }
            Line {
                group: "commands"
                key: "PreLaunchCommand"
                title: "Before launch"
                note: "A command Prism runs before the game"
            }
            Line {
                group: "commands"
                key: "PostExitCommand"
                title: "After exit"
                note: "A command Prism runs after the game"
            }
            SettingRow {
                title: "Everything else"
                note: "Prism's other settings for this instance (console, native libraries, environment)"
                TextButton {
                    text: "Open in Prism"
                    icon: "open_in_new"
                    onClicked: if (root.game) Daemon.call("minecraft.prism", { id: root.game.id }, undefined)
                }
            }
        }
    }

    // Prism picks the Java: its automatic switch, or no path chosen at all
    readonly property bool javaAuto: !!get("java", "AutomaticJava") || !get("java", "JavaPath")
    function javaName(path) {
        const j = javas.find(j => j.path === path);
        return j ? "Java " + (j.version ?? "?") : path;
    }

    // A titled group of rows; with `group`, a reset button once the instance sets its own
    component Section: Column {
        id: section
        property string title
        property string group
        default property alias rows: body.data
        width: parent ? parent.width : 0
        spacing: Theme.space.m
        Item {
            width: parent.width
            height: 32
            SText {
                anchors.verticalCenter: parent.verticalCenter
                text: section.title
                color: Theme.primary
                font.weight: Theme.font.weightTitle
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 1
                font.pixelSize: Theme.font.small
            }
            PressButton {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                visible: section.group !== "" && root.own(section.group)
                width: resetRow.implicitWidth + Theme.space.l * 2
                height: 32
                color: Theme.glassRaised
                Row {
                    id: resetRow
                    anchors.centerIn: parent
                    spacing: Theme.space.xs
                    Icon {
                        anchors.verticalCenter: parent.verticalCenter
                        name: "restart_alt"
                        size: 16
                        color: Theme.fgSurfaceVariant
                    }
                    SText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Use Prism's defaults"
                        dim: true
                        font.pixelSize: Theme.font.small
                    }
                }
                onClicked: root.reset(section.group)
            }
        }
        Column {
            id: body
            width: parent.width
            spacing: Theme.space.l
        }
    }

    component Toggle: SettingRow {
        id: toggle
        property string group
        property string key
        Switch {
            checked: !!root.get(toggle.group, toggle.key)
            onToggled: on => root.set(toggle.group, { [toggle.key]: on })
        }
    }

    // A command line or arguments: what applies, edited in place
    component Line: SettingRow {
        id: line
        property string group
        property string key
        below: true
        Field {
            width: parent.width
            mono: true
            placeholder: root.own(line.group) ? "None" : "Prism's default: " + (root.defaults[line.key] || "none")
            value: root.own(line.group) ? (root.values[line.key] ?? "") : ""
            onCommitted: t => root.set(line.group, { [line.key]: t.trim() })
        }
    }
}
