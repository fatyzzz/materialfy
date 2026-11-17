import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Плитка альбома / исполнителя / плейлиста: обложка в фигуре MD3E, название, подпись.
// При наведении фигура морфится, у исполнителей — круг. showPlay — кнопка «Слушать» на обложке по наведению;
// badge — плашка в углу («РАДИО»), pinned — булавка (локальное закрепление). ПКМ — menuRequested(точка окна).
Item {
    id: root

    property string cover
    property string title
    property string subtitle
    property bool round: false          // исполнитель
    property bool explicit: false
    property string placeholderIcon: "album"
    // что на обложке — для запасной обложки не со Spotify (MorphImage.fallbackKind/Artist/Title)
    property string coverKind
    property string coverArtist
    property string coverTitle
    property real size: 168
    property var shape: round ? "circle" : "softSquare"
    property var hoverShape: round ? "cookie12" : "cookie9"
    property bool showPlay: false
    property bool playing: false        // играет из этой плитки — иконка паузы
    property string badge
    property bool pinned: false
    readonly property bool hovered: area.containsMouse || (playLoader.item !== null && playLoader.item.hovering)
    // Телефон: наведения нет — отклик на касание. Обложка морфится в «печеньку» и чуть поджимается; с задержкой
    // 70 мс: касание, с которого начинается прокрутка полки, не успевает её дёрнуть
    property bool _touched: false
    Timer { id: touchDelay; interval: 70; onTriggered: root._touched = area.pressed }
    // быстрый тап (короче задержки) тоже виден: короткая вспышка, пока открывается страница
    Timer { id: tapFlash; interval: 180; onTriggered: root._touched = false }
    readonly property bool _morphed: hovered || _touched

    signal clicked()
    signal playClicked()
    signal menuRequested(point point)

    implicitWidth: size
    implicitHeight: col.implicitHeight

    ColumnLayout {
        id: col
        width: root.size
        spacing: 8

        Item {
            Layout.preferredWidth: root.size
            Layout.preferredHeight: root.size
            scale: root._touched ? 0.95 : 1
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }

            MorphImage {
                anchors.fill: parent
                visible: root.cover !== ""
                source: root.cover
                fallbackKind: root.coverKind
                fallbackArtist: root.coverArtist
                fallbackTitle: root.coverTitle
                shape: root._morphed ? root.hoverShape : root.shape
                duration: Theme.motion.spatialDefault
            }
            // Заглушка, плашка, булавка и «Слушать» создаются, только когда нужны: плиток на полках сотни,
            // а без обложки, с плашкой или под курсором — единицы
            Loader {
                anchors.fill: parent
                active: root.cover === ""
                sourceComponent: MorphShape {
                    shape: root._morphed ? root.hoverShape : root.shape
                    color: Theme.surfaceContainerHigh
                    Icon { anchors.centerIn: parent; name: root.placeholderIcon; size: root.size * 0.3; color: Theme.fgSurfaceVariant }
                }
            }

            Loader {
                x: 8
                y: 8
                active: root.badge !== ""
                sourceComponent: Rectangle {
                    implicitWidth: badgeLabel.implicitWidth + 12
                    implicitHeight: 20
                    radius: 6
                    color: Theme.primary
                    Label { id: badgeLabel; anchors.centerIn: parent; text: root.badge; type: "labelSmall"; weight: 700; color: Theme.fgPrimary }
                }
            }
            Loader {
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 8
                active: root.pinned
                sourceComponent: Rectangle {
                    width: 28
                    height: 28
                    radius: 14
                    color: Theme.primaryContainer
                    Icon { anchors.centerIn: parent; name: "push_pin"; size: 16; fill: 1; color: Theme.fgPrimaryContainer }
                }
            }

            // «Слушать» по наведению: всплывает снизу и дорастает (OutBack)
            Loader {
                id: playLoader
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 8
                width: 48
                height: 48
                active: root.showPlay && (root.hovered || root.playing || item !== null)
                sourceComponent: Item {
                    id: playHost
                    readonly property bool hovering: playArea.containsMouse
                    // созданная наведением кнопка всплывает с анимацией, уже играющая — сразу на месте
                    property bool _armed: root.playing
                    Component.onCompleted: Qt.callLater(() => _armed = true)
                    readonly property bool shown: _armed && (root.hovered || root.playing)
                    visible: opacity > 0
                    opacity: shown ? 1 : 0
                    scale: shown ? 1 : 0.8
                    transform: Translate { y: playHost.shown ? 0 : 8; Behavior on y { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } } }
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                    PlayButton {
                        anchors.fill: parent
                        size: 48
                        playing: root.playing
                        spin: false
                        pausedShape: "cookie6"
                        playingShape: "cookie9"
                        onClicked: root.playClicked()
                    }
                    MouseArea { id: playArea; anchors.fill: parent; hoverEnabled: !Theme.mobile; acceptedButtons: Qt.NoButton }
                }
            }
        }

        RowLayout {
            Layout.preferredWidth: root.size
            spacing: 4
            Label {
                id: titleLabel
                // не шире плитки (минус плашка «E»): длинное название не залезает на соседнюю, а затухает
                Layout.preferredWidth: Math.min(Math.ceil(naturalWidth), root.size - (root.explicit ? 20 : 0))
                Layout.maximumWidth: Layout.preferredWidth
                Layout.alignment: root.round ? Qt.AlignHCenter : Qt.AlignLeft
                text: root.title
                type: "titleSmall"
                fade: true
                horizontalAlignment: root.round && !overflowing ? Text.AlignHCenter : Text.AlignLeft
            }
            Rectangle {
                visible: root.explicit
                implicitWidth: 16
                implicitHeight: 16
                radius: 4
                color: Theme.surfaceContainerHighest
                Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
            }
            Item { Layout.fillWidth: true; visible: !root.round }
        }
        Label {
            visible: root.subtitle !== ""
            Layout.preferredWidth: root.size
            Layout.topMargin: -6
            text: root.subtitle
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            fade: true
            horizontalAlignment: root.round && !overflowing ? Text.AlignHCenter : Text.AlignLeft
        }
    }

    MouseArea {
        id: area
        z: -1
        anchors.fill: parent
        hoverEnabled: !Theme.mobile
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onPressedChanged: {
            if (pressed && Theme.mobile) {
                touchDelay.restart()
            } else {
                touchDelay.stop()
                root._touched = false
            }
        }
        // телефон: меню — долгим нажатием (правой кнопки нет)
        onPressAndHold: mouse => { if (Theme.mobile) { MobileShell.haptic(); root.menuRequested(root.mapToItem(null, mouse.x, mouse.y)) } else mouse.accepted = false }
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton) {
                root.menuRequested(root.mapToItem(null, mouse.x, mouse.y))
            } else {
                if (Theme.mobile) {
                    root._touched = true
                    tapFlash.restart()
                }
                root.clicked()
            }
        }
    }
}
