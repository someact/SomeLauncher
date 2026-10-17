import QtQuick
import qs.core

// The first start: bring in what Steam, Lutris, Twintail and Prism already
// have (each can be left out), optionally name a game folder for the scan,
// then add a game by hand or carry on. Shown once (Config.ui.welcomed).
Dialog {
    id: root

    property var sources: ({ steam: true, lutris: true, twintail: true, prism: true })
    property bool importing: false
    property var result: null                // library.import's reply
    property string error: ""
    property string gameDir: ""

    signal addRequested

    modal: true
    cardWidth: 620
    cardHeight: col.implicitHeight + Theme.space.xl * 2

    readonly property var sourceList: [
        { id: "steam", name: "Steam", note: "installed games, owned ones with an API key" },
        { id: "lutris", name: "Lutris", note: "Windows and Linux games" },
        { id: "twintail", name: "Twintail", note: "Genshin Impact, Zenless Zone Zero" },
        { id: "prism", name: "Prism", note: "Minecraft instances" }
    ]

    function run() {
        const picked = sourceList.map(s => s.id).filter(id => sources[id]);
        importing = true;
        error = "";
        if (gameDir !== "")
            Daemon.call("settings.set", { key: "gameDirs", value: [gameDir] }, undefined);
        Daemon.call("library.import", { sources: picked }, (r, e) => {
            importing = false;
            if (e)
                error = e.message;
            result = r;
        });
    }

    // Scrolls when the window is too low for it
    Flickable {
        anchors.fill: parent
        contentHeight: col.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        Column {
            id: col
            width: parent.width
            spacing: Theme.space.l

            SText {
                text: root.result ? "All set" : "Welcome to SomeLauncher"
                font.pixelSize: Theme.font.headline
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                wrapMode: Text.Wrap
                dim: true
                text: {
                    if (root.importing)
                        return "Looking through your launchers…";
                    const r = root.result;
                    if (!r)
                        return "Bring in the games your other launchers already have. Nothing is moved or changed, and they keep working as before.";
                    const n = r.added.length;
                    return n === 0 ? "No new games found. Games you add by hand get their art and details the same way." : (n === 1 ? "1 game added." : n + " games added.") + " Art and store details come in over the next minute.";
                }
            }

            // The sources
            Column {
                width: parent.width
                visible: !root.result
                spacing: Theme.space.s
                Repeater {
                    model: root.sourceList
                    PressButton {
                        id: src
                        required property var modelData
                        readonly property bool on: !!root.sources[modelData.id]
                        width: col.width
                        height: 52
                        radius: Theme.radius.normal
                        color: Theme.glassRaised
                        enabled: !root.importing
                        onClicked: {
                            const s = Object.assign({}, root.sources);
                            s[modelData.id] = !s[modelData.id];
                            root.sources = s;
                        }
                        Column {
                            x: Theme.space.l
                            width: parent.width - x - 80
                            anchors.verticalCenter: parent.verticalCenter
                            SText {
                                text: src.modelData.name
                                font.weight: Theme.font.weightTitle
                            }
                            SText {
                                width: parent.width
                                text: src.modelData.note
                                dim: true
                                font.pixelSize: Theme.font.small
                            }
                        }
                        Switch {
                            anchors.right: parent.right
                            anchors.rightMargin: Theme.space.l
                            anchors.verticalCenter: parent.verticalCenter
                            checked: src.on
                            onToggled: src.clicked()
                        }
                    }
                }
            }

            SettingRow {
                visible: !root.result
                below: true
                title: "Game folder"
                note: "Optional: where you keep games that no launcher knows; new folders there are offered when you add a game"
                Row {
                    width: parent.width
                    spacing: Theme.space.s
                    Field {
                        width: parent.width - 40 - Theme.space.s
                        mono: true
                        value: root.gameDir
                        placeholder: "~/Games"
                        onCommitted: text => root.gameDir = text.trim().replace(/^~/, Paths.home)
                    }
                    IconButton {
                        width: 40
                        height: 40
                        icon: "folder_open"
                        iconSize: 20
                        onClicked: Pick.folder("", p => root.gameDir = p)
                    }
                }
            }

            // What failed, per importer (the others still ran)
            SText {
                width: parent.width
                visible: text !== ""
                text: root.error || (root.result ? Object.keys(root.result.errors).map(k => `${k}: ${root.result.errors[k]}`).join("\n") : "")
                color: Theme.error
                wrapMode: Text.Wrap
                font.pixelSize: Theme.font.small
            }

            Item {
                width: parent.width
                height: 40
                Row {
                    anchors.right: parent.right
                    spacing: Theme.space.s
                    TextButton {
                        visible: !root.result
                        enabled: !root.importing
                        text: "Skip"
                        onClicked: root.closeRequested()
                    }
                    TextButton {
                        visible: !root.result
                        primary: true
                        enabled: !root.importing && root.sourceList.some(s => root.sources[s.id])
                        icon: "download"
                        text: root.importing ? "Importing" : "Import"
                        onClicked: root.run()
                    }
                    TextButton {
                        visible: !!root.result
                        icon: "add"
                        text: "Add a game by hand"
                        onClicked: root.addRequested()
                    }
                    TextButton {
                        visible: !!root.result
                        primary: true
                        text: "Done"
                        onClicked: root.closeRequested()
                    }
                }
            }
        }
    }
}
