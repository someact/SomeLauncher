pragma Singleton

import QtQuick
import Quickshell

// Spring presets, the same as SomeShell's (hypr/core/motion.lua matches them).
// Springs keep their velocity when retargeted, so every move can be interrupted.
// Use them through components/Spring.qml. While a game runs (5d) or with
// reduce-motion on, springs collapse to short critically damped moves.
Singleton {
    readonly property bool reduced: Config.motion.reduce || Shell.gameMode
    readonly property real speed: Math.max(0.25, Config.motion.speed)

    readonly property var presets: ({
        smooth: { spring: 4.2, damping: 0.42, mass: 1.0 },
        snappy: { spring: 6.5, damping: 0.55, mass: 1.0 },
        bouncy: { spring: 5.0, damping: 0.36, mass: 1.0 },
        gentle: { spring: 2.6, damping: 0.45, mass: 1.0 },
        reduced: { spring: 30, damping: 1.0, mass: 1.0 }
    })

    function get(name) {
        const p = presets[reduced ? "reduced" : name] ?? presets.smooth;
        return { spring: p.spring * speed * speed, damping: p.damping, mass: p.mass };
    }

    // For things that cannot spring (colors, opacity in sequences)
    readonly property int fast: reduced ? 80 : Math.round(140 / speed)
    readonly property int normal: reduced ? 120 : Math.round(220 / speed)
    readonly property int slow: reduced ? 160 : Math.round(380 / speed)
}
