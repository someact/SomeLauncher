import QtQuick
import qs.core

// An FPS cap: off, the monitor's refresh, or a number (a field shows for
// that). With `allowDefault` (a game's own setting) "Default" comes first and
// means null. `edited(v)`: null, false, "monitor" or a number.
Column {
    id: root

    property var value: null
    property bool allowDefault: false
    property int refresh: 0                  // the monitor's, for the label
    signal edited(var v)

    readonly property string mode: value === null || value === undefined ? (allowDefault ? "default" : "off") : value === false || value === 0 ? "off" : value === "monitor" ? "monitor" : "fps"
    property bool customOpen: false
    spacing: Theme.space.s

    Segmented {
        options: (root.allowDefault ? [{ id: "default", name: "Default" }] : []).concat([
            { id: "off", name: "Off" },
            { id: "monitor", name: root.refresh ? `Monitor (${root.refresh})` : "Monitor" },
            { id: "fps", name: "Custom" }
        ])
        current: root.customOpen ? "fps" : root.mode
        onPicked: id => {
            root.customOpen = id === "fps";
            if (id === "default")
                root.edited(null);
            else if (id === "off")
                root.edited(false);
            else if (id === "monitor")
                root.edited("monitor");
            else if (root.mode !== "fps")
                field.focusField();
        }
    }
    Row {
        visible: root.mode === "fps" || root.customOpen
        spacing: Theme.space.s
        Field {
            id: field
            width: 96
            mono: true
            value: typeof root.value === "number" && root.value > 0 ? String(root.value) : ""
            placeholder: "60"
            onCommitted: text => {
                const n = parseInt(text);
                if (n > 0) {
                    root.customOpen = false;
                    root.edited(n);
                }
            }
        }
        SText {
            anchors.verticalCenter: parent.verticalCenter
            text: "frames per second"
            dim: true
        }
    }
}
