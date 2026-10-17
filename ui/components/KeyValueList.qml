import QtQuick
import qs.core

// Edits a flat {key: value} object (env vars, DLL overrides): a row per entry
// with a remove button, and an empty row at the end that adds one.
// `edited(object)` fires with the whole new object.
Column {
    id: root

    property var value: ({})
    property string keyPlaceholder: "NAME"
    property string valuePlaceholder: "value"
    signal edited(var object)

    readonly property var keys: Object.keys(value ?? {}).sort()

    spacing: Theme.space.s

    function set(oldKey, key, val) {
        const o = Object.assign({}, value);
        if (oldKey !== "")
            delete o[oldKey];
        if (key !== "")
            o[key] = val;
        edited(o);
    }

    Repeater {
        model: root.keys
        Row {
            required property string modelData
            spacing: Theme.space.s
            Field {
                width: (root.width - 40 - Theme.space.s * 2) * 0.45
                value: parent.modelData
                mono: true
                onCommitted: text => root.set(parent.modelData, text.trim(), root.value[parent.modelData])
            }
            Field {
                width: (root.width - 40 - Theme.space.s * 2) * 0.55
                value: root.value[parent.modelData] ?? ""
                mono: true
                onCommitted: text => root.set(parent.modelData, parent.modelData, text)
            }
            IconButton {
                width: 40
                height: 40
                icon: "close"
                iconSize: 18
                onClicked: root.set(parent.modelData, "", "")
            }
        }
    }
    // The new entry: added once it has a name
    Row {
        spacing: Theme.space.s
        Field {
            id: newKey
            width: (root.width - 40 - Theme.space.s * 2) * 0.45
            placeholder: root.keyPlaceholder
            mono: true
        }
        Field {
            id: newValue
            width: (root.width - 40 - Theme.space.s * 2) * 0.55
            placeholder: root.valuePlaceholder
            mono: true
            onCommitted: add.clicked()
        }
        IconButton {
            id: add
            width: 40
            height: 40
            icon: "add"
            iconSize: 20
            enabled: newKey.text.trim() !== ""
            onClicked: {
                const k = newKey.text.trim();
                if (k === "")
                    return;
                root.set("", k, newValue.text);
                newKey.text = "";
                newValue.text = "";
            }
        }
    }
}
