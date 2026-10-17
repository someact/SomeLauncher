import QtQuick
import Quickshell
import qs.core
import qs.components

// Settings: the library (game folders, imports, checks), keys, the launch
// defaults every game starts from (this machine's profile), Proton builds,
// the looks, and the daemon. Daemon settings go through settings.set; the
// looks are config.json (Config). Every change applies at once.
Flickable {
    id: root

    contentHeight: col.implicitHeight + Theme.space.xxl * 2
    boundsBehavior: Flickable.StopAtBounds

    property var settings: ({})
    property var status: null
    property var protons: ({ builds: [], default: "" })
    property var power: null                 // power.status: profiles, battery, gaming
    property var available: []               // GE-Proton and Proton-CachyOS releases to download
    property var downloads: ({})             // id → {label, done, total}
    property string message: ""              // the last job's outcome
    property bool messageError: false
    readonly property var defaults: settings.defaults ?? {}

    function refresh() {
        Daemon.call("settings.get", undefined, r => root.settings = r ?? {});
        Daemon.call("status", undefined, r => root.status = r);
        Daemon.call("power.status", undefined, r => root.power = r);
        loadProtons();
        Daemon.call("downloads.list", undefined, r => {
            const d = {};
            for (const j of r ?? [])
                d[j.id] = j;
            root.downloads = d;
        });
    }
    function loadProtons() {
        Daemon.call("proton.list", undefined, r => {
            if (r)
                root.protons = r;
        });
        let got = [];
        let left = 2;
        for (const source of ["ge", "cachyos"])
            Daemon.call("proton.available", { source }, (r, e) => {
                got = got.concat((r ?? []).slice(0, 3));
                if (--left === 0)
                    root.available = got;
            });
    }
    function set(key, value) {
        Daemon.call("settings.set", { key, value }, (r, e) => {
            if (e)
                say(e.message, true);
        });
    }
    function setDefault(key, value) {
        const d = Object.assign({ mangohud: false, gamemode: false, gamescope: { enabled: false } }, defaults);
        if (value === null)
            delete d[key];
        else
            d[key] = value;
        set("defaults", d);
    }
    function say(text, isError) {
        message = text;
        messageError = !!isError;
        sayHide.restart();
    }
    function job(method, params, done) {
        Daemon.call(method, params, (r, e) => {
            if (e)
                say(e.message, true);
            else if (done)
                done(r);
        });
    }
    function size(bytes) {
        return bytes >= 1e9 ? (bytes / 1e9).toFixed(1) + " GB" : Math.round(bytes / 1e6) + " MB";
    }

    Component.onCompleted: refresh()
    Connections {
        target: Daemon
        function onReadyChanged() {
            if (Daemon.ready)
                root.refresh();
        }
        function onEvent(name, data) {
            if (name === "power.changed") {
                Daemon.call("power.status", undefined, r => root.power = r);
            } else if (name === "settings.changed") {
                const s = Object.assign({}, root.settings);
                if (data.value === null)
                    delete s[data.key];
                else
                    s[data.key] = data.value;
                root.settings = s;
            } else if (name === "download.progress") {
                const d = Object.assign({}, root.downloads);
                d[data.id] = data;
                root.downloads = d;
            } else if (name === "download.done") {
                const d = Object.assign({}, root.downloads);
                delete d[data.id];
                root.downloads = d;
                root.say(data.error ? `${data.label}: ${data.error}` : `${data.label}: done`, !!data.error && data.error !== "cancelled");
                if (data.label.toLowerCase().includes("proton"))
                    root.loadProtons();
            }
        }
    }

    Column {
        id: col
        x: Theme.space.xxl
        y: Theme.space.xxl
        width: Math.min(root.width - Theme.space.xxl * 2, 760)
        spacing: Theme.space.xl

        Item {
            width: parent.width
            height: heading.implicitHeight
            SText {
                id: heading
                text: "Settings"
                font.pixelSize: Theme.font.headline
                font.weight: Theme.font.weightTitle
            }
            SText {
                anchors.right: parent.right
                anchors.left: heading.right
                anchors.leftMargin: Theme.space.xl
                anchors.verticalCenter: heading.verticalCenter
                horizontalAlignment: Text.AlignRight
                opacity: sayHide.running ? 1 : 0
                Behavior on opacity {
                    NumberAnimation { duration: Motion.normal }
                }
                text: root.message
                color: root.messageError ? Theme.error : Theme.primary
            }
            Timer {
                id: sayHide
                interval: 6000
            }
        }

        // Running jobs (art, Proton downloads, update checks)
        Repeater {
            model: Object.keys(root.downloads)
            Card {
                id: dl
                required property string modelData
                readonly property var d: root.downloads[modelData] ?? null
                SettingRow {
                    title: dl.d ? dl.d.label : ""
                    note: dl.d && dl.d.total ? Math.round(dl.d.done * 100 / dl.d.total) + "%" : "Starting"
                    TextButton {
                        text: "Cancel"
                        onClicked: root.job("downloads.cancel", { id: Number(dl.modelData) })
                    }
                }
                Rectangle {
                    width: parent.width
                    height: 4
                    radius: 2
                    color: Theme.glassRaised
                    Rectangle {
                        width: dl.d && dl.d.total ? parent.width * dl.d.done / dl.d.total : 0
                        height: 4
                        radius: 2
                        color: Theme.primary
                        Behavior on width {
                            Spring { preset: "smooth" }
                        }
                    }
                }
            }
        }

        Card {
            title: "Library"
            SettingRow {
                below: true
                title: "Game folders"
                note: "Folders whose new subfolders are offered when you add a game"
                Column {
                    width: parent.width
                    spacing: Theme.space.s
                    Repeater {
                        model: root.settings.gameDirs ?? []
                        Row {
                            id: dirRow
                            required property string modelData
                            required property int index
                            spacing: Theme.space.s
                            Rectangle {
                                width: col.width - Theme.space.xl * 2 - 40 - Theme.space.s
                                height: 40
                                radius: Theme.radius.small + 2
                                color: Theme.glassRaised
                                SText {
                                    x: Theme.space.m
                                    width: parent.width - Theme.space.m * 2
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: dirRow.modelData
                                    mono: true
                                    elide: Text.ElideMiddle
                                }
                            }
                            IconButton {
                                width: 40
                                height: 40
                                icon: "close"
                                iconSize: 18
                                onClicked: {
                                    const dirs = (root.settings.gameDirs ?? []).filter((d, i) => i !== dirRow.index);
                                    root.set("gameDirs", dirs.length ? dirs : null);
                                }
                            }
                        }
                    }
                    TextButton {
                        icon: "create_new_folder"
                        text: "Add a folder"
                        onClicked: Pick.folder("", p => {
                            const dirs = root.settings.gameDirs ?? [];
                            if (!dirs.includes(p))
                                root.set("gameDirs", dirs.concat([p]));
                        })
                    }
                }
            }
            SettingRow {
                visible: (root.settings.scanIgnored ?? []).length > 0
                title: "Dismissed folders"
                note: (root.settings.scanIgnored ?? []).length + " folders are no longer offered"
                TextButton {
                    text: "Offer again"
                    onClicked: root.set("scanIgnored", null)
                }
            }
            PathSetting {
                key: "prefixDir"
                title: "Prefix folder"
                note: "Where new wine prefixes go; empty: Prefixes in the first game folder"
            }
            SettingRow {
                title: "Import"
                note: "Look through Steam, Lutris, Twintail and Prism again; nothing you changed is overwritten"
                TextButton {
                    id: importButton
                    property bool running: false
                    enabled: !running
                    icon: "download"
                    text: running ? "Importing" : "Import"
                    onClicked: {
                        running = true;
                        Daemon.call("library.import", undefined, (r, e) => {
                            running = false;
                            if (e)
                                root.say(e.message, true);
                            else
                                root.say(r.added.length + " added, " + r.updated.length + " updated" + Object.keys(r.errors).map(k => `; ${k}: ${r.errors[k]}`).join(""), Object.keys(r.errors).length > 0);
                        });
                    }
                }
            }
            SettingRow {
                title: "Art and details"
                note: "Fetch missing art and store details for every game"
                TextButton {
                    text: "Fetch"
                    onClicked: root.job("art.fetch", {}, () => root.job("meta.refresh", {}))
                }
            }
            SettingRow {
                title: "Updates"
                note: "Check Steam games for a newer build (done on its own twice a day)"
                TextButton {
                    text: "Check now"
                    onClicked: root.job("builds.check", {})
                }
            }
        }

        Card {
            title: "Keys"
            SettingRow {
                below: true
                title: "SteamGridDB API key"
                note: "Better art for every game: steamgriddb.com → Preferences → API"
                Field {
                    width: parent.width
                    secret: true
                    mono: true
                    value: root.settings.steamGridDbKey ?? ""
                    placeholder: "Not set: Steam's art only"
                    onCommitted: text => root.set("steamGridDbKey", text.trim() || null)
                }
            }
            SettingRow {
                below: true
                title: "Steam Web API key"
                note: "Adds the Steam games you own but have not installed, and achievements (Phase 7): steamcommunity.com/dev/apikey"
                Field {
                    width: parent.width
                    secret: true
                    mono: true
                    value: root.settings.steamApiKey ?? ""
                    placeholder: "Not set"
                    onCommitted: text => root.set("steamApiKey", text.trim() || null)
                }
            }
            SettingRow {
                below: true
                title: "SteamID"
                note: "SteamID64 of your account; empty: the one that logged in last"
                Field {
                    width: parent.width
                    mono: true
                    value: root.settings.steamId ?? ""
                    placeholder: "7656119…"
                    onCommitted: text => root.set("steamId", text.trim() || null)
                }
            }
        }

        Card {
            title: "Launch defaults"
            note: "This machine's profile: what every game starts from unless its own settings say otherwise"
            SettingRow {
                below: true
                title: "Proton"
                note: "For games that name none"
                Select {
                    width: parent.width
                    current: root.settings.defaultProton ?? ""
                    options: [{ id: "", name: "The newest GE-Proton", note: root.protons.default }].concat(root.protons.builds.map(b => ({ id: b.name, name: b.name })))
                    onPicked: id => root.set("defaultProton", id || null)
                }
            }
            SettingRow {
                visible: !!root.power && root.power.profiles.length > 0
                title: "Performance while gaming"
                note: "The power profile goes to performance while a game runs (balanced on battery) and back after" + (root.power && root.power.gaming ? ". A game is running now" : "")
                Switch {
                    checked: (root.settings.gaming ?? {}).powerProfile !== false
                    onToggled: on => root.set("gaming", Object.assign({}, root.settings.gaming ?? {}, { powerProfile: on }))
                }
            }
            SettingRow {
                visible: !!root.power && !!root.power.battery
                title: "Battery bypass while gaming"
                note: root.power && root.power.battery && !root.power.battery.limitSupported ? "This laptop's driver has no charge limit for UPower to use" : "On the charger, the battery's charge limit goes on while a game runs, so the laptop runs from the charger; off again after"
                Switch {
                    enabled: !!root.power && !!root.power.battery && root.power.battery.limitSupported
                    checked: (root.settings.gaming ?? {}).chargeLimit !== false
                    onToggled: on => root.set("gaming", Object.assign({}, root.settings.gaming ?? {}, { chargeLimit: on }))
                }
            }
            SettingRow {
                title: "MangoHud"
                note: "FPS and frame-time overlay"
                Switch {
                    checked: !!root.defaults.mangohud
                    onToggled: on => root.setDefault("mangohud", on)
                }
            }
            SettingRow {
                title: "GameMode"
                note: "gamemoderun: CPU governor and priority while playing" + (Shell.present ? "; SomeShell's game mode follows it" : "")
                Switch {
                    checked: !!root.defaults.gamemode
                    onToggled: on => root.setDefault("gamemode", on)
                }
            }
            SettingRow {
                title: "Gamescope"
                note: "Every game in its own compositor"
                Switch {
                    checked: !!(root.defaults.gamescope && root.defaults.gamescope.enabled)
                    onToggled: on => root.setDefault("gamescope", Object.assign({}, root.defaults.gamescope ?? {}, { enabled: on }))
                }
            }
            GamescopeEditor {
                width: parent.width
                visible: !!(root.defaults.gamescope && root.defaults.gamescope.enabled)
                value: root.defaults.gamescope ?? {}
                onEdited: v => root.setDefault("gamescope", v)
            }
            SettingRow {
                title: "Discrete GPU"
                note: "On a hybrid-GPU laptop; automatic: on there, nothing elsewhere"
                Segmented {
                    options: [{ id: "auto", name: "Automatic" }, { id: "on", name: "On" }, { id: "off", name: "Off" }]
                    current: root.defaults.prime === undefined || root.defaults.prime === null ? "auto" : root.defaults.prime ? "on" : "off"
                    onPicked: id => root.setDefault("prime", id === "auto" ? null : id === "on")
                }
            }
            PathSetting {
                key: "umuRun"
                title: "umu-run"
                note: "Empty: umu-run on PATH, else the one Lutris ships"
                folder: false
            }
        }

        Card {
            title: "Proton"
            SettingRow {
                title: "Installed"
                note: root.protons.builds.length + " builds; the ones in your compatibilitytools.d can be removed"
            }
            Repeater {
                model: root.protons.builds
                SettingRow {
                    id: build
                    required property var modelData
                    title: modelData.name
                    note: ({ user: "compatibilitytools.d", system: "System", steam: "Steam", lutris: "Lutris", twintail: "Twintail" })[modelData.source] ?? modelData.source
                    // Two clicks: the first arms it for 4 s
                    IconButton {
                        id: del
                        property bool armed: false
                        visible: build.modelData.source === "user"
                        width: 36
                        height: 36
                        icon: armed ? "delete_forever" : "delete"
                        iconSize: 18
                        color: armed ? Qt.alpha(Theme.error, 0.25) : "transparent"
                        iconColor: armed ? Theme.error : Theme.fgSurface
                        onClicked: {
                            if (!armed) {
                                armed = true;
                                disarm.restart();
                                return;
                            }
                            armed = false;
                            root.job("proton.remove", { name: build.modelData.name }, () => root.loadProtons());
                        }
                        Timer {
                            id: disarm
                            interval: 4000
                            onTriggered: del.armed = false
                        }
                    }
                }
            }
            SettingRow {
                visible: root.available.length > 0
                title: "Download"
                note: "The newest GE-Proton and Proton-CachyOS releases, checked against their sha512"
            }
            Repeater {
                model: root.available
                SettingRow {
                    id: release
                    required property var modelData
                    title: modelData.tag
                    note: (modelData.source === "ge" ? "GE-Proton" : "Proton-CachyOS") + " · " + root.size(modelData.size) + " · " + new Date(modelData.published).toLocaleDateString(Qt.locale(), "d MMM yyyy")
                    TextButton {
                        enabled: !release.modelData.installed
                        text: release.modelData.installed ? "Installed" : "Install"
                        onClicked: root.job("proton.install", { source: release.modelData.source, tag: release.modelData.tag }, () => root.say("Downloading " + release.modelData.tag))
                    }
                }
            }
        }

        Card {
            title: "Appearance"
            SettingRow {
                visible: Shell.present
                title: "Follow SomeShell"
                note: "Use SomeShell's glass tint and rim"
                Switch {
                    checked: Config.glass.followShell
                    onToggled: on => Config.glass.followShell = on
                }
            }
            SettingRow {
                visible: !Theme.followShell
                title: "Glass tint"
                note: "How much the surface covers the blur behind the window"
                Slider {
                    width: 220
                    from: 0.1
                    to: 0.95
                    value: Config.glass.tint
                    label: Math.round(Config.glass.tint * 100) + "%"
                    onMoved: v => Config.glass.tint = Math.round(v * 100) / 100
                }
            }
            SettingRow {
                visible: !Theme.followShell
                title: "Light rim"
                note: "A 1 px rim and a soft highlight on glass"
                Switch {
                    checked: Config.glass.rim
                    onToggled: on => Config.glass.rim = on
                }
            }
            SettingRow {
                title: "Reduce motion"
                note: "Short, plain moves instead of springs (always on while a game runs)"
                Switch {
                    checked: Config.motion.reduce
                    onToggled: on => Config.motion.reduce = on
                }
            }
            SettingRow {
                visible: !Config.motion.reduce
                title: "Motion speed"
                Slider {
                    width: 220
                    from: 0.5
                    to: 2
                    step: 0.1
                    value: Config.motion.speed
                    label: Config.motion.speed.toFixed(1) + "×"
                    onMoved: v => Config.motion.speed = Math.round(v * 10) / 10
                }
            }
            SettingRow {
                title: "Home shows"
                note: "Games played within this time; Library keeps them all"
                Segmented {
                    options: [{ id: 7, name: "7 days" }, { id: 30, name: "30 days" }, { id: 90, name: "90 days" }, { id: 0, name: "All" }]
                    current: Config.home.recentDays
                    onPicked: id => Config.home.recentDays = id
                }
            }
        }

        Card {
            title: "About"
            SettingRow {
                title: "somelauncherd"
                note: Daemon.ready ? `${Daemon.version} · pid ${root.status ? root.status.pid : "?"} · ${root.status ? root.status.db : ""}` : (Daemon.problem || "Not connected")
                TextButton {
                    visible: Daemon.ready
                    text: "Restart"
                    onClicked: Daemon.call("shutdown", undefined, undefined)
                }
            }
            SettingRow {
                title: "SomeShell"
                note: Shell.present ? "Found: using its colors" + (Theme.followShell ? " and glass" : "") + (Shell.gameMode ? "; game mode is on" : "") : "Not found: using the launcher's own colors"
            }
            SettingRow {
                title: "Show the first-run dialog again"
                TextButton {
                    text: "Show"
                    onClicked: Config.ui.welcomed = false
                }
            }
        }
    }

    // A titled glass card of settings
    component Card: Glass {
        id: card
        property string title: ""
        property string note: ""
        default property alias rows: cardCol.data
        width: col.width
        height: cardCol.implicitHeight + Theme.space.xl * 2
        radius: Theme.radius.large
        Column {
            id: cardCol
            x: Theme.space.xl
            y: Theme.space.xl
            width: parent.width - Theme.space.xl * 2
            spacing: Theme.space.l
            SText {
                visible: card.title !== ""
                text: card.title
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                visible: card.note !== ""
                text: card.note
                dim: true
                wrapMode: Text.Wrap
            }
        }
    }

    // A path setting: typed, or picked
    component PathSetting: SettingRow {
        id: ps
        property string key
        property bool folder: true
        below: true
        Row {
            width: parent.width
            spacing: Theme.space.s
            Field {
                width: parent.width - 40 - Theme.space.s
                mono: true
                value: root.settings[ps.key] ?? ""
                onCommitted: text => root.set(ps.key, text.trim() || null)
            }
            IconButton {
                width: 40
                height: 40
                icon: ps.folder ? "folder_open" : "draft"
                iconSize: 20
                onClicked: {
                    if (ps.folder)
                        Pick.folder(root.settings[ps.key] ?? "", p => root.set(ps.key, p));
                    else
                        Pick.file(root.settings[ps.key] ?? "", p => root.set(ps.key, p));
                }
            }
        }
    }
}
