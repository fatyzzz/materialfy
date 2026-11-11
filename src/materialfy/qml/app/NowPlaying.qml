import QtQuick
import QtQuick.Layouts
import QtQuick.Effects
import Md3
import Materialfy.Core

// Полноэкранный плеер. Раскрывается через «окно» — случайную фигуру MD3 Expressive:
// фаза 1 — фигура быстро вращается и растёт из маленькой (на обложке мини-плеера) до средней,
// уходя к центру; фаза 2 — растёт на весь экран, пока весь экран не окажется внутри неё.
// Закрытие — тот же путь в обратную сторону. Маска (MultiEffect) включена только на время перехода.
Item {
    id: root

    property bool open: false
    // Уровни звука нужны (Player.levelsWanted ставит Main.qml — их просит и островок): у другого устройства
    // уровня нет — пульт не просит его и не «дышит»; мини-плееру (MiniMode) тоже — там обложка и фон дышат в такт
    readonly property bool wantsLevels: ((visible && open && !(layoutCompact && sheetOpen))
                                       || (Window.window && Window.window.layoutMode === "mini"))
        && Settings.coverPulse && Player.playing && Player.isLocal === true && !Theme.quiet
    property point origin: Qt.point(width / 2, height - 60)
    property string tab: "lyrics"          // lyrics | queue | "" ("" — центр-режим: без очереди/текста)
    readonly property bool coverOnly: root.tab === ""
    // Фоновая сборка вкладок после раскрытия плеера: первый клик без холодной сборки
    property bool _tabsWarmed: false
    Timer {
        id: tabsWarmup
        interval: 400
        onTriggered: root._tabsWarmed = true
    }
    // узкое окно: одна колонка; текст/очередь открываются вместо плеера (sheetOpen) переключателем сверху
    property bool compact: false
    property bool sheetOpen: false
    // Раскладка (одна колонка / две) меняется не мгновенно, а «сквозным затуханием» MD3:
    // старая гаснет, раскладка переключается, новая проявляется с лёгким увеличением
    property bool layoutCompact: false
    Component.onCompleted: layoutCompact = compact
    onCompactChanged: {
        if (open && visible) {
            fadeThrough.restart()
        } else {
            fadeThrough.stop()
            layoutCompact = compact
            body.opacity = 1
            body.scale = 1
        }
    }
    SequentialAnimation {
        id: fadeThrough
        NumberAnimation { target: body; property: "opacity"; to: 0; duration: Theme.motion.effectsFast; easing.type: Easing.InCubic }
        ScriptAction { script: root.layoutCompact = root.compact }
        ParallelAnimation {
            NumberAnimation { target: body; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault; easing.type: Easing.OutCubic }
            NumberAnimation { target: body; property: "scale"; from: 0.94; to: 1; duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
        }
    }
    property real reveal: open ? 1 : 0
    readonly property bool animating: revealAnim.running
    readonly property Item coverItem: bigCover

    readonly property var _windowShapes: ["cookie9", "cookie12", "cookie7", "flower8", "flower6", "clover4", "sunny", "cookie6"]
    readonly property real _split: 0.45
    // reduced motion: без растущей фигуры — экран сразу во весь размер и проявляется затуханием
    readonly property real _g: Theme.reducedMotion ? (reveal > 0 ? 1 : 0) : reveal
    readonly property real _a: Math.min(1, _g / _split)
    readonly property real _b: Math.max(0, (_g - _split) / (1 - _split))

    function _inOut(x) {
        return x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2
    }

    function show(from) {
        // Повторное раскрытие отменяет уход, а не получает его запоздалый onFinished.
        dismiss.stop()
        _miniReleased = false
        if (_dropY > 0)
            dropBack.restart()
        if (from)
            origin = from
        if (!open)
            blob.jumpTo(_windowShapes[Math.floor(Math.random() * _windowShapes.length)])
        open = true
        sheetOpen = false
        forceActiveFocus()
        if (!root._tabsWarmed)
            tabsWarmup.restart()
    }
    function hide() {
        // телефон: плеер уезжает вниз, как лист, а мини-плеер появляется сам (holdsMini), а не принимает
        // в себя сжавшийся плеер. После свайпа — с места, где его отпустил палец, время — по оставшемуся пути
        if (Theme.mobile && open && !Theme.reducedMotion && !Theme.lowPower) {
            if (dismiss.running)
                return
            dropBack.stop()
            dismissMove.duration = Math.max(100, Theme.motion.spatialFast * (1 - Math.min(1, _dropY / Math.max(1, height))))
            dismissMove.to = height
            dismiss.restart()
            return
        }
        collapseIn.stop()
        collapseBack.stop()
        _swipeClosing = collapse > 0 || _dropY > 0   // недосвёрнутый плеер закрывается сразу, без «капли»
        open = false
        collapse = 0
        _dropY = 0
        _swipeClosing = false
    }

    // ---- Закрытие на телефоне: лист вниз ----
    // _dropY — сдвиг плеера вниз (палец или анимация). Мини-плеер скрыт, пока плеер открыт (holdsMini),
    // и появляется с середины ухода — плеер уходит, плашка встаёт на место следом
    property real _dropY: 0
    property bool _miniReleased: false
    readonly property bool holdsMini: open && !_miniReleased
    transform: Translate { y: root._dropY }
    ParallelAnimation {
        id: dismiss
        NumberAnimation {
            id: dismissMove
            target: root; property: "_dropY"
            // трогается сразу (у emphasizedAccelerate первые ~100 мс почти стоят — после нажатия казалось,
            // что плеер медлит) и разгоняется к краю
            easing.type: Easing.BezierSpline; easing.bezierCurve: [0.3, 0.15, 0.9, 0.7, 1.0, 1.0]
        }
        SequentialAnimation {
            PauseAnimation { duration: dismissMove.duration * 0.45 }
            ScriptAction { script: root._miniReleased = true }
        }
        onFinished: {
            root._swipeClosing = true
            root.open = false
            root._dropY = 0
            root._swipeClosing = false
            root._miniReleased = false
        }
    }
    NumberAnimation {
        id: dropBack
        target: root; property: "_dropY"; to: 0
        duration: Theme.motion.spatialFast
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }

    // ---- Сворачивание в мини-плеер (телефон): свайп вниз ведёт его пальцем, кнопка и «Назад» — анимацией ----
    // collapse 0…1: большая обложка уменьшается до обложки плашки и летит на её место (весь плеер — вместе с
    // ней, одним масштабом), рамка плеера стягивается от экрана к прямоугольнику плашки, углы скругляются;
    // в конце плеер растворяется в настоящей плашке под ним
    // этапы: верхняя панель гаснет первой (0…0,2), подписи и кнопки — следом (0,05…0,4), обложка — до конца
    readonly property real _detail: 1 - Math.max(0, Math.min(1, (collapse - 0.05) / 0.35))
    readonly property real _chrome: 1 - Math.max(0, Math.min(1, collapse / 0.2))
    readonly property int _openMs: Theme.motion.playerReveal
    property Item collapseTarget: null       // плашка мини-плеера
    property Item collapseCover: null        // её обложка
    property real collapse: 0
    property bool _swipeClosing: false

    // Телефон: свайп обложки влево/вправо — следующий/предыдущий трек (как в мини-плеере). Обложка едет за
    // пальцем, после порога улетает за край, трек переключается, и новая обложка въезжает с другой стороны —
    // когда она загрузилась (bigCover.swapped) или через 1,5 с
    property real _coverDx: 0
    property int _coverDir: 0                // -1 — следующий (уехала влево), 1 — предыдущий, 0 — нет
    function _coverSwipeAway(dir) {
        MobileShell.haptic()
        _coverDir = dir
        coverSettle.stop()
        coverFlyOut.to = dir * (width * 0.5 + bigCover.width)
        coverFlyOut.restart()
    }
    function _coverArrive() {
        if (_coverDir === 0 || coverFlyOut.running)
            return
        _coverDir = 0
        coverArrival.stop()
        coverSettle.restart()
    }
    NumberAnimation {
        id: coverFlyOut
        target: root; property: "_coverDx"
        duration: Theme.motion.effectsDefault; easing.type: Easing.OutCubic
        onFinished: {
            if (root._coverDir < 0)
                Player.next()
            else
                Player.previous()
            root._coverDx = -root._coverDir * bigCover.width * 0.45
            coverArrival.restart()
        }
    }
    NumberAnimation {
        id: coverSettle
        target: root; property: "_coverDx"; to: 0
        duration: Theme.motion.spatialDefault
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }
    Timer { id: coverArrival; interval: 1500; onTriggered: root._coverArrive() }
    Connections {
        target: Player
        // обложка та же (трек из того же альбома) — подмены не будет, въезжаем сразу
        function onTrackChanged() { Qt.callLater(() => { if (!bigCover.pending) root._coverArrive() }) }
    }
    // большая обложка при collapse = 0: центр и сторона (в координатах плеера)
    property point _bc: Qt.point(width / 2, height / 3)
    property real _bs: 300
    function _measure() {
        const c = bigCover.mapToItem(root, bigCover.width / 2, bigCover.height / 2)
        _bc = c
        _bs = Math.max(1, bigCover.width)
    }
    function _rectOf(item, fallback) {
        if (!item)
            return fallback
        const p = item.mapToItem(root, 0, 0)
        return Qt.rect(p.x, p.y, item.width, item.height)
    }
    readonly property rect _target: { collapse; return _rectOf(collapseTarget, Qt.rect(8, height - 140, width - 16, 64)) }
    readonly property rect _targetCover: { collapse; return _rectOf(collapseCover, Qt.rect(_target.x + 8, _target.y + 8, 48, 48)) }
    // масштаб содержимого и куда встаёт центр обложки
    readonly property real _s: 1 + (_targetCover.width / _bs - 1) * collapse
    // по горизонтали — по центру рамки (плашка во всю ширину) и только в конце к обложке слева
    readonly property real _cx: _bc.x + (_targetCover.x + _targetCover.width / 2 - _bc.x) * Math.pow(collapse, 3)
    readonly property real _cy: _bc.y + (_targetCover.y + _targetCover.height / 2 - _bc.y) * collapse
    readonly property real _tx: _cx - _bc.x * _s
    readonly property real _ty: _cy - _bc.y * _s
    // рамка (в координатах плеера) и она же в координатах содержимого — для маски
    readonly property rect _frame: Qt.rect(_target.x * collapse, _target.y * collapse,
                                           width + (_target.width - width) * collapse,
                                           height + (_target.height - height) * collapse)
    readonly property rect _window: Qt.rect((_frame.x - _tx) / _s, (_frame.y - _ty) / _s, _frame.width / _s, _frame.height / _s)
    NumberAnimation {
        id: collapseIn
        target: root; property: "collapse"; to: 1
        easing.type: Easing.BezierSpline
        easing.bezierCurve: Theme.motion.smoothMove
        onFinished: {
            root._swipeClosing = true
            root.open = false
            root.collapse = 0
            root._swipeClosing = false
        }
    }
    NumberAnimation {
        id: collapseBack
        target: root; property: "collapse"; to: 0
        duration: Theme.motion.spatialDefault
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }

    visible: reveal > 0
    // уезжая вниз, плеер немного тает — под ним уже видно приложение
    opacity: (Theme.reducedMotion ? reveal : 1) * (1 - Math.min(1, _dropY / Math.max(1, height)) * 0.4)
    focus: open
    Keys.onEscapePressed: compact && sheetOpen ? sheetOpen = false : hide()

    Behavior on reveal {
        enabled: !root._swipeClosing
        NumberAnimation {
            id: revealAnim
            // программный рендер — сразу: кадры раскрытия (два слоя и маска на весь экран) рисует процессор
            duration: Theme.lowPower ? 0 : Theme.reducedMotion ? Theme.motion.effectsDefault : root.open ? root._openMs : 620
            easing.type: Easing.Linear     // кривые — у каждой фазы свои, см. blob
            // onFinished здесь не приходит (анимация внутри Behavior сигнал не шлёт).
            // Linear заканчивается ровно на to — снап не нужен.
        }
    }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    // Подложка сворачивания: из экрана — в плашку мини-плеера (её цвет и скругление); фон плеера над ней гаснет
    Rectangle {
        id: collapseCard
        visible: root.collapse > 0
        x: root._frame.x
        y: root._frame.y
        width: root._frame.width
        height: root._frame.height
        // скругление набирается в начале: рамка сразу читается как карточка
        radius: Theme.shape.large * Math.min(1, root.collapse * 4)
        readonly property color from: Theme.surface
        readonly property color to: Theme.surfaceContainerHighest
        color: Qt.rgba(from.r + (to.r - from.r) * root.collapse, from.g + (to.g - from.g) * root.collapse,
                       from.b + (to.b - from.b) * root.collapse, 1)
        opacity: 1 - Math.max(0, (root.collapse - 0.85) / 0.15)
    }

    Elevation { target: collapseCard; level: 3; visible: root.collapse > 0 }

    // Маска сворачивания (в координатах содержимого): окно у обложки со скруглением плашки
    Item {
        id: collapseMask
        anchors.fill: parent
        visible: false
        layer.enabled: root.collapse > 0
        Rectangle {
            x: root._window.x
            y: root._window.y
            width: root._window.width
            height: root._window.height
            radius: Theme.shape.large * Math.min(1, root.collapse * 4) / root._s
            color: "white"
        }
    }

    // Маска раскрытия: фигура-«окно»
    Item {
        id: revealMask
        anchors.fill: parent
        visible: false
        layer.enabled: root.animating || root.reveal < 1
        MorphShape {
            id: blob
            readonly property real e1: root._inOut(root._a)
            readonly property real e2: root._inOut(root._b)
            readonly property real small: 36
            readonly property real medium: 0.42 * Math.min(root.width, root.height)
            // Во впадинах фигура уже описанного круга — берём с запасом, чтобы углы окна оказались внутри
            readonly property real full: 2 * 1.6 * Math.hypot(root.width / 2, root.height / 2)
            readonly property real d: root._b > 0 ? medium + (full - medium) * e2 : small + (medium - small) * e1
            readonly property real cx: root.origin.x + (root.width / 2 - root.origin.x) * e1
            readonly property real cy: root.origin.y + (root.height / 2 - root.origin.y) * e1
            x: cx - d / 2
            y: cy - d / 2
            width: d
            height: d
            rotation: (Theme.mobile ? 20 : 240) * e1 + (Theme.mobile ? 10 : 50) * e2
            color: "white"
            tinted: false
        }
    }

    // Содержимое (в самом конце сворачивания гаснет — без «обрубка» в крошечной фигуре)
    Item {
        id: content
        anchors.fill: parent
        // сворачивание в мини-плеер: масштаб, затем сдвиг (окно у обложки встаёт в рамку плашки)
        transform: [
            Scale { xScale: root._s; yScale: root._s },
            Translate { x: root._tx; y: root._ty }
        ]
        // в конце сжатия плеер растворяется в настоящей плашке под ним
        opacity: Math.min(1, root.reveal / 0.12) * (1 - Math.max(0, (root.collapse - 0.7) / 0.3))
        layer.enabled: root.reveal < 1 || root.collapse > 0
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: root.collapse > 0 ? collapseMask : revealMask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1.0
        }

        // Фон: размытая обложка + вуаль цвета темы. При сворачивании в мини-плеер гаснет — остаётся подложка-плашка
        Item {
        anchors.fill: parent
        opacity: 1 - Math.min(1, root.collapse * 2.5)
        visible: opacity > 0
        Rectangle { anchors.fill: parent; color: Theme.surface }
        // Режим, сила размытия и «плавание» — из настроек (см. CoverBackdrop); при смене трека фон
        // меняется вместе с цветами темы — под той же шторкой (см. TrackBackdrop)
        TrackBackdrop {
            anchors.fill: parent
            // на весь экран — крупная обложка (CoverBackdrop размер в URL не подменяет)
            source: Player.coverLarge !== "" ? Player.coverLarge : Player.cover
            mode: Settings.backdropMode
            blur: Settings.nowPlayingBlur
            flow: Settings.nowPlayingDrift
            followTheme: Settings.accentFromCover
            running: root.visible && Player.playing
        }
        Rectangle {
            anchors.fill: parent
            color: Theme.surface
            opacity: Theme.dark ? 0.62 : 0.55
        }
        }

        // Блокирует клики по приложению под плеером
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: wheel => wheel.accepted = true }

        // Телефон: свайп вниз по плееру — свернуть (текст песни и очередь листаются сами — там жест их).
        // Слой поверх всего содержимого: на самом content обработчик не видел касаний — их принимали MouseArea
        // выше (блокировка кликов, кнопки), и до обработчиков родителя нажатие не доходило. Отсюда DragHandler
        // видит нажатие первым и держит его пассивно: тапы и горизонтальные ползунки работают как раньше.
        Item {
            anchors.fill: parent
            z: 100
            DragHandler {
                id: swipeDown
                enabled: Theme.mobile && root.open && !BackStack.hasOverlay && !(root.layoutCompact && root.sheetOpen)
                target: null
                xAxis.enabled: false
                // Сохраняем скорость последнего кадра. Отмена захвата только возвращает плеер на место.
                // Плеер едет за пальцем вниз целиком, как лист
                property real vy: 0
                onTranslationChanged: {
                    if (!active || dismiss.running)
                        return
                    root._dropY = Math.max(0, translation.y)
                    vy = centroid.velocity.y
                }
                onActiveChanged: {
                    if (active) {
                        vy = 0
                        dropBack.stop()
                    }
                }
                onGrabChanged: transition => {
                    const released = transition === PointerDevice.UngrabExclusive
                    if (!released && transition !== PointerDevice.CancelGrabExclusive)
                        return
                    if (dismiss.running)
                        return
                    if (released && (root._dropY > root.height * 0.18 || (vy > 900 && root._dropY > 24)))
                        root.hide()
                    else
                        dropBack.restart()
                }
            }
        }

        // Верхняя панель: свернуть + переключатель очередь/текст (над правой колонкой)
        RowLayout {
            id: topRow
            opacity: root._chrome           // сворачивание: панель гаснет первой
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 16
            // телефон: под статус-баром
            anchors.topMargin: 16 + Theme.safeTop
            spacing: 12
            IconButton { icon: "keyboard_arrow_down"; Accessible.name: I18n.t("Minimize"); style: "tonal"; onClicked: root.hide() }
            Item { Layout.fillWidth: true }
            DeviceButton { iconOnly: root.layoutCompact; usePanel: false }   // правая панель — под этим экраном
            ButtonGroup {
                model: [{ icon: "title", accessibleName: I18n.t("Lyrics") }, { icon: "queue_music", accessibleName: I18n.t("Open queue") }]
                autoSelect: false
                currentIndex: root.layoutCompact && !root.sheetOpen ? -1 : root.tab === "lyrics" ? 0 : root.tab === "queue" ? 1 : -1
                onActivated: i => {
                    const next = i === 0 ? "lyrics" : "queue"
                    if (root.layoutCompact && root.sheetOpen && root.tab === next) {
                        root.sheetOpen = false
                    } else if (!root.layoutCompact && root.tab === next) {
                        // Повторное нажатие активной вкладки — центр-режим: только плеер по центру
                        root.tab = ""
                        root.sheetOpen = false  // иначе после сужения окна — пустой экран
                    } else {
                        root.tab = next
                        root.sheetOpen = true
                    }
                }
            }
            // плеер закрывает всё окно — кнопки окна и перетаскивание за пустое место панели здесь свои
            WindowButtons {
                id: windowButtons
                visible: !Theme.mobile
                Layout.alignment: Qt.AlignVCenter
                Layout.leftMargin: 4
                window: root.Window.window
            }

            DragHandler {
                property bool manual: false
                enabled: !Theme.mobile
                target: null
                grabPermissions: PointerHandler.TakeOverForbidden
                onActiveChanged: if (active) manual = !root.Window.window.startSystemMove()
                onCentroidChanged: if (active && manual && root.Window.window.visibility === Window.Windowed) {
                    root.Window.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
                    root.Window.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
                }
            }
            TapHandler {
                enabled: !Theme.mobile
                acceptedButtons: Qt.LeftButton
                onDoubleTapped: windowButtons.toggleMaximized()
            }
        }

        Item {
            id: body
            anchors.top: topRow.bottom
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: root.layoutCompact ? 16 : 32
            anchors.topMargin: 8
            // телефон: над полосой навигации системы
            anchors.bottomMargin: (root.layoutCompact ? 16 : 32) + Theme.safeBottom

            // ---- Колонка плеера (слева, с отступом от края): обложка и управление ----
            ColumnLayout {
                id: playerColumn
                visible: !root.layoutCompact || !root.sheetOpen
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                // x/width интерполируются blend'ом (0 — обычный, 1 — центр): ресайз окна мгновенен,
                // переход — плавный. Правая панель занимает остаток и тает по мере вытеснения.
                property real _blend: root.coverOnly && !root.layoutCompact ? 1 : 0
                Behavior on _blend { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
                readonly property real _normalW: Math.min(460, root.width * 0.42)
                readonly property real _normalX: Math.max(16, root.width * 0.06)
                readonly property real _coverW: Math.min(640, root.width * 0.55)
                readonly property real _coverX: (body.width - _coverW) / 2
                x: root.layoutCompact ? 0 : _normalX + (_coverX - _normalX) * _blend
                width: root.layoutCompact ? body.width : _normalW + (_coverW - _normalW) * _blend
                spacing: 12
                // ширина колонки из размеров окна, а не из уже разложенной колонки: иначе обложка и подписи
                // меняют ширину, от которой сами зависят (recursive rearrange)
                readonly property real colWidth: playerColumn.width
                readonly property real controlWidth: root.layoutCompact && colWidth < 4 * 56 + 72 + 8
                                                     ? (Theme.mobile ? 48 : 40) : 56
                readonly property string ctl: controlWidth < 56 ? "s" : "m"
                // Высота подписей и пульта меняется: длинный заголовок, заблокированная громкость,
                // потерянное устройство. Остаток для обложки считаем по самим контролам.
                readonly property real controlsHeight: {
                    const items = visibleChildren
                    let total = Math.max(0, items.length - 1) * spacing
                    for (let i = 0; i < items.length; i++) {
                        const item = items[i]
                        total += item.Layout.topMargin + item.Layout.bottomMargin
                        if (item === bigCover)
                            continue
                        const preferred = item.Layout.preferredHeight
                        total += Math.max(item.Layout.minimumHeight, preferred >= 0 ? preferred : item.implicitHeight)
                    }
                    return total
                }

                Item { Layout.fillHeight: true }

                // «Дыхание» в такт: уровень звука плеера (просит Main.qml по root.wantsLevels); доля над
                // скользящим средним — удар
                property real avgLevel: 0
                property real beat: 0
                Behavior on beat { NumberAnimation { duration: 110; easing.type: Easing.OutQuad } }
                Connections {
                    target: Player
                    enabled: root.open && playerColumn.visible && root.wantsLevels
                    function onLevelChanged() {
                        playerColumn.avgLevel += (Player.level - playerColumn.avgLevel) * 0.06
                        const b = Math.max(0, Math.min(1, (Player.level - playerColumn.avgLevel) * 4))
                        // пишем только значимые сдвиги: каждый старт Behavior перерисовывает всё окно,
                        // а события уровня приходят 30–60 раз в секунду
                        if (Math.abs(b - playerColumn.beat) > 0.04)
                            playerColumn.beat = b
                    }
                }
                Connections {
                    target: Player
                    function onStateChanged() { if (!Player.playing) playerColumn.beat = 0 }
                }

                NowPlayingCover {
                    id: bigCover
                    // Центр-режим: панель справа убрана, колонка шире — обложке даём вырасти по ширине;
                    // вертикальный бюджет тот же (текст и управление остаются)
                    // Размер интерполируется blend'ом (0 — обычный, 1 — центр-режим): ресайз окна мгновенен
                    property real _max: root.coverOnly ? 520 : 440
                    Behavior on _max { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
                    readonly property real s: root.layoutCompact ? Math.max(0, Math.min(playerColumn.colWidth * (Theme.mobile ? 0.84 : 1), body.height - playerColumn.controlsHeight, 360))
                                                           : Math.max(0, Math.min(playerColumn.colWidth, body.height - playerColumn.controlsHeight, _max))
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: s
                    Layout.preferredHeight: s
                    Layout.bottomMargin: root.layoutCompact ? 12 : 20
                    source: Player.coverLarge !== "" ? Player.coverLarge : Player.cover
                    // сворачивание: обложка заранее принимает форму обложки мини-плеера
                    restShape: Player.playing ? "cookie12" : root.collapse > 0.2 ? "softSquare" : "square"
                    direction: Player.direction
                    pulse: root.wantsLevels ? playerColumn.beat : 0
                    instantSwap: root._coverDir !== 0
                    onSwapped: root._coverArrive()
                    // свайп: едет за пальцем с лёгким наклоном и тает к краю; ждёт новую обложку за краем невидимой
                    opacity: root._coverDir !== 0 && !coverFlyOut.running ? 0 : 1 - Math.min(1, Math.abs(root._coverDx) / (width * 1.2)) * 0.6
                    transform: [
                        Translate { x: root._coverDx },
                        Rotation { origin.x: bigCover.width / 2; origin.y: bigCover.height; angle: root._coverDx / Math.max(1, bigCover.width) * 10 }
                    ]
                    DragHandler {
                        id: coverSwipe
                        enabled: Theme.mobile && root.open && !BackStack.hasOverlay && Player.hasTrack && root.collapse === 0 && root._dropY === 0 && !dismiss.running
                        target: null
                        yAxis.enabled: false
                        // translation обнуляется раньше, чем приходит active=false: смещение и скорость запоминаем
                        property real lastX: 0
                        property real vx: 0
                        onTranslationChanged: {
                            if (!active || root._coverDir !== 0)
                                return
                            lastX = translation.x
                            vx = centroid.velocity.x
                            root._coverDx = lastX
                        }
                        onActiveChanged: {
                            if (active) {
                                lastX = 0
                                vx = 0
                                coverSettle.stop()
                            }
                        }
                        onGrabChanged: transition => {
                            const released = transition === PointerDevice.UngrabExclusive
                            if (!released && transition !== PointerDevice.CancelGrabExclusive)
                                return
                            if (root._coverDir !== 0)
                                return
                            const flick = Math.abs(vx) > 900 && Math.abs(lastX) > 24
                            if (released && (Math.abs(lastX) > bigCover.width * 0.3 || flick))
                                root._coverSwipeAway(lastX < 0 ? -1 : 1)
                            else
                                coverSettle.restart()
                        }
                    }
                    spinning: Settings.coverSpin && Player.playing && root.open && Player.isLocal === true
                    DeviceBadge {
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: parent.width * 0.06
                        size: 34
                        ringColor: Theme.surface
                    }
                    DjBadge {
                        anchors.left: parent.left
                        anchors.top: parent.top
                        anchors.margins: parent.width * 0.06
                    }
                }

                // Подписи при смене трека проявляются заново, чуть снизу
                property real infoIn: 1
                Connections {
                    target: Player
                    function onTrackChanged() {
                        if (root.visible && playerColumn._lastId !== Player.trackId) {
                            playerColumn.infoIn = 0
                            infoAnim.restart()
                        }
                        playerColumn._lastId = Player.trackId
                    }
                }
                property string _lastId: ""
                TapHandler {
                    enabled: Player.hasTrack
                    acceptedButtons: Qt.RightButton
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad   // касание — не правый клик: у него нет кнопок, TapHandler принял бы любой тап
                    onTapped: (point) => {
                        const p = playerColumn.mapToItem(null, point.position.x, point.position.y)
                        Catalog.requestTrackMenu(Player.track, p.x, p.y)
                    }
                }
                NumberAnimation {
                    id: infoAnim
                    target: playerColumn
                    property: "infoIn"
                    to: 1
                    duration: Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }

                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: Player.title
                    type: root.layoutCompact ? "headlineSmall" : "headlineMedium"
                    weight: 700
                    // длинное название — в две строки, а не «по коридору красных фон…»
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                    opacity: playerColumn.infoIn * root._detail
                    transform: Translate { y: (1 - playerColumn.infoIn) * 14 }
                }
                ArtistLinks {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: Math.min(naturalWidth, playerColumn.colWidth)
                    Layout.maximumWidth: playerColumn.colWidth
                    Layout.topMargin: -6
                    centered: true
                    visible: Player.isEpisode !== true
                    refs: Player.track ? (Player.track.artistRefs ?? []) : []
                    text: Player.artist
                    opacity: Math.max(0, playerColumn.infoIn * 1.3 - 0.3)    // чуть позже названия
                    transform: Translate { y: (1 - playerColumn.infoIn) * 18 }
                    type: "titleMedium"
                    color: Theme.fgSurfaceVariant
                }
                LinkLabel {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.maximumWidth: playerColumn.colWidth
                    Layout.topMargin: -6
                    horizontalAlignment: Text.AlignHCenter
                    visible: Player.isEpisode === true
                    // у выпуска подкаста под названием — подкаст: ссылка на его страницу
                    linkEnabled: (Player.track.showId ?? "") !== ""
                    onClicked: Catalog.openShow(Player.track.showId ?? "")
                    opacity: Math.max(0, playerColumn.infoIn * 1.3 - 0.3) * root._detail
                    transform: Translate { y: (1 - playerColumn.infoIn) * 18 }
                    text: Player.artist
                    type: "titleMedium"
                    color: Theme.fgSurfaceVariant
                }
                LinkLabel {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.maximumWidth: playerColumn.colWidth
                    Layout.topMargin: -6
                    horizontalAlignment: Text.AlignHCenter
                    // у выпуска подкаста и трека без альбома ссылки нет
                    linkEnabled: (Player.track.albumId ?? "") !== ""
                    onClicked: Catalog.openAlbum(Player.track.albumId ?? "")
                    opacity: Math.max(0, playerColumn.infoIn * 1.3 - 0.3) * root._detail    // чуть позже названия
                    transform: Translate { y: (1 - playerColumn.infoIn) * 18 }
                    text: Player.album + (Player.codec !== "" && Player.isLocal === true ? "  ·  " + Player.codec : "")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }

                // Прогресс
                readonly property alias progress: playerProgress.value
                PlayerProgress { id: playerProgress; active: bigSeek.visible && !bigSeek.dragging }
                RowLayout {
                    opacity: root._detail   // сворачивание в мини-плеер: подписи и кнопки гаснут раньше обложки
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    spacing: 12
                    Label {
                        text: root._time(bigSeek.dragging ? bigSeek.dragValue * Player.durationMs : Player.positionMs)
                        type: "labelMedium"
                        color: Theme.fgSurfaceVariant
                    }
                    WavyProgress {
                        id: bigSeek
                        Layout.fillWidth: true
                        thickness: 6
                        amplitude: 4
                        wavelength: 48
                        value: playerColumn.progress
                        wavy: Player.playing
                        interactive: true
                        onCommitted: v => Player.seek(v)
                    }
                    Label {
                        text: "−" + root._time(Player.durationMs - Player.positionMs)
                        type: "labelMedium"
                        color: Theme.fgSurfaceVariant
                    }
                }

                // Кнопки
                RowLayout {
                    opacity: root._detail   // сворачивание в мини-плеер: подписи и кнопки гаснут раньше обложки
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 4
                    // узкий телефон (< 380): 4 × 56 + 72 с промежутками по 12 шире колонки — распирали весь экран
                    spacing: root.layoutCompact ? Math.max(2, Math.min(12, (playerColumn.colWidth - 4 * playerColumn.controlWidth - 72) / 4)) : 12
                    component TransportButton: IconButton {
                        size: playerColumn.ctl
                        implicitWidth: playerColumn.controlWidth
                        implicitHeight: playerColumn.controlWidth
                    }
                    TransportButton {
                        icon: Player.isEpisode ? "replay_10" : "shuffle"
                        checkable: !Player.isEpisode; autoToggle: false; checked: !Player.isEpisode && Player.shuffle
                        enabled: Player.isEpisode || Player.source !== "radio"
                        style: checked ? "tonal" : "standard"
                        onClicked: {
                            if (Player.isEpisode)
                                Player.seekRelative(-10000)
                            else
                                Player.shuffle = !Player.shuffle
                        }
                    }
                    TransportButton { icon: "skip_previous"; Accessible.name: I18n.t("Previous track"); iconColor: Theme.fgSurface; onClicked: Player.previous() }
                    PlayButton {
                        size: root.layoutCompact ? 72 : 96
                        playing: Player.playing
                        loading: Player.buffering
                        spin: Player.isLocal === true
                        containerColor: Theme.primaryContainer
                        iconColor: Theme.fgPrimaryContainer
                        onClicked: Player.togglePlay()
                    }
                    TransportButton { icon: "skip_next"; Accessible.name: I18n.t("Next track"); iconColor: Theme.fgSurface; onClicked: Player.next() }
                    TransportButton {
                        icon: Player.isEpisode ? "forward_30" : Player.repeat === "one" ? "repeat_one" : "repeat"
                        checkable: !Player.isEpisode; autoToggle: false; checked: !Player.isEpisode && Player.repeat !== "off"
                        style: checked ? "tonal" : "standard"
                        onClicked: Player.isEpisode ? Player.seekRelative(30000) : Player.cycleRepeat()
                    }
                }

                // Не рекомендовать (радио) · устройство · громкость · лайк или закладка выпуска
                RowLayout {
                    opacity: root._detail   // сворачивание в мини-плеер: подписи и кнопки гаснут раньше обложки
                    id: volumeRow
                    // громкость играющего устройства; у части устройств её меняют только на них самих
                    readonly property bool locked: Player.remote === true && Player.canSetVolume === false
                    readonly property bool slider: !Theme.mobile || Player.remote === true
                    readonly property bool stretch: Theme.mobile && slider
                    Layout.alignment: Qt.AlignHCenter
                    Layout.fillWidth: stretch
                    Layout.topMargin: 4
                    spacing: stretch ? 8 : 12
                    IconButton {
                        visible: Player.source === "radio" && !Player.isEpisode
                        icon: "thumb_down"
                        enabled: Player.hasTrack
                        onClicked: Library.dislike(Player.track)
                    }
                    DeviceBadge.Info { id: npDevice; type: (Player.device ?? {}).type ?? "" }
                    Icon {
                        visible: Player.remote === true
                        Layout.leftMargin: 8
                        name: npDevice.icon
                        size: 22
                        fill: 1
                        color: Theme.primary
                    }
                    // Телефон, музыка играет здесь: громкость — кнопками телефона, отдельный ползунок не нужен
                    Icon {
                        visible: volumeRow.slider
                        Layout.leftMargin: Player.remote === true ? 0 : 8
                        name: volumeRow.locked ? "lock" : Player.volume > 0.5 ? "volume_up" : Player.volume > 0 ? "volume_down" : "volume_off"
                        size: 22
                    }
                    Slider {
                        visible: volumeRow.slider
                        Layout.fillWidth: volumeRow.stretch
                        Layout.minimumWidth: volumeRow.stretch ? 48 : 0
                        Layout.preferredWidth: root.layoutCompact ? Math.max(48, Math.min(220, playerColumn.colWidth - 260)) : 220
                        Accessible.name: I18n.t("Volume")
                        value: Player.volume
                        trackHeight: 14
                        handleHeight: 36
                        enabled: !volumeRow.locked
                        opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                        onMoved: v => Player.volume = v
                    }
                    LikeButton { visible: !Player.isEpisode; Layout.leftMargin: 8; track: Player.track }
                    IconButton {
                        visible: Player.isEpisode
                        Layout.leftMargin: 8
                        icon: "bookmark"
                        checkable: true
                        autoToggle: false
                        checked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
                        enabled: Player.uri !== ""
                        onClicked: Library.toggleSaved(Player.uri)
                    }
                    IconButton {
                        id: bigMore
                        icon: "more_horiz"
                        Accessible.name: I18n.t("More")
                        iconSize: 30
                        iconWeight: 700
                        enabled: Player.hasTrack
                        onClicked: {
                            const p = bigMore.mapToItem(null, 0, 0)
                            Catalog.requestTrackMenuAt(Player.track, p.x, p.y, bigMore.width, bigMore.height)
                        }
                    }
                }
                Label {
                    opacity: root._detail   // сворачивание в мини-плеер: подписи и кнопки гаснут раньше обложки
                    visible: volumeRow.locked
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: -4
                    text: I18n.t("Volume can only be changed on the device")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }

                // Пульт: «Играет на … · Слушать здесь», устройство пропало — «Продолжить здесь?»
                ConnectBanner {
                    opacity: root._detail   // сворачивание в мини-плеер: подписи и кнопки гаснут раньше обложки
                    id: npBanner
                    Layout.fillWidth: true
                    Layout.maximumWidth: 460
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 6
                }

                Item { Layout.fillHeight: true }
            }

            // ---- Колонка очереди / текста (справа) ----
            // Смена вкладки — shared axis X (порядок как у переключателя: текст слева, очередь справа):
            // уходящая уезжает в свою сторону и гаснет, новая выезжает с другой.
            Item {
                // Центр-режим: панель тает и сжимается, место отдаётся колонке плеера
                opacity: root.coverOnly ? 0 : 1
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                visible: opacity > 0.001 && (!root.layoutCompact || root.sheetOpen)
                enabled: !root.coverOnly
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                // Геометрия — целевая (не от анимированной колонки): текст и очередь при возврате
                // из центра раскладываются сразу, анимируется только прозрачность
                x: root.layoutCompact ? 0 : playerColumn._normalX + playerColumn._normalW + 48
                width: root.layoutCompact ? parent.width
                    : Math.max(0, parent.width - x - Math.max(16, root.width * 0.06))

                component TabShift: NumberAnimation {
                    duration: Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }

                Item {
                    id: queueTab
                    property real shown: root.tab === "queue" ? 1 : 0
                    Behavior on shown { TabShift {} }
                    anchors.fill: parent
                    visible: shown > 0.001
                    opacity: Math.pow(shown, 1.6)
                    transform: Translate { x: (1 - queueTab.shown) * 80 }
                    // во время выезда строкам очереди нечего пересчитывать: отключаем интерактив,
                    // чтобы hover-морфы и StateLayer не инвалидировали кадры анимации
                    enabled: shown > 0.99
                    // Сборка очереди асинхронно: холодная сборка на клик давала фриз
                    Loader {
                        anchors.fill: parent
                        asynchronous: true
                        active: queueTab.shown > 0.001 || root._tabsWarmed
                        sourceComponent: QueueView { anchors.fill: parent; compact: true }
                    }
                }

                // Текст песни собирается асинхронно: холодная сборка на клик давала фриз
                Item {
                    id: lyricsTab
                    property real shown: root.tab === "lyrics" ? 1 : 0
                    Behavior on shown { TabShift {} }
                    anchors.fill: parent
                    visible: shown > 0.001 && root.visible
                    opacity: Math.pow(shown, 1.6)
                    transform: Translate { x: -(1 - lyricsTab.shown) * 80 }
                    enabled: shown > 0.99
                    Loader {
                        anchors.fill: parent
                        asynchronous: true
                        active: lyricsTab.shown > 0.001 || root._tabsWarmed
                        sourceComponent: LyricsView {
                            anchors.fill: parent
                            baseFontSize: Math.max(26, Math.min(40, root.width / 36))
                        }
                    }
                }
            }
        }
    }
}
