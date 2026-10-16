import QtQuick
import qs.core

// Text in the launcher's type: Google Sans Flex at the body weight, tabular
// digits so playtime and progress never jitter. `dim` is the secondary color.
Text {
    property bool mono: false
    property bool dim: false

    color: dim ? Theme.fgSurfaceVariant : Theme.fgSurface
    renderType: Text.NativeRendering
    verticalAlignment: Text.AlignVCenter
    elide: Text.ElideRight
    font.family: mono ? Theme.font.mono : Theme.font.ui
    font.pixelSize: Theme.font.normal
    font.weight: Theme.font.weight
    font.features: ({ "tnum": 1 })
}
