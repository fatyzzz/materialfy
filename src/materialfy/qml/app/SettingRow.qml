import QtQuick
import QtQuick.Layouts
import Md3

// Строка настройки: заголовок + пояснение слева, контрол справа (кладётся внутрь).
// Если контрол не помещается рядом с подписью (узкое окно) — он уходит под неё.
GridLayout {
    id: root

    property string title
    property string description
    default property alias control: slot.data

    Layout.fillWidth: true
    // ширину задаёт родитель: иначе неявная ширина двух колонок распирает строку и она не переносится
    Layout.preferredWidth: 0
    // перенос контрола под подпись — с запасом ±16 px, иначе у границы строка мигала бы при растягивании
    property bool _stacked: false
    function _updateStack() {
        if (width <= 0)
            return
        const edge = slot.implicitWidth + 200
        _stacked = _stacked ? width < edge + 16 : width < edge - 16
    }
    onWidthChanged: _updateStack()
    Component.onCompleted: _stacked = width > 0 && width < slot.implicitWidth + 200
    columns: _stacked ? 1 : 2
    columnSpacing: 24
    rowSpacing: 10

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        // длинный заголовок (узкий экран, русский) — переносится, а не обрезается
        Label { Layout.fillWidth: true; text: root.title; type: "bodyLarge"; wrapMode: Text.WordWrap; elide: Text.ElideNone }
        Label {
            visible: root.description !== ""
            Layout.fillWidth: true
            text: root.description
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
    }
    Item {
        id: slot
        objectName: "settingSlot"
        // под подписью контрол занимает всю строку; ButtonGroup по этой ширине ужимается сам
        readonly property real availableWidth: root._stacked ? root.width : Infinity
        // естественная ширина контрола, а не ужатая: иначе ужатый контрол «помещался» бы рядом с подписью
        implicitWidth: {
            let w = 0
            for (let i = 0; i < children.length; ++i) {
                const c = children[i]
                if (c.visible)
                    w = Math.max(w, c.implicitWidth)
            }
            return w
        }
        implicitHeight: {
            let h = 0
            for (let i = 0; i < children.length; ++i)
                if (children[i].visible)
                    h = Math.max(h, children[i].y + children[i].height)
            return h
        }
        // под подписью — ровно по строке: естественная ширина контрола не распирает колонку
        Layout.fillWidth: root._stacked
        Layout.preferredWidth: root._stacked ? 0 : implicitWidth
        // контрол стал шире (другой язык, длиннее подпись кнопки) — пересчитать перенос
        onImplicitWidthChanged: root._updateStack()
    }
}
