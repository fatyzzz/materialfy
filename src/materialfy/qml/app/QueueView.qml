import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Очередь в порядке воспроизведения: «Играет из …», «Сейчас играет», «Далее в очереди» (свои треки,
// перестановка за ручку на этом компьютере), «Далее из «…»» (контекст, не цель для перетаскивания).
// Занимает родителя; используется в RightPanel и во вкладке очереди NowPlaying.
Item {
    id: root

    property bool compact: false

    readonly property var queue: Player.queue
    readonly property real rowHeight: Theme.trackRowHeight(compact)
    readonly property int contextPreview: 20     // столько строк контекста видно до «Ещё N треков»

    // ---- Границы секций (порядок в модели: current → manual → context) ----
    property int manualStart: -1
    property int contextStart: -1
    property int sectionRevision: 0
    readonly property int count: queue ? queue.count : 0
    readonly property int manualRows: manualStart < 0 ? 0 : (contextStart >= 0 ? contextStart : count) - manualStart
    readonly property int contextRows: contextStart >= 0 ? count - contextStart : 0
    property bool contextExpanded: false
    readonly property int contextShown: contextExpanded ? contextRows : Math.min(contextRows, contextPreview)
    readonly property int contextMore: Math.max(0, Player.contextRemaining - contextShown)
    readonly property bool dropZoneShown: Player.isLocal && Player.hasTrack && Player.manualCount === 0

    function recount() {
        if (!queue) {
            manualStart = -1
            contextStart = -1
            return
        }
        manualStart = queue.find("section", "manual")
        contextStart = queue.find("section", "context")
        sectionRevision++
    }
    // Один пересчёт после диффа модели. Заголовки принадлежат строкам, а не кешу секций ListView:
    // смена section.property во время обновления давала неверные подписи и нативное падение Qt.
    function scheduleRecount() { Qt.callLater(root.recount) }
    Component.onCompleted: scheduleRecount()
    Connections {
        target: Player
        function onQueueChanged() {
            root.scheduleRecount()
            root._resetDrag()
        }
    }
    Connections {
        target: root.queue
        function onCountChanged() { root.scheduleRecount() }
        function onModelReset() { root.scheduleRecount() }
        // после каждого диффа, в том числе замены строк на месте, о которой queueChanged не сообщает
        function onRowsChanged() { root.scheduleRecount() }
    }
    // новый контекст — снова свёрнут
    readonly property string _contextUri: Player.contextUri
    on_ContextUriChanged: contextExpanded = false

    // ---- Перестановка в «Далее в очереди» ----
    property int dragFrom: -1
    property int dragTo: -1
    property real dragOffset: 0
    property bool shiftAnimated: true

    function beginDrag(m) {
        shiftAnimated = true
        dragFrom = m
        dragTo = m
        dragOffset = 0
    }
    function updateDrag(dy) {
        if (dragFrom < 0)
            return
        dragOffset = Math.max(-dragFrom * rowHeight, Math.min((manualRows - 1 - dragFrom) * rowHeight, dy))
        dragTo = Math.max(0, Math.min(manualRows - 1, dragFrom + Math.round(dragOffset / rowHeight)))
    }
    function endDrag() {
        if (dragFrom < 0)
            return
        if (dragTo !== dragFrom && Player.queueReorderable) {
            // строка стоит на новом месте, пока не придёт обновлённая модель
            dragOffset = (dragTo - dragFrom) * rowHeight
            Player.moveManual(dragFrom, dragTo)
            settleTimer.restart()
        } else {
            _resetDrag()
        }
    }
    function _resetDrag() {
        if (dragFrom < 0)
            return
        settleTimer.stop()
        shiftAnimated = false
        dragFrom = -1
        dragTo = -1
        dragOffset = 0
        Qt.callLater(() => root.shiftAnimated = true)
    }
    Timer { id: settleTimer; interval: 600; onTriggered: root._resetDrag() }
    function shiftFor(m) {
        if (dragFrom < 0 || m < 0 || m === dragFrom)
            return 0
        if (dragFrom < dragTo && m > dragFrom && m <= dragTo)
            return -rowHeight
        if (dragTo < dragFrom && m >= dragTo && m < dragFrom)
            return rowHeight
        return 0
    }

    // полный словарь строки; uid и title — только зависимости привязки (строку заменили на месте)
    function rowData(i, uid, title) { return queue.get(i) }

    // ---- Очистка с возвратом ----
    function clearQueue() {
        const saved = []
        for (let i = Math.max(0, manualStart); manualStart >= 0 && i < count; i++) {
            const t = queue.get(i)
            if (t.section !== "manual")
                break
            saved.push(t)
        }
        Player.clearManualQueue()
        if (saved.length > 0)
            undoBar.show(I18n.t("Queue cleared"), I18n.t("Undo"), () => {
                for (const t of saved)
                    Player.addToQueue(t)
            })
    }

    // ---- Бросок трека из списков ----
    function _droppedTrack(drop) {
        if (drop.source && typeof drop.source.trackData === "function")
            return drop.source.trackData()
        if (drop.hasText || drop.formats.indexOf("application/x-materialfy-track") >= 0) {
            try {
                return JSON.parse(drop.getDataAsString("application/x-materialfy-track"))
            } catch (e) {
                return null
            }
        }
        return null
    }
    // позиция в «Далее в очереди» для точки списка; -1 — не цель
    function _dropIndexAt(x, y) {
        if (!Player.isLocal)
            return manualRows
        const p = root.mapToItem(list.contentItem, x, y)
        const i = list.indexAt(p.x, p.y)
        if (i < 0)
            return dropZoneHover.containsDrag || Player.manualCount === 0 ? 0 : -1
        const t = queue.get(i)
        if (t.section === "current")
            return 0
        if (t.section === "manual") {
            const item = list.itemAtIndex(i)
            const below = item ? p.y - item.y - item.headerHeight > root.rowHeight / 2 : false
            return i - manualStart + (below ? 1 : 0)
        }
        return -1
    }

    // ---- Пусто ----
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 32, 300)
        visible: root.count === 0
        icon: "queue_music"
        shape: "cookie6"
        animated: !Theme.reducedMotion
        title: I18n.t("Queue is empty")
        text: Player.remote ? I18n.t("The queue on “{0}” is empty", Player.device.name ?? "")
                            : I18n.t("Play a playlist or album. To play a track next, choose “Add to queue” in its menu or drag it here")
    }

    // «Играет на …» и «Играет из …» — над списком, не прокручиваются
    Column {
        id: top
        width: parent.width
        spacing: 0

        Item {
            width: parent.width
            height: Player.remote ? banner.implicitHeight + 12 : 0
            Behavior on height { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
            opacity: Player.remote ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
            clip: true
            ConnectBanner {
                id: banner
                width: parent.width
                variant: Player.deviceLost ? "lost" : "normal"
            }
        }
        Item {
            width: parent.width
            height: ctxCard.visible ? ctxCard.height + 4 : 0
            QueueContextCard {
                id: ctxCard
                width: parent.width
                visible: root.count > 0 && (Player.queueContext.uri ?? "") !== ""
                context: Player.queueContext
                onClicked: Catalog.openUri(Player.queueContext.uri)
            }
        }
    }

    ListView {
        id: list
        anchors.fill: parent
        anchors.topMargin: top.height
        visible: root.count > 0
        clip: true
        model: root.queue
        boundsBehavior: Theme.mobile ? Flickable.OvershootBounds : Flickable.StopAtBounds
        acceptedButtons: Theme.mobile ? Qt.LeftButton : Qt.NoButton
        cacheBuffer: 400
        // без reuseItems: замер на живой очереди показал регрессию — при частых диффах плеера
        // переклейка данных на переиспользуемые делегаты дороже обновления биндингов на месте

        delegate: Item {
            id: cell
            required property int index
            required property var model
            readonly property string sec: model.section
            readonly property int m: sec === "manual" ? index - root.manualStart : -1
            readonly property bool hidden: sec === "context" && index - root.contextStart >= root.contextShown
            readonly property bool lifted: m >= 0 && m === root.dragFrom
            readonly property bool startsSection: root.sectionRevision >= 0
                && index >= 0 && index < root.count
                && (index === 0 || (root.queue && (root.queue.get(index - 1) ?? {}).section !== sec))
            readonly property real headerHeight: sectionHeader.height

            width: ListView.view.width
            height: hidden ? 0 : headerHeight + root.rowHeight
            visible: !hidden
            z: lifted ? 10 : 0

            Loader {
                id: sectionHeader
                width: cell.width
                height: item ? item.implicitHeight : 0
                active: cell.startsSection && !cell.hidden
                sourceComponent: Column {
                    width: cell.width
                    spacing: 4
                    Item {
                        width: parent.width
                        height: cell.sec === "context" && root.dropZoneShown ? zone.height + 8 : 0
                        visible: height > 0
                        QueueDropZone {
                            id: zone
                            y: 8
                            width: parent.width
                            active: dropZoneHover.containsDrag
                        }
                    }
                    QueueSectionHeader {
                        width: parent.width
                        title: cell.sec === "current" ? I18n.t("Now playing")
                             : cell.sec === "manual" ? I18n.t("Next in queue")
                             : Player.contextTitle !== "" ? I18n.t("Next from “{0}”", Player.contextTitle) : I18n.t("Next up")
                        count: cell.sec === "manual" ? Player.manualCount : -1
                        actionIcon: cell.sec === "manual" && Player.canClearQueue ? "clear_all" : ""
                        actionText: I18n.t("Clear")
                        note: cell.sec === "manual" && Player.remote ? I18n.t("New tracks go to the end")
                            : cell.sec === "context" && root.dragFrom >= 0 ? I18n.t("Can't drop here") : ""
                        onAction: root.clearQueue()
                    }
                }
            }

            // место, куда встанет поднятая строка
            Rectangle {
                visible: cell.lifted && root.dragTo !== root.dragFrom
                y: cell.headerHeight + (root.dragTo - root.dragFrom) * root.rowHeight
                width: parent.width
                height: root.rowHeight
                radius: Theme.shape.large
                color: "transparent"
                border.width: 2
                border.color: Theme.primary
            }

            QueueRow {
                id: row
                width: parent.width
                height: root.rowHeight
                compact: root.compact
                y: cell.headerHeight + (cell.lifted ? root.dragOffset : root.shiftFor(cell.m))
                Behavior on y {
                    enabled: !row.dragging && root.shiftAnimated
                    NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
                }
                // свойства корня делегата трогают переходы ListView (add/displaced), поэтому приглушаем строку
                opacity: root.dragFrom >= 0 && cell.sec === "context" ? 0.45 : 1
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                scale: row.dragging ? 1.03 : 1
                Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                track: root.rowData(cell.index, cell.model.uid, cell.model.title)
                current: cell.sec === "current"
                draggable: cell.m >= 0 && Player.queueReorderable
                reserveHandle: Player.queueReorderable === true
                dragging: cell.lifted && root.dragFrom >= 0
                activeFocusOnTab: true
                onActivated: Player.playQueueRow(cell.model.uid)
                onDragStarted: root.beginDrag(cell.m)
                onDragMoved: dy => root.updateDrag(dy)
                onDragFinished: root.endDrag()
                Keys.onPressed: e => {
                    if (!(e.modifiers & Qt.ControlModifier) || cell.m < 0 || !Player.queueReorderable)
                        return
                    if (e.key === Qt.Key_Up && cell.m > 0) {
                        Player.moveManual(cell.m, cell.m - 1)
                        e.accepted = true
                    } else if (e.key === Qt.Key_Down && cell.m < root.manualRows - 1) {
                        Player.moveManual(cell.m, cell.m + 1)
                        e.accepted = true
                    }
                }
                Keys.onReturnPressed: Player.playQueueRow(cell.model.uid)
            }
        }

        footer: Column {
            width: list.width
            spacing: 8
            topPadding: 4

            Button {
                visible: root.contextShown < root.contextRows || (root.contextMore > 0 && Player.contextUri !== "")
                anchors.horizontalCenter: parent.horizontalCenter
                style: "text"
                icon: root.contextShown < root.contextRows ? "expand_more" : "chevron_right"
                // свёрнутые строки раскрываются на месте, остальное — на странице контекста
                text: I18n.n(root.contextMore, "{0} more track", "{0} more tracks")
                onClicked: {
                    if (root.contextShown < root.contextRows)
                        root.contextExpanded = true
                    else
                        Catalog.openUri(Player.contextUri)
                }
            }
            QueueDropZone {
                visible: root.contextRows === 0 && root.dropZoneShown
                width: parent.width
                active: dropZoneHover.containsDrag
            }
            Item { width: 1; height: root.compact ? 16 : 24 }
        }

        add: Transition {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motion.effectsDefault }
            NumberAnimation { property: "y"; from: Theme.reducedMotion ? 0 : 8; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
        remove: Transition {
            NumberAnimation { property: "opacity"; to: 0; duration: Theme.motion.effectsFast }
        }
        displaced: Transition {
            NumberAnimation { property: "y"; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
            NumberAnimation { property: "opacity"; to: 1; duration: Theme.motion.effectsFast }
        }
        move: Transition {
            NumberAnimation { property: "y"; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
    }
    SmoothScroll { flickable: list; wheelStep: Settings.wheelStep; visible: list.visible }

    // трек, брошенный из списков: на этом компьютере — на место, на другом устройстве — в конец
    DropArea {
        id: dropZoneHover
        anchors.fill: parent
        keys: ["materialfy/track", "application/x-materialfy-track"]
        onDropped: drop => {
            const t = root._droppedTrack(drop)
            if (!t) {
                drop.accepted = false
                return
            }
            if (!Player.isLocal) {
                Player.addToQueue(t)
            } else {
                const at = root._dropIndexAt(drop.x, drop.y)
                if (at < 0) {
                    drop.accepted = false
                    return
                }
                Player.insertManual(t, at)
            }
            drop.acceptProposedAction()
        }
    }

    // ---- Snackbar «Очередь очищена · Вернуть» ----
    Snackbar {
        id: undoBar
        anchors.fill: parent
        bottomMargin: 12
        z: 20
    }
}
