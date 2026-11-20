import QtQuick
import QtMultimedia
import Md3

// Canvas трека: зацикленное видео без звука на весь фон плеера. Грузится Loader'ом —
// если QtMultimedia нет в сборке, плеер остаётся с обычным фоном.
Item {
    id: root

    property url source
    property bool running: true
    // сколько видео видно (Settings.canvasOpacity): под ним размытая обложка
    property real strength: 1
    readonly property bool ready: player.hasVideo && player.playbackState === MediaPlayer.PlayingState

    opacity: ready ? strength : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

    MediaPlayer {
        id: player
        source: root.source
        loops: MediaPlayer.Infinite
        videoOutput: output
        onSourceChanged: if (root.running) play()
    }
    VideoOutput {
        id: output
        anchors.fill: parent
        fillMode: VideoOutput.PreserveAspectCrop
    }
    onRunningChanged: running ? player.play() : player.pause()
}
