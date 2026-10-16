import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Новинки: личный «Радар новинок» Spotify (шапка + треки, играет как плейлист) и сетка свежих альбомов
// и синглов исполнителей, на которых подписан пользователь (плитка открывает страницу альбома).
Page {
    id: page

    title: I18n.t("New releases")
    subtitle: I18n.t("Fresh music from artists you listen to")

    readonly property var radar: Releases.radar ?? ({})
    readonly property string radarId: radar.playlistId ?? ""
    readonly property string radarUri: (radar.uri ?? "") !== "" ? radar.uri : (radarId !== "" ? "spotify:playlist:" + radarId : "")
    // playlistTracks — ref-counted lifecycle API: this page is a second possible owner of Radar.
    // Keep the model stable while visible and release it when the retained page is hidden/destroyed.
    property var radarTracks: null
    property string _radarOwnerId: ""
    // Скрытие снимает владение (отмена чтения), модель остаётся: возврат на Releases
    // не пересоздаёт строки. Обнуление — при смене id и уничтожении.
    function _syncRadarModel() {
        if (radarId === _radarOwnerId)
            return
        _releaseRadarModel()
        if (radarId !== "" && visible) {
            _radarOwnerId = radarId
            radarTracks = Library.playlistTracks(radarId)
        }
    }
    function _releaseRadarModel() {
        if (_radarOwnerId !== "")
            Library.releasePlaylist(_radarOwnerId)
        _radarOwnerId = ""
        radarTracks = null
    }
    function _releaseRadarOwnership() {
        if (_radarOwnerId !== "")
            Library.releasePlaylist(_radarOwnerId)
        _radarOwnerId = ""
    }
    readonly property bool radarPlaying: radarUri !== "" && Player.contextUri === radarUri
    property bool allTracks: false
    readonly property int shownTracks: 8

    property string filter: "all"          // all | album | single
    readonly property var releases: Releases.releases ?? []
    readonly property var shown: filter === "all" ? releases : releases.filter(r => r.group === filter)
    property bool allReleases: false

    Component.onCompleted: { _syncRadarModel(); Releases.refresh(false) }
    onRadarIdChanged: _syncRadarModel()
    onVisibleChanged: {
        if (visible) {
            _syncRadarModel()
            Releases.refresh(false)
        } else {
            _releaseRadarOwnership()
        }
    }
    Component.onDestruction: _releaseRadarModel()

    function openRadar() {
        router.push("playlist", { playlistId: radarId, playlistTitle: radar.title ?? "", cover: radar.cover ?? "" })
    }
    function playRadar() {
        if (radarPlaying && Player.hasTrack)
            Player.togglePlay()
        else if (radarTracks && radarTracks.count > 0)
            Player.playFrom(radarTracks, 0)
        else
            Player.playContext(radarUri, "")
    }

    QtObject {
        id: tilesPerRow
        readonly property int value: Math.max(1, Math.floor((page.width - 2 * page.sideMargin + 20) / (168 + 20)))
    }

    // ---- «Радар новинок» ----
    SectionHeader {
        text: I18n.t("Release Radar")
        actionText: page.radarId !== "" ? I18n.t("Open playlist") : ""
        onAction: page.openRadar()
    }

    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 16
        size: 48
        contained: true
        visible: page.radarId === "" && !Releases.radarChecked && Auth.state === "signedIn"
    }

    // не нашёлся: Spotify не отдал категорию «Сделано для вас», а в библиотеке его нет
    Item {
        Layout.fillWidth: true
        Layout.preferredHeight: missingRow.implicitHeight + 32
        visible: page.radarId === "" && Releases.radarChecked
        Rectangle {
            anchors.fill: parent
            radius: 24
            color: Theme.surfaceContainerLow
        }
        RowLayout {
            id: missingRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.margins: 16
            spacing: 16
            MorphShape {
                Layout.preferredWidth: 56
                Layout.preferredHeight: 56
                shape: "cookie9"
                color: Theme.secondaryContainer
                Icon { anchors.centerIn: parent; name: "new_releases"; size: 28; color: Theme.fgSecondaryContainer }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label { Layout.fillWidth: true; text: I18n.t("Release Radar wasn't found"); type: "titleMedium"; weight: 600 }
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Spotify builds it on Fridays for people who listen to enough music. If it's in your Spotify, save it to your library and it will show up here.")
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
            }
        }
    }

    // шапка «Радара»: обложка, название, описание, Слушать / Перемешать
    RowLayout {
        Layout.fillWidth: true
        visible: page.radarId !== ""
        spacing: page.width < 560 ? 16 : 24

        MorphImage {
            Layout.preferredWidth: page.width < 560 ? 104 : 144
            Layout.preferredHeight: Layout.preferredWidth
            source: page.radar.cover ?? ""
            shape: page.radarPlaying && Player.playing ? "cookie12" : "cookie9"
            duration: Theme.motion.spatialSlow
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: page.openRadar() }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            Label { text: I18n.t("Playlist").toUpperCase() + " · SPOTIFY"; type: "labelLarge"; color: Theme.primary; font.letterSpacing: 1.2 }
            Label {
                Layout.fillWidth: true
                text: page.radar.title ?? ""
                type: page.width < 560 ? "headlineSmall" : "headlineMedium"
                weight: 700
            }
            Label {
                Layout.fillWidth: true
                visible: text !== ""
                text: page.radar.description ?? ""
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                maximumLineCount: 2
            }
            Flow {
                Layout.fillWidth: true
                Layout.topMargin: 10
                spacing: 12
                Button {
                    text: page.radarPlaying && Player.playing ? I18n.t("Pause") : I18n.t("Play")
                    icon: page.radarPlaying && Player.playing ? "pause" : "play_arrow"
                    size: "m"
                    onClicked: page.playRadar()
                }
                Button {
                    text: I18n.t("Shuffle")
                    icon: "shuffle"
                    style: "tonal"
                    size: "m"
                    onClicked: page.radarTracks && page.radarTracks.count > 0 ? Player.shuffleFrom(page.radarTracks) : Player.shuffleContext(page.radarUri)
                }
            }
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: -8
        visible: page.radarId !== ""
        spacing: 0
        Repeater {
            model: page.radarTracks
            TrackRow {
                required property int index
                required property var model
                visible: page.allTracks || index < page.shownTracks
                Layout.fillWidth: true
                number: index + 1
                trackId: model.trackId
                title: model.title
                version: model.version
                artists: model.artists
                artistRefs: model.artistRefs
                albumId: model.albumId
                album: model.album
                cover: model.cover
                uri: model.uri ?? ""
                durationMs: model.durationMs
                explicit: model.explicit
                available: model.available
                liked: Library.likesRevision >= 0 && Library.isLiked(model.trackId)
                current: model.trackId === Player.trackId
                track: page.radarTracks.get(index)
                // с этого трека — плейлистом: дальше играет весь «Радар»
                onActivated: Player.playFrom(page.radarTracks, index)
                onLikeClicked: Library.toggleLike(page.radarTracks.get(index))
            }
        }
        LoadingIndicator {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 8
            size: 40
            visible: page.radarTracks !== null && page.radarTracks.loading && page.radarTracks.count === 0
        }
        Button {
            Layout.alignment: Qt.AlignLeft
            Layout.topMargin: 4
            visible: page.radarTracks !== null && page.radarTracks.count > page.shownTracks
            text: page.radarTracks === null ? "" : page.allTracks ? I18n.t("Show less") : I18n.n(page.radarTracks.count, "All {0} track", "All {0} tracks")
            style: "text"
            onClicked: page.allTracks = !page.allTracks
        }
    }

    // ---- Новые релизы ----
    SectionHeader {
        text: I18n.t("New releases from your artists")
        actionText: Releases.loading ? "" : I18n.t("Refresh")
        onAction: Releases.refresh(true)
    }
    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: -12
        spacing: 8
        visible: page.releases.length > 0
        Chip { text: I18n.t("All"); selected: page.filter === "all"; onClicked: page.filter = "all" }
        Chip { text: I18n.t("Albums"); selected: page.filter === "album"; onClicked: page.filter = "album" }
        Chip { text: I18n.t("Singles and EPs"); selected: page.filter === "single"; onClicked: page.filter = "single" }
        Item { Layout.fillWidth: true }
        Label {
            text: I18n.t("past 6 weeks")
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
    }

    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 24
        size: 64
        contained: true
        visible: Releases.loading && page.releases.length === 0
    }
    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 24
        visible: !Releases.loading && page.releases.length === 0
        icon: "album"
        title: Releases.errorText !== "" ? I18n.t("New releases didn't load") : I18n.t("Nothing new yet")
        text: Releases.errorText !== "" ? Releases.errorText
              : I18n.t("Your artists haven't released anything in the past 6 weeks. Follow artists and their new releases will show up here.")
        actionText: Releases.errorText !== "" ? I18n.t("Retry") : ""
        actionIcon: "refresh"
        onAction: Releases.refresh(true)
    }

    // фильтр ничего не оставил — сказать об этом, а не показывать пустое место
    Label {
        Layout.fillWidth: true
        visible: page.releases.length > 0 && page.shown.length === 0
        text: page.filter === "album" ? I18n.t("No new albums in the past 6 weeks")
                                      : I18n.t("No new singles or EPs in the past 6 weeks")
        type: "bodyLarge"
        color: Theme.fgSurfaceVariant
        wrapMode: Text.WordWrap
    }

    Flow {
        Layout.fillWidth: true
        spacing: 20
        Repeater {
            model: page.shown
            MediaTile {
                required property int index
                required property var modelData
                visible: page.allReleases || index < tilesPerRow.value * 3
                cover: modelData.cover ?? ""
                title: modelData.title ?? ""
                subtitle: [modelData.artists ?? "", modelData.dateLabel ?? ""].filter(x => x).join(" · ")
                badge: modelData.group === "single" ? I18n.any(modelData.kind).toUpperCase() : ""
                explicit: modelData.explicit === true
                showPlay: true
                playing: Player.playing && (modelData.uri ?? "") !== "" && Player.contextUri === modelData.uri
                onClicked: Catalog.openAlbum(modelData.albumId)
                onPlayClicked: {
                    if (Player.contextUri === modelData.uri && Player.hasTrack)
                        Player.togglePlay()
                    else
                        Player.playContext(modelData.uri, "")
                }
            }
        }
    }
    Button {
        Layout.alignment: Qt.AlignHCenter
        visible: page.shown.length > tilesPerRow.value * 3
        text: page.allReleases ? I18n.t("Show less") : I18n.t("Show all ({0})", page.shown.length)
        style: "tonal"
        onClicked: page.allReleases = !page.allReleases
    }
}
