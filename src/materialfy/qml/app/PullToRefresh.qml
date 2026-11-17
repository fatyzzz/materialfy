import QtQuick
import Md3
import Materialfy.Core

// Телефон: «потянуть вниз — обновить». Кружок с круговой стрелкой выезжает из-под верхнего края за пальцем
// (стрелка доворачивается по ходу), на пороге — отзыв вибрацией; отпустили за порогом — refresh() и индикатор
// загрузки, пока busy (но не меньше 600 мс, иначе он только мелькнёт). Кладётся поверх прокручиваемой области,
// в того же родителя: PullToRefresh { flickable: list; busy: Model.loading; onRefresh: Model.refresh() }.
Item {
    id: root

    required property Flickable flickable
    property bool busy: false
    property real threshold: 80
    signal refresh()

    anchors.horizontalCenter: parent.horizontalCenter
    width: 40
    height: 40
    z: 5

    readonly property bool _active: enabled && Theme.mobile
    // OvershootBounds пускает за край только бросок, палец — нет: здесь нужен DragAndOvershootBounds.
    // У верхнего края палец тянет содержимое вниз, verticalOvershoot < 0
    Binding {
        target: root.flickable
        property: "boundsBehavior"
        value: Flickable.DragAndOvershootBounds
        when: root._active
    }
    readonly property real pull: _active ? Math.max(0, -flickable.verticalOvershoot) : 0
    readonly property real progress: Math.min(1, pull / threshold)
    property bool spinning: false
    property bool _hapticDone: false

    visible: _active && (pull > 1 || spinning)
    // по пальцу — от «за краем» до ~60 px; пока грузится — стоит на 16
    y: spinning ? 16 : -height + Math.min(pull, threshold * 1.4) * 0.75
    opacity: spinning ? 1 : Math.min(1, progress * 1.5)
    scale: spinning ? 1 : 0.6 + 0.4 * progress
    Behavior on y {
        enabled: root.spinning
        NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast }
    }

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: width / 2
        color: Theme.surfaceContainerHigh
    }
    Elevation { target: bg; level: 2 }
    Icon {
        anchors.centerIn: parent
        visible: !root.spinning
        name: "refresh"
        size: 22
        color: root.progress >= 1 ? Theme.primary : Theme.fgSurfaceVariant
        rotation: root.progress * 300
    }
    LoadingIndicator {
        anchors.centerIn: parent
        visible: root.spinning
        running: root.spinning
        size: 30
    }

    onProgressChanged: {
        if (!flickable.dragging)
            return
        if (progress >= 1 && !_hapticDone) {
            _hapticDone = true
            MobileShell.haptic()
        } else if (progress < 1) {
            _hapticDone = false
        }
    }
    Connections {
        target: root.flickable
        function onDraggingChanged() {
            if (root.flickable.dragging || !root._active)
                return
            if (root.progress >= 1 && !root.spinning) {
                root.spinning = true
                minSpin.restart()
                root.refresh()
            }
            root._hapticDone = false
        }
    }
    Timer { id: minSpin; interval: 600 }
    // загрузка кончилась и 600 мс прошло — индикатор уходит
    readonly property bool _done: spinning && !busy && !minSpin.running
    on_DoneChanged: if (_done) spinning = false
}
