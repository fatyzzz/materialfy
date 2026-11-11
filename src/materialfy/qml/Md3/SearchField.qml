import QtQuick
import Materialfy.Core

// Поле поиска-пилюля: обводка primary в фокусе, кнопка очистки.
Item {
    id: root

    property alias text: input.text
    property string placeholder: I18n.t("Search")
    property string leadingIcon: "search"
    property alias echoMode: input.echoMode
    readonly property alias inputItem: input

    signal accepted(string text)

    implicitWidth: 360
    implicitHeight: Math.max(48, Theme.type.bodyLarge.line + 16)

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceContainerHigh
        border.width: input.activeFocus ? 2 : 0
        border.color: Theme.primary
        Behavior on border.width { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    StateLayer {
        anchors.fill: parent
        radius: height / 2
        ripple: false
        cursorShape: Qt.IBeamCursor
        onClicked: input.forceActiveFocus()
    }

    Icon {
        id: lead
        anchors.left: parent.left
        anchors.leftMargin: 16
        anchors.verticalCenter: parent.verticalCenter
        name: root.leadingIcon
        size: 22
        color: input.activeFocus ? Theme.primary : Theme.fgSurfaceVariant
    }

    TextInput {
        id: input
        Component.onCompleted: MobileShell.prepareAccessibility(input, root)
        anchors.left: lead.right
        anchors.leftMargin: 12
        anchors.right: clear.left
        anchors.rightMargin: 4
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        verticalAlignment: TextInput.AlignVCenter
        Accessible.name: root.placeholder
        color: Theme.fgSurface
        selectionColor: Theme.primaryContainer
        selectedTextColor: Theme.fgPrimaryContainer
        font.family: Theme.fontFamily
        font.pixelSize: Theme.type.bodyLarge.size
        font.letterSpacing: Theme.type.bodyLarge.tracking
        font.variableAxes: Theme.fontAxes(Theme.type.bodyLarge.size, 400)
        clip: true
        // без фокуса длинный текст виден с начала, а не прокручен к курсору в конце
        autoScroll: activeFocus
        onAccepted: root.accepted(text)

        Label {
            anchors.fill: parent
            visible: !input.text && !input.preeditText
            text: root.placeholder
            type: "bodyLarge"
            color: Theme.fgSurfaceVariant
        }
    }

    IconButton {
        id: clear
        anchors.right: parent.right
        anchors.rightMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        size: "s"
        icon: "close"
        Accessible.name: I18n.t("Clear search")
        opacity: input.text ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        onClicked: { input.clear(); input.forceActiveFocus() }
    }
}
