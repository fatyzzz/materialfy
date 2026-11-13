import QtQuick
import Md3

// Ряд чипов разделов настроек. В узком окне прокручивается по горизонтали;
// выбранный чип сам въезжает в видимую часть.
Item {
    id: root

    property var sections: []          // [{ key, text, icon }]
    property string current
    signal picked(string key)

    implicitHeight: Math.max(48, row.implicitHeight + 8)
    implicitWidth: row.implicitWidth

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: row.implicitWidth
        contentHeight: height
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentWidth > width
        clip: true

        Behavior on contentX {
            enabled: !flick.moving
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }

        Row {
            id: row
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
            Repeater {
                id: chips
                model: root.sections
                Chip {
                    required property var modelData
                    text: modelData.text
                    icon: modelData.icon ?? ""
                    selected: root.current === modelData.key
                    onClicked: root.picked(modelData.key)
                }
            }
        }

        // колесо мыши крутит ряд по горизонтали, только если он не помещается
        WheelHandler {
            enabled: flick.interactive
            acceptedDevices: PointerDevice.Mouse
            onWheel: e => {
                flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width, flick.contentX - e.angleDelta.y))
                e.accepted = true
            }
        }
    }

    onCurrentChanged: {
        const i = sections.findIndex(s => s.key === current)
        const chip = i >= 0 ? chips.itemAt(i) : null
        if (!chip || !flick.interactive)
            return
        if (chip.x < flick.contentX)
            flick.contentX = Math.max(0, chip.x - 16)
        else if (chip.x + chip.width > flick.contentX + flick.width)
            flick.contentX = Math.min(flick.contentWidth - flick.width, chip.x + chip.width - flick.width + 16)
    }
}
