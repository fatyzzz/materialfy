import QtQuick
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.platform as Platform
import Md3
import Materialfy.Core
import "app"

// Главное окно: сайдбар | контент (+ мини-плеер) | правая панель.
Window {
    id: win

    property string startPage: "home"     // --set startPage=settings | artist:<id> (для разработки)
    // --set autoPlay=<spotify uri> — включить контекст, как только поднялось воспроизведение; --set openNowPlaying=true —
    // затем раскрыть «Сейчас играет» (для скриншотов документации)
    property string autoPlay: ""
    property bool openNowPlaying: false
    // --set browse=album:<id>,artist:<id>,back,home,panel,sidebar… — one step every 4 s once the session is up:
    // open a page, go back, switch a section, toggle the right panel or the sidebar (memory measurements)
    property string browse: ""
    // --set floatingPlayer=island|taskbar|both|off — вместо настройки (снимки); --set islandState=idle|peek|compact|
    // volume|announcement|expanded — вид островка; --set taskbarEmbed=false — виджет панели задач сразу поверх панели, без встраивания;
    // --set floatingShot=<path.png> — снимок окна островка или виджета (и при --screenshot), --set taskbarTheme=dark|light
    property string floatingPlayer: ""
    property string islandState: ""
    property bool taskbarEmbed: true
    property string floatingShot: ""
    property string taskbarTheme: ""

    // размер и положение — последние сохранённые (см. _restoreGeometry); окно показывается уже на месте
    width: 1280
    height: 800
    // Меньше обычной раскладки окно уходит в «компактную» (сайдбар — шторкой), ещё меньше — в мини-плеер
    // мини-режим: обложка, название и «назад / слушать / вперёд» — уже не помещаются
    minimumWidth: 400
    minimumHeight: 120
    visible: false
    title: Player.hasTrack ? Player.title + " — " + Player.artist + " · Materialfy" : "Materialfy"
    // «Поверх всех окон» (кнопка мини-плеера, меню трека); не запоминается между запусками
    property bool pinnedOnTop: false
    // закрепление — свойство мини-режима: окно, развёрнутое обратно, поверх всех не висит
    onLayoutModeChanged: if (layoutMode !== "mini") pinnedOnTop = false
    // Кадры рисует процессор (программный рендер) — ставит mf_shell.cpp, watch_software_rendering
    property bool softwareRendering: false
    // Wayland: протокола «поверх всех окон» нет (GNOME, Weston флаг игнорируют), и окно не знает своих координат
    readonly property bool wayland: Qt.platform.pluginName.indexOf("wayland") === 0
    readonly property bool canPin: !wayland
    // Плеер вне окна (Settings → Окно → «Плеер поверх окон»): островок — Windows и X11 (на Wayland окно не может
    // встать у края экрана само), панель задач — только Windows
    readonly property string floatingMode: {
        const mode = floatingPlayer !== "" ? floatingPlayer : Settings.floatingPlayer
        if (mode === "both" && Qt.platform.os === "windows")
            return mode
        if (mode === "island" && !wayland && (Qt.platform.os === "windows" || Qt.platform.os === "linux"))
            return mode
        if (mode === "taskbar" && Qt.platform.os === "windows")
            return mode
        return "off"
    }
    flags: Qt.Window | Qt.FramelessWindowHint | (pinnedOnTop ? Qt.WindowStaysOnTopHint : 0)
    // Linux/macOS: окно прозрачное, фон и скруглённые углы рисует WindowFrame (альфа-канал окна
    // включается цветом при создании, поэтому цвет не меняется по ходу работы)
    color: Qt.platform.os === "windows" ? Theme.surface : "transparent"

    // Раскладка по размеру окна. Порог с гистерезисом: вход в режим — у порога, выход — на 24 px дальше,
    // поэтому у границы режим не дёргается туда-обратно. Переключается сразу в обе стороны, прямо во время
    // перетаскивания края: режим меняется лишь при пересечении порога, а не на каждом шаге.
    property string layoutMode: _modeFor(width, height, "")
    readonly property bool mini: layoutMode === "mini"
    function _modeFor(w, h, cur) {
        const band = 24
        const miniNow = cur === "mini" ? (w < 480 + band || h < 400 + band) : (w < 480 || h < 400)
        if (miniNow)
            return "mini"
        return (cur === "full" || cur === "") ? (w < 720 ? "compact" : "full") : (w < 720 + band ? "compact" : "full")
    }
    function _reevaluateMode() {
        const next = _modeFor(width, height, layoutMode)
        if (next !== layoutMode)
            layoutMode = next
    }
    // размер и положение — из настроек; заодно разрывает привязку layoutMode (дальше — только _reevaluateMode)
    Component.onCompleted: _restoreGeometry()
    onWidthChanged: { _reevaluateMode(); if (_geoReady) geoSave.restart() }
    onHeightChanged: { _reevaluateMode(); if (_geoReady) geoSave.restart() }

    // 0 — обычная сцена, 1 — мини-плеер; между ними — перекрёстное растворение с лёгким масштабом
    property real miniProgress: mini ? 1 : 0
    Behavior on miniProgress {
        NumberAnimation { duration: 320; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
    }
    property bool _miniUsed: false
    onMiniChanged: {
        if (mini) {
            _miniUsed = true
            if (nowPlaying.open)
                nowPlaying.hide()
        }
    }

    readonly property bool panelFits: width >= 1100
    readonly property bool panelShown: Settings.rightPanelOpen && panelFits && layoutMode === "full"
    // Открытая правая панель забирает 340 px: в окне уже 1400 боковая панель сворачивается в рейку,
    // иначе контенту остаётся меньше, чем в самом узком окне, и мини-плеер с «орбитой» в него не влезают
    readonly property bool compactSidebar: Settings.sidebarCollapsed || width < 980 || (panelShown && width < 1400)

    // Закрытие окна прячет его в трей (если это включено и трей в системе есть) — музыка играет дальше.
    // Выход — из меню значка или Ctrl+Q.
    readonly property bool trayWanted: typeof materialfyTray !== "undefined" && materialfyTray && Settings.closeToTray
    readonly property bool trayReady: tray.object !== null && tray.object.available

    // Из мини-плеера — обратно к обычному размеру (последний сохранённый, но не меньше обычной раскладки)
    function expandFromMini() {
        if (visibility !== Window.Windowed)
            visibility = Window.Windowed
        const w = Math.max(Settings.windowWidth, 960), h = Math.max(Settings.windowHeight, 640)
        const s = screen
        x = Math.max(s.virtualX, Math.min(x, s.virtualX + s.width - w))
        y = Math.max(s.virtualY, Math.min(y, s.virtualY + s.height - h))
        width = w
        height = h
    }

    function showWindow() {
        show()
        raise()
        requestActivate()
    }

    // ---- Размер и положение окна между запусками ----
    property bool _geoReady: false
    function _onSomeScreen(px, py, w) {
        // хотя бы верх окна (полоса, за которую его можно перетащить) — на одном из мониторов
        for (const s of Qt.application.screens) {
            if (px + w / 2 >= s.virtualX && px + w / 2 < s.virtualX + s.width && py >= s.virtualY && py + 24 < s.virtualY + s.height)
                return true
        }
        return false
    }
    function _restoreGeometry() {
        // --size (снимки, отладка) важнее сохранённого
        const cliSize = Qt.application.arguments.some(a => a === "--size" || a.startsWith("--size="))
        if (!cliSize) {
            width = Math.max(minimumWidth, Settings.windowWidth)
            height = Math.max(minimumHeight, Settings.windowHeight)
        }
        if (wayland) {
            // положение окна на Wayland выбирает композитор
        } else if (!cliSize && Settings.windowX > -1000000 && _onSomeScreen(Settings.windowX, Settings.windowY, width)) {
            x = Settings.windowX
            y = Settings.windowY
        } else {
            x = screen.virtualX + Math.max(0, (screen.width - width) / 2)
            y = screen.virtualY + Math.max(0, (screen.height - height) / 2)
        }
        layoutMode = _modeFor(width, height, "")
        if (!cliSize && Settings.windowMaximized)
            showMaximized()
        else
            show()
        _geoReady = true
    }
    function _saveGeometry() {
        if (!_geoReady)
            return
        if (visibility === Window.Maximized) {
            Settings.windowMaximized = true
            return
        }
        // свёрнутое, на весь экран или спрятанное в трей — последнее обычное положение не трогаем
        if (visibility !== Window.Windowed)
            return
        Settings.windowMaximized = false
        Settings.windowWidth = width
        Settings.windowHeight = height
        if (!wayland) {      // там x/y всегда 0 — не затираем положение, сохранённое под X11
            Settings.windowX = x
            Settings.windowY = y
        }
    }
    Timer { id: geoSave; interval: 400; onTriggered: win._saveGeometry() }
    onXChanged: if (_geoReady) geoSave.restart()
    onYChanged: if (_geoReady) geoSave.restart()
    onVisibilityChanged: if (_geoReady) geoSave.restart()

    onClosing: close => {
        _saveGeometry()
        if (trayWanted && trayReady) {
            close.accepted = false
            hide()
        } else {
            Qt.quit()
        }
    }

    Instantiator {
        id: tray
        active: win.trayWanted
        delegate: Platform.SystemTrayIcon {
            visible: true
            // одноцветный значок под панель: Windows — тема панели задач, иначе — тема Qt; macOS — шаблон (mask)
            readonly property bool darkPanel: Settings.systemTaskbarTheme !== ""
                ? Settings.systemTaskbarTheme === "dark"
                : Qt.styleHints.colorScheme !== Qt.ColorScheme.Light
            readonly property real dpr: win.screen ? win.screen.devicePixelRatio : 1
            icon.source: Qt.resolvedUrl("../assets/tray-" + (darkPanel && Qt.platform.os !== "osx" ? "white" : "black")
                                        + "-" + (dpr >= 1.75 ? 32 : dpr >= 1.25 ? 24 : 16) + ".png")
            icon.mask: Qt.platform.os === "osx"
            tooltip: Player.hasTrack ? Player.title + " — " + Player.artist : "Materialfy"
            onActivated: reason => {
                if (reason === Platform.SystemTrayIcon.MiddleClick)
                    Player.togglePlay()
                else if (reason === Platform.SystemTrayIcon.Context) {
                    // на Windows меню трея — виджетное, к значку Qt его не привязывает: открываем сами у курсора
                    if (Qt.platform.os === "windows")
                        trayMenu.open()
                } else
                    win.visible ? win.hide() : win.showWindow()
            }
            menu: Platform.Menu {
                id: trayMenu
                Platform.MenuItem {
                    text: win.visible ? I18n.t("Hide window") : I18n.t("Show window")
                    onTriggered: win.visible ? win.hide() : win.showWindow()
                }
                Platform.MenuSeparator {}
                Platform.MenuItem {
                    text: Player.playing ? I18n.t("Pause") : I18n.t("Resume")
                    enabled: Player.hasTrack
                    onTriggered: Player.togglePlay()
                }
                Platform.MenuItem { text: I18n.t("Next track"); enabled: Player.hasTrack; onTriggered: Player.next() }
                Platform.MenuItem { text: I18n.t("Previous track"); enabled: Player.hasTrack; onTriggered: Player.previous() }
                Platform.MenuSeparator {}
                Platform.MenuItem { text: I18n.t("Quit"); onTriggered: Qt.quit() }
            }
        }
    }

    // Всё содержимое окна — источник снимка для шторки смены темы (ThemeWipe поверх, вне его)
    Item {
        id: scene
        anchors.fill: parent
        // в мини-плеере сцена не выгружается (страницы и прокрутка сохраняются), а только гаснет и не рисуется
        visible: win.miniProgress < 1
        enabled: !win.mini
        // уходящая раскладка гаснет быстрее, чем проявляется новая
        opacity: Math.max(0, 1 - win.miniProgress * 1.6)
        scale: Theme.reducedMotion ? 1 : 1 - 0.04 * win.miniProgress
        layer.enabled: win.miniProgress > 0 && win.miniProgress < 1

        Rectangle { anchors.fill: parent; color: Theme.surface }

        // Атмосферный фон: размытая обложка за всем окном. Сайдбар лежит прямо на ней,
        // панели контента полупрозрачны — там свечение слабее.
        TrackBackdrop {
            id: ambient
            anchors.fill: parent
            visible: Settings.ambientBackground && Player.cover !== ""
            opacity: Theme.dark ? 0.3 : 0.35
            source: Player.cover
            // зерно «стекла» под всем интерфейсом — лишнее
            mode: Settings.backdropMode === "glass" ? "gauss" : Settings.backdropMode
            flow: Settings.nowPlayingDrift
            followTheme: Settings.accentFromCover
            running: visible && Player.playing && !nowPlaying.open
        }

        RowLayout {
            anchors.fill: parent
            anchors.margins: 8
            anchors.leftMargin: 0
            spacing: 8

            Sidebar {
                id: sidebar
                Layout.fillHeight: true
                Layout.preferredWidth: implicitWidth
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                router: router
                compact: win.compactSidebar
                drawer: win.layoutMode !== "full"
                onNewPlaylistRequested: newPlaylistDialog.open(null)
            }

            // Контент
            Surface {
                id: content
                Layout.fillWidth: true
                Layout.fillHeight: true
                level: "container"
                translucency: ambient.visible ? 0.3 : 0
                clip: true

                TopBar {
                    id: topBar
                    anchors.left: parent.left
                    anchors.right: parent.right
                    router: router
                    window: win
                    showMenuButton: win.layoutMode !== "full"
                    onMenuClicked: sidebar.openDrawer()
                    z: 2
                }

                Item {
                    id: pageHost
                    anchors.top: topBar.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom

                    Router {
                        id: router
                        // settings/releases/liked строятся один раз и живут: первый вход инкубирует
                        // (600–1200 мс), все следующие мгновенны. Параметрические (плейлисты,
                        // артисты) — LRU-слоты в keepAliveSlots.
                        keepAlive: ["home", "settings", "releases", "liked"]
                        keepAliveSlots: 10
                        pages: ({
                            home: Qt.resolvedUrl("app/pages/HomePage.qml"),
                            search: Qt.resolvedUrl("app/pages/SearchPage.qml"),
                            library: Qt.resolvedUrl("app/pages/LibraryPage.qml"),
                            folder: Qt.resolvedUrl("app/pages/FolderPage.qml"),
                            liked: Qt.resolvedUrl("app/pages/LikedPage.qml"),
                            playlist: Qt.resolvedUrl("app/pages/PlaylistPage.qml"),
                            album: Qt.resolvedUrl("app/pages/AlbumPage.qml"),
                            artist: Qt.resolvedUrl("app/pages/ArtistPage.qml"),
                            show: Qt.resolvedUrl("app/pages/ShowPage.qml"),
                            episode: Qt.resolvedUrl("app/pages/EpisodePage.qml"),
                            radio: Qt.resolvedUrl("app/pages/RadioPage.qml"),
                            releases: Qt.resolvedUrl("app/pages/ReleasesPage.qml"),
                            settings: Qt.resolvedUrl("app/pages/SettingsPage.qml"),
                            account: Qt.resolvedUrl("app/pages/AccountPage.qml"),
                            playback: Qt.resolvedUrl("app/pages/PlaybackSetupPage.qml")
                        })
                        anchors.fill: parent
                        // startPage=artist:<id> (album:, playlist:, show:, episode:) — страница с параметром. Каталог
                        // читается через сессию librespot, поэтому такая страница открывается, когда сессия поднялась
                        Component.onCompleted: {
                            Qt.callLater(router._prewarm)   // до любого return: работает и без deep-link
                            const [name, id] = win.startPage.split(":")
                            if (!id) {
                                reset(name)
                                return
                            }
                            reset("home")
                            const open = () => {
                                if (["ready", "error", "premiumRequired"].indexOf(Auth.playbackState) < 0)
                                    return
                                Auth.playbackChanged.disconnect(open)
                                reset(name, ({ [name + "Id"]: id }))
                            }
                            Auth.playbackChanged.connect(open)
                            open()
                        }
                    }
                    Connections {
                        target: Auth
                        enabled: win.autoPlay !== ""
                        function onPlaybackChanged() {
                            if (Auth.playbackState !== "ready")
                                return
                            Player.playContext(win.autoPlay, "")
                            win.autoPlay = ""
                            if (win.openNowPlaying)
                                nowPlayingTimer.start()
                        }
                    }
                    Timer {
                        id: browseTimer
                        property var steps: win.browse !== "" ? win.browse.split(",") : []
                        interval: 4000
                        repeat: true
                        running: steps.length > 0 && ["ready", "error", "premiumRequired"].indexOf(Auth.playbackState) >= 0
                        onTriggered: {
                            const step = steps[0]
                            steps = steps.slice(1)
                            const [name, id] = step.split(":")
                            if (name === "back")
                                router.back()
                            else if (name === "panel")
                                Settings.rightPanelOpen = !Settings.rightPanelOpen
                            else if (name === "sidebar")
                                Settings.sidebarCollapsed = !Settings.sidebarCollapsed
                            else if (!id)
                                router.reset(name)
                            else
                                router.push(name, ({ [name + "Id"]: id }))
                        }
                    }
                    Timer {
                        id: nowPlayingTimer
                        interval: 4000
                        onTriggered: nowPlaying.show(Qt.point(win.width / 2, win.height - 60))
                    }
                }

                MiniPlayer {
                    id: miniPlayer
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 16
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(parent.width - 32, 1080)
                    compact: win.layoutMode !== "full" || win.width < 860
                    z: 3
                    onOpenNowPlaying: center => nowPlaying.show(center)
                }
            }

            // Правая панель
            Item {
                Layout.fillHeight: true
                Layout.preferredWidth: win.panelShown ? 340 : 0
                Layout.leftMargin: win.panelShown ? 0 : -8   // убирает зазор RowLayout у закрытой панели
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Behavior on Layout.leftMargin { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                clip: true

                // Создаётся при первом открытии: в закрытом виде она не нужна, а собирать её дорого.
                // Закрытая не просто прозрачна, а скрыта: иначе её анимации (морфы пустой очереди, текст
                // песни) крутятся за кадром и перерисовывают всё окно. (Скрыт Loader, а не место в RowLayout:
                // невидимое место раскладка выкидывает вместе с промежутком, и страница стала бы шире.)
                Loader {
                    width: 340
                    height: parent.height
                    visible: parent.width > 0
                    active: win.panelShown || item !== null
                    sourceComponent: RightPanel {
                        opacity: win.panelShown ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    }
                }
            }
        }

        NowPlaying {
            id: nowPlaying
            anchors.fill: parent
            compact: win.layoutMode !== "full"
            z: 100
        }
    }

    Loader {
        id: miniMode
        anchors.fill: parent
        // однажды созданный мини-плеер остаётся (без пересоздания при каждом сжатии), невидимый — не рисуется
        active: win.mini || win._miniUsed
        visible: win.miniProgress > 0
        enabled: win.mini
        opacity: Math.max(0, (win.miniProgress - 0.25) / 0.75)
        scale: Theme.reducedMotion ? 1 : 0.96 + 0.04 * win.miniProgress
        layer.enabled: win.miniProgress > 0 && win.miniProgress < 1
        sourceComponent: MiniMode {
            window: win
            onExpandRequested: win.expandFromMini()
        }
    }

    // Смена цвета (и фона — он меняется под снимком) под новую обложку. Способ — из настроек;
    // направление: вперёд по очереди — слева направо, назад — справа налево; волна идёт от обложки
    ThemeWipe {
        id: themeWipe
        anchors.fill: parent
        source: win.mini && miniMode.item ? miniMode.item : scene
        // reduced motion: без шторок и волн — только растворение
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: win.mini && miniMode.item ? miniMode.item.coverItem
                  : nowPlaying.open ? nowPlaying.coverItem : miniPlayer.coverItem
        z: 1000
        Component.onCompleted: Theme.wipe = themeWipe
    }
    Binding { target: Theme; property: "wipeDirection"; value: Player.direction }
    // Уровень звука — полноэкранному плееру и мини-режиму («дыхание» обложки), полоскам островка и пульсу
    // обложки в панели задач
    Binding {
        target: Player
        property: "levelsWanted"
        value: nowPlaying.wantsLevels || (islandWindow.object !== null && islandWindow.object.wantsLevels)
            || (taskbarWindow.object !== null && taskbarWindow.object.wantsLevels)
    }

    // ---- Плеер вне окна: живёт рядом с главным окном, и пока оно свёрнуто или в трее ----
    function screenIndicesFromSetting(settingVal, screenCount) {
        if (!settingVal || settingVal === "" || settingVal === "0")
            return [0]
        if (settingVal === "all") {
            const arr = []
            for (let i = 0; i < screenCount; ++i) arr.push(i)
            return arr
        }
        const parts = settingVal.split(",")
        const res = []
        for (const p of parts) {
            const idx = parseInt(p.trim(), 10)
            if (!isNaN(idx) && idx >= 0 && idx < screenCount && res.indexOf(idx) === -1)
                res.push(idx)
        }
        return res.length > 0 ? res : [0]
    }

    readonly property var activeIslandScreens: {
        const screens = Qt.application.screens
        const count = screens ? screens.length : 1
        return win.screenIndicesFromSetting(Settings.floatingIslandScreens, count)
    }

    readonly property var activeTaskbarScreens: {
        const screens = Qt.application.screens
        const count = screens ? screens.length : 1
        return win.screenIndicesFromSetting(Settings.taskbarScreens, count)
    }

    // Снимок (--screenshot) снимает только главное окно — плеер вне окна там не нужен (кроме floatingShot).
    // Островок живёт и без трека (тонкая полоска у края), виджет в панели задач — только с треком
    readonly property bool _floatingAllowed: floatingShot !== "" || !Qt.application.arguments.some(a => a.startsWith("--screenshot"))

    Instantiator {
        id: islandWindow
        active: (win.floatingMode === "island" || win.floatingMode === "both") && win._floatingAllowed
        model: win.activeIslandScreens
        delegate: FloatingIsland {
            required property int modelData
            mainWindow: win
            screenIndex: modelData
            targetScreen: (Qt.application.screens && modelData < Qt.application.screens.length) ? Qt.application.screens[modelData] : null
            forcedState: win.islandState
            shotPath: win.floatingShot
        }
    }
    Instantiator {
        id: taskbarWindow
        active: (win.floatingMode === "taskbar" || win.floatingMode === "both") && win._floatingAllowed && Player.hasTrack
            && !_taskbarReset
        property bool _taskbarReset: false
        model: win.activeTaskbarScreens
        delegate: TaskbarPlayer {
            required property int modelData
            mainWindow: win
            screenIndex: modelData
            targetScreen: (Qt.application.screens && modelData < Qt.application.screens.length) ? Qt.application.screens[modelData] : null
            embed: win.taskbarEmbed
            shotPath: win.floatingShot
            forcedTheme: win.taskbarTheme
            // Explorer перезапустился и унёс окно с собой — создать заново (не из обработчика самого окна)
            onLost: taskbarRecreate.start()
        }
    }
    Timer {
        id: taskbarRecreate
        interval: 0
        onTriggered: {
            taskbarWindow._taskbarReset = true
            taskbarWindow._taskbarReset = false
        }
    }
    Binding {
        target: Theme
        property: "lowPower"
        value: Settings.lowPowerMode === "on" || (Settings.lowPowerMode !== "off" && win.softwareRendering)
    }
    // Окно не в фокусе (или свёрнуто) — декоративные анимации стоят
    // закреплённый мини-плеер висит на виду поверх других окон — его анимации не замирают и без фокуса
    Binding {
        target: Theme; property: "calm"
        value: Settings.calmWhenInactive && (win.visibility === Window.Minimized
               || (!win.active && !(win.pinnedOnTop && win.layoutMode === "mini")))
    }

    // Переходы к исполнителю / альбому / плейлисту — откуда угодно (строки треков, плееры)
    Connections {
        target: Catalog
        function onOpenRequested(name, props) {
            if (win.mini)
                win.expandFromMini()
            artistMenu.close()
            artistPreview.hide()
            if (nowPlaying.open)
                nowPlaying.hide()
            // поиск по ссылке при уже открытом поиске — тот же экран с новым запросом
            const page = router.currentItem
            if (name === "search" && router.currentName === "search" && page && page.showQuery)
                page.showQuery(Catalog.searchQuery)
            else
                router.push(name, props)
        }
        function onPreviewRequested(id, x, y, w) { artistPreview.show(id, x, y, w) }
        function onPreviewCancelled() { artistPreview.hideSoon() }
        function onChooseArtist(refs) {
            artistMenu.refs = refs
            artistMenu.open(refs.map(r => ({ text: r.name, icon: "person" })), cursor.point.position)
        }
        function onTrackMenuRequested(track, x, y, w, h) {
            artistPreview.hide()
            // «Убрать из этого плейлиста» — только на странице своего плейлиста
            const page = router.currentItem
            trackMenu.playlistId = !nowPlaying.open && router.currentName === "playlist" && page && page.canEdit ? page.playlistId : ""
            trackMenu.show(track, Qt.point(x, y), w > 0 && h > 0 ? Qt.size(w, h) : undefined)
        }
    }
    // Короткие сообщения сервисов; «не ответил» приходит только от Player
    Connections {
        target: Library
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Player
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Devices
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    // Snackbar для страниц: win.notify(текст[, действие, функция])
    function notify(text, actionText, onAction) {
        snackbar.show(text, actionText ?? "", onAction ?? null)
    }

    // «Радио по треку / исполнителю»: станция сразу играет, без перехода и без вопроса; сид выбран и на
    // орбите главной, чтобы там было видно, что звучит. Пустой title — подпись подставит сервис.
    function radioFrom(seedUri, title) {
        Radio.select(seedUri)
        Radio.playStation(seedUri, title)
    }

    // Настройки на нужном разделе (меню островка и виджета панели задач)
    function openSettings(section) {
        if (nowPlaying.open)
            nowPlaying.hide()
        if (win.mini)
            win.expandFromMini()
        router.reset("settings", section ? { section: section } : undefined)
    }

    function trackMenuAction(id, track, item) {
        // очередь, настройки и переходы живут в обычной раскладке
        if (win.mini && (id === "queue" || id === "sound" || id === "removefromplaylist"))
            win.expandFromMini()
        if (id === "radio:track") {
            win.radioFrom("spotify:track:" + track.trackId, track.title ?? "")
        } else if (id.startsWith("radio:artist:")) {
            const artistId = id.slice("radio:artist:".length)
            const ref = (track.artistRefs ?? []).find(r => r.id === artistId)
            win.radioFrom("spotify:artist:" + artistId, ref ? ref.name : (track.artists ?? ""))
        } else if (id === "addqueue") {
            Player.addToQueue(track)
            snackbar.show(I18n.t("Added to queue"))
        } else if (id === "dislike") {
            Library.dislike(track)
        } else if (id === "like") {
            Library.toggleLike(track)
        } else if (id === "save") {
            const uri = track.isEpisode ? (track.uri ?? "") : "spotify:album:" + (track.albumId ?? "")
            const was = Library.isSaved(uri)
            Library.toggleSaved(uri)
            snackbar.show(was ? I18n.t("Removed from library") : I18n.t("Saved to library"))
        } else if (id === "removefromplaylist") {
            const page = router.currentItem
            if (router.currentName === "playlist" && page && typeof track.pos === "number")
                page.trackRemoved(track.pos)
        } else if (id === "repeat") {
            Player.cycleRepeat()
        } else if (id === "queue") {
            if (nowPlaying.open)
                nowPlaying.hide()
            Settings.rightPanelTab = "queue"
            Settings.rightPanelOpen = true
        } else if (id === "sound") {
            if (nowPlaying.open)
                nowPlaying.hide()
            router.reset("settings", { section: "playback" })
        } else if (id === "playlist:new") {
            newPlaylistDialog.open(track)
        } else if (id.startsWith("playlist:")) {
            Library.addToPlaylist(track, id.slice("playlist:".length))
        } else if (id === "share:track") {
            Catalog.copyLink(Catalog.trackUrl(track))
            snackbar.show(I18n.t("Link copied"))
        } else if (id.startsWith("share:artist:")) {
            Catalog.copyLink(Catalog.artistUrl(id.slice("share:artist:".length)))
            snackbar.show(I18n.t("Link copied"))
        } else if (id === "goto:track") {
            Catalog.openAlbum(track.albumId, track.trackId)
        } else if (id === "goto:album") {
            Catalog.openAlbum(track.albumId)
        } else if (id === "goto:artist") {
            Catalog.openArtists(track.artistRefs ?? [])
        } else if (id === "goto:show") {
            Catalog.openShow(track.showId ?? "")
        }
    }
    HoverHandler { id: cursor }
    ArtistPreview {
        id: artistPreview
        anchors.fill: parent
        z: 1900
    }
    Menu {
        id: artistMenu
        property var refs: []
        anchors.fill: parent
        z: 2000
        onPicked: i => Catalog.openArtist(refs[i].id)
    }
    TrackMenu {
        id: trackMenu
        z: 2000
        onTriggered: (id, track, item) => win.trackMenuAction(id, track, item)
    }
    NewPlaylistDialog {
        id: newPlaylistDialog
        z: 2100
    }
    // общий Client ID упёрся в лимит: как подключить своё приложение
    ApiLimitDialog {
        z: 2120
    }
    UpdateSheet {
        z: 2150
    }
    Snackbar {
        id: snackbar
        anchors.fill: parent
        z: 2200
    }
    // Прошлый запуск упал или зависал (в crash.log новые записи) — предложить сообщить, один раз
    Timer {
        interval: 2500
        running: CrashReport.pending
        onTriggered: {
            snackbar.show(I18n.t("Materialfy closed unexpectedly last time"), I18n.t("Report"), () => {
                Qt.openUrlExternally(CrashReport.folder)
                Qt.openUrlExternally(CrashReport.issueUrl)
            }, 20000)
            CrashReport.dismiss()
        }
    }

    ResizeEdges { window: win }
    // забирает всё содержимое окна к себе: фон окна и скругление углов (см. WindowFrame)
    WindowFrame { window: win }

    Loader {
        active: typeof materialfyDebugFps !== "undefined" && materialfyDebugFps
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 16
        z: 10000
        sourceComponent: FpsOverlay {}
    }

    // Навигация мышью «назад» и горячие клавиши
    TapHandler {
        acceptedButtons: Qt.BackButton
        onTapped: nowPlaying.open ? nowPlaying.hide() : router.back()
    }
    Shortcut { sequences: [StandardKey.Back, "Alt+Left"]; onActivated: nowPlaying.open ? nowPlaying.hide() : router.back() }

    // Плеер (пробел не перехватывается, пока фокус в поле ввода)
    Shortcut { sequence: "Space"; onActivated: Player.togglePlay() }
    Shortcut { sequences: ["Ctrl+Right", "Media Next"]; onActivated: Player.next() }
    Shortcut { sequences: ["Ctrl+Left", "Media Previous"]; onActivated: Player.previous() }
    Shortcut { sequence: "Ctrl+Up"; onActivated: Player.volume = Math.min(1, Player.volume + 0.05) }
    Shortcut { sequence: "Ctrl+Down"; onActivated: Player.volume = Math.max(0, Player.volume - 0.05) }
    Shortcut { sequence: "Shift+Right"; onActivated: Player.seekMs(Player.positionMs + 10000) }
    Shortcut { sequence: "Shift+Left"; onActivated: Player.seekMs(Player.positionMs - 10000) }
    Shortcut {
        sequence: "Ctrl+P"
        onActivated: nowPlaying.open ? nowPlaying.hide() : !win.mini && Player.hasTrack && nowPlaying.show(null)
    }
    Shortcut {
        sequences: [StandardKey.Find]
        onActivated: {
            // в списке треков — сначала поиск по нему; повторное нажатие — общий поиск
            if (!nowPlaying.open && router.currentItem && router.currentItem.findInPage && router.currentItem.findInPage())
                return
            if (nowPlaying.open)
                nowPlaying.hide()
            // Поиск уже открыт: дедуп reset не перестроит страницу и focus не случится сам — зовём явно.
            if (router.currentName === "search" && router.currentItem && router.currentItem.focusSearch) {
                router.currentItem.focusSearch()
                return
            }
            router.reset("search")
            // фокус берёт сама SearchPage в Component.onCompleted — при инкубации reset currentItem ещё старый
        }
    }
    Shortcut { sequence: "Ctrl+,"; onActivated: router.reset("settings") }
    Shortcut { sequence: "Ctrl+Shift+L"; onActivated: router.reset("library") }
    Shortcut { sequence: "Ctrl+B"; onActivated: Settings.sidebarCollapsed = !Settings.sidebarCollapsed }
    Shortcut { sequence: "Ctrl+L"; onActivated: { Settings.rightPanelTab = "lyrics"; Settings.rightPanelOpen = !Settings.rightPanelOpen } }
    Shortcut { sequences: [StandardKey.Quit]; onActivated: Qt.quit() }
    // F11: на весь экран и обратно — к тому, каким окно было (развёрнутым или обычным)
    property bool _maximizedBeforeFullScreen: false
    Shortcut {
        sequence: "F11"
        onActivated: {
            if (win.visibility === Window.FullScreen) {
                win.visibility = win._maximizedBeforeFullScreen ? Window.Maximized : Window.Windowed
            } else {
                win._maximizedBeforeFullScreen = win.visibility === Window.Maximized
                win.visibility = Window.FullScreen
            }
        }
    }
}
