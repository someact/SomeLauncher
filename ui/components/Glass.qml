import QtQuick
import qs.core

// Frosted surface: a tint over the frost, a 1 px light rim and a soft top
// highlight (from SomeShell components/Glass.qml, 2e12a96). The launcher is a
// normal window, so Hyprland's window blur is the frost: keep the window color
// transparent behind glass.
Rectangle {
    id: root

    property color tint: Theme.glassTint
    property bool highlight: Theme.glassRimOn

    radius: Theme.radius.large
    color: tint
    border.width: 1
    border.color: Theme.glassRim
    Behavior on color {
        ColorAnimation { duration: Motion.normal }
    }

    Rectangle {
        anchors.fill: parent
        radius: root.radius
        visible: root.highlight
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.glassHighlight }
            GradientStop { position: 0.5; color: "transparent" }
        }
    }
}
