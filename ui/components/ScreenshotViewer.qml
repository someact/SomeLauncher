import QtQuick
import qs.core

// Screenshots one at a time over the page: the picture fitted, its date, and
// arrows (also Left/Right and the wheel) through `shots` ([{path, time}],
// newest first). Escape or a click beside the picture closes it.
FocusScope {
    id: root

    property var shots: []
    property int index: -1
    readonly property bool open: index >= 0 && index < shots.length
    readonly property var shot: open ? shots[index] : null
    signal closeRequested

    visible: fade > 0.01
    property real fade: open ? 1 : 0
    Behavior on fade {
        NumberAnimation { duration: Motion.normal; easing.type: Easing.OutCubic }
    }
    onOpenChanged: if (open) forceActiveFocus()

    function step(by) {
        if (shots.length)
            index = (index + by + shots.length) % shots.length;
    }
    Keys.onEscapePressed: closeRequested()
    Keys.onLeftPressed: step(-1)
    Keys.onRightPressed: step(1)

    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.82 * root.fade)
    }
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.BackButton
        onClicked: root.closeRequested()
        onWheel: wheel => root.step(wheel.angleDelta.y > 0 ? -1 : 1)
    }

    Image {
        id: picture
        anchors.fill: parent
        anchors.margins: Theme.space.xxl * 1.5
        anchors.bottomMargin: Theme.space.xxl * 2
        source: root.shot ? Paths.url(root.shot.path) : ""
        sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        opacity: root.fade
        scale: 0.97 + 0.03 * root.fade
        // A click on the picture itself does not close
        MouseArea {
            x: (parent.width - parent.paintedWidth) / 2
            y: (parent.height - parent.paintedHeight) / 2
            width: parent.paintedWidth
            height: parent.paintedHeight
        }
    }

    SText {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.space.xl
        opacity: root.fade
        color: Theme.fgOnArtDim
        text: root.shot ? new Date(root.shot.time * 1000).toLocaleString(Qt.locale(), "d MMM yyyy, HH:mm") + "   ·   " + (root.index + 1) + " / " + root.shots.length : ""
    }

    IconButton {
        anchors.left: parent.left
        anchors.leftMargin: Theme.space.l
        anchors.verticalCenter: parent.verticalCenter
        visible: root.shots.length > 1
        opacity: root.fade
        icon: "chevron_left"
        color: Qt.rgba(1, 1, 1, 0.12)
        iconColor: Theme.fgOnArt
        onClicked: root.step(-1)
    }
    IconButton {
        anchors.right: parent.right
        anchors.rightMargin: Theme.space.l
        anchors.verticalCenter: parent.verticalCenter
        visible: root.shots.length > 1
        opacity: root.fade
        icon: "chevron_right"
        color: Qt.rgba(1, 1, 1, 0.12)
        iconColor: Theme.fgOnArt
        onClicked: root.step(1)
    }
    Row {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.space.l
        spacing: Theme.space.s
        opacity: root.fade
        IconButton {
            icon: "folder_open"
            color: Qt.rgba(1, 1, 1, 0.12)
            iconColor: Theme.fgOnArt
            onClicked: if (root.shot) Qt.openUrlExternally(Paths.url(root.shot.path.substring(0, root.shot.path.lastIndexOf("/"))))
        }
        IconButton {
            icon: "close"
            color: Qt.rgba(1, 1, 1, 0.12)
            iconColor: Theme.fgOnArt
            onClicked: root.closeRequested()
        }
    }
}
