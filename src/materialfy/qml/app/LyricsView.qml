import QtQuick
import QtQuick.Effects
import Md3
import Materialfy.Core

// Синхронный текст играющего трека с заливкой по словам (порт виджета Word Lyrics для DMS).
// Активная строка держится на высоте anchorPos; при смене строки текст едет каскадом:
// строки ниже трогаются чуть позже. Неактивные строки приглушены и размыты, паузы — «• • •».
// Клик по строке — перемотка к ней. Колесо, тачпад и перетаскивание прокручивают текст вручную:
// слежение за строкой встаёт на паузу и возвращается через resumeMs или кнопкой «К текущей строке».
Item {
    id: root

    // Базовый размер задаёт место вызова (полный плеер, боковая панель); настройка «Размер текста»
    // масштабирует его. Всё остальное (высота строки, отступы, фигуры пауз) считается от fontSize.
    property real baseFontSize: 34
    property real fontSize: baseFontSize * Settings.lyricsFontScale
    property real anchorPos: spicy ? 0.36 : 0.33
    // Подложка под текстом: "none" — прозрачно; "soft" | "strong" — затемнение в цвет темы
    readonly property string scrimMode: Settings.lyricsScrim
    readonly property real scrimAlpha: scrimMode === "strong" ? 0.6 : (scrimMode === "soft" ? 0.28 : 0)

    // Стиль отображения:
    // "normal" — классический вид: каскад, размытые соседние строки, выравнивание влево;
    // "spicy" — выразительная волна по словам с широким градиентным заполнением и мягким свечением;
    // "karaoke" — караоке: крупная строка с чёткой заливкой по словам и плавным переходом между строками.
    readonly property string mode: Settings.lyricsMode
    readonly property bool spicy: mode === "spicy"
    readonly property bool karaoke: mode === "karaoke"

    // Параметры внешнего вида и поведения: настройки переопределяют значения по умолчанию стиля
    readonly property string align: Settings.lyricsTextAlign
    readonly property real blurAmount: Settings.lyricsBlurAmount
    readonly property bool blurInactive: blurAmount > 0.001
    readonly property string linesMode: Settings.lyricsVisibleLines
    readonly property string fillMode: Settings.lyricsFillMode
    readonly property bool motion: Settings.lyricsAnimate && !Theme.reducedMotion

    // Ручные настройки акцента строки, амплитуды волны слов и прозрачности соседних строк
    // (с безопасным запасом по умолчанию, если мост Settings собран без новых ключей).
    readonly property real userActiveScale: Settings.lyricsActiveScale ?? 0
    readonly property real activeScale: userActiveScale >= 1.0
        ? userActiveScale
        : (spicy ? 1.12 : (karaoke ? 1.16 : 1.0))
    readonly property real baseIdleScale: spicy ? 0.88 : (karaoke ? 0.90 : 0.94)
    readonly property real idleScale: Settings.lyricsIdleScale > 0
        ? Math.min(activeScale, Settings.lyricsIdleScale)
        : baseIdleScale
    readonly property real wordBounce: Settings.lyricsWordBounce ?? 1.0
    readonly property real dimAlpha: Settings.lyricsDimOpacity ?? (spicy ? 0.34 : 0.35)
    readonly property real baseSpacingScale: spicy ? 1.12 : (karaoke ? 1.08 : 1.0)
    readonly property real spacingScale: baseSpacingScale * Settings.lyricsLineSpacing
    readonly property real highlightMultiplier: Settings.lyricsHighlight
    property bool showSource: Settings.lyricsShowSource

    // Число видимых строк вокруг активной: "one"/"1" → 0, "three"/"3" → 1, "all"/"15" → все,
    // либо любое число N из ползунка «Видимые строки» (показывает N строк вокруг текущей).
    readonly property int maxVisibleDist: {
        const s = String(linesMode ?? "all").trim()
        if (s === "all" || s === "")
            return 999
        if (s === "one")
            return 0
        if (s === "three")
            return 1
        const n = parseInt(s, 10)
        if (!isNaN(n) && n >= 1)
            return n >= 15 ? 999 : Math.floor((n - 1) / 2)
        return 999
    }

    property real fadeTopPct: 0.22
    property real fadeBottomPct: 0.18
    property int scrollDuration: !Settings.lyricsAnimate ? 0 : (Settings.lyricsScrollMs >= 0 ? Settings.lyricsScrollMs : (spicy ? 420 : (karaoke ? 440 : 600)))
    property int scrollStagger: !Settings.lyricsAnimate ? 0 : (Settings.lyricsStaggerMs >= 0 ? Settings.lyricsStaggerMs : (spicy ? 28 : (karaoke ? 26 : 42)))
    // Плавное упреждение прокрутки: строка мягко доезжает к anchorPos и увеличивается к началу первой ноты,
    // а подсветка и заливка идут точно по синхронизированным часам аудио (см. tick()).
    property int scrollLeadMs: 240
    readonly property int scrollAheadMs: !Settings.lyricsAnimate ? 0 : (spicy ? 240 : (karaoke ? 250 : Math.max(scrollLeadMs, Math.round(scrollDuration * 0.45))))
    property color textColor: Theme.fgSurface
    property int resumeMs: 3000

    // Превью в настройках — тот же вид, но со своими строками и часами; обычный путь не меняется.
    property bool previewMode: false
    property var previewLines: []
    property real previewPos: 0
    readonly property var lines: previewMode ? previewLines : Lyrics.lines
    // Трек уже сменился, а текст ещё от прошлого: гасим и не обрабатываем (иначе позиция 0 запускает
    // каскад прокрутки по всем строкам старого текста — несколько тяжёлых кадров подряд).
    readonly property bool stale: !previewMode && Lyrics.trackId !== Player.trackId
    readonly property bool live: visible && !stale && lines.length > 0
    onLiveChanged: if (live && !previewMode) { jumpTimer.restart(); sync(Player.positionMs / 1000); tick() }

    // Шрифт изначально растеризуется в полном размере активной строки (fontSize * activeScale):
    // тогда у активной строки масштаб слоя равен ровно 1.0 (100% нативная резкость пикселей без мыла),
    // а неактивные строки плавно уменьшаются через GPU Scale (idleScale / activeScale) без перестройки раскладки!
    readonly property real activeFontPx: Math.round(fontSize * activeScale)
    readonly property real rowH: activeFontPx * 1.22
    readonly property real lineGap: fontSize * 0.56 * root.spacingScale
    readonly property real liftPx: fontSize * 0.075 * root.wordBounce
    readonly property real sidePad: Math.max(12, fontSize * 0.4)
    readonly property var scrollBezier: [0.22, 1.0, 0.36, 1.0, 1.0, 1.0]
    readonly property url fillShader: Qt.resolvedUrl("shaders/wordfill.frag.qsb")

    property real pos: 0
    property int activeIndex: -1
    // Forces a re-layout of the words: the preview sets its lines before the delegates exist,
    // and the layout binding cannot see their implicit widths until something wakes it.
    property int layoutRevision: 0
    property int _targetScrollIndex: -1
    readonly property int scrollIndex: _targetScrollIndex

    signal shifted(real dy)

    onLinesChanged: {
        jumpTimer.restart()      // новый текст встаёт на место без анимации
        following = true
        manualAnim.running = false
        activeIndex = -1
        _targetScrollIndex = -1
        rebuild()
        tick()
    }

    // ── строки ──────────────────────────────────────────────────────────────
    // Текст на 40–60 строк — это сотни элементов-слов. Созданные разом (да ещё в двух представлениях:
    // плеер и боковая панель), они давали провал ~300 мс при каждой смене трека. Поэтому строки
    // строятся только в видимом представлении и порциями по несколько штук за кадр.
    property var _lines: []                // текст, по которому построены делегаты
    property var _pending: null            // новый текст ждёт, пока разберутся строки старого
    property int _built: 0
    property bool _tearing: false
    readonly property int buildChunk: 3

    // Разборка старых строк тоже порциями (разом — ~50 мс). Делегаты привязаны к _lines, поэтому
    // новый текст подставляется только после того, как старые строки убраны.
    function rebuild() {
        _pending = lines
        if (lineModel.count > 0)
            _tearing = true
        else
            _adopt()
        builder.running = true
    }
    function _adopt() {
        _lines = _pending ?? []
        _pending = null
        _built = 0
    }
    onVisibleChanged: if (visible && _built < _lines.length) builder.running = true

    // Трек сменился — старые строки можно убирать, не дожидаясь нового текста (он уже погас)
    onStaleChanged: if (stale) staleTeardown.restart()
    Timer {
        id: staleTeardown
        interval: 180
        onTriggered: {
            if (root.stale && lineModel.count > 0) {
                root._tearing = true
                builder.running = true
            }
        }
    }

    ListModel { id: lineModel }
    FrameAnimation {
        id: builder
        running: false
        onTriggered: {
            if (root._tearing) {
                const n = Math.min(lineModel.count, root.buildChunk * 2)
                if (n > 0)
                    lineModel.remove(lineModel.count - n, n)
                if (lineModel.count === 0) {
                    root._tearing = false
                    if (root._pending !== null) {
                        root._adopt()
                    } else if (!root.stale && root.lines.length > 0) {
                        root._lines = root.lines
                        root._built = 0
                    } else {
                        root._lines = []
                        root._built = 0
                    }
                }
                return
            }
            if (!root.visible || root._built >= root._lines.length) {
                running = false
                return
            }
            const until = Math.min(root._lines.length, root._built + root.buildChunk)
            for (; root._built < until; root._built++)
                lineModel.append({ line: root._built })
            jumpTimer.restart()               // пока строится — без каскада прокрутки
            if (root._built >= root._lines.length) {
                running = false
                // Слова существуют только теперь: будим раскладку, иначе все они стоят в x=0.
                root.layoutRevision++
                root.tick()
            }
        }
    }

    // ── синхронизация с аудио (PLL) ──────────────────────────────────────────
    // Плеер присылает позицию несколько раз в секунду; между отсчётами часы идут непрерывно с
    // учётом скорости воспроизведения. Ошибка подмешивается low-pass-фильтром (τ = 300 мс), как
    // в Spicy Lyrics, а не множителем скорости на каждом отсчёте: при редком шаге коррекция
    // скоростью перелетала цель, и текст шёл то раньше, то позже. Большой скачок (перемотка,
    // смена трека) ставит часы сразу. syncLeadSec (80 мс) — упреждение восприятия и кадров.
    property real _basePos: 0
    property real _baseTime: 0
    property real _posOffset: 0
    property real _measuredPos: 0
    property real _measuredAt: 0
    property bool _measured: false
    property real _lastTick: 0
    // Spicy Lyrics uses a 100 ms forward lead for the same reason; 80 ms leaves room
    // for the audio device buffer without pushing the sweep visibly ahead.
    property real syncLeadSec: 0.08
    readonly property real clockTau: 0.30
    property real _seekTarget: 0
    property real _seekStartMs: 0
    property real _seekGuardUntil: 0
    // пока говорит DJ, трек стоит на месте: позиция не идёт, хотя плеер «играет».
    // То же, пока трек ещё грузится: «играет» в это время оптимистично, а звука нет.
    readonly property bool advancing: Player.playing && !Player.narrating && !Player.buffering
    property bool _wasPlaying: advancing
    onAdvancingChanged: if (!previewMode) resyncPlayback()

    function rawPlayerPos() {
        if (!_wasPlaying)
            return _basePos
        const dt = Math.max(0, (Date.now() - _baseTime) / 1000)
        const spd = Settings.playbackSpeed > 0 ? Settings.playbackSpeed : 1.0
        return _basePos + dt * spd
    }
    function playerPos() {
        return Math.max(0, rawPlayerPos() + _posOffset + (_wasPlaying ? syncLeadSec : 0))
    }
    function sync(p) {
        _basePos = Math.max(0, p)
        _baseTime = Date.now()
        _posOffset = 0
        _measured = false
        _lastTick = 0
    }

    // Пауза/буферизация: возвращаемся к измеренной позиции, а не к замороженной
    function resyncPlayback() {
        if (root.advancing && !root._wasPlaying)
            root.sync(Player.positionMs / 1000)
        else
            root.sync(root.rawPlayerPos())
        root._wasPlaying = root.advancing
    }

    Connections {
        target: Player
        function onPositionChanged() {
            if (!root.live || root.previewMode)
                return
            const now = Date.now()
            const measured = Player.positionMs / 1000
            // После клика по строке плеер может ещё до ~1 с присылать старую позицию до перемотки
            if (now < root._seekGuardUntil && Math.abs(measured - root._seekTarget) > 0.35) {
                if (now - root._seekStartMs < 1000)
                    root._seekGuardUntil = now + 180
                return
            }
            if (Math.abs(measured - (root.rawPlayerPos() + root._posOffset)) > 0.5) {
                // Перемотка, смена трека или большой провал: часы ставятся сразу
                root.sync(measured)
            } else {
                root._measuredPos = measured
                root._measuredAt = now
                root._measured = true
            }
            root.tick()
        }
        function onStateChanged() { if (!root.previewMode) root.resyncPlayback() }
    }

    Connections {
        target: Settings
        function onLyricsSyncMsChanged() { root.tick() }
    }

    // Ошибку измеренной позиции подмешиваем в часы плавно: между отсчётами плеера
    // позиция экстраполируется, а не догоняется скачком скорости.
    function tickClock() {
        const now = Date.now()
        if (_measured && advancing) {
            const spd = Settings.playbackSpeed > 0 ? Settings.playbackSpeed : 1.0
            const measuredNow = _measuredPos + Math.max(0, (now - _measuredAt) / 1000) * spd
            const err = measuredNow - (rawPlayerPos() + _posOffset)
            const dt = _lastTick > 0 ? Math.min(0.1, Math.max(0, (now - _lastTick) / 1000)) : 0.016
            _posOffset += err * (1 - Math.exp(-dt / clockTau))
        }
        _lastTick = now
    }

    function tick() {
        if (root.previewMode) {
            pos = root.previewPos
        } else {
            tickClock()
            pos = playerPos() + Settings.lyricsSyncMs / 1000
        }
        const L = lines

        // Подсветка и заливка идут ровно по синхронизированной позиции: активна всегда строго одна текущая строка
        const at = pos
        let i = activeIndex
        if (i >= L.length || (i >= 0 && L[i].t > at) || i < -1) {
            let lo = 0, hi = L.length - 1
            i = -1
            while (lo <= hi) {
                const mid = (lo + hi) >> 1
                if (L[mid].t <= at) { i = mid; lo = mid + 1 } else hi = mid - 1
            }
        } else {
            while (i + 1 < L.length && L[i + 1].t <= at)
                i++
        }
        if (i !== activeIndex)
            activeIndex = i

        // Упреждение прокрутки и подготовки входящей строки: колонка и масштаб начинают движение чуть заранее
        const atScroll = pos + scrollAheadMs / 1000
        let si = _targetScrollIndex
        if (si >= L.length || (si >= 0 && L[si].t > atScroll) || si < -1) {
            let lo = 0, hi = L.length - 1
            si = -1
            while (lo <= hi) {
                const mid = (lo + hi) >> 1
                if (L[mid].t <= atScroll) { si = mid; lo = mid + 1 } else hi = mid - 1
            }
        } else {
            while (si + 1 < L.length && L[si + 1].t <= atScroll)
                si++
        }
        if (si !== _targetScrollIndex)
            _targetScrollIndex = si
    }

    // Волна по словам считается от позиции в каждом кадре: на общих 30 Гц она шла бы ступеньками
    // на мониторах 120–240 Гц. Программный рендер (lowPower) — по общим часам Ticker (~30 Гц).
    FrameAnimation {
        running: root.live && !root.previewMode && root.advancing && !Theme.lowPower
        onTriggered: root.tick()
    }
    TickAnimation {
        running: root.live && !root.previewMode && root.advancing && Theme.lowPower
        onTriggered: root.tick()
    }

    function seekTo(t) {
        // строки показаны со сдвигом синхронизации — перематываем туда, где строка и правда звучит
        const at = Math.max(0, t + 0.02 - Settings.lyricsSyncMs / 1000)
        const now = Date.now()
        _seekTarget = at
        _seekStartMs = now
        _seekGuardUntil = now + 350
        Player.seekMs(Math.round(at * 1000))
        sync(at)
        tick()
    }

    function wordProgress(w, p) {
        if (p <= w.s) return 0
        if (p >= w.e) return 1
        if (!w.p) return (p - w.s) / Math.max(0.001, w.e - w.s)
        let total = 0, done = 0
        for (const part of w.p) total += part[0]
        for (const part of w.p) {
            if (p >= part[2]) done += part[0]
            else if (p > part[1]) done += part[0] * (p - part[1]) / Math.max(0.001, part[2] - part[1])
        }
        return Math.min(1, done / total)
    }

    function easeOut(x) {
        return 1 - Math.pow(1 - Math.max(0, Math.min(1, x)), 3)
    }

    TextMetrics {
        id: spaceMetrics
        font.family: Theme.fontFamily
        font.pixelSize: root.activeFontPx
        font.variableAxes: Theme.fontAxes(root.activeFontPx, 700)
        text: " "
    }

    // ── нет текста ──────────────────────────────────────────────────────────
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 32, 380)
        visible: !root.previewMode && !root.stale && root.lines.length === 0 && Lyrics.status !== "loading"
        icon: "title"
        shape: "flower6"
        title: !Player.hasTrack ? I18n.t("Nothing's playing")
             : Player.isEpisode ? I18n.t("Episodes don't have lyrics")
             : Lyrics.status === "error" ? I18n.t("Lyrics didn't load")
             : I18n.t("No lyrics")
        text: !Player.hasTrack ? I18n.t("Lyrics for the playing track will appear here")
            : Player.isEpisode ? I18n.t("Only songs have lyrics")
            : Lyrics.status === "error" ? I18n.t("Sources didn't respond — I'll try again on the next track")
            : I18n.t("No source has lyrics for this track")
    }
    LoadingIndicator {
        anchors.centerIn: parent
        visible: !root.previewMode && (Lyrics.status === "loading" || (root.stale && Player.hasTrack))
    }

    // ── подложка под текст ──────────────────────────────────────────────────
    Rectangle {
        id: scrimRect
        anchors.fill: parent
        anchors.topMargin: 8
        anchors.bottomMargin: 8
        anchors.leftMargin: Math.max(4, root.sidePad * 0.25)
        anchors.rightMargin: Math.max(4, root.sidePad * 0.25)
        radius: Theme.shape.extraLarge
        color: Theme.surface
        opacity: root.lines.length > 0 && !root.stale ? root.scrimAlpha : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root.motion ? 260 : 0 } }
    }

    // ── текст ───────────────────────────────────────────────────────────────
    Item {
        id: viewport
        anchors.fill: parent
        clip: true
        visible: root.lines.length > 0 && opacity > 0
        opacity: root.stale || root._tearing ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: 160 } }

        // Края растворяются в прозрачность (а не в цвет фона — под текстом бывает обложка). На программном
        // рендере — без слоя, края просто обрезаны: слой с меняющимся текстом удваивает кадры (Qt просит
        // следующий после каждого изменения содержимого) и сам стоит лишнего прохода по всему тексту
        layer.enabled: !Theme.lowPower
        layer.effect: ShaderEffect {
            property real fadeTop: root.fadeTopPx / Math.max(1, viewport.height)
            property real fadeBottom: root.fadeBottomPx / Math.max(1, viewport.height)
            property real cut: 0
            fragmentShader: Qt.resolvedUrl("shaders/edgefade.frag.qsb")
        }

        Column {
            id: col
            x: root.sidePad
            width: parent.width - root.sidePad * 2
            spacing: 0

            Repeater {
                id: lineRep
                model: lineModel
                delegate: LyricLine {}
            }
        }

        WheelHandler {
            target: null
            enabled: !root.previewMode
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            orientation: Qt.Vertical
            property real fineUntil: 0   // тачпад Windows: доли щелчка — сразу, без доводки (см. SmoothScroll)
            onWheel: event => {
                // тачпад узнаём по фазе жеста (см. SmoothScroll): пиксели 1:1, колесо — шагами с доводкой
                if (event.phase !== Qt.NoScrollPhase) {
                    root.userScroll(event.pixelDelta.y, false)
                } else {
                    const a = Math.abs(event.angleDelta.y)
                    const now = Date.now()
                    if (a % 120 !== 0)
                        fineUntil = now + 400
                    const notches = event.angleDelta.y !== 0 ? event.angleDelta.y / 120 : event.pixelDelta.y / 15
                    root.userScroll(notches * Settings.wheelStep, now >= fineUntil)
                }
            }
        }
        DragHandler {
            id: dragger
            target: null
            enabled: !root.previewMode
            xAxis.enabled: false
            property real _last: 0
            onActiveChanged: _last = 0
            onTranslationChanged: {
                if (!active)
                    return
                root.userScroll(translation.y - _last, false)
                _last = translation.y
            }
        }
    }

    Button {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 20
        z: 2
        size: "xs"
        style: "tonal"
        icon: "lyrics"
        text: I18n.t("Back to current line")
        visible: opacity > 0
        opacity: !root.previewMode && !root.following && root.lines.length > 0 && !root.stale ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        onClicked: root.resumeFollowing()
    }

    Label {
        visible: root.showSource && root.lines.length > 0 && Lyrics.source !== ""
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 4
        text: Lyrics.kind === "word" ? I18n.t("{0} · word by word", Lyrics.source) : I18n.t("{0} · line by line", Lyrics.source)
        type: "labelSmall"
        color: Theme.fgSurfaceVariant
        opacity: 0.6
    }

    // Верхнее затухание не заходит на целевую строку (используем scrollIndex, как и colTargetY)
    readonly property real fadeTopPxTarget: {
        const want = viewport.height * fadeTopPct
        const idx = Math.max(0, scrollIndex)
        const it = lineRep.count > idx ? lineRep.itemAt(idx) : null
        if (!it || !following)
            return want
        return Math.max(0, Math.min(want, colTargetY + restY(idx) - lineGap * 0.25))
    }
    readonly property real fadeTopPx: fadeTopPxTarget
    readonly property real fadeBottomPx: viewport.height * fadeBottomPct

    // Прокрутка: колонка сразу встаёт в цель, а каждая строка получает компенсирующий сдвиг,
    // который уходит в 0 со своей задержкой. Цель — «устоявшаяся» раскладка: раскрывающаяся
    // пауза двигает только строки под собой, а не весь текст туда-обратно.
    function restY(idx) {
        const it = lineRep.itemAt(idx)
        if (!it)
            return 0
        let y = it.y
        for (let j = 0; j < idx; j++) {
            const g = lineRep.itemAt(j)
            if (g && g.isGap)
                y += g.restHeight - g.height
        }
        return y
    }

    readonly property real colTargetY: {
        const idx = Math.max(0, scrollIndex)
        const it = lineRep.count > idx ? lineRep.itemAt(idx) : null
        if (!it)
            return viewport.height * anchorPos
        return viewport.height * anchorPos - (restY(idx) + it.restHeight / 2)
    }

    property int _scrollIndex: -2
    onColTargetYChanged: Qt.callLater(applyScroll)

    function applyScroll() {
        if (!following)
            return
        const dy = col.y - colTargetY
        col.y = colTargetY
        _scrollIndex = scrollIndex
        if (!jumpTimer.running && Math.abs(dy) > 0.5)
            shifted(dy)
    }

    Timer { id: jumpTimer; interval: 250 }

    // ── ручная прокрутка ────────────────────────────────────────────────────
    // Пока пользователь листает, колонка стоит там, куда её увели (col.y), а активная строка
    // только подсвечивается; возврат — тем же каскадом строк, что и обычная смена строки.
    property bool following: true
    property real _manualTarget: 0
    readonly property real manualMaxY: viewport.height * anchorPos
    readonly property real manualMinY: viewport.height * anchorPos - col.height

    function _clampManual(y) { return Math.max(manualMinY, Math.min(manualMaxY, y)) }
    function userScroll(dy, smooth) {
        if (lines.length === 0)
            return
        if (following) {
            following = false
            _manualTarget = col.y
        }
        const base = smooth && manualAnim.running ? _manualTarget : col.y
        _manualTarget = _clampManual(base + dy)
        if (smooth) {
            manualAnim.running = true
        } else {
            manualAnim.running = false
            col.y = _manualTarget
        }
        resumeTimer.restart()
    }
    function resumeFollowing() {
        resumeTimer.stop()
        manualAnim.running = false
        if (following)
            return
        following = true
        const dy = col.y - colTargetY
        col.y = colTargetY
        _scrollIndex = scrollIndex
        if (Math.abs(dy) > 0.5)
            shifted(dy)
    }

    Timer {
        id: resumeTimer
        interval: root.resumeMs
        onTriggered: if (!dragger.active) root.resumeFollowing(); else restart()
    }

    // доводка колеса — как в SmoothScroll: экспонента, целые пиксели
    FrameAnimation {
        id: manualAnim
        running: false
        onTriggered: {
            const d = root._manualTarget - col.y
            if (Math.abs(d) < 0.5) {
                col.y = root._manualTarget
                running = false
                return
            }
            const step = d * (1 - Math.exp(-frameTime * 14))
            col.y = Math.round(col.y + (Math.abs(step) < 1 ? Math.sign(d) : step))
        }
    }

    // ── строка ──────────────────────────────────────────────────────────────
    component LyricLine: Item {
        id: line

        required property int index
        readonly property var modelData: root._lines[index] ?? ({ gap: true, t: 0, e: 0 })

        readonly property bool isGap: !!modelData.gap
        readonly property bool isActive: index === root.activeIndex
        // Активна всегда строго одна строка (isActive): две строки никогда не поют одновременно.
        // Входящая строка начинает плавно раскрываться в масштабе и чёткости синхронно с упреждением прокрутки
        // (isTarget), а уходящая строка сохраняет шейдер и плавно затухает в течение 340 мс после смены строки
        // (outgoingTailSec), чтобы переход между строками в Караоке, Spicy и Классике был идеально мягким.
        readonly property bool filling: !isGap && isActive
        readonly property real leadSec: Math.max(0.14, root.scrollAheadMs / 1000)
        readonly property bool isTarget: !isGap && (isActive || (root.following && index === root.activeIndex + 1 && ((modelData.t ?? 0) - root.pos) <= leadSec))
        readonly property real outgoingElapsed: (root.activeIndex >= 0 && index === root.activeIndex - 1)
            ? Math.max(0, root.pos - (root.lines[root.activeIndex]?.t ?? modelData.e ?? 0))
            : 999
        readonly property bool isOutgoing: !isGap && outgoingElapsed < 0.34
        readonly property bool wordsAnimating: !isGap && (isActive || isTarget || isOutgoing)
        readonly property int dist: root.activeIndex < 0 ? index + 1 : Math.abs(index - root.activeIndex)
        readonly property real gapProgress: isGap && isActive ? Math.max(0, Math.min(1, (root.pos - (modelData.t ?? 0)) / Math.max(0.1, (modelData.e ?? 0) - (modelData.t ?? 0)))) : 0
        readonly property bool hovered: hover.hovered

        // И в Spicy, и в Караоке, и при ручном увеличении масштаба строки шрифт всегда отрендерен
        // в полном размере activeFontPx (масштаб 1.0 на активной строке = 100% чёткость без мыла),
        // а неактивные строки мягко уменьшены до idleNormScale через GPU Scale без перестройки текста!
        readonly property real idleNormScale: root.idleScale / Math.max(1.0, root.activeScale)
        readonly property real targetScale: !isGap ? (isTarget ? 1.0 : idleNormScale) : 1.0
        property real lineScale: targetScale
        Behavior on lineScale {
            enabled: root.motion && !Theme.lowPower
            NumberAnimation {
                duration: root.karaoke ? 380 : (root.spicy ? 320 : 300)
                easing.type: Easing.BezierSpline
                easing.bezierCurve: root.scrollBezier
            }
        }
        readonly property real lineSize: root.activeFontPx
        readonly property real rowH: root.rowH
        // Bumped as each word delegate appears: the layout needs their real widths.
        property int layoutRevision: 0

        // Фильтр видимости строк (ползунок от 1 до 15 / Все)
        readonly property bool inLineRange: {
            if (root.maxVisibleDist >= 999)
                return true
            return root.activeIndex < 0 ? index <= root.maxVisibleDist : dist <= root.maxVisibleDist
        }

        width: col.width
        // Свёрнутая пауза сохраняет волосок высоты: Column не расставляет элементы нулевого размера.
        // Раскрывается она вместе с прокруткой (scrollIndex), чтобы колонка не дёргалась.
        readonly property real restHeight: isGap
            ? ((isActive || index === root.scrollIndex) ? root.rowH * 1.1 + root.lineGap : 0.01)
            : wordsBox.height + root.lineGap
        height: restHeight
        clip: isGap

        Behavior on height {
            enabled: line.isGap
            SequentialAnimation {
                PauseAnimation { duration: Math.max(0, Math.min(7, line.index - Math.max(0, root.scrollIndex) + 1)) * root.scrollStagger }
                NumberAnimation { duration: root.scrollDuration; easing.type: Easing.BezierSpline; easing.bezierCurve: root.scrollBezier }
            }
        }

        // Появление нового текста: строки выплывают снизу волной, начиная от активной.
        // Непопадающие в диапазон строки мягко растворяются до 0, сохраняя раскладку.
        property real appear: 0
        property real rangeOpacity: inLineRange ? 1 : 0
        Behavior on rangeOpacity {
            NumberAnimation { duration: root.motion ? 260 : 0; easing.type: Easing.OutCubic }
        }
        opacity: appear * rangeOpacity
        transform: [
            Translate { id: shift },
            Translate { y: (1 - line.appear) * root.fontSize * 1.1 }
        ]
        Component.onCompleted: Qt.callLater(() => {
            const rel = line.index - Math.max(0, root.activeIndex - 2)
            if (rel < 0 || rel > 14) {
                line.appear = 1
                return
            }
            appearAnim.delayMs = root.motion ? (root.spicy ? 60 + rel * 35 : 110 + rel * 50) : 0
            appearAnim.start()
        })
        SequentialAnimation {
            id: appearAnim
            property int delayMs: 0
            PauseAnimation { duration: appearAnim.delayMs }
            NumberAnimation {
                target: line
                property: "appear"
                to: 1
                duration: root.motion ? (root.spicy ? 480 : 620) : 0
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Theme.motion.emphasizedDecelerate
            }
        }

        Connections {
            target: root
            function onShifted(dy) {
                const wasMoving = scrollAnim.running
                scrollAnim.stop()
                const anchor = root.scrollIndex >= 0 ? root.scrollIndex : Math.max(0, root.activeIndex)
                const rel = line.index - anchor
                // строкам за экраном анимация не нужна
                const sy = col.y + line.y + shift.y + dy
                if (sy > viewport.height * 2 || sy + line.height < -viewport.height) {
                    shift.y = 0
                    return
                }
                shift.y += dy
                scrollAnim.delayMs = wasMoving ? 0 : Math.max(0, Math.min(7, rel + 1)) * root.scrollStagger
                scrollAnim.interrupted = wasMoving
                scrollAnim.start()
            }
        }

        SequentialAnimation {
            id: scrollAnim
            property int delayMs: 0
            property bool interrupted: false
            PauseAnimation { duration: scrollAnim.delayMs }
            NumberAnimation {
                target: shift
                property: "y"
                to: 0
                duration: root.scrollDuration
                easing.type: scrollAnim.interrupted ? Easing.OutCubic : Easing.BezierSpline
                easing.bezierCurve: root.scrollBezier
            }
        }

        Rectangle {
            x: -root.sidePad * 0.6
            y: -root.lineGap * 0.3
            width: line.width + root.sidePad * 1.2
            height: wordsBox.height + root.lineGap * 0.6
            radius: Theme.shape.large
            color: root.textColor
            opacity: line.hovered && !line.isGap ? 0.08 : 0
            Behavior on opacity { NumberAnimation { duration: 160 } }
        }

        Item {
            id: wordsBox
            visible: !line.isGap
            width: line.width
            height: line.isGap ? 0 : Math.max(1, layout.rows) * line.rowH

            // Плавное вертикальное скольжение при входе/выходе строки и в режиме «Вся строка»
            property real wholeLineLift: {
                if (!root.motion)
                    return 0
                if (line.filling && root.fillMode === "line")
                    return -root.liftPx * (root.spicy ? 0.85 : 0.60)
                if (root.karaoke && !line.isTarget && !line.filling)
                    return (root.activeIndex >= 0 && line.index < root.activeIndex) ? -root.fontSize * 0.045 : root.fontSize * 0.045
                return 0
            }
            Behavior on wholeLineLift {
                enabled: root.motion && !Theme.lowPower
                NumberAnimation {
                    duration: root.karaoke ? 360 : 260
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: root.scrollBezier
                }
            }

            transform: Translate {
                y: wordsBox.wholeLineLift
            }

            // Плавная прозрачность строки без скачков при включении/выключении шейдера слов:
            // wordsBox.opacity всегда следует плавной кривой fadeOpacity (0.35 ↔ 1.0),
            // а внутри шейдера неспетые части слов компенсируются через childDim = dimAlpha / fadeOpacity,
            // поэтому неспетые слова всегда сохраняют точную яркость root.dimAlpha без двойного затемнения,
            // а уходящая строка мягко гаснет от 1.0 к inactiveOpacity вместе с уменьшением масштаба!
            readonly property real inactiveOpacity: root.fillMode === "off"
                ? (line.hovered ? 0.92 : 0.78)
                : Math.max(
                    0.12,
                    root.dimAlpha * (line.hovered ? 1.85 : (1 - (root.spicy ? 0.11 : 0.09) * Math.max(0, line.dist - 1)))
                )
            property real fadeOpacity: (line.filling || (line.isTarget && root.fillMode === "word")) ? 1.0 : inactiveOpacity
            Behavior on fadeOpacity {
                NumberAnimation {
                    duration: root.motion ? (root.karaoke ? 320 : (root.spicy ? 230 : 280)) : 0
                    easing.type: Easing.OutCubic
                }
            }
            readonly property real childDim: Math.min(1.0, root.dimAlpha / Math.max(0.15, fadeOpacity))
            opacity: fadeOpacity

            // Неактивные строки размываются статически без анимации blur,
            // чтобы Qt отрисовывал FBO один раз и кэшировал текстуру.
            // Размытие сохраняется во время масштабирования, чтобы слой не мигал при переключении.
            readonly property real lineBlur: (root.spicy ? 0.34 : 0.28) * root.blurAmount
            readonly property bool blurred: root.blurInactive && line.inLineRange && line.dist < 6
                && !line.isTarget && !line.filling && !line.hovered
            layer.enabled: blurred
            layer.smooth: true
            layer.effect: MultiEffect {
                blurEnabled: true
                blurMax: root.spicy ? 18 : 16
                blur: wordsBox.lineBlur
                autoPaddingEnabled: true
            }

            // Перенос слов по строкам с поддержкой выравнивания влево, по центру и вправо.
            // Так как на активной строке lineScale == 1.0, доступная ширина равна полной ширине колонки!
            readonly property var layout: {
                const revision = root.layoutRevision + line.layoutRevision
                const n = wordRep.count
                const avail = Math.max(1, width)
                const pos = []
                const widths = []
                let x = 0, row = 0
                for (let i = 0; i < n; i++) {
                    const it = wordRep.itemAt(i)
                    const w = it ? it.implicitWidth : 0
                    const tw = it ? it.textWidth : 0
                    if (x > 0 && x + tw > avail) {
                        row++
                        x = 0
                    }
                    pos.push({ x: x, row: row })
                    x += w
                    widths[row] = x - (it && it.spaced ? Math.max(0, w - tw) : 0)
                }
                let maxW = 0
                for (let r = 0; r <= row; r++)
                    if ((widths[r] ?? 0) > maxW) maxW = widths[r]
                if (root.align === "center") {
                    for (let i = 0; i < pos.length; i++)
                        pos[i].x += Math.max(0, (maxW - (widths[pos[i].row] ?? 0)) / 2)
                } else if (root.align === "right") {
                    for (let i = 0; i < pos.length; i++)
                        pos[i].x += Math.max(0, maxW - (widths[pos[i].row] ?? 0))
                }
                return { pos: pos, rows: row + 1, lineWidth: maxW }
            }

            Item {
                id: lineWords
                x: root.align === "center"
                    ? Math.max(0, (wordsBox.width - (wordsBox.layout?.lineWidth ?? 0)) / 2)
                    : (root.align === "right" ? Math.max(0, wordsBox.width - (wordsBox.layout?.lineWidth ?? 0)) : 0)
                width: Math.max(1, wordsBox.layout?.lineWidth ?? 0)
                height: wordsBox.height

                transform: Scale {
                    origin.x: root.align === "center" ? lineWords.width / 2 : (root.align === "right" ? lineWords.width : 0)
                    origin.y: lineWords.height / 2
                    xScale: line.lineScale
                    yScale: line.lineScale
                }

                Repeater {
                    id: wordRep
                    model: line.isGap ? [] : line.modelData.w
                    onItemAdded: line.layoutRevision++
                    delegate: LyricWord {
                        lineActive: line.filling
                        lineOutgoing: line.isOutgoing
                        outgoingElapsed: line.outgoingElapsed
                        wordAnimating: line.wordsAnimating
                        shaderDimAlpha: wordsBox.childDim
                        // 1 — слово уже спели, 2 — в активной строке, 0 — ещё не пели
                        sungState: (root.activeIndex >= 0 && line.index < root.activeIndex) ? 1
                                 : (line.filling ? 2 : 0)
                        fontSize: line.lineSize
                        rowH: line.rowH
                        useWordShader: line.wordsAnimating && root.fillMode === "word"
                        x: wordsBox.layout.pos[index]?.x ?? 0
                        y: (wordsBox.layout.pos[index]?.row ?? 0) * line.rowH
                    }
                }
            }
        }

        // Пауза: три фигуры «дышат» и заполняются
        Row {
            visible: line.isGap
            anchors.verticalCenter: parent.verticalCenter
            anchors.verticalCenterOffset: -root.lineGap / 2
            x: root.align === "center"
                ? Math.max(0, (parent.width - width) / 2)
                : (root.align === "right" ? Math.max(0, parent.width - width - root.fontSize * 0.1) : root.fontSize * 0.1)
            spacing: root.fontSize * 0.24
            transformOrigin: Item.Center
            scale: {
                if (!line.isActive)
                    return 0.6
                const breathe = 1 + 0.09 * Math.sin(root.pos * Math.PI * 2 / 1.9)
                const tail = Math.max(0, Math.min(1, ((line.modelData.e ?? 0) - root.pos) / 0.45))
                return breathe * (0.35 + 0.65 * root.easeOut(tail))
            }
            opacity: line.isActive ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 300 } }

            // Вместо точек — три маленькие фигуры MD3E: «наливаются» по очереди и медленно вращаются
            Repeater {
                model: ["cookie4", "clover4", "cookie6"]
                MorphShape {
                    required property int index
                    required property string modelData
                    readonly property real fill: Math.max(0, Math.min(1, line.gapProgress * 3 - index))
                    width: root.fontSize * 0.6
                    height: width
                    shape: fill >= 1 ? "cookie9" : modelData
                    color: root.textColor
                    opacity: root.dimAlpha + (1 - root.dimAlpha) * fill
                    rotation: line.isActive ? root.pos * (index % 2 ? -40 : 50) + index * 30 : 0
                }
            }
        }

        // Обработчики, а не MouseArea: MouseArea забрала бы нажатие, и перетаскивание текста не началось бы
        HoverHandler {
            id: hover
            enabled: !root.previewMode && !line.isGap && line.inLineRange
            cursorShape: Qt.PointingHandCursor
        }
        TapHandler {
            enabled: !root.previewMode && !line.isGap && line.inLineRange
            onTapped: {
                root.seekTo(line.modelData.t)
                root.resumeFollowing()
            }
        }
    }

    // ── слово ───────────────────────────────────────────────────────────────
    component LyricWord: Item {
        id: word

        required property var modelData
        required property int index
        property bool lineActive: false
        property bool lineOutgoing: false
        property real outgoingElapsed: 999
        property bool wordAnimating: false
        property bool useWordShader: false
        property real shaderDimAlpha: root.dimAlpha
        property real fontSize: root.activeFontPx
        property real rowH: root.rowH

        readonly property string txt: modelData.x.replace(/\s+$/, "")
        readonly property bool spaced: /\s$/.test(modelData.x)
        readonly property real textWidth: label.implicitWidth
        readonly property real dur: Math.max(0.001, (modelData.e ?? 0) - (modelData.s ?? 0))
        readonly property bool held: dur >= 0.95 && txt.length <= 14

        // Spicy: долгое слово рисуется по буквам, каждая со сдвигом по времени поднимается, растёт и светится.
        // Только простые скрипты без составных знаков: для RTL, комбинируемых акцентов и эмодзи
        // раскладка по буквам ломает формирование и ширину — там остаётся обычный label.
        // Не меняется посреди слова, поэтому раскладка не прыгает.
        readonly property bool lettered: root.spicy && root.motion && !Theme.lowPower
            && root.fillMode === "word" && held && txt.length >= 3
            && /^[A-Za-z\u00C0-\u024F\u0400-\u04FF'\u2019-]+$/.test(txt)
        readonly property var letters: lettered ? Array.from(txt) : []
        // Сила растёт с длительностью (корень после 2 с, куб до неё), как в Spicy Lyrics
        readonly property real waveAmount: {
            const x = Math.max(1.0, dur) / 2.0
            return Math.min(1.2, (x > 1 ? Math.sqrt(x) : x * x * x) * 0.6)
        }
        // Импульс буквы i, 0..1..0: стартует внутри первых 40% слова и не переживает слово больше чем на 0.3 с
        function letterPulse(i, n) {
            const start = (modelData.s ?? 0) + (n > 1 ? i / (n - 1) : 0) * dur * 0.4
            const end = Math.min(start + Math.max(1.0, dur), (modelData.e ?? 0) + 0.3)
            if (end <= start)
                return 0
            const x = (root.pos - start) / (end - start)
            return (x <= 0 || x >= 1) ? 0 : Math.sin(Math.PI * x)
        }
        // На активной строке и во время 340 мс плавного выхода предыдущей строки (lineOutgoing)
        // заливка доходит до конца без мгновенного скачка при смене строки.
        readonly property real progress: {
            if (lineActive)
                return root.wordProgress(modelData, root.pos)
            if (lineOutgoing)
                return Math.max(root.wordProgress(modelData, root.pos), Math.min(1, outgoingElapsed / 0.18))
            return sungState === 1 ? 1 : 0
        }
        required property int sungState

        // Плавная кривая Эрмита (smoothstep) и трёхфазная огибающая (Attack → Sustain → Release)
        // в реальном времени (секундах), как пружины в Spicy Lyrics:
        // слово быстро всплывает за ~200 мс, держится на гребне волны, пока звучит нота, и мягко
        // опускается за ~220 мс после окончания, образуя непрерывную бегущую волну по соседним словам
        // во всех трёх стилях (Spicy, Караоке и Классика).
        function ease(x) { return x <= 0 ? 0 : (x >= 1 ? 1 : x * x * (3 - 2 * x)) }

        readonly property real envelope: {
            if (!wordAnimating || root.fillMode !== "word")
                return 0
            const s = (modelData.s ?? 0) - 0.015
            const e = (modelData.e ?? 0)
            const t = root.pos
            const attack = Math.min(0.22, Math.max(0.13, dur * 0.62))
            const fallStart = Math.max(e, s + attack)
            const release = root.karaoke ? 0.24 : 0.21
            if (t <= s)
                return 0
            if (t >= fallStart + release)
                return 0
            if (t < s + attack)
                return word.ease((t - s) / attack)
            if (t <= fallStart)
                return 1.0
            return 1.0 - word.ease((t - fallStart) / release)
        }

        readonly property bool alreadySung: sungState === 1 || ((lineActive || lineOutgoing) && root.pos >= (modelData.e ?? 0))

        // Масштаб слова: в Spicy (0.96 → 1.0 + 0.072*bounce → 1.00), в Караоке — мягкий импульс (1.0 → 1.032 → 1.0)
        readonly property real sungScale: {
            if (root.fillMode !== "word")
                return 1.0
            const env = word.envelope
            if (root.spicy) {
                const rest = word.alreadySung ? 1.0 : 0.96
                if (env <= 0.001)
                    return rest
                const peak = 1.0 + 0.072 * root.wordBounce
                const vibrato = (word.held && env > 0.85) ? 0.014 * root.wordBounce * Math.sin((root.pos - (modelData.s ?? 0)) * Math.PI * 4.5) : 0
                return rest + (peak - rest) * env + vibrato
            }
            if (root.karaoke) {
                return 1.0 + 0.032 * root.wordBounce * env
            }
            return 1.0 + 0.042 * word.glow * root.wordBounce
        }

        // Вертикальный подъём слова («волна»):
        // плавно поднимается по огибающей envelope во время звучания слова и мягко возвращается в 0
        // без резкого сброса в момент переключения на следующую строку.
        readonly property real sungLiftEm: {
            if (root.fillMode !== "word")
                return 0
            const env = word.envelope
            if (root.spicy) {
                const rest = word.alreadySung ? 0.0 : 0.014 * root.wordBounce
                if (env <= 0.001)
                    return rest
                const peak = -0.088 * root.wordBounce
                return rest + (peak - rest) * env
            }
            if (root.karaoke)
                return -0.055 * root.wordBounce * env
            return -0.048 * root.wordBounce * env
        }

        readonly property real glow: {
            if (!wordAnimating || root.fillMode !== "word")
                return 0
            if (root.spicy)
                return 0.90 * word.envelope * root.highlightMultiplier
            if (root.karaoke)
                return 0.25 * word.envelope * root.highlightMultiplier
            return (held ? Math.sin(Math.PI * progress) : 0) * root.highlightMultiplier
        }

        // Пробел на конце слова — из общей метрики, отмасштабированной под размер строки
        implicitWidth: label.implicitWidth + (spaced ? spaceMetrics.advanceWidth * (fontSize / Math.max(1, root.activeFontPx)) : 0)
        implicitHeight: rowH

        // Слой шейдера включается ТОЛЬКО во время прохождения заливки по слову или пока не угасло свечение.
        // До и после этого слово рисуется нативным текстом без FBO и без шейдера.
        readonly property bool sweeping: word.useWordShader
            && ((progress > 0 && progress < 1) || word.glow > 0.005)

        Text {
            id: label
            // Google Sans рисует ≈1.36em в высоту, а строка держит 1.22em: текстура слоя без запаса
            // срезала нижние выносные элементы. Добираем высоту паддингом — текст остаётся на месте.
            readonly property real layerOverscan: Math.ceil(word.fontSize * 0.25)
            height: word.rowH + 2 * layerOverscan
            topPadding: layerOverscan
            bottomPadding: layerOverscan
            anchors.verticalCenter: parent.verticalCenter
            verticalAlignment: Text.AlignVCenter
            text: word.txt
            font.family: Theme.fontFamily
            font.pixelSize: word.fontSize
            font.weight: Theme.variableFont ? Font.Normal : Font.Bold
            font.variableAxes: Theme.fontAxes(word.fontSize, 700)
            color: root.textColor
            // Неспетое слово на активной/выходящей строке затемняется нативно; спетое — полностью непрозрачно
            opacity: word.useWordShader ? (word.sweeping ? 1.0 : (word.progress >= 1 ? 1.0 : word.shaderDimAlpha)) : 1.0
            // невидимый, но по-прежнему меряет: ширина слова не должна ждать Repeater букв
            visible: !word.lettered
            transformOrigin: Item.Center
            scale: word.sungScale

            transform: Translate {
                id: wordTranslate
                y: word.sungLiftEm * word.fontSize
                Behavior on y {
                    // Behavior работает только вне анимации слова (во время волны sungLiftEm меняется непрерывно)
                    enabled: !word.wordAnimating && root.motion && !Theme.lowPower
                    NumberAnimation { duration: 105; easing.type: Easing.OutCubic }
                }
            }

            layer.enabled: word.sweeping && !word.lettered
            layer.smooth: true
            layer.effect: ShaderEffect {
                readonly property real featherPx: word.fontSize * (root.spicy ? 0.72 : (root.karaoke ? 0.14 : 0.45))
                readonly property real baseFeather: root.karaoke
                    ? Math.max(0.025, Math.min(0.10, featherPx / Math.max(1, label.width)))
                    : (root.spicy
                        ? Math.max(0.34, Math.min(0.62, featherPx / Math.max(1, label.width)))
                        : Math.min(0.5, featherPx / Math.max(1, label.width) / 2))
                property real feather: Math.max(0.005, baseFeather * root.highlightMultiplier)
                property real progress: -feather + word.progress * (1 + 2 * feather)
                property real dimAlpha: word.shaderDimAlpha
                property real glow: word.glow
                fragmentShader: root.fillShader
            }
        }
        // Кернинг даёт advanceWidth префикса, а не сумма ширин отдельных букв
        FontMetrics {
            id: letterMetrics
            font: label.font
        }

        Item {
            id: lettersBox
            visible: word.lettered
            width: label.implicitWidth
            height: word.rowH
            anchors.verticalCenter: parent.verticalCenter
            transformOrigin: Item.Center
            scale: word.sungScale
            transform: Translate {
                y: word.sungLiftEm * word.fontSize
                Behavior on y {
                    enabled: !word.wordAnimating && root.motion && !Theme.lowPower
                    NumberAnimation { duration: 105; easing.type: Easing.OutCubic }
                }
            }

            Repeater {
                model: word.letters

                Text {
                    id: letter
                    required property string modelData
                    required property int index
                    readonly property real pulse: word.lettered && word.wordAnimating ? word.letterPulse(index, word.letters.length) : 0
                    readonly property real centered: index - (word.letters.length - 1) / 2
                    readonly property real overscan: Math.ceil(word.fontSize * 0.25)
                    readonly property real fullWidth: Math.max(1, label.implicitWidth)
                    readonly property real a: x / fullWidth
                    readonly property real b: (x + implicitWidth) / fullWidth
                    readonly property bool sweepingLetter: word.useWordShader
                        && ((word.progress > 0 && word.progress < 1) || pulse > 0.005)

                    x: letterMetrics.advanceWidth(word.txt.slice(0, index + 1)) - letterMetrics.advanceWidth(modelData)
                    y: -overscan
                    height: word.rowH + 2 * overscan
                    topPadding: overscan
                    bottomPadding: overscan
                    verticalAlignment: Text.AlignVCenter
                    text: modelData
                    font.family: Theme.fontFamily
                    font.pixelSize: word.fontSize
                    font.weight: Theme.variableFont ? Font.Normal : Font.Bold
                    font.variableAxes: Theme.fontAxes(word.fontSize, 700)
                    color: root.textColor
                    opacity: word.useWordShader ? (sweepingLetter ? 1.0 : (word.progress >= 1 ? 1.0 : word.shaderDimAlpha)) : 1.0
                    transformOrigin: Item.Center
                    scale: 1.0 + 0.07 * word.waveAmount * root.wordBounce * pulse

                    transform: Translate {
                        // Разлёт не больше ~0.07 em у края слова, чтобы не съедать пробел до следующего
                        x: 0.07 * Math.min(1, word.waveAmount) * word.fontSize * (letter.centered / Math.max(1, word.letters.length / 2)) * letter.pulse
                        y: -0.025 * word.waveAmount * word.fontSize * root.wordBounce * letter.pulse
                    }

                    layer.enabled: sweepingLetter
                    layer.smooth: true
                    layer.effect: ShaderEffect {
                        // Та же граница, что у слова целиком, пересчитанная в локальные координаты буквы
                        readonly property real featherWord: Math.max(0.005, Math.max(0.34, Math.min(0.62, word.fontSize * 0.72 / letter.fullWidth)) * root.highlightMultiplier)
                        readonly property real edge: -featherWord + word.progress * (1 + 2 * featherWord)
                        readonly property real span: Math.max(0.001, letter.b - letter.a)
                        property real progress: (edge - letter.a) / span
                        property real feather: featherWord / span
                        property real dimAlpha: word.shaderDimAlpha
                        property real glow: Math.max(word.glow, letter.pulse * 0.9 * word.waveAmount * root.highlightMultiplier)
                        fragmentShader: root.fillShader
                    }
                }
            }
        }
    }
}
