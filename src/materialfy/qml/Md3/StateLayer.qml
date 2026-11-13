import QtQuick
import QtQuick.Effects

// Интерактивная поверхность MD3: слой hover/press + ripple, обрезанный по скруглению.
// Кладётся поверх содержимого компонента: StateLayer { anchors.fill: parent; radius: ... }
MouseArea {
    id: root

    property color color: Theme.fgSurface
    property real radius: 0
    property real topLeftRadius: radius
    property real topRightRadius: radius
    property real bottomLeftRadius: radius
    property real bottomRightRadius: radius
    property bool ripple: true
    property bool dragged: false

    // на касаниях наведения нет: синтезированная из касания мышь оставляет «залипшую» подсветку
    hoverEnabled: !Theme.mobile
    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor

    readonly property real _opacity: !enabled ? 0
        : dragged ? Theme.stateLayer.dragged
        : pressed ? Theme.stateLayer.pressed
        : containsMouse ? Theme.stateLayer.hover
        : activeFocus ? Theme.stateLayer.focus : 0

    Rectangle {
        anchors.fill: parent
        color: root.color
        opacity: root._opacity
        topLeftRadius: root.topLeftRadius
        topRightRadius: root.topRightRadius
        bottomLeftRadius: root.bottomLeftRadius
        bottomRightRadius: root.bottomRightRadius
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    // Ripple создаётся при первом нажатии: StateLayer в списках сотни (у строки трека их несколько),
    // а нажимают единицы — волна, маска и анимации каждого стоили бы при каждом открытии страницы.
    // Слой включается только на время анимации, чтобы не держать лишних FBO.
    property Item _ripple: null
    // место волны — под содержимым, вложенным в StateLayer
    Item { id: rippleSlot; anchors.fill: parent }

    Component {
        id: rippleComponent
        Item {
            id: rippleHost
            anchors.fill: parent
            visible: wave.opacity > 0
            layer.enabled: visible
            layer.effect: MultiEffect {
                maskEnabled: true
                maskSource: mask
                maskThresholdMin: 0.5
                maskSpreadAtMin: 1.0
            }

            function start(x, y) {
                fade.stop()
                wave.cx = x
                wave.cy = y
                grow.restart()
            }
            function release() { fade.start() }

            Rectangle {
                id: wave
                property real cx: 0
                property real cy: 0
                readonly property real maxR: Math.hypot(Math.max(cx, root.width - cx), Math.max(cy, root.height - cy))
                x: cx - width / 2
                y: cy - height / 2
                width: 2 * maxR * scaleFactor
                height: width
                radius: width / 2
                color: root.color
                opacity: 0
                property real scaleFactor: 0
            }

            Rectangle {
                id: mask
                parent: rippleSlot      // рядом с волной, не внутри её слоя
                anchors.fill: parent
                visible: false
                // слой-маска — только пока виден ripple
                layer.enabled: rippleHost.visible
                topLeftRadius: root.topLeftRadius
                topRightRadius: root.topRightRadius
                bottomLeftRadius: root.bottomLeftRadius
                bottomRightRadius: root.bottomRightRadius
            }

            ParallelAnimation {
                id: grow
                NumberAnimation {
                    target: wave; property: "scaleFactor"; from: 0.1; to: 1
                    duration: Theme.mobile ? Theme.motion.spatialDefault : 450
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
                NumberAnimation { target: wave; property: "opacity"; to: Theme.stateLayer.pressed; duration: 75 }
            }

            NumberAnimation {
                id: fade
                target: wave; property: "opacity"; to: 0; duration: Theme.mobile ? Theme.motion.effectsDefault : 300
            }
        }
    }

    onPressed: mouse => {
        if (!ripple || Theme.reducedMotion)
            return
        if (!_ripple)
            _ripple = rippleComponent.createObject(rippleSlot)
        _ripple.start(mouse.x, mouse.y)
    }
    onReleased: if (_ripple) _ripple.release()
    onCanceled: if (_ripple) _ripple.release()
}
