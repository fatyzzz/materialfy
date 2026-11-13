import QtQuick
import Md3
import Materialfy.Core

// «Мне нравится» для трека (dict из модели или Player.track). Состояние — из Library, клик — переключить.
IconButton {
    id: root

    property var track: ({})
    readonly property string _id: track && track.trackId ? String(track.trackId) : ""
    readonly property bool liked: Library.likesRevision >= 0 && _id !== "" && Library.isLiked(_id)

    icon: "favorite"
    Accessible.name: liked ? I18n.t("Remove from Liked Songs") : I18n.t("Save to Liked Songs")
    checkable: true
    autoToggle: false
    checked: liked
    enabled: _id !== ""
    onClicked: {
        // телефон: лайк — с отзывом под пальцем, как у системных кнопок
        if (Theme.mobile)
            MobileShell.haptic()
        Library.toggleLike(track)
    }

    // Лайк: «удар сердца» и всплеск — кольцо расходится и гаснет, шесть искр разлетаются.
    // Снятие лайка — короткое «сжатие» без всплеска
    onLikedChanged: {
        if (!_ready || Theme.reducedMotion === true)
            return
        if (liked) {
            beat.restart()
            burst.restart()
        } else {
            shrink.restart()
        }
    }
    property bool _ready: false
    Component.onCompleted: _ready = true
    SequentialAnimation {
        id: beat
        NumberAnimation { target: root; property: "scale"; to: 1.22; duration: 110; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "scale"; to: 1; duration: 260; easing.type: Easing.OutBack; easing.overshoot: 2.5 }
    }
    SequentialAnimation {
        id: shrink
        NumberAnimation { target: root; property: "scale"; to: 0.82; duration: 90; easing.type: Easing.OutCubic }
        NumberAnimation { target: root; property: "scale"; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast }
    }

    property real _burst: 1             // 0 → 1 — ход всплеска; 1 — всплеска нет
    NumberAnimation { id: burst; target: root; property: "_burst"; from: 0; to: 1; duration: 440; easing.type: Easing.OutCubic }
    Item {
        anchors.centerIn: parent
        width: 0
        height: 0
        visible: root._burst < 1
        z: -1
        Rectangle {
            readonly property real d: 18 + root._burst * 26
            x: -d / 2
            y: -d / 2
            width: d
            height: d
            radius: d / 2
            color: "transparent"
            border.width: Math.max(0.5, 3 * (1 - root._burst))
            border.color: Theme.primary
            opacity: 0.8 * (1 - root._burst)
        }
        Repeater {
            model: 6
            Rectangle {
                required property int index
                readonly property real a: index * Math.PI / 3 + Math.PI / 6
                readonly property real r: 12 + root._burst * 16
                readonly property real s: 4.5 * (1 - root._burst * 0.6)
                x: Math.cos(a) * r - s / 2
                y: Math.sin(a) * r - s / 2
                width: s
                height: s
                radius: s / 2
                color: index % 2 ? Theme.primary : Theme.tertiary
                opacity: 1 - root._burst * root._burst
            }
        }
    }
}
