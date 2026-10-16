import QtQuick
import qs.core

// The Steam logo on the cards of games that run through Steam or need the
// Steam client (`runner.needsSteam`). The glyph is the Nerd Font's.
Rectangle {
    property var game: null
    readonly property bool needsSteam: !!game && game.kind !== "steam" && !!game.runner && !!game.runner.needsSteam

    visible: !!game && (game.kind === "steam" || needsSteam)
    width: 26
    height: 26
    radius: 13
    color: Theme.scrim

    Text {
        anchors.centerIn: parent
        text: ""
        color: Theme.fgOnArt
        font.family: Theme.font.mono
        font.pixelSize: 16
    }
}
