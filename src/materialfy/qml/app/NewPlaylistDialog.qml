import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Диалог «Новый плейлист»: название → Library.createPlaylist(title, track). Без трека — пустой плейлист.
Item {
    id: root

    property var track: null
    property bool opened: false
    onVisibleChanged: visible ? BackStack.push(root) : BackStack.pop(root)
    Component.onDestruction: BackStack.pop(root)

    function open(t) {
        track = t ?? null
        field.text = ""
        opened = true
        field.inputItem.forceActiveFocus()
    }
    function close() { opened = false }
    function submit() {
        const title = field.text.trim()
        if (title === "")
            return
        if (track && (track.trackId ?? "") !== "")
            Library.createPlaylist(title, track)
        else
            Library.createPlaylist(title)
        close()
    }

    anchors.fill: parent
    visible: opened || scrim.opacity > 0

    Rectangle {
        id: scrim
        anchors.fill: parent
        color: Qt.alpha(Theme.scrim, 0.32)
        opacity: root.opened ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onClicked: root.close()
            onWheel: wheel => {}
        }
    }

    Rectangle {
        id: card
        anchors.centerIn: parent
        // телефон: открытая клавиатура закрывает низ экрана — карточка поднимается над ней
        anchors.verticalCenterOffset: Theme.mobile && Qt.inputMethod.visible ? -Qt.inputMethod.keyboardRectangle.height / 2 : 0
        width: Math.min(400, parent.width - 48)
        implicitHeight: content.implicitHeight + 48
        height: implicitHeight
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        scale: Theme.reducedMotion || root.opened ? 1 : Theme.mobile ? 0.96 : 0.92
        opacity: root.opened ? 1 : 0
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Theme.mobile ? Easing.OutCubic : Easing.OutBack } }
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Keys.onEscapePressed: root.close()

        Elevation { target: card; level: 3 }

        // клики по карточке не закрывают диалог
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons }

        ColumnLayout {
            id: content
            anchors.fill: parent
            anchors.margins: 24
            spacing: 16

            Label { text: I18n.t("New playlist"); type: "headlineSmall" }

            SearchField {
                id: field
                Layout.fillWidth: true
                placeholder: I18n.t("Name")
                leadingIcon: "queue_music"
                onAccepted: root.submit()
                Keys.onEscapePressed: root.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 8
                Button { text: I18n.t("Cancel"); style: "text"; onClicked: root.close() }
                Button {
                    text: I18n.t("Create")
                    enabled: field.text.trim() !== ""
                    onClicked: root.submit()
                }
            }
        }
    }
}
