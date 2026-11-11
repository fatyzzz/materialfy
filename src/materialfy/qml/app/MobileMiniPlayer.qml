import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Мини-плеер телефона: плашка над нижней навигацией — обложка, трек и исполнитель (или «Играет на …»),
// лайк, «играть/пауза»; снизу — тонкая полоска прогресса.
// Тап или свайп вверх — полноэкранный плеер; свайп влево/вправо — следующий/предыдущий трек.
Item {
    id: root

    readonly property Item coverItem: coverBox
    signal openNowPlaying(point coverCenter)

    implicitHeight: 64
    // полноэкранный плеер открыт — плашки нет; он уходит — она появляется сама (MobileMain: holdsMini)
    property bool suppressed: false
    readonly property bool shown: Player.hasTrack && !suppressed
    visible: opacity > 0.01
    opacity: shown ? 1 : 0
    // прячется быстро (плеер раскрывается поверх), появляется чуть дольше
    Behavior on opacity { NumberAnimation { duration: root.shown ? Theme.motion.effectsDefault : Theme.motion.effectsFast } }
    // появляется, выезжая снизу и дорастая с лёгким пружинным перелётом; уходит вниз
    transform: [
        Scale {
            origin.x: root.width / 2
            origin.y: root.height
            xScale: root._appear
            yScale: root._appear
        },
        Translate { y: (1 - root._appear) * 160 }
    ]
    property real _appear: shown ? 1 : 0.8
    Behavior on _appear {
        enabled: Theme.reducedMotion !== true
        NumberAnimation {
            duration: root.shown ? Theme.motion.spatialDefault : Theme.motion.effectsDefault
            easing.type: Easing.BezierSpline
            easing.bezierCurve: root.shown ? Theme.motion.springDefault : Theme.motion.emphasizedAccelerate
        }
    }

    function _open() {
        root.openNowPlaying(coverBox.mapToItem(null, coverBox.width / 2, coverBox.height / 2))
    }

    PlayerProgress { id: playerProgress; active: root.visible; smooth: false }

    // Переключение свайпом: плашка едет за пальцем 1:1, после порога улетает за край, трек переключается,
    // и плашка с новым треком въезжает с другой стороны (когда плеер сообщил о смене или через 1,2 с)
    property real _dx: 0
    property real _dy: 0
    property int _dir: 0                     // -1 — следующий (уехала влево), 1 — предыдущий, 0 — нет
    function _swipeAway(dir) {
        MobileShell.haptic()
        _dir = dir
        settle.stop()
        flyOut.to = dir * (root.width + 24)
        flyOut.restart()
    }
    NumberAnimation {
        id: flyOut
        target: root; property: "_dx"
        duration: 160; easing.type: Easing.InCubic
        onFinished: {
            if (root._dir < 0)
                Player.next()
            else
                Player.previous()
            // новый трек въедет с противоположной стороны
            root._dx = -root._dir * (root.width * 0.35)
            arrivalTimeout.restart()
        }
    }
    NumberAnimation {
        id: settle
        target: root; property: "_dx"; to: 0
        duration: Theme.motion.spatialDefault
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }
    NumberAnimation { id: settleY; target: root; property: "_dy"; to: 0; duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic }
    function _arrive() {
        if (_dir === 0)
            return
        _dir = 0
        arrivalTimeout.stop()
        settle.restart()
    }
    Timer { id: arrivalTimeout; interval: 1200; onTriggered: root._arrive() }
    // Подписи нового трека проявляются заново, чуть снизу (при свайпе плашка и так въезжает целиком)
    property real _infoIn: 1
    property string _lastTrack: Player.trackId
    NumberAnimation {
        id: infoAnim
        target: root; property: "_infoIn"; to: 1
        duration: Theme.motion.spatialDefault
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }
    Connections {
        target: Player
        function onTrackChanged() {
            if (root._dir === 0 && root._lastTrack !== Player.trackId && root.visible && Theme.reducedMotion !== true) {
                root._infoIn = 0
                infoAnim.restart()
            }
            root._lastTrack = Player.trackId
            root._arrive()
        }
        // play/pause: иконка коротко пружинит (у playing общий сигнал stateChanged — с буферизацией)
        function onStateChanged() {
            if (root._wasPlaying === Player.playing)
                return
            root._wasPlaying = Player.playing
            if (Theme.reducedMotion !== true)
                playPop.restart()
        }
    }
    property bool _wasPlaying: Player.playing

    Item {
        id: card
        width: parent.width
        height: parent.height
        x: root._dx
        y: root._dy
        // нажатие чуть поджимает плашку — отклик сразу под пальцем, до открытия плеера
        scale: cardTap.pressed && !swipe.active ? 0.97 : 1
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }
        // ждёт новый трек за краем — прозрачная; въезжая, проявляется
        opacity: root._dir !== 0 && !flyOut.running ? 0 : 1 - Math.min(1, Math.abs(x) / root.width) * 0.7

        Rectangle {
            id: bg
            anchors.fill: parent
            radius: Theme.shape.large
            color: Theme.surfaceContainerHighest
        }
        Elevation { target: bg; level: 2 }

        // Тонкий прогресс по нижнему краю, внутри скругления
        Item {
            anchors.fill: parent
            clip: true
            Rectangle {
                anchors.bottom: parent.bottom
                anchors.left: parent.left
                anchors.leftMargin: Theme.shape.large
                width: Math.max(0, (parent.width - 2 * Theme.shape.large) * playerProgress.value)
                height: 2
                radius: 1
                color: Theme.primary
            }
            Rectangle {
                anchors.bottom: parent.bottom
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: Theme.shape.large
                anchors.rightMargin: Theme.shape.large
                height: 2
                radius: 1
                color: Theme.fgSurface
                opacity: 0.12
                z: -1
            }
        }

        StateLayer {
            id: cardTap
            anchors.fill: parent
            radius: Theme.shape.large
            enabled: Player.hasTrack
            onClicked: root._open()
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 4
            spacing: 12

            Item {
                id: coverBox
                Layout.preferredWidth: 48
                Layout.preferredHeight: 48
                MorphImage {
                    id: miniCover
                    anchors.fill: parent
                    visible: Player.cover !== ""
                    source: Player.cover
                    fallbackKind: Player.album !== "" ? "album" : "track"
                    fallbackArtist: Player.artist.split(", ")[0]
                    fallbackTitle: Player.album !== "" ? Player.album : Player.title
                    shape: Player.playing ? "cookie12" : "softSquare"
                    MaskSpin { target: miniCover; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true }
                }
                MorphShape {
                    anchors.fill: parent
                    visible: Player.cover === ""
                    shape: "softSquare"
                    color: Theme.surfaceContainerHigh
                    Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 24; color: Theme.fgSurfaceVariant }
                }
                DeviceBadge {
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.rightMargin: -4
                    anchors.bottomMargin: -4
                    ringColor: Theme.surfaceContainerHighest
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                opacity: root._infoIn
                transform: Translate { y: (1 - root._infoIn) * 8 }
                Label {
                    Layout.fillWidth: true
                    // играет другое устройство, а данных трека ещё нет — не пустая строка
                    text: Player.title !== "" ? Player.title : remoteLabel.shown ? "Spotify Connect" : ""
                    type: "titleSmall"
                    weight: 600
                }
                Label {
                    Layout.fillWidth: true
                    visible: !remoteLabel.shown || Player.errorText !== ""
                    text: Player.errorText !== "" ? Player.errorText : Player.artist
                    type: "bodySmall"
                    color: Player.errorText !== "" ? Theme.error : Theme.fgSurfaceVariant
                }
                RemoteLabel {
                    id: remoteLabel
                    Layout.fillWidth: true
                    maxWidth: parent.width
                    visible: shown && Player.errorText === ""
                    brief: false
                }
            }

            // устройства — здесь, только пока музыка не на этом телефоне; иначе — в полноэкранном плеере
            DeviceButton {
                visible: Player.remote === true || Player.deviceLost === true || Player.transferring === true
                iconOnly: true
            }
            LikeButton { visible: !Player.isEpisode; track: Player.track }
            IconButton {
                visible: Player.isEpisode
                icon: "bookmark"
                checkable: true
                autoToggle: false
                checked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
                enabled: Player.uri !== ""
                onClicked: Library.toggleSaved(Player.uri)
            }
            Item {
                implicitWidth: 48
                implicitHeight: 48
                IconButton {
                    id: playToggle
                    anchors.fill: parent
                    icon: Player.buffering || Player.playing ? "pause" : "play_arrow"
                    Accessible.name: Player.buffering || Player.playing ? I18n.t("Pause") : I18n.t("Play")
                    iconSize: 30
                    iconFill: 1
                    iconColor: Theme.fgSurface
                    enabled: Player.hasTrack
                    onClicked: Player.togglePlay()
                }
                SequentialAnimation {
                    id: playPop
                    NumberAnimation { target: playToggle; property: "scale"; to: 0.78; duration: 70; easing.type: Easing.OutCubic }
                    NumberAnimation { target: playToggle; property: "scale"; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast }
                }
                LoadingIndicator {
                    anchors.centerIn: parent
                    size: 44
                    color: Theme.primary
                    visible: Player.buffering
                    opacity: 0.5
                }
            }
        }
    }

    // Слой поверх плашки: обработчик на самом root не видел касаний — их забирали кнопки и область нажатия
    // плашки (MouseArea принимает нажатие, до обработчиков родителя оно не доходит). Здесь DragHandler получает
    // нажатие первым, держит его пассивно (тап по кнопке проходит) и забирает жест, когда палец поехал.
    Item {
        anchors.fill: parent
        z: 10
        DragHandler {
            id: swipe
            target: null
            enabled: Player.hasTrack
            // translation обнуляется раньше, чем приходит active=false: смещение и скорость запоминаем по ходу
            property point last: Qt.point(0, 0)
            property real vx: 0
            onTranslationChanged: {
                if (!active || root._dir !== 0)
                    return
                last = Qt.point(translation.x, translation.y)
                vx = centroid.velocity.x
                const horizontal = Math.abs(last.x) >= Math.abs(last.y)
                root._dx = horizontal ? last.x : 0
                root._dy = horizontal ? 0 : Math.max(-24, Math.min(0, last.y * 0.3))
            }
            onActiveChanged: {
                if (active) {
                    last = Qt.point(0, 0)
                    vx = 0
                    settle.stop()
                    settleY.stop()
                }
            }
            onGrabChanged: transition => {
                const released = transition === PointerDevice.UngrabExclusive
                if (!released && transition !== PointerDevice.CancelGrabExclusive)
                    return
                if (root._dir !== 0)
                    return
                const t = last
                settleY.restart()
                const flick = Math.abs(vx) > 900 && Math.abs(t.x) > 24
                if (released && (Math.abs(t.x) > root.width * 0.28 || flick) && Math.abs(t.x) > Math.abs(t.y)) {
                    root._swipeAway(t.x < 0 ? -1 : 1)
                } else {
                    settle.restart()
                    if (released && t.y < -40 && Math.abs(t.y) > Math.abs(t.x))
                        root._open()
                }
            }
        }
    }
}
