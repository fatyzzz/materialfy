import QtQuick

// Filter chip MD3. Состояние `selected` задаётся снаружи и не переключается само,
// поэтому группа чипов работает как радио: selected: model === current; onClicked: current = model.
Item {
    id: root

    property string text
    property string icon
    property bool selected: false
    readonly property alias pressed: area.pressed

    signal clicked()

    Accessible.role: Accessible.RadioButton
    Accessible.name: text
    Accessible.checkable: true
    Accessible.checked: selected
    Accessible.onPressAction: if (enabled) clicked()
    activeFocusOnTab: true
    Keys.onReturnPressed: if (enabled) clicked()
    Keys.onSpacePressed: if (enabled) clicked()

    implicitHeight: 32
    // без текста — квадратный чип с одной иконкой
    readonly property bool _iconOnly: text === "" && _leading
    implicitWidth: _iconOnly ? 32 : _leftPad + content.implicitWidth + 16
    readonly property bool _leading: selected || icon !== ""
    readonly property real _leftPad: _iconOnly ? 7 : _leading ? 10 : 16   // для иконки — до её видимого контура

    property real cornerRadius: area.pressed ? 4 : 8
    Behavior on cornerRadius { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
    Behavior on implicitWidth { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springDefault } }

    // галочка выбранного чипа «выпрыгивает», а не появляется рывком
    onSelectedChanged: if (selected && Theme.reducedMotion !== true) checkPop.restart()
    NumberAnimation {
        id: checkPop
        target: leadIcon; property: "scale"
        from: 0.2; to: 1
        duration: Theme.motion.spatialFast
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast
    }

    Rectangle {
        anchors.fill: parent
        radius: root.cornerRadius
        color: root.selected ? Theme.secondaryContainer : "transparent"
        border.width: root.selected ? 0 : 1
        border.color: Theme.outlineVariant
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Row {
        id: content
        anchors.left: parent.left
        anchors.leftMargin: root._leftPad
        anchors.verticalCenter: parent.verticalCenter
        spacing: 6

        Icon {
            id: leadIcon
            visible: root._leading
            tight: true
            name: root.selected ? "check" : root.icon
            size: 18
            color: root.selected ? Theme.fgSecondaryContainer : Theme.primary
            anchors.verticalCenter: parent.verticalCenter
        }
        Label {
            visible: root.text !== ""
            text: root.text
            type: "labelLarge"
            color: root.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
            anchors.verticalCenter: parent.verticalCenter
            Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
        }
    }

    StateLayer {
        id: area
        anchors.fill: parent
        radius: root.cornerRadius
        color: root.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        onClicked: root.clicked()
    }
}
