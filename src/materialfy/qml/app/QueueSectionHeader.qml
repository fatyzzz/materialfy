import QtQuick
import QtQuick.Layouts
import Md3

// Заголовок секции очереди: подпись, бейдж-счётчик, волнистая линия до края, необязательная кнопка.
Item {
    id: root

    property string title
    property int count: -1                 // < 0 — без бейджа
    property string actionIcon
    property string actionText             // подсказка кнопки
    property string note                   // строка под заголовком («Новые треки встают в конец»)

    signal action()

    implicitHeight: col.implicitHeight + 16
    implicitWidth: 240

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 8
        anchors.rightMargin: 4
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 4
        spacing: 2

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(32, Theme.type.titleSmall.line, Theme.type.labelSmall.line + 4)
            spacing: 8

            Label {
                Layout.preferredWidth: implicitWidth
                Layout.maximumWidth: col.width - 96
                text: root.title
                type: "titleSmall"
                color: Theme.fgSurfaceVariant
            }
            Rectangle {
                visible: root.count >= 0
                implicitWidth: Math.max(20, countLabel.implicitWidth + 12)
                implicitHeight: Math.max(20, Theme.type.labelSmall.line + 4)
                radius: height / 2
                color: Theme.secondaryContainer
                Label {
                    id: countLabel
                    anchors.centerIn: parent
                    text: root.count
                    type: "labelSmall"
                    color: Theme.fgSecondaryContainer
                }
            }
            WavyDivider {
                Layout.fillWidth: true
                Layout.preferredHeight: 8
                Layout.minimumWidth: 16
            }
            IconButton {
                visible: root.actionIcon !== ""
                icon: root.actionIcon
                size: "xs"
                onClicked: root.action()
            }
        }
        Label {
            visible: root.note !== ""
            Layout.fillWidth: true
            text: root.note
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
        }
    }
}
