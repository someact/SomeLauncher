import QtQuick
import qs.core

// Adding a game by hand, three steps: pick its program or folder (or one the
// folder scan found), confirm the name and what it is (art and details), then
// how it runs. Nothing is added until the last step; then the game's page opens.
Dialog {
    id: root

    property int step: 0
    property var guess: null                 // games.guess for the picked path
    property string name: ""
    property string exe: ""
    property string kind: "proton"
    property var candidate: null             // the chosen match, matched once the game is added
    property string proton: ""
    property bool needsSteam: false
    property bool busy: false
    property string error: ""
    property var scanned: []                 // library.scan: new folders under the game folders
    property bool hasGameDirs: false
    property var protons: ({ builds: [], default: "" })
    readonly property var steps: ["Pick", "Name and art", "Runner"]

    signal added(int id)

    cardWidth: 720
    cardHeight: Math.min(parent ? parent.height - Theme.space.xl * 2 : 640, 680)

    onOpenChanged: if (open) reset()
    onStepChanged: body.contentY = 0
    function reset() {
        step = 0;
        guess = null;
        candidate = null;
        proton = "";
        needsSteam = false;
        error = "";
        busy = false;
        Daemon.call("library.scan", undefined, r => root.scanned = r ?? []);
        Daemon.call("settings.get", { key: "gameDirs" }, r => root.hasGameDirs = !!r && r.length > 0);
        Daemon.call("proton.list", undefined, r => {
            if (r)
                root.protons = r;
        });
    }

    function use(g) {
        name = g.name;
        guess = g;
        exe = g.exe ?? "";
        kind = g.kind === "native" ? "native" : "proton";
        candidate = null;
        error = "";
        step = 1;
    }
    // Another game picked: the picker searches its name afresh
    onGuessChanged: if (matcher.item) Qt.callLater(() => {
        matcher.item.chosenKey = "";
        matcher.item.search();
    })
    function guessPath(path) {
        if (path.trim() === "")
            return;
        busy = true;
        error = "";
        Daemon.call("games.guess", { path: path.trim() }, (g, e) => {
            busy = false;
            if (e)
                error = e.message;
            else
                use(g);
        });
    }
    function ignore(dir) {
        Daemon.call("settings.get", { key: "scanIgnored" }, r => {
            const list = (r ?? []).concat([dir]);
            Daemon.call("settings.set", { key: "scanIgnored", value: list }, () => root.scanned = root.scanned.filter(s => s.installDir !== dir));
        });
    }
    function add() {
        const runner = {};
        if (kind === "proton" && proton !== "")
            runner.proton = proton;
        if (kind === "proton" && needsSteam)
            runner.needsSteam = true;
        busy = true;
        error = "";
        Daemon.call("games.add", { name: name.trim() || guess.name, kind, installDir: guess.installDir, exe: exe || null, runner }, (g, e) => {
            busy = false;
            if (e) {
                error = e.message;
                return;
            }
            if (candidate)
                Daemon.call("meta.match", { id: g.id, source: candidate.source, sourceId: candidate.id }, undefined);
            root.added(g.id);
        });
    }

    // Header: the steps
    Row {
        id: header
        width: parent.width
        spacing: Theme.space.s
        Repeater {
            model: root.steps
            Row {
                required property string modelData
                required property int index
                spacing: Theme.space.s
                Rectangle {
                    width: 26
                    height: 26
                    radius: 13
                    anchors.verticalCenter: parent.verticalCenter
                    color: index <= root.step ? Theme.primary : Theme.glassRaised
                    Behavior on color {
                        ColorAnimation { duration: Motion.fast }
                    }
                    SText {
                        anchors.centerIn: parent
                        text: index < root.step ? "" : String(index + 1)
                        color: index <= root.step ? Theme.fgPrimary : Theme.fgSurfaceVariant
                        font.weight: Theme.font.weightTitle
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: index < root.step
                        name: "check"
                        size: 16
                        color: Theme.fgPrimary
                    }
                }
                SText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData
                    dim: index !== root.step
                    font.weight: index === root.step ? Theme.font.weightTitle : Theme.font.weight
                }
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: index < root.steps.length - 1
                    width: 28
                    height: 1
                    color: Theme.outlineVariant
                }
            }
        }
    }
    IconButton {
        anchors.right: parent.right
        anchors.verticalCenter: header.verticalCenter
        width: 36
        height: 36
        iconSize: 20
        icon: "close"
        onClicked: root.closeRequested()
    }

    Flickable {
        id: body
        anchors.top: header.bottom
        anchors.topMargin: Theme.space.xl
        anchors.bottom: footer.top
        anchors.bottomMargin: Theme.space.l
        width: parent.width
        clip: true
        contentHeight: (root.step === 0 ? pick.implicitHeight : root.step === 1 ? naming.implicitHeight : running.implicitHeight) + Theme.space.m
        boundsBehavior: Flickable.StopAtBounds

        // 1: pick
        Column {
            id: pick
            width: body.width
            visible: root.step === 0
            spacing: Theme.space.l

            SText {
                text: "Add a game"
                font.pixelSize: Theme.font.headline
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                text: "Choose the game's program, or the folder it is in: the name, the program and the runner are guessed from it."
                dim: true
                wrapMode: Text.Wrap
            }
            Row {
                width: parent.width
                spacing: Theme.space.m
                BigChoice {
                    width: (parent.width - Theme.space.m) / 2
                    icon: "draft"
                    title: "Choose a program"
                    note: ".exe, a Linux binary or a script"
                    onClicked: Pick.file("", p => root.guessPath(p), "Programs (*.exe *.sh *.x86_64 *.AppImage *.bat)")
                }
                BigChoice {
                    width: (parent.width - Theme.space.m) / 2
                    icon: "folder_open"
                    title: "Choose a folder"
                    note: "The game's install folder"
                    onClicked: Pick.folder("", p => root.guessPath(p))
                }
            }
            Field {
                width: parent.width
                mono: true
                placeholder: "…or type a path and press Enter"
                onCommitted: text => root.guessPath(text)
            }
            SText {
                width: parent.width
                visible: root.error !== "" || root.busy
                text: root.busy ? "Looking…" : root.error
                color: root.busy ? Theme.fgSurfaceVariant : Theme.error
                wrapMode: Text.Wrap
            }

            SText {
                visible: root.scanned.length > 0
                topPadding: Theme.space.s
                text: "Found in your game folders"
                font.weight: Theme.font.weightTitle
            }
            Repeater {
                model: root.scanned
                PressButton {
                    id: found
                    required property var modelData
                    width: pick.width
                    height: 56
                    radius: Theme.radius.normal
                    color: Theme.glassRaised
                    onClicked: root.use(modelData)
                    Icon {
                        x: Theme.space.l
                        anchors.verticalCenter: parent.verticalCenter
                        name: Library.kindIcon(found.modelData.kind)
                        color: Theme.fgSurfaceVariant
                    }
                    Column {
                        x: Theme.space.l + 20 + Theme.space.m
                        width: parent.width - x - 48
                        anchors.verticalCenter: parent.verticalCenter
                        SText {
                            width: parent.width
                            text: found.modelData.name
                            font.weight: Theme.font.weightTitle
                        }
                        SText {
                            width: parent.width
                            text: found.modelData.installDir
                            dim: true
                            font.pixelSize: Theme.font.small
                            elide: Text.ElideMiddle
                        }
                    }
                    IconButton {
                        anchors.right: parent.right
                        anchors.rightMargin: Theme.space.s
                        anchors.verticalCenter: parent.verticalCenter
                        width: 36
                        height: 36
                        icon: "close"
                        iconSize: 18
                        color: "transparent"
                        onClicked: root.ignore(found.modelData.installDir)
                    }
                }
            }
            SText {
                width: parent.width
                visible: !root.hasGameDirs
                text: "Add your game folders in Settings and new games in them show up here."
                dim: true
                font.pixelSize: Theme.font.small
                wrapMode: Text.Wrap
            }
        }

        // 2: name and art
        Column {
            id: naming
            width: body.width
            visible: root.step === 1
            spacing: Theme.space.l

            SettingRow {
                below: true
                title: "Name"
                Field {
                    width: parent.width
                    value: root.name
                    onCommitted: text => {
                        root.name = text.trim();
                        if (matcher.item)
                            matcher.item.search();
                    }
                }
            }
            SettingRow {
                visible: !!root.guess && root.guess.candidates.length > 1
                below: true
                title: "Program"
                note: "The one Play starts"
                Select {
                    width: parent.width
                    current: root.exe
                    options: (root.guess ? root.guess.candidates : []).map(p => ({ id: p, name: p.split("/").pop(), note: p.slice(root.guess.installDir.length + 1) }))
                    onPicked: id => root.exe = id
                }
            }
            SettingRow {
                below: true
                title: "Which game is it?"
                note: "Brings the cover, background, logo and store details. You can skip it and match later."
                Loader {
                    id: matcher
                    width: parent.width
                    // Made again for each picked game, so it searches the new name
                    active: root.step >= 1 && root.guess !== null
                    sourceComponent: MatchPicker {
                        width: matcher.width
                        query: root.name
                        onChosen: c => root.candidate = c
                    }
                }
            }
        }

        // 3: runner
        Column {
            id: running
            width: body.width
            visible: root.step === 2
            spacing: Theme.space.xl

            SettingRow {
                title: "Runs as"
                note: root.kind === "proton" ? "A Windows game, through Proton (umu-run)" : "A Linux game, run as it is"
                Segmented {
                    options: [{ id: "proton", name: "Windows" }, { id: "native", name: "Linux" }]
                    current: root.kind
                    onPicked: id => root.kind = id
                }
            }
            SettingRow {
                visible: root.kind === "proton"
                below: true
                title: "Proton"
                Select {
                    width: parent.width
                    current: root.proton
                    options: [{ id: "", name: "Default", note: root.protons.default || "the newest GE-Proton" }].concat(root.protons.builds.map(b => ({ id: b.name, name: b.name })))
                    onPicked: id => root.proton = id
                }
            }
            SettingRow {
                visible: root.kind === "proton"
                title: "Needs Steam"
                note: "Co-op that needs the real Steam client (OnlineFix, gbe_fork)"
                Switch {
                    checked: root.needsSteam
                    onToggled: on => root.needsSteam = on
                }
            }
            SText {
                width: parent.width
                text: "Everything else (arguments, env, DLL overrides, gamescope, MangoHud) is in the game's settings once it is added."
                dim: true
                wrapMode: Text.Wrap
            }
            SText {
                width: parent.width
                visible: root.error !== ""
                text: root.error
                color: Theme.error
                wrapMode: Text.Wrap
            }
        }
    }

    Row {
        id: footer
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        spacing: Theme.space.s
        TextButton {
            visible: root.step > 0
            text: "Back"
            onClicked: root.step--
        }
        TextButton {
            visible: root.step === 1
            primary: true
            text: "Next"
            onClicked: root.step = 2
        }
        TextButton {
            visible: root.step === 2
            primary: true
            enabled: !root.busy
            icon: "add"
            text: root.busy ? "Adding" : "Add game"
            onClicked: root.add()
        }
    }
    SText {
        anchors.left: parent.left
        anchors.right: footer.left
        anchors.rightMargin: Theme.space.m
        anchors.verticalCenter: footer.verticalCenter
        visible: root.step > 0 && root.guess !== null
        text: root.guess ? root.guess.installDir : ""
        dim: true
        font.pixelSize: Theme.font.small
        elide: Text.ElideMiddle
    }

    component BigChoice: PressButton {
        id: choice
        property string icon
        property string title
        property string note
        height: 132
        radius: Theme.radius.large
        color: Theme.glassRaised
        Column {
            anchors.centerIn: parent
            spacing: Theme.space.s
            Icon {
                anchors.horizontalCenter: parent.horizontalCenter
                name: choice.icon
                size: 32
                color: Theme.primary
            }
            SText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: choice.title
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
            SText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: choice.note
                dim: true
                font.pixelSize: Theme.font.small
            }
        }
    }
}
