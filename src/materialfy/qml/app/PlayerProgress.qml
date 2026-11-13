import QtQuick
import Md3
import Materialfy.Core

// Доля проигранного для полосы перемотки, плавно между обновлениями позиции (они приходят ~5 раз в секунду,
// у другого устройства — опросом): досчитывается по общим часам Ticker (~30 Гц), в тех же кадрах, что и бегущая
// волна. Раньше это была анимация (Behavior) на каждое обновление: она шла почти без пауз на частоте монитора
// и перерисовывала всё окно, пока играет музыка, даже у скрытой полосы.
QtObject {
    id: root

    property bool active: true             // полосу видно и её не тянут
    // false — без досчёта между обновлениями позиции (~5 раз в секунду): тонкой полоске мини-плеера плавность
    // не нужна, а каждый тик Ticker перерисовывает всё окно
    property bool smooth: true
    property real value: Player.position

    readonly property Connections _sync: Connections {
        target: Player
        function onPositionChanged() { root.value = Player.position }
        function onStateChanged() { root.value = Player.position }
    }

    readonly property TickAnimation _tick: TickAnimation {
        running: root.smooth && root.active && Player.playing && !Player.narrating && !Player.buffering && Player.durationMs > 0 && !Theme.quiet
        onTriggered: root.value = Math.min(1, root.value + root._tick.frameTime * 1000 / Player.durationMs)
    }
}
