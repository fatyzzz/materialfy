pragma Singleton
import QtQuick
import Materialfy.Core
import "ColorMix.js" as Mix
import "Typography.js" as Typography

// Токены MD3 Expressive. Цвета берутся из ThemeEngine и плавно анимируются при смене обложки.
Item {
    id: root

    readonly property var engine: ThemeEngine
    readonly property bool dark: ThemeEngine.dark
    property int colorDuration: 800

    // ---- Цветовые роли ---------------------------------------------------
    // Смена схемы — ОДНА анимация прогресса _p, все роли смешиваются по ней синхронно (в OKLab).
    // Роли onX из спецификации названы fgX (onPrimary → fgPrimary, inverseOnSurface → fgInverseSurface):
    // QML считает имена вида onSomething обработчиками сигналов, и Behavior на них роняет движок.

    // true, пока идёт смена схемы: компоненты отключают свои Behavior на цвет, чтобы не было двойной анимации.
    readonly property bool transitioning: _anim.running || (wipe !== null && wipe.running)

    // Если задан ThemeWipe — схема меняется «шторкой» в сторону wipeDirection (+1 вправо, -1 влево),
    // иначе — плавной анимацией всех цветов.
    property Item wipe: null
    // Шторки окон поверх приложения (островок, плеер в панели задач): идут вместе с основной. Снимки
    // со старыми цветами снимаются во всех окнах, схема меняется, когда готовы все, и снимки уходят разом
    property var floatingWipes: []
    function addWipe(w) { floatingWipes = floatingWipes.filter(x => x !== w).concat([w]) }
    function removeWipe(w) { floatingWipes = floatingWipes.filter(x => x !== w) }
    // Окно не в фокусе: декоративные бесконечные анимации стоят. Каждая такая анимация заставляет
    // перерисовывать всё окно на частоте монитора — это основная нагрузка в фоне.
    property bool calm: false
    // Режим для слабых ПК: настройка «вкл» или (по умолчанию, «авто») кадры рисует процессор — программный рендер
    // (llvmpipe, WARP: нет драйвера видеокарты; Main.qml берёт из softwareRendering окна). Каждый кадр там стоит
    // десятки миллисекунд ядра: декоративное движение стоит всегда, окно без слоя-скругления.
    property bool lowPower: false
    // Декоративные бесконечные анимации (дрейф фигур и фона, вращение и «дыхание» обложек, бегущая волна) стоят.
    // Индикаторы загрузки смотрят только на calm: на программном рендере они крутятся — это не украшение.
    readonly property bool quiet: calm || lowPower
    // Минимум анимаций: настройка «Анимации» или системный флаг. Пространственные длительности
    // обнуляются, остаются короткие затухания; волны и морфы компоненты выключают сами.
    // Режим для слабых ПК, включённый вручную, тоже: на совсем слабой машине тормозят и переходы.
    readonly property bool reducedMotion: Settings.animations === "reduced" || Settings.lowPowerMode === "on"
        || (Settings.animations === "system" && Settings.systemReducedMotion === true)
    // Раскладка телефона (MobileMain.qml, сборка для Android и --mobile): касания вместо курсора — без наведения,
    // крупные цели, нижние листы вместо всплывающих меню. safeTop/safeBottom — системные панели и вырез экрана.
    property bool mobile: false
    property real safeTop: 0
    property real safeBottom: 0
    // Цвет схемы, когда ничего не играет
    readonly property color defaultSeed: "#8C6DFF"   // Ultraviolet, BRAND_SEED в bridges/theme.rs
    // Новая схема применена: instant — мгновенно под шторкой, иначе — началась плавная анимация.
    // По нему синхронно с цветами меняется то, что зависит от обложки (атмосферный фон).
    signal schemeApplied(bool instant)
    property int wipeDirection: 1

    // публичное имя → ключ в ThemeEngine.colors
    readonly property var _roles: ({
        background: "background",
        fgBackground: "onBackground",
        surface: "surface",
        surfaceDim: "surfaceDim",
        surfaceBright: "surfaceBright",
        surfaceContainerLowest: "surfaceContainerLowest",
        surfaceContainerLow: "surfaceContainerLow",
        surfaceContainer: "surfaceContainer",
        surfaceContainerHigh: "surfaceContainerHigh",
        surfaceContainerHighest: "surfaceContainerHighest",
        fgSurface: "onSurface",
        surfaceVariant: "surfaceVariant",
        fgSurfaceVariant: "onSurfaceVariant",
        outline: "outline",
        outlineVariant: "outlineVariant",
        inverseSurface: "inverseSurface",
        fgInverseSurface: "inverseOnSurface",
        shadow: "shadow",
        scrim: "scrim",
        surfaceTint: "surfaceTint",
        primary: "primary",
        primaryDim: "primaryDim",
        fgPrimary: "onPrimary",
        primaryContainer: "primaryContainer",
        fgPrimaryContainer: "onPrimaryContainer",
        inversePrimary: "inversePrimary",
        primaryFixed: "primaryFixed",
        primaryFixedDim: "primaryFixedDim",
        fgPrimaryFixed: "onPrimaryFixed",
        fgPrimaryFixedVariant: "onPrimaryFixedVariant",
        secondary: "secondary",
        secondaryDim: "secondaryDim",
        fgSecondary: "onSecondary",
        secondaryContainer: "secondaryContainer",
        fgSecondaryContainer: "onSecondaryContainer",
        secondaryFixed: "secondaryFixed",
        secondaryFixedDim: "secondaryFixedDim",
        fgSecondaryFixed: "onSecondaryFixed",
        fgSecondaryFixedVariant: "onSecondaryFixedVariant",
        tertiary: "tertiary",
        tertiaryDim: "tertiaryDim",
        fgTertiary: "onTertiary",
        tertiaryContainer: "tertiaryContainer",
        fgTertiaryContainer: "onTertiaryContainer",
        tertiaryFixed: "tertiaryFixed",
        tertiaryFixedDim: "tertiaryFixedDim",
        fgTertiaryFixed: "onTertiaryFixed",
        fgTertiaryFixedVariant: "onTertiaryFixedVariant",
        error: "error",
        errorDim: "errorDim",
        fgError: "onError",
        errorContainer: "errorContainer",
        fgErrorContainer: "onErrorContainer",
        primaryPaletteKeyColor: "primaryPaletteKeyColor",
        secondaryPaletteKeyColor: "secondaryPaletteKeyColor",
        tertiaryPaletteKeyColor: "tertiaryPaletteKeyColor",
        neutralPaletteKeyColor: "neutralPaletteKeyColor",
        neutralVariantPaletteKeyColor: "neutralVariantPaletteKeyColor",
        errorPaletteKeyColor: "errorPaletteKeyColor"
    })

    property var _fromLab: ({})
    property var _toLab: ({})
    property real _p: 1

    function _mix(name) {
        const key = _roles[name]
        const b = _toLab[key]
        if (!b)  // до Component.onCompleted
            return ThemeEngine.colors[key]
        return Mix.mix(_fromLab[key] ?? b, b, _p)
    }

    function _startTransition() {
        const from = {}
        for (const name in _roles)
            from[_roles[name]] = Mix.toLab(root[name])  // текущий видимый цвет, даже посреди перехода
        _fromLab = from
        _toLab = Mix.labMap(ThemeEngine.colors)
        _p = 0
        _anim.restart()
        schemeApplied(false)
    }

    Component.onCompleted: {
        const lab = Mix.labMap(ThemeEngine.colors)
        _fromLab = lab
        _toLab = lab
    }

    function _applyNow() {
        _anim.stop()
        const lab = Mix.labMap(ThemeEngine.colors)
        _toLab = lab
        _fromLab = lab
        _p = 1
        schemeApplied(true)
    }

    Connections {
        target: ThemeEngine
        function onSchemeChanged() {
            // программный рендер: сразу, без шторки на всё окно (каждый её кадр процессор рисует целиком)
            if (root.lowPower)
                root._applyNow()
            else if (root.wipe !== null && root.wipe.visible)
                root._wipeAll()
            else
                root._startTransition()
        }
    }

    property var _wiping: []
    property int _wipeLeft: 0
    function _wipeAll() {
        const ws = [wipe].concat(floatingWipes.filter(w => w && w.onScreen))
        _wiping = ws
        _wipeLeft = ws.length
        for (const w of ws)
            w.start(wipeDirection, root._wipeReady)
        _wipeTimeout.restart()
    }
    function _wipeReady(w) {
        if (_wipeLeft <= 0) {       // опоздал: схема уже сменилась
            w.reveal()
            return
        }
        if (--_wipeLeft === 0)
            _wipeGo()
    }
    function _wipeGo() {
        _wipeTimeout.stop()
        _wipeLeft = 0
        _applyNow()
        for (const w of _wiping)
            w.reveal()
        _wiping = []
    }
    // окно, которое не рисуется (спрятано), снимок не даст: долго не ждём
    Timer { id: _wipeTimeout; interval: 300; onTriggered: if (root._wipeLeft > 0) root._wipeGo() }

    NumberAnimation {
        id: _anim
        target: root
        property: "_p"
        from: 0
        to: 1
        duration: root.colorDuration
        easing.type: Easing.InOutCubic
    }

    readonly property color background: _mix("background")
    readonly property color fgBackground: _mix("fgBackground")
    readonly property color surface: _mix("surface")
    readonly property color surfaceDim: _mix("surfaceDim")
    readonly property color surfaceBright: _mix("surfaceBright")
    readonly property color surfaceContainerLowest: _mix("surfaceContainerLowest")
    readonly property color surfaceContainerLow: _mix("surfaceContainerLow")
    readonly property color surfaceContainer: _mix("surfaceContainer")
    readonly property color surfaceContainerHigh: _mix("surfaceContainerHigh")
    readonly property color surfaceContainerHighest: _mix("surfaceContainerHighest")
    readonly property color fgSurface: _mix("fgSurface")
    readonly property color surfaceVariant: _mix("surfaceVariant")
    readonly property color fgSurfaceVariant: _mix("fgSurfaceVariant")
    readonly property color outline: _mix("outline")
    readonly property color outlineVariant: _mix("outlineVariant")
    readonly property color inverseSurface: _mix("inverseSurface")
    readonly property color fgInverseSurface: _mix("fgInverseSurface")
    readonly property color shadow: _mix("shadow")
    readonly property color scrim: _mix("scrim")
    readonly property color surfaceTint: _mix("surfaceTint")
    readonly property color primary: _mix("primary")
    readonly property color primaryDim: _mix("primaryDim")
    readonly property color fgPrimary: _mix("fgPrimary")
    readonly property color primaryContainer: _mix("primaryContainer")
    readonly property color fgPrimaryContainer: _mix("fgPrimaryContainer")
    readonly property color inversePrimary: _mix("inversePrimary")
    readonly property color primaryFixed: _mix("primaryFixed")
    readonly property color primaryFixedDim: _mix("primaryFixedDim")
    readonly property color fgPrimaryFixed: _mix("fgPrimaryFixed")
    readonly property color fgPrimaryFixedVariant: _mix("fgPrimaryFixedVariant")
    readonly property color secondary: _mix("secondary")
    readonly property color secondaryDim: _mix("secondaryDim")
    readonly property color fgSecondary: _mix("fgSecondary")
    readonly property color secondaryContainer: _mix("secondaryContainer")
    readonly property color fgSecondaryContainer: _mix("fgSecondaryContainer")
    readonly property color secondaryFixed: _mix("secondaryFixed")
    readonly property color secondaryFixedDim: _mix("secondaryFixedDim")
    readonly property color fgSecondaryFixed: _mix("fgSecondaryFixed")
    readonly property color fgSecondaryFixedVariant: _mix("fgSecondaryFixedVariant")
    readonly property color tertiary: _mix("tertiary")
    readonly property color tertiaryDim: _mix("tertiaryDim")
    readonly property color fgTertiary: _mix("fgTertiary")
    readonly property color tertiaryContainer: _mix("tertiaryContainer")
    readonly property color fgTertiaryContainer: _mix("fgTertiaryContainer")
    readonly property color tertiaryFixed: _mix("tertiaryFixed")
    readonly property color tertiaryFixedDim: _mix("tertiaryFixedDim")
    readonly property color fgTertiaryFixed: _mix("fgTertiaryFixed")
    readonly property color fgTertiaryFixedVariant: _mix("fgTertiaryFixedVariant")
    readonly property color error: _mix("error")
    readonly property color errorDim: _mix("errorDim")
    readonly property color fgError: _mix("fgError")
    readonly property color errorContainer: _mix("errorContainer")
    readonly property color fgErrorContainer: _mix("fgErrorContainer")
    readonly property color primaryPaletteKeyColor: _mix("primaryPaletteKeyColor")
    readonly property color secondaryPaletteKeyColor: _mix("secondaryPaletteKeyColor")
    readonly property color tertiaryPaletteKeyColor: _mix("tertiaryPaletteKeyColor")
    readonly property color neutralPaletteKeyColor: _mix("neutralPaletteKeyColor")
    readonly property color neutralVariantPaletteKeyColor: _mix("neutralVariantPaletteKeyColor")
    readonly property color errorPaletteKeyColor: _mix("errorPaletteKeyColor")

    // Прозрачность слоёв состояния
    readonly property QtObject stateLayer: QtObject {
        readonly property real hover: 0.08
        readonly property real focus: 0.10
        readonly property real pressed: 0.10
        readonly property real dragged: 0.16
        readonly property real disabledContent: 0.38
        readonly property real disabledContainer: 0.12
    }

    // ---- Типографика ---------------------------------------------------------
    // Google Sans (OFL, вшит в assets/fonts). Google Sans Flex не подходит: в нём нет кириллицы.
    readonly property string fontFamily: _pickFamily(["Google Sans", "Roboto Flex", "Roboto", "Noto Sans"])
    readonly property string iconFamily: "Material Symbols Rounded"
    readonly property bool variableFont: fontFamily === "Google Sans"

    // Оси вариативного Google Sans: wght 400..700, opsz 17..18 (17 — текстовый крой для мелкого кегля).
    function fontAxes(size, weight) {
        if (!variableFont)
            return {}
        return { "wght": Math.max(400, Math.min(700, weight)), "opsz": size < 16 ? 17 : 18 }
    }

    function _pickFamily(candidates) {
        const available = Qt.fontFamilies()
        for (const f of candidates)
            if (available.indexOf(f) >= 0)
                return f
        return Qt.application.font.family
    }

    // size / lineHeight / weight / letterSpacing (px)
    readonly property real fontScale: mobile ? MobileShell.fontScale : 1
    readonly property var type: Typography.scaled(_baseType, sp =>
        root.mobile && root.fontScale !== 1 ? MobileShell.textSize(sp) : sp)
    readonly property var _baseType: ({
        displayLarge:   { size: 57, line: 64, weight: 400, tracking: -0.25 },
        displayMedium:  { size: 45, line: 52, weight: 400, tracking: 0 },
        displaySmall:   { size: 36, line: 44, weight: 400, tracking: 0 },
        headlineLarge:  { size: 32, line: 40, weight: 400, tracking: 0 },
        headlineMedium: { size: 28, line: 36, weight: 400, tracking: 0 },
        headlineSmall:  { size: 24, line: 32, weight: 400, tracking: 0 },
        titleLarge:     { size: 22, line: 28, weight: 400, tracking: 0 },
        titleMedium:    { size: 16, line: 24, weight: 500, tracking: 0.15 },
        titleSmall:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
        bodyLarge:      { size: 16, line: 24, weight: 400, tracking: 0.5 },
        bodyMedium:     { size: 14, line: 20, weight: 400, tracking: 0.25 },
        bodySmall:      { size: 12, line: 16, weight: 400, tracking: 0.4 },
        labelLarge:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
        labelMedium:    { size: 12, line: 16, weight: 500, tracking: 0.5 },
        labelSmall:     { size: 11, line: 16, weight: 500, tracking: 0.5 }
    })

    function trackRowHeight(compact) {
        return Math.max(compact ? 56 : 64, type.bodyLarge.line + type.bodyMedium.line + (compact ? 12 : 20))
    }

    // ---- Формы (радиусы скругления) -----------------------------------------
    readonly property QtObject shape: QtObject {
        readonly property real none: 0
        readonly property real extraSmall: 4
        readonly property real small: 8
        readonly property real medium: 12
        readonly property real large: 16
        readonly property real largeIncreased: 20
        readonly property real extraLarge: 28
        readonly property real extraLargeIncreased: 32
        readonly property real extraExtraLarge: 48
        readonly property real full: 9999
    }

    // ---- Отступы -------------------------------------------------------------
    readonly property QtObject space: QtObject {
        readonly property real xs: 4
        readonly property real s: 8
        readonly property real m: 12
        readonly property real l: 16
        readonly property real xl: 24
        readonly property real xxl: 32
    }

    // ---- Motion --------------------------------------------------------------
    // Пружины MD3 Expressive аппроксимированы кривыми с перелётом (OutBack),
    // эффекты (цвет, прозрачность) — без перелёта.
    readonly property QtObject motion: QtObject {
        // Телефон: короткий отклик, спокойное перемещение; для больших раскрытий — отдельный токен.
        readonly property int press: root.reducedMotion ? 0 : 100
        readonly property int spatialFast: root.reducedMotion ? 0 : root.mobile ? 200 : 300
        readonly property int spatialDefault: root.reducedMotion ? 0 : root.mobile ? 280 : 420
        readonly property int spatialSlow: root.reducedMotion ? 0 : root.mobile ? 380 : 550
        readonly property int playerReveal: root.reducedMotion ? 0 : root.mobile ? 440 : 660
        readonly property int effectsFast: root.mobile ? 110 : 130
        readonly property int effectsDefault: root.mobile ? 150 : 170
        readonly property int effectsSlow: root.mobile ? 200 : 250
        readonly property real overshoot: root.mobile ? 0.6 : 1.25

        readonly property var emphasized: [0.2, 0.0, 0.0, 1.0, 1.0, 1.0]
        // ровное перемещение «из точки в точку» (плеер → мини-плеер): мягкий разгон и мягкое торможение,
        // без рывка в середине, как у emphasized
        readonly property var smoothMove: [0.4, 0.0, 0.2, 1.0, 1.0, 1.0]
        readonly property var emphasizedDecelerate: [0.05, 0.7, 0.1, 1.0, 1.0, 1.0]
        readonly property var emphasizedAccelerate: [0.3, 0.0, 0.8, 0.15, 1.0, 1.0]
        readonly property var standard: [0.2, 0.0, 0.0, 1.0, 1.0, 1.0]
        // Пружины MD3 Expressive в виде кривых (токены motion-scheme expressive): быстрая — для мелких
        // элементов (индикатор, иконка, чип), обычная — для крупных; эффекты — без перелёта
        readonly property var springFast: [0.42, 1.67, 0.21, 0.90, 1.0, 1.0]
        readonly property var springDefault: [0.38, 1.21, 0.22, 1.00, 1.0, 1.0]
        readonly property var effects: [0.31, 0.94, 0.34, 1.00, 1.0, 1.0]
    }
}
