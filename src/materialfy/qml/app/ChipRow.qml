import QtQuick
import Md3

// Ряд фильтр-чипов. На десктопе переносится по строкам; на телефоне — одна строка с горизонтальной
// прокруткой (две-три строки чипов отнимали пол-экрана над результатами). Ряд выходит под поля страницы
// (bleed), чтобы чипы уезжали к краю экрана, а не обрывались на отступе; выбранный чип сам въезжает в вид.
// model: [{ text, icon? , <keyRole>? }]; current — ключ выбранного (значение keyRole, без него — индекс).
Item {
    id: root

    property var model: []
    property var current
    property string keyRole: ""
    property real chipHeight: 32
    property real bleed: 0
    readonly property bool scrolling: Theme.mobile
    signal picked(var key, var data)

    function _key(data, index) { return keyRole !== "" ? data[keyRole] : index }

    implicitWidth: scrolling ? 0 : flow.implicitWidth
    implicitHeight: scrolling ? chipHeight : flow.implicitHeight

    Component {
        id: chipDelegate
        Chip {
            required property var modelData
            required property int index
            implicitHeight: root.chipHeight
            text: modelData.text
            icon: modelData.icon ?? ""
            selected: root.current === root._key(modelData, index)
            onClicked: root.picked(root._key(modelData, index), modelData)
        }
    }

    Flow {
        id: flow
        visible: !root.scrolling
        width: root.width
        spacing: 8
        Repeater { model: root.scrolling ? [] : root.model; delegate: chipDelegate }
    }

    Flickable {
        id: flick
        visible: root.scrolling
        x: -root.bleed
        width: root.width + 2 * root.bleed
        height: root.height
        contentWidth: row.implicitWidth + 2 * root.bleed
        contentHeight: height
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.OvershootBounds
        interactive: contentWidth > width
        clip: true

        Behavior on contentX {
            enabled: !flick.moving && Theme.reducedMotion !== true
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }

        Row {
            id: row
            x: root.bleed
            height: parent.height
            spacing: 8
            Repeater { id: rowChips; model: root.scrolling ? root.model : []; delegate: chipDelegate }
        }
    }

    function _reveal() {
        if (!scrolling || !flick.interactive)
            return
        const i = (model ?? []).findIndex((d, n) => _key(d, n) === current)
        const chip = i >= 0 ? rowChips.itemAt(i) : null
        if (!chip)
            return
        const left = chip.x + row.x - 16
        const right = chip.x + row.x + chip.width + 16
        if (left < flick.contentX)
            flick.contentX = Math.max(0, left)
        else if (right > flick.contentX + flick.width)
            flick.contentX = Math.min(flick.contentWidth - flick.width, right - flick.width)
    }
    onCurrentChanged: _reveal()
}
