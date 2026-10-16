import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Главная: сверху сцена «Радио» (сиды станций на орбите), ниже — полки Home.shelves: «Быстрый доступ»,
// «Недавно прослушанное», «Сделано для вас», «Продолжить подкасты», «Топ-исполнители», «Топ-треки».
Page {
    id: page

    title: I18n.t("Home")
    showHeader: false

    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property bool compact: width < 760
    // Полки: Repeater по составу полок [{key, kind}] — он меняется, только если полки добавились, ушли
    // или переставились. Данные полки кладутся в её Loader на месте и только если изменились: обновление
    // «Недавно прослушанного» не пересоздаёт «Сделано для вас» и остальные плитки.
    property var shelfIds: []
    function _syncShelves() {
        const list = signedIn ? (Home.shelves ?? []) : []
        const ids = list.map(s => ({ key: s.key ?? "", kind: s.kind ?? "" }))
        if (JSON.stringify(ids) !== JSON.stringify(shelfIds))
            shelfIds = ids
        for (let i = 0; i < list.length; ++i) {
            const loader = shelfRepeater.itemAt(i)
            if (!loader)
                continue
            const json = JSON.stringify(list[i])
            if (loader.json !== json) {
                loader.json = json
                loader.shelfData = list[i]
            }
        }
    }
    Connections {
        target: Home
        function onShelvesChanged() { page._syncShelves() }
    }
    onSignedInChanged: _syncShelves()

    onVisibleChanged: if (visible && signedIn) Home.refresh(false)
    Component.onCompleted: {
        _syncShelves()
        if (signedIn)
            Home.refresh(false)
    }
    Connections {
        target: Auth
        function onSignedIn() { Home.refresh(false) }
    }

    function notify(text, actionText, onAction) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, actionText ?? "", onAction ?? null)
    }
    // Прокрутить к сцене радио (после «Радио по треку» из меню)
    function scrollToTop() { flickable.contentY = 0 }

    // ---- Плитки: открыть / играть / меню ----
    function openEntry(e) {
        const k = e.kind ?? ""
        if (k === "dj")             // у DJ нет списка треков: плитка сразу включает его
            playEntry(e)
        else if (k === "liked")
            router.push("liked")
        else if (k === "playlist")
            router.push("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
        else if (k === "radio")     // id — сид (spotify:track:…), uri — станция; Radio.open понимает оба
            router.push("radio", { seed: (e.id ?? "") !== "" ? e.id : (e.uri ?? "") })
        else if (e.uri)
            Catalog.openUri(e.uri)
    }
    // играет (или стоит на паузе) именно отсюда
    function isCurrent(e) {
        return Player.hasTrack && ((e.kind ?? "") === "liked" ? Player.source === "liked"
                                   : (e.uri ?? "") !== "" && Player.contextUri === e.uri)
    }
    // toggle — кнопка на плитке: у играющего отсюда она показывает паузу и ставит на паузу, а не запускает
    // заново (у радио — без новой подборки из 50 треков); «Слушать» в меню только продолжает
    function playEntry(e, toggle) {
        const k = e.kind ?? ""
        if (isCurrent(e)) {
            if (toggle !== false || !Player.playing)
                Player.togglePlay()
            return
        }
        if (k === "liked") {
            if (Library.liked && Library.liked.count > 0)
                Player.playFrom(Library.liked, 0)
            else if (e.uri)
                Player.playContext(e.uri, "")
        } else if (k === "radio") {
            Radio.playStation(e.uri, e.title ?? "")
        } else if (e.uri) {
            Player.playContext(e.uri, "")
        }
    }
    function isPlaying(e) {
        return Player.playing && (e.uri ?? "") !== "" && Player.contextUri === e.uri
    }
    property var _menuEntry: null
    function showMenu(e, point) {
        _menuEntry = e
        const k = e.kind ?? ""
        const items = [{ id: "play", text: I18n.t("Play"), icon: "play_arrow" }]
        if (k !== "radio" && k !== "artist" && k !== "dj")
            items.push({ id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle", enabled: k === "liked" || !!Player.shuffleContext })
        if (k === "playlist" || k === "album" || k === "artist")
            items.push({ id: "radio", text: ({ playlist: I18n.t("Playlist radio"), album: I18n.t("Album radio"), artist: I18n.t("Artist radio") })[k], icon: "radio",
                         enabled: !Radio.locked })
        if (k !== "liked" && k !== "radio" && k !== "dj" && (e.uri ?? "") !== "") {
            const pinned = Library.pinsRevision >= 0 && Library.isPinned(e.uri)
            items.push({ separator: true }, { id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin in library"), icon: "push_pin", hint: I18n.t("local") },
                       { id: "link", text: I18n.t("Copy link"), icon: "content_copy" })
        }
        tileMenu.open(items, tileMenu.mapFromItem(null, point.x, point.y))
    }
    function _menuAction(id) {
        const e = _menuEntry
        if (!e)
            return
        if (id === "play")
            playEntry(e, false)
        else if (id === "shuffle")
            e.kind === "liked" ? Player.shuffleFrom(Library.liked) : Player.shuffleContext(e.uri)
        else if (id === "radio") {
            Radio.playStation(e.uri, e.title ?? "")
            notify(I18n.t("“{0}” radio", e.title ?? ""))
        } else if (id === "pin") {
            const was = Library.isPinned(e.uri)
            Library.togglePin(e.uri)
            notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
        } else if (id === "link") {
            Catalog.copyLink(Catalog.uriUrl(e.uri))
            notify(I18n.t("Link copied"))
        }
    }

    // ---- Сцена «Радио» ----
    RadioOrbit {
        id: orbit
        Layout.fillWidth: true
        Layout.topMargin: 8
        Layout.preferredHeight: implicitHeight
        compact: page.compact
        signedIn: page.signedIn
        premium: Auth.premium
        groups: page.signedIn ? Radio.seeds : []
        selected: Radio.selected
        locked: page.signedIn && Radio.locked
        loading: Radio.loading
        playing: Radio.active && Player.playing
        errorText: Radio.errorText
        onPicked: uri => Radio.select(uri)
        onAddRequested: item => finder.open(item)
        onPlayClicked: Radio.play()
        onOpenList: page.router.push("radio", { seed: Radio.selected })
        onSaveRequested: {
            Radio.save()
            page.notify(I18n.t("Saving station as a playlist…"))
        }
        onRefreshRequested: Radio.refresh()
        onConnectRequested: page.router.reset(Auth.premium ? "playback" : "account")
        onMoreRequested: page.router.reset("settings", { section: "playback" })
        onLoginRequested: page.router.reset("account")
    }

    // ---- Полки ----
    ColumnLayout {
        Layout.fillWidth: true
        visible: page.signedIn
        spacing: 12

        // Загрузка: заглушки без мерцания
        ColumnLayout {
            visible: Home.loading && page.shelfIds.length === 0
            Layout.fillWidth: true
            spacing: 12
            SectionHeader { text: I18n.t("Quick access") }
            GridLayout {
                Layout.fillWidth: true
                columns: page.compact ? 2 : 3
                columnSpacing: 10
                rowSpacing: 10
                Repeater {
                    model: 6
                    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 64; radius: Theme.shape.large; color: Theme.surfaceContainerHigh }
                }
            }
            SectionHeader { text: I18n.t("Recently played") }
            // полка заглушек шире страницы: обрезается, а не растягивает колонку (и сцену над ней)
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: skelRow.implicitHeight
                clip: true
                Row {
                    id: skelRow
                    spacing: 24
                    Repeater { model: 6; SkeletonTile { size: 168 } }
                }
            }
        }

        EmptyState {
            visible: Home.errorText !== "" && page.shelfIds.length === 0 && !Home.loading
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 24
            icon: "cloud_off"
            title: I18n.t("Couldn't load shelves")
            text: Home.errorText
            actionText: I18n.t("Retry")
            actionIcon: "refresh"
            onAction: Home.refresh(true)
        }

        Repeater {
            id: shelfRepeater
            model: page.shelfIds
            Loader {
                id: shelf
                required property var modelData        // {key, kind}
                property var shelfData: ({})
                property string json
                Layout.fillWidth: true
                sourceComponent: modelData.key === "quick" ? quickShelf
                    : modelData.kind === "tracks" ? tracksShelf
                    : rowShelf
            }
        }
    }

    // «Быстрый доступ»: сетка 2–4 колонки
    Component {
        id: quickShelf
        ColumnLayout {
            readonly property var shelfData: parent ? parent.shelfData : ({})
            spacing: 12
            SectionHeader { text: shelfData.title ?? "" }
            GridLayout {
                Layout.fillWidth: true
                columns: Math.max(2, Math.min(4, Math.floor((page.width - 2 * page.sideMargin) / 300)))
                columnSpacing: 10
                rowSpacing: 10
                Repeater {
                    model: shelfData.items ?? []
                    QuickAccessTile {
                        required property var modelData
                        Layout.fillWidth: true
                        entry: modelData
                        onClicked: page.openEntry(modelData)
                        onPlayClicked: page.playEntry(modelData)
                        onMenuRequested: p => page.showMenu(modelData, p)
                    }
                }
            }
        }
    }

    // Топ-треки: две колонки строк (в узком окне — одна), «Показать ещё»
    Component {
        id: tracksShelf
        ColumnLayout {
            id: tracksCol
            readonly property var shelfData: parent ? parent.shelfData : ({})
            readonly property var tracks: shelfData.items ?? []
            property bool expanded: false
            spacing: 12
            SectionHeader {
                text: tracksCol.shelfData.title ?? ""
                actionText: tracksCol.tracks.length > 8 ? (tracksCol.expanded ? I18n.t("Show less") : I18n.t("Show more")) : ""
                onAction: tracksCol.expanded = !tracksCol.expanded
            }
            GridLayout {
                Layout.fillWidth: true
                Layout.leftMargin: -12
                Layout.rightMargin: -12
                columns: page.compact ? 1 : 2
                columnSpacing: 16
                rowSpacing: 0
                Repeater {
                    model: tracksCol.expanded ? tracksCol.tracks : tracksCol.tracks.slice(0, 8)
                    TrackRow {
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        number: index + 1
                        showAlbum: false
                        track: modelData
                        trackId: modelData.trackId ?? ""
                        title: modelData.title ?? ""
                        version: modelData.version ?? ""
                        artists: modelData.artists ?? ""
                        artistRefs: modelData.artistRefs ?? []
                        albumId: modelData.albumId ?? ""
                        album: modelData.album ?? ""
                        cover: modelData.cover ?? ""
                        uri: modelData.uri ?? ""
                        durationMs: modelData.durationMs ?? 0
                        explicit: modelData.explicit ?? false
                        available: modelData.available ?? true
                        liked: Library.likesRevision >= 0 && Library.isLiked(trackId)
                        current: trackId !== "" && trackId === Player.trackId
                        onActivated: Player.playUris(tracksCol.tracks.map(t => t.uri), index)
                        onLikeClicked: Library.toggleLike(modelData)
                    }
                }
            }
        }
    }

    // Горизонтальная полка (плитки, выпуски, исполнители): стрелки листают, колесо листает страницу
    Component {
        id: rowShelf
        ColumnLayout {
            id: rowCol
            readonly property var shelfData: parent ? parent.shelfData : ({})
            readonly property string kind: shelfData.kind ?? "tiles"
            // Полка шире страницы? Условие для стрелок листания (см. ниже, почему не через `visible`)
            readonly property bool scrollable: strip.contentWidth > strip.width + 1
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 8
                z: 1        // подсказка у «ⓘ» длиннее строки заголовка: поверх плиток полки, а не под ними
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)
                    text: rowCol.shelfData.title ?? ""
                    type: "headlineSmall"
                    weight: 600
                }
                Icon {
                    visible: rowCol.shelfData.key === "madeForYou"
                    name: "info"
                    size: 20
                    color: Theme.fgSurfaceVariant
                    HoverHandler { id: infoHover }
                    Rectangle {
                        visible: infoHover.hovered
                        x: 28
                        y: -8
                        z: 10
                        width: 300
                        height: tip.implicitHeight + 16
                        radius: Theme.shape.small
                        color: Theme.inverseSurface
                        Label {
                            id: tip
                            x: 10
                            y: 8
                            width: parent.width - 20
                            text: I18n.t("Personal playlists and mixes from Spotify’s “Made For You” page — read through your account session")
                            type: "bodySmall"
                            color: Theme.fgInverseSurface
                            wrapMode: Text.WordWrap
                        }
                    }
                }
                Label {
                    visible: rowCol.shelfData.key === "topArtists"
                    text: I18n.t("last 4 weeks")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }
                Item { Layout.fillWidth: true }
                // Стрелки не скрываются через `visible`: показ/скрытие меняет ширину строки, от неё
                // зависит `strip.width`, и условие «контент шире полки» начинало мигать — ColumnLayout
                // переполировывался бесконечно (в раскладке полки «possible polish() loop», карточки
                // наезжали друг на друга). Место под стрелки занято всегда, а невидимые выключены.
                IconButton {
                    icon: "chevron_left"
                    size: "xs"
                    enabled: rowCol.scrollable && !strip.atXBeginning
                    opacity: rowCol.scrollable ? 1 : 0
                    onClicked: strip.pageBy(-1)
                }
                IconButton {
                    icon: "chevron_right"
                    size: "xs"
                    enabled: rowCol.scrollable && !strip.atXEnd
                    opacity: rowCol.scrollable ? 1 : 0
                    onClicked: strip.pageBy(1)
                }
            }
            ListView {
                id: strip
                Layout.fillWidth: true
                Layout.leftMargin: -32
                Layout.rightMargin: -32
                Layout.preferredHeight: rowCol.kind === "episodes" ? 104 : rowCol.kind === "artists" ? 200 : 220
                orientation: ListView.Horizontal
                spacing: 24
                leftMargin: 32
                rightMargin: 32
                interactive: false
                clip: true
                // плитки за краем не создаются, ушедшие за край переиспользуются
                reuseItems: true
                cacheBuffer: 360
                model: rowCol.shelfData.items ?? []
                function pageBy(dir) {
                    const target = Math.max(originX - leftMargin, Math.min(originX + contentWidth - width + rightMargin,
                                            contentX + dir * (width - 120)))
                    scrollAnim.to = target
                    scrollAnim.restart()
                }
                NumberAnimation {
                    id: scrollAnim
                    target: strip
                    property: "contentX"
                    duration: Theme.reducedMotion === true ? 0 : Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Theme.motion.emphasized
                }
                // Shift + колесо — листать полку
                WheelHandler {
                    acceptedModifiers: Qt.ShiftModifier
                    onWheel: event => {
                        const d = event.angleDelta.y !== 0 ? event.angleDelta.y : event.angleDelta.x
                        strip.contentX = Math.max(strip.originX - strip.leftMargin,
                            Math.min(strip.originX + strip.contentWidth - strip.width + strip.rightMargin, strip.contentX - d))
                    }
                }
                delegate: Loader {
                    required property var modelData
                    sourceComponent: rowCol.kind === "episodes" ? episodeCard : rowCol.kind === "artists" ? artistTile : mediaTile
                }
                // Плитки — одни компоненты на всю полку, а не по три на каждую плитку
                Component {
                    id: mediaTile
                    MediaTile {
                        readonly property var e: parent ? parent.modelData : ({})
                        size: 168
                        cover: e.cover ?? ""
                        coverKind: e.kind === "album" || e.kind === "artist" ? e.kind : ""
                        coverArtist: e.kind === "artist" ? (e.title ?? "") : (e.subtitle ?? "").split(", ")[0]
                        coverTitle: e.kind === "album" ? (e.title ?? "") : ""
                        title: e.title ?? ""
                        subtitle: e.subtitle ?? ""
                        round: e.kind === "artist"
                        shape: e.kind === "radio" ? "flower8" : e.kind === "dj" ? "cookie12" : e.kind === "artist" ? "circle" : "softSquare"
                        badge: e.kind === "radio" ? I18n.t("RADIO") : ""
                        placeholderIcon: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts", radio: "radio", dj: "graphic_eq" })[e.kind] ?? "album"
                        showPlay: true
                        playing: page.isPlaying(e)
                        onClicked: page.openEntry(e)
                        onPlayClicked: page.playEntry(e)
                        onMenuRequested: p => page.showMenu(e, p)
                        onHoveredChanged: if (hovered && e.kind === "playlist") router.prewarmKeyed("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
                    }
                }
                Component {
                    id: artistTile
                    MediaTile {
                        readonly property var a: parent ? parent.modelData : ({})
                        size: 148
                        round: true
                        cover: a.cover ?? ""
                        coverKind: "artist"
                        coverArtist: a.name ?? ""
                        title: a.name ?? ""
                        subtitle: I18n.t("Artist")
                        placeholderIcon: "person"
                        onClicked: Catalog.openArtist(a.artistId ?? "")
                        onMenuRequested: p => page.showMenu({ kind: "artist", uri: a.uri ?? "", title: a.name ?? "", id: a.artistId ?? "" }, p)
                    }
                }
                Component {
                    id: episodeCard
                    EpisodeCard {
                        readonly property var ep: parent ? parent.modelData : ({})
                        episode: ep
                        onClicked: Catalog.openEpisode(ep.episodeId ?? "")
                    }
                }
            }
        }
    }

    RadioSeedFinder {
        id: finder
        parent: page
        anchors.fill: parent
        z: 40
        onPicked: (uri, title) => Radio.select(uri)
    }
    Menu {
        id: tileMenu
        parent: page
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => page._menuAction(id)
    }
}
