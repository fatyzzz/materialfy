import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Строка библиотеки (вид «список»): обложка 48 (исполнитель — круг, папка — иконка, Любимые — градиент),
// название + булавка / «только чтение», подпись, колонки «Автор», «Добавлено», «Слушали» (последняя
// прячется в узком окне), ⋯ по наведению. entry — словарь LIBRARY_KEYS.
Item {
    id: root

    property var entry: ({})
    property bool showPlayed: width >= 860
    property bool showAdded: width >= 640
    readonly property string kind: entry.kind ?? ""

    signal clicked()
    signal menuRequested(point point)

    implicitHeight: 64

    function date(iso) {
        if (!iso)
            return "—"
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return "—"
        return I18n.date(d)
    }
    function ago(iso) {
        if (!iso)
            return "—"
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return "—"
        return I18n.relative(d)
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 16

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredWidth: 3
            spacing: 12

            Item {
                Layout.preferredWidth: 48
                Layout.preferredHeight: 48
                Rectangle {
                    anchors.fill: parent
                    visible: root.kind === "liked"
                    radius: 12
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: Theme.primaryContainer }
                        GradientStop { position: 1; color: Theme.tertiaryContainer }
                    }
                    Icon { anchors.centerIn: parent; name: "favorite"; size: 22; fill: 1; color: Theme.fgPrimaryContainer }
                }
                Rectangle {
                    anchors.fill: parent
                    visible: root.kind === "folder"
                    radius: 12
                    color: Theme.surfaceContainerHigh
                    Icon { anchors.centerIn: parent; name: "folder"; size: 22; color: Theme.fgSurfaceVariant }
                }
                MorphImage {
                    anchors.fill: parent
                    visible: root.kind !== "liked" && root.kind !== "folder" && (root.entry.cover ?? "") !== ""
                    source: visible ? root.entry.cover : ""
                    shape: root.kind === "artist" ? (hover.hovered ? "cookie12" : "circle") : (hover.hovered ? "cookie9" : "softSquare")
                }
                MorphShape {
                    anchors.fill: parent
                    visible: root.kind !== "liked" && root.kind !== "folder" && (root.entry.cover ?? "") === ""
                    shape: root.kind === "artist" ? "circle" : "softSquare"
                    color: Theme.surfaceContainerHigh
                    Icon {
                        anchors.centerIn: parent
                        name: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts" })[root.kind] ?? "music_note"
                        size: 22
                        color: Theme.fgSurfaceVariant
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Label {
                        Layout.fillWidth: implicitWidth > width
                        Layout.maximumWidth: Math.ceil(implicitWidth)
                        text: root.kind === "liked" ? I18n.t("Liked Songs") : (root.entry.title ?? "")
                        type: "bodyLarge"
                        weight: 500
                    }
                    Icon { visible: root.entry.pinned === true; name: "push_pin"; size: 18; fill: 1; color: Theme.primary }
                    ReadOnlyBadge { visible: root.kind === "folder" }
                    Item { Layout.fillWidth: true }
                }
                Label {
                    Layout.fillWidth: true
                    text: root.entry.subtitle ?? ""
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
            }
        }

        Label {
            // телефон: владелец — уже в подписи под названием, отдельной колонке нет места
            visible: !Theme.mobile
            Layout.fillWidth: true
            Layout.preferredWidth: 1.4
            text: root.kind === "liked" ? I18n.t("You") : (root.entry.owner ?? "")
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
        Label {
            visible: root.showAdded
            Layout.preferredWidth: 120
            text: root.date(root.entry.addedAt)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
        Label {
            visible: root.showPlayed
            Layout.preferredWidth: 120
            text: root.ago(root.entry.playedAt)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            // «На прошлой неделе» — в две строки, а не с многоточием
            wrapMode: Text.WordWrap
            maximumLineCount: 2
        }
        IconButton {
            id: more
            Layout.preferredWidth: Theme.mobile ? 40 : 32
            Layout.preferredHeight: Theme.mobile ? 40 : 32
            size: Theme.mobile ? "s" : "xs"
            iconSize: Theme.mobile ? 24 : 26
            iconWeight: Theme.mobile ? 500 : 700
            icon: Theme.mobile ? "more_vert" : "more_horiz"
            opacity: Theme.mobile || hover.hovered || pressed ? 1 : 0
            onClicked: root.menuRequested(more.mapToItem(null, 0, more.height))
        }
    }

    HoverHandler { id: hover }
    TapHandler {
        acceptedButtons: Qt.RightButton
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad   // касание — не правый клик: у него нет кнопок, TapHandler принял бы любой тап
        onTapped: point => root.menuRequested(root.mapToItem(null, point.position.x, point.position.y))
    }
    StateLayer {
        z: -1
        anchors.fill: parent
        radius: Theme.shape.large
        onClicked: root.clicked()
        onPressAndHold: mouse => {
            if (Theme.mobile) {
                MobileShell.haptic()
                root.menuRequested(mapToItem(null, mouse.x, mouse.y))
            }
            else
                mouse.accepted = false
        }
    }
}
