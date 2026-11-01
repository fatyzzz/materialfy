import QtQuick
import QtQuick.Effects
import Md3
import Materialfy.Core

// Плеер в панели задач Windows: пилюля MD3 Expressive (чёрная или в тоне обложки) — обложка, название,
// исполнитель и тонкая волна прогресса, «пуск» и «дальше» («назад» — при наведении). Окно встраивается
// в Shell_TrayWnd (WindowShell.taskbarPlace, cpp/floating.cpp) или, если не вышло (Windhawk, второй монитор),
// стоит над панелью, которой оно принадлежит. Раз в 2 с встаёт на место заново (панель сменила размер,
// значки сдвинулись); Explorer унёс окно с собой при перезапуске — lost(), Main.qml создаёт его заново.
// Зажать обложку или название и потянуть — виджет едет вдоль панели и липнет к местам (taskbarWidgetPos).
// Колесо — громкость (уровень — на обложке), стрелки и пробел при наведении — как у островка.
Window {
    id: root

    property var mainWindow: null
    property var targetScreen: null
    property int screenIndex: 0
    property bool embed: true
    signal lost()
    // --set floatingShot=<path.png>: снимок окна через 9 с без встраивания; --set taskbarTheme=dark|light — тема панели
    property string shotPath: ""
    property string forcedTheme: ""
    Timer {
        interval: 9000
        running: root.shotPath !== ""
        onTriggered: root.contentItem.grabToImage(result => result.saveToFile(root.shotPath))
    }

    screen: targetScreen || (mainWindow ? mainWindow.screen : null)

    readonly property int slotWidth: 264
    width: slotWidth
    height: 40
    color: "transparent"
    title: "Materialfy"
    flags: Qt.Tool | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
    // Без владельца из Qt: окно, принадлежащее главному, Windows прячет вместе с ним (владелец — панель, см. C++)
    transientParent: null

    readonly property bool motion: !Theme.reducedMotion && !Theme.lowPower
    // уровень звука нужен только «пульсу» обложки
    readonly property bool wantsLevels: visible && motion && Settings.coverPulse && Player.playing && Player.isLocal === true
    readonly property bool colorMode: Settings.floatingPlayerStyle === "color"
    readonly property bool _onCover: colorMode && Player.cover !== ""
    // На тёмном стекле над обложкой и на чёрном — светлые; цветной без обложки — тональный контейнер
    readonly property color ink: colorMode && !_onCover ? Theme.fgPrimaryContainer : "#ffffff"
    readonly property color inkDim: Qt.alpha(ink, 0.72)
    // на чёрном — «фиксированный» акцент MD3: одинаково светлый в светлой и тёмной теме
    readonly property color accent: colorMode ? ink : Theme.primaryFixedDim

    // 1 — в панели задач, 2 — над ней, 0 — некуда (вертикальная, узкая или спрятанная панель): окно спрятано
    property int placement: 0
    property bool _ready: false
    property bool _fullscreen: false
    property bool dragActive: false
    visible: _ready && placement > 0 && !(placement === 2 && _fullscreen)
    // Qt при показе окна сбрасывает его владельца — вернуть панель сразу, а не через 2 с
    onVisibleChanged: if (visible) Qt.callLater(place)

    function currentPos() {
        if (Settings.taskbarPositions !== "") {
            try {
                const map = JSON.parse(Settings.taskbarPositions)
                if (map && map[String(screenIndex)] !== undefined)
                    return map[String(screenIndex)]
            } catch (e) {}
        }
        return Settings.taskbarWidgetPos || "tray"
    }
    function savePos(pos) {
        let map = {}
        try { map = Settings.taskbarPositions !== "" ? JSON.parse(Settings.taskbarPositions) : {} } catch (e) {}
        map[String(screenIndex)] = pos
        Settings.taskbarPositions = JSON.stringify(map)
        if (screenIndex === 0)
            Settings.taskbarWidgetPos = pos
    }

    function place() {
        if (shotPath !== "") {
            placement = 1
            return
        }
        const result = WindowShell.taskbarPlace(root, slotWidth, height, embed, currentPos())
        if (result < 0) {
            placement = 0
            root.lost()
            return
        }
        placement = result
    }
    Component.onCompleted: {
        WindowShell.makeToolWindow(root)
        place()
        _ready = true
    }
    Component.onDestruction: {
        WindowShell.keyHook(root, false)
        WindowShell.taskbarDetach(root)
    }

    Timer {
        interval: 2000
        repeat: true
        running: root._ready && root.shotPath === ""
        onTriggered: {
            if (!root.dragActive)
                root.place()
            root._fullscreen = root.placement === 2 && WindowShell.fullscreenOn(root)
        }
    }
    Connections {
        target: Settings
        function onTaskbarWidgetPosChanged() { if (!root.dragActive) root.place() }
        function onTaskbarPositionsChanged() { if (!root.dragActive) root.place() }
    }

    // Клавиши без фокуса: ← → треки, ↑ ↓ громкость, пробел
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
        coverVolume.show()
        volumeShown.restart()
    }
    Timer { id: volumeShown; interval: 1100 }

    function _time(ms) {
        const s = Math.max(0, Math.floor(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    Item {
        id: content
        anchors.fill: parent

        HoverHandler {
            id: hover
            onHoveredChanged: WindowShell.keyHook(root, hovered)
        }
        WheelHandler {
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: event => {
                const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
                root.nudgeVolume(touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05)
            }
        }
        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: point => menu.openAt(Qt.point(point.position.x, point.position.y))
        }

        // ---- Пилюля ----
        Rectangle {
            id: pill
            anchors.fill: parent
            radius: height / 2
            clip: true
            color: root.colorMode ? Theme.primaryContainer : "#000000"
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, root.colorMode ? 0.14 : 0.10)

            // Цветной: обложка, размытая и «плавающая», как фон плеера в приложении (настройки фона);
            // по форме пилюли обрезает маска (clip режет по прямоугольнику)
            Item {
                id: coverFill
                anchors.fill: parent
                visible: root._onCover
                layer.enabled: visible
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: coverMask
                    maskThresholdMin: 0.5
                    maskSpreadAtMin: 1.0
                }
                TrackBackdrop {
                    anchors.fill: parent
                    source: Player.cover
                    mode: Settings.backdropMode
                    blur: Settings.nowPlayingBlur
                    flow: Settings.nowPlayingDrift
                    running: root.motion && Player.playing
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
                radius: height / 2
            }
            // слой состояния MD3: 8 % при наведении
            Rectangle {
                anchors.fill: parent
                color: root.ink
                opacity: root.dragActive ? 0.12 : hover.hovered ? 0.08 : 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            }
        }
        // при перетаскивании — обводка акцентом
        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: "transparent"
            border.width: 2
            border.color: Theme.primary
            opacity: root.dragActive ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        }

        // ---- Обложка и подписи: клик — открыть Materialfy, зажать и тянуть — двигать виджет ----
        Item {
            id: track
            anchors.left: parent.left
            anchors.leftMargin: 4
            anchors.right: buttons.left
            anchors.rightMargin: 4
            anchors.top: parent.top
            anchors.bottom: parent.bottom

            Item {
                id: cover
                width: 32
                height: 32
                anchors.verticalCenter: parent.verticalCenter
                scale: root.dragActive ? 0.92 : 1
                Behavior on scale { NumberAnimation { duration: Theme.motion.effectsFast } }
                MorphImage {
                    id: coverImage
                    anchors.fill: parent
                    visible: Player.cover !== ""
                    source: Player.cover
                    eager: true
                    fallbackKind: Player.album !== "" ? "album" : "track"
                    fallbackArtist: Player.artist.split(", ")[0]
                    fallbackTitle: Player.album !== "" ? Player.album : Player.title
                    // MD3E: на паузе — мягкий квадрат, играет — «печенье»; при наведении — круг
                    shape: hover.hovered ? "circle" : Player.playing ? "cookie12" : "softSquare"
                    // как в приложении: «Пульс обложки» и «Вращение рамки» из настроек
                    pulse: root.wantsLevels ? Math.min(0.24, Player.level * 0.35) : 0
                    MaskSpin { target: coverImage; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true && root.motion }
                }
                MorphShape {
                    anchors.fill: parent
                    visible: Player.cover === ""
                    shape: "softSquare"
                    color: Qt.alpha(root.ink, 0.14)
                    Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 18; color: root.inkDim }
                }
                CoverVolume {
                    id: coverVolume
                    anchors.fill: parent
                    radius: hover.hovered || Player.playing ? width / 2 : width * 0.3
                    accent: root.accent
                }
            }
            Column {
                id: texts
                anchors.left: cover.right
                anchors.leftMargin: 9
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.verticalCenterOffset: -5
                spacing: -2
                Label {
                    width: parent.width
                    text: Player.title
                    type: "labelMedium"
                    weight: 600
                    color: root.ink
                    elide: Text.ElideRight
                }
                Label {
                    width: parent.width
                    text: volumeShown.running ? I18n.t("Volume {0}%", Math.round(Player.volume * 100))
                        : seekBar.scrubbing ? root._time(seekBar.shownValue * Player.durationMs) + " / " + root._time(Player.durationMs)
                        : Player.artist
                    type: "labelSmall"
                    weight: 400
                    color: root.inkDim
                    elide: Text.ElideRight
                }
            }
            // волна прогресса под подписями: не бежит (виджет виден всегда), двигается с позицией трека.
            // Клик или протяжка — перемотка (поверх области «открыть / перетащить виджет»); пока навели или
            // тянут, вместо исполнителя — время
            WavyProgress {
                id: seekBar
                readonly property bool scrubbing: dragging || seekHover.hovered
                anchors.left: texts.left
                anchors.right: parent.right
                anchors.rightMargin: 4
                anchors.top: texts.bottom
                height: 8
                z: 2
                thickness: scrubbing ? 4 : 3
                amplitude: 1.5
                wavelength: 12
                speed: 0
                gap: 3
                wavy: Player.playing && !scrubbing
                value: Player.position
                interactive: Player.durationMs > 0
                activeColor: root.accent
                stopColor: activeColor
                trackColor: Qt.alpha(root.ink, 0.32)
                onCommitted: v => Player.seek(v)
                Behavior on thickness { NumberAnimation { duration: Theme.motion.effectsFast } }
                HoverHandler { id: seekHover; margin: 4 }
            }

            MouseArea {
                id: grab
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                cursorShape: root.dragActive ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                preventStealing: true
                property point _start
                // тянуть начинают, сдвинув мышь на 6 px или подержав 250 мс
                Timer { id: holdTimer; interval: 250; onTriggered: grab._beginDrag() }
                function _beginDrag() {
                    if (root.dragActive || !pressed || root.shotPath !== "")
                        return
                    root.dragActive = true
                    WindowShell.taskbarDrag(root, root.slotWidth, root.height, 0)
                }
                onPressed: mouse => {
                    _start = Qt.point(mouse.x, mouse.y)
                    holdTimer.restart()
                }
                onPositionChanged: mouse => {
                    if (!root.dragActive && Math.abs(mouse.x - _start.x) + Math.abs(mouse.y - _start.y) > 6)
                        _beginDrag()
                    if (root.dragActive)
                        WindowShell.taskbarDrag(root, root.slotWidth, root.height, 1)
                }
                onReleased: {
                    holdTimer.stop()
                    if (!root.dragActive) {
                        WindowShell.raiseMain()
                        return
                    }
                    const pos = WindowShell.taskbarDrag(root, root.slotWidth, root.height, 2)
                    root.dragActive = false
                    if (pos !== "")
                        root.savePos(pos)
                }
                onCanceled: {
                    holdTimer.stop()
                    if (root.dragActive)
                        WindowShell.taskbarDrag(root, root.slotWidth, root.height, 2)
                    root.dragActive = false
                    root.place()
                }
            }
        }

        // ---- Кнопки: «назад» выезжает при наведении, «пуск» — заполненная фигура MD3E, «дальше» ----
        Row {
            id: buttons
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            spacing: 0
            BarButton {
                icon: "skip_previous"
                width: hover.hovered ? 30 : 0
                opacity: hover.hovered ? 1 : 0
                visible: width > 0.5
                clip: true
                Behavior on width { enabled: root.motion; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: Player.previous()
            }
            PlayButton {
                anchors.verticalCenter: parent.verticalCenter
                size: 32
                playing: Player.playing
                loading: Player.buffering
                containerColor: root.accent
                iconColor: root.colorMode ? (root._onCover ? "#000000" : Theme.primaryContainer) : Theme.fgPrimaryFixed
                spin: false
                onClicked: Player.togglePlay()
            }
            BarButton {
                icon: "skip_next"
                onClicked: Player.next()
            }
        }
    }

    // Смена цветов под новую обложку — той же шторкой, что и в окне приложения, и одновременно с ней
    ThemeWipe {
        id: barWipe
        anchors.fill: parent
        source: content
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: coverImage
        Component.onCompleted: Theme.addWipe(barWipe)
        Component.onDestruction: Theme.removeWipe(barWipe)
    }

    FloatingMenu {
        id: menu
        anchors.fill: parent
        mainWindow: root.mainWindow
    }

    component BarButton: Item {
        id: button
        property string icon
        signal clicked()
        width: 30
        height: 30
        anchors.verticalCenter: parent ? parent.verticalCenter : undefined
        Rectangle {
            anchors.fill: parent
            radius: tap.pressed ? 9 : 15
            color: Qt.alpha(root.ink, tap.pressed ? 0.16 : bhover.hovered ? 0.1 : 0)
            Behavior on radius { NumberAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.centerIn: parent
            name: button.icon
            size: 20
            fill: 1
            color: root.ink
        }
        HoverHandler { id: bhover }
        TapHandler { id: tap; onTapped: button.clicked() }
    }
}
