import QtQuick
import Materialfy.Core

// Иконка Material Symbols Rounded по имени лигатуры: Icon { name: "play_arrow" }.
// Центрируется по фактическому контуру глифа, а не по его ячейке шрифта: часть глифов
// (favorite, queue_music…) нарисована выше центра ячейки и в квадратной подложке смотрится смещённой.
// Поправку меряет IconMetrics (Python) по отрисованным пикселям — FontMetrics для лигатур врёт.
Item {
    id: root

    property string name
    property real size: 24
    property real fill: 0          // 0..1, анимируемая ось FILL
    property real weight: 400      // 100..700
    property real grade: 0         // -25..200
    property color color: Theme.fgSurfaceVariant
    property bool opticalCenter: true
    // true — ширина по видимому контуру глифа (для иконок рядом с текстом: ровные отступы)
    property bool tight: false
    // Предок растянут через scale (островок): нативный глиф растеризуется в своём размере и при растяжении
    // мылится, поэтому рисуем его сразу в итоговом размере и сжимаем обратно
    property real renderScale: 1
    readonly property real inkWidth: _ink.width > 0 ? _ink.width : size
    readonly property real inkHeight: _ink.height > 0 ? _ink.height : size

    Behavior on fill { NumberAnimation { duration: Theme.motion.effectsDefault } }

    implicitWidth: tight ? inkWidth : size
    implicitHeight: size

    readonly property rect _ink: opticalCenter && name !== "" ? IconMetrics.ink(name, size) : Qt.rect(0, 0, 0, 0)

    Text {
        id: glyph
        text: IconMetrics.glyph(root.name)
        color: root.color
        textFormat: Text.PlainText
        renderType: Text.NativeRendering  // distance-field (QtRendering) дырявит заливку вариативного шрифта
        font.family: Theme.iconFamily
        font.pixelSize: root.size * root.renderScale
        font.variableAxes: ({ "FILL": root.fill, "wght": root.weight, "GRAD": root.grade, "opsz": Math.max(20, Math.min(48, root.size)) })
        scale: 1 / root.renderScale
        transformOrigin: Item.TopLeft

        // Целые пиксели: нативный растеризатор иначе размывает края.
        x: Math.round(((root.width - implicitWidth / root.renderScale) / 2 - root._ink.x) * root.renderScale) / root.renderScale
        y: Math.round(((root.height - implicitHeight / root.renderScale) / 2 - root._ink.y) * root.renderScale) / root.renderScale
    }
}
