import QtQuick
import QtQuick.Dialogs
import Quickshell
import qs.core

// A game's settings, a glass sheet that slides over the game page from the
// right. Simple holds what most games need, each one click away: the runner,
// Proton, Needs Steam, gamescope, MangoHud and GameMode. Advanced holds the
// rest: paths, arguments, env, DLL overrides, scripts, the match, logs and
// removal. Every change is saved at once (games.update), nothing to apply.
Item {
    id: root

    property var game: null
    property bool open: false
    property string tab: "simple"
    signal closeRequested

    readonly property var runner: game && game.runner ? game.runner : ({})
    readonly property bool wine: !!game && (game.kind === "proton" || game.kind === "hoyo")
    readonly property bool wrapped: !!game && (wine || game.kind === "native")
    // What applies: the game's own gamescope settings replace the default's whole
    readonly property var gamescope: runner.gamescope ?? defaults.gamescope ?? ({ enabled: false })

    // From the daemon, fetched each time the sheet opens
    property var defaults: ({})
    property var protons: ({ builds: [], default: "" })
    property bool desktopEntry: false
    property var logs: []
    property var plan: null
    property var monitor: null               // where games start (status.monitor)
    property string error: ""

    function load() {
        error = "";
        plan = null;
        Daemon.call("settings.get", { key: "defaults" }, r => root.defaults = Object.assign({ mangohud: false, gamemode: true, gamescope: { enabled: false } }, r ?? {}));
        Daemon.call("status", undefined, r => root.monitor = r ? r.monitor ?? null : null);
        Daemon.call("proton.list", undefined, r => {
            if (r)
                root.protons = r;
        });
        if (!game)
            return;
        Daemon.call("games.desktop", { id: game.id }, r => root.desktopEntry = !!r && r.enabled);
        Daemon.call("games.logs", { id: game.id }, r => root.logs = r ?? []);
    }
    onOpenChanged: if (open) load()

    function update(fields) {
        if (game)
            Library.update(game.id, fields, e => root.error = e ? e.message : "");
    }
    // `runner` is replaced whole; a null in the patch removes the key (the default again)
    function setRunner(patch) {
        const r = Object.assign({}, runner);
        for (const k in patch) {
            if (patch[k] === null || patch[k] === undefined)
                delete r[k];
            else
                r[k] = patch[k];
        }
        update({ runner: r });
    }

    // Arguments as one line: quotes keep spaces together
    function splitArgs(s) {
        const out = [];
        const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
        let m;
        while ((m = re.exec(s)) !== null)
            out.push(m[1] ?? m[2] ?? m[3]);
        return out;
    }
    function joinArgs(a) {
        return (a ?? []).map(x => x === "" || /[\s"']/.test(x) ? `"${x}"` : x).join(" ");
    }
    function onOff(b) {
        return b ? "on" : "off";
    }

    Keys.onEscapePressed: closeRequested()

    // The page beside the sheet dims; a click there closes it
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
        // Over bright art the frost alone is not enough: nearly opaque
        color: Qt.rgba(Theme.surface.r, Theme.surface.g, Theme.surface.b, 0.94)
        border.width: 1
        border.color: Theme.glassRim

        MouseArea {
            anchors.fill: parent    // clicks on the sheet stay on it
        }

        // Header: the name, Simple/Advanced and close
        Item {
            id: header
            x: Theme.space.xl
            y: Theme.space.xl
            width: parent.width - Theme.space.xl * 2
            height: 44
            Column {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - close.width - Theme.space.m
                SText {
                    width: parent.width
                    text: "Settings"
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
                id: close
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                icon: "close"
                onClicked: root.closeRequested()
            }
        }
        Segmented {
            id: tabs
            x: Theme.space.xl
            anchors.top: header.bottom
            anchors.topMargin: Theme.space.l
            options: [{ id: "simple", name: "Simple" }, { id: "advanced", name: "Advanced" }]
            current: root.tab
            onPicked: id => root.tab = id
        }
        SText {
            anchors.left: tabs.right
            anchors.leftMargin: Theme.space.l
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.xl
            anchors.verticalCenter: tabs.verticalCenter
            visible: root.error !== ""
            text: root.error
            color: Theme.error
            font.pixelSize: Theme.font.small
            wrapMode: Text.Wrap
            maximumLineCount: 2
        }

        Flickable {
            id: flick
            anchors.top: tabs.bottom
            anchors.topMargin: Theme.space.l
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.space.m
            x: Theme.space.xl
            width: parent.width - Theme.space.xl * 2
            clip: true
            contentHeight: (root.tab === "simple" ? simple.implicitHeight : advanced.implicitHeight) + Theme.space.xl
            boundsBehavior: Flickable.StopAtBounds
            onContentHeightChanged: if (contentY > Math.max(0, contentHeight - height)) contentY = Math.max(0, contentHeight - height)

            // Simple
            Column {
                id: simple
                width: flick.width
                visible: root.tab === "simple"
                spacing: Theme.space.xl

                Section {
                    title: "Runner"
                    SettingRow {
                        visible: !!root.game && (root.game.kind === "proton" || root.game.kind === "native")
                        title: "Runs as"
                        note: "A Windows game runs through Proton (umu-run), a Linux one as it is"
                        Segmented {
                            options: [{ id: "proton", name: "Windows" }, { id: "native", name: "Linux" }]
                            current: root.game ? root.game.kind : ""
                            onPicked: id => root.update({ kind: id })
                        }
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind === "steam"
                        title: "Runs through Steam"
                        note: {
                            const s = root.game ? root.game.sources.find(s => s.source === "steam") : null;
                            const p = s && s.data ? s.data.proton : null;
                            return "Steam keeps its own Proton and launch options" + (p ? ` (this game: ${p})` : "") + ". Arguments below are added.";
                        }
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind === "hoyo"
                        title: "HoYoverse game"
                        note: "Runs through Proton. Install, update and repair come with Phase 6."
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind === "minecraft"
                        title: "Prism instance"
                        note: "Its settings live in Prism for now (Phase 8 brings them here)."
                    }
                    SettingRow {
                        visible: root.wine
                        below: true
                        title: "Proton"
                        note: "The build this game runs on"
                        Select {
                            width: parent.width
                            readonly property var build: root.protons.builds.find(b => b.name === root.runner.proton || b.dir === root.runner.proton) ?? null
                            current: !root.runner.proton ? "" : build ? build.name : root.runner.proton
                            options: {
                                const o = [{ id: "", name: "Default", note: (root.protons.default || "the newest GE-Proton") }];
                                for (const b of root.protons.builds)
                                    o.push({ id: b.name, name: b.name, note: { user: "Installed", system: "System", steam: "Steam", lutris: "Lutris", twintail: "Twintail" }[b.source] ?? b.source });
                                if (root.runner.proton && !build)
                                    o.push({ id: root.runner.proton, name: root.runner.proton.split("/").pop(), note: "Not found" });
                                return o;
                            }
                            onPicked: id => root.setRunner({ proton: id === "" ? null : id })
                        }
                    }
                    SettingRow {
                        visible: root.wine
                        title: "Needs Steam"
                        note: "For co-op that needs the real Steam client (OnlineFix, gbe_fork): Steam starts first and the prefix gets its files"
                        Switch {
                            checked: !!root.runner.needsSteam
                            onToggled: on => root.setRunner({ needsSteam: on ? true : null })
                        }
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind === "steam"
                        below: true
                        title: "Arguments"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.joinArgs(root.runner.args)
                            placeholder: "-dx12 -skipintro"
                            onCommitted: text => root.setRunner({ args: text.trim() === "" ? null : root.splitArgs(text) })
                        }
                    }
                }

                Section {
                    visible: root.wrapped
                    title: "Performance and overlay"
                    WrapperRow {
                        key: "mangohud"
                        title: "MangoHud"
                        what: "FPS and frame-time overlay"
                    }
                    WrapperRow {
                        key: "gamemode"
                        title: "GameMode"
                        what: "gamemoderun: CPU governor and priority while playing"
                    }
                    SettingRow {
                        title: "Gamescope"
                        note: "Runs the game in its own compositor: upscaling, a fixed size and refresh, HDR. " + (root.runner.gamescope ? "Set for this game" : "Default: " + root.onOff(!!root.gamescope.enabled))
                        Row {
                            spacing: Theme.space.s
                            ResetButton {
                                visible: !!root.runner.gamescope
                                onClicked: root.setRunner({ gamescope: null })
                            }
                            Switch {
                                anchors.verticalCenter: parent.verticalCenter
                                checked: !!root.gamescope.enabled
                                onToggled: on => root.setRunner({ gamescope: Object.assign({}, root.gamescope, { enabled: on }) })
                            }
                        }
                    }
                    GamescopeEditor {
                        width: parent.width
                        monitor: root.monitor
                        visible: !!root.gamescope.enabled
                        value: root.gamescope
                        onEdited: v => root.setRunner({ gamescope: v })
                    }
                    SettingRow {
                        below: true
                        title: "FPS cap"
                        note: {
                            const d = root.defaults.fpsCap;
                            return "Default: " + (d === "monitor" ? "the monitor's refresh" : d ? d + " fps" : "none");
                        }
                        FpsCapPicker {
                            allowDefault: true
                            value: root.runner.fpsCap ?? null
                            refresh: root.monitor ? root.monitor.refresh : 0
                            onEdited: v => root.setRunner({ fpsCap: v })
                        }
                    }
                }

                SText {
                    width: parent.width
                    visible: !!root.game && root.game.kind === "minecraft"
                    text: "Nothing to set here yet."
                    dim: true
                }
            }

            // Advanced
            Column {
                id: advanced
                width: flick.width
                visible: root.tab === "advanced"
                spacing: Theme.space.xl

                Section {
                    title: "Game"
                    SettingRow {
                        below: true
                        title: "Name"
                        Field {
                            width: parent.width
                            value: root.game ? root.game.name : ""
                            onCommitted: text => {
                                if (text.trim() !== "")
                                    root.update({ name: text.trim() });
                            }
                        }
                    }
                    SettingRow {
                        below: true
                        title: "Categories"
                        note: "Library rows, separated by commas; empty uses the store's (" + ((root.game && root.game.meta && root.game.meta.categories || []).join(", ") || "none") + ")"
                        Field {
                            width: parent.width
                            value: root.game ? root.game.categories.join(", ") : ""
                            placeholder: "Co-op, Favourites of 2026"
                            onCommitted: text => root.update({ categories: text.split(",").map(c => c.trim()).filter(c => c !== "") })
                        }
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind !== "steam"
                        below: true
                        title: "Build"
                        note: "The version you have installed, noted by hand (Steam games are checked for you)"
                        Field {
                            width: parent.width
                            value: root.game && root.game.build ? root.game.build.recorded ?? "" : ""
                            placeholder: "1.0.3"
                            onCommitted: text => root.update({ recordedBuild: text.trim() === "" ? null : text.trim() })
                        }
                    }
                    SettingRow {
                        title: "Match"
                        note: root.game && root.game.meta ? `${root.game.meta.name} (${root.game.meta.source === "steam" ? "Steam " + root.game.meta.sourceId : "SteamGridDB"})` : "Not matched: no store details or automatic art"
                        PressButton {
                            id: change
                            width: 96
                            height: 36
                            radius: 18
                            color: Theme.glassRaised
                            active: matcher.visible
                            activeColor: Theme.secondaryContainer
                            onClicked: matcher.visible = !matcher.visible
                            SText {
                                anchors.centerIn: parent
                                text: matcher.visible ? "Done" : "Change"
                                color: change.active ? Theme.fgSecondaryContainer : Theme.fgSurface
                            }
                        }
                    }
                    Loader {
                        id: matcher
                        width: parent.width
                        visible: false
                        active: visible
                        sourceComponent: MatchPicker {
                            width: matcher.width
                            gameId: root.game ? root.game.id : -1
                            onMatched: matcher.visible = false
                        }
                    }
                    SettingRow {
                        below: true
                        title: "Art"
                        note: "Replace any image with your own file; the reset button goes back to the automatic one"
                        Flow {
                            width: parent.width
                            spacing: Theme.space.s
                            Repeater {
                                model: [{ id: "hero", name: "Background" }, { id: "grid", name: "Cover" }, { id: "logo", name: "Logo" }, { id: "icon", name: "Icon" }]
                                ArtSlot {
                                    required property var modelData
                                    kind: modelData.id
                                    label: modelData.name
                                }
                            }
                        }
                    }
                }

                Section {
                    visible: !!root.game && root.game.kind !== "minecraft"
                    title: "Launch"
                    PathRow {
                        visible: !!root.game && root.game.kind !== "steam"
                        title: "Executable"
                        field: "exe"
                    }
                    SettingRow {
                        visible: !!root.game && root.game.kind !== "steam"
                        below: true
                        title: "Arguments"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.joinArgs(root.runner.args)
                            placeholder: "-dx12 -skipintro"
                            onCommitted: text => root.setRunner({ args: text.trim() === "" ? null : root.splitArgs(text) })
                        }
                    }
                    PathRow {
                        visible: !!root.game && root.game.kind !== "steam"
                        title: "Working folder"
                        note: "Empty: the executable's folder"
                        field: "workingDir"
                        folder: true
                        inRunner: true
                    }
                    PathRow {
                        visible: root.wine
                        title: "Wine prefix"
                        note: "Empty: made on the first launch under the prefix folder"
                        field: "prefix"
                        folder: true
                    }
                    SettingRow {
                        visible: root.wrapped
                        below: true
                        title: "Environment"
                        KeyValueList {
                            width: parent.width
                            value: root.runner.env ?? {}
                            keyPlaceholder: "VARIABLE"
                            onEdited: o => root.setRunner({ env: Object.keys(o).length ? o : null })
                        }
                    }
                    SettingRow {
                        visible: root.wine
                        below: true
                        title: "DLL overrides"
                        note: "n,b: the game's own DLL first, then Wine's"
                        KeyValueList {
                            width: parent.width
                            value: root.runner.dllOverrides ?? {}
                            keyPlaceholder: "dll"
                            valuePlaceholder: "n,b"
                            onEdited: o => root.setRunner({ dllOverrides: Object.keys(o).length ? o : null })
                        }
                    }
                    SettingRow {
                        visible: root.wine
                        below: true
                        title: "umu game id"
                        note: "GAMEID for protonfixes, e.g. umu-1145360; empty: umu-0"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.runner.gameId ?? ""
                            placeholder: "umu-0"
                            onCommitted: text => root.setRunner({ gameId: text.trim() || null })
                        }
                    }
                    SettingRow {
                        visible: root.wine && !!root.runner.needsSteam
                        below: true
                        title: "Steam app id"
                        note: "Written to steam_appid.txt; empty: OnlineFix's, else 480"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.runner.steamAppId ?? ""
                            placeholder: "480"
                            onCommitted: text => root.setRunner({ steamAppId: text.trim() || null })
                        }
                    }
                    SettingRow {
                        visible: root.wrapped
                        title: "Discrete GPU"
                        note: "On a hybrid-GPU laptop; default: on there"
                        Segmented {
                            options: [{ id: "default", name: "Default" }, { id: "on", name: "On" }, { id: "off", name: "Off" }]
                            current: root.runner.prime === undefined || root.runner.prime === null ? "default" : root.runner.prime ? "on" : "off"
                            onPicked: id => root.setRunner({ prime: id === "default" ? null : id === "on" })
                        }
                    }
                }

                Section {
                    visible: root.wrapped
                    title: "Scripts"
                    SettingRow {
                        below: true
                        title: "Before launch"
                        note: "A shell command with the game's env; if it fails the game does not start"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.runner.preLaunch ?? ""
                            onCommitted: text => root.setRunner({ preLaunch: text.trim() || null })
                        }
                    }
                    SettingRow {
                        below: true
                        title: "After exit"
                        Field {
                            width: parent.width
                            mono: true
                            value: root.runner.postExit ?? ""
                            onCommitted: text => root.setRunner({ postExit: text.trim() || null })
                        }
                    }
                }

                Section {
                    title: "Logs and launch"
                    SettingRow {
                        visible: !!root.game && root.game.kind !== "minecraft"
                        title: "Command"
                        note: "What Play runs, without running it"
                        SmallButton {
                            text: root.plan ? "Hide" : "Show"
                            onClicked: {
                                if (root.plan) {
                                    root.plan = null;
                                    return;
                                }
                                Daemon.call("games.plan", { id: root.game.id }, (r, e) => root.plan = e ? { error: e.message } : r);
                            }
                        }
                    }
                    Rectangle {
                        width: parent.width
                        visible: root.plan !== null
                        height: planText.implicitHeight + Theme.space.m * 2
                        radius: Theme.radius.small + 2
                        color: Theme.surfaceLow
                        TextEdit {
                            id: planText
                            x: Theme.space.m
                            y: Theme.space.m
                            width: parent.width - Theme.space.m * 2
                            readOnly: true
                            selectByMouse: true
                            wrapMode: TextEdit.WrapAnywhere
                            color: root.plan && root.plan.error ? Theme.error : Theme.fgSurface
                            selectionColor: Theme.primaryContainer
                            font.family: Theme.font.mono
                            font.pixelSize: Theme.font.small
                            text: {
                                const p = root.plan;
                                if (!p)
                                    return "";
                                if (p.error)
                                    return p.error;
                                const env = Object.keys(p.env ?? {}).map(k => `${k}=${p.env[k]}`);
                                return [...env, p.argv.join(" "), p.cwd ? "in " + p.cwd : ""].filter(l => l).join("\n");
                            }
                        }
                    }
                    SettingRow {
                        title: "Logs"
                        note: root.logs.length ? "The last " + root.logs.length + " runs" : "Nothing logged yet"
                    }
                    Repeater {
                        model: root.logs
                        SettingRow {
                            id: logRow
                            required property var modelData
                            title: new Date(modelData.time * 1000).toLocaleString(Qt.locale(), "d MMM yyyy, HH:mm")
                            note: Math.max(1, Math.round(modelData.size / 1024)) + " KB"
                            SmallButton {
                                text: "Open"
                                onClicked: Quickshell.execDetached(["xdg-open", logRow.modelData.path])
                            }
                        }
                    }
                }

                Section {
                    title: "Library"
                    SettingRow {
                        visible: !!root.game && root.game.kind !== "minecraft"
                        title: "Desktop entry"
                        note: "Show the game in your app launcher"
                        Switch {
                            checked: root.desktopEntry
                            onToggled: on => Daemon.call("games.desktop", { id: root.game.id, enabled: on }, (r, e) => {
                                if (r)
                                    root.desktopEntry = r.enabled;
                                root.error = e ? e.message : "";
                            })
                        }
                    }
                    SettingRow {
                        title: "Hidden"
                        note: "Kept out of Home and Library; the Hidden filter shows it"
                        Switch {
                            checked: !!root.game && root.game.hidden
                            onToggled: on => root.update({ hidden: on })
                        }
                    }
                    SettingRow {
                        title: "Remove from library"
                        note: "The game's files stay where they are"
                        PressButton {
                            id: remove
                            property bool armed: false
                            width: label.implicitWidth + Theme.space.xl * 2
                            height: 36
                            radius: 18
                            color: armed ? Theme.error : Qt.alpha(Theme.error, 0.16)
                            hoverColor: armed ? Theme.error : Qt.alpha(Theme.error, 0.26)
                            onClicked: {
                                if (!armed) {
                                    armed = true;
                                    disarm.restart();
                                    return;
                                }
                                Daemon.call("games.remove", { id: root.game.id }, (r, e) => {
                                    if (e)
                                        root.error = e.message;
                                    else
                                        Nav.closeGame();
                                });
                            }
                            Timer {
                                id: disarm
                                interval: 4000
                                onTriggered: remove.armed = false
                            }
                            SText {
                                id: label
                                anchors.centerIn: parent
                                text: remove.armed ? "Click again to remove" : "Remove"
                                color: remove.armed ? Theme.surface : Theme.error
                                font.weight: Theme.font.weightTitle
                            }
                        }
                    }
                }
            }
        }
    }

    // A titled group of settings
    component Section: Column {
        property string title
        width: parent ? parent.width : 0
        spacing: Theme.space.l
        SText {
            text: parent.title
            color: Theme.primary
            font.weight: Theme.font.weightTitle
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
            font.pixelSize: Theme.font.small
        }
    }

    // On, off, or the default: a switch showing what applies and a reset once the game sets its own
    component WrapperRow: SettingRow {
        id: wrapper
        property string key
        property string what
        readonly property bool own: root.runner[key] !== undefined && root.runner[key] !== null
        readonly property bool on: own ? !!root.runner[key] : !!root.defaults[key]
        title: "MangoHud"
        note: what + ". " + (own ? "Set for this game" : "Default: " + root.onOff(!!root.defaults[key]))
        Row {
            spacing: Theme.space.s
            ResetButton {
                visible: wrapper.own
                onClicked: root.setRunner({ [wrapper.key]: null })
            }
            Switch {
                anchors.verticalCenter: parent.verticalCenter
                checked: wrapper.on
                onToggled: on => root.setRunner({ [wrapper.key]: on })
            }
        }
    }

    component ResetButton: IconButton {
        anchors.verticalCenter: parent ? parent.verticalCenter : undefined
        width: 32
        height: 32
        icon: "restart_alt"
        iconSize: 18
        color: "transparent"
    }

    component SmallButton: PressButton {
        id: small
        property string text
        width: smallLabel.implicitWidth + Theme.space.xl * 2
        height: 36
        radius: 18
        color: Theme.glassRaised
        SText {
            id: smallLabel
            anchors.centerIn: parent
            text: small.text
        }
    }

    // A path: typed, or picked with the file dialog
    component PathRow: SettingRow {
        id: pathRow
        property string field
        property bool folder: false
        property bool inRunner: false     // runner.workingDir rather than a game field
        readonly property string current: root.game ? ((inRunner ? root.runner[field] : root.game[field]) ?? "") : ""
        below: true
        function set(path) {
            const v = path.trim() === "" ? null : path.trim();
            if (inRunner)
                root.setRunner({ [field]: v });
            else
                root.update({ [field]: v });
        }
        Row {
            width: parent.width
            spacing: Theme.space.s
            Field {
                width: parent.width - 40 - Theme.space.s
                mono: true
                value: pathRow.current
                onCommitted: text => pathRow.set(text)
            }
            IconButton {
                width: 40
                height: 40
                icon: pathRow.folder ? "folder_open" : "draft"
                iconSize: 20
                onClicked: {
                    const start = pathRow.current || (root.game ? root.game.installDir ?? "" : "");
                    if (pathRow.folder)
                        Pick.folder(start, p => pathRow.set(p));
                    else
                        Pick.file(start, p => pathRow.set(p));
                }
            }
        }
    }

    // One art kind: its image, replace from a file, back to automatic
    component ArtSlot: Column {
        id: slot
        property string kind
        property string label
        readonly property string src: Library.art(root.game, kind)
        spacing: Theme.space.xs
        Rectangle {
            width: slot.kind === "grid" ? 80 : slot.kind === "icon" || slot.kind === "logo" ? 96 : 192
            height: slot.kind === "grid" ? 120 : 108
            radius: Theme.radius.small + 2
            color: Theme.surfaceLow
            border.width: 1
            border.color: Theme.glassRim
            clip: true
            Image {
                anchors.fill: parent
                anchors.margins: slot.kind === "logo" || slot.kind === "icon" ? Theme.space.s : 1
                source: slot.src
                sourceSize: Qt.size(width * 2, height * 2)
                fillMode: slot.kind === "logo" || slot.kind === "icon" ? Image.PreserveAspectFit : Image.PreserveAspectCrop
                asynchronous: true
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: Pick.file("", p => Daemon.call("art.set", { id: root.game.id, kind: slot.kind, origin: p }, (g, e) => root.error = e ? e.message : ""), "Images (*.png *.jpg *.jpeg *.webp)")
            }
        }
        Row {
            spacing: 2
            SText {
                anchors.verticalCenter: parent.verticalCenter
                text: slot.label
                dim: true
                font.pixelSize: Theme.font.small
            }
            IconButton {
                width: 24
                height: 24
                icon: "restart_alt"
                iconSize: 16
                color: "transparent"
                onClicked: Daemon.call("art.set", { id: root.game.id, kind: slot.kind }, (g, e) => root.error = e ? e.message : "")
            }
        }
    }
}
