import QtQuick
import QtQuick.Shapes

// Меню нижним листом MD3 (раскладка телефона): тот же API, что у Menu — open(items), close(), picked(index),
// triggered(id, item); пункты — { id, text, icon, enabled, checked, hint, separator, submenu }.
// Подменю открывается на месте списка (сверху — строка «‹ назад» с именем родителя). Закрыть — тап по
// затемнению, свайп листа вниз или «назад» системы (MobileMain зовёт back()).
FocusScope {
    id: root

    property var items: []
    // шапка листа — о чём меню: { title, subtitle, image } (трек: обложка, название, исполнители); null — без неё
    property var header: null
    // вершина стека — показанный список: [{ items, title, parentIndex }]
    property var _levels: []
    readonly property var _current: _levels.length ? _levels[_levels.length - 1] : { items: [], title: "", parentIndex: -1 }
    property bool opened: false
    property Item _previousFocus: null

    signal picked(int index)
    signal triggered(string id, var item)

    function open(list) {
        if (!opened)
            _previousFocus = Window.window ? Window.window.activeFocusItem : null
        items = list
        _levels = [{ items: list, title: "", parentIndex: -1 }]
        sheet.dragOffset = 0
        flick.contentY = 0
        opened = true
        forceActiveFocus()
    }
    function close() {
        if (!opened)
            return
        const restoreFocus = activeFocus
        opened = false
        focus = false
        const previous = _previousFocus
        _previousFocus = null
        if (restoreFocus && previous && previous.visible && previous.enabled && previous.Window.window === Window.window)
            previous.forceActiveFocus()
    }
    // «назад» системы: из подменю — к родителю, иначе закрыть. false — лист и так закрыт
    function back() {
        if (!opened)
            return false
        if (_levels.length > 1) {
            const parentScroll = _current.parentScroll
            _levels = _levels.slice(0, -1)
            flick.contentY = parentScroll
        } else {
            close()
        }
        return true
    }

    function _usable(item) {
        return item && !item.separator && item.enabled !== false
    }
    function _activate(index) {
        const item = _current.items[index]
        if (!_usable(item))
            return
        if (item.submenu && item.submenu.length > 0) {
            _levels = _levels.concat([{ items: item.submenu, title: item.text ?? "", parentIndex: index, parentScroll: flick.contentY }])
            flick.contentY = 0
            return
        }
        const top = _levels.length === 1
        close()
        if (top)
            picked(index)
        triggered(item.id ?? "", item)
    }

    property real progress: opened ? 1 : 0
    Behavior on progress {
        NumberAnimation {
            duration: root.opened ? Theme.motion.spatialFast : Theme.motion.effectsDefault
            easing.type: Easing.BezierSpline
            easing.bezierCurve: root.opened ? Theme.motion.emphasizedDecelerate : Theme.motion.emphasizedAccelerate
        }
    }
    visible: opened || progress > 0.001

    // Затемнение под листом: клики по приложению не проходят, тап — закрыть
    Rectangle {
        anchors.fill: parent
        color: Theme.scrim
        opacity: 0.32 * root.progress
        MouseArea {
            anchors.fill: parent
            enabled: root.opened
            onClicked: root.close()
            onWheel: wheel => wheel.accepted = true
        }
    }

    Rectangle {
        id: sheet
        property real dragOffset: 0
        readonly property real maxHeight: root.height - Theme.safeTop - 48
        width: Math.min(root.width, 640)
        height: Math.min(maxHeight, column.implicitHeight + 8 + Theme.safeBottom)
        anchors.horizontalCenter: parent.horizontalCenter
        y: root.height - height * root.progress + dragOffset
        topLeftRadius: Theme.shape.extraLarge
        topRightRadius: Theme.shape.extraLarge
        color: Theme.surfaceContainerLow
        Behavior on height { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }

        // клики внутри листа не уходят в затемнение
        MouseArea { anchors.fill: parent; onWheel: wheel => wheel.accepted = true }

        // Ручка и свайп вниз — закрыть
        Item {
            id: grip
            width: parent.width
            height: 28
            Rectangle {
                anchors.centerIn: parent
                width: 32
                height: 4
                radius: 2
                color: Theme.fgSurfaceVariant
                opacity: 0.4
            }
            DragHandler {
                target: null
                xAxis.enabled: false
                onTranslationChanged: if (active) sheet.dragOffset = Math.max(0, translation.y)   // в конце жеста translation сбрасывается в 0
                onGrabChanged: transition => {
                    const released = transition === PointerDevice.UngrabExclusive
                    if (!released && transition !== PointerDevice.CancelGrabExclusive)
                        return
                    if (released && sheet.dragOffset > sheet.height * 0.25)
                        root.close()
                    sheet.dragOffset = 0
                }
            }
        }

        Flickable {
            id: flick
            anchors.top: grip.bottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Theme.safeBottom
            contentHeight: column.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            interactive: contentHeight > height

            Column {
                id: column
                width: flick.width
                bottomPadding: 8

                // Шапка: какой трек — без неё по листу на весь экран не понять, к чему пункты. В подменю — нет
                Item {
                    visible: root.header !== null && root._levels.length === 1
                    width: parent.width
                    height: visible ? 64 : 0
                    MorphImage {
                        x: 24
                        width: 48
                        height: 48
                        anchors.verticalCenter: parent.verticalCenter
                        visible: (root.header?.image ?? "") !== ""
                        source: root.header?.image ?? ""
                        shape: "softSquare"
                        fallbackTitle: root.header?.title ?? ""
                    }
                    Column {
                        x: (root.header?.image ?? "") !== "" ? 88 : 24
                        width: parent.width - x - 24
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 2
                        Label {
                            width: parent.width
                            text: root.header?.title ?? ""
                            type: "titleMedium"
                            weight: 600
                        }
                        Label {
                            width: parent.width
                            visible: text !== ""
                            text: root.header?.subtitle ?? ""
                            type: "bodyMedium"
                            color: Theme.fgSurfaceVariant
                        }
                    }
                }
                Rectangle {
                    visible: root.header !== null && root._levels.length === 1
                    x: 24
                    width: parent.width - 48
                    height: visible ? 1 : 0
                    color: Theme.outlineVariant
                    opacity: 0.6
                }

                // Подменю: «‹ Добавить в плейлист»
                Item {
                    visible: root._levels.length > 1
                    width: parent.width
                    height: visible ? 52 : 0
                    Shape {
                        x: 24
                        width: 24
                        height: 24
                        anchors.verticalCenter: parent.verticalCenter
                        preferredRendererType: Shape.CurveRenderer
                        ShapePath {
                            strokeColor: Theme.fgSurface
                            strokeWidth: 2
                            fillColor: "transparent"
                            capStyle: ShapePath.RoundCap
                            joinStyle: ShapePath.RoundJoin
                            startX: 15; startY: 6
                            PathLine { x: 9; y: 12 }
                            PathLine { x: 15; y: 18 }
                        }
                    }
                    Label {
                        x: 64
                        width: parent.width - x - 24
                        anchors.verticalCenter: parent.verticalCenter
                        text: root._current.title
                        type: "titleMedium"
                        weight: 600
                    }
                    StateLayer {
                        anchors.fill: parent
                        onClicked: root.back()
                    }
                }
                Rectangle {
                    visible: root._levels.length > 1
                    x: 24
                    width: parent.width - 48
                    height: visible ? 1 : 0
                    color: Theme.outlineVariant
                    opacity: 0.6
                }

                Repeater {
                    model: root._current.items

                    Item {
                        id: row
                        required property var modelData
                        required property int index
                        readonly property bool isSeparator: modelData.separator === true
                        readonly property bool usable: root._usable(modelData)
                        readonly property bool hasSub: !isSeparator && !!modelData.submenu && modelData.submenu.length > 0
                        Accessible.ignored: isSeparator || !root.opened
                        Accessible.role: Accessible.MenuItem
                        Accessible.name: modelData.text ?? ""
                        Accessible.onPressAction: if (root.opened) root._activate(row.index)
                        activeFocusOnTab: root.opened && usable
                        Keys.onReturnPressed: root._activate(index)
                        Keys.onSpacePressed: root._activate(index)

                        width: column.width
                        height: isSeparator ? 17 : 52
                        // открытие: пункты проявляются каскадом снизу (от хода листа, без своих таймеров);
                        // при закрытии лист уезжает целиком
                        readonly property real _t: root.opened ? Math.max(0, Math.min(1, (root.progress - Math.min(index, 10) * 0.04) / 0.55)) : 1
                        opacity: _t
                        transform: Translate { y: (1 - row._t) * 16 }

                        Rectangle {
                            visible: row.isSeparator
                            x: 24
                            width: parent.width - 48
                            height: 1
                            anchors.verticalCenter: parent.verticalCenter
                            color: Theme.outlineVariant
                            opacity: 0.6
                        }

                        Item {
                            visible: !row.isSeparator
                            anchors.fill: parent
                            opacity: row.usable ? 1 : Theme.stateLayer.disabledContent

                            Icon {
                                visible: (row.modelData.icon ?? "") !== ""
                                x: 24
                                anchors.verticalCenter: parent.verticalCenter
                                name: row.modelData.icon ?? ""
                                size: 24
                                color: Theme.fgSurfaceVariant
                            }
                            Label {
                                x: 64
                                width: parent.width - x - 24 - (row.hasSub || row.modelData.checked === true ? 32 : 0)
                                       - (hint.visible ? hint.width + 12 : 0)
                                anchors.verticalCenter: parent.verticalCenter
                                Accessible.ignored: true
                                text: row.modelData.text ?? ""
                                type: "bodyLarge"
                                color: Theme.fgSurface
                            }
                            Label {
                                id: hint
                                visible: (row.modelData.hint ?? "") !== ""
                                anchors.right: parent.right
                                anchors.rightMargin: 24 + (row.hasSub || row.modelData.checked === true ? 32 : 0)
                                anchors.verticalCenter: parent.verticalCenter
                                width: Math.min(implicitWidth, parent.width * 0.25)
                                Accessible.ignored: true
                                text: row.modelData.hint ?? ""
                                type: "labelMedium"
                                color: Theme.fgSurfaceVariant
                            }
                            Icon {
                                visible: row.modelData.checked === true
                                anchors.right: parent.right
                                anchors.rightMargin: 24
                                anchors.verticalCenter: parent.verticalCenter
                                name: "check"
                                size: 24
                                color: Theme.primary
                            }
                            Shape {
                                visible: row.hasSub
                                anchors.right: parent.right
                                anchors.rightMargin: 24
                                width: 24
                                height: 24
                                anchors.verticalCenter: parent.verticalCenter
                                preferredRendererType: Shape.CurveRenderer
                                ShapePath {
                                    strokeColor: Theme.fgSurfaceVariant
                                    strokeWidth: 2
                                    fillColor: "transparent"
                                    capStyle: ShapePath.RoundCap
                                    joinStyle: ShapePath.RoundJoin
                                    startX: 9; startY: 6
                                    PathLine { x: 15; y: 12 }
                                    PathLine { x: 9; y: 18 }
                                }
                            }
                        }

                        StateLayer {
                            visible: !row.isSeparator
                            enabled: row.usable && root.opened
                            anchors.fill: parent
                            onClicked: root._activate(row.index)
                        }
                        Rectangle {
                            anchors.fill: parent
                            anchors.margins: 2
                            visible: root.opened && row.activeFocus
                            color: "transparent"
                            border.color: Theme.primary
                            border.width: 2
                            radius: Theme.shape.medium
                        }
                    }
                }
            }
        }
    }
}
