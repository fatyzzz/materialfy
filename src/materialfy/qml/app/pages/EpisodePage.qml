import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Выпуск подкаста: шапка, карточка прослушивания, описание с главами, ещё выпуски шоу.
Page {
    id: page

    property string episodeId
    readonly property QtObject ep: Catalog.episode(episodeId)
    readonly property var info: ep ? ep.info : ({})
    readonly property var showInfo: ep ? ep.show : ({})
    readonly property bool compact: width < 840     // окно 900 px (рейл) или 1280 с правой панелью
    readonly property bool isCurrent: (info.uri ?? "") !== "" && Player.uri === info.uri

    title: info.title ?? ""
    showHeader: false
    readonly property bool heroTitle: true     // название — в шапке; в верхней панели — после прокрутки

    // год / месяц и год / полная дата — по точности даты ("October 9, 2024", «9 октября 2024»)
    function _date(iso) {
        return I18n.releaseDate(iso ?? "")
    }
    function _minutes(ms) {
        return I18n.duration(Math.max(60000, ms))
    }

    underlay: HeroShapes {
        width: page.width
        active: page.visible && !Theme.mobile
        headerHeight: hero.height + 24
        scroll: page.flickable.contentY
        contentX: page.sideMargin - 16
        contentWidth: page.width - 2 * contentX
        seed: page.info.title ?? ""
        shapesOpacity: 0.5
    }

    // ---- Шапка ----
    Item {
        id: hero
        Layout.fillWidth: true
        Layout.preferredHeight: heroRow.implicitHeight + 32

        // телефон: обложка над подписями, по центру
        GridLayout {
            id: heroRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            columns: Theme.mobile ? 1 : 2
            rowSpacing: 16
            columnSpacing: page.compact ? 20 : 28

            MorphImage {
                Layout.preferredWidth: Theme.mobile ? 168 : page.compact ? 128 : 168
                Layout.preferredHeight: Layout.preferredWidth
                Layout.alignment: Theme.mobile ? Qt.AlignHCenter : Qt.AlignTop
                source: page.info.coverLarge || page.info.cover || ""
                shape: page.isCurrent ? "cookie12" : "softSquare"
                duration: Theme.motion.spatialSlow
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Label {
                    text: page.info.number ? I18n.t("EPISODE {0}", page.info.number) : I18n.t("EPISODE")
                    type: "labelMedium"
                    color: Theme.primary
                    font.letterSpacing: 1.2
                }
                Label {
                    Layout.fillWidth: true
                    text: page.info.title ?? ""
                    type: page.compact ? "headlineMedium" : "headlineLarge"
                    weight: 600
                    wrapMode: Text.WordWrap
                    maximumLineCount: 3
                }
                LinkLabel {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: page.info.showName || page.showInfo.name || ""
                    type: "titleMedium"
                    color: Theme.primary
                    linkEnabled: (page.info.showId ?? "") !== ""
                    onClicked: Catalog.openShow(page.info.showId)
                }
                Label {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: [page._date(page.info.releaseDate), (page.info.durationMs ?? 0) > 0 ? page._minutes(page.info.durationMs) : "",
                           page.info.explicit ? "18+" : ""].filter(x => x).join(" · ")
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
            }
        }
    }

    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        size: 64
        contained: true
        visible: page.ep !== null && page.ep.loading && (page.info.uri ?? "") === ""
    }
    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        visible: page.ep !== null && !page.ep.loading && page.ep.errorText !== ""
        icon: "cloud_off"
        title: I18n.t("Couldn't load episode")
        text: page.ep ? page.ep.errorText : ""
    }

    EpisodeProgressCard {
        Layout.fillWidth: true
        Layout.maximumWidth: 880
        visible: (page.info.uri ?? "") !== ""
        episode: page.info
        compact: page.compact
        onShared: snackbar.show(I18n.t("Link copied"))
    }

    // ---- Описание и главы ----
    ColumnLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: 880
        visible: (page.info.description ?? "") !== ""
        spacing: 12
        SectionHeader { text: I18n.t("Description") }
        Label {
            Layout.fillWidth: true
            visible: text !== ""
            // строки-главы показывает ChapterList
            text: chapters.chapters.length > 0
                  ? (page.info.description ?? "").split("\n").filter(l => !/^\s*[(\[]?(\d{1,2}:)?\d{1,2}:\d{2}/.test(l)).join("\n").trim()
                  : (page.info.description ?? "")
            textFormat: Text.PlainText
            type: "bodyLarge"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
        ChapterList {
            id: chapters
            Layout.fillWidth: true
            Layout.topMargin: 8
            text: page.info.description ?? ""
            uri: page.info.uri ?? ""
            durationMs: page.info.durationMs ?? 0
        }
    }

    // ---- Ещё выпуски ----
    SectionHeader {
        visible: page.ep !== null && page.ep.more.count > 0
        text: I18n.t("More episodes")
        actionText: (page.info.showId ?? "") !== "" ? I18n.t("All episodes") : ""
        onAction: Catalog.openShow(page.info.showId)
    }
    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: -12
        spacing: 0
        Repeater {
            model: page.ep ? page.ep.more : null
            EpisodeRow {
                required property int index
                Layout.fillWidth: true
                visible: index < 3
                episode: page.ep.more.get(index)
                compact: page.compact
            }
        }
    }

    Snackbar {
        id: snackbar
        parent: page
        anchors.fill: parent
        z: 50
    }
}
