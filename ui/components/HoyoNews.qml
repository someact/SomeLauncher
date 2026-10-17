import QtQuick
import Quickshell.Widgets
import qs.core

// The Online page's news card: HoYoPlay's banners (one at a time, turning
// every few seconds unless motion is reduced; dots pick one) over the posts,
// in tabs: Events, Notices, Info. A click opens the link in the browser.
// `content` is a hoyo.content answer.
Rectangle {
    id: root

    property var content: null
    readonly property var banners: content ? content.banners.filter(b => b.icon) : []
    readonly property var tabs: {
        const posts = content ? content.posts : [];
        return [{ id: "activity", name: "Events" }, { id: "announce", name: "Notices" }, { id: "info", name: "Info" }].filter(t => posts.some(p => p.kind === t.id));
    }
    property string tab: ""
    readonly property string shownTab: tabs.some(t => t.id === tab) ? tab : tabs.length ? tabs[0].id : ""
    readonly property var posts: content ? content.posts.filter(p => p.kind === shownTab).slice(0, 4) : []
    property int banner: 0
    onContentChanged: banner = 0

    implicitHeight: col.implicitHeight + Theme.space.m * 2
    radius: Theme.radius.large
    color: Qt.rgba(0, 0, 0, 0.42)
    border.width: 1
    border.color: Theme.glassRim
    visible: banners.length > 0 || tabs.length > 0

    Timer {
        interval: 6000
        repeat: true
        running: root.banners.length > 1 && !Motion.reduced && root.visible && !hover.hovered
        onTriggered: root.banner = (root.banner + 1) % root.banners.length
    }
    HoverHandler {
        id: hover
    }

    Column {
        id: col
        x: Theme.space.m
        y: Theme.space.m
        width: parent.width - Theme.space.m * 2
        spacing: Theme.space.s

        ClippingRectangle {
            id: bannerBox
            width: parent.width
            // HoYoPlay's banners are 690 x 320
            height: Math.round(width * 320 / 690)
            visible: root.banners.length > 0
            radius: Theme.radius.normal
            color: Qt.rgba(1, 1, 1, 0.06)

            Repeater {
                model: root.banners
                Image {
                    required property var modelData
                    required property int index
                    anchors.fill: parent
                    source: modelData.icon
                    sourceSize: Qt.size(Math.ceil(bannerBox.width * Screen.devicePixelRatio), Math.ceil(bannerBox.height * Screen.devicePixelRatio))
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                    opacity: index === root.banner && status === Image.Ready ? 1 : 0
                    visible: opacity > 0
                    Behavior on opacity {
                        NumberAnimation { duration: Motion.slow }
                    }
                }
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    const b = root.banners[root.banner];
                    if (b && b.link)
                        Qt.openUrlExternally(b.link);
                }
            }
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Theme.space.s
                spacing: 6
                visible: root.banners.length > 1
                Repeater {
                    model: root.banners.length
                    Rectangle {
                        required property int index
                        width: index === root.banner ? 18 : 6
                        height: 6
                        radius: 3
                        color: index === root.banner ? Theme.fgOnArt : Qt.rgba(1, 1, 1, 0.45)
                        Behavior on width {
                            Spring { preset: "snappy" }
                        }
                        MouseArea {
                            anchors.fill: parent
                            anchors.margins: -6
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.banner = parent.index
                        }
                    }
                }
            }
        }

        // Tabs: plain labels, the picked one bright and underlined
        Row {
            visible: root.tabs.length > 0
            spacing: Theme.space.l
            leftPadding: Theme.space.xs
            topPadding: Theme.space.xs
            Repeater {
                model: root.tabs
                Item {
                    required property var modelData
                    readonly property bool on: modelData.id === root.shownTab
                    width: tabLabel.implicitWidth
                    height: 28
                    SText {
                        id: tabLabel
                        anchors.verticalCenter: parent.verticalCenter
                        text: parent.modelData.name
                        color: parent.on ? Theme.fgOnArt : Theme.fgOnArtDim
                        font.weight: parent.on ? Theme.font.weightTitle : Theme.font.weight
                    }
                    Rectangle {
                        anchors.bottom: parent.bottom
                        width: parent.width
                        height: 2
                        radius: 1
                        color: Theme.primary
                        visible: parent.on
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.tab = parent.modelData.id
                    }
                }
            }
        }

        Column {
            width: parent.width
            Repeater {
                model: root.posts
                Rectangle {
                    required property var modelData
                    width: parent.width
                    height: 34
                    radius: Theme.radius.small
                    color: postMouse.containsMouse ? Qt.rgba(1, 1, 1, 0.08) : "transparent"
                    SText {
                        anchors.left: parent.left
                        anchors.leftMargin: Theme.space.xs
                        anchors.right: date.left
                        anchors.rightMargin: Theme.space.m
                        anchors.verticalCenter: parent.verticalCenter
                        text: parent.modelData.title
                        color: Theme.fgOnArt
                    }
                    SText {
                        id: date
                        anchors.right: parent.right
                        anchors.rightMargin: Theme.space.xs
                        anchors.verticalCenter: parent.verticalCenter
                        text: parent.modelData.date
                        color: Theme.fgOnArtDim
                        font.pixelSize: Theme.font.small
                    }
                    MouseArea {
                        id: postMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: if (parent.modelData.link) Qt.openUrlExternally(parent.modelData.link)
                    }
                }
            }
        }
    }
}
