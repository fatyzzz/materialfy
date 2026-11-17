import QtQuick

// Иконка-кнопка MD3 Expressive: style standard | filled | tonal | outlined,
// size xs | s | m | l | xl, width narrow | default | wide. Поддерживает toggle.
Item {
    id: root

    property string icon
    property string style: "standard"
    property string size: "s"
    property string widthMode: "default"
    property bool checkable: false
    property bool checked: false
    // false — состояние задаётся только снаружи (checked: binding), клик лишь шлёт clicked()
    property bool autoToggle: true
    property color iconColor: _contentColor
    // Мелким глифам (три точки more_horiz) стандартного кегля и веса мало: в кнопке они теряются
    property real iconSize: _spec.icon
    property real iconWeight: 400
    property real iconFill: _selected ? 1 : 0
    readonly property alias pressed: area.pressed
    readonly property alias hovered: area.containsMouse

    signal clicked()

    Accessible.role: checkable ? Accessible.CheckBox : Accessible.Button
    Accessible.checkable: checkable
    Accessible.checked: checked
    Accessible.onPressAction: root.activate()

    function activate() {
        if (!enabled)
            return
        if (checkable && autoToggle)
            checked = !checked
        clicked()
    }

    readonly property var _spec: ({
        xs: { h: 32,  icon: 20, narrow: 28,  wide: 40,  pressedR: 8,  squareR: 12 },
        s:  { h: 40,  icon: 24, narrow: 32,  wide: 52,  pressedR: 8,  squareR: 12 },
        m:  { h: 56,  icon: 24, narrow: 48,  wide: 72,  pressedR: 12, squareR: 16 },
        l:  { h: 96,  icon: 32, narrow: 64,  wide: 128, pressedR: 16, squareR: 28 },
        xl: { h: 136, icon: 40, narrow: 104, wide: 184, pressedR: 16, squareR: 28 }
    })[size]

    readonly property bool _selected: checkable && checked

    readonly property color containerColor: {
        if (!enabled)
            return style === "standard" || style === "outlined" ? "transparent" : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContainer)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.surfaceContainer : Theme.primary
        case "tonal": return checkable && !checked ? Theme.surfaceContainerHighest : Theme.secondaryContainer
        case "outlined": return _selected ? Theme.inverseSurface : "transparent"
        // Выбранная standard-кнопка — с тональной подложкой: одного цвета иконки мало,
        // при неудачной палитре primary почти не отличается от fgSurfaceVariant.
        default: return _selected ? Theme.secondaryContainer : "transparent"
        }
    }
    readonly property color _contentColor: {
        if (!enabled)
            return Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.fgSurfaceVariant : Theme.fgPrimary
        case "tonal": return checkable && !checked ? Theme.fgSurfaceVariant : Theme.fgSecondaryContainer
        case "outlined": return _selected ? Theme.fgInverseSurface : Theme.fgSurfaceVariant
        default: return _selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        }
    }

    property real cornerRadius: area.pressed ? _spec.pressedR : _selected ? _spec.squareR : height / 2
    Behavior on cornerRadius {
        NumberAnimation { duration: area.pressed ? Theme.motion.press : Theme.motion.spatialFast; easing.type: Theme.mobile ? Easing.OutCubic : Easing.OutBack; easing.overshoot: Theme.motion.overshoot }
    }

    implicitHeight: _spec.h
    implicitWidth: widthMode === "narrow" ? _spec.narrow : widthMode === "wide" ? _spec.wide : _spec.h

    Rectangle {
        id: container
        anchors.fill: parent
        radius: Math.min(root.cornerRadius, height / 2)
        color: root.containerColor
        border.width: root.style === "outlined" && !root._selected ? 1 : 0
        border.color: Theme.outlineVariant
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Icon {
        anchors.centerIn: parent
        name: root.icon
        size: root.iconSize
        weight: root.iconWeight
        fill: root.iconFill
        color: root.iconColor
    }

    StateLayer {
        id: area
        anchors.fill: parent
        enabled: root.enabled
        radius: container.radius
        color: root.iconColor
        onClicked: root.activate()
    }
}
