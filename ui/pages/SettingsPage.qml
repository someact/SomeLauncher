import QtQuick
import qs.core
import qs.components

// Settings (Phase 4f fills it). For now: the daemon's state and the looks.
Flickable {
    id: root
    contentHeight: col.implicitHeight + Theme.space.xxl * 2
    boundsBehavior: Flickable.StopAtBounds

    property var status: null
    function refresh() {
        Daemon.call("status", undefined, (r, e) => root.status = r);
    }
    Component.onCompleted: refresh()
    Connections {
        target: Daemon
        function onReadyChanged() {
            root.refresh();
        }
    }

    Column {
        id: col
        x: Theme.space.xxl
        y: Theme.space.xxl
        width: root.width - Theme.space.xxl * 2
        spacing: Theme.space.l

        SText {
            text: "Settings"
            font.pixelSize: Theme.font.headline
            font.weight: Theme.font.weightTitle
        }

        Glass {
            width: parent.width
            height: info.implicitHeight + Theme.space.xl * 2
            radius: Theme.radius.large
            Column {
                id: info
                x: Theme.space.xl
                y: Theme.space.xl
                width: parent.width - Theme.space.xl * 2
                spacing: Theme.space.s
                SText {
                    text: "Daemon"
                    font.pixelSize: Theme.font.large
                    font.weight: Theme.font.weightTitle
                }
                SText {
                    text: Daemon.ready ? `somelauncherd ${Daemon.version} · pid ${root.status?.pid ?? "?"}` : (Daemon.problem || "Not connected")
                    dim: true
                }
                SText {
                    visible: !!root.status
                    text: "Library: " + (root.status?.db ?? "")
                    dim: true
                    width: parent.width
                }
                SText {
                    text: Shell.present ? "SomeShell found: using its colors" + (Theme.followShell ? " and glass" : "") : "SomeShell not found: using the launcher's own colors"
                    dim: true
                }
            }
        }
    }
}
