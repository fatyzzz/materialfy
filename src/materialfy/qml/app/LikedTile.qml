import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Большая плитка «Любимые треки» (две ячейки сетки библиотеки): градиент primaryContainer → tertiaryContainer,
// образцы треков, фигура-ореол с сердцем (cookie12 → flower8 при наведении), «Слушать».
Item {
    id: root

    property int trackCount: Library.liked ? Library.liked.count : 0
    readonly property bool hovered: area.containsMouse || playArea.containsMouse

    signal clicked()
    signal playClicked()
    signal menuRequested(point point)

    implicitWidth: 358
    implicitHeight: 216

    // телефон: нажатие чуть поджимает карточку; быстрый тап — короткой вспышкой (страница ещё открывается)
    scale: Theme.mobile && (area.pressed || tapFlash.running) ? 0.97 : 1
    Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }
    Timer { id: tapFlash; interval: 160 }

    // «Исполнитель Трек · …» по первым строкам Любимых
    readonly property string sample: {
        const m = Library.liked
        const out = []
        for (let i = 0; m && i < Math.min(6, m.count); i++) {
            const t = m.get(i)
            out.push((t.artists ?? "") + " " + (t.title ?? ""))
        }
        return out.join(" · ")
    }

    Rectangle {
        id: card
        anchors.fill: parent
        radius: Theme.shape.extraLarge
        clip: true
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Theme.primaryContainer }
            GradientStop { position: 1; color: Theme.tertiaryContainer }
        }

        // clip у Rectangle — по прямоугольнику, не по скруглению: фигура не должна доходить до угла карточки,
        // иначе за скруглённым углом остаётся её квадратный край
        MorphShape {
            x: parent.width - width + 4
            y: -14
            width: 150
            height: 150
            shape: root.hovered ? "flower8" : "cookie12"
            color: Qt.alpha(Theme.fgPrimaryContainer, 0.18)
            Icon {
                anchors.centerIn: parent
                anchors.horizontalCenterOffset: -2
                anchors.verticalCenterOffset: 7
                name: "favorite"
                size: 56
                fill: 1
                color: Theme.fgPrimaryContainer
            }
        }

        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 24
            anchors.rightMargin: 96
            anchors.bottomMargin: 20
            spacing: 2
            Label {
                visible: text !== ""
                Layout.fillWidth: true
                Layout.bottomMargin: 8
                text: root.sample
                type: "bodySmall"
                color: Theme.fgPrimaryContainer
                opacity: 0.85
                wrapMode: Text.WordWrap
                maximumLineCount: 2
                elide: Text.ElideRight
            }
            // узкая плитка (телефон, русский): заголовок ужимается, а не заезжает под кнопку
            Label {
                Layout.fillWidth: true
                text: I18n.t("Liked Songs"); type: "headlineMedium"; weight: 500; color: Theme.fgPrimaryContainer
                fontSizeMode: Text.HorizontalFit
                minimumPixelSize: 18
                elide: Text.ElideRight
            }
            Label {
                text: Library.liked && Library.liked.loading && root.trackCount === 0 ? I18n.t("Loading…")
                    : I18n.n(root.trackCount, "{0} track", "{0} tracks")
                type: "bodyMedium"
                color: Theme.fgPrimaryContainer
                opacity: 0.8
            }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: !Theme.mobile
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton) {
                root.menuRequested(root.mapToItem(null, mouse.x, mouse.y))
                return
            }
            tapFlash.restart()
            root.clicked()
        }
        // телефон: меню — долгим нажатием (правой кнопки нет)
        onPressAndHold: mouse => { if (Theme.mobile) { MobileShell.haptic(); root.menuRequested(root.mapToItem(null, mouse.x, mouse.y)) } else mouse.accepted = false }
    }

    PlayButton {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 20
        size: 56
        spin: false
        pausedShape: "cookie6"
        playingShape: "cookie9"
        playing: Player.playing && Player.source === "liked"
        onClicked: root.playClicked()
        MouseArea { id: playArea; anchors.fill: parent; hoverEnabled: !Theme.mobile; acceptedButtons: Qt.NoButton }
    }
}
