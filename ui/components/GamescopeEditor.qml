import QtQuick
import qs.core

// Gamescope's options (2f): output and game size, refresh, FSR, HDR,
// fullscreen. `value` is a gamescope object; `edited(v)` fires with the whole
// new one. The game settings sheet and the launch defaults share it.
Column {
    id: gse

    property var value: ({})
    signal edited(var v)
    spacing: Theme.space.m
    function set(k, v) {
        const o = Object.assign({}, value);
        if (v === null || v === "" || (typeof v === "number" && isNaN(v)))
            delete o[k];
        else
            o[k] = v;
        edited(o);
    }
    Row {
        spacing: Theme.space.s
        NumField { label: "Width"; key: "width"; editor: gse }
        NumField { label: "Height"; key: "height"; editor: gse }
        NumField { label: "Refresh"; key: "refresh"; editor: gse }
    }
    Row {
        spacing: Theme.space.s
        NumField { label: "Game width"; key: "gameWidth"; editor: gse }
        NumField { label: "Game height"; key: "gameHeight"; editor: gse }
    }
    SText {
        width: parent.width
        text: "Empty sizes use the monitor's. A game size under the output upscales" + (gse.value.fsr ? " with FSR." : ".")
        dim: true
        font.pixelSize: Theme.font.small
        wrapMode: Text.Wrap
    }
    Flow {
        width: parent.width
        spacing: Theme.space.s
        Repeater {
            model: [{ id: "fsr", name: "FSR" }, { id: "hdr", name: "HDR" }, { id: "fullscreen", name: "Fullscreen" }]
            Chip {
                required property var modelData
                label: modelData.name
                icon: gse.value[modelData.id] ? "check" : ""
                active: !!gse.value[modelData.id]
                onClicked: gse.set(modelData.id, gse.value[modelData.id] ? null : true)
            }
        }
    }

    // A number of the gamescope editor
    component NumField: Column {
        id: num
        property Item editor
        property string label
        property string key
        spacing: 2
        SText {
            text: parent.label
            dim: true
            font.pixelSize: Theme.font.small
        }
        Field {
            width: 104
            mono: true
            value: num.editor && num.editor.value[num.key] ? String(num.editor.value[num.key]) : ""
            placeholder: "auto"
            onCommitted: text => num.editor.set(num.key, text.trim() === "" ? null : parseInt(text))
        }
    }
}
