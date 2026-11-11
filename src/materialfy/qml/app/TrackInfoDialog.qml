import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Окно «О треке» (пункт меню трека): всё, что о нём известно. Сразу — то, что было в строке трека;
// остальное (прослушивания, дата выпуска, лейбл, номер, ISRC, популярность, правообладатели) —
// по Catalog.requestTrackDetails → trackDetailsReady.
Item {
    id: root

    property var track: ({})            // словарь строки трека (TRACK_KEYS)
    property var details: ({})          // ответ trackDetailsReady (TrackDetails), пусто — ещё не пришёл
    property bool loading: false
    property bool opened: false
    onVisibleChanged: visible ? BackStack.push(root) : BackStack.pop(root)
    Component.onDestruction: BackStack.pop(root)

    function open(t) {
        track = t ?? ({})
        details = ({})
        loading = (track.trackId ?? "") !== ""
        opened = true
        flick.contentY = 0
        card.forceActiveFocus()
        if (loading)
            Catalog.requestTrackDetails(track)
    }
    function close() { opened = false }

    // поле: из ответа, иначе из строки
    function _v(key, fallback) {
        const d = details[key]
        if (d !== undefined && d !== null && d !== "" && d !== 0)
            return d
        const t = track[key]
        return t !== undefined && t !== null ? t : fallback
    }

    readonly property string _trackId: _v("trackId", "")
    readonly property string _uri: _v("uri", "") || (_trackId !== "" ? "spotify:track:" + _trackId : "")
    readonly property string _albumId: _v("albumId", "")
    readonly property var _refs: (_v("artistRefs", []) ?? []).filter(r => r && r.id)

    // строки «название — значение»; copy — значение можно скопировать
    readonly property var _rows: {
        const rows = []
        const add = (label, value, copy) => { if (value !== "" && value !== undefined && value !== null) rows.push({ label: label, value: String(value), copy: copy === true }) }
        const ms = Number(_v("durationMs", 0))
        if (ms > 0)
            add(I18n.t("Duration"), I18n.clock(ms))
        const plays = Number(_v("playCount", 0))
        if (plays > 0)
            add(I18n.t("Plays"), I18n.grouped(plays))
        add(I18n.t("Release date"), I18n.releaseDate(_v("releaseDate", "")))
        add(I18n.t("Label"), _v("label", ""))
        const kind = _v("albumKind", "")
        if (kind !== "")
            add(I18n.t("Release type"), I18n.any(kind))
        const number = Number(_v("trackNumber", 0)), total = Number(_v("albumTrackCount", 0))
        if (number > 0)
            add(I18n.t("Track"), total >= number ? I18n.t("{0} of {1}", number, total) : number)
        const disc = Number(_v("discNumber", 0))
        if (disc > 1)
            add(I18n.t("Disc"), disc)
        if (_v("explicit", false) === true)
            add(I18n.t("Explicit"), I18n.t("Yes"))
        const popularity = Number(details.popularity ?? -1)
        if (popularity >= 0)
            add(I18n.t("Popularity"), I18n.t("{0} / 100", popularity))
        add("ISRC", _v("isrc", ""), true)
        add(I18n.t("Spotify URI"), _uri, true)
        return rows
    }
    readonly property var _copyrights: {
        const lines = (details.copyrights ?? []).slice()
        const courtesy = details.courtesyLine ?? ""
        if (courtesy !== "")
            lines.push(courtesy)
        return lines
    }

    Connections {
        target: Catalog
        function onTrackDetailsReady(trackId, info) {
            if (!root.opened || trackId !== (root.track.trackId ?? ""))
                return
            root.details = info ?? ({})
            root.loading = false
        }
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
        width: Math.min(480, parent.width - 48)
        height: Math.min(content.implicitHeight + 48, parent.height - 48)
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        scale: root.opened ? 1 : 0.92
        opacity: root.opened ? 1 : 0
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Keys.onEscapePressed: root.close()

        Elevation { target: card; level: 3 }

        // клики по карточке не закрывают окно
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: wheel => wheel.accepted = false }

        Flickable {
            id: flick
            anchors.fill: parent
            anchors.margins: 24
            contentHeight: content.implicitHeight
            clip: contentHeight > height
            interactive: contentHeight > height
            boundsBehavior: Flickable.StopAtBounds

            ColumnLayout {
                id: content
                width: flick.width
                spacing: 16

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    MorphImage {
                        Layout.preferredWidth: 88
                        Layout.preferredHeight: 88
                        Layout.alignment: Qt.AlignTop
                        source: root._v("coverLarge", "") || root._v("cover", "")
                        shape: "softSquare"
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2
                        Label {
                            Layout.fillWidth: true
                            text: root._v("title", "")
                            type: "titleLarge"
                            weight: 600
                            wrapMode: Text.Wrap
                            maximumLineCount: 3
                        }
                        Flow {
                            Layout.fillWidth: true
                            spacing: 4
                            visible: root._refs.length > 0 || root._v("artists", "") !== ""
                            Repeater {
                                model: root._refs.length > 0 ? root._refs : [{ id: "", name: root._v("artists", "") }]
                                LinkLabel {
                                    required property var modelData
                                    required property int index
                                    text: modelData.name + (index < root._refs.length - 1 ? "," : "")
                                    type: "bodyLarge"
                                    color: Theme.fgSurfaceVariant
                                    linkEnabled: modelData.id !== ""
                                    onClicked: { root.close(); Catalog.openArtist(modelData.id) }
                                }
                            }
                        }
                        LinkLabel {
                            Layout.fillWidth: true
                            visible: text !== ""
                            text: root._v("album", "")
                            type: "bodyMedium"
                            color: Theme.fgSurfaceVariant
                            linkEnabled: root._albumId !== ""
                            onClicked: { root.close(); Catalog.openAlbum(root._albumId, root._trackId) }
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Repeater {
                        model: root._rows
                        RowLayout {
                            id: infoRow
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredHeight: 36
                            spacing: 12
                            Label {
                                // русские подписи («Прослушивания», «Ненормативная…») длиннее английских: на телефоне
                                // колонка шире, иначе они обрывались многоточием
                                Layout.preferredWidth: Theme.mobile ? Math.min(150, content.width * 0.45) : Math.min(132, content.width * 0.36)
                                text: infoRow.modelData.label
                                type: "bodyMedium"
                                color: Theme.fgSurfaceVariant
                            }
                            Label {
                                Layout.fillWidth: true
                                text: infoRow.modelData.value
                                type: "bodyMedium"
                                font.features: ({ "tnum": 1 })
                            }
                            IconButton {
                                id: copyButton
                                property bool done: false
                                visible: infoRow.modelData.copy
                                Layout.preferredWidth: 32
                                Layout.preferredHeight: 32
                                size: "xs"
                                icon: done ? "check" : "content_copy"
                                iconColor: Theme.fgSurfaceVariant
                                onClicked: {
                                    Catalog.copyLink(infoRow.modelData.value)
                                    done = true
                                    copiedReset.restart()
                                }
                                Timer { id: copiedReset; interval: 1500; onTriggered: copyButton.done = false }
                            }
                        }
                    }
                }

                RowLayout {
                    visible: root.loading
                    Layout.fillWidth: true
                    spacing: 12
                    LoadingIndicator { size: 24 }
                    Label {
                        text: I18n.t("Loading details…")
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                }

                ColumnLayout {
                    visible: root._copyrights.length > 0
                    Layout.fillWidth: true
                    spacing: 2
                    Repeater {
                        model: root._copyrights
                        Label {
                            required property var modelData
                            Layout.fillWidth: true
                            text: modelData
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                            wrapMode: Text.Wrap
                        }
                    }
                }

                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    layoutDirection: Qt.RightToLeft
                    spacing: 8
                    Button { text: I18n.t("Close"); onClicked: root.close() }
                    Button {
                        // телефон: коротко — с «Закрыть» в один ряд
                        text: Theme.mobile ? "Spotify" : I18n.t("Open in Spotify")
                        icon: "open_in_new"
                        style: "text"
                        enabled: root._uri !== ""
                        onClicked: Qt.openUrlExternally(Catalog.uriUrl(root._uri))
                    }
                }
            }
        }
    }
}
