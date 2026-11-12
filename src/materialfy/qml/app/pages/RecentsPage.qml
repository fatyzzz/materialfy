import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// «Недавнее» как в телефоне: по дням, подряд сыгранное из одного плейлиста/альбома — одна строка с числом треков
// (Library.recentDays: локальная история + недавнее Spotify). Клик по контексту открывает его, по треку — играет.
Page {
    id: page

    title: I18n.t("Recents")

    property var days: []
    function reload() { days = Library.recentDays().days ?? [] }
    Component.onCompleted: reload()
    // история меняется вместе с моделью «Недавние треки»
    Connections {
        target: Library.recent
        function onRowsChanged() { page.reload() }
    }

    function dayLabel(iso) {
        if (!iso)
            return ""
        const p = iso.split("-")
        const day = new Date(+p[0], +p[1] - 1, +p[2])
        const today = new Date()
        today.setHours(0, 0, 0, 0)
        const diff = Math.round((today - day) / 86400000)
        if (diff === 0)
            return I18n.t("Today")
        if (diff === 1)
            return I18n.t("Yesterday")
        return day.toLocaleDateString(Qt.locale(), diff < 7 ? "dddd" : "d MMM yyyy")
    }
    function kindLabel(kind) {
        return ({ playlist: I18n.t("Playlist"), album: I18n.t("Album"), artist: I18n.t("Artist"),
                  show: I18n.t("Podcast"), collection: I18n.t("Liked Songs"), station: I18n.t("Radio") })[kind] ?? ""
    }

    EmptyState {
        visible: page.days.length === 0
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        icon: "history"
        title: I18n.t("Nothing played yet")
    }

    Repeater {
        model: page.days
        delegate: ColumnLayout {
            id: day
            required property var modelData
            Layout.fillWidth: true
            spacing: 4
            SectionHeader { text: page.dayLabel(day.modelData.date) }
            Repeater {
                model: day.modelData.rows
                delegate: ColumnLayout {
                    id: row
                    required property var modelData
                    readonly property bool isTrack: modelData.kind === "track"
                    readonly property var items: modelData.items ?? []
                    property bool open: false
                    Layout.fillWidth: true
                    spacing: 0
                    Item {
                        Layout.fillWidth: true
                        implicitHeight: 64
                        StateLayer {
                            anchors.fill: parent
                            radius: Theme.shape.medium
                            // как в телефоне: по контексту — что из него играло, а не сам плейлист
                            onClicked: row.isTrack ? Player.playUris([row.modelData.uri], 0) : row.open = !row.open
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            spacing: 12
                            MorphImage {
                                Layout.preferredWidth: 48
                                Layout.preferredHeight: 48
                                source: row.modelData.cover ?? ""
                                shape: row.modelData.kind === "artist" ? "circle" : "softSquare"
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    Layout.fillWidth: true
                                    text: row.modelData.title || (row.items[0]?.title ?? "")
                                    type: "bodyLarge"
                                    elide: Text.ElideRight
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: row.isTrack ? (row.modelData.subtitle ?? "")
                                          : [page.kindLabel(row.modelData.kind), row.modelData.tracks > 1 ? I18n.n(row.modelData.tracks, "{0} track", "{0} tracks") : ""]
                                                .filter(x => x).join(" · ")
                                    type: "bodyMedium"
                                    color: Theme.fgSurfaceVariant
                                }
                            }
                            Icon {
                                visible: !row.isTrack
                                name: "expand_more"
                                rotation: row.open ? 180 : 0
                                size: 24
                                color: Theme.fgSurfaceVariant
                            }
                        }
                    }
                    Repeater {
                        model: row.open ? row.items : []
                        delegate: Item {
                            id: item
                            required property var modelData
                            Layout.fillWidth: true
                            implicitHeight: 52
                            StateLayer {
                                anchors.fill: parent
                                radius: Theme.shape.medium
                                onClicked: row.modelData.kind === "station" ? Player.playUris([item.modelData.uri], 0)
                                                                            : Player.playContext(row.modelData.uri, item.modelData.uri)
                            }
                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 68
                                anchors.rightMargin: 8
                                spacing: 12
                                MorphImage {
                                    Layout.preferredWidth: 36
                                    Layout.preferredHeight: 36
                                    source: item.modelData.cover ?? ""
                                    shape: "softSquare"
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0
                                    Label { Layout.fillWidth: true; text: item.modelData.title ?? ""; type: "bodyMedium"; elide: Text.ElideRight }
                                    Label {
                                        Layout.fillWidth: true
                                        text: item.modelData.artists ?? ""
                                        type: "bodySmall"
                                        color: Theme.fgSurfaceVariant
                                        elide: Text.ElideRight
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
