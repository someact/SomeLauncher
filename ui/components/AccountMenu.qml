import QtQuick
import qs.core

// Who Minecraft plays as: Prism's accounts (`minecraft.accounts`), the pick
// saved as the daemon's `minecraftAccount` (Play passes it to Prism as `-a`).
// A pill with the name; a click unfolds the list under it, over whatever is
// below. Adding an account or signing in again is Prism's: the last row opens it.
Item {
    id: root

    property int gameId: -1                 // whose Prism install's accounts
    property bool open: false
    property var accounts: []
    property var chosen: null               // the saved pick (null: Prism's active account)
    property string current: ""             // who Play uses

    implicitWidth: pill.width
    implicitHeight: pill.height

    function load() {
        Daemon.call("minecraft.accounts", gameId >= 0 ? { id: gameId } : {}, (r, e) => {
            if (!r)
                return;
            root.accounts = r.accounts;
            root.chosen = r.chosen;
            root.current = r.current ?? "";
        });
    }
    onGameIdChanged: load()
    Component.onCompleted: load()
    onOpenChanged: if (open) load()     // Prism may have changed them meanwhile
    Connections {
        target: Daemon
        function onEvent(name, data) {
            if (name === "settings.changed" && data.key === "minecraftAccount")
                root.load();
        }
    }
    function pick(name) {
        open = false;
        current = name ?? "";
        Daemon.call("settings.set", { key: "minecraftAccount", value: name }, undefined);
    }

    PressButton {
        id: pill
        anchors.right: parent.right
        width: row.implicitWidth + Theme.space.m * 2 + 4
        height: 40
        visible: root.accounts.length > 0
        color: Qt.rgba(0, 0, 0, 0.35)
        hoverColor: Qt.rgba(0, 0, 0, 0.5)
        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: "transparent"
            border.width: 1
            border.color: root.open ? Theme.primary : Theme.glassRim
        }
        Row {
            id: row
            anchors.centerIn: parent
            spacing: Theme.space.s
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: "account_circle"
                color: Theme.fgOnArt
            }
            SText {
                anchors.verticalCenter: parent.verticalCenter
                text: root.current || "Account"
                color: Theme.fgOnArt
                font.weight: Theme.font.weightTitle
            }
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: "expand_more"
                size: 18
                color: Theme.fgOnArtDim
                rotation: root.open ? 180 : 0
                Behavior on rotation {
                    Spring { preset: "snappy" }
                }
            }
        }
        onClicked: root.open = !root.open
    }

    Rectangle {
        id: panel
        anchors.right: parent.right
        anchors.top: pill.bottom
        anchors.topMargin: Theme.space.s
        width: 260
        height: root.open ? list.implicitHeight + 8 : 0
        visible: height > 1
        clip: true
        radius: Theme.radius.normal
        color: Theme.surfaceHigh
        border.width: 1
        border.color: Theme.glassRim
        Behavior on height {
            Spring { preset: "snappy" }
        }
        Column {
            id: list
            x: 4
            y: 4
            width: parent.width - 8
            Repeater {
                model: root.accounts
                Row_ {
                    required property var modelData
                    label: modelData.name
                    note: (modelData.type === "msa" ? "Microsoft" : "Offline") + (modelData.active ? " · Prism's active account" : "")
                    icon: modelData.type === "msa" ? "account_circle" : "person_off"
                    selected: modelData.name === root.current
                    onClicked: root.pick(modelData.active && root.chosen === null ? null : modelData.name)
                }
            }
            Rectangle {
                width: parent.width
                height: 1
                color: Theme.outlineVariant
            }
            Row_ {
                label: "Manage accounts in Prism"
                icon: "open_in_new"
                onClicked: {
                    root.open = false;
                    Daemon.call("minecraft.prism", {}, undefined);
                }
            }
        }
    }

    component Row_: PressButton {
        id: item
        property string label
        property string note: ""
        property string icon
        property bool selected: false
        width: list.width
        height: note === "" ? 40 : 48
        radius: Theme.radius.small
        active: selected
        activeColor: Theme.secondaryContainer
        Icon {
            id: glyph
            x: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            name: item.icon
            size: 20
            color: item.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        }
        Column {
            anchors.left: glyph.right
            anchors.leftMargin: Theme.space.m
            anchors.right: parent.right
            anchors.rightMargin: Theme.space.m
            anchors.verticalCenter: parent.verticalCenter
            SText {
                width: parent.width
                text: item.label
                color: item.selected ? Theme.fgSecondaryContainer : Theme.fgSurface
                font.weight: item.selected ? Theme.font.weightTitle : Theme.font.weight
            }
            SText {
                width: parent.width
                visible: item.note !== ""
                text: item.note
                dim: !item.selected
                color: item.selected ? Qt.alpha(Theme.fgSecondaryContainer, 0.75) : Theme.fgSurfaceVariant
                font.pixelSize: Theme.font.small
            }
        }
    }
}
