import QtQuick
import QtQuick.Shapes
import Md3
import "../Md3/Shapes.js" as Shapes

// Шапка боковой панели: знак «Волна-M» и надпись «Materialfy» (локап как в wordmark.svg).
// Знак — кнопка «свернуть / развернуть панель», как в ChatGPT и Claude: при наведении или фокусе с клавиатуры
// печенька морфится в круг, «M» с бегунком растворяется, на её месте проявляется иконка действия.
// Морф и смена иконки — переходы свойств по токенам Theme.motion; при Theme.reducedMotion — мгновенно.
Item {
    id: root

    LogoPaths { id: logo }

    property real markSize: 40                 // диаметр печеньки
    property bool expanded: true               // true — действие «свернуть», false — «развернуть»
    property bool wordmarkVisible: expanded
    // Наведение или фокус; стенды и снимки могут выставить вручную
    property bool highlighted: area.containsMouse || mark.activeFocus
    readonly property string actionIcon: expanded ? "left_panel_close" : "left_panel_open"
    readonly property string actionText: expanded ? I18n.t("Collapse sidebar") : I18n.t("Expand sidebar")
    readonly property alias markItem: mark

    signal activated()

    readonly property real _k: markSize / 480  // координаты LogoPaths: печенька — квадрат 0..480
    readonly property bool _instant: Theme.reducedMotion

    implicitWidth: wordmark.x + wordmark.width
    implicitHeight: markSize

    Item {
        id: mark
        objectName: "sidebarLogoMark"
        width: root.markSize
        height: root.markSize
        anchors.verticalCenter: parent.verticalCenter

        activeFocusOnTab: true
        Accessible.role: Accessible.Button
        Accessible.name: root.actionText
        Accessible.description: "Materialfy"
        Accessible.onPressAction: root.activated()
        Keys.onPressed: e => {
            if (e.key === Qt.Key_Space || e.key === Qt.Key_Return || e.key === Qt.Key_Enter) {
                e.accepted = true
                root.activated()
            }
        }

        // 0 — знак (печенька + M), 1 — кнопка (круг + иконка действия)
        property real t: root.highlighted ? 1 : 0
        Behavior on t {
            enabled: !root._instant
            NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
        // «M» гаснет в первой половине морфа, иконка проявляется во второй (с небольшим нахлёстом,
        // чтобы не было пустого кадра) — без каши из двух глифов
        readonly property real glyphOut: Math.max(0, 1 - t / 0.55)
        readonly property real iconIn: Math.max(0, (t - 0.45) / 0.55)

        // Нажатие — короткое сжатие с пружиной
        scale: area.pressed ? 0.9 : 1
        Behavior on scale {
            enabled: !root._instant
            NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: Theme.motion.overshoot }
        }

        // Слой состояния MD3 (Lilac поверх Ultraviolet): наведение 8 %, фокус 10 %, нажатие 10 %
        property real stateOpacity: area.pressed ? Theme.stateLayer.pressed
                                    : root.highlighted ? (mark.activeFocus && !area.containsMouse ? Theme.stateLayer.focus : Theme.stateLayer.hover) : 0
        Behavior on stateOpacity { enabled: !root._instant; NumberAnimation { duration: Theme.motion.effectsFast } }

        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                // градиент icon.svg: (80, 40) → (432, 472) при печеньке 16..496
                fillGradient: LinearGradient {
                    x1: mark.width * 64 / 480; y1: mark.height * 24 / 480
                    x2: mark.width * 416 / 480; y2: mark.height * 456 / 480
                    GradientStop { position: 0; color: Qt.tint(logo.ultraviolet[0], Qt.alpha(logo.lilac, mark.stateOpacity)) }
                    GradientStop { position: 1; color: Qt.tint(logo.ultraviolet[1], Qt.alpha(logo.lilac, mark.stateOpacity)) }
                }
                PathPolyline {
                    // зубцы доворачиваются на полшага, пока сглаживаются в круг
                    path: Shapes.outline(logo.cookie, "circle", mark.t, 96, mark.width, mark.height, mark.t * Math.PI / 8, 0, 0, 0)
                }
            }
        }

        // Знак: волна-M и бегунок
        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            opacity: mark.glyphOut
            visible: opacity > 0
            scale: 0.6 + 0.4 * mark.glyphOut
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: logo.lilac
                scale: Qt.size(root._k, root._k)
                PathSvg { path: logo.glyph }
            }
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: logo.ember
                scale: Qt.size(root._k, root._k)
                PathSvg { path: logo.runner }
            }
        }

        Icon {
            anchors.centerIn: parent
            name: root.actionIcon
            size: 24
            weight: 500
            color: logo.lilac
            opacity: mark.iconIn
            visible: opacity > 0
            scale: 0.6 + 0.4 * mark.iconIn
            rotation: (1 - mark.iconIn) * (root.expanded ? 45 : -45)
        }

        // Индикатор фокуса MD3: кольцо 3 px с зазором 2 px, только при фокусе с клавиатуры
        Rectangle {
            anchors.centerIn: parent
            width: parent.width + 10
            height: width
            radius: width / 2
            color: "transparent"
            border.width: 3
            border.color: Theme.secondary
            visible: mark.activeFocus
        }

        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPressed: tip.hide()
            onClicked: root.activated()
        }
    }

    // Надпись в кривых из wordmark.svg (без знака), цвет onSurface; гаснет в рейле
    Shape {
        id: wordmark
        x: mark.x + logo.textLeft * root._k
        width: logo.textWidth * root._k
        height: root.markSize
        anchors.verticalCenter: parent.verticalCenter
        preferredRendererType: Shape.CurveRenderer
        opacity: root.wordmarkVisible ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root._instant ? 0 : Theme.motion.effectsFast } }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: Theme.fgSurface
            scale: Qt.size(root._k, root._k)
            PathSvg { path: logo.text }
        }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: logo.ember
            scale: Qt.size(root._k, root._k)
            PathSvg { path: logo.textDot }
        }
    }

    // Подсказка MD3 (plain tooltip). Живёт в корне окна: панель обрезает своё содержимое, а в рейле
    // подсказка стоит справа от знака, за краем панели. В развёрнутой панели — под знаком.
    Timer {
        id: tipDelay
        interval: 600
        onTriggered: tip.show()
    }
    Connections {
        target: area
        function onContainsMouseChanged() {
            if (area.containsMouse && !area.pressed)
                tipDelay.restart()
            else if (!mark.activeFocus) {
                tipDelay.stop()
                tip.hide()
            }
        }
    }
    Connections {
        target: mark
        function onActiveFocusChanged() {
            if (mark.activeFocus)
                tip.show()
            else if (!area.containsMouse)
                tip.hide()
        }
    }
    onActionTextChanged: if (tip.shown) tip.place()

    Rectangle {
        id: tip
        property bool shown: false
        function place() {
            const host = parent
            if (!host)
                return
            const p = root.expanded ? mark.mapToItem(host, 0, mark.height + 8)
                                    : mark.mapToItem(host, mark.width + 12, (mark.height - height) / 2)
            x = Math.max(8, Math.min(host.width - width - 8, Math.round(p.x)))
            y = Math.round(p.y)
        }
        function show() { if (root.visible) { place(); shown = true } }
        function hide() { tipDelay.stop(); shown = false }

        parent: root.Window.contentItem
        z: 1000
        width: tipLabel.implicitWidth + 16
        height: 24
        radius: Theme.shape.extraSmall
        color: Theme.inverseSurface
        opacity: shown && root.visible ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root._instant ? 0 : Theme.motion.effectsFast } }

        Label {
            id: tipLabel
            anchors.centerIn: parent
            text: root.actionText
            type: "bodySmall"
            color: Theme.fgInverseSurface
        }
    }
}
