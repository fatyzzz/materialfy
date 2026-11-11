import QtQuick

// Connected button group MD3 Expressive (как Normal | Circular в референсе).
// model: ["Normal", "Circular"] или [{ text, icon }]; currentIndex — выбранный сегмент.
// Не помещается в maxWidth (узкий телефон, длинный язык) — сначала прячутся иконки, потом сегменты
// сжимаются пропорционально, подписи с многоточием. implicitWidth — всегда естественная ширина.
// Внутри SettingRow maxWidth берётся из ширины строки сам.
Item {
    id: root

    property var model: []
    property int currentIndex: 0
    property string size: "s"
    property bool showCheck: false
    // false — currentIndex задаётся только снаружи (binding), клик лишь шлёт activated()
    property bool autoSelect: true
    property color segmentColor: Theme.surfaceContainerHighest   // невыбранный сегмент
    property real spacing: 2
    property real maxWidth: parent && parent.objectName === "settingSlot" ? parent.availableWidth : Infinity

    signal activated(int index)

    readonly property var spec: ({ xs: { h: 32, pad: 12, inner: 4 }, s: { h: 40, pad: 16, inner: 8 }, m: { h: 56, pad: 24, inner: 8 } })[size]
    readonly property int count: model ? model.length : 0
    // естественная ширина: с иконками и без
    property real _withIcons: 0
    property real _noIcons: 0
    readonly property bool _dropIcons: _withIcons > maxWidth + 0.5
    // без иконок всё ещё шире — поля сегментов уже (до 8 px), и только потом сжатие с многоточием
    readonly property real _padCut: _dropIcons && _noIcons > maxWidth + 0.5 ? Math.max(0, spec.pad - 8) : 0
    readonly property real _natural: _dropIcons ? _noIcons - 2 * _padCut * count : _withIcons
    readonly property real _scale: _natural > maxWidth && _natural > 0
                                   ? Math.max(0, (maxWidth - spacing * (count - 1)) / (_natural - spacing * (count - 1))) : 1
    function _measure() {
        let a = 0, b = 0
        for (let i = 0; i < repeater.count; ++i) {
            const it = repeater.itemAt(i)
            if (!it)
                return
            a += it.fullWidth
            b += it.bareWidth
        }
        const gaps = spacing * Math.max(0, count - 1)
        _withIcons = a + gaps
        _noIcons = b + gaps
    }

    implicitWidth: _withIcons
    implicitHeight: spec.h
    width: Math.min(implicitWidth, maxWidth)

    Row {
        spacing: root.spacing

        Repeater {
            id: repeater
            model: root.model
            onItemAdded: Qt.callLater(root._measure)

            delegate: Item {
                id: seg

                required property var modelData
                required property int index
                readonly property bool selected: index === root.currentIndex
                readonly property bool first: index === 0
                readonly property bool last: index === root.model.length - 1
                readonly property string label: typeof modelData === "string" ? modelData : (modelData.text ?? "")
                Accessible.role: Accessible.RadioButton
                Accessible.name: typeof modelData === "string" ? modelData : (modelData.accessibleName ?? seg.label)
                Accessible.checkable: true
                Accessible.checked: seg.selected
                Accessible.onPressAction: seg.activate()
                activeFocusOnTab: true
                Keys.onReturnPressed: seg.activate()
                Keys.onSpacePressed: seg.activate()
                function activate() {
                    if (!enabled)
                        return
                    if (root.autoSelect)
                        root.currentIndex = index
                    root.activated(index)
                }
                readonly property string iconName: typeof modelData === "string" ? "" : (modelData.icon ?? "")
                readonly property var spec: root.spec
                readonly property bool showIcon: icon.n !== "" && !root._dropIcons
                readonly property real bareWidth: caption.implicitWidth + 2 * spec.pad
                readonly property real fullWidth: bareWidth + (icon.n !== "" ? 18 + (seg.label !== "" ? content.spacing : 0) : 0)
                onFullWidthChanged: Qt.callLater(root._measure)

                // Выбранный сегмент и внешние углы — полностью круглые, внутренние — малое скругление.
                readonly property real full: height / 2
                property real leftR: selected || first ? full : (area.pressed ? spec.inner / 2 : spec.inner)
                property real rightR: selected || last ? full : (area.pressed ? spec.inner / 2 : spec.inner)
                Behavior on leftR { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                Behavior on rightR { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }

                implicitHeight: spec.h
                implicitWidth: ((showIcon ? fullWidth : bareWidth) - 2 * root._padCut) * root._scale
                width: implicitWidth
                height: implicitHeight

                Rectangle {
                    id: bg
                    anchors.fill: parent
                    color: seg.selected ? Theme.secondaryContainer : root.segmentColor
                    topLeftRadius: seg.leftR
                    bottomLeftRadius: seg.leftR
                    topRightRadius: seg.rightR
                    bottomRightRadius: seg.rightR
                    Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
                }

                Row {
                    id: content
                    anchors.centerIn: parent
                    spacing: 6
                    Icon {
                        id: icon
                        tight: true
                        readonly property string n: seg.iconName !== "" ? seg.iconName : (root.showCheck && seg.selected ? "check" : "")
                        visible: seg.showIcon
                        name: n
                        size: 18
                        fill: seg.selected ? 1 : 0
                        color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Label {
                        id: caption
                        visible: seg.label !== ""
                        width: Math.min(implicitWidth, seg.width - 2 * (seg.spec.pad - root._padCut)
                                        - (icon.visible ? 18 + content.spacing : 0))
                        elide: Text.ElideRight
                        // ужатый сегмент: шрифт чуть меньше (до 11 px) вместо многоточия
                        fontSizeMode: root._scale < 1 ? Text.HorizontalFit : Text.FixedSize
                        minimumPixelSize: 11
                        text: seg.label
                        type: "labelLarge"
                        color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                StateLayer {
                    id: area
                    anchors.fill: parent
                    color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                    topLeftRadius: seg.leftR
                    bottomLeftRadius: seg.leftR
                    topRightRadius: seg.rightR
                    bottomRightRadius: seg.rightR
                    onClicked: seg.activate()
                }
            }
        }
    }
}
