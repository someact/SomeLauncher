import QtQuick
import qs.core

// Installing a Hoyo game (hoyo.plan, then hoyo.install): where it goes, the
// voice packs with their sizes, what it downloads and takes on disk against
// the free space. A picked folder is where the game's own folder goes, unless
// it is already named after the game. "Already installed" adopts a folder
// instead (hoyo.adopt). `installed(id)` once the job has started or the
// folder was adopted.
Dialog {
    id: root

    property var hoyoGame: null              // a hoyo.games entry
    property var plan: null
    property var voices: []
    property string dir: ""
    property bool loading: false
    property bool starting: false
    property string error: ""
    signal installed(int id)

    readonly property string name: hoyoGame ? hoyoGame.name : ""
    readonly property bool fits: !plan || plan.free === null || plan.free >= plan.size
    readonly property var voiceNames: ({ "zh-cn": "Chinese", "en-us": "English", "ja-jp": "Japanese", "ko-kr": "Korean" })

    cardWidth: 560
    cardHeight: col.implicitHeight + Theme.space.xl * 2
    onCloseRequested: open = false
    onOpenChanged: {
        if (!open)
            return;
        plan = null;
        voices = [];
        dir = "";
        error = "";
        load();
    }

    function load() {
        if (!hoyoGame)
            return;
        loading = true;
        const params = { biz: hoyoGame.biz };
        if (voices.length)
            params.voices = voices;
        if (dir)
            params.dir = dir;
        const biz = hoyoGame.biz;
        Daemon.call("hoyo.plan", params, (r, e) => {
            if (!root.hoyoGame || root.hoyoGame.biz !== biz)
                return;
            root.loading = false;
            root.error = e ? e.message : "";
            if (!r)
                return;
            root.plan = r;
            root.voices = r.voices;
            root.dir = r.dir;
        });
    }
    function toggleVoice(v) {
        const on = voices.includes(v);
        // At least one: the game needs a voice pack
        if (on && voices.length === 1)
            return;
        voices = on ? voices.filter(x => x !== v) : voices.concat([v]);
        load();
    }
    function pickDir() {
        const start = dir ? dir.replace(/\/[^/]*$/, "") : "";
        Pick.folder(start, path => {
            const base = path.replace(/\/+$/, "").split("/").pop();
            root.dir = base === root.name ? path : path.replace(/\/+$/, "") + "/" + root.name;
            root.load();
        });
    }
    function install() {
        starting = true;
        error = "";
        Daemon.call("hoyo.install", { biz: hoyoGame.biz, dir, voices }, (r, e) => {
            root.starting = false;
            if (e) {
                root.error = e.message;
                return;
            }
            root.open = false;
            root.installed(r.game);
        });
    }
    function adopt() {
        Pick.folder(dir ? dir.replace(/\/[^/]*$/, "") : "", path => {
            root.error = "";
            Daemon.call("hoyo.adopt", { path }, (g, e) => {
                if (e) {
                    root.error = e.message;
                    return;
                }
                root.open = false;
                root.installed(g.id);
            });
        });
    }

    Column {
        id: col
        width: parent.width
        spacing: Theme.space.l

        SText {
            width: parent.width
            text: "Install " + root.name + (root.plan ? " " + root.plan.version : "")
            font.pixelSize: Theme.font.title
            font.weight: Theme.font.weightTitle
        }

        SettingRow {
            title: "Folder"
            note: root.dir || "…"
            TextButton {
                text: "Change"
                onClicked: root.pickDir()
            }
        }

        Column {
            width: parent.width
            spacing: Theme.space.s
            SText {
                text: "Voice packs"
                font.weight: Theme.font.weightTitle
            }
            Flow {
                width: parent.width
                spacing: Theme.space.s
                Repeater {
                    model: root.plan ? root.plan.voiceSizes : []
                    PressButton {
                        id: voice
                        required property var modelData
                        readonly property bool on: root.voices.includes(modelData.voice)
                        width: voiceRow.implicitWidth + Theme.space.l * 2
                        height: 40
                        color: on ? Theme.secondaryContainer : Theme.glassRaised
                        hoverColor: on ? Qt.lighter(Theme.secondaryContainer, 1.1) : Theme.glassRaisedHover
                        onClicked: root.toggleVoice(modelData.voice)
                        Row {
                            id: voiceRow
                            anchors.centerIn: parent
                            spacing: Theme.space.s
                            Icon {
                                anchors.verticalCenter: parent.verticalCenter
                                name: voice.on ? "check" : "add"
                                size: 18
                                color: voice.on ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                            }
                            SText {
                                anchors.verticalCenter: parent.verticalCenter
                                text: (root.voiceNames[voice.modelData.voice] ?? voice.modelData.voice) + "  " + Library.bytes(voice.modelData.downloadSize)
                                color: voice.on ? Theme.fgSecondaryContainer : Theme.fgSurface
                            }
                        }
                    }
                }
            }
        }

        // What it takes
        Row {
            spacing: Theme.space.xl
            visible: !!root.plan
            Repeater {
                model: root.plan ? [
                    { label: "Download", value: Library.bytes(root.plan.downloadSize) },
                    { label: "On disk", value: Library.bytes(root.plan.size) },
                    { label: "Free", value: root.plan.free === null ? "Unknown" : Library.bytes(root.plan.free) }
                ] : []
                Column {
                    required property var modelData
                    required property int index
                    spacing: 2
                    SText {
                        text: parent.modelData.label
                        dim: true
                        font.pixelSize: Theme.font.small
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 1
                    }
                    SText {
                        text: parent.modelData.value
                        color: parent.index === 2 && !root.fits ? Theme.error : Theme.fgSurface
                        font.pixelSize: Theme.font.large
                        font.weight: Theme.font.weightTitle
                    }
                }
            }
        }

        SText {
            width: parent.width
            visible: text !== ""
            text: root.error || (!root.fits ? "Not enough free space in this folder's disk." : root.loading && !root.plan ? "Asking HoYoverse…" : "")
            color: root.error || !root.fits ? Theme.error : Theme.fgSurfaceVariant
            wrapMode: Text.Wrap
            maximumLineCount: 4
        }

        Item {
            width: parent.width
            height: 40
            TextButton {
                anchors.left: parent.left
                text: "Already installed"
                icon: "folder_open"
                onClicked: root.adopt()
            }
            Row {
                anchors.right: parent.right
                spacing: Theme.space.s
                TextButton {
                    text: "Cancel"
                    onClicked: root.open = false
                }
                TextButton {
                    primary: true
                    icon: "download"
                    text: root.starting ? "Starting…" : "Install"
                    enabled: !!root.plan && root.fits && !root.loading && !root.starting
                    onClicked: root.install()
                }
            }
        }
    }
}
