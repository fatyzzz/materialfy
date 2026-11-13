import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Папка rootlist (только чтение): фигура cookie4 → cookie9 при наведении, мозаика до 4 обложек,
// бейдж папки внизу слева.
Item {
    id: root
    // телефон: нажатие чуть поджимает плитку; быстрый тап — короткой вспышкой
    scale: Theme.mobile && (area.pressed || tapFlash.running) ? 0.97 : 1
    Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }
    Timer { id: tapFlash; interval: 160 }

    property string title
    property string subtitle
    property var covers: []
    property real size: 168
    readonly property bool hovered: area.containsMouse

    signal clicked()
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

            MorphShape {
                anchors.fill: parent
                shape: root.hovered ? "cookie9" : "cookie4"
                color: Theme.surfaceContainerHigh
            }
            Grid {
                anchors.fill: parent
                anchors.margins: 24
                columns: 2
                spacing: 6
                readonly property real cell: (width - spacing) / 2
                Repeater {
                    // меньше четырёх обложек — повторяются, чтобы мозаика была полной
                    model: {
                        const c = (root.covers ?? []).filter(x => x)
                        if (c.length === 0)
                            return []
                        const out = []
                        for (let i = 0; i < 4; i++)
                            out.push(c[i % c.length])
                        return out
                    }
                    MorphImage {
                        required property string modelData
                        width: parent.cell
                        height: parent.cell
                        source: modelData
                        shape: "softSquare"
                    }
                }
            }
            Icon {
                anchors.centerIn: parent
                visible: (root.covers ?? []).length === 0
                name: "folder"
                size: root.size * 0.3
                color: Theme.fgSurfaceVariant
            }
            Rectangle {
                x: 10
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 10
                width: 36
                height: 36
                radius: 12
                color: Theme.secondaryContainer
                Icon { anchors.centerIn: parent; name: "folder"; size: 20; fill: 1; color: Theme.fgSecondaryContainer }
            }
        }

        Label {
            Layout.preferredWidth: root.size
            text: root.title
            type: "titleSmall"
        }
        Label {
            visible: root.subtitle !== ""
            Layout.preferredWidth: root.size
            Layout.topMargin: -6
            text: root.subtitle
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
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
}
