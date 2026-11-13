import QtQuick
import Materialfy.Core

// Поле ввода MD3: подпись сверху, поле 48 px с r = 16, в фокусе — обводка primary 2 px.
// trailingIcon "visibility" у поля пароля переключает показ символов.
Item {
    id: root

    property alias text: input.text
    property string label
    property string placeholderText
    property string leadingIcon
    property string trailingIcon
    property int echoMode: TextInput.Normal
    property alias validator: input.validator
    property alias inputMethodHints: input.inputMethodHints
    property alias maximumLength: input.maximumLength
    property bool error: false
    property string supportingText
    readonly property alias inputItem: input
    readonly property bool _password: echoMode === TextInput.Password
    property bool _reveal: false

    signal accepted()
    signal trailingClicked()
    signal edited()

    implicitWidth: 240
    // в строке настроек на узком телефоне — не шире строки
    property real maxWidth: parent && parent.objectName === "settingSlot" ? parent.availableWidth : Infinity
    width: Math.min(implicitWidth, maxWidth)
    implicitHeight: column.implicitHeight
    opacity: enabled ? 1 : Theme.stateLayer.disabledContent

    function forceActiveFocus() { input.forceActiveFocus() }

    Column {
        id: column
        width: parent.width
        spacing: 6

        Label {
            visible: root.label !== ""
            width: parent.width
            text: root.label
            type: "labelMedium"
            color: root.error ? Theme.error : input.activeFocus ? Theme.primary : Theme.fgSurfaceVariant
            Behavior on color { ColorAnimation { duration: Theme.motion.effectsFast } }
        }

        Item {
            width: parent.width
            height: Math.max(48, Theme.type.bodyLarge.line + 16)

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.large
                color: Theme.surfaceContainerHighest
                border.width: input.activeFocus || root.error ? 2 : 0
                border.color: root.error ? Theme.error : Theme.primary
                Behavior on border.width { NumberAnimation { duration: Theme.motion.effectsFast } }
            }

            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.large
                ripple: false
                cursorShape: Qt.IBeamCursor
                onClicked: input.forceActiveFocus()
            }

            Icon {
                id: lead
                visible: root.leadingIcon !== ""
                anchors.left: parent.left
                anchors.leftMargin: 14
                anchors.verticalCenter: parent.verticalCenter
                name: root.leadingIcon
                size: 20
                color: input.activeFocus ? Theme.primary : Theme.fgSurfaceVariant
            }

            TextInput {
                id: input
                Component.onCompleted: MobileShell.prepareAccessibility(input, root)
                Accessible.name: root.label !== "" ? root.label : root.placeholderText
                anchors.left: lead.visible ? lead.right : parent.left
                anchors.leftMargin: lead.visible ? 10 : 16
                anchors.right: trail.visible ? trail.left : parent.right
                anchors.rightMargin: trail.visible ? 2 : 16
                anchors.verticalCenter: parent.verticalCenter
                color: Theme.fgSurface
                selectionColor: Theme.primaryContainer
                selectedTextColor: Theme.fgPrimaryContainer
                selectByMouse: true
                echoMode: root._password && root._reveal ? TextInput.Normal : root.echoMode
                font.family: Theme.fontFamily
                font.pixelSize: Theme.type.bodyLarge.size
                font.letterSpacing: Theme.type.bodyLarge.tracking
                font.variableAxes: Theme.fontAxes(Theme.type.bodyLarge.size, 400)
                clip: true
                // без фокуса длинный текст виден с начала, а не прокручен к курсору в конце
                autoScroll: activeFocus
                onAccepted: root.accepted()
                onTextEdited: root.edited()

                Label {
                    anchors.fill: parent
                    verticalAlignment: Text.AlignVCenter
                    visible: !input.text && !input.preeditText
                    text: root.placeholderText
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
            }

            IconButton {
                id: trail
                visible: root.trailingIcon !== "" || root._password
                anchors.right: parent.right
                anchors.rightMargin: 4
                anchors.verticalCenter: parent.verticalCenter
                size: "xs"
                icon: root._password ? (root._reveal ? "visibility_off" : "visibility")
                                     : root.trailingIcon
                onClicked: {
                    if (root._password)
                        root._reveal = !root._reveal
                    root.trailingClicked()
                }
            }
        }

        Label {
            visible: root.supportingText !== ""
            width: parent.width
            text: root.supportingText
            type: "bodySmall"
            color: root.error ? Theme.error : Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
        }
    }
}
