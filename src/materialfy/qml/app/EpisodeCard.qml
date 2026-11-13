import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// «Продолжить подкасты»: карточка 300×104, обложка 80, выпуск, шоу, прямой прогресс 4 px,
// «Осталось N мин» / «Новый выпуск». episode — EPISODE_KEYS.
Item {
    id: root

    property var episode: ({})
    readonly property real progress: (episode.durationMs ?? 0) > 0 ? Math.min(1, (episode.resumeMs ?? 0) / episode.durationMs) : 0
    readonly property bool started: (episode.resumeMs ?? 0) > 0 && episode.fullyPlayed !== true
    readonly property bool hovered: area.containsMouse

    signal clicked()
    signal menuRequested(point point)

    implicitWidth: 300
    implicitHeight: 104

    Rectangle {
        anchors.fill: parent
        radius: 24
        color: Theme.surfaceContainerHigh
    }
    Rectangle {
        anchors.fill: parent
        radius: 24
        color: Theme.fgSurface
        opacity: area.pressed ? Theme.stateLayer.pressed : root.hovered ? Theme.stateLayer.hover : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 12
        MorphImage {
            Layout.preferredWidth: 80
            Layout.preferredHeight: 80
            source: root.episode.cover ?? ""
            shape: root.hovered ? "cookie9" : "softSquare"
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Label { Layout.fillWidth: true; text: root.episode.title ?? ""; type: "titleSmall" }
            Label { Layout.fillWidth: true; text: root.episode.showName ?? ""; type: "bodySmall"; color: Theme.fgSurfaceVariant }
            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: 8
                height: 4
                radius: 2
                color: Theme.secondaryContainer
                Rectangle {
                    width: parent.width * root.progress
                    height: parent.height
                    radius: 2
                    color: Theme.primary
                }
            }
            Label {
                Layout.topMargin: 6
                text: root.started
                      ? I18n.t("{0} min left", Math.max(1, Math.round(((root.episode.durationMs ?? 0) - (root.episode.resumeMs ?? 0)) / 60000)))
                      : root.episode.fullyPlayed === true ? I18n.t("Played") : I18n.t("New episode")
                type: "labelSmall"
                color: root.started || root.episode.fullyPlayed === true ? Theme.fgSurfaceVariant : Theme.primary
            }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: !Theme.mobile
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => mouse.button === Qt.RightButton ? root.menuRequested(root.mapToItem(null, mouse.x, mouse.y)) : root.clicked()
        // телефон: меню — долгим нажатием (правой кнопки нет)
        onPressAndHold: mouse => { if (Theme.mobile) { MobileShell.haptic(); root.menuRequested(root.mapToItem(null, mouse.x, mouse.y)) } else mouse.accepted = false }
    }
}
