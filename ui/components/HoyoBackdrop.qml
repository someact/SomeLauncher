import QtQuick
import QtMultimedia
import qs.core

// One HoYoPlay background for the Online page: the still picture, its webm
// loop over it (only while `live`; unloaded otherwise, so a hidden, unfocused
// or paused launcher decodes nothing) and the theme picture over both. The
// three share one 16:9 crop, so the theme's logo and text sit where they were
// drawn; the crop leans left, where the theme puts them. Files come from the
// daemon's cache (hoyo.media), a loop is downloaded once.
Item {
    id: root

    property var background: null            // {image, video?, theme?}
    property bool live: false
    readonly property bool hasTheme: theme.status === Image.Ready
    readonly property bool ready: still.status === Image.Ready

    property string imagePath: ""
    property string videoPath: ""
    property string themePath: ""

    clip: true

    function local(url, done) {
        if (!url) {
            done("");
            return;
        }
        Daemon.call("hoyo.media", { url }, (r, e) => done(r ? Paths.url(r.path) : ""));
    }
    // Each answer is checked against the background still wanted
    onBackgroundChanged: {
        imagePath = videoPath = themePath = "";
        const b = background;
        if (!b)
            return;
        local(b.image, p => {
            if (root.background === b)
                root.imagePath = p;
        });
        local(b.theme ?? "", p => {
            if (root.background === b)
                root.themePath = p;
        });
        // The loop only once someone may watch it
        if (b.video && live)
            fetchVideo();
    }
    onLiveChanged: if (live && background && background.video && videoPath === "") fetchVideo()
    function fetchVideo() {
        const b = background;
        local(b.video, p => {
            if (root.background === b)
                root.videoPath = p;
        });
    }

    // 16:9, covering the item; overflow is cropped 20 % from the left
    Item {
        id: canvas
        readonly property real aspect: 16 / 9
        readonly property bool tall: root.width / Math.max(1, root.height) < aspect
        width: tall ? root.height * aspect : root.width
        height: tall ? root.height : root.width / aspect
        x: -(width - root.width) * 0.2
        y: -(height - root.height) * 0.5
        readonly property size decode: Qt.size(Math.ceil(width * Screen.devicePixelRatio), Math.ceil(height * Screen.devicePixelRatio))

        Image {
            id: still
            anchors.fill: parent
            source: root.imagePath
            sourceSize: canvas.decode
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity {
                NumberAnimation { duration: Motion.slow }
            }
        }
        Loader {
            anchors.fill: parent
            active: root.live && root.videoPath !== ""
            sourceComponent: Item {
                id: loop
                // Set by the first frame; a loop going back to 0 keeps it
                property bool started: false
                MediaPlayer {
                    id: player
                    source: root.videoPath
                    videoOutput: out
                    loops: MediaPlayer.Infinite
                    Component.onCompleted: play()
                    onPositionChanged: if (player.position > 0) loop.started = true
                    onErrorOccurred: (error, message) => console.warn("Online: background loop:", message)
                }
                VideoOutput {
                    id: out
                    anchors.fill: parent
                    fillMode: VideoOutput.PreserveAspectCrop
                    // Over the still once frames come, so the switch never flashes
                    opacity: loop.started ? 1 : 0
                    Behavior on opacity {
                        NumberAnimation { duration: Motion.slow }
                    }
                }
            }
        }
        Image {
            id: theme
            anchors.fill: parent
            source: root.themePath
            sourceSize: canvas.decode
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity {
                NumberAnimation { duration: Motion.slow }
            }
        }
    }
}
