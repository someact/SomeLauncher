import QtQuick
import qs.core

// SpringAnimation with a named preset from core/Motion.qml. Only inside a
// Behavior (`Behavior on x { Spring { preset: "snappy" } }`) or as
// `Spring on <prop>`: inside a Sequential/ParallelAnimation Qt segfaults.
SpringAnimation {
    property string preset: "smooth"
    readonly property var params: Motion.get(preset)
    spring: params.spring
    damping: params.damping
    mass: params.mass
    epsilon: 0.05
}
