import QtQuick
import qs.core

// A pill slider between `from` and `to` (from SomeShell components/Slider.qml,
// 2e12a96, without the icon). `moved(v)` fires while dragging or scrolling;
// `label` is drawn on the right.
Item {
    id: root

    property real value: 0
    property real from: 0
    property real to: 1
    property real step: 0.05
    property string label: ""
    readonly property real frac: (value - from) / (to - from)
    readonly property bool dragging: mouse.pressed

    signal moved(real value)

    implicitWidth: 240
    implicitHeight: 32

    function set(f) {
        const v = from + Math.max(0, Math.min(1, f)) * (to - from);
        moved(Math.round(v / step) * step);
    }

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: Theme.glassRaised
        border.width: 1
        border.color: Theme.glassRim
        clip: true
        Rectangle {
            height: parent.height
            radius: height / 2
            color: Theme.primary
            width: Math.max(height, parent.width * Math.min(1, root.frac))
            Behavior on width {
                enabled: !root.dragging
                Spring { preset: "snappy" }
            }
        }
    }
    SText {
        anchors.right: parent.right
        anchors.rightMargin: Theme.space.m
        anchors.verticalCenter: parent.verticalCenter
        text: root.label
        mono: true
        font.pixelSize: Theme.font.small
        color: root.frac > 0.8 ? Theme.fgPrimary : Theme.fgSurfaceVariant
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        preventStealing: true
        onPressed: event => root.set(event.x / width)
        onPositionChanged: event => {
            if (pressed)
                root.set(event.x / width);
        }
        onWheel: event => root.moved(Math.max(root.from, Math.min(root.to, root.value + (event.angleDelta.y > 0 ? root.step : -root.step))))
    }
}
