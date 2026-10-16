pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Material 3 colors from matugen plus design tokens, the same names as SomeShell's
// core/Theme.qml so components move between the two unchanged. Colors come from
// SomeShell's matugen output when it exists, else our own (Paths.colors), else
// the fallbacks below. The file is watched: a new wallpaper recolors live.
Singleton {
    id: root

    property var c: ({})
    function pick(name, fallback) {
        return c[name] ?? fallback;
    }

    readonly property color primary: pick("primary", "#c6c0ff")
    readonly property color fgPrimary: pick("on_primary", "#2e2a60")
    readonly property color primaryContainer: pick("primary_container", "#454077")
    readonly property color fgPrimaryContainer: pick("on_primary_container", "#e4dfff")
    readonly property color secondaryContainer: pick("secondary_container", "#47455a")
    readonly property color fgSecondaryContainer: pick("on_secondary_container", "#e4dff9")
    readonly property color tertiary: pick("tertiary", "#eab8d0")
    readonly property color error: pick("error", "#ffb4ab")
    readonly property color surface: pick("surface", "#131318")
    readonly property color surfaceLow: pick("surface_container_low", "#1b1b21")
    readonly property color surfaceContainer: pick("surface_container", "#1f1f25")
    readonly property color surfaceHigh: pick("surface_container_high", "#2a292f")
    readonly property color surfaceHighest: pick("surface_container_highest", "#35343a")
    readonly property color fgSurface: pick("on_surface", "#e5e1e9")
    readonly property color fgSurfaceVariant: pick("on_surface_variant", "#c8c5d0")
    readonly property color outline: pick("outline", "#928f9a")
    readonly property color outlineVariant: pick("outline_variant", "#47464f")
    readonly property color shadow: pick("shadow", "#000000")

    // Over game art the text is always light, whatever the theme mode
    readonly property color fgOnArt: "#f4f2f8"
    readonly property color fgOnArtDim: "#c4c2cc"
    readonly property color scrim: Qt.rgba(0, 0, 0, 0.55)

    // Glass: a tint over the compositor frost (Hyprland blurs the window), a
    // 1 px light rim and a soft top highlight (components/Glass.qml). Without
    // frost (SomeShell's game mode) the tint turns nearly opaque.
    readonly property bool followShell: Config.glass.followShell && Shell.glass !== null
    readonly property real glassAlpha: followShell ? (Shell.glass.tint ?? Config.glass.tint) : Config.glass.tint
    readonly property bool glassRimOn: followShell ? (Shell.glass.rim ?? Config.glass.rim) : Config.glass.rim
    readonly property bool blur: !Shell.gameMode
    readonly property color glassTint: Qt.rgba(surface.r, surface.g, surface.b, blur ? glassAlpha : 0.94)
    readonly property color glassRaised: Qt.rgba(fgSurface.r, fgSurface.g, fgSurface.b, 0.07)
    readonly property color glassRaisedHover: Qt.rgba(fgSurface.r, fgSurface.g, fgSurface.b, 0.12)
    readonly property color glassRim: Qt.rgba(1, 1, 1, glassRimOn ? 0.14 : 0.05)
    readonly property color glassHighlight: Qt.rgba(1, 1, 1, glassRimOn ? 0.07 : 0)

    readonly property QtObject radius: QtObject {
        readonly property int small: 8
        readonly property int normal: 14
        readonly property int large: 22
        readonly property int xl: 32
        readonly property int full: 999
    }
    readonly property QtObject space: QtObject {
        readonly property int xs: 4
        readonly property int s: 8
        readonly property int m: 12
        readonly property int l: 16
        readonly property int xl: 24
        readonly property int xxl: 40
    }
    readonly property QtObject font: QtObject {
        readonly property string ui: "Google Sans Flex"
        readonly property string mono: "JetBrainsMono Nerd Font"
        readonly property string icon: "Material Symbols Rounded"
        readonly property int weight: 450
        readonly property int weightTitle: 550
        readonly property int weightDisplay: 650
        readonly property int small: 11
        readonly property int normal: 13
        readonly property int large: 16
        readonly property int title: 20
        readonly property int headline: 28
        readonly property int display: 44
    }

    FileView {
        path: Shell.present ? Paths.shellColors : Paths.colors
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: {
            try {
                root.c = JSON.parse(text());
            } catch (e) {
                console.warn("Theme: bad colors file", e);
            }
        }
    }
}
