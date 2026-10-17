pragma Singleton

import QtQuick
import Quickshell

// Which game page is open over the current page, and where it grows from;
// whether the add-game wizard is open.
// `Nav.openGame(id, card)` opens it from a card (its art Item: the page's art
// starts at that rect and radius); `Nav.closeGame()` shrinks it back. `gameId`
// stays set until the page has finished closing (`open` is false by then).
Singleton {
    id: root

    property int gameId: -1
    property bool open: false
    property rect from: Qt.rect(0, 0, 0, 0)    // scene coordinates; empty: the page fades instead
    property real fromRadius: 0
    property bool adding: false                // the add-game wizard is open

    function addGame() {
        adding = true;
    }

    function openGame(id, item) {
        if (item) {
            const p = item.mapToItem(null, 0, 0);
            from = Qt.rect(p.x, p.y, item.width, item.height);
            fromRadius = item.radius ?? 0;
        } else {
            from = Qt.rect(0, 0, 0, 0);
        }
        gameId = id;
        open = true;
    }
    function closeGame() {
        open = false;
    }
    // The page calls this once it has shrunk away
    function closed() {
        if (!open)
            gameId = -1;
    }
}
