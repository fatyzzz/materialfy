import QtQuick

// Кнопка MD3 Expressive: style filled | tonal | outlined | text | elevated, size xs | s | m | l | xl.
// При нажатии скругление «сжимается» (shape morph); toggle-кнопка в выбранном состоянии становится квадратнее.
Item {
    id: root

    property string text
    property string icon
    property string style: "filled"
    property string size: "s"
    property bool checkable: false
    property bool checked: false
    // false — состояние задаётся только снаружи (checked: binding), клик лишь шлёт clicked()
    property bool autoToggle: true
    readonly property alias pressed: area.pressed
    readonly property alias hovered: area.containsMouse

    signal clicked()

    Accessible.role: checkable ? Accessible.CheckBox : Accessible.Button
    Accessible.name: text
    Accessible.checkable: checkable
    Accessible.checked: checked
    Accessible.onPressAction: root.activate()
    activeFocusOnTab: true
    Keys.onReturnPressed: root.activate()
    Keys.onSpacePressed: root.activate()

    function activate() {
        if (!enabled)
            return
        if (checkable && autoToggle)
            checked = !checked
        clicked()
    }

    readonly property var _spec: ({
        xs: { h: 32,  pad: 12, gap: 4,  icon: 20, type: "labelLarge",    pressedR: 8,  squareR: 12 },
        s:  { h: 40,  pad: 16, gap: 8,  icon: 20, type: "labelLarge",    pressedR: 8,  squareR: 12 },
        m:  { h: 56,  pad: 24, gap: 8,  icon: 24, type: "titleMedium",   pressedR: 12, squareR: 16 },
        l:  { h: 96,  pad: 48, gap: 12, icon: 32, type: "headlineSmall", pressedR: 16, squareR: 28 },
        xl: { h: 136, pad: 64, gap: 16, icon: 40, type: "headlineLarge", pressedR: 16, squareR: 28 }
    })[size]

    readonly property bool _selected: checkable && checked

    readonly property color containerColor: {
        if (!enabled)
            return style === "text" || style === "outlined" ? "transparent" : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContainer)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.surfaceContainer : Theme.primary
        case "tonal": return _selected ? Theme.secondary : Theme.secondaryContainer
        case "elevated": return _selected ? Theme.primary : Theme.surfaceContainerLow
        case "outlined": return _selected ? Theme.inverseSurface : "transparent"
        default: return "transparent"
        }
    }
    readonly property color contentColor: {
        if (!enabled)
            return Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.fgSurfaceVariant : Theme.fgPrimary
        case "tonal": return _selected ? Theme.fgSecondary : Theme.fgSecondaryContainer
        case "elevated": return _selected ? Theme.fgPrimary : Theme.primary
        case "outlined": return _selected ? Theme.fgInverseSurface : Theme.fgSurfaceVariant
        default: return Theme.primary
        }
    }

    property real cornerRadius: area.pressed ? _spec.pressedR : _selected ? _spec.squareR : height / 2
    Behavior on cornerRadius {
        NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: Theme.motion.overshoot }
    }

    implicitHeight: _spec.h
    implicitWidth: content.implicitWidth + 2 * _spec.pad
    opacity: 1

    Rectangle {
        id: container
        anchors.fill: parent
        radius: Math.min(root.cornerRadius, height / 2)
        color: root.containerColor
        border.width: root.style === "outlined" && !root._selected ? 1 : 0
        border.color: root.enabled ? Theme.outlineVariant : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContainer)
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Elevation {
        target: container
        level: root.style === "elevated" && root.enabled ? (area.containsMouse ? 2 : 1) : 0
    }

    Row {
        id: content
        anchors.centerIn: parent
        spacing: root._spec.gap

        Icon {
            visible: root.icon !== ""
            tight: true   // отступы MD3 — от видимой иконки, иначе узкие глифы (logout) «съезжают»
            name: root.icon
            size: root._spec.icon
            fill: root._selected ? 1 : 0
            color: root.contentColor
            anchors.verticalCenter: parent.verticalCenter
        }
        Label {
            visible: root.text !== ""
            text: root.text
            type: root._spec.type
            color: root.contentColor
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    StateLayer {
        id: area
        anchors.fill: parent
        enabled: root.enabled
        radius: container.radius
        color: root.contentColor
        onClicked: root.activate()
    }
}
