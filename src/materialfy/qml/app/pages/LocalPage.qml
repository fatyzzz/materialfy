import QtQuick
import QtQuick.Layouts
import QtQuick.Dialogs
import Md3
import Materialfy.Core
import ".."

// «Локальные файлы»: mp3, flac и ogg из выбранной папки. Своя очередь у плеера файлов, мимо Spotify и Connect;
// обход папки и теги читает LocalMusic вне GUI-потока, повторный обход берёт неизменённые файлы из кэша.
// Обложек нет: подбор по исполнителю (Covers) ушёл бы в сеть с тегами чужих файлов.
Item {
    id: page

    property var router
    readonly property string title: I18n.t("Local files")
    readonly property var tracks: LocalMusic.tracks
    readonly property int count: tracks ? tracks.count : 0
    // nofolder | missing | scanning | empty | list
    readonly property string view: LocalMusic.folder === "" ? "nofolder"
                                 : LocalMusic.missing ? "missing"
                                 : count > 0 ? "list"
                                 : LocalMusic.scanning ? "scanning" : "empty"

    function _time(ms) {
        const s = Math.round(ms / 1000)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    FolderDialog {
        id: folderDialog
        title: I18n.t("Choose a music folder")
        onAccepted: LocalMusic.setFolder(selectedFolder.toString())
    }

    ListView {
        id: list
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        model: page.view === "list" ? page.tracks : null
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        reuseItems: true
        cacheBuffer: 640

        header: ColumnLayout {
            width: list.width
            spacing: 16

            ColumnLayout {
                Layout.fillWidth: true
                Layout.topMargin: 24
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                spacing: 4
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Local files")
                    type: "displaySmall"
                    weight: 600
                }
                Label {
                    Layout.fillWidth: true
                    visible: LocalMusic.folder !== ""
                    text: page.count > 0 ? I18n.n(page.count, "{0} track", "{0} tracks") + " · " + LocalMusic.folder
                                         : LocalMusic.folder
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
            }

            // у пропавшей папки действие одно (выбрать другую), оно в пустом состоянии ниже
            RowLayout {
                visible: page.view !== "nofolder" && page.view !== "missing"
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                spacing: 12
                Button {
                    visible: page.count > 0
                    text: I18n.t("Play")
                    icon: "play_arrow"
                    size: "m"
                    onClicked: LocalMusic.play(0)
                }
                Button {
                    text: I18n.t("Change folder")
                    icon: "folder_open"
                    style: "tonal"
                    size: "m"
                    onClicked: folderDialog.open()
                }
                IconButton {
                    icon: "refresh"
                    size: "m"
                    enabled: !LocalMusic.scanning
                    onClicked: LocalMusic.rescan()
                }
                LoadingIndicator {
                    visible: LocalMusic.scanning && page.count > 0
                    running: visible
                    size: 32
                }
            }

            Label {
                visible: LocalMusic.errorText !== ""
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                text: LocalMusic.errorText
                type: "bodyMedium"
                color: Theme.error
            }

            Item { Layout.preferredHeight: 4 }
        }

        delegate: Item {
            id: row
            required property int index
            required property var model
            readonly property bool current: model.uri === Player.uri
            width: ListView.view.width
            implicitHeight: 64

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.large
                color: row.current ? Qt.alpha(Theme.primary, 0.12) : "transparent"
            }
            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.large
                onClicked: LocalMusic.play(row.index)
            }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 16
                spacing: 16

                Label {
                    Layout.preferredWidth: 28
                    text: row.index + 1
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    horizontalAlignment: Text.AlignRight
                }
                Item {
                    Layout.preferredWidth: 48
                    Layout.preferredHeight: 48
                    MorphShape {
                        anchors.fill: parent
                        shape: row.current ? "cookie9" : "softSquare"
                        color: row.current ? Theme.primaryContainer : Theme.surfaceContainerHighest
                    }
                    Icon {
                        anchors.centerIn: parent
                        name: row.current && Player.playing ? "graphic_eq" : "music_note"
                        size: 22
                        color: row.current ? Theme.fgPrimaryContainer : Theme.fgSurfaceVariant
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 3
                    spacing: 2
                    Label {
                        Layout.fillWidth: true
                        text: row.model.title
                        type: "bodyLarge"
                        weight: 500
                        color: row.current ? Theme.primary : Theme.fgSurface
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: row.model.artists
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                }
                Label {
                    visible: row.width > 720
                    Layout.fillWidth: true
                    Layout.preferredWidth: 2
                    text: row.model.album
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
                Label {
                    Layout.preferredWidth: 44
                    text: row.model.durationMs > 0 ? page._time(row.model.durationMs) : ""
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    horizontalAlignment: Text.AlignRight
                }
            }
        }

        // пустые состояния под шапкой, в той же прокрутке; снизу запас под плавающий мини-плеер
        footer: Item {
            width: list.width
            implicitHeight: page.view === "list" ? 120 : emptyState.implicitHeight + 160

            LoadingIndicator {
                anchors.horizontalCenter: parent.horizontalCenter
                y: 48
                visible: page.view === "scanning"
                running: visible
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                y: 112
                visible: page.view === "scanning"
                text: I18n.t("Looking for music in this folder…")
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
            }
            EmptyState {
                id: emptyState
                visible: page.view === "nofolder" || page.view === "missing" || page.view === "empty"
                anchors.horizontalCenter: parent.horizontalCenter
                y: 32
                width: Math.min(parent.width - 64, 420)
                icon: page.view === "missing" ? "search_off" : "folder_open"
                shape: "clover4"
                title: page.view === "nofolder" ? I18n.t("Play music from your computer")
                     : page.view === "missing" ? I18n.t("This folder isn’t there anymore")
                     : I18n.t("No music in this folder")
                text: page.view === "nofolder" ? I18n.t("Choose a folder with mp3, flac or ogg files. They play here, apart from your Spotify queue")
                    : page.view === "missing" ? I18n.t("It was moved, renamed or is on a drive that isn’t connected")
                    : I18n.t("Only mp3, flac and ogg files are shown, from this folder and the ones inside it")
                // у пустой папки «Сменить папку» и «Обновить» уже в шапке
                actionText: page.view === "missing" ? I18n.t("Choose another folder")
                          : page.view === "nofolder" ? I18n.t("Choose folder") : ""
                actionIcon: "folder_open"
                onAction: folderDialog.open()
            }
        }
    }
}
