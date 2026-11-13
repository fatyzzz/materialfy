import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Плитка «Быстрого доступа» на главной: 64 px, обложка 48 (softSquare → cookie9 при наведении),
// название, значок у играющего контекста, «Слушать» проявляется по наведению. entry — LIBRARY_KEYS.
Item {
    id: root

    property var entry: ({})
    readonly property bool liked: (entry.kind ?? "") === "liked"
    readonly property bool hovered: area.containsMouse || playArea.containsMouse
    readonly property bool playingHere: Player.hasTrack && (liked ? Player.source === "liked"
                                        : (entry.uri ?? "") !== "" && Player.contextUri === entry.uri)

    signal clicked()
    signal playClicked()
    signal menuRequested(point point)

    implicitHeight: 64
    implicitWidth: 300

    Rectangle {
        anchors.fill: parent
        radius: Theme.shape.large
        color: Theme.surfaceContainerHigh
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
    Rectangle {
        anchors.fill: parent
        radius: Theme.shape.large
        color: Theme.fgSurface
        opacity: area.pressed ? Theme.stateLayer.pressed : root.hovered ? Theme.stateLayer.hover : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 12
        spacing: 12

        Item {
            Layout.preferredWidth: 48
            Layout.preferredHeight: 48
            MorphShape {
                anchors.fill: parent
                visible: root.liked
                shape: root.hovered ? "cookie9" : "softSquare"
                color: Theme.primaryContainer
                Icon { anchors.centerIn: parent; name: "favorite"; size: 22; fill: 1; color: Theme.fgPrimaryContainer }
            }
            MorphImage {
                anchors.fill: parent
                visible: !root.liked
                source: root.entry.cover ?? ""
                shape: root.hovered ? "cookie9" : ((root.entry.kind ?? "") === "artist" ? "circle" : "softSquare")
            }
        }
        Label {
            Layout.fillWidth: true
            text: root.liked ? I18n.t("Liked Songs") : (root.entry.title ?? "")
            type: "titleSmall"
            fade: true
        }
        Icon {
            visible: root.playingHere && !root.hovered
            name: "volume_up"
            size: 20
            color: Theme.primary
        }
        Item {
            Layout.preferredWidth: 40
            Layout.preferredHeight: 40
            visible: root.hovered || root.playingHere
            PlayButton {
                anchors.fill: parent
                size: 40
                spin: false
                pausedShape: "cookie6"
                playingShape: "cookie9"
                playing: root.playingHere && Player.playing
                opacity: root.hovered ? 1 : 0
                scale: root.hovered ? 1 : 0.8
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                onClicked: root.playClicked()
            }
            MouseArea { id: playArea; anchors.fill: parent; hoverEnabled: !Theme.mobile; acceptedButtons: Qt.NoButton }
        }
    }
}
