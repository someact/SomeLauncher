import QtQuick
import qs.core

// Material Symbols glyph by ligature name, e.g. Icon { name: "sports_esports" }
// Qt opens a font face per distinct variable-axis set, so FILL snaps to 0/1 and
// opsz to the font's optical sizes. Never animate an axis.
Text {
    property string name
    property real size: 20
    property real fill: 0

    readonly property int _opsz: size >= 44 ? 48 : size >= 32 ? 40 : size >= 22 ? 24 : 20

    text: name
    color: Theme.fgSurface
    renderType: Text.NativeRendering
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    font.family: Theme.font.icon
    font.pixelSize: size
    font.variableAxes: ({ "FILL": fill >= 0.5 ? 1 : 0, "opsz": _opsz })
}
