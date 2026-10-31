import QtQuick

// Пункт навигации сайдбара: пилюля во всю ширину; у выбранного — иконка в квадратике цвета primary.
// Иконка всегда на одном и том же месте (iconInset от левого края) — и в развёрнутом, и в рейле,
// поэтому при сворачивании сайдбара меняется только ширина, ничего не прыгает.
Item {
    id: root

    property string icon
    property string text
    property bool selected: false
    property bool compact: false          // подпись и хвост гаснут, пилюля сжимается вместе с шириной
    property real iconInset: 14           // от левого края пилюли до квадрата иконки (28 px)
    property real labelWidth: 180         // фиксированная: при анимации ширины текст не переносится и не «прыгает» многоточием
    property string trailingIcon
    property color trailingColor: Theme.primary
    property string imageSource

    signal clicked()
    // ПКМ по пункту: меню действий строки (закрепить и т. п.)
    signal rightClicked(var point)

    implicitHeight: 48
    implicitWidth: 240
    clip: true

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: root.selected ? Theme.secondaryContainer : "transparent"
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Item {
        id: iconBox
        x: root.iconInset
        width: 28
        height: 28
        anchors.verticalCenter: parent.verticalCenter

        Rectangle {
            anchors.fill: parent
            radius: 8
            color: Theme.primary
            scale: root.selected && root.imageSource === "" ? 1 : 0.4
            opacity: root.selected && root.imageSource === "" ? 1 : 0
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: 2 } }
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.centerIn: parent
            visible: root.imageSource === ""
            name: root.icon
            size: 20
            fill: root.selected ? 1 : 0
            color: root.selected ? Theme.fgPrimary : Theme.fgSurfaceVariant
        }
        // Обложка (плейлисты) создаётся только при наличии картинки: скрытый MorphImage
        // всё равно участвовал в каждом кадре перекраски темы.
        Loader {
            anchors.fill: parent
            active: root.imageSource !== ""
            sourceComponent: MorphImage {
                source: root.imageSource
                shape: root.selected ? "cookie9" : "softSquare"
            }
        }
    }

    Label {
        x: iconBox.x + iconBox.width + 8   // иконка 20 в квадрате 28 → видимый зазор ~12
        anchors.verticalCenter: parent.verticalCenter
        width: root.labelWidth - (root.trailingIcon !== "" ? 28 : 0)
        text: root.text
        type: "labelLarge"
        color: root.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        opacity: root.compact ? 0 : 1
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    Icon {
        visible: root.trailingIcon !== "" && opacity > 0
        opacity: root.compact ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        anchors.right: parent.right
        anchors.rightMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        name: root.trailingIcon
        size: 18
        fill: 1
        color: root.trailingColor
    }

    StateLayer {
        id: hit
        anchors.fill: parent
        radius: height / 2
        color: Theme.fgSurface
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                root.rightClicked(mouse)
            else
                root.clicked()
        }
    }

    // В рейле подписей нет — подсказка MD3 справа от пункта после короткой задержки. Живёт в корне окна:
    // пункт и панель обрезают своё содержимое.
    Timer {
        id: tipDelay
        interval: 500
        running: root.compact && hit.containsMouse && !hit.pressed && root.text !== ""
    }
    Rectangle {
        id: tip
        readonly property bool shown: root.compact && hit.containsMouse && !hit.pressed && !tipDelay.running && root.text !== ""
        parent: root.Window.contentItem
        z: 1000
        width: Math.min(tipLabel.implicitWidth + 16, 320)
        height: 24
        radius: Theme.shape.extraSmall
        color: Theme.inverseSurface
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        onShownChanged: if (shown && parent) {
            const p = root.mapToItem(parent, root.width + 8, (root.height - height) / 2)
            x = Math.round(p.x)
            y = Math.round(p.y)
        }

        Label {
            id: tipLabel
            anchors.verticalCenter: parent.verticalCenter
            x: 8
            width: parent.width - 16
            text: root.text
            type: "bodySmall"
            color: Theme.fgInverseSurface
        }
    }
}
