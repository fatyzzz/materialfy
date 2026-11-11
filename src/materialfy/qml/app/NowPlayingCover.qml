import QtQuick
import Materialfy.Core
import Md3

// Обложка полноэкранного плеера. На телефоне — короткое перекрёстное затухание с небольшим масштабом;
// на десктопе — морф через фигуру. Двойной буфер: новая показывается, когда загрузилась.
// pulse — «дыхание» фигуры в такт, spinning — медленное вращение рамки (обложка внутри неподвижна).
Item {
    id: root

    property string source
    property var restShape: "cookie12"
    property real pulse: 0
    property bool spinning: false
    property int direction: 1              // +1 — вперёд по очереди, -1 — назад: куда вращаться
    // обложку унёс свайп (NowPlaying): новая встаёт на место сразу, без морфа — въезд анимирует свайп
    property bool instantSwap: false
    readonly property bool pending: _pending
    signal swapped()

    property int _front: 0
    property bool _pending: false
    readonly property var _layers: [a, b]

    onSourceChanged: {
        const front = _layers[_front], back = _layers[1 - _front]
        if (front.source.toString() === "" || !visible) {   // первая обложка / плеер закрыт — без анимации
            front.source = source
            return
        }
        if (front.source.toString() === source)
            return
        back.source = source
        _pending = true
        if (back.status === Image.Ready)
            _go()
        else
            timeout.restart()
    }

    Timer { id: timeout; interval: 1500; onTriggered: if (root._pending) root._go() }
    Connections {
        target: root._layers[1 - root._front]
        function onStatusChanged() {
            if (root._pending && root._layers[1 - root._front].status === Image.Ready)
                root._go()
        }
    }

    function _go() {
        _pending = false
        timeout.stop()
        outAnim.complete()
        inAnim.complete()
        const oldL = _layers[_front], newL = _layers[1 - _front]
        if (instantSwap || Theme.reducedMotion || Theme.lowPower || !visible) {
            newL.jumpTo(restShape)
            newL.shape = Qt.binding(() => root.restShape)
            newL.scale = 1
            newL.rotation = 0
            newL.opacity = 1
            newL.z = 1
            oldL.z = 0
            oldL.opacity = 0
            _front = 1 - _front
            swapped()
            return
        }
        newL.jumpTo(Theme.mobile ? restShape : "flower8")
        newL.scale = Theme.mobile ? 0.96 : 0.2
        newL.rotation = Theme.mobile ? 0 : 120 * direction
        newL.opacity = 0
        newL.z = 1
        oldL.z = 0
        if (!Theme.mobile)
            oldL.shape = "cookie4"
        outAnim.target = oldL
        outRot.to = Theme.mobile ? 0 : -90 * direction
        inAnim.target = newL
        _front = 1 - _front
        newL.shape = Qt.binding(() => root.restShape)   // морф цветок → своя фигура
        outAnim.start()
        inAnim.start()
        swapped()
    }

    component Layer: MorphImage {
        id: layer
        required property int index
        anchors.fill: parent
        eager: true                        // закрытый плеер держит обложку готовой: раскрытие без подгрузки
        fallbackKind: Player.album !== "" ? "album" : "track"
        fallbackArtist: Player.artist.split(", ")[0]
        fallbackTitle: Player.album !== "" ? Player.album : Player.title
        duration: Theme.motion.spatialSlow
        shape: root.restShape
        pulse: index === root._front ? root.pulse : 0
        MaskSpin { target: layer; spinning: layer.index === root._front && root.spinning }
        visible: opacity > 0.01
    }

    Layer { id: a; index: 0 }
    Layer { id: b; index: 1; opacity: 0 }

    ParallelAnimation {
        id: outAnim
        property Item target
        NumberAnimation { target: outAnim.target; property: "scale"; to: Theme.mobile ? 0.97 : 0.15; duration: Theme.mobile ? Theme.motion.effectsDefault : 300; easing.type: Theme.mobile ? Easing.OutCubic : Easing.InCubic }
        NumberAnimation { id: outRot; target: outAnim.target; property: "rotation"; duration: Theme.mobile ? Theme.motion.effectsDefault : 300; easing.type: Theme.mobile ? Easing.OutCubic : Easing.InCubic }
        NumberAnimation { target: outAnim.target; property: "opacity"; to: 0; duration: Theme.mobile ? Theme.motion.effectsDefault : 300; easing.type: Theme.mobile ? Easing.OutCubic : Easing.InQuad }
    }
    SequentialAnimation {
        id: inAnim
        property Item target
        PauseAnimation { duration: Theme.mobile ? 0 : 100 }
        ParallelAnimation {
            NumberAnimation { target: inAnim.target; property: "scale"; to: 1; duration: Theme.mobile ? Theme.motion.spatialDefault : 540; easing.type: Theme.mobile ? Easing.OutCubic : Easing.OutBack; easing.overshoot: 1.25 }
            NumberAnimation { target: inAnim.target; property: "rotation"; to: 0; duration: Theme.mobile ? Theme.motion.spatialDefault : 540; easing.type: Easing.OutCubic }
            NumberAnimation { target: inAnim.target; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault; easing.type: Easing.OutCubic }
        }
    }
}
