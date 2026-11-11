import QtQuick
import QtQuick.Layouts
import QtQuick.Dialogs
import Md3
import Materialfy.Core

// Шапка плейлиста в режиме правки: название и описание правятся прямо в шапке (сохранение по Enter или
// потере фокуса — сразу в Spotify), «Сменить» на обложке, «Готово», «Добавить треки», «Публичный».
// Каждое изменение уходит в Web API сразу (транзакций нет); ошибки откатывает сервис (Library.notice).
ColumnLayout {
    id: root

    required property string playlistId
    property var info: ({})                // Library.playlistInfo
    property string cover
    property bool own: false               // свой (не совместный) — можно менять название, описание, видимость
    property bool panelOpen: false
    property real pageWidth: 1000

    signal done()
    signal addTracks()
    signal notify(string text)

    spacing: 20

    // Switch сам переключает checked (привязка рвётся) — свежие данные плейлиста возвращают её значение
    onInfoChanged: publicSwitch.checked = info.public === true

    function _commitTitle() {
        const t = titleInput.text.trim()
        if (t === "" ) {
            titleInput.text = root.info.title ?? ""
            return
        }
        if (t !== (root.info.title ?? "")) {
            Library.renamePlaylist(root.playlistId, t)
            root.notify(I18n.t("Name saved to Spotify"))
        }
    }
    function _commitDescription() {
        const d = descInput.text.trim()
        if (d !== (root.info.description ?? "")) {
            Library.setDescription(root.playlistId, d)
            root.notify(I18n.t("Description saved"))
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: root.pageWidth < 560 ? 18 : 28

        // Обложка: cookie9 → softSquare в правке, «Сменить» поверх
        Item {
            Layout.preferredWidth: root.pageWidth > 760 ? 184 : root.pageWidth < 560 ? 96 : 128
            Layout.preferredHeight: Layout.preferredWidth
            Layout.alignment: Qt.AlignTop
            MorphImage {
                anchors.fill: parent
                visible: root.cover !== ""
                source: root.cover
                shape: "softSquare"
                duration: Theme.reducedMotion === true ? 0 : Theme.motion.spatialDefault
            }
            MorphShape {
                anchors.fill: parent
                visible: root.cover === ""
                shape: "softSquare"
                color: Theme.primaryContainer
                Icon { anchors.centerIn: parent; name: "queue_music"; size: parent.width * 0.38; fill: 1; color: Theme.fgPrimaryContainer }
            }
            Button {
                id: coverButton
                visible: root.own
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 10
                text: I18n.t("Change")
                icon: "photo_camera"
                style: "tonal"
                size: "xs"
                opacity: 0
                scale: 0.9
                Component.onCompleted: { opacity = 1; scale = 1 }
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                onClicked: coverDialog.open()
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignBottom
            spacing: 6

            Label {
                text: root.info.collaborative ? I18n.t("COLLABORATIVE PLAYLIST") : I18n.t("PLAYLIST")
                type: "labelLarge"
                color: Theme.primary
                font.letterSpacing: 1.2
            }

            // Название: подчёркивание outlineVariant → primary 2 px в фокусе
            Item {
                Layout.fillWidth: true
                implicitHeight: titleInput.implicitHeight + 8
                TextInput {
                    id: titleInput
                    Accessible.name: I18n.t("Name")
                    Component.onCompleted: MobileShell.prepareAccessibility(titleInput, root)
                    width: parent.width
                    text: root.info.title ?? ""
                    readOnly: !root.own
                    maximumLength: 100
                    color: Theme.fgSurface
                    selectionColor: Theme.primaryContainer
                    selectedTextColor: Theme.fgPrimaryContainer
                    selectByMouse: true
                    clip: true
                    readonly property var _t: Theme.type[root.pageWidth > 760 ? "displayMedium" : root.pageWidth < 560 ? "headlineMedium" : "displaySmall"]
                    font.family: Theme.fontFamily
                    font.pixelSize: _t.size
                    font.variableAxes: Theme.fontAxes(_t.size, 500)
                    onAccepted: { root._commitTitle(); focus = false }
                    onActiveFocusChanged: if (!activeFocus) root._commitTitle()
                    Keys.onEscapePressed: { text = root.info.title ?? ""; focus = false }
                }
                Rectangle {
                    anchors.bottom: parent.bottom
                    width: parent.width
                    height: titleInput.activeFocus ? 2 : 1
                    visible: root.own
                    color: titleInput.activeFocus ? Theme.primary : Theme.outlineVariant
                    Behavior on color { ColorAnimation { duration: Theme.motion.effectsFast } }
                }
            }

            // Описание: заливка surfaceContainerHigh 60 %, скругление сверху
            Item {
                Layout.fillWidth: true
                Layout.topMargin: 4
                visible: root.own || (root.info.description ?? "") !== ""
                implicitHeight: Math.max(56, descInput.implicitHeight + 20)
                Rectangle {
                    anchors.fill: parent
                    visible: root.own
                    topLeftRadius: Theme.shape.medium
                    topRightRadius: Theme.shape.medium
                    color: Qt.alpha(Theme.surfaceContainerHigh, 0.6)
                    Rectangle {
                        anchors.bottom: parent.bottom
                        width: parent.width
                        height: descInput.activeFocus ? 2 : 1
                        color: descInput.activeFocus ? Theme.primary : Theme.outlineVariant
                    }
                }
                TextEdit {
                    id: descInput
                    Accessible.name: I18n.t("Add a description")
                    Component.onCompleted: MobileShell.prepareAccessibility(descInput, root)
                    x: 12
                    y: 10
                    width: parent.width - 24
                    text: root.info.description ?? ""
                    readOnly: !root.own
                    wrapMode: TextEdit.Wrap
                    color: Theme.fgSurfaceVariant
                    selectionColor: Theme.primaryContainer
                    selectedTextColor: Theme.fgPrimaryContainer
                    selectByMouse: true
                    font.family: Theme.fontFamily
                    font.pixelSize: Theme.type.bodyLarge.size
                    font.variableAxes: Theme.fontAxes(Theme.type.bodyLarge.size, 400)
                    onActiveFocusChanged: if (!activeFocus) root._commitDescription()
                    // не длиннее 300 символов (ограничение Spotify)
                    onTextChanged: if (text.length > 300) { text = text.slice(0, 300); cursorPosition = 300 }
                    Keys.onReturnPressed: event => {
                        if (event.modifiers & Qt.ShiftModifier)
                            event.accepted = false
                        else
                            focus = false
                    }
                    Keys.onEscapePressed: { text = root.info.description ?? ""; focus = false }
                    Label {
                        anchors.fill: parent
                        visible: descInput.text === "" && !descInput.activeFocus
                        text: I18n.t("Add a description")
                        type: "bodyLarge"
                        color: Theme.fgSurfaceVariant
                        opacity: 0.7
                    }
                }
            }
        }
    }

    Flow {
        Layout.fillWidth: true
        spacing: 12
        Button {
            text: I18n.t("Done")
            icon: "check"
            size: "m"
            onClicked: { titleInput.focus = false; descInput.focus = false; root.done() }
        }
        Button {
            text: I18n.t("Add tracks")
            icon: "playlist_add"
            style: "tonal"
            size: "m"
            checkable: true
            autoToggle: false
            checked: root.panelOpen
            onClicked: root.addTracks()
        }
        RowLayout {
            visible: root.own
            height: 56
            spacing: 10
            Label { text: I18n.t("Public"); type: "labelLarge"; color: Theme.fgSurfaceVariant }
            Switch {
                id: publicSwitch
                checked: root.info.public === true
                onToggled: value => {
                    Library.setPublic(root.playlistId, value)
                    root.notify(value ? I18n.t("Playlist is now public") : I18n.t("Playlist is now private: only you can see it"))
                }
            }
        }
        RowLayout {
            height: 56
            spacing: 6
            Icon { name: "cloud_done"; size: 18; color: Theme.fgSurfaceVariant }
            Label { text: I18n.t("Changes are saved to Spotify right away"); type: "bodySmall"; color: Theme.fgSurfaceVariant }
        }
    }

    FileDialog {
        id: coverDialog
        title: I18n.t("Playlist cover")
        nameFilters: [I18n.t("Images ({0})", "*.jpg *.jpeg *.png")]
        onAccepted: {
            Library.uploadCover(root.playlistId, selectedFile.toString())
            root.notify(I18n.t("Cover: JPEG up to 256 KB — Spotify will apply it in a couple of minutes"))
        }
    }
}
