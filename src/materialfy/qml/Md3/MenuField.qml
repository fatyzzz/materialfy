import QtQuick

// Выпадающий выбор MD3: пилюля 44 px со значением и arrow_drop_down, открывает Md3/Menu.
// model — список строк или {id, text}; currentValue — строка или id выбранного пункта.
Item {
    id: root

    property var model: []
    property var currentValue
    property string label
    property string placeholderText: I18n.t("Not selected")
    property string leadingIcon

    signal activated(var value)

    readonly property var _items: (model ?? []).map(m => typeof m === "object" && m !== null
        ? { id: m.id, text: m.text ?? String(m.id) }
        : { id: m, text: String(m) })
    readonly property int currentIndex: _items.findIndex(m => m.id === currentValue)
    readonly property string currentText: currentIndex >= 0 ? _items[currentIndex].text : ""

    // телефон: под подписью строки настроек — шире, чтобы значение («Системный по умолчанию») не обрезалось
    implicitWidth: Theme.mobile ? 280 : 240
    // в строке настроек на узком телефоне — не шире строки
    property real maxWidth: parent && parent.objectName === "settingSlot" ? parent.availableWidth : Infinity
    width: Math.min(implicitWidth, maxWidth)
    implicitHeight: column.implicitHeight
    opacity: enabled ? 1 : Theme.stateLayer.disabledContent

    Column {
        id: column
        width: parent.width
        spacing: 6

        Label {
            visible: root.label !== ""
            width: parent.width
            text: root.label
            type: "labelMedium"
            color: menu.opened ? Theme.primary : Theme.fgSurfaceVariant
        }

        Item {
            id: field
            width: parent.width
            height: 44

            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: Theme.surfaceContainerHighest
                border.width: menu.opened ? 2 : 0
                border.color: Theme.primary
                Behavior on border.width { NumberAnimation { duration: Theme.motion.effectsFast } }
            }

            Icon {
                id: lead
                visible: root.leadingIcon !== ""
                anchors.left: parent.left
                anchors.leftMargin: 14
                anchors.verticalCenter: parent.verticalCenter
                name: root.leadingIcon
                size: 20
            }

            Label {
                anchors.left: lead.visible ? lead.right : parent.left
                anchors.leftMargin: lead.visible ? 10 : 18
                anchors.right: arrow.left
                anchors.rightMargin: 4
                anchors.verticalCenter: parent.verticalCenter
                text: root.currentText !== "" ? root.currentText : root.placeholderText
                type: "bodyLarge"
                color: root.currentText !== "" ? Theme.fgSurface : Theme.fgSurfaceVariant
            }

            Icon {
                id: arrow
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                name: "arrow_drop_down"
                size: 24
                rotation: menu.opened ? 180 : 0
                Behavior on rotation { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
            }

            StateLayer {
                anchors.fill: parent
                radius: height / 2
                onClicked: {
                    menu.open(root._items.map(m => ({ id: String(m.id), text: m.text, checked: m.id === root.currentValue })),
                              Qt.point(0, field.height + 4))
                }
            }

            Menu {
                id: menu
                anchors.fill: parent
                onPicked: i => {
                    const item = root._items[i]
                    if (item)
                        root.activated(item.id)
                }
            }
        }
    }
}
