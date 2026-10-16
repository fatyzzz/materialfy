import QtQuick
import Md3

// Стек страниц. Живые экземпляры страниц хранятся в стеке, поэтому «назад» сохраняет прокрутку и состояние.
// Переходы MD3: между разделами — fade through, вглубь/назад — shared axis X.
Item {
    id: root

    // имя страницы → url компонента
    property var pages: ({})
    property var stack: []                 // [{ name, props, item }]
    readonly property int depth: stack.length
    readonly property var current: stack.length ? stack[stack.length - 1] : null
    readonly property string currentName: current ? current.name : ""
    readonly property string rootName: stack.length ? stack[0].name : ""
    readonly property Item currentItem: current ? current.item : null

    // Страницы, которые живут между посещениями. "home" — без параметров (главная тяжёлая — сотни
    // обложек, терять прокрутку и пересоздавать её на каждый возврат дорого). keepAliveSlots — сколько
    // страниц С параметрами (плейлисты, артисты) держать живыми по принципу LRU: повторное открытие
    // того же плейлиста «туда-сюда» мгновенно и сохраняет прокрутку, без перестройки строк.
    property var keepAlive: ["home"]
    // Строятся синхронно при первом входе (стартовый экран); остальные keepAlive — асинхронно.
    property var keepAliveSync: ["home"]
    property int keepAliveSlots: 6
    property var _kept: ({})                // имя → живой экземпляр из keepAlive (без параметров)
    property var _keptKeyed: []             // LRU: [{ key, name, item }], младшие в конце

    property var _components: ({})
    // Прогрев компиляции страниц: первое открытие каждой страницы компилирует её QML-цепочку
    // синхронно на GUI-потоке — фриз первого открытия. Компилируем по одному компоненту на тик
    // после старта, пока юзер на главной; при реальной навигации пауза, догрев после неё.
    // Один тик = одна страница: скролл главной не дёргается.
    property bool _prewarmed: false
    // Порядок: сначала компиляция тяжёлых страниц (плейлист, библиотека), затем тихая сборка
    // тех, что живут вечно (settings/liked/...): их экземпляры сразу переиспользуются при входе.
    // Параметрические (плейлист, артист) инкубировать без props нельзя — держим только компиляцию.
    function _prewarm() {
        if (_prewarmed)
            return
        _prewarmed = true
        const names = Object.keys(pages).filter(n => keepAlive.indexOf(n) < 0)
        prewarmTimer.names = names.concat(keepAlive.filter(n => n !== "home" && _kept[n] === undefined))
        prewarmTimer.at = 0
        prewarmTimer.restart()
    }
    Timer {
        id: prewarmTimer
        interval: 80
        repeat: true
        property var names: []
        property int at: 0
        onTriggered: {
            if (_pending !== null) {
                restart()
                return
            }
            if (at >= names.length) {
                stop()
                return
            }
            const name = names[at++]
            if (keepAlive.indexOf(name) >= 0 && _kept[name] === undefined) {
                let comp = _components[name]
                if (!comp) {
                    comp = Qt.createComponent(pages[name])
                    _components[name] = comp
                }
                if (comp.status === Component.Error) {
                    console.error("Router prewarm:", comp.errorString())
                    return
                }
                // Тихая сборка вечной страницы: невидимая, без push, сразу в _kept.
                const item = comp.createObject(container, { router: root, width: container.width, height: container.height })
                if (!item) {
                    console.error("Router prewarm:", comp.errorString())
                    return
                }
                item.width = Qt.binding(() => container.width)
                item.height = Qt.binding(() => container.height)
                item.visible = false
                _kept[name] = item
            }
        }
    }
    property var _leaving: []               // элементы, которые уничтожить после анимации
    property Item _out: null
    property var _pending: null             // незавершённая инкубация: { name, props, comp, incubator, prev, kind }

    function _same(entry, name, props) {
        return entry && entry.name === name && JSON.stringify(entry.props ?? {}) === JSON.stringify(props ?? {})
    }

    // Повторный запрос той же страницы, пока она ещё строится, — не новый push, а то же самое
    function _pendingSame(name, props) {
        return _pending !== null && _pending.name === name && JSON.stringify(_pending.props) === JSON.stringify(props ?? {})
    }

    function push(name, props) {
        if (_same(current, name, props) || _pendingSame(name, props))
            return
        if (_pending)
            _cancelPending()                // push A → push B: недостроенную A отменяем и строим B
        const prev = currentItem
        const entry = _make(name, props, "forward")
        if (!entry)
            return                          // инкубация уже запущена (или ошибка — уже в логе), продолжение — в _onIncubated
        stack = stack.concat([entry])
        _animate(prev, entry.item, "forward")
    }

    // Переход в раздел верхнего уровня: стек сбрасывается.
    function reset(name, props) {
        if (_pendingSame(name, props) || (depth === 1 && _same(current, name, props)))
            return
        if (_pending)
            _cancelPending()                // reset всё равно переписывает стек — инкубация больше не нужна
        const prev = currentItem
        const entry = _make(name, props, "fade")
        if (!entry)
            return
        _leaving = _leaving.concat(stack.map(e => e.item).filter(i => i !== prev))
        if (prev)
            _leaving = _leaving.concat([prev])
        stack = [entry]
        _animate(prev, entry.item, "fade")
    }

    function back() {
        if (_pending !== null) {
            // Страница ещё не попала в стек: отмена инкубации и есть «назад», стек не трогаем
            _cancelPending()
            return true
        }
        if (depth < 2)
            return false
        const popped = stack[stack.length - 1]
        stack = stack.slice(0, -1)
        _leaving = _leaving.concat([popped.item])
        const next = currentItem
        next.visible = true
        _animate(popped.item, next, "back")
        return true
    }

    // null-возврат при живой _pending — не ошибка, а «строится асинхронно»; kind нужен _onIncubated
    function _make(name, props, kind) {
        const url = pages[name]
        if (!url) {
            console.error("Router: no such page", name)
            return null
        }
        let comp = _components[name]
        if (!comp) {
            comp = Qt.createComponent(url)
            _components[name] = comp
        }
        if (comp.status === Component.Error) {
            console.error("Router:", comp.errorString())
            return null
        }
        const keep = keepAlive.indexOf(name) >= 0 && Object.keys(props ?? {}).length === 0
        const kept = keep ? _kept[name] : null
        if (kept && !stack.some(e => e.item === kept)) {
            kept.opacity = 0
            return { name: name, props: {}, item: kept }
        }
        // keepAlive без живого экземпляра: home — синхронно (старт), остальные асинхронно —
        // единственная сборка не должна фризить, дальше страница живёт вечно
        // Страницы с параметрами: ключ = имя + props. Живой экземпляр с тем же ключом переиспользуем
        // (инstant-возврат, скролл на месте); иначе строим инкубацией и кладём в LRU.
        const key = name + "|" + JSON.stringify(props ?? {})
        const keptKeyed = _takeKeptKeyed(key)
        if (keptKeyed && !stack.some(e => e.item === keptKeyed)) {
            keptKeyed.opacity = 0
            return { name: name, props: props ?? {}, item: keptKeyed }
        }
        // Размер — сразу при создании: страница, созданная нулевой ширины, тут же «дорастала» до настоящей,
        // и Behavior шапки (обложка, отступы), колонки альбома у строк и раскладки полсекунды анимировались
        // и перекладывались в каждом кадре перехода. Инкубации это касается так же — начальные width/height
        // и непрозрачность 0, чтобы недостроенная страница не мигнула, а binding цепляем уже на Ready.
        if (keep && keepAliveSync.indexOf(name) >= 0) {
            const item = comp.createObject(container, Object.assign({ router: root, width: container.width, height: container.height },
                                                                    props ?? {}))
            if (!kept)
                _kept[name] = item
            // не anchors.fill: анимации переходов двигают x
            item.width = Qt.binding(() => container.width)
            item.height = Qt.binding(() => container.height)
            item.opacity = 0
            return { name: name, props: props ?? {}, item: item }
        }
        _incubate(comp, name, props, kind)
        return null
    }

    function _incubate(comp, name, props, kind) {
        const incubator = comp.incubateObject(
            container,
            Object.assign({ router: root, width: container.width, height: container.height }, props ?? {}, { opacity: 0 }),
            Qt.Asynchronous)
        _pending = { name: name, props: props ?? {}, comp: comp, incubator: incubator, prev: currentItem, kind: kind, keep: keepAlive.indexOf(name) >= 0 && Object.keys(props ?? {}).length === 0 }
        incubator.onStatusChanged = function() { _onIncubated(incubator) }
    }

    function _onIncubated(incubator) {
        const p = _pending
        if (p === null || p.incubator !== incubator) {
            // Устаревший запрос (сменился push/reset/back): недостроенное уничтожаем,
            // чтобы оно не копилося в container с opacity 0 и не продолжало загрузки.
            if (incubator.status === Component.Ready && incubator.object)
                incubator.object.destroy()
            return
        }
        if (incubator.status === Component.Loading)
            return
        _pending = null
        if (incubator.status !== Component.Ready) {
            console.error("Router:", p.comp.errorString())
            return
        }
        const item = incubator.object
        // Кадра с чужим размером нет: до Ready страница прозрачна, binding навешиваем тем же кадром, что и анимацию
        item.width = Qt.binding(() => container.width)
        item.height = Qt.binding(() => container.height)
        item.opacity = 0
        if (p.keep && _kept[p.name] === undefined)
            _kept[p.name] = item
        _rememberKeyed(p, item)
        const entry = { name: p.name, props: p.props, item: item }
        if (p.kind === "prewarm") {
            // Тихий прогрев: страница скрыта в LRU, никакого стека и анимации.
            item.visible = false
            item.z = 0
            return
        }
        if (p.kind === "forward") {
            stack = stack.concat([entry])
            _animate(p.prev, item, "forward")
        } else {
            _leaving = _leaving.concat(stack.map(e => e.item).filter(i => i !== p.prev))
            if (p.prev)
                _leaving = _leaving.concat([p.prev])
            stack = [entry]
            _animate(p.prev, item, "fade")
        }
    }

    // Прогрев параметрической страницы (наведение на карточку): строим инкубацией без push,
    // результат сразу в LRU. Клик по карточке найдёт живой экземпляр — переход мгновенный.
    function prewarmKeyed(name, props) {
        if (_components[name] === undefined)
            return
        if (_pendingSame(name, props) || stack.some(e => _same(e, name, props)))
            return
        const key = name + "|" + JSON.stringify(props ?? {})
        if (_takeKeptKeyed(key))
            return
        if (_pending !== null)
            return                          // не перебиваем настоящий переход
        const comp = _components[name] || Qt.createComponent(pages[name])
        if (comp.status === Component.Error || comp.status === Component.Loading)
            return
        _components[name] = comp
        _incubate(comp, name, props, "prewarm")
    }

    function _takeKeptKeyed(key) {
        for (let i = 0; i < _keptKeyed.length; i++) {
            if (_keptKeyed[i].key === key) {
                const e = _keptKeyed[i]
                _keptKeyed.splice(i, 1)
                _keptKeyed.push(e)          // LRU: недавний — в конец
                return e.item
            }
        }
        return null
    }

    function _rememberKeyed(p, item) {
        const key = p.name + "|" + JSON.stringify(p.props)
        if (_takeKeptKeyed(key))
            return                          // уже в LRU (не должно случиться, но безопасно)
        _keptKeyed.push({ key: key, name: p.name, item: item })
        while (_keptKeyed.length > keepAliveSlots) {
            const evicted = _keptKeyed.shift()
            if (!stack.some(e => e.item === evicted.item))
                evicted.item.destroy()
        }
    }

    function _cancelPending() {
        const p = _pending
        if (p === null)
            return
        // У JS-инкубатора нет cancel(): помечаем запрос устаревшим. _onIncubated увидит
        // p !== _pending и уничтожит недостроенный объект сам, когда статус дойдёт до Ready.
        _pending = null
        if (p.incubator.status === Component.Ready && p.incubator.object)
            p.incubator.object.destroy()
    }

    function _animate(from, to, kind) {
        if (anim.running)
            anim.complete()
        _out = from
        from = from ?? dummy
        // Разделы (fade through): новая страница всплывает снизу и чуть «дорастает» до размера.
        // Вглубь/назад (shared axis X): страницы едут по горизонтали навстречу друг другу.
        const axis = kind === "fade" ? "y" : "x"
        const d = kind === "forward" ? 140 : kind === "back" ? -140 : 72
        outFade.target = from
        outMove.target = from
        outMove.property = axis
        outMove.to = kind === "fade" ? -24 : -d / 2
        outScale.target = from
        outScale.to = kind === "fade" ? 0.97 : 1
        inFade.target = to
        inMove.target = to
        inMove.property = axis
        inMove.from = d
        inScale.target = to
        inScale.from = kind === "fade" ? 0.94 : 1
        to.x = 0
        to.y = 0
        to.visible = true
        to.z = 1
        from.z = 0
        anim.start()
        // Программный рендер: страница сменяется сразу. Каждый из ~35 кадров перехода процессор рисует
        // целиком, вместе с только что созданной страницей, — это и есть «подвисание» при открытии
        if (Theme.lowPower)
            anim.complete()
    }

    Item {
        id: container
        anchors.fill: parent
    }

    Item { id: dummy; visible: false }

    // Ушедшая страница оставляет в куче JS десятки МБ мусора (её модели, привязки): сборщик сам доходит до
    // него не скоро, а память процесса всё это время занята. Полная сборка — через секунду после перехода,
    // когда анимация закончилась (5–25 мс), и только если страницы правда уничтожены.
    Timer { id: gcTimer; interval: 1000; onTriggered: gc() }

    ParallelAnimation {
        id: anim

        NumberAnimation { id: outFade; property: "opacity"; to: 0; duration: 160; easing.type: Easing.OutCubic }
        NumberAnimation {
            id: outMove; duration: 300
            easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedAccelerate
        }
        NumberAnimation { id: outScale; property: "scale"; duration: 300; easing.type: Easing.OutCubic }
        SequentialAnimation {
            PauseAnimation { duration: 60 }
            ParallelAnimation {
                NumberAnimation { id: inFade; property: "opacity"; from: 0; to: 1; duration: 260; easing.type: Easing.OutCubic }
                NumberAnimation {
                    id: inMove; to: 0; duration: 560
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
                NumberAnimation {
                    id: inScale; property: "scale"; to: 1; duration: 560
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
            }
        }

        onFinished: {
            if (root._out) {
                root._out.visible = false
                root._out.x = 0
                root._out.y = 0
                root._out.scale = 1
            }
            const kept = Object.values(root._kept).concat(root._keptKeyed.map(e => e.item))
            for (const item of root._leaving) {
                if (kept.indexOf(item) < 0) {
                    item.destroy()
                    gcTimer.restart()
                } else if (item !== root.currentItem) {
                    item.visible = false
                    item.x = 0
                    item.y = 0
                    item.scale = 1
                }
            }
            root._leaving = []
            root._out = null
        }
    }
}
