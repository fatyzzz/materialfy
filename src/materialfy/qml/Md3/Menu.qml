import QtQuick
import QtQuick.Shapes
import QtQuick.Templates as T

// Всплывающее меню MD3: карточка со списком пунктов у точки окна. Клик мимо или Esc — закрыть.
// Меню — отдельное всплывающее окно (Popup.Window): выходит за края окна приложения (в мини-плеере
// 280×120 иначе не поместится) и прижимается к краям экрана. Подменю — в том же окне, рядом с карточкой:
// Qt отдаёт мышь последнему открытому popup-окну, и в отдельном окне подменю главная карточка глохла.
// Кладётся в окно: Menu { anchors.fill: parent }; open(items, point[, size]) — точка в координатах окна.
// Пункт: { id, text, icon, enabled (true), checked (undefined — без галочки), hint (серый текст справа),
// separator: true, submenu: [пункты] }.
// picked(index) — выбран пункт верхнего уровня (индекс в списке); triggered(id, item) — любой выбранный пункт.
Item {
    id: root

    property var items: []
    readonly property bool opened: mainPopup.visible
    // Отдельным окном (Popup.Window). На Wayland по умолчанию — нет (см. CardPopup); островок KDE просит да: его окно
    // меньше меню, а где оно на экране, известно (окно ставит скрипт KWin)
    property bool windowed: !Qt.platform.pluginName.startsWith("wayland")

    signal picked(int index)
    signal triggered(string id, var item)

    // поле под тень карточки внутри прозрачного окна
    readonly property int _pad: 12

    // size — размер кнопки, у которой открыто меню (point — её левый верхний угол): карточка встаёт
    // под кнопкой, правым краем к её правому краю, а не влезает вниз — над ней. Без size меню открывается
    // от точки (курсор при ПКМ) вправо-вниз и отражается влево/вверх у края.
    function open(list, point, size) {
        closeSub()
        items = list
        main.currentIndex = -1
        const g = root.mapToGlobal(point.x, point.y)
        anchorBox.x = point.x
        anchorBox.y = point.y
        anchorBox.width = size ? Math.max(1, size.width) : 1
        anchorBox.height = size ? Math.max(1, size.height) : 1
        mainPopup.anchor = Qt.rect(g.x, g.y, size ? size.width : 0, size ? size.height : 0)
        mainPopup.area = _screenArea(g)
        mainPopup.windowArea = _windowArea()
        mainPopup.open()
        main.forceActiveFocus()
    }
    function close() {
        closeSub()
        mainPopup.close()
    }
    function closeSub() {
        hoverTimer.stop()
        sub.parentIndex = -1
        _subOpen = false
    }
    property bool _subOpen: false

    // Экран под якорем (окно может стоять на двух мониторах сразу) в глобальных координатах; панель задач
    // снизу/справа срезается по доступной области рабочего стола (availableGeometry экрана в QML не видна)
    function _screenArea(g) {
        // Wayland не сообщает, где на экране окно: mapToGlobal даёт координаты внутри окна, а геометрия
        // монитора — глобальная (второй монитор начинается не с нуля), и меню уезжало. Там граница — само окно.
        if (!root.windowed)
            return _windowArea()
        const w = root.Window.window
        let s = w ? w.screen : null
        for (const sc of Qt.application.screens) {
            if (g.x >= sc.virtualX && g.x < sc.virtualX + sc.width && g.y >= sc.virtualY && g.y < sc.virtualY + sc.height) {
                s = sc
                break
            }
        }
        if (!s || !(s.width > 0)) {
            const o = root.mapToGlobal(0, 0)
            return Qt.rect(o.x, o.y, root.width, root.height)
        }
        let right = s.virtualX + s.width
        let bottom = s.virtualY + s.height
        if (s.desktopAvailableWidth > s.virtualX)
            right = Math.min(right, s.desktopAvailableWidth)
        if (s.desktopAvailableHeight > s.virtualY)
            bottom = Math.min(bottom, s.desktopAvailableHeight)
        return Qt.rect(s.virtualX, s.virtualY, right - s.virtualX, bottom - s.virtualY)
    }

    // Окно приложения в глобальных координатах: меню держится внутри него, если помещается
    function _windowArea() {
        const w = root.Window.window
        if (!w || !w.contentItem)
            return Qt.rect(0, 0, 0, 0)
        const o = w.contentItem.mapToGlobal(0, 0)
        return Qt.rect(o.x, o.y, w.width, w.height)
    }

    function _usable(item) {
        return item && !item.separator && item.enabled !== false
    }

    function _openSub(index, focusFirst) {
        hoverTimer.stop()
        const item = items[index]
        if (!_usable(item) || !item.submenu || item.submenu.length === 0)
            return
        if (sub.parentIndex !== index || !_subOpen) {
            sub.model = item.submenu
            sub.parentIndex = index
            sub.currentIndex = -1
            const row = main.rowAt(index)
            mainPopup.subRowY = row ? row.mapToItem(main, 0, 0).y : 0
            _subOpen = true
            subEnter.restart()
        }
        if (focusFirst) {
            sub.currentIndex = sub.step(-1, 1)
            sub.forceActiveFocus()
        }
    }

    function _activate(card, index) {
        const item = card.model[index]
        if (!_usable(item))
            return
        if (card.level === 0 && item.submenu && item.submenu.length > 0) {
            _openSub(index, true)
            return
        }
        close()
        if (card.level === 0)
            picked(index)
        triggered(item.id ?? "", item)
    }

    // Окно приложения ушло в фон — меню закрывается (как у системных меню)
    Connections {
        target: Qt.application
        function onStateChanged() {
            if (Qt.application.state !== Qt.ApplicationActive)
                root.close()
        }
    }

    // Задержка открытия/закрытия подменю по наведению: диагональный путь мыши к подменю его не схлопывает
    Timer {
        id: hoverTimer
        property int index: -1
        interval: 160
        onTriggered: {
            const item = root.items[index]
            if (item && item.submenu && item.submenu.length > 0)
                root._openSub(index, false)
            else
                root.closeSub()
        }
    }

    // Wayland, отдельным окном: Qt отдаёт композитору прямоугольник родителя попапа как «контрол», от которого меню
    // встаёт и отражается (parentControlGeometry). Родитель — всё окно, и KWin ставил меню у его правого края;
    // поэтому родитель — точка или кнопка, у которой открыли меню
    Item { id: anchorBox; width: 1; height: 1 }

    // Всплывающее окно под одну карточку: прозрачное, с полем под тень; позиция — в глобальных координатах
    component CardPopup: T.Popup {
        id: pop
        property rect area: Qt.rect(0, 0, 0, 0)
        property Item card: null

        // На Wayland — внутри окна: композитор сам двигает отдельные всплывающие окна, и меню оказывалось
        // не у кнопки. Границы там всё равно по окну (см. _screenArea).
        popupType: root.windowed ? T.Popup.Window : T.Popup.Item
        modal: false
        dim: false
        focus: true
        padding: root._pad
        background: null
        parent: root.windowed && Qt.platform.pluginName.startsWith("wayland") ? anchorBox : root
        // x/y попапа — относительно его родителя; считаем в глобальных и переводим
        function toLocal(gx, gy) { return parent.mapFromGlobal(gx, gy) }

        // размер окна — по карточке (неявный размер контейнера до Popup.Window не доходит)
        contentWidth: card ? card.implicitWidth : 0
        contentHeight: card ? card.implicitHeight : 0
        // в Templates (без стиля) размер попапа к содержимому не привязан
        implicitWidth: contentWidth + leftPadding + rightPadding
        implicitHeight: contentHeight + topPadding + bottomPadding

        enter: Transition {
            ParallelAnimation {
                NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motion.effectsFast }
                NumberAnimation { property: "scale"; from: 0.9; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.OutBack }
            }
        }
        exit: Transition {
            NumberAnimation { property: "opacity"; from: 1; to: 0; duration: Theme.motion.effectsFast }
        }
    }

    component MenuCard: Rectangle {
        id: card

        property var model: []
        property int level: 0
        property int currentIndex: -1
        property int parentIndex: -1   // для подменю: пункт-родитель
        readonly property bool hasIcons: (model || []).some(it => (it.icon ?? "") !== "")
        readonly property bool hasTrailing: (model || []).some(it => typeof it.checked === "boolean" || (it.submenu && it.submenu.length > 0))

        function rowAt(i) { return rows.itemAt(i) }
        // Следующий доступный пункт от from в направлении dir (пропуская разделители и выключенные)
        function step(from, dir) {
            const n = (model || []).length
            let i = from < 0 ? (dir > 0 ? -1 : n) : from
            for (let k = 0; k < n; ++k) {
                i = ((i + dir) % n + n) % n
                if (root._usable(model[i]))
                    return i
            }
            return -1
        }

        implicitWidth: list.implicitWidth + 16
        implicitHeight: list.implicitHeight + 16
        width: implicitWidth
        height: implicitHeight
        radius: Theme.shape.large
        color: Qt.alpha(Theme.surfaceContainer, 0.96)
        border.width: 1
        border.color: Qt.alpha(Theme.outlineVariant, 0.5)

        Keys.onPressed: event => {
            switch (event.key) {
            case Qt.Key_Down:
                currentIndex = step(currentIndex, 1)
                break
            case Qt.Key_Up:
                currentIndex = step(currentIndex, -1)
                break
            case Qt.Key_Right:
                if (level === 0 && currentIndex >= 0)
                    root._openSub(currentIndex, true)
                break
            case Qt.Key_Left:
                if (level === 1) {
                    root.closeSub()
                    main.forceActiveFocus()
                }
                break
            case Qt.Key_Return:
            case Qt.Key_Enter:
            case Qt.Key_Space:
                if (currentIndex >= 0)
                    root._activate(card, currentIndex)
                break
            case Qt.Key_Escape:
                if (level === 1) {
                    root.closeSub()
                    main.forceActiveFocus()
                } else {
                    root.close()
                }
                break
            default:
                return
            }
            event.accepted = true
        }

        Elevation { target: card; level: 2 }

        Column {
            id: list
            x: 8
            y: 8

            Repeater {
                id: rows
                model: card.model

                Item {
                    id: row
                    required property var modelData
                    required property int index
                    readonly property bool isSeparator: modelData.separator === true
                    readonly property bool usable: root._usable(modelData)
                    readonly property bool hasSub: !isSeparator && card.level === 0 && !!modelData.submenu && modelData.submenu.length > 0
                    readonly property bool current: card.currentIndex === index
                    readonly property real contentLeft: 12 + (card.hasIcons ? 20 + 12 : 0)

                    width: isSeparator ? 1 : Math.max(204, contentLeft + label.implicitWidth + (card.hasTrailing ? 16 + 20 : 0)
                                                     + (hintLabel.visible ? 16 + hintLabel.implicitWidth : 0) + 12)
                    height: isSeparator ? 9 : 40

                    Rectangle {
                        visible: row.isSeparator
                        x: 4
                        width: list.width - 8
                        height: 1
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.outlineVariant
                        opacity: 0.6
                    }

                    // Подсветка: текущий пункт с клавиатуры и открытый родитель подменю
                    Rectangle {
                        visible: !row.isSeparator
                        width: list.width
                        height: parent.height
                        radius: Theme.shape.medium
                        color: Theme.fgSurface
                        opacity: (row.current && !hit.containsMouse) || (row.hasSub && root._subOpen && sub.parentIndex === row.index)
                                 ? Theme.stateLayer.focus : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                    }

                    Item {
                        visible: !row.isSeparator
                        width: list.width
                        height: parent.height
                        opacity: row.usable ? 1 : Theme.stateLayer.disabledContent

                        Icon {
                            visible: (row.modelData.icon ?? "") !== ""
                            x: 12
                            anchors.verticalCenter: parent.verticalCenter
                            name: row.modelData.icon ?? ""
                            size: 20
                            color: Theme.fgSurfaceVariant
                        }
                        Label {
                            id: label
                            x: row.contentLeft
                            anchors.verticalCenter: parent.verticalCenter
                            text: row.modelData.text ?? ""
                            type: "bodyMedium"
                            color: Theme.fgSurface
                        }
                        // пояснение справа серым («локально»)
                        Label {
                            id: hintLabel
                            visible: (row.modelData.hint ?? "") !== ""
                            anchors.right: parent.right
                            anchors.rightMargin: 12 + (card.hasTrailing ? 20 + 8 : 0)
                            anchors.verticalCenter: parent.verticalCenter
                            text: row.modelData.hint ?? ""
                            type: "labelSmall"
                            color: Theme.fgSurfaceVariant
                        }
                        Icon {
                            visible: row.modelData.checked === true
                            x: parent.width - 12 - 20
                            anchors.verticalCenter: parent.verticalCenter
                            name: "check"
                            size: 20
                            color: Theme.primary
                        }
                        // Шеврон «›»: в урезанном шрифте иконок chevron_right нет
                        Shape {
                            visible: row.hasSub
                            x: parent.width - 12 - 20
                            width: 20
                            height: 20
                            anchors.verticalCenter: parent.verticalCenter
                            preferredRendererType: Shape.CurveRenderer
                            ShapePath {
                                strokeColor: Theme.fgSurfaceVariant
                                strokeWidth: 1.8
                                fillColor: "transparent"
                                capStyle: ShapePath.RoundCap
                                joinStyle: ShapePath.RoundJoin
                                startX: 8; startY: 5
                                PathLine { x: 13; y: 10 }
                                PathLine { x: 8; y: 15 }
                            }
                        }
                    }

                    StateLayer {
                        id: hit
                        visible: !row.isSeparator
                        enabled: row.usable
                        width: list.width
                        height: parent.height
                        radius: Theme.shape.medium
                        onContainsMouseChanged: {
                            if (!containsMouse)
                                return
                            card.currentIndex = row.index
                            if (card.level === 0) {
                                hoverTimer.index = row.index
                                hoverTimer.restart()
                                card.forceActiveFocus()
                            } else {
                                hoverTimer.stop()
                            }
                        }
                        onClicked: root._activate(card, row.index)
                    }
                }
            }
        }
    }

    // Позиции — биндингами: размер карточки известен только после раскладки пунктов.
    // Границы — окно приложения, если карточка в нём помещается (в мини-плеере — нет: тогда экран).
    // По горизонтали: от кнопки — правым краем к её правому краю, от точки — вправо; не влезает — в другую
    // сторону. По вертикали: под якорем, не влезает — над ним, не влезает и так — где места больше.
    CardPopup {
        id: mainPopup
        property rect anchor: Qt.rect(0, 0, 0, 0)
        property rect windowArea: Qt.rect(0, 0, 0, 0)
        readonly property real gap: anchor.height > 0 ? 4 : 0
        readonly property rect bounds: {
            const a = area, w = windowArea
            if (w.width < main.width + 16 || w.height < main.height + 16)
                return a
            const l = Math.max(a.x, w.x), t = Math.max(a.y, w.y)
            const r = Math.min(a.x + a.width, w.x + w.width), b = Math.min(a.y + a.height, w.y + w.height)
            return r - l >= main.width + 16 && b - t >= main.height + 16 ? Qt.rect(l, t, r - l, b - t) : a
        }
        readonly property bool toLeft: anchor.width > 0
                                       ? anchor.x + anchor.width - main.width >= bounds.x + 8
                                       : anchor.x + main.width > bounds.x + bounds.width - 8
        readonly property bool below: anchor.y + anchor.height + gap + main.height <= bounds.y + bounds.height - 8
                                      || anchor.y - gap - main.height < bounds.y + 8
                                         && bounds.y + bounds.height - anchor.y - anchor.height >= anchor.y - bounds.y
        // карточка в глобальных координатах
        readonly property real mx: Math.max(bounds.x + 8, Math.min(bounds.x + bounds.width - main.width - 8,
                                       toLeft ? anchor.x + anchor.width - main.width : anchor.x))
        readonly property real my: Math.max(bounds.y + 8, Math.min(bounds.y + bounds.height - main.height - 8,
                                       below ? anchor.y + anchor.height + gap : anchor.y - gap - main.height))
        // подменю: справа от карточки, не влезает — слева; по вертикали — у строки-родителя (subRowY — от верха карточки)
        property real subRowY: 0
        readonly property real sx: mx + main.width - 4 + sub.width > area.x + area.width - 8
                                   ? Math.max(area.x + 8, mx - sub.width + 4)
                                   : mx + main.width - 4
        readonly property real sy: Math.max(area.y + 8, Math.min(my + subRowY - 8, area.y + area.height - sub.height - 8))
        // окно охватывает обе карточки
        readonly property real left: root._subOpen ? Math.min(mx, sx) : mx
        readonly property real top: root._subOpen ? Math.min(my, sy) : my
        readonly property real right: root._subOpen ? Math.max(mx + main.width, sx + sub.width) : mx + main.width
        readonly property real bottom: root._subOpen ? Math.max(my + main.height, sy + sub.height) : my + main.height
        x: toLocal(left - root._pad, top - root._pad).x
        y: toLocal(left - root._pad, top - root._pad).y
        closePolicy: T.Popup.CloseOnEscape | T.Popup.CloseOnPressOutside
        // раскрывается из угла у якоря
        transformOrigin: below ? (toLeft ? Item.TopRight : Item.TopLeft) : (toLeft ? Item.BottomRight : Item.BottomLeft)
        onClosed: root.closeSub()
        card: pane

        Item {
            id: pane
            implicitWidth: mainPopup.right - mainPopup.left
            implicitHeight: mainPopup.bottom - mainPopup.top

            MenuCard {
                id: main
                x: mainPopup.mx - mainPopup.left
                y: mainPopup.my - mainPopup.top
                level: 0
                model: root.items
            }

            MenuCard {
                id: sub
                x: mainPopup.sx - mainPopup.left
                y: mainPopup.sy - mainPopup.top
                visible: root._subOpen
                level: 1
                transformOrigin: x < main.x ? Item.TopRight : Item.TopLeft
                HoverHandler { onHoveredChanged: if (hovered) hoverTimer.stop() }

                ParallelAnimation {
                    id: subEnter
                    NumberAnimation { target: sub; property: "opacity"; from: 0; to: 1; duration: Theme.motion.effectsFast }
                    NumberAnimation { target: sub; property: "scale"; from: 0.9; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.OutBack }
                }
            }
        }
    }
}
