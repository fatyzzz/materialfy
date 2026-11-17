import QtQuick

// Плавная прокрутка для любого Flickable/ListView: SmoothScroll { flickable: list }.
// Колесо мыши: щелчок сдвигает цель на wheelStep, contentY догоняет её экспоненциально
// (независимо от частоты кадров — одинаково на 60 и 144 Гц); быстрые щелчки накапливаются.
// Тачпад: пиксели 1:1 без задержки. Плюс тонкий индикатор прокрутки.
// Перетаскивание контента мышью стоит выключить у самого Flickable: acceptedButtons: Qt.NoButton.
Item {
    id: root

    required property Flickable flickable
    property real wheelStep: 150          // px за щелчок колеса
    property real smoothing: 14           // 1/с: больше — быстрее доводка
    property real maxBoost: 2             // быстрое вращение колеса разгоняет шаг до wheelStep × maxBoost
    property real _recent: 0              // «щелчков» за последние ~0,25 с (с затуханием)
    property real _lastWheel: 0
    property bool showScrollBar: true

    readonly property real minY: flickable.originY
    readonly property real maxY: flickable.originY + Math.max(0, flickable.contentHeight - flickable.height)
    property real _target: 0

    // --debug-wheel: печатать сырые события колеса
    readonly property bool debugWheel: typeof materialfyDebugWheel !== "undefined" && materialfyDebugWheel

    signal userScrolled()   // колесо, тачпад или индикатор — не программная прокрутка

    function clampY(y) { return Math.max(minY, Math.min(maxY, y)) }

    // Тачпад: сдвиг сразу, без сглаживания
    property real _fineUntil: 0           // до этого момента поток событий без фаз — тачпад Windows
    property bool _momentum: false        // в этом жесте система сама шлёт инерцию
    property var _samples: []             // [{ t, d }] — сдвиги пальца за последние ~0,1 с
    function _direct(dy) {
        smooth.running = false
        flickable.contentY = clampY(flickable.contentY + dy)
        _target = flickable.contentY
        directPulse.restart()
    }
    Timer { id: directPulse; interval: 500 }   // индикатор прокрутки виден и при сдвигах тачпадом
    function _sample(now, d) {
        _samples.push({ t: now, d: d })
        while (_samples.length > 0 && now - _samples[0].t > 100)
            _samples.shift()
    }
    // px/с по знаку pixelDelta (flick(0, v) с v > 0 едет к началу, как палец вниз)
    function _speed(now) {
        const s = _samples.filter(p => now - p.t <= 100)
        if (s.length < 2 || now - s[s.length - 1].t > 50)
            return 0
        const span = s[s.length - 1].t - s[0].t
        let sum = 0
        for (let i = 1; i < s.length; ++i)
            sum += s[i].d
        return span > 0 ? sum * 1000 / span : 0
    }

    function scrollTo(y, animated) {
        _target = clampY(y)
        if (animated === false) {
            smooth.running = false
            flickable.contentY = _target
        } else {
            smooth.running = true
        }
    }

    WheelHandler {
        parent: root.flickable
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        orientation: Qt.Vertical
        onWheel: event => {
            root.userScrolled()
            if (root.debugWheel)
                console.log(`wheel: type=${event.device.type} name="${event.device.name}" phase=${event.phase}`
                            + ` angle=${event.angleDelta.y} pixel=${event.pixelDelta.y} inverted=${event.inverted}`)
            // Тачпад узнаём по фазе жеста (Begin/Update/End есть только у пальцевой прокрутки).
            // Ни тип устройства, ни pixelDelta не надёжны: на Wayland колесо мыши тоже присылает
            // pixelDelta (~15 px за щелчок), а часть мышей приходит с типом «TouchPad».
            const now = Date.now()
            const touchpad = event.phase !== Qt.NoScrollPhase
            if (touchpad) {
                if (root.flickable.flicking)
                    root.flickable.cancelFlick()
                if (event.phase === Qt.ScrollBegin) {
                    root._samples = []
                    root._momentum = false
                } else if (event.phase === Qt.ScrollMomentum) {
                    root._momentum = true   // инерцию даёт сама система (macOS)
                }
                const dy = event.pixelDelta.y
                if (dy !== 0) {
                    root._sample(now, dy)
                    root._direct(-dy)
                }
                // Палец отпущен, а система инерции не даёт (Wayland, X11): докручиваем сами — flick
                // со скоростью последних ~0,1 с, как у пальца на сенсорном экране
                if (event.phase === Qt.ScrollEnd && !root._momentum && ["osx", "macos"].indexOf(Qt.platform.os) < 0) {
                    const v = root._speed(now)
                    root._samples = []
                    if (Math.abs(v) > 120)
                        root.flickable.flick(0, v)
                }
                return
            }
            if (root.flickable.flicking)
                root.flickable.cancelFlick()
            // Тачпад Windows приходит без фаз, как колесо (WM_MOUSEWHEEL), но долями щелчка произвольной
            // величины и десятками событий в секунду (инерцию после отпускания система досылает такими же
            // событиями). Сглаживание и разгон для него — запаздывание за пальцем и «тугая» прокрутка, поэтому
            // любая доля щелчка двигает список сразу, пропорционально (hi-res колесу так тоже лучше: его
            // доли и есть плавность), и ещё 0,4 с после неё — весь поток. Сглаживание — целым щелчкам.
            const a = Math.abs(event.angleDelta.y)
            if (a % 120 !== 0)
                root._fineUntil = now + 400
            if (now < root._fineUntil && event.angleDelta.y !== 0) {
                root._direct(-event.angleDelta.y / 120 * root.wheelStep)
                return
            }
            // Целые щелчки: цель сдвигается на wheelStep за щелчок, contentY догоняет её плавно.
            // Нет angleDelta — пересчитываем пиксели: Wayland отдаёт 15 px на щелчок.
            const notches = event.angleDelta.y !== 0 ? event.angleDelta.y / 120 : event.pixelDelta.y / 15
            // Разгон — по числу щелчков за последние ~0,25 с, а не по интервалу между событиями,
            // иначе hi-res колесо разгонялось бы уже внутри одного щелчка.
            root._recent = root._recent * Math.exp(-(now - root._lastWheel) / 250) + Math.abs(notches)
            root._lastWheel = now
            const boost = Math.max(1, Math.min(root.maxBoost, 1 + (root._recent - 2) * 0.25))
            const base = smooth.running ? root._target : root.flickable.contentY
            root._target = root.clampY(base - notches * root.wheelStep * boost)
            smooth.running = true
        }
    }

    FrameAnimation {
        id: smooth
        running: false
        onTriggered: {
            const target = root.clampY(root._target)
            const d = target - root.flickable.contentY
            if (Math.abs(d) < 0.5) {
                root.flickable.contentY = target
                running = false
                return
            }
            // Целые пиксели: иконки (нативный растеризатор) привязаны к пиксельной сетке, а текст и
            // картинки — нет; при дробном contentY они «дрожат» друг относительно друга.
            const step = d * (1 - Math.exp(-frameTime * root.smoothing))
            root.flickable.contentY = Math.round(root.flickable.contentY + (Math.abs(step) < 1 ? Math.sign(d) : step))
        }
    }

    // Тонкий индикатор прокрутки: проявляется при движении и наведении, тянется мышью.
    Item {
        id: bar
        parent: root.flickable
        visible: root.showScrollBar && root.flickable.contentHeight > root.flickable.height
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: 4
        width: 12

        readonly property bool active: root.flickable.movingVertically || smooth.running || directPulse.running
                                      || barArea.containsMouse || barArea.pressed
        opacity: active ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: bar.active ? Theme.motion.effectsFast : 600 } }

        readonly property real ratio: root.flickable.height / Math.max(root.flickable.contentHeight, 1)
        readonly property real thumbH: Math.max(32, height * ratio)
        readonly property real pos: root.maxY > root.minY ? (root.flickable.contentY - root.minY) / (root.maxY - root.minY) : 0

        Rectangle {
            readonly property bool wide: barArea.containsMouse || barArea.pressed
            anchors.right: parent.right
            width: wide ? 8 : 4
            radius: width / 2
            height: bar.thumbH
            y: (bar.height - height) * bar.pos
            color: Theme.fgSurfaceVariant
            opacity: wide ? 0.7 : 0.45
            Behavior on width { NumberAnimation { duration: Theme.motion.effectsFast } }
        }

        MouseArea {
            id: barArea
            anchors.fill: parent
            hoverEnabled: !Theme.mobile
            preventStealing: true
            property real grabOffset: 0
            function yFor(mouseY) {
                const p = (mouseY - grabOffset) / Math.max(1, bar.height - bar.thumbH)
                return root.minY + Math.max(0, Math.min(1, p)) * (root.maxY - root.minY)
            }
            onPressed: mouse => {
                root.userScrolled()
                const thumbY = (bar.height - bar.thumbH) * bar.pos
                grabOffset = mouse.y >= thumbY && mouse.y <= thumbY + bar.thumbH ? mouse.y - thumbY : bar.thumbH / 2
                root.scrollTo(yFor(mouse.y), grabOffset === bar.thumbH / 2)
            }
            onPositionChanged: mouse => { if (pressed) root.scrollTo(yFor(mouse.y), false) }
        }
    }
}
