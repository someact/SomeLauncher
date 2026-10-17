import QtQuick
import Quickshell.Widgets
import qs.core

// The Mods tab of a Hoyo game's settings (XXMI, hoyo/xxmi.rs): the switch
// (off by default: mods break HoYoverse's terms), XXMI's versions with an
// update, and the game's mods folder: each mod with its preview and its own
// switch, the folder to open, Twintail's mods to copy in. Read again each
// time the tab shows and after an XXMI job, never polled.
Column {
    id: root

    property var game: null
    property bool shown: false
    signal runnerPatch(var patch)

    readonly property string biz: Library.hoyoBiz(game)
    readonly property string pkg: biz.startsWith("hk4e") ? "gimi" : biz.startsWith("nap") ? "zzmi" : ""
    readonly property bool on: !!game && !!game.runner && game.runner.xxmi === true
    readonly property var job: Library.jobs["xxmi"] ?? null
    property var status: null
    property var mods: []
    property string modsDir: ""
    property string error: ""
    property string note: ""
    readonly property var p: status && status.packages ? status.packages[pkg] ?? null : null
    readonly property bool ready: !!p && p.ready
    readonly property bool outdated: !!status && !!status.outdated && status.outdated.length > 0

    spacing: Theme.space.xl

    function load(force) {
        if (!biz)
            return;
        Daemon.call("xxmi.status", { force: !!force }, r => {
            if (r)
                root.status = r;
        });
        Daemon.call("xxmi.mods", { biz }, (r, e) => {
            root.error = e ? e.message : "";
            root.mods = r ? r.mods : [];
            root.modsDir = r ? r.dir : "";
        });
    }
    onShownChanged: if (shown) load(false)
    onBizChanged: if (shown) load(false)
    // An update just ended: what is installed now
    onJobChanged: if (!job && shown) load(false)

    function update() {
        error = "";
        Daemon.call("xxmi.update", undefined, (r, e) => root.error = e ? e.message : "");
    }
    function toggle(on) {
        root.runnerPatch({ xxmi: on ? true : null });
        if (on && !ready && !job)
            update();
    }
    function setMod(m, on) {
        Daemon.call("xxmi.setMod", { biz, folder: m.folder, enabled: on }, (r, e) => {
            root.error = e ? e.message : "";
            root.load(false);
        });
    }
    function importTwintail() {
        note = "";
        Daemon.call("xxmi.importTwintail", { biz }, (r, e) => {
            root.error = e ? e.message : "";
            if (r)
                root.note = r.copied.length ? "Copied " + r.copied.length + (r.copied.length === 1 ? " mod" : " mods") + " from Twintail" : "Twintail has no mods that are not here already";
            root.load(false);
        });
    }
    function versions() {
        if (!status)
            return "…";
        const i = status.installed ?? {};
        const name = pkg === "gimi" ? "GIMI" : "ZZMI";
        if (!i.libs || !i[pkg])
            return "Not installed yet (about 10 MB from the projects' GitHub releases)";
        let s = `XXMI ${i.libs} · ${name} ${i[pkg]} · loader ${i.loader ?? "?"}`;
        if (outdated)
            s += " · updates: " + status.outdated.map(k => k === "libs" ? "XXMI" : k === "loader" ? "loader" : k.toUpperCase()).join(", ");
        else if (status.latest)
            s += " · up to date";
        else if (status.error)
            s += " · can't check for updates";
        return s;
    }

    Column {
        width: parent.width
        spacing: Theme.space.l
        SText {
            text: "XXMI"
            color: Theme.primary
            font.weight: Theme.font.weightTitle
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
            font.pixelSize: Theme.font.small
        }
        SettingRow {
            title: "Mods"
            note: "3DMigoto mods through XXMI (" + (root.pkg === "gimi" ? "GIMI" : "ZZMI") + "), loaded when the game starts. Mods break HoYoverse's terms of service and can get an account banned: use them at your own risk."
            Switch {
                checked: root.on
                onToggled: on => root.toggle(on)
            }
        }
        SettingRow {
            title: "XXMI"
            note: root.versions()
            TextButton {
                text: root.job ? "Updating " + (root.job.total ? Math.floor(root.job.done * 100 / root.job.total) + " %" : "…") : root.ready && !root.outdated ? "Check" : root.ready ? "Update" : "Install"
                enabled: !root.job
                onClicked: root.ready && !root.outdated ? root.load(true) : root.update()
            }
        }
    }

    Column {
        width: parent.width
        spacing: Theme.space.m

        Item {
            width: parent.width
            height: 40
            SText {
                anchors.verticalCenter: parent.verticalCenter
                text: "Mods folder" + (root.mods.length ? " · " + root.mods.filter(m => m.enabled).length + " of " + root.mods.length + " on" : "")
                color: Theme.primary
                font.weight: Theme.font.weightTitle
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 1
                font.pixelSize: Theme.font.small
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: Theme.space.s
                IconButton {
                    width: 40
                    height: 40
                    icon: "refresh"
                    iconSize: 20
                    onClicked: root.load(false)
                }
                TextButton {
                    visible: !!root.p && root.p.twintail
                    text: "Copy from Twintail"
                    icon: "content_copy"
                    onClicked: root.importTwintail()
                }
                TextButton {
                    text: "Open"
                    icon: "folder_open"
                    enabled: root.modsDir !== "" && root.ready
                    onClicked: Qt.openUrlExternally(Paths.url(root.modsDir))
                }
            }
        }

        SText {
            width: parent.width
            visible: text !== ""
            text: root.error || root.note
            color: root.error ? Theme.error : Theme.fgSurfaceVariant
            wrapMode: Text.Wrap
            maximumLineCount: 3
        }
        SText {
            width: parent.width
            visible: root.mods.length === 0
            text: root.ready ? "No mods yet. Each mod is a folder: put it in the mods folder, then refresh." : "Mods go in a folder that XXMI's install makes."
            dim: true
            wrapMode: Text.Wrap
        }

        Repeater {
            model: root.mods
            Rectangle {
                id: row
                required property var modelData
                width: root.width
                height: 64
                radius: Theme.radius.normal
                color: Theme.glassRaised
                ClippingRectangle {
                    id: thumb
                    x: Theme.space.s
                    anchors.verticalCenter: parent.verticalCenter
                    width: 48
                    height: 48
                    radius: Theme.radius.small
                    color: Theme.surfaceHighest
                    Image {
                        anchors.fill: parent
                        visible: status === Image.Ready
                        source: row.modelData.preview ? Paths.url(row.modelData.preview) : ""
                        sourceSize: Qt.size(96, 96)
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: !row.modelData.preview
                        name: "extension"
                        color: Theme.fgSurfaceVariant
                    }
                }
                SText {
                    anchors.left: thumb.right
                    anchors.leftMargin: Theme.space.m
                    anchors.right: modSwitch.left
                    anchors.rightMargin: Theme.space.m
                    anchors.verticalCenter: parent.verticalCenter
                    text: row.modelData.name
                    opacity: row.modelData.enabled ? 1 : 0.6
                    font.weight: Theme.font.weightTitle
                }
                Switch {
                    id: modSwitch
                    anchors.right: parent.right
                    anchors.rightMargin: Theme.space.m
                    anchors.verticalCenter: parent.verticalCenter
                    checked: row.modelData.enabled
                    onToggled: on => root.setMod(row.modelData, on)
                }
            }
        }
    }
}
