import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Библиотека: разделы чипами (Плейлисты · Альбомы · Исполнители · Подкасты), у плейлистов — владелец
// (Все · Мои · Подписки · Совместные); сетка или список (Settings.libraryView), сортировка
// (Settings.librarySort), локальный фильтр, папки rootlist (только чтение), локальные закрепления.
// Сетка виртуализирована: ListView из рядов по cols плиток; «Любимые треки» — первая плитка на две ячейки.
Item {
    id: page

    property var router
    property string folderId
    property string folderTitle
    readonly property string title: folderId !== "" ? (folderTitle !== "" ? folderTitle : I18n.t("Folder")) : I18n.t("Library")
    readonly property bool showHeader: false
    readonly property bool scrolled: list.contentY - list.originY > 8

    property string kind: "playlists"         // playlists | albums | artists | shows
    property string owner: "all"              // all | mine | followed | collab
    property bool searchOpen: false
    property string query
    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property bool inFolder: folderId !== ""
    readonly property bool grid: Settings.libraryView !== "list"
    readonly property bool showLiked: kind === "playlists" && !inFolder && owner === "all" && query.trim() === ""

    readonly property var model: !signedIn ? null
        : inFolder ? Library.folderChildren(folderId)
        : Library.items(kind, owner, Settings.librarySort)

    component Head: Label {
        property bool active: false
        type: "labelMedium"
        color: active ? Theme.primary : Theme.fgSurfaceVariant
    }

    readonly property var sorts: [
        { id: "recent", text: I18n.t("Recents") },
        { id: "added", text: I18n.t("Recently added") },
        { id: "alpha", text: I18n.t("Alphabetical") },
        { id: "author", text: I18n.t("Creator") },
        { id: "spotify", text: I18n.t("Spotify order") }
    ]
    readonly property string sortText: (sorts.find(s => s.id === Settings.librarySort) ?? sorts[0]).text

    // ---- Элементы: модель → массив (фильтр по тексту, без «Любимых» — у них своя плитка) ----
    property var entries: []
    function _rebuild() {
        const out = []
        const m = page.model
        const q = page.query.trim().toLowerCase()
        // папки в корне есть только в порядке Spotify; их дети — внутри папки. В других сортировках
        // сервис отдаёт плоский список (folderId — для справки), и такие строки не прячем
        const folders = new Set()
        let liked = -1
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "folder")
                folders.add(e.id)
            else if (e.kind === "liked")
                liked = e.trackCount ?? -1
        }
        likedTotal = liked
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "liked")
                continue
            if (!page.inFolder && folders.has(e.folderId ?? ""))
                continue
            if (q !== "" && ((e.title ?? "") + " " + (e.subtitle ?? "") + " " + (e.owner ?? "")).toLowerCase().indexOf(q) < 0)
                continue
            out.push(e)
        }
        entries = out
    }
    onModelChanged: { _rebuild(); _animateIn() }
    onQueryChanged: _rebuild()
    Component.onCompleted: _rebuild()
    Connections {
        target: page.model
        ignoreUnknownSignals: true
        function onCountChanged() { page._rebuild() }
        function onModelReset() { page._rebuild() }
        function onDataChanged() { page._rebuild() }
        function onRowsMoved() { page._rebuild() }
    }
    Connections {
        target: Library
        function onPinsChanged() { page._rebuild() }
    }

    // Появление плиток после смены раздела: каскад с задержкой 25 мс (не больше 12 шагов)
    property bool _cascade: false
    function _animateIn() {
        if (Theme.reducedMotion === true)
            return
        _cascade = true
        cascadeOff.restart()
    }
    Timer { id: cascadeOff; interval: 900; onTriggered: page._cascade = false }

    // ---- Сетка: ячейка от 168, зазор 28 × 22, ряды по cols ----
    // Остаток ширины делится между плитками (они растут), а не уходит в зазоры: иначе при 2–3 колонках
    // между плитками зияли бы промежутки по 100+ px
    readonly property real minCell: 168
    readonly property real gapY: 28
    readonly property real innerWidth: list.width - 64
    readonly property int cols: Math.max(2, Math.floor((innerWidth + 22) / (minCell + 22)))
    readonly property real cell: Math.max(minCell, Math.floor((innerWidth - (cols - 1) * 22) / cols))
    readonly property real gapX: cols > 1 ? Math.max(22, (innerWidth - cols * cell) / (cols - 1)) : 0
    readonly property var rows: {
        const out = []
        let row = []
        let used = 0
        if (showLiked) {
            row.push({ liked: true })
            used = 2
        }
        for (const e of entries) {
            if (used >= cols) {
                out.push(row)
                row = []
                used = 0
            }
            row.push(e)
            used++
        }
        if (row.length > 0)
            out.push(row)
        return out
    }
    readonly property var listEntries: showLiked ? [{ kind: "liked", title: I18n.t("Liked Songs"),
        subtitle: I18n.t("Playlist") + " · " + I18n.n(likedCount, "{0} track", "{0} tracks"), uri: "" }].concat(entries) : entries
    // всего «Любимых»: из строки kind "liked" (сервис знает общее число), иначе — сколько уже загружено
    property int likedTotal: -1
    readonly property int likedCount: likedTotal >= 0 ? likedTotal : Library.liked ? Library.liked.count : 0
    // смена вида или раздела — список с начала
    onGridChanged: Qt.callLater(() => list.positionViewAtBeginning())
    onKindChanged: Qt.callLater(() => list.positionViewAtBeginning())

    function notify(text, actionText, onAction) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, actionText ?? "", onAction ?? null)
    }

    // ---- Действия ----
    // Наведение на карточку: страница строится в фоне, клик открывает мгновенно.
    function prewarmEntry(e) {
        if (!e || e.kind !== "playlist")
            return
        router.prewarmKeyed("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
    }

    function open(e) {
        const k = e.kind ?? ""
        if (k === "liked")
            router.push("liked")
        else if (k === "folder")
            router.push("folder", { folderId: e.id, folderTitle: e.title ?? "" })
        else if (k === "playlist")
            router.push("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
        else if (k === "album")
            Catalog.openAlbum(e.id)
        else if (k === "artist")
            Catalog.openArtist(e.id)
        else if (k === "show")
            Catalog.openShow(e.id)
    }
    function play(e) {
        if ((e.kind ?? "") === "liked" || e.liked) {
            if (Library.liked && Library.liked.count > 0)
                Player.playFrom(Library.liked, 0)
            return
        }
        if (e.uri)
            Player.playContext(e.uri, "")
    }
    function _isPlaying(e) {
        return Player.playing && (e.uri ?? "") !== "" && Player.contextUri === e.uri
    }

    property var _menuEntry: null
    function showMenu(e, point) {
        _menuEntry = e
        const p = tileMenu.mapFromItem(null, point.x, point.y)
        tileMenu.open(_menuItems(e), p)
    }
    function _menuItems(e) {
        const k = e.kind ?? ""
        if (k === "folder")
            return [
                { id: "open", text: I18n.t("Open folder"), icon: "folder_open" },
                { separator: true },
                { id: "rename", text: I18n.t("Rename — Spotify only"), icon: "edit", enabled: false },
                { id: "move", text: I18n.t("Move — Spotify only"), icon: "folder", enabled: false }
            ]
        if (k === "liked")
            return [
                { id: "play", text: I18n.t("Play"), icon: "play_arrow" },
                { id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle" }
            ]
        const pinned = Library.pinsRevision >= 0 && Library.isPinned(e.uri ?? "")
        const radioText = ({ playlist: I18n.t("Playlist radio"), album: I18n.t("Album radio"), artist: I18n.t("Artist radio") })[k] ?? I18n.t("Radio")
        const items = [
            { id: "play", text: I18n.t("Play"), icon: "play_arrow" },
            { id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle", enabled: !!Player.shuffleContext },
            { id: "radio", text: radioText, icon: "radio", enabled: k !== "show" },
            { separator: true },
            { id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin to top"), icon: "push_pin", hint: I18n.t("local") }
        ]
        if (k === "playlist" && e.canEdit && !e.readOnly) {
            items.push({ id: "edit", text: I18n.t("Edit"), icon: "edit" })
            if ((e.ownerId ?? "") === (Auth.account.id ?? "-"))
                items.push({ id: "delete", text: I18n.t("Delete playlist"), icon: "delete" })
        } else if (k === "playlist") {
            items.push({ id: "unfollow", text: I18n.t("Unfollow"), icon: "remove_circle" })
        } else {
            items.push({ id: "unsave", text: k === "artist" ? I18n.t("Unfollow artist") : I18n.t("Remove from library"), icon: "remove_circle" })
        }
        items.push({ separator: true }, { id: "link", text: I18n.t("Copy link"), icon: "content_copy", enabled: (e.uri ?? "") !== "" })
        return items
    }
    function _menuAction(id) {
        const e = _menuEntry
        if (!e)
            return
        const t = e.title ?? ""
        if (id === "open" || id === "edit") {
            open(e)
        } else if (id === "play") {
            play(e)
        } else if (id === "shuffle") {
            if ((e.kind ?? "") === "liked")
                Player.shuffleFrom(Library.liked)
            else if (Player.shuffleContext)
                Player.shuffleContext(e.uri)
        } else if (id === "radio") {
            Radio.playStation(e.uri, t)
            notify(I18n.t("“{0}” radio", t))
        } else if (id === "pin") {
            const was = Library.isPinned(e.uri)
            Library.togglePin(e.uri)
            notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
        } else if (id === "delete" || id === "unfollow") {
            Library.unfollowPlaylist(e.id)
            notify(id === "delete" ? I18n.t("Playlist deleted") : I18n.t("Unfollowed “{0}”", t), I18n.t("Undo"), () => Library.followPlaylist(e.id))
        } else if (id === "unsave") {
            Library.toggleSaved(e.uri)
            notify(I18n.t("Removed from library"), I18n.t("Undo"), () => Library.toggleSaved(e.uri))
        } else if (id === "link") {
            Catalog.copyLink(Catalog.uriUrl(e.uri))
            notify(I18n.t("Link copied"))
        }
    }

    // ---- Кнопки в верхней панели: поиск по библиотеке, сортировка, сетка | список ----
    readonly property Component toolbar: signedIn ? toolbarComponent : null
    Component {
        id: toolbarComponent
        Row {
            spacing: 8
            Item {
                width: page.searchOpen ? 240 : 0
                height: 40
                clip: true
                anchors.verticalCenter: parent.verticalCenter
                Behavior on width { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                SearchField {
                    id: libSearch
                    width: 240
                    height: 40
                    placeholder: I18n.t("Search in library")
                    text: page.query
                    onTextChanged: page.query = text
                    Keys.onEscapePressed: { page.query = ""; page.searchOpen = false }
                }
            }
            IconButton {
                anchors.verticalCenter: parent.verticalCenter
                icon: "search"
                checkable: true
                autoToggle: false
                checked: page.searchOpen
                onClicked: {
                    page.searchOpen = !page.searchOpen
                    if (page.searchOpen)
                        Qt.callLater(() => libSearch.inputItem.forceActiveFocus())
                    else
                        page.query = ""
                }
            }
            Button {
                id: sortButton
                visible: !page.inFolder
                anchors.verticalCenter: parent.verticalCenter
                style: "text"
                icon: "sort"
                text: page.width < 760 ? "" : page.sortText
                onClicked: {
                    const p = sortButton.mapToItem(sortMenu, 0, sortButton.height)
                    page._menuEntry = null
                    sortMenu.open(page.sorts.map(s => ({ id: s.id, text: s.text, checked: s.id === Settings.librarySort }))
                        .concat([{ separator: true }, { id: "info", text: I18n.t("Order isn't saved to Spotify"), icon: "info", enabled: false }]), p)
                }
            }
            ButtonGroup {
                anchors.verticalCenter: parent.verticalCenter
                model: [{ icon: "grid_view" }, { icon: "view_list" }]
                autoSelect: false
                currentIndex: page.grid ? 0 : 1
                onActivated: i => page._switchView(i === 0 ? "grid" : "list")
            }
        }
    }

    // Сетка ↔ список: гаснет и проявляется заново
    function _switchView(v) {
        if (Settings.libraryView === v)
            return
        if (Theme.reducedMotion === true) {
            Settings.libraryView = v
            return
        }
        viewSwap.next = v
        viewSwap.restart()
    }
    SequentialAnimation {
        id: viewSwap
        property string next
        NumberAnimation { target: list; property: "opacity"; to: 0; duration: Theme.motion.effectsDefault / 2 }
        ScriptAction { script: { Settings.libraryView = viewSwap.next; page._animateIn() } }
        NumberAnimation { target: list; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault }
    }

    ListView {
        id: list
        anchors.fill: parent
        model: !page.signedIn ? [] : page.grid ? page.rows : page.listEntries
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        acceptedButtons: Qt.NoButton
        cacheBuffer: 800
        // делегат — Loader со стабильным sourceComponent (меняется только при смене сетка/список),
        // поэтому строки можно переиспользовать при прокрутке
        reuseItems: true
        spacing: page.grid ? page.gapY : 0

        header: Item {
            width: list.width
            height: page.signedIn ? headerCol.implicitHeight + (page.grid ? 20 : 12) : 0
            visible: page.signedIn

            ColumnLayout {
                id: headerCol
                x: 32
                y: 4
                width: parent.width - 64
                spacing: 12

                // Хлебные крошки папки
                RowLayout {
                    visible: page.inFolder
                    Layout.fillWidth: true
                    spacing: 8
                    Label {
                        text: I18n.t("Library")
                        type: "bodyLarge"
                        color: Theme.fgSurfaceVariant
                        TapHandler { onTapped: page.router.reset("library") }
                        HoverHandler { cursorShape: Qt.PointingHandCursor }
                    }
                    Icon { name: "chevron_right"; size: 20; color: Theme.fgSurfaceVariant }
                    Label { text: page.title; type: "titleLarge"; weight: 500 }
                    ReadOnlyBadge { text: I18n.t("Spotify folder · read-only") }
                    Item { Layout.fillWidth: true }
                }

                // Разделы
                Flow {
                    visible: !page.inFolder
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: [
                            { id: "playlists", text: I18n.t("Playlists"), n: Library.playlists.count },
                            { id: "albums", text: I18n.t("Albums"), n: Library.albums.count },
                            { id: "artists", text: I18n.t("Artists"), n: Library.artists.count },
                            { id: "shows", text: I18n.t("Podcasts"), n: Library.shows.count }
                        ]
                        Chip {
                            required property var modelData
                            text: modelData.text + (modelData.n > 0 ? "  " + modelData.n : "")
                            selected: page.kind === modelData.id
                            onClicked: { page.kind = modelData.id; page.owner = "all" }
                        }
                    }
                }
                // Владелец (только плейлисты)
                Flow {
                    visible: !page.inFolder && page.kind === "playlists"
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: [
                            { id: "all", text: I18n.t("All") }, { id: "mine", text: I18n.t("Mine") },
                            { id: "followed", text: I18n.t("Followed") }, { id: "collab", text: I18n.t("Collaborative") }
                        ]
                        Chip {
                            required property var modelData
                            implicitHeight: 28
                            text: modelData.text
                            selected: page.owner === modelData.id
                            onClicked: page.owner = modelData.id
                        }
                    }
                }

                // Явная сортировка догружает страницы
                Rectangle {
                    visible: Library.pagesTotal > 0 && Library.pagesLoaded < Library.pagesTotal
                    implicitWidth: loadRow.implicitWidth + 18
                    implicitHeight: 32
                    radius: 16
                    color: Theme.secondaryContainer
                    Row {
                        id: loadRow
                        anchors.verticalCenter: parent.verticalCenter
                        x: 6
                        spacing: 8
                        LoadingIndicator { size: 22; color: Theme.fgSecondaryContainer; running: parent.parent.visible; anchors.verticalCenter: parent.verticalCenter }
                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            text: I18n.t("Loading remaining pages · {0} of {1} — loaded items are already here", Library.pagesLoaded, Library.pagesTotal)
                            type: "labelMedium"
                            color: Theme.fgSecondaryContainer
                        }
                    }
                }

                // Подсказки
                Rectangle {
                    visible: page.kind === "shows" && !page.inFolder || page.inFolder
                    Layout.fillWidth: true
                    implicitHeight: hintRow.implicitHeight + 24
                    radius: Theme.shape.large
                    color: Theme.surfaceContainerHigh
                    RowLayout {
                        id: hintRow
                        x: 16
                        y: 12
                        width: parent.width - 32
                        spacing: 12
                        Icon { Layout.alignment: Qt.AlignTop; name: "info"; size: 20; color: Theme.primary }
                        Label {
                            Layout.fillWidth: true
                            text: page.inFolder
                                ? I18n.t("Playlists in this folder can be opened, played and edited (your own). The folder itself can't be changed — librespot reads folders but can't write them.")
                                : I18n.t("Audiobooks from saved shows are hidden: librespot can't play them.")
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                // Заголовки колонок списка
                RowLayout {
                    visible: !page.grid && page.listEntries.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    spacing: 16
                    readonly property string sort: Settings.librarySort
                    Head { Layout.fillWidth: true; Layout.preferredWidth: 3; text: I18n.t("Title") + (active ? "  ↓" : ""); active: parent.sort === "alpha" }
                    Head { Layout.fillWidth: true; Layout.preferredWidth: 1.4; text: I18n.t("Creator") + (active ? "  ↓" : ""); active: parent.sort === "author" }
                    Head { visible: list.width - 40 >= 640; Layout.preferredWidth: 120; text: I18n.t("Added") + (active ? "  ↓" : ""); active: parent.sort === "added" }
                    Head { visible: list.width - 40 >= 860; Layout.preferredWidth: 120; text: I18n.t("Last played") + (active ? "  ↓" : ""); active: parent.sort === "recent" }
                    Item { Layout.preferredWidth: 32 }
                }
                Rectangle {
                    visible: !page.grid && page.listEntries.length > 0
                    Layout.fillWidth: true
                    Layout.topMargin: -12
                    height: 1
                    color: Theme.outlineVariant
                }
            }
        }

        delegate: Loader {
            id: cellLoader
            required property var modelData
            required property int index
            width: list.width
            sourceComponent: page.grid ? gridRow : listRow
        }

        footer: Item { width: list.width; height: 140 }
    }
    SmoothScroll { flickable: list; wheelStep: Settings.wheelStep }

    // Ряд сетки: плитки по cols, равномерно по ширине
    Component {
        id: gridRow
        Item {
            id: rowItem
            readonly property var items: parent ? parent.modelData : []
            readonly property int rowIndex: parent ? parent.index : 0
            width: list.width
            height: rowContent.implicitHeight
            Row {
                id: rowContent
                x: 32
                spacing: page.gapX
                Repeater {
                    model: rowItem.items
                    Loader {
                        id: tileLoader
                        required property var modelData
                        required property int index
                        readonly property int order: rowItem.rowIndex * page.cols + index
                        sourceComponent: modelData.liked ? likedTile : modelData.kind === "folder" ? folderTile : mediaTile
                        // каскад появления: fade + сдвиг снизу 12 px
                        opacity: 1
                        transform: Translate { id: shift; y: 0 }
                        Component.onCompleted: {
                            if (!page._cascade)
                                return
                            opacity = 0
                            shift.y = 12
                            appear.start()
                        }
                        SequentialAnimation {
                            id: appear
                            PauseAnimation { duration: Math.min(tileLoader.order, 12) * 25 }
                            ParallelAnimation {
                                NumberAnimation { target: tileLoader; property: "opacity"; to: 1; duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
                                NumberAnimation { target: shift; property: "y"; to: 0; duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
                            }
                        }
                        Component {
                            id: likedTile
                            LikedTile {
                                width: 2 * page.cell + page.gapX
                                trackCount: page.likedCount
                                height: page.cell + 48
                                onClicked: page.open({ kind: "liked" })
                                onPlayClicked: page.play({ kind: "liked" })
                                onMenuRequested: p => page.showMenu({ kind: "liked" }, p)
                            }
                        }
                        Component {
                            id: folderTile
                            FolderTile {
                                size: page.cell
                                title: tileLoader.modelData.title ?? ""
                                subtitle: tileLoader.modelData.subtitle ?? ""
                                covers: tileLoader.modelData.covers ?? []
                                onClicked: page.open(tileLoader.modelData)
                                onMenuRequested: p => page.showMenu(tileLoader.modelData, p)
                            }
                        }
                        Component {
                            id: mediaTile
                            MediaTile {
                                readonly property var e: tileLoader.modelData
                                size: page.cell
                                cover: e.cover ?? ""
                                title: e.title ?? ""
                                subtitle: e.subtitle ?? ""
                                round: e.kind === "artist"
                                placeholderIcon: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts" })[e.kind] ?? "album"
                                pinned: e.pinned === true
                                showPlay: true
                                playing: page._isPlaying(e)
                                onClicked: page.open(e)
                                onPlayClicked: page._isPlaying(e) ? Player.togglePlay() : page.play(e)
                                onMenuRequested: p => page.showMenu(e, p)
                                onHoveredChanged: if (hovered) page.prewarmEntry(e)
                            }
                        }
                    }
                }
            }
        }
    }

    Component {
        id: listRow
        Item {
            readonly property var e: parent ? parent.modelData : ({})
            implicitHeight: 64
            LibraryRow {
                x: 20
                width: parent.width - 40
                entry: parent.e
                onClicked: page.open(parent.e)
                onMenuRequested: p => page.showMenu(parent.e, p)
            }
        }
    }

    // ---- Загрузка, пусто, не вошли ----
    Grid {
        visible: page.signedIn && page.model !== null && page.model.loading === true && page.model.count === 0
        x: 32
        y: 120
        width: page.width - 64
        columns: page.cols
        columnSpacing: page.gapX
        rowSpacing: page.gapY
        Repeater {
            model: 10
            SkeletonTile { size: page.cell }
        }
    }
    EmptyState {
        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.max(140, (parent.height - implicitHeight) / 2)
        width: Math.min(parent.width - 64, 420)
        visible: page.signedIn && page.model !== null && page.model.loading !== true && page.entries.length === 0 && !page.showLiked
        // не загрузилось (сессия не ответила, Web API под 429) — ошибка и «Повторить», а не «пусто»
        readonly property bool failed: Library.errorText !== "" && page.query.trim() === "" && page.owner === "all"
        readonly property var texts: failed
            ? ["cloud_off", I18n.t("Couldn’t load your library"), Library.errorText]
            : page.query.trim() !== "" || page.owner !== "all"
            ? ["search_off", I18n.t("Nothing found"), page.query.trim() !== "" ? I18n.t("Try a different search") : I18n.t("This section is empty")]
            : ({
                playlists: ["queue_music", page.inFolder ? I18n.t("Folder is empty") : I18n.t("No playlists"), I18n.t("Playlists you create or save will appear here.")],
                albums: ["album", I18n.t("No saved albums"), I18n.t("Saved albums will appear here.")],
                artists: ["person", I18n.t("No followed artists"), I18n.t("Artists you follow will appear here.")],
                shows: ["podcasts", I18n.t("No podcasts yet"), I18n.t("Podcasts you follow will appear here.")]
            })[page.kind]
        icon: texts[0]
        title: texts[1]
        text: texts[2]
        // ничего не нашлось — кнопка снимает то, что сузило список (поиск или фильтр владельца)
        actionText: failed ? I18n.t("Retry")
                  : page.query.trim() !== "" ? I18n.t("Clear search")
                  : page.owner !== "all" ? I18n.t("Show all")
                  : page.kind === "albums" ? I18n.t("Find albums") : ""
        actionIcon: failed ? "refresh" : page.query.trim() !== "" || page.owner !== "all" ? "close" : "search"
        onAction: {
            if (failed) {
                Library.refresh()
            } else if (page.query.trim() !== "") {
                page.query = ""
                page.searchOpen = false
            } else if (page.owner !== "all") {
                page.owner = "all"
            } else {
                page.router.reset("search")
            }
        }
    }
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, 420)
        visible: !page.signedIn
        icon: "library_music"
        title: I18n.t("Sign in to see your library")
        text: I18n.t("Playlists, albums, artists and podcasts will load from your Spotify account")
        actionText: I18n.t("Sign in")
        actionIcon: "login"
        onAction: page.router.reset("account")
    }

    Menu {
        id: tileMenu
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => page._menuAction(id)
    }
    Menu {
        id: sortMenu
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => { if (id !== "info") Settings.librarySort = id }
    }
}
