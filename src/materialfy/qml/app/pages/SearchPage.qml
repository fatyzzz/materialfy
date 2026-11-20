import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Поиск: поле с задержкой ввода, фильтры-чипы. «Всё» — лучший результат + по несколько из каждой
// категории, остальные фильтры — полный список одной категории.
Page {
    id: page
    title: I18n.t("Search")
    showHeader: false

    property string initialQuery: Catalog.searchQuery
    property int filter: 0
    readonly property var filters: [
        { text: I18n.t("All"), type: "all" }, { text: I18n.t("Tracks"), type: "tracks" }, { text: I18n.t("Albums"), type: "albums" },
        { text: I18n.t("Artists"), type: "artists" }, { text: I18n.t("Playlists"), type: "playlists" },
        { text: I18n.t("Podcasts"), type: "shows" }, { text: I18n.t("Episodes"), type: "episodes" }
    ]
    function showFilter(type) { filter = Math.max(0, filters.findIndex(f => f.type === type)) }
    readonly property string type: filters[filter].type
    readonly property bool all: type === "all"
    readonly property bool signedIn: Auth.state === "signedIn"
    // в старом бинарнике новых моделей поиска может ещё не быть
    readonly property var showsModel: Catalog.searchShows ?? null
    readonly property var episodesModel: Catalog.searchEpisodes ?? null
    readonly property int perRow: Math.max(1, Math.floor((width - 2 * sideMargin + 20) / (168 + 20)))

    function followersText(n) {
        if (!(n > 0))
            return ""
        return I18n.nCompact(n, "{0} follower", "{0} followers")
    }
    function focusSearch() { field.inputItem.forceActiveFocus() }
    // Catalog has already started the search before opening the page.
    property bool _showingQuery: false
    function showQuery(q) {
        if (q === "")
            return
        _showingQuery = true
        filter = 0
        field.text = q
        debounce.stop()
        _showingQuery = false
    }
    Component.onCompleted: {
        showQuery(initialQuery)
        Qt.callLater(focusSearch)
    }

    Timer {
        id: debounce
        interval: 350
        onTriggered: Catalog.search(field.text, page.type)
    }
    onTypeChanged: if (!_showingQuery && field.text.trim() !== "") Catalog.search(field.text, page.type)

    SearchField {
        id: field
        Layout.fillWidth: true
        Layout.maximumWidth: 720
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 8
        implicitHeight: 56
        placeholder: I18n.t("Track, album, artist, playlist or podcast")
        onTextChanged: if (!page._showingQuery) debounce.restart()
        // Enter — искать сразу, не дожидаясь паузы ввода; Esc — очистить поле
        onAccepted: {
            debounce.stop()
            if (field.text.trim() !== "")
                Catalog.search(field.text, page.type)
        }
        Keys.onEscapePressed: event => {
            if (field.text !== "")
                field.text = ""
            else
                event.accepted = false
        }
    }

    Flow {
        Layout.fillWidth: true
        Layout.maximumWidth: 720
        Layout.alignment: Qt.AlignHCenter
        spacing: 8
        Repeater {
            model: page.filters
            Chip {
                required property var modelData
                required property int index
                text: modelData.text
                selected: page.filter === index
                onClicked: page.filter = index
            }
        }
    }

    // ---- Состояния ----
    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        visible: !page.signedIn || Catalog.searchState === "idle" || Catalog.searchState === "empty" || Catalog.searchState === "error"
        icon: !page.signedIn ? "login" : Catalog.searchState === "empty" ? "search_off"
            : Catalog.searchState === "error" ? "cloud_off" : "travel_explore"
        title: !page.signedIn ? I18n.t("Sign in to search")
             : Catalog.searchState === "empty" ? I18n.t("Nothing found")
             : Catalog.searchState === "error" ? I18n.t("Search didn't respond")
             : I18n.t("What do you want to hear?")
        text: !page.signedIn ? I18n.t("Search works through your Spotify account")
            : Catalog.searchState === "empty" ? I18n.t("Try different wording or another filter")
            : Catalog.searchState === "error" ? I18n.t("Check your connection and try again")
            : I18n.t("Search for tracks, albums, artists, playlists and podcasts. You can also just paste an open.spotify.com link")
        actionText: !page.signedIn ? I18n.t("Sign in") : Catalog.searchState === "error" ? I18n.t("Retry") : ""
        actionIcon: !page.signedIn ? "login" : "refresh"
        onAction: {
            if (!page.signedIn)
                page.router.reset("account")
            else
                Catalog.search(field.text, page.type)
        }
    }
    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        size: 64
        contained: true
        visible: page.signedIn && Catalog.searchState === "loading"
    }

    // ---- Результаты ----
    ColumnLayout {
        id: results
        Layout.fillWidth: true
        spacing: 24
        visible: page.signedIn && Catalog.searchState === "ok"

        // Лучший результат
        Item {
            id: bestCard
            readonly property var best: Catalog.best
            readonly property var item: best.item ?? ({})
            visible: page.all && best.type !== undefined
            Layout.fillWidth: true
            Layout.preferredHeight: 168

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.extraLarge
                color: Theme.surfaceContainerHigh
            }
            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.extraLarge
                onClicked: page.openBest()
            }
            RowLayout {
                anchors.fill: parent
                anchors.margins: 20
                spacing: 24
                MorphImage {
                    Layout.preferredWidth: 128
                    Layout.preferredHeight: 128
                    source: bestCard.item.coverLarge || bestCard.item.cover || ""
                    shape: bestCard.best.type === "artist" ? "circle" : "softSquare"
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 4
                    Label {
                        text: (({ track: I18n.t("Track"), album: bestCard.item.kind ? I18n.any(bestCard.item.kind) : I18n.t("Album"), artist: I18n.t("Artist"), playlist: I18n.t("Playlist"), show: I18n.t("Podcast") })[bestCard.best.type] ?? "").toUpperCase()
                        type: "labelLarge"
                        color: Theme.primary
                        font.letterSpacing: 1.2
                    }
                    Label {
                        Layout.fillWidth: true
                        text: bestCard.item.title ?? bestCard.item.name ?? ""
                        type: "headlineMedium"
                        weight: 700
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: bestCard.item.artists || bestCard.item.owner || bestCard.item.publisher
                              || page.followersText(bestCard.item.followers ?? 0)
                              || bestCard.item.genres || ""
                        type: "bodyLarge"
                        color: Theme.fgSurfaceVariant
                    }
                }
                PlayButton {
                    visible: bestCard.best.type === "track"
                    size: 64
                    playing: Player.playing && Player.trackId === (bestCard.item.trackId ?? "-")
                    onClicked: page.openBest()
                }
            }
        }

        // Треки
        SectionHeader {
            visible: Catalog.searchTracks.count > 0 && (page.all || page.type === "tracks")
            text: I18n.t("Tracks")
            actionText: page.all && Catalog.totals.tracks > 5 ? I18n.t("Show all") : ""
            onAction: page.showFilter("tracks")
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.topMargin: -12
            visible: page.all || page.type === "tracks"
            spacing: 0
            Repeater {
                model: Catalog.searchTracks
                TrackRow {
                    required property int index
                    required property var model
                    visible: !page.all || index < 5
                    Layout.fillWidth: true
                    trackId: model.trackId
                    title: model.title
                    version: model.version
                    artists: model.artists
                    artistRefs: model.artistRefs
                    albumId: model.albumId
                    album: model.album
                    cover: model.cover
                    durationMs: model.durationMs
                    explicit: model.explicit
                    available: model.available
                    liked: Library.likesRevision >= 0 && Library.isLiked(model.trackId)
                    current: model.trackId === Player.trackId
                    onActivated: Player.playFrom(Catalog.searchTracks, index)
                    onLikeClicked: Library.toggleLike(Catalog.searchTracks.get(index))
                    track: Catalog.searchTracks.get(index)
                }
            }
        }

        // Исполнители
        SectionHeader {
            visible: Catalog.searchArtists.count > 0 && (page.all || page.type === "artists")
            text: I18n.t("Artists")
            actionText: page.all && Catalog.totals.artists > page.perRow ? I18n.t("Show all") : ""
            onAction: page.showFilter("artists")
        }
        Flow {
            Layout.fillWidth: true
            visible: page.all || page.type === "artists"
            spacing: 20
            Repeater {
                model: Catalog.searchArtists
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    size: 148
                    round: true
                    placeholderIcon: "person"
                    cover: model.cover
                    title: model.name
                    onClicked: Catalog.openArtist(model.artistId)
                }
            }
        }

        // Альбомы
        SectionHeader {
            visible: Catalog.searchAlbums.count > 0 && (page.all || page.type === "albums")
            text: I18n.t("Albums")
            actionText: page.all && Catalog.totals.albums > page.perRow ? I18n.t("Show all") : ""
            onAction: page.showFilter("albums")
        }
        Flow {
            Layout.fillWidth: true
            visible: page.all || page.type === "albums"
            spacing: 20
            Repeater {
                model: Catalog.searchAlbums
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    cover: model.cover
                    title: model.title
                    subtitle: [model.artists, model.year || ""].filter(x => x).join(" · ")
                    explicit: model.explicit
                    onClicked: Catalog.openAlbum(model.albumId)
                }
            }
        }

        // Плейлисты
        SectionHeader {
            visible: Catalog.searchPlaylists.count > 0 && (page.all || page.type === "playlists")
            text: I18n.t("Playlists")
            actionText: page.all && Catalog.totals.playlists > page.perRow ? I18n.t("Show all") : ""
            onAction: page.showFilter("playlists")
        }
        Flow {
            Layout.fillWidth: true
            visible: page.all || page.type === "playlists"
            spacing: 20
            Repeater {
                model: Catalog.searchPlaylists
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    placeholderIcon: "queue_music"
                    cover: model.cover
                    title: model.title
                    subtitle: model.owner
                    onClicked: {
                        const p = Catalog.searchPlaylists.get(index)
                        Catalog.openPlaylist({ playlistId: p.playlistId, title: p.title, cover: p.cover })
                    }
                }
            }
        }

        // Подкасты
        SectionHeader {
            visible: page.showsModel !== null && page.showsModel.count > 0 && (page.all || page.type === "shows")
            text: I18n.t("Podcasts")
            actionText: page.all && (Catalog.totals.shows ?? 0) > page.perRow ? I18n.t("Show all") : ""
            onAction: page.showFilter("shows")
        }
        Flow {
            Layout.fillWidth: true
            visible: page.all || page.type === "shows"
            spacing: 20
            Repeater {
                model: page.showsModel
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    placeholderIcon: "podcasts"
                    cover: model.cover
                    title: model.name
                    subtitle: model.publisher
                    explicit: model.explicit
                    onClicked: Catalog.openShow(model.showId)
                }
            }
        }

        // Выпуски
        SectionHeader {
            visible: page.episodesModel !== null && page.episodesModel.count > 0 && (page.all || page.type === "episodes")
            text: I18n.t("Episodes")
            actionText: page.all && (Catalog.totals.episodes ?? 0) > 3 ? I18n.t("Show all") : ""
            onAction: page.showFilter("episodes")
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.topMargin: -12
            visible: page.all || page.type === "episodes"
            spacing: 0
            Repeater {
                model: page.episodesModel
                EpisodeRow {
                    required property int index
                    visible: !page.all || index < 3
                    Layout.fillWidth: true
                    compact: true
                    episode: page.episodesModel.get(index)
                }
            }
        }
    }

    function openBest() {
        const b = Catalog.best
        if (!b.type)
            return
        if (b.type === "artist") Catalog.openArtist(b.item.artistId)
        else if (b.type === "album") Catalog.openAlbum(b.item.albumId)
        else if (b.type === "playlist") Catalog.openPlaylist({ playlistId: b.item.playlistId, title: b.item.title, cover: b.item.cover })
        else if (b.type === "show") Catalog.openShow(b.item.showId)
        else if (b.type === "track") {
            const row = Catalog.searchTracks.find("trackId", b.item.trackId)
            if (row >= 0) Player.playFrom(Catalog.searchTracks, row)
        }
    }
}
