import QtQuick
import QtQuick.Layouts
import QtCore as Persist
import Md3
import Materialfy.Core
import ".."

// Поиск: поле с задержкой ввода, фильтры-чипы. «Всё» — лучший результат + по несколько из каждой
// категории, остальные фильтры — полный список одной категории.
Page {
    id: page
    title: I18n.t("Search")
    showHeader: false

    property string initialQuery
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
    // телефон: плитки от 140 с зазором 16 растягиваются на всю ширину (две колонки на любом экране)
    readonly property real tileGap: Theme.mobile ? 16 : 20
    readonly property int perRow: Theme.mobile ? Math.max(2, Math.floor((width - 2 * sideMargin + tileGap) / (140 + tileGap)))
                                               : Math.max(1, Math.floor((width - 2 * sideMargin + 20) / (168 + 20)))
    readonly property real tile: Theme.mobile ? Math.floor((width - 2 * sideMargin - (perRow - 1) * tileGap) / perRow) : 168

    function followersText(n) {
        if (!(n > 0))
            return ""
        return I18n.nCompact(n, "{0} follower", "{0} followers")
    }
    function focusSearch() { field.inputItem.forceActiveFocus() }
    // запрос уже запущен снаружи (Catalog.openUri("spotify:search:…")): только показать его
    function showQuery(q) {
        if (!q)
            return
        resetSearchFeedback()
        _syncingQuery = true
        filter = 0
        field.text = q
        debounce.stop()
        _syncingQuery = false
        updateSearchFeedback()
    }
    Component.onCompleted: {
        showQuery(initialQuery)
        // телефон: клавиатура — только по касанию поля, а не при каждом входе в раздел
        if (!Theme.mobile)
            Qt.callLater(focusSearch)
    }

    Timer {
        id: debounce
        interval: 350
        onTriggered: {
            page.submitSearch(false)
            page.updateSearchFeedback()
        }
    }

    readonly property string query: field.text.trim()
    readonly property bool currentPage: visible && (!router || router.currentItem === page)
    // Catalog также используется в подборе трека для радио и добавлении в плейлист.
    readonly property string searchState: Catalog.searchQuery === query && Catalog.searchKind === type
                                          ? Catalog.searchState : "idle"
    property bool _syncingQuery: false
    property bool _retried: false

    function resetSearchFeedback() {
        retryTimer.stop()
        rememberTimer.stop()
        _retried = false
    }
    function submitSearch(save) {
        debounce.stop()
        if (!signedIn)
            return
        if (save && query !== "")
            remember(query)
        Catalog.search(query, type)
    }
    function updateSearchFeedback() {
        if (!currentPage || !signedIn || query === "" || debounce.running)
            return
        if (searchState === "ok") {
            if (recent[0] !== query)
                rememberTimer.restart()
        } else if (searchState === "error" && !_retried && !retryTimer.running) {
            retryTimer.start()
        }
    }
    onTypeChanged: {
        if (!_syncingQuery && query !== "") {
            resetSearchFeedback()
            submitSearch(false)
            updateSearchFeedback()
        }
    }
    onCurrentPageChanged: {
        if (currentPage) {
            Qt.callLater(restoreSearch)
        } else {
            debounce.stop()
            retryTimer.stop()
            rememberTimer.stop()
        }
    }
    function restoreSearch() {
        if (!currentPage)
            return
        if (query !== "" && searchState === "idle" && !debounce.running)
            submitSearch(false)
        updateSearchFeedback()
    }
    onSearchStateChanged: updateSearchFeedback()
    Timer {
        id: retryTimer
        interval: 600
        onTriggered: {
            if (page.currentPage && page.signedIn && page.query !== "" && page.searchState === "error") {
                page._retried = true
                page.submitSearch(false)
            }
        }
    }

    // ---- Недавние запросы ----
    // Запрос попадает в историю, когда по нему есть результаты и его не меняли 2,5 с (или нажат Enter / выбран
    // из истории) — недопечатанные «pyr», «pyro» не сохраняются. Хранится локально (QSettings), до 8 штук
    Persist.Settings {
        id: searchHistory
        // свой файл: без него QSettings из QML требует organizationDomain, а приложение его не задаёт
        location: Persist.StandardPaths.writableLocation(Persist.StandardPaths.AppConfigLocation) + "/search-history.ini"
        category: "SearchHistory"
        property string recent: "[]"
    }
    readonly property var recent: {
        try {
            const list = JSON.parse(searchHistory.recent)
            return Array.isArray(list) ? list.filter(q => typeof q === "string" && q !== "") : []
        } catch (e) {
            return []
        }
    }
    function remember(q) {
        q = (q ?? "").trim()
        if (q.length < 2)
            return
        const low = q.toLowerCase()
        searchHistory.recent = JSON.stringify([q].concat(recent.filter(x => x.toLowerCase() !== low)).slice(0, 8))
    }
    function forget(q) { searchHistory.recent = JSON.stringify(recent.filter(x => x !== q)) }
    function searchFor(q) {
        field.text = q
        resetSearchFeedback()
        submitSearch(true)
        updateSearchFeedback()
    }
    Timer {
        id: rememberTimer
        interval: 2500
        onTriggered: {
            if (page.currentPage && page.signedIn && page.searchState === "ok")
                page.remember(page.query)
        }
    }
    readonly property bool showRecent: signedIn && query === "" && recent.length > 0

    SearchField {
        id: field
        Layout.fillWidth: true
        Layout.maximumWidth: 720
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 8
        implicitHeight: 56
        // телефон: коротко — длинная подсказка не помещается в поле
        placeholder: Theme.mobile ? I18n.t("Search") : I18n.t("Track, album, artist, playlist or podcast")
        onTextChanged: {
            if (!page._syncingQuery) {
                page.resetSearchFeedback()
                debounce.restart()
            }
        }
        // Enter — искать сразу, не дожидаясь паузы ввода; Esc — очистить поле
        onAccepted: {
            page.resetSearchFeedback()
            page.submitSearch(true)
        }
        Keys.onEscapePressed: event => {
            if (field.text !== "")
                field.text = ""
            else
                event.accepted = false
        }
    }

    ChipRow {
        Layout.fillWidth: true
        Layout.maximumWidth: 720
        Layout.alignment: Qt.AlignHCenter
        bleed: page.sideMargin
        model: page.filters
        current: page.filter
        onPicked: key => page.filter = key
    }

    // ---- Состояния ----
    // Недавние запросы — вместо приглашения, пока поле пустое
    ColumnLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: 720
        Layout.alignment: Qt.AlignHCenter
        visible: page.showRecent
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.bottomMargin: 4
            Label {
                Layout.fillWidth: true
                text: I18n.t("Recent searches")
                type: "titleMedium"
                weight: 600
            }
            Button {
                style: "text"
                text: I18n.t("Clear")
                onClicked: searchHistory.recent = "[]"
            }
        }
        Repeater {
            model: page.recent
            Item {
                id: recentRow
                required property string modelData
                Layout.fillWidth: true
                implicitHeight: 52
                StateLayer {
                    anchors.fill: parent
                    radius: Theme.shape.large
                    onClicked: page.searchFor(recentRow.modelData)
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    spacing: 16
                    Icon { name: "history"; size: 22; color: Theme.fgSurfaceVariant }
                    Label {
                        Layout.fillWidth: true
                        text: recentRow.modelData
                        type: "bodyLarge"
                    }
                    IconButton {
                        icon: "close"
                        Accessible.name: I18n.t("Remove") + " · " + recentRow.modelData
                        size: "s"
                        onClicked: page.forget(recentRow.modelData)
                    }
                }
            }
        }
    }

    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        readonly property bool failed: page.query !== "" && page.searchState === "error" && !retryTimer.running && !debounce.running
        readonly property bool empty: page.query !== "" && page.searchState === "empty" && !debounce.running
        visible: !page.signedIn || (page.query === "" && !page.showRecent) || empty || failed
        icon: !page.signedIn ? "login" : empty ? "search_off" : failed ? "cloud_off" : "travel_explore"
        title: !page.signedIn ? I18n.t("Sign in to search")
             : empty ? I18n.t("Nothing found")
             : failed ? I18n.t("Search didn't respond")
             : I18n.t("What do you want to hear?")
        text: !page.signedIn ? I18n.t("Search works through your Spotify account")
            : empty ? I18n.t("Try different wording or another filter")
            : failed ? I18n.t("Check your connection and try again")
            : I18n.t("Search for tracks, albums, artists, playlists and podcasts. You can also just paste an open.spotify.com link")
        actionText: !page.signedIn ? I18n.t("Sign in") : failed ? I18n.t("Retry") : ""
        actionIcon: !page.signedIn ? "login" : "refresh"
        onAction: {
            if (!page.signedIn)
                page.router.reset("account")
            else {
                page.resetSearchFeedback()
                page.submitSearch(false)
            }
        }
    }
    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        size: 64
        contained: true
        visible: page.signedIn && page.query !== "" && (debounce.running || page.searchState === "loading" || retryTimer.running)
    }

    // ---- Результаты ----
    ColumnLayout {
        id: results
        Layout.fillWidth: true
        spacing: 24
        visible: page.signedIn && page.query !== "" && !debounce.running && page.searchState === "ok"

        // Лучший результат
        Item {
            id: bestCard
            readonly property var best: Catalog.best
            readonly property var item: best.item ?? ({})
            visible: page.all && best.type !== undefined
            Layout.fillWidth: true
            Layout.preferredHeight: Theme.mobile ? 120 : 168

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
                anchors.margins: Theme.mobile ? 16 : 20
                spacing: Theme.mobile ? 16 : 24
                MorphImage {
                    Layout.preferredWidth: Theme.mobile ? 88 : 128
                    Layout.preferredHeight: Layout.preferredWidth
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
                        type: Theme.mobile ? "titleLarge" : "headlineMedium"
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
                    size: Theme.mobile ? 48 : 64
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
            // строки треков — вровень с полями страницы (их собственный отступ 8 на телефоне)
            Layout.leftMargin: Theme.mobile ? -8 : 0
            Layout.rightMargin: Theme.mobile ? -8 : 0
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
            spacing: page.tileGap
            Repeater {
                model: Catalog.searchArtists
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    size: Theme.mobile ? page.tile : 148
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
            spacing: page.tileGap
            Repeater {
                model: Catalog.searchAlbums
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    size: page.tile
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
            spacing: page.tileGap
            Repeater {
                model: Catalog.searchPlaylists
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    size: page.tile
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
            spacing: page.tileGap
            Repeater {
                model: page.showsModel
                MediaTile {
                    required property int index
                    required property var model
                    visible: !page.all || index < page.perRow
                    size: page.tile
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
