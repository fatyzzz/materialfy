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
    readonly property bool wantsLevels: ((visible && open) || (Window.window && Window.window.layoutMode === "mini"))
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
        open = false
    }

    visible: reveal > 0
    opacity: Theme.reducedMotion ? reveal : 1
    focus: open
    Keys.onEscapePressed: compact && sheetOpen ? sheetOpen = false : hide()

    Behavior on reveal {
        NumberAnimation {
            id: revealAnim
            // программный рендер — сразу: кадры раскрытия (два слоя и маска на весь экран) рисует процессор
            duration: Theme.lowPower ? 0 : Theme.reducedMotion ? Theme.motion.effectsDefault : root.open ? 820 : 620
            easing.type: Easing.Linear     // кривые — у каждой фазы свои, см. blob
            // onFinished здесь не приходит (анимация внутри Behavior сигнал не шлёт).
            // Linear заканчивается ровно на to — снап не нужен.
        }
    }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
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
            rotation: 240 * e1 + 50 * e2
            color: "white"
            tinted: false
        }
    }

    // Содержимое (в самом конце сворачивания гаснет — без «обрубка» в крошечной фигуре)
    Item {
        id: content
        anchors.fill: parent
        opacity: Math.min(1, root.reveal / 0.12)
        layer.enabled: root.reveal < 1
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: revealMask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1.0
        }

        // Фон: размытая обложка + вуаль цвета темы
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

        // Блокирует клики по приложению под плеером
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: wheel => wheel.accepted = true }

        // Верхняя панель: свернуть + переключатель очередь/текст (над правой колонкой)
        RowLayout {
            id: topRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 16
            spacing: 12
            IconButton { icon: "keyboard_arrow_down"; style: "tonal"; onClicked: root.hide() }
            Item { Layout.fillWidth: true }
            DeviceButton { iconOnly: root.layoutCompact; usePanel: false }   // правая панель — под этим экраном
            ButtonGroup {
                model: [{ icon: "title" }, { icon: "queue_music" }]
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
                Layout.alignment: Qt.AlignVCenter
                Layout.leftMargin: 4
                window: root.Window.window
            }

            DragHandler {
                property bool manual: false
                target: null
                grabPermissions: PointerHandler.TakeOverForbidden
                onActiveChanged: if (active) manual = !root.Window.window.startSystemMove()
                onCentroidChanged: if (active && manual && root.Window.window.visibility === Window.Windowed) {
                    root.Window.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
                    root.Window.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
                }
            }
            TapHandler {
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

            // ---- Колонка плеера (слева, с отступом от края): обложка и управление ----
            ColumnLayout {
                id: left
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
                readonly property real colWidth: left.width

                Item { Layout.fillHeight: true }

                // «Дыхание» в такт: уровень звука плеера (просит Main.qml по root.wantsLevels); доля над
                // скользящим средним — удар
                property real avgLevel: 0
                property real beat: 0
                Behavior on beat { NumberAnimation { duration: 110; easing.type: Easing.OutQuad } }
                Connections {
                    target: Player
                    function onLevelChanged() {
                        left.avgLevel += (Player.level - left.avgLevel) * 0.06
                        const b = Math.max(0, Math.min(1, (Player.level - left.avgLevel) * 4))
                        // пишем только значимые сдвиги: каждый старт Behavior перерисовывает всё окно,
                        // а события уровня приходят 30–60 раз в секунду
                        if (Math.abs(b - left.beat) > 0.04)
                            left.beat = b
                    }
                }
                Connections {
                    target: Player
                    function onStateChanged() { if (!Player.playing) left.beat = 0 }
                }

                NowPlayingCover {
                    id: bigCover
                    // Всё остальное в колонке занимает ~360 px (в узком окне ~310) — обложка берёт оставшееся
                    // плашка пульта (если видна) тоже забирает высоту
                    // Центр-режим: панель справа убрана, колонка шире — обложке даём вырасти по ширине;
                    // вертикальный бюджет тот же (текст и управление остаются)
                    // Размер интерполируется blend'ом (0 — обычный, 1 — центр-режим): ресайз окна мгновенен
                    property real _max: root.coverOnly ? 520 : 440
                    Behavior on _max { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
                    readonly property real s: root.layoutCompact ? Math.max(96, Math.min(left.colWidth, body.height - 316 - npBanner.implicitHeight, 360))
                                                           : Math.max(160, Math.min(left.colWidth, body.height - 360 - npBanner.implicitHeight, _max))
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: s
                    Layout.preferredHeight: s
                    Layout.bottomMargin: root.layoutCompact ? 12 : 20
                    source: Player.coverLarge !== "" ? Player.coverLarge : Player.cover
                    restShape: Player.playing ? "cookie12" : "square"
                    direction: Player.direction
                    pulse: Settings.coverPulse && Player.isLocal === true ? left.beat : 0
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
                        if (root.visible && left._lastId !== Player.trackId) {
                            left.infoIn = 0
                            infoAnim.restart()
                        }
                        left._lastId = Player.trackId
                    }
                }
                property string _lastId: ""
                TapHandler {
                    enabled: Player.hasTrack
                    acceptedButtons: Qt.RightButton
                    onTapped: (point) => {
                        const p = left.mapToItem(null, point.position.x, point.position.y)
                        Catalog.requestTrackMenu(Player.track, p.x, p.y)
                    }
                }
                NumberAnimation {
                    id: infoAnim
                    target: left
                    property: "infoIn"
                    to: 1
                    duration: 520
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }

                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: Player.title
                    type: root.layoutCompact ? "headlineSmall" : "headlineMedium"
                    weight: 700
                    opacity: left.infoIn
                    transform: Translate { y: (1 - left.infoIn) * 14 }
                }
                LinkLabel {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.maximumWidth: left.colWidth
                    Layout.topMargin: -6
                    horizontalAlignment: Text.AlignHCenter
                    // у выпуска подкаста под названием — подкаст: ссылка на его страницу
                    linkEnabled: Player.isEpisode ? (Player.track.showId ?? "") !== "" : (Player.track.artistRefs ?? []).length > 0
                    previewArtistId: !Player.isEpisode && (Player.track.artistRefs ?? []).length === 1 ? Player.track.artistRefs[0].id : ""
                    onClicked: Player.isEpisode ? Catalog.openShow(Player.track.showId ?? "") : Catalog.openArtists(Player.track.artistRefs ?? [])
                    opacity: Math.max(0, left.infoIn * 1.3 - 0.3)    // чуть позже названия
                    transform: Translate { y: (1 - left.infoIn) * 18 }
                    text: Player.artist
                    type: "titleMedium"
                    color: Theme.fgSurfaceVariant
                }
                LinkLabel {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.maximumWidth: left.colWidth
                    Layout.topMargin: -6
                    horizontalAlignment: Text.AlignHCenter
                    // у выпуска подкаста и трека без альбома ссылки нет
                    linkEnabled: (Player.track.albumId ?? "") !== ""
                    onClicked: Catalog.openAlbum(Player.track.albumId ?? "")
                    opacity: Math.max(0, left.infoIn * 1.3 - 0.3)    // чуть позже названия
                    transform: Translate { y: (1 - left.infoIn) * 18 }
                    text: Player.album + (Player.codec !== "" && Player.isLocal === true ? "  ·  " + Player.codec : "")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }

                // Прогресс
                readonly property alias progress: playerProgress.value
                PlayerProgress { id: playerProgress; active: bigSeek.visible && !bigSeek.dragging }
                RowLayout {
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
                        value: left.progress
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
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 4
                    spacing: 12
                    IconButton {
                        icon: Player.isEpisode ? "replay_10" : "shuffle"; size: "m"
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
                    IconButton { icon: "skip_previous"; size: "m"; iconColor: Theme.fgSurface; onClicked: Player.previous() }
                    PlayButton {
                        size: root.layoutCompact ? 72 : 96
                        playing: Player.playing
                        loading: Player.buffering
                        spin: Player.isLocal === true
                        containerColor: Theme.primaryContainer
                        iconColor: Theme.fgPrimaryContainer
                        onClicked: Player.togglePlay()
                    }
                    IconButton { icon: "skip_next"; size: "m"; iconColor: Theme.fgSurface; onClicked: Player.next() }
                    IconButton {
                        icon: Player.isEpisode ? "forward_30" : Player.repeat === "one" ? "repeat_one" : "repeat"; size: "m"
                        checkable: !Player.isEpisode; autoToggle: false; checked: !Player.isEpisode && Player.repeat !== "off"
                        style: checked ? "tonal" : "standard"
                        onClicked: Player.isEpisode ? Player.seekRelative(30000) : Player.cycleRepeat()
                    }
                }

                // Не рекомендовать (радио) · устройство · громкость · лайк или закладка выпуска
                RowLayout {
                    id: volumeRow
                    // громкость играющего устройства; у части устройств её меняют только на них самих
                    readonly property bool locked: Player.remote === true && Player.canSetVolume === false
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 4
                    spacing: 12
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
                    Icon {
                        Layout.leftMargin: Player.remote === true ? 0 : 8
                        name: volumeRow.locked ? "lock" : Player.volume > 0.5 ? "volume_up" : Player.volume > 0 ? "volume_down" : "volume_off"
                        size: 22
                    }
                    Slider {
                        Layout.preferredWidth: root.layoutCompact ? Math.min(220, left.colWidth - 260) : 220
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
                    visible: volumeRow.locked
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: -4
                    text: I18n.t("Volume can only be changed on the device")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }

                // Пульт: «Играет на … · Слушать здесь», устройство пропало — «Продолжить здесь?»
                ConnectBanner {
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
                x: root.layoutCompact ? 0 : left._normalX + left._normalW + 48
                width: root.layoutCompact ? parent.width
                    : Math.max(0, parent.width - x - Math.max(16, root.width * 0.06))

                component TabShift: NumberAnimation {
                    duration: 460
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
