import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Статистика прослушиваний этого ПК (Library.listeningStats): время за сегодня/неделю/месяц/всё, столбики
// по последним дням — только локальная история (у Spotify нет времени прослушивания). Топ треков и исполнителей —
// из аккаунта Spotify за выбранный период, как в телефоне; не ответил — топ этого ПК.
ColumnLayout {
    id: root

    property int days: 14
    property bool narrow: false
    readonly property var stats: { void Library.historyRevision; return Library.listeningStats(days) }
    readonly property var dayList: stats.days ?? []
    readonly property int peak: Math.max(1, ...dayList.map(d => d.seconds))
    readonly property var ranges: ["SHORT_TERM", "MEDIUM_TERM", "LONG_TERM"]
    property string range: "SHORT_TERM"
    readonly property var account: Library.accountTop ?? {}
    readonly property bool fromSpotify: account.ok === true && account.range === range
                                        && ((account.tracks ?? []).length > 0 || (account.artists ?? []).length > 0)
    Component.onCompleted: Library.loadAccountTop(range)
    onRangeChanged: Library.loadAccountTop(range)
    // страница открылась раньше входа: топ перечитывается, когда аккаунт или сессия готовы
    Connections {
        target: Auth
        function onStateChanged() { if (Auth.state === "signedIn" && !root.fromSpotify) Library.loadAccountTop(root.range) }
        function onPlaybackChanged() { if (Auth.playbackState === "ready" && !root.fromSpotify) Library.loadAccountTop(root.range) }
    }

    spacing: 12

    // "YYYY-MM-DD" — местная дата (Date.fromLocaleDateString дала бы полночь UTC, и в UTC−3 день съезжал бы назад)
    function dateOf(iso) {
        const p = iso.split("-")
        return new Date(+p[0], +p[1] - 1, +p[2])
    }

    function duration(seconds) {
        const minutes = Math.floor(seconds / 60)
        if (minutes < 60)
            return I18n.t("{0} min", minutes)
        return I18n.t("{0} h {1} min", Math.floor(minutes / 60), minutes % 60)
    }

    SectionHeader { text: I18n.t("Listening on this computer") }

    Label {
        visible: (root.stats.plays ?? 0) === 0
        Layout.fillWidth: true
        text: I18n.t("Nothing played here yet. Songs you listen to for 30 seconds or more show up here.")
        type: "bodyMedium"
        color: Theme.fgSurfaceVariant
        wrapMode: Text.WordWrap
    }

    GridLayout {
        visible: (root.stats.plays ?? 0) > 0
        Layout.fillWidth: true
        columns: root.narrow ? 2 : 4
        columnSpacing: 24
        rowSpacing: 16
        Total { seconds: root.stats.today ?? 0; label: I18n.t("Today") }
        Total { seconds: root.stats.week ?? 0; label: I18n.t("This week") }
        Total { seconds: root.stats.month ?? 0; label: I18n.t("This month") }
        Total {
            seconds: root.stats.total ?? 0
            label: root.stats.since ? I18n.t("Since {0}", root.dateOf(root.stats.since).toLocaleDateString(Qt.locale(), "d MMM yyyy")) : ""
        }
    }

    // Столбики по дням: сегодня — основным цветом, остальные — приглушённым
    Rectangle {
        visible: (root.stats.plays ?? 0) > 0
        Layout.fillWidth: true
        implicitHeight: 200
        radius: Theme.shape.large
        color: Theme.surfaceContainerHigh

        RowLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 6
            Repeater {
                model: root.dayList
                delegate: ColumnLayout {
                    id: day
                    required property var modelData
                    required property int index
                    readonly property bool today: index === root.dayList.length - 1
                    readonly property date when: root.dateOf(modelData.date)
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1            // столбцы поровну, независимо от ширины подписи
                    Layout.fillHeight: true
                    spacing: 6
                    Item {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Rectangle {
                            anchors.bottom: parent.bottom
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: Math.min(parent.width, 28)
                            height: day.modelData.seconds > 0 ? Math.max(4, parent.height * day.modelData.seconds / root.peak) : 2
                            radius: Math.min(width / 2, 8)
                            color: day.today ? Theme.primary : day.modelData.seconds > 0 ? Theme.secondary : Theme.outlineVariant
                            opacity: day.today || day.modelData.seconds === 0 ? 1 : 0.55
                        }
                        HoverHandler { id: dayHover }
                        Rectangle {
                            visible: dayHover.hovered && day.modelData.seconds > 0
                            anchors.bottom: parent.bottom
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.bottomMargin: 8
                            width: tip.implicitWidth + 16
                            height: tip.implicitHeight + 8
                            radius: Theme.shape.small
                            color: Theme.inverseSurface
                            Label {
                                id: tip
                                anchors.centerIn: parent
                                text: root.duration(day.modelData.seconds)
                                type: "labelSmall"
                                color: Theme.fgInverseSurface
                            }
                        }
                    }
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        text: day.when.toLocaleDateString(Qt.locale(), "d")
                        type: "labelSmall"
                        weight: day.today ? 700 : 400
                        color: day.today ? Theme.fgSurface : Theme.fgSurfaceVariant
                    }
                }
            }
        }
    }

    RowLayout {
        visible: root.fromSpotify || (root.stats.plays ?? 0) > 0
        Layout.fillWidth: true
        Layout.topMargin: 8
        spacing: 16
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Label { text: I18n.t("Your top"); type: "titleLarge"; weight: 600 }
            Label {
                Layout.fillWidth: true
                text: root.fromSpotify ? I18n.t("From your Spotify account, as on your phone")
                                       : I18n.t("From this computer, Spotify did not answer")
                type: "bodySmall"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
            }
        }
        ButtonGroup {
            model: [I18n.t("4 weeks"), I18n.t("6 months"), I18n.t("1 year")]
            autoSelect: false
            size: "xs"
            currentIndex: Math.max(0, root.ranges.indexOf(root.range))
            onActivated: i => root.range = root.ranges[i]
        }
    }

    GridLayout {
        visible: root.fromSpotify || (root.stats.plays ?? 0) > 0
        Layout.fillWidth: true
        columns: root.narrow ? 1 : 2
        columnSpacing: 24
        rowSpacing: 12
        TopList {
            title: I18n.t("Top songs")
            entries: root.fromSpotify ? root.account.tracks : (root.stats.topTracks ?? [])
            onPicked: entry => Player.playUris([entry.id], 0)
        }
        TopList {
            title: I18n.t("Top artists")
            artists: true
            entries: root.fromSpotify ? root.account.artists : (root.stats.topArtists ?? [])
            onPicked: entry => { if (entry.id !== "") Catalog.openUri("spotify:artist:" + entry.id) }
        }
    }

    component Total: ColumnLayout {
        property int seconds
        property string label
        Layout.fillWidth: true
        spacing: 2
        Label { Layout.fillWidth: true; text: root.duration(parent.seconds); type: "headlineSmall"; weight: 700 }
        Label { Layout.fillWidth: true; text: parent.label; type: "bodyMedium"; color: Theme.fgSurfaceVariant }
    }

    // Топ: место, обложка/аватар, название, сколько раз и сколько времени; клик — играть трек / открыть исполнителя
    component TopList: ColumnLayout {
        id: list
        property string title
        property var entries: []
        property bool artists: false
        signal picked(var entry)
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignTop
        spacing: 4
        Label { text: list.title; type: "titleMedium"; weight: 600; Layout.bottomMargin: 4 }
        Repeater {
            model: list.entries
            delegate: Item {
                id: row
                required property var modelData
                required property int index
                Layout.fillWidth: true
                implicitHeight: 56
                // в истории фото исполнителя нет: ищем по имени тем же поиском, что и запасные обложки
                property string photo: ""
                property string _photoKey: ""
                Component.onCompleted: if (list.artists && !modelData.cover) _photoKey = Covers.find("artist", modelData.title ?? "", "")
                Connections {
                    target: row._photoKey !== "" ? Covers : null
                    function onFound(key, url) {
                        if (key === row._photoKey)
                            row.photo = url
                    }
                }
                StateLayer { anchors.fill: parent; radius: Theme.shape.medium; onClicked: list.picked(row.modelData) }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    spacing: 12
                    Label {
                        Layout.preferredWidth: 16
                        text: row.index + 1
                        type: "titleSmall"
                        color: Theme.fgSurfaceVariant
                        horizontalAlignment: Text.AlignRight
                    }
                    Item {
                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 40
                        Avatar { visible: list.artists; anchors.fill: parent; size: 40; name: row.modelData.title ?? "" }
                        MorphImage {
                            visible: !list.artists || ((row.modelData.cover || row.photo) !== "" && status === Image.Ready)
                            eager: true
                            anchors.fill: parent
                            source: list.artists ? (row.modelData.cover || row.photo) : (row.modelData.cover ?? "")
                            fallbackKind: list.artists ? "" : "track"
                            fallbackArtist: (row.modelData.subtitle ?? "").split(", ")[0]
                            fallbackTitle: row.modelData.title ?? ""
                            shape: list.artists ? "circle" : "softSquare"
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Label { Layout.fillWidth: true; text: row.modelData.title ?? ""; type: "bodyLarge" }
                        Label {
                            Layout.fillWidth: true
                            text: list.artists && row.modelData.seconds !== undefined ? root.duration(row.modelData.seconds)
                                                                                      : (row.modelData.subtitle ?? "")
                            type: "bodyMedium"
                            color: Theme.fgSurfaceVariant
                        }
                    }
                    Label {
                        visible: row.modelData.plays !== undefined
                        text: I18n.n(row.modelData.plays ?? 0, "{0} play", "{0} plays")
                        type: "labelLarge"
                        color: Theme.fgSurfaceVariant
                    }
                }
            }
        }
    }
}
