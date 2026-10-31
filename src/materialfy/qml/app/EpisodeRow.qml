import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Строка выпуска: обложка · «10 окт · 58 мин» · название · описание · кнопка play, статус
// (новый / начат / играет / прослушан) и действия (в очередь, сохранить, меню). Клик — страница выпуска.
Item {
    id: root

    property var episode: ({})          // EPISODE_KEYS
    property bool compact: false
    property bool showCover: true

    readonly property string uri: episode.uri ?? ""
    readonly property bool isCurrent: uri !== "" && Player.uri === uri
    readonly property bool playing: isCurrent && Player.playing
    readonly property int durationMs: episode.durationMs ?? 0
    readonly property int resumeMs: episode.resumeMs ?? 0
    readonly property bool fullyPlayed: episode.fullyPlayed === true && !isCurrent
    readonly property bool started: isCurrent || (resumeMs > 0 && !fullyPlayed)
    readonly property real progress: isCurrent ? Player.position
                                   : durationMs > 0 ? Math.min(1, resumeMs / durationMs) : 0
    readonly property bool saved: Library.savedRevision >= 0 && uri !== "" ? Library.isSaved(uri) : episode.saved === true
    readonly property bool hovered: hover.hovered

    implicitHeight: content.implicitHeight + 24
    implicitWidth: 480

    // словарь для меню трека и очереди
    function trackData() {
        return {
            trackId: episode.episodeId ?? "", uri: uri, title: episode.title ?? "",
            artists: episode.showName ?? "", artistRefs: [], album: episode.showName ?? "", albumId: "",
            cover: episode.cover ?? "", coverLarge: episode.coverLarge ?? "", durationMs: durationMs,
            explicit: episode.explicit === true, available: true, isEpisode: true, showId: episode.showId ?? ""
        }
    }
    function play() {
        if (isCurrent)
            Player.togglePlay()
        else
            Player.playEpisode(uri, fullyPlayed ? 0 : resumeMs)
    }

    function _date(iso) {
        const m = /^(\d{4})(?:-(\d{2}))?(?:-(\d{2}))?/.exec(iso ?? "")
        if (!m)
            return ""
        if (!m[2])
            return m[1]
        if (!m[3])
            return I18n.monthYear(new Date(Number(m[1]), Number(m[2]) - 1, 1))
        return I18n.shortDate(new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])))
    }
    function _minutes(ms) {
        return I18n.duration(Math.max(60000, ms))      // не меньше «1 мин»
    }

    Rectangle {
        anchors.fill: parent
        radius: Theme.shape.extraLarge
        color: root.isCurrent ? Qt.alpha(Theme.primary, 0.12) : "transparent"
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }
    StateLayer {
        anchors.fill: parent
        radius: Theme.shape.extraLarge
        onClicked: if ((root.episode.episodeId ?? "") !== "") Catalog.openEpisode(root.episode.episodeId)
    }
    HoverHandler { id: hover }
    // выпуск можно перетащить в очередь, как строку трека
    QueueDragSource {
        anchors.fill: parent
        z: -2
        source: root
        enabled: root.uri !== ""
        title: root.episode.title ?? ""
        cover: root.episode.cover ?? ""
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: point => {
            const p = root.mapToItem(null, point.position.x, point.position.y)
            Catalog.requestTrackMenu(root.trackData(), p.x, p.y)
        }
    }

    RowLayout {
        id: content
        x: 12
        y: 12
        width: root.width - 24
        spacing: root.compact ? 12 : 16

        MorphImage {
            visible: root.showCover
            readonly property real side: root.compact ? 64 : 88
            Layout.preferredWidth: side
            Layout.preferredHeight: side
            Layout.alignment: Qt.AlignTop
            source: root.episode.cover ?? ""
            shape: root.isCurrent ? "cookie9" : "softSquare"
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Label {
                    text: [root._date(root.episode.releaseDate), root.durationMs > 0 ? root._minutes(root.durationMs) : ""].filter(x => x).join(" · ")
                    type: "labelMedium"
                    color: Theme.fgSurfaceVariant
                }
                Rectangle {
                    visible: root.episode.explicit === true
                    implicitWidth: 16
                    implicitHeight: 16
                    radius: 4
                    color: Theme.surfaceContainerHighest
                    Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
                }
                Item { Layout.fillWidth: true }
            }
            Label {
                Layout.fillWidth: true
                text: root.episode.title ?? ""
                type: "titleMedium"
                weight: 600
                color: root.isCurrent ? Theme.primary : Theme.fgSurface
                Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
            }
            Label {
                visible: text !== ""
                Layout.fillWidth: true
                text: root.episode.description ?? ""
                textFormat: Text.PlainText
                type: "bodySmall"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                maximumLineCount: root.compact ? 1 : 2
            }

            // подвал: play, статус, действия
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 6
                spacing: 12

                Item {
                    implicitWidth: 40
                    implicitHeight: 40
                    PlayButton {
                        anchors.fill: parent
                        visible: root.isCurrent
                        size: 40
                        playing: Player.playing
                        loading: Player.buffering
                        pausedShape: "circle"
                        spin: false
                        onClicked: root.play()
                    }
                    Rectangle {
                        anchors.fill: parent
                        visible: !root.isCurrent
                        radius: width / 2
                        color: Theme.secondaryContainer
                        Icon {
                            anchors.centerIn: parent
                            name: "play_arrow"
                            size: 24
                            fill: 1
                            color: Theme.fgSecondaryContainer
                        }
                        StateLayer {
                            anchors.fill: parent
                            radius: width / 2
                            color: Theme.fgSecondaryContainer
                            onClicked: root.play()
                        }
                    }
                }

                // статус
                Row {
                    spacing: 8
                    Layout.alignment: Qt.AlignVCenter
                    Rectangle {
                        visible: !root.started && !root.fullyPlayed
                        anchors.verticalCenter: parent.verticalCenter
                        width: 8
                        height: 8
                        radius: 4
                        color: Theme.primary
                    }
                    Icon {
                        visible: root.fullyPlayed
                        anchors.verticalCenter: parent.verticalCenter
                        name: "check_circle"
                        size: 18
                        fill: 1
                        color: Theme.primary
                    }
                    WavyProgress {
                        visible: root.started
                        anchors.verticalCenter: parent.verticalCenter
                        width: root.compact ? 72 : 120
                        height: 12
                        value: root.progress
                        wavy: root.playing && !Theme.quiet && !Theme.reducedMotion
                        amplitude: 2
                        interactive: false
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: root.fullyPlayed ? I18n.t("Played")
                            : root.started ? I18n.t("{0} left", root._minutes(Math.max(0, root.durationMs - root.progress * root.durationMs)))
                            : I18n.t("Unplayed")
                        type: "labelMedium"
                        color: root.isCurrent || !root.started ? Theme.primary : Theme.fgSurfaceVariant
                    }
                }

                Item { Layout.fillWidth: true }

                Row {
                    spacing: 0
                    opacity: root.hovered || root.isCurrent || root.saved ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                    IconButton {
                        icon: "add_to_queue"
                        size: "xs"
                        visible: root.hovered || root.isCurrent
                        onClicked: Player.addUriToQueue(root.uri)
                    }
                    IconButton {
                        icon: root.saved ? "bookmark_added" : "bookmark"
                        size: "xs"
                        iconColor: root.saved ? Theme.primary : Theme.fgSurfaceVariant
                        onClicked: Library.toggleSaved(root.uri)
                    }
                    // «⋯» создаётся с первого наведения и остаётся (строк много, а наведена одна);
                    // видимость — на самой кнопке, как и раньше
                    Loader {
                        // невидимый item всё равно занимает ширину в Row — прячем сам Loader
                        active: root.hovered || root.isCurrent || item !== null
                        visible: item !== null && item.visible
                        sourceComponent: IconButton {
                            id: moreButton
                            icon: "more_horiz"
                            size: "xs"
                            visible: root.hovered || root.isCurrent
                            onClicked: {
                                const p = moreButton.mapToItem(null, 0, 0)
                                Catalog.requestTrackMenuAt(root.trackData(), p.x, p.y, moreButton.width, moreButton.height)
                            }
                        }
                    }
                }
            }
        }
    }
}
