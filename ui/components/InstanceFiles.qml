import QtQuick
import qs.core

// A Minecraft instance's Files tab: a tile per folder of its game folder that
// exists (from `minecraft.instance`'s `folders`), opened in the file manager,
// and the instance in Prism for everything we do not show.
Flickable {
    id: root

    property var game: null
    property var info: null

    readonly property var known: ({
        mods: { name: "Mods", icon: "extension" },
        resourcepacks: { name: "Resource packs", icon: "texture" },
        shaderpacks: { name: "Shader packs", icon: "wb_sunny" },
        saves: { name: "Worlds", icon: "public" },
        screenshots: { name: "Screenshots", icon: "photo_library" },
        config: { name: "Config", icon: "tune" },
        logs: { name: "Logs", icon: "description" },
        "crash-reports": { name: "Crash reports", icon: "bug_report" }
    })
    readonly property var tiles: {
        if (!info)
            return [];
        const t = [{ name: "Game folder", icon: "folder", path: info.gameDir, note: "Everything Minecraft keeps" }];
        for (const f of info.folders)
            t.push({ name: known[f].name, icon: known[f].icon, path: info.gameDir + "/" + f, note: root.note(f) });
        t.push({ name: "Instance folder", icon: "folder_special", path: info.dir, note: "Prism's files for it" });
        return t;
    }
    function note(f) {
        const i = info;
        if (f === "mods")
            return i.mods + " on" + (i.modsDisabled ? ", " + i.modsDisabled + " off" : "");
        if (f === "resourcepacks")
            return i.resourcePacks + (i.resourcePacks === 1 ? " pack" : " packs");
        if (f === "shaderpacks")
            return i.shaderPacks + (i.shaderPacks === 1 ? " pack" : " packs");
        if (f === "saves")
            return i.worlds.length + (i.worlds.length === 1 ? " world" : " worlds");
        if (f === "screenshots")
            return i.screenshots.length + (i.screenshots.length === 1 ? " picture" : " pictures");
        return "";
    }

    contentHeight: col.height + Theme.space.xl
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    Column {
        id: col
        width: root.width
        spacing: Theme.space.xl

        Flow {
            id: flow
            width: parent.width
            spacing: Theme.space.m
            readonly property int columns: Math.max(1, Math.floor((width + spacing) / (220 + spacing)))
            readonly property real cell: (width - spacing * (columns - 1)) / columns
            Repeater {
                model: root.tiles
                PressButton {
                    id: tile
                    required property var modelData
                    width: flow.cell
                    height: 72
                    radius: Theme.radius.normal
                    color: Theme.glassRaised
                    Icon {
                        id: tileIcon
                        x: Theme.space.l
                        anchors.verticalCenter: parent.verticalCenter
                        name: tile.modelData.icon
                        size: 24
                        color: Theme.primary
                    }
                    Column {
                        anchors.left: tileIcon.right
                        anchors.leftMargin: Theme.space.m
                        anchors.right: parent.right
                        anchors.rightMargin: Theme.space.m
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 2
                        SText {
                            width: parent.width
                            text: tile.modelData.name
                            font.weight: Theme.font.weightTitle
                        }
                        SText {
                            width: parent.width
                            visible: text !== ""
                            text: tile.modelData.note
                            dim: true
                            font.pixelSize: Theme.font.small
                        }
                    }
                    onClicked: Qt.openUrlExternally(Paths.url(tile.modelData.path))
                }
            }
        }

        Column {
            width: parent.width
            spacing: Theme.space.s
            SText {
                text: "Everything else"
                font.pixelSize: Theme.font.large
                font.weight: Theme.font.weightTitle
            }
            SText {
                width: parent.width
                wrapMode: Text.Wrap
                dim: true
                text: "Renaming, copying, exporting, versions and loaders, and Prism's other instance settings stay in Prism for now."
            }
            TextButton {
                text: "Open in Prism"
                icon: "open_in_new"
                onClicked: if (root.game) Daemon.call("minecraft.prism", { id: root.game.id }, undefined)
            }
        }
    }
}
