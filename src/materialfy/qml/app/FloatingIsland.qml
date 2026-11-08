import QtQuick
import QtQuick.Effects
import Md3
import Materialfy.Core

// Островок («динамический вырез»): отдельное окно поверх всех у верхнего края экрана. Чёрный (как железо)
// или цветной (тон обложки). Верх прямой — островок растёт из кромки экрана; на Mac с вырезом его верх —
// сам вырез, по бокам вогнутые «плечи». Состояния (макет docs/design/50-dynamic-island.html):
//   idle         — ничего не играет: тонкая полоска 120 × 5 у кромки (на Mac — ничего, вырез и есть полоска).
//                  Курсор упёрся в верхний край над ней (на Mac — навели на вырез) — выезжает peek
//   peek         — 260 × 44: «Ничего не играет», клик открывает Materialfy
//   compact      — 164 × 36: обложка слева, полоски уровня (или пауза) справа
//   volume       — 240 × 36: колесо или ↑ ↓ — проценты по центру, уровень на обложке
//   announcement — 360 × 52 на 2,5 с при смене трека: обложка, название и исполнитель со шторкой
//   expanded     — 400 × 158 при наведении: обложка, кнопки, волна прогресса, «назад / пуск / дальше»
// Наведение — опросом курсора (WindowShell.cursorIn), а не HoverHandler: окно с маской и без фокуса Qt
// узнаёт о мыши поздно, а полоску в покое курсор только задевает у самого края. Клавиши без фокуса
// (← → треки, ↑ ↓ громкость, пробел) — пока курсор над островком (cpp/floating.cpp).
Window {
    id: root

    required property Window mainWindow
    property var targetScreen: null
    property int screenIndex: 0
    property string forcedState: ""
    property string shotPath: ""

    Timer {
        interval: 9000
        running: root.shotPath !== ""
        onTriggered: root.contentItem.grabToImage(result => result.saveToFile(root.shotPath))
    }

    // Вырез экрана Mac (ставит WindowShell.makeToolWindow, cpp/floating_mac.mm): там ничего не видно,
    // содержимое раскрытого островка — под ним. Без выреза — 0
    property int notchW: 0
    property int notchH: 0
    readonly property bool mac: notchH > 0
    readonly property int ear: 10

    readonly property bool motion: !Theme.reducedMotion && !Theme.lowPower
    readonly property bool wantsLevels: visible && mode !== "idle" && mode !== "peek" && Player.playing
        && Player.isLocal === true && !Theme.lowPower

    // ---- Цвета: чёрный (как железо) или цветной (тон обложки) ----
    readonly property bool colorMode: Settings.floatingPlayerStyle === "color" && !mac  // на Mac тело сливается с вырезом
    // цветной: размытая обложка под тёмным стеклом (текст светлый) или, пока обложки нет, тональный контейнер
    readonly property bool _onCover: colorMode && Player.cover !== ""
    readonly property color body: colorMode ? Theme.primaryContainer : "#000000"
    readonly property color ink: colorMode && !_onCover ? Theme.fgPrimaryContainer : "#ffffff"
    readonly property color inkDim: Qt.alpha(ink, 0.72)
    // на чёрном — «фиксированный» акцент MD3: он одинаково светлый в светлой и тёмной теме приложения
    readonly property color accent: colorMode ? ink : Theme.primaryFixedDim

    // ---- Состояние ----
    property bool _open: false          // раскрыт наведением
    property bool _peek: false          // в покое выехал по курсору у края
    property bool _wings: false         // Mac: курсор у выреза — по бокам от него выехали крылья с кнопками
    property bool _announce: false
    property bool _vol: false
    property bool _keys: false          // подсказка с клавишами под раскрытым островком

    readonly property string mode: forcedState !== "" ? forcedState
        : !Player.hasTrack ? (_peek ? "peek" : "idle")
        : _open ? "expanded"
        : _wings ? "wings"
        : _announce ? "announcement"
        : _vol ? "volume"
        : mac ? "idle"                  // Mac: в покое — только вырез, островок — по наведению
        : "compact"

    // край экрана: top / bottom / left / right / free (на Mac — всегда top, островок в вырезе)
    readonly property string edge: Qt.platform.os === "osx" ? "top" : Settings.floatingEdge
    readonly property bool vertical: edge === "left" || edge === "right"
    readonly property bool free: edge === "free"
    // у края тянется только вдоль него, свободный — куда угодно
    readonly property bool _moveX: !vertical
    readonly property bool _moveY: vertical || free

    readonly property int expandedH: notchH + 158
    readonly property int wingW: 106
    // Сбоку (left / right) островок вертикальный: прямой стороной к краю, растёт внутрь экрана; текст не повёрнут
    readonly property int sideW: 216
    readonly property int sideH: 220
    readonly property int sideBaseH: 112   // выше этого — уже раскрытие (island.t)
    readonly property int targetW: vertical ? (mode === "expanded" || mode === "announcement" ? sideW
            : mode === "peek" ? 200
            : mode === "volume" ? 48
            : mode === "idle" ? 5
            : 36)
        : mode === "expanded" ? 400
        : mode === "wings" ? notchW + 2 * wingW
        : mode === "announcement" ? 360
        : mode === "peek" ? 260
        : mode === "volume" ? 240
        : mode === "idle" ? (mac ? notchW : 120)
        : (mac ? notchW + 80 : 164)
    readonly property int targetH: vertical ? (mode === "expanded" ? sideH
            : mode === "announcement" ? 108
            : mode === "peek" ? 96
            : mode === "volume" ? sideBaseH
            : mode === "idle" ? 120
            : 84)
        : mode === "expanded" ? expandedH
        : mode === "announcement" ? notchH + 52
        : mode === "peek" ? notchH + 44
        : mode === "idle" || mode === "wings" ? (mac ? notchH : free ? 10 : 5)
        : mac && mode === "compact" ? notchH
        : notchH + 36

    // окно — по раскрытому островку своего края и подсказке клавиш под ним (сбоку островок посередине окна)
    width: vertical ? Math.max(sideW + 24, keysPopover.width + 16) : Math.max(400, notchW + 2 * wingW) + 2 * ear + 32
    height: vertical ? sideH + 2 * 52 : expandedH + 56 // запас на подсказку клавиш под островком
    color: "transparent"
    // KDE Wayland: окно ставит скрипт KWin и находит его по заголовку (cpp: crates/mf-platform/src/kwin.rs)
    readonly property bool kwin: Qt.platform.pluginName.indexOf("wayland") === 0 && WindowShell.kwinAvailable()
    readonly property string kwinTitle: "Materialfy Island " + screenIndex
    title: kwin ? kwinTitle : "Materialfy"
    flags: Qt.Tool | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
    transientParent: null
    // x и y — от целевого экрана, а не от screen: macOS сам переводит окно на экран, где лежат его координаты
    readonly property var _screen: targetScreen || (mainWindow ? mainWindow.screen : null)
    screen: _screen
    // У края — сдвиг от его середины (floatingOffsetX), свободный — место на экране (floatingFreeX / Y); то и другое
    // задаётся в настройках и перетаскиванием, за край экрана не уезжает
    readonly property int _maxOffset: _screen ? Math.max(0, Math.round(((vertical ? _screen.height - height : _screen.width - width)) / 2)) : 0
    readonly property int offsetX: mac ? 0 : Math.max(-_maxOffset, Math.min(_maxOffset, Settings.floatingOffsetX))
    function _free(v, fallback, max) { return v === -1000000 ? fallback : Math.max(0, Math.min(max, v)) }
    readonly property int _freeX: _screen ? _free(Settings.floatingFreeX, Math.round((_screen.width - width) / 2), Math.max(0, _screen.width - width)) : 0
    readonly property int _freeY: _screen ? _free(Settings.floatingFreeY, 0, Math.max(0, _screen.height - height)) : 0
    // пока тащат (Windows, X11) — окно там, куда его довели (_dragX / _dragY)
    property bool _dragging: false
    property real _dragX: 0
    property real _dragY: 0
    x: _dragging ? _dragX
        : !_screen ? 0
        : _screen.virtualX + (free ? _freeX
            : edge === "left" ? 0
            : edge === "right" ? _screen.width - width
            : Math.round((_screen.width - width) / 2) + offsetX)
    y: _dragging ? _dragY
        : !_screen ? 0
        : _screen.virtualY + (free ? _freeY
            : edge === "bottom" ? _screen.height - height
            : vertical ? Math.round((_screen.height - height) / 2) + offsetX
            : 0)

    property bool _ready: false
    property bool _fullscreen: false
    // macOS: островок — только в вырезе экрана; экран без выреза его не показывает
    visible: _ready && (!_fullscreen || shotPath !== "") && (Qt.platform.os !== "osx" || mac)

    // x, y, ширина и высота меняются пачкой при создании: скрипт KWin грузим один раз, а не на каждое изменение
    Timer {
        id: kwinTimer
        interval: 100
        onTriggered: WindowShell.kwinPlace(root.kwinTitle, Math.round(root.x), Math.round(root.y), Math.round(root.width), Math.round(root.height),
                                            Settings.floatingHideFullscreen, Settings.floatingHideMaximized, root._moveX, root._moveY)
    }
    function _kwinPlace() {
        if (kwin && _screen)
            kwinTimer.restart()
    }
    onXChanged: _kwinPlace()
    onYChanged: _kwinPlace()
    onWidthChanged: _kwinPlace()
    onHeightChanged: _kwinPlace()
    Connections {
        target: Settings
        function onFloatingHideFullscreenChanged() { root._kwinPlace() }
        function onFloatingHideMaximizedChanged() { root._kwinPlace() }
        function onFloatingEdgeChanged() { root._relocate() }
    }

    // Сменили край: свернуться, забыть наведение и тянущееся перетаскивание, новая маска и место. На KDE Wayland окно
    // переставит скрипт KWin чуть позже — пока не встало, островок не показываем (не мелькает на старом месте)
    property bool _settling: false
    Timer { id: settle; interval: 300; onTriggered: root._settling = false }
    function _relocate() {
        leave.stop()
        _collapse()
        _dwell = 0
        _inside = false
        _announce = false
        _vol = false
        _dragging = false
        _moving = false
        if (kwin) {
            _settling = true
            settle.restart()
        }
        // новый край — сразу в новом размере, без перетекания из старого
        Qt.callLater(() => { widthAnim.complete(); heightAnim.complete(); _applyMask() })
        _kwinPlace()
    }

    // Отпустили окно (x, y — его левый верхний угол на рабочем столе): у края запоминаем сдвиг вдоль него, свободный —
    // место на экране. Скрипт KWin возвращает окно на место, даже если настройка не изменилась (прижимает к краю)
    function _drop(x, y) {
        if (!_screen)
            return
        const lim = (v, lo, hi) => Math.max(lo, Math.min(hi, Math.round(v)))
        if (free) {
            Settings.floatingFreeX = lim(x - _screen.virtualX, 0, Math.max(0, _screen.width - width))
            Settings.floatingFreeY = lim(y - _screen.virtualY, 0, Math.max(0, _screen.height - height))
        } else {
            const offset = vertical ? y - _screen.virtualY - Math.round((_screen.height - height) / 2)
                : x - _screen.virtualX - Math.round((_screen.width - width) / 2)
            Settings.floatingOffsetX = lim(offset, -_maxOffset, _maxOffset)
        }
        _kwinPlace()
    }
    // KDE Wayland: окно тащит сам KWin (startSystemMove), его скрипт сообщает, где отпустили. Ответа нет (скрипт не
    // загрузился) — через 30 с перестаём ждать
    property bool _moving: false
    Timer { id: moveWait; interval: 30000; onTriggered: root._moving = false }
    Connections {
        target: WindowShell
        enabled: root.kwin
        function onKwinMoved(title, x, y) {
            if (title !== root.kwinTitle)
                return
            root._moving = false
            moveWait.stop()
            root._drop(x, y)
        }
    }

    Component.onCompleted: {
        _kwinPlace()
        WindowShell.makeToolWindow(root)
        _applyMask()
        _ready = true
    }
    Component.onDestruction: { WindowShell.keyHook(root, false); if (kwin) WindowShell.kwinRelease(kwinTitle) }
    // Qt сбрасывает уровень окна при показе (macOS): снова над строкой меню
    onVisibleChanged: if (!visible) { _collapse(); WindowShell.keyHook(root, false) } else if (Qt.platform.os === "osx") WindowShell.makeToolWindow(root)

    // ---- Курсор: опрос 10 раз в секунду ----
    // В покое — полоса у самой кромки над полоской (курсор «упёрся» в край), на Mac — вырез; иначе — сам островок.
    // Зажата кнопка мыши (тащат окно к краю, выделяют) — не раскрываемся.
    property int _dwell: 0
    readonly property int shrinkMs: 220
    readonly property var growCurve: [0.38, 1.21, 0.22, 1.0, 1.0, 1.0]
    readonly property var shrinkCurve: [0.3, 0.0, 0.1, 1.0, 1.0, 1.0]
    property bool _inside: false
    on_InsideChanged: WindowShell.keyHook(root, _inside && mode !== "idle")
    Timer {
        // раскрыт — чаще: уход курсора ловим за ~40 мс, а не за 100
        interval: root._open || root._peek || root._wings ? 40 : 100
        repeat: true
        running: root.visible && root.forcedState === ""
        onTriggered: {
            const idle = root.mode === "idle"
            // в покое у края — полоса 320 × 3 вдоль кромки экрана, свободный — сама полоска
            const edgeZone = idle && !root.mac && !root.free
            const zw = idle && root.mac ? root.notchW + 20 : edgeZone ? (root.vertical ? 3 : 320) : island.width
            const zh = idle && root.mac ? root.notchH : edgeZone ? (root.vertical ? 320 : 3) : island.height
            const zx = root.edge === "right" ? island.x + island.width - zw : root.edge === "left" ? island.x : island.x + (island.width - zw) / 2
            const zy = root.edge === "bottom" ? island.y + island.height - zh : root.vertical ? island.y + (island.height - zh) / 2 : island.y
            const hit = WindowShell.cursorIn(root, Math.round(zx), Math.round(zy), Math.round(zw), Math.round(zh))
            const busy = islandMenu.opened || seek.dragging || root._dragging || root._moving
            root._inside = hit > 0 || busy
            if (hit === 1 && !busy) {
                root._dwell += 1
                // ~100–200 мс у края или над островком: случайный пролёт мыши к вкладкам браузера не раскрывает
                if (root._dwell >= (idle ? 2 : 1))
                    root._hover()
            } else if (hit === 0 && !busy) {
                root._dwell = 0
                if (root._open || root._peek || root._wings)
                    leave.start()
            }
            if (root._inside)
                leave.stop()
        }
    }
    // Короткая пауза — прощает выскочивший на пару пикселей курсор, но не держит островок открытым
    Timer { id: leave; interval: 120; onTriggered: root._collapse() }

    // Наведение: на Mac — крылья у выреза (панель — по клику), иначе — сразу раскрытие
    function _hover() {
        if (!mac || !Player.hasTrack || _open)
            return _expand()
        leave.stop()
        _wings = true
        _announce = false
    }
    function _expand() {
        leave.stop()
        if (!Player.hasTrack) {
            _peek = true
            return
        }
        if (_open)
            return
        _open = true
        _announce = false
        // подсказка с клавишами — первые три раскрытия
        if (Settings.floatingKeyHints < 3) {
            Settings.floatingKeyHints = Settings.floatingKeyHints + 1
            _keys = true
            keysTimer.restart()
        }
    }
    function _collapse() {
        _open = false
        _wings = false
        _peek = false
        _keys = false
    }
    Timer { id: keysTimer; interval: 2800; onTriggered: root._keys = false }
    Timer { id: announceEnd; interval: 2500; onTriggered: root._announce = false }
    Timer { id: volTimer; interval: 1200; onTriggered: root._vol = false }

    // Приложение на весь экран или развёрнутое окно на мониторе островка — прячемся по настройкам (Windows; раз в 1,5 с).
    // На KDE Wayland это делает скрипт KWin (kwin.rs)
    Timer {
        interval: 1500
        repeat: true
        running: Qt.platform.os === "windows" && root._ready
        triggeredOnStart: true
        onTriggered: root._fullscreen = (Settings.floatingHideFullscreen && WindowShell.fullscreenOn(root))
            || (Settings.floatingHideMaximized && WindowShell.maximizedOn(root))
    }

    // ---- Смена трека: баннер, шторка текста, «танец» обложки ----
    property string _lastUri: Player.uri
    property string _prevTitle: Player.title
    property string _prevArtist: Player.artist
    property real _textSlideX: 0
    property real _textSlideOpacity: 1
    property string _danceShape: ""
    Connections {
        target: Player
        function onTrackChanged() {
            if (Player.uri === root._lastUri)
                return
            const first = root._lastUri === ""
            const oldTitle = root._prevTitle
            const oldArtist = root._prevArtist
            root._lastUri = Player.uri
            root._prevTitle = Player.title
            root._prevArtist = Player.artist
            if (first || !Player.hasTrack)
                return
            if (!root._open) {
                root._announce = true
                announceEnd.restart()
            }
            if (!root.motion)
                return
            curtain.trigger(oldTitle, oldArtist)
            textSlide.restart()
            coverDance.restart()
        }
    }

    // ---- Клавиши без фокуса и колесо ----
    function floatingKey(code) {
        switch (Number(code)) {
        case 1: Player.previous(); break
        case 2: Player.next(); break
        case 3: nudgeVolume(0.05); break
        case 4: nudgeVolume(-0.05); break
        case 5: Player.togglePlay(); break
        }
    }
    function nudgeVolume(d) {
        if (Player.remote === true && Player.canSetVolume === false)
            return
        Player.volume = Math.max(0, Math.min(1, Player.volume + d))
        if (!_open && Player.hasTrack) {
            _vol = true
            volTimer.restart()
        }
        cmpVolume.show()
        expVolume.show()
    }

    // ---- Маска окна: ровно фигура островка; сторона у кромки экрана прямая (скругление за краем окна отрезано) ----
    readonly property string _maskKey: [Math.round(island.x), Math.round(island.y), Math.round(island.width), Math.round(island.height),
        Math.round(island.radius), keysPopover.visible, mode === "idle", edge, width, height].join()
    on_MaskKeyChanged: Qt.callLater(_applyMask)
    function _applyMask() {
        let x = island.x - (mac && mode !== "idle" ? ear : 0)
        let y = island.y
        let w = island.width + (mac && mode !== "idle" ? 2 * ear : 0)
        let h = island.height
        let r = Math.round(island.radius)
        if (keysPopover.visible) {
            // с подсказкой — прямоугольник вокруг островка и неё
            const x2 = Math.max(x + w, keysPopover.x + keysPopover.width)
            const y2 = Math.max(y + h, keysPopover.y + keysPopover.height)
            x = Math.min(x, keysPopover.x)
            y = Math.min(y, keysPopover.y)
            w = x2 - x
            h = y2 - y
            r = 0
        } else if (edge === "top") {
            y -= r
            h += r
        } else if (edge === "bottom") {
            h += r
        } else if (edge === "left") {
            x -= r
            w += r
        } else if (edge === "right") {
            w += r
        }
        WindowShell.setMaskRounded(root, Math.floor(x), Math.floor(y), Math.max(1, Math.ceil(w)), Math.max(1, Math.ceil(h)), r)
    }

    // ================= Тело островка =================
    // Островок и его фон — источник снимка для шторки смены темы (окно постоянного размера: снимок не тянется)
    Item {
        id: stage
        anchors.fill: parent
        opacity: root._settling ? 0 : 1

        Item {
            id: island
            // прижат к краю окна, к которому окно прижато на экране. Не anchors: при смене края якоря снимаются и
            // ставятся не разом, и островок оставался растянутым до следующего наведения
            x: root.edge === "left" ? 0 : root.edge === "right" ? parent.width - width : Math.round((parent.width - width) / 2)
            y: root.edge === "bottom" ? parent.height - height : root.vertical ? Math.round((parent.height - height) / 2) : 0
            width: root.targetW
            height: root.targetH
            readonly property real radius: root.mode === "idle" ? (root.free ? 5 : 3)
                : root.mode === "expanded" ? 30
                : root.mode === "announcement" || root.mode === "peek" ? 22
                : root.mode === "wings" ? 14
                : 18
            // скруглены только углы не у кромки экрана; свободный — все
            readonly property real radiusTL: root.edge === "top" || root.edge === "left" ? 0 : radius
            readonly property real radiusTR: root.edge === "top" || root.edge === "right" ? 0 : radius
            readonly property real radiusBL: root.edge === "bottom" || root.edge === "left" ? 0 : radius
            readonly property real radiusBR: root.edge === "bottom" || root.edge === "right" ? 0 : radius
            // 0 — компактный, 1 — раскрытый: по нему слои переливаются друг в друга
            readonly property real t: root.vertical ? Math.max(0, Math.min(1, (height - root.sideBaseH) / (root.sideH - root.sideBaseH)))
                : Math.max(0, Math.min(1, (height - root.notchH - 36) / (root.expandedH - root.notchH - 36)))

            // Раскрытие — пружина; сворачивание — быстрое и без отскока: убрал мышь — островок сразу уходит
            Behavior on width {
                id: widthMotion
                enabled: root.motion
                NumberAnimation {
                    id: widthAnim
                    duration: widthMotion.targetValue < island.width ? root.shrinkMs : Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: widthMotion.targetValue < island.width ? root.shrinkCurve : root.growCurve
                }
            }
            Behavior on height {
                id: heightMotion
                enabled: root.motion
                NumberAnimation {
                    id: heightAnim
                    duration: heightMotion.targetValue < island.height ? root.shrinkMs : Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: heightMotion.targetValue < island.height ? root.shrinkCurve : root.growCurve
                }
            }

            Rectangle {
                id: bg
                anchors.fill: parent
                topLeftRadius: island.radiusTL
                topRightRadius: island.radiusTR
                bottomLeftRadius: island.radiusBL
                bottomRightRadius: island.radiusBR
                antialiasing: true
                clip: true
                color: root.mode === "idle" ? (root.colorMode ? Theme.primary : "#101114") : root.body
                // Mac: в покое чёрное тело сливается с вырезом — крылья видно, как они в него уезжают
                border.width: root.mode === "idle" && !root.mac ? 1 : 0
                border.color: Qt.rgba(1, 1, 1, 0.28)

                // Цветной: обложка размыта под тёмным стеклом и «плавает», пока играет (настройки фона плеера). clip режет
                // по прямоугольнику — по фигуре островка обрезает маска
                Item {
                    id: coverFill
                    anchors.fill: parent
                    visible: root._onCover && root.mode !== "idle"
                    layer.enabled: visible
                    layer.effect: MultiEffect {
                        maskEnabled: true
                        maskSource: coverMask
                        maskThresholdMin: 0.5
                        maskSpreadAtMin: 1.0
                    }
                    // с двойным буфером: без провала в цвет, пока грузится новая обложка
                    TrackBackdrop {
                        anchors.fill: parent
                        source: Player.cover
                        mode: Settings.backdropMode
                        blur: Settings.nowPlayingBlur
                        flow: Settings.nowPlayingDrift
                        running: root.mode !== "idle" && root.motion && Player.playing
                        // меняется вместе с цветами — под шторкой islandWipe, как фон окна приложения
                        followTheme: Settings.accentFromCover
                    }
                    Rectangle {
                        anchors.fill: parent
                        color: Qt.rgba(0, 0, 0, 0.42)
                    }
                }
                Rectangle {
                    id: coverMask
                    anchors.fill: parent
                    visible: false
                    layer.enabled: coverFill.visible
                    topLeftRadius: island.radiusTL
                    topRightRadius: island.radiusTR
                    bottomLeftRadius: island.radiusBL
                    bottomRightRadius: island.radiusBR
                }
            }

            // «Плечи»: вогнутые скругления у кромки экрана рядом с вырезом (Mac)
            component Ear: Item {
                id: earItem
                property bool mirrored: false
                width: root.ear
                height: root.ear
                visible: root.mac && root.mode !== "idle"
                clip: true
                Rectangle {
                    readonly property real r: root.ear + 30
                    x: earItem.mirrored ? root.ear - r : -r
                    y: root.ear - r
                    width: 2 * r
                    height: 2 * r
                    radius: r
                    color: "transparent"
                    border.width: 30
                    border.color: root.body
                    antialiasing: true
                }
            }
            Ear { x: -root.ear }
            Ear { x: island.width; mirrored: true }

            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
                    root.nudgeVolume(touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05)
                }
            }
            // Тянем островок: у края — вдоль него, свободный — куда угодно; окно идёт за курсором сразу. KDE Wayland: окно
            // само себя не двигает — тащит KWin (startSystemMove), его скрипт держит ось (kwin.rs). Иначе двигаем сами,
            // как TopBar: курсор сдвинулся от точки захвата — сдвигаем окно на столько же
            DragHandler {
                id: slide
                target: null
                acceptedButtons: Qt.LeftButton
                enabled: !root.mac && root._screen !== null
                onActiveChanged: {
                    if (active && root.kwin) {
                        root._moving = root.startSystemMove()
                        if (root._moving)
                            moveWait.restart()
                    } else if (active) {
                        root._dragX = root.x
                        root._dragY = root.y
                        root._dragging = true
                    } else if (root._dragging) {
                        root._drop(root._dragX, root._dragY)
                        root._dragging = false
                    }
                }
                onCentroidChanged: {
                    if (!active || !root._dragging)
                        return
                    const s = root._screen
                    const lim = (v, lo, hi) => Math.max(lo, Math.min(hi, v))
                    if (root._moveX)
                        root._dragX = lim(root._dragX + centroid.scenePosition.x - centroid.scenePressPosition.x, s.virtualX, s.virtualX + s.width - root.width)
                    if (root._moveY)
                        root._dragY = lim(root._dragY + centroid.scenePosition.y - centroid.scenePressPosition.y, s.virtualY, s.virtualY + s.height - root.height)
                }
            }
            TapHandler {
                acceptedButtons: Qt.LeftButton
                onTapped: point => {
                    if (root.mode === "peek")
                        WindowShell.raiseMain()
                    else if (root.mode === "wings") {
                        // клик по самому вырезу; кнопки на крыльях — свои
                        if (point.position.x > root.wingW && point.position.x < island.width - root.wingW)
                            root._expand()
                    } else if (root.mode !== "expanded")
                        root._expand()
                }
            }
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: point => islandMenu.openAt(island.mapToItem(root.contentItem, point.position.x, point.position.y))
            }

            // ================= Компактный слой: compact / volume / announcement / peek =================
            Item {
                id: compactLayer
                x: 0
                y: root.mac && root.mode !== "compact" ? root.notchH : 0
                width: parent.width
                height: parent.height - y
                visible: root.mode !== "idle" && root.mode !== "wings" && island.t < 0.7
                opacity: Math.max(0, 1 - island.t * 1.6)

                // обложка слева, сбоку — сверху (в покое — значок приложения)
                Item {
                    id: cmpCover
                    x: root.vertical ? Math.round((parent.width - width) / 2) : root.mode === "announcement" || root.mode === "peek" ? 10 : 9
                    y: root.vertical ? (root.mode === "announcement" || root.mode === "peek" ? 14 : 7) : Math.round((parent.height - height) / 2)
                    width: root.mode === "announcement" ? 34 : root.mode === "peek" ? 28 : 22
                    height: width
                    Behavior on width { enabled: root.motion; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }

                    MorphImage {
                        id: cmpCoverImg
                        anchors.fill: parent
                        visible: Player.hasTrack && Player.cover !== ""
                        source: Player.cover
                        eager: true
                        fallbackKind: Player.album !== "" ? "album" : "track"
                        fallbackArtist: Player.artist.split(", ")[0]
                        fallbackTitle: Player.album !== "" ? Player.album : Player.title
                        shape: root._danceShape !== "" ? root._danceShape : Player.playing ? "cookie12" : "softSquare"
                        pulse: root.motion && Player.playing && Settings.coverPulse ? Math.min(0.24, Player.level * 0.35) : 0
                        MaskSpin { target: cmpCoverImg; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true && root.motion }
                    }
                    MorphShape {
                        anchors.fill: parent
                        visible: !cmpCoverImg.visible
                        shape: "softSquare"
                        color: Qt.alpha(root.ink, 0.14)
                        Icon {
                            anchors.centerIn: parent
                            name: !Player.hasTrack ? "music_note" : Player.isEpisode ? "podcasts" : "music_note"
                            size: Math.round(parent.width * 0.58)
                            color: root.inkDim
                        }
                    }
                    CoverVolume {
                        id: cmpVolume
                        anchors.fill: parent
                        radius: Player.playing ? width / 2 : width * 0.3
                        accent: root.accent
                    }
                }

                // баннер смены трека и peek: две строки текста (сбоку — под обложкой, по центру)
                Item {
                    id: textBox
                    x: root.vertical ? 12 : cmpCover.x + cmpCover.width + 10
                    y: root.vertical ? cmpCover.y + cmpCover.height + 8 : Math.round((parent.height - height) / 2)
                    width: root.vertical ? parent.width - 24 : (root.mode === "peek" ? parent.width - 14 : cmpBars.x - 8) - x
                    height: textCol.height
                    visible: root.mode === "announcement" || root.mode === "peek"
                    clip: true
                    Column {
                        id: textCol
                        width: parent.width
                        spacing: 1
                        transform: Translate { x: root.mode === "announcement" ? root._textSlideX : 0 }
                        opacity: root.mode === "announcement" ? root._textSlideOpacity : 1
                        Label {
                            width: parent.width
                            text: Player.hasTrack ? Player.title : I18n.t("Nothing playing")
                            type: "labelLarge"
                            weight: 600
                            color: root.ink
                            elide: Text.ElideRight
                            horizontalAlignment: root.vertical ? Text.AlignHCenter : Text.AlignLeft
                        }
                        Label {
                            width: parent.width
                            horizontalAlignment: root.vertical ? Text.AlignHCenter : Text.AlignLeft
                            text: !Player.hasTrack ? I18n.t("Click to open Materialfy")
                                : Player.errorText !== "" ? Player.errorText : Player.artist
                            type: "labelMedium"
                            weight: 400
                            color: Player.hasTrack && Player.errorText !== "" ? Theme.error : root.inkDim
                            elide: Text.ElideRight
                        }
                    }
                }

                // громкость: проценты по центру (сбоку узко — без значка, уровень и так на обложке)
                Row {
                    anchors.centerIn: parent
                    visible: root.mode === "volume"
                    spacing: 6
                    Icon {
                        visible: !root.vertical
                        anchors.verticalCenter: parent.verticalCenter
                        name: Player.volume <= 0 ? "volume_off" : Player.volume < 0.4 ? "volume_down" : "volume_up"
                        size: 16
                        fill: 1
                        color: root.accent
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: Math.round(Player.volume * 100) + "%"
                        type: "labelLarge"
                        weight: 600
                        font.features: { "tnum": 1 }
                        color: root.ink
                    }
                }

                // полоски уровня справа (сбоку — снизу), на паузе — значок паузы
                Item {
                    id: cmpBars
                    x: root.vertical ? Math.round((parent.width - width) / 2) : parent.width - width - 12
                    y: root.vertical ? parent.height - height - 10 : Math.round((parent.height - height) / 2)
                    width: 22
                    height: 16
                    visible: root.mode === "compact" || root.mode === "volume" || (root.mode === "announcement" && !root.vertical)
                    Bars {
                        anchors.fill: parent
                        visible: Player.playing
                    }
                    Icon {
                        anchors.centerIn: parent
                        name: "pause"
                        size: 16
                        fill: 1
                        color: root.accent
                        visible: !Player.playing
                    }
                }

                // шторка: старое название уезжает за лучом от обложки
                AnnouncementCurtain {
                    id: curtain
                    anchors.fill: parent
                    // старое название — справа от обложки; сбоку текст под ней, хватает сдвига текста
                    visible: root.mode === "announcement" && running && !root.vertical
                    cmpCover: cmpCover
                    motion: root.motion
                    ink: root.ink
                    inkDim: root.inkDim
                    accent: root.accent
                    colorMode: root.colorMode
                }
            }

            // ================= Крылья у выреза (Mac): слева обложка и лайк, справа управление =================
            Item {
                id: wingsLayer
                anchors.fill: parent
                clip: true
                visible: opacity > 0.01
                opacity: root.mode === "wings" ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }

                Row {
                    x: 12
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 6
                    transform: Translate { x: root.mode === "wings" ? 0 : 24; Behavior on x { enabled: root.motion; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } } }
                    MorphImage {
                        id: wingCover
                        anchors.verticalCenter: parent.verticalCenter
                        width: 24
                        height: 24
                        source: Player.cover
                        eager: true
                        fallbackKind: Player.album !== "" ? "album" : "track"
                        fallbackArtist: Player.artist.split(", ")[0]
                        fallbackTitle: Player.album !== "" ? Player.album : Player.title
                        shape: Player.playing ? "cookie12" : "softSquare"
                        pulse: root.motion && Player.playing && Settings.coverPulse ? Math.min(0.24, Player.level * 0.35) : 0
                        TapHandler { onTapped: WindowShell.raiseMain() }
                    }
                    Bars {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 22
                        height: 14
                        opacity: Player.playing ? 1 : 0.3
                    }
                    IslandBtn {
                        readonly property string trackId: Player.track && Player.track.trackId ? String(Player.track.trackId) : ""
                        readonly property bool liked: Library.likesRevision >= 0 && trackId !== "" && Library.isLiked(trackId)
                        anchors.verticalCenter: parent.verticalCenter
                        visible: Player.isEpisode !== true
                        size: 30
                        iconSize: 18
                        icon: "favorite"
                        filled: liked
                        tint: liked ? root.accent : root.inkDim
                        onClicked: Library.toggleLike(Player.track)
                    }
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    transform: Translate { x: root.mode === "wings" ? 0 : -24; Behavior on x { enabled: root.motion; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } } }
                    IslandBtn { size: 30; iconSize: 20; icon: "skip_previous"; filled: true; onClicked: Player.previous() }
                    IslandBtn { size: 30; iconSize: 22; icon: Player.playing ? "pause" : "play_arrow"; filled: true; tint: root.accent; onClicked: Player.togglePlay() }
                    IslandBtn { size: 30; iconSize: 20; icon: "skip_next"; filled: true; onClicked: Player.next() }
                }
            }

            // ================= Раскрытый слой =================
            Item {
                id: expLayer
                anchors.fill: parent
                visible: island.t > 0.2
                opacity: Math.max(0, Math.min(1, (island.t - 0.2) / 0.65))
                clip: true
                readonly property int topY: root.notchH + 14

                // сбоку: обложка по центру, кнопки по бокам от неё, текст под ней
                Item {
                    id: headerRow
                    x: root.vertical ? 12 : 18
                    y: expLayer.topY
                    width: parent.width - 2 * x
                    height: expCover.height

                    Item {
                        id: expCover
                        x: root.vertical ? Math.round((parent.width - width) / 2) : 0
                        width: root.vertical ? 64 : 52
                        height: width
                        MorphImage {
                            id: expCoverImg
                            anchors.fill: parent
                            visible: Player.cover !== ""
                            source: Player.coverLarge !== "" ? Player.coverLarge : Player.cover
                            fallbackKind: Player.album !== "" ? "album" : "track"
                            fallbackArtist: Player.artist.split(", ")[0]
                            fallbackTitle: Player.album !== "" ? Player.album : Player.title
                            shape: root._danceShape !== "" ? root._danceShape : Player.playing ? "cookie12" : "softSquare"
                            pulse: root.motion && Player.playing && Settings.coverPulse ? Math.min(0.24, Player.level * 0.35) : 0
                            MaskSpin { target: expCoverImg; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true && root.motion }
                        }
                        MorphShape {
                            anchors.fill: parent
                            visible: Player.cover === ""
                            shape: "softSquare"
                            color: Qt.alpha(root.ink, 0.14)
                            Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 28; color: root.inkDim }
                        }
                        CoverVolume {
                            id: expVolume
                            anchors.fill: parent
                            radius: Player.playing ? width / 2 : width * 0.3
                            accent: root.accent
                        }
                        TapHandler { onTapped: WindowShell.raiseMain() }
                    }

                    Column {
                        x: root.vertical ? 0 : expCover.width + 12
                        y: root.vertical ? parent.height + 8 : Math.round((parent.height - height) / 2)
                        width: root.vertical ? parent.width : sideButtons.x - 8 - x
                        spacing: 2
                        clip: true
                        transform: Translate { x: root._textSlideX }
                        opacity: root._textSlideOpacity
                        Label {
                            width: parent.width
                            text: Player.title
                            type: "titleSmall"
                            color: root.ink
                            elide: Text.ElideRight
                            horizontalAlignment: root.vertical ? Text.AlignHCenter : Text.AlignLeft
                        }
                        Label {
                            width: parent.width
                            horizontalAlignment: root.vertical ? Text.AlignHCenter : Text.AlignLeft
                            text: Player.errorText !== "" ? Player.errorText : Player.artist
                            type: "bodySmall"
                            color: Player.errorText !== "" ? Theme.error : root.inkDim
                            elide: Text.ElideRight
                        }
                        TapHandler { onTapped: WindowShell.raiseMain() }
                    }

                    // сбоку кнопки расходятся по обе стороны обложки: промежуток — её ширина
                    Row {
                        id: sideButtons
                        x: root.vertical ? Math.round((parent.width - width) / 2) : parent.width - width
                        y: Math.round((parent.height - height) / 2)
                        spacing: root.vertical ? expCover.width + 16 : 2
                        IslandBtn {
                            readonly property string trackId: Player.track && Player.track.trackId ? String(Player.track.trackId) : ""
                            readonly property bool liked: Library.likesRevision >= 0 && trackId !== "" && Library.isLiked(trackId)
                            // сбоку место держим и у подкаста, иначе вторая кнопка уедет под обложку
                            visible: root.vertical || Player.isEpisode !== true
                            opacity: Player.isEpisode === true ? 0 : 1
                            enabled: Player.isEpisode !== true
                            icon: "favorite"
                            filled: liked
                            tint: liked ? root.accent : root.inkDim
                            onClicked: Library.toggleLike(Player.track)
                        }
                        IslandBtn {
                            icon: "open_in_new"
                            tint: root.inkDim
                            onClicked: WindowShell.raiseMain()
                        }
                    }
                }

                // время — волна — время
                PlayerProgress { id: progress; active: expLayer.visible && !seek.dragging }
                Item {
                    x: headerRow.x
                    y: expLayer.topY + (root.vertical ? 118 : 60)
                    width: headerRow.width
                    height: 16
                    Label {
                        id: elapsed
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        width: 34
                        text: root._time(seek.dragging ? seek.dragValue * Player.durationMs : Player.positionMs)
                        type: "labelSmall"
                        font.features: { "tnum": 1 }
                        color: root.inkDim
                    }
                    WavyProgress {
                        id: seek
                        anchors.left: elapsed.right
                        anchors.right: remaining.left
                        anchors.leftMargin: 6
                        anchors.rightMargin: 6
                        anchors.verticalCenter: parent.verticalCenter
                        height: 14
                        thickness: 3
                        amplitude: Player.playing && root.motion ? 2.5 : 0
                        wavelength: 24
                        value: progress.value
                        wavy: Player.playing && root.motion
                        interactive: true
                        activeColor: root.accent
                        stopColor: activeColor
                        trackColor: Qt.alpha(root.ink, 0.2)
                        onCommitted: v => Player.seek(v)
                    }
                    Label {
                        id: remaining
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        width: 34
                        horizontalAlignment: Text.AlignRight
                        text: root._time(Player.durationMs)
                        type: "labelSmall"
                        font.features: { "tnum": 1 }
                        color: root.inkDim
                    }
                }

                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: expLayer.topY + (root.vertical ? 144 : 84)
                    spacing: 14
                    IslandBtn {
                        anchors.verticalCenter: parent.verticalCenter
                        size: 40
                        icon: "skip_previous"
                        iconSize: 26
                        filled: true
                        onClicked: Player.previous()
                    }
                    PlayButton {
                        anchors.verticalCenter: parent.verticalCenter
                        size: 46
                        playing: Player.playing
                        loading: Player.buffering
                        containerColor: root.accent
                        iconColor: root.colorMode ? root.body : Theme.fgPrimaryFixed
                        spin: root.motion && Player.isLocal === true
                        onClicked: Player.togglePlay()
                    }
                    IslandBtn {
                        anchors.verticalCenter: parent.verticalCenter
                        size: 40
                        icon: "skip_next"
                        iconSize: 26
                        filled: true
                        onClicked: Player.next()
                    }
                }
            }
        }
    }

    // Смена цветов под новую обложку — той же шторкой, что и в окне приложения, и одновременно с ней
    ThemeWipe {
        id: islandWipe
        anchors.fill: parent
        source: stage
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: root.mode === "expanded" ? expCover : cmpCover
        Component.onCompleted: Theme.addWipe(islandWipe)
        Component.onDestruction: Theme.removeWipe(islandWipe)
    }

    // ================= Подсказка с клавишами (первые три раскрытия) =================
    Rectangle {
        id: keysPopover
        // под островком (снизу — над ним), сбоку — у того же края
        x: root.edge === "left" ? 8 : root.edge === "right" ? parent.width - width - 8 : Math.round((parent.width - width) / 2)
        y: root.edge === "bottom" ? island.y - height - 8 : island.y + island.height + 8
        width: keysRow.implicitWidth + 24
        height: 32
        radius: 16
        color: Qt.rgba(0.055, 0.06, 0.086, 0.94)
        border.width: 1
        border.color: Qt.rgba(1, 1, 1, 0.09)
        visible: opacity > 0.01
        opacity: root.mode === "expanded" && root._keys && island.t > 0.95 ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        onVisibleChanged: Qt.callLater(root._applyMask)

        Row {
            id: keysRow
            anchors.centerIn: parent
            spacing: 6
            KbdKey { text: "←" }
            KbdKey { text: "→" }
            Label { text: I18n.t("tracks"); type: "labelSmall"; color: Qt.rgba(1, 1, 1, 0.72); anchors.verticalCenter: parent.verticalCenter }
            Label { text: "·"; color: Qt.rgba(1, 1, 1, 0.35); anchors.verticalCenter: parent.verticalCenter }
            KbdKey { text: "↑" }
            KbdKey { text: "↓" }
            Label { text: I18n.t("volume"); type: "labelSmall"; color: Qt.rgba(1, 1, 1, 0.72); anchors.verticalCenter: parent.verticalCenter }
            Label { text: "·"; color: Qt.rgba(1, 1, 1, 0.35); anchors.verticalCenter: parent.verticalCenter }
            KbdKey { text: I18n.t("Space") }
            Label { text: I18n.t("pause"); type: "labelSmall"; color: Qt.rgba(1, 1, 1, 0.72); anchors.verticalCenter: parent.verticalCenter }
        }
    }

    // ================= Анимации смены трека =================
    ParallelAnimation {
        id: textSlide
        NumberAnimation { target: root; property: "_textSlideX"; from: 24; to: 0; duration: 460; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
        NumberAnimation { target: root; property: "_textSlideOpacity"; from: 0; to: 1; duration: 320; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
    }
    // обложка: «цветок» → «печенье» с перелётом → обычная фигура
    SequentialAnimation {
        id: coverDance
        PropertyAction { target: root; property: "_danceShape"; value: "flower8" }
        NumberAnimation { targets: [expCover, cmpCover]; property: "scale"; from: 1; to: 0.88; duration: 130; easing.type: Easing.InQuad }
        PauseAnimation { duration: 200 }
        PropertyAction { target: root; property: "_danceShape"; value: "cookie12" }
        NumberAnimation { targets: [expCover, cmpCover]; property: "scale"; to: 1.08; duration: 260; easing.type: Easing.OutBack; easing.overshoot: 1.35 }
        PauseAnimation { duration: 260 }
        NumberAnimation { targets: [expCover, cmpCover]; property: "scale"; to: 1; duration: 220; easing.type: Easing.OutQuad }
        PropertyAction { target: root; property: "_danceShape"; value: "" }
    }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    FloatingMenu {
        id: islandMenu
        anchors.fill: parent
        mainWindow: root.mainWindow
    }

    component IslandBtn: Item {
        id: ibtn
        property string icon
        property int iconSize: 20
        property bool filled: false
        property color tint: root.ink
        property real size: 34
        signal clicked()
        width: size
        height: size
        scale: root.motion && tap.pressed ? 0.9 : 1
        Behavior on scale { NumberAnimation { duration: Theme.motion.effectsFast } }
        Rectangle {
            anchors.fill: parent
            radius: tap.pressed ? ibtn.size * 0.3 : ibtn.size / 2
            color: Qt.alpha(root.ink, hover.hovered ? 0.12 : 0)
            Behavior on radius { NumberAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.centerIn: parent
            name: ibtn.icon
            size: ibtn.iconSize
            fill: ibtn.filled ? 1 : 0
            color: ibtn.tint
        }
        HoverHandler { id: hover }
        TapHandler { id: tap; onTapped: ibtn.clicked() }
    }

    component KbdKey: Rectangle {
        property alias text: klbl.text
        width: Math.max(20, klbl.implicitWidth + 10)
        height: 20
        radius: 6
        anchors.verticalCenter: parent.verticalCenter
        color: "#2a2c36"
        border.width: 1
        border.color: Qt.rgba(1, 1, 1, 0.15)
        Label {
            id: klbl
            anchors.centerIn: parent
            type: "labelSmall"
            weight: 600
            color: "#ffffff"
        }
    }

    // Полоски уровня: настоящий уровень звука (своё воспроизведение) или ровное движение (другое устройство)
    component Bars: Item {
        id: bars
        property var levels: [0.45, 0.8, 0.6, 0.35]
        property var _history: []
        property real _peak: 0.05
        property real _phase: 0
        Connections {
            target: Player
            enabled: Player.playing && Player.isLocal === true
            function onLevelChanged() {
                const h = [Player.level].concat(bars._history).slice(0, 10)
                bars._history = h
                bars._peak = Math.max(0.05, bars._peak * 0.992, Player.level)
                const gain = [0.85, 1, 0.9, 0.75]
                bars.levels = gain.map((g, i) => Math.max(0.2, Math.min(1, (h[i * 3] ?? 0) / bars._peak * g)))
            }
        }
        TickAnimation {
            running: bars.visible && Player.playing && Player.isLocal !== true && root.motion
            onTriggered: {
                bars._phase += frameTime
                bars.levels = [0, 1, 2, 3].map(i => 0.25 + 0.75 * Math.abs(Math.sin(bars._phase * (2.1 + i * 0.7) + i * 1.3)))
            }
        }
        Repeater {
            model: 4
            Rectangle {
                required property int index
                x: index * 5.5
                width: 3
                radius: 1.5
                height: Math.max(3, bars.height * bars.levels[index])
                anchors.verticalCenter: parent.verticalCenter
                color: root.accent
                Behavior on height { enabled: root.motion; NumberAnimation { duration: 90; easing.type: Easing.OutQuad } }
            }
        }
    }
}
