import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import "app"

// Раскладка телефона (сборка для Android; на десктопе — --mobile): шапка страницы | страница | мини-плеер над
// нижней навигацией; «Сейчас играет» — на весь экран. Страницы, меню, диалоги и действия — те же, что в Main.qml.
// Системные панели Android прозрачные: интерфейс рисуется под ними, отступы — Theme.safeTop / safeBottom.
Window {
    id: win
    objectName: "materialfyMobileWindow"

    property string startPage: "home"     // --set startPage=settings | artist:<id>

    width: 412
    height: 915
    visible: true
    title: Player.hasTrack ? Player.title + " — " + Player.artist + " · Materialfy" : "Materialfy"
    color: Theme.surface
    // Android: окно под статус-баром и полосой навигации (edge-to-edge)
    flags: MobileShell.android ? (Qt.Window | Qt.ExpandedClientAreaHint | Qt.NoTitleBarBackgroundHint) : Qt.Window

    // Совместимость с компонентами, написанными под Main.qml (они спрашивают окно о раскладке)
    readonly property string layoutMode: "compact"
    readonly property bool mini: false
    readonly property bool panelFits: false
    readonly property bool wayland: false
    readonly property bool canPin: false
    property bool softwareRendering: false
    readonly property bool mobile: true

    // Разделы нижней навигации; остальные страницы открываются поверх раздела (стек Router)
    readonly property var sections: [
        { name: "home", icon: "home", text: I18n.t("Home") },
        { name: "search", icon: "search", text: I18n.t("Search") },
        { name: "library", icon: "library_music", text: I18n.t("Library") },
        { name: "settings", icon: "settings", text: I18n.t("Settings") }
    ]
    // Раздел, к которому относится открытая страница (у вложенных — раздел корня стека)
    readonly property string section: {
        const name = router.rootName
        for (const s of sections)
            if (s.name === name)
                return name
        // «Аккаунт», «Воспроизведение» — из настроек; «Любимые», «Новинки», папки — из медиатеки
        if (name === "account" || name === "playback")
            return "settings"
        if (name === "liked" || name === "releases" || name === "folder")
            return "library"
        return "home"
    }

    // ---- Безопасные зоны: системные панели, вырез камеры. На десктопе — как у типичного телефона ----
    Binding { target: Theme; property: "mobile"; value: true }
    Binding {
        target: Theme; property: "safeTop"
        value: MobileShell.android ? Math.max(safe.SafeArea.margins.top, 0) : 32
    }
    Binding {
        target: Theme; property: "safeBottom"
        value: MobileShell.android ? Math.max(safe.SafeArea.margins.bottom, 0) : 16
    }
    Item { id: safe; anchors.fill: parent }

    // Значки статус-бара и навигации — под тему: тёмные на светлой
    Connections {
        target: Theme
        function onDarkChanged() { MobileShell.setSystemBars(!Theme.dark) }
    }
    Component.onCompleted: {
        MobileShell.setSystemBars(!Theme.dark)
        MobileShell.setMediaLiked(_mediaLiked)
    }

    // ---- Плеер → уведомление и экран блокировки; их кнопки → плеер ----
    // сердце в карточке плеера системы — как на мини-плеере
    readonly property bool _mediaLiked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
    on_MediaLikedChanged: MobileShell.setMediaLiked(_mediaLiked)
    function syncMedia() {
        const cover = Player.coverFile !== "" ? Player.coverFile : (Player.coverLarge !== "" ? Player.coverLarge : Player.cover)
        MobileShell.updateMedia(Player.title, Player.artist, Player.album, cover, Player.durationMs, Player.positionMs,
                                Player.playing, Player.hasTrack, Player.hasTrack, Player.hasTrack, Player.isLocal === true)
    }
    // Сигналы плеера приходят пачкой (трек, состояние, позиция) — одно обновление на пачку. Qt.callLater, а не
    // Timer: таймеры QML идут от отрисовки и в фоне (окно не показано) не срабатывают — уведомление застывало
    function _queueMediaSync() { Qt.callLater(win.syncMedia) }
    Connections {
        target: Player
        function onTrackChanged() { win._queueMediaSync() }
        function onStateChanged() { win._queueMediaSync() }
        function onCoverFileChanged() { win._queueMediaSync() }
        function onDeviceChanged() { win._queueMediaSync() }
        // позицию система продолжает сама; обновляем после перемотки (скачок больше 2 с от ожидаемой)
        property real _at: 0
        property real _pos: 0
        function onPositionChanged() {
            const now = Date.now()
            const expected = _pos + (Player.playing ? now - _at : 0)
            if (Math.abs(Player.positionMs - expected) > 2000)
                win._queueMediaSync()
            _at = now
            _pos = Player.positionMs
        }
    }
    Connections {
        target: MobileShell
        function onMediaCommand(command, arg) {
            switch (command) {
            case "play": Player.play(); break
            case "pause": Player.pause(); break
            case "toggle": Player.togglePlay(); break
            case "next": Player.next(); break
            case "previous": Player.previous(); break
            case "seek": Player.seekMs(arg); break
            case "stop": Player.pause(); break
            case "back": win.goBack(); break     // «Назад» системы (MaterialfyActivity)
            case "like": if (Player.uri !== "") Library.toggleSaved(Player.uri); break   // сердце карточки плеера
            }
        }
    }

    // Вход в браузере: на время ожидания ответа процесс держит короткий foreground-сервис (иначе Android может
    // заморозить приложение в фоне, и ответ Spotify на 127.0.0.1 повиснет)
    readonly property bool _browserWait: Auth.state === "awaitingBrowser" || Auth.playbackState === "authorizing"
                                         || Auth.webState === "authorizing" || Auth.personalState === "authorizing"
    on_BrowserWaitChanged: MobileShell.setBrowserWait(_browserWait)

    // ---- «Назад» системы: клавиатура → меню и диалоги → плеер → стек страниц → главная → свернуть ----
    function goBack() {
        if (Qt.inputMethod.visible) {
            Qt.inputMethod.hide()
            return
        }
        if (trackMenu.back() || BackStack.back())
            return
        if (nowPlaying.open) {
            if (nowPlaying.sheetOpen)
                nowPlaying.sheetOpen = false
            else
                nowPlaying.hide()
            return
        }
        if (router.currentItem && typeof router.currentItem.handleBack === "function" && router.currentItem.handleBack())
            return
        if (router.back())
            return
        if (router.rootName !== "home") {
            router.reset("home")
            return
        }
        MobileShell.moveTaskToBack()
    }
    // Android шлёт «Назад» через MaterialfyActivity (mediaCommand "back"); клавиша — для клавиатуры и эмулятора.
    // Одна последовательность: "Back" и StandardKey.Back на Android — одна клавиша, и Shortcut с двумя
    // одинаковыми считался неоднозначным (activatedAmbiguously вместо activated)
    Shortcut {
        sequence: "Back"
        context: Qt.ApplicationShortcut
        onActivated: win.goBack()
        onActivatedAmbiguously: win.goBack()
    }
    Shortcut { sequence: "Esc"; onActivated: win.goBack() }

    Item {
        id: scene
        anchors.fill: parent

        Rectangle { anchors.fill: parent; color: Theme.surface }

        // Атмосферный фон — мягче, чем на десктопе: на маленьком экране он ближе к тексту
        TrackBackdrop {
            id: ambient
            anchors.fill: parent
            visible: Settings.ambientBackground && Player.cover !== ""
            opacity: Theme.dark ? 0.22 : 0.26
            source: Player.cover
            mode: Settings.backdropMode === "glass" ? "gauss" : Settings.backdropMode
            // за страницами — без «дрейфа»: при прозрачности 22 % его почти не видно, а он 30 раз в секунду
            // перерисовывал весь экран, пока играет музыка (≈8 % CPU). Дрейф остаётся в полноэкранном плеере
            flow: 0
            followTheme: Settings.accentFromCover
            running: visible && Player.playing && !nowPlaying.open && Qt.application.state === Qt.ApplicationActive
        }

        Item {
            id: content
            anchors.fill: parent
            anchors.bottomMargin: navBar.height

            // Полоса под статус-баром: при прокрутке страница уходит под неё, а не под значки
            Rectangle {
                id: statusScrim
                anchors.top: parent.top
                width: parent.width
                height: Theme.safeTop
                color: Theme.surface
                opacity: ambient.visible ? 0.86 : 1
                z: 3
            }

            TopBar {
                id: topBar
                anchors.top: statusScrim.bottom
                anchors.left: parent.left
                anchors.right: parent.right
                router: router
                window: win
                z: 2
                // главная: новинки и аккаунт — справа в шапке (в нижней навигации им места нет); на других
                // страницах слота нет совсем, иначе он занимал бы место кнопок страницы
                trailing: router.currentName === "home" ? homeActions : null
            }
            Component {
                id: homeActions
                Row {
                    spacing: 4
                    IconButton {
                        icon: "new_releases"
                        onClicked: router.push("releases")
                    }
                    Item {
                        width: 40
                        height: 40
                        Avatar {
                            anchors.centerIn: parent
                            size: 32
                            name: Auth.account ? (Auth.account.displayName ?? "") : ""
                            source: Auth.account ? (Auth.account.avatar ?? "") : ""
                        }
                        StateLayer {
                            anchors.fill: parent
                            radius: width / 2
                            onClicked: router.push("account")
                        }
                    }
                }
            }

            Item {
                id: pageHost
                anchors.top: topBar.bottom
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                // без обрезки: фон шапки страницы (баннер исполнителя, фигуры) заходит под прозрачную верхнюю панель

                Router {
                    id: router
                    anchors.fill: parent
                    pages: ({
                        home: Qt.resolvedUrl("app/pages/HomePage.qml"),
                        search: Qt.resolvedUrl("app/pages/SearchPage.qml"),
                        library: Qt.resolvedUrl("app/pages/LibraryPage.qml"),
                        folder: Qt.resolvedUrl("app/pages/FolderPage.qml"),
                        liked: Qt.resolvedUrl("app/pages/LikedPage.qml"),
                        playlist: Qt.resolvedUrl("app/pages/PlaylistPage.qml"),
                        album: Qt.resolvedUrl("app/pages/AlbumPage.qml"),
                        artist: Qt.resolvedUrl("app/pages/ArtistPage.qml"),
                        show: Qt.resolvedUrl("app/pages/ShowPage.qml"),
                        episode: Qt.resolvedUrl("app/pages/EpisodePage.qml"),
                        radio: Qt.resolvedUrl("app/pages/RadioPage.qml"),
                        releases: Qt.resolvedUrl("app/pages/ReleasesPage.qml"),
                        settings: Qt.resolvedUrl("app/pages/SettingsPage.qml"),
                        account: Qt.resolvedUrl("app/pages/AccountPage.qml"),
                        playback: Qt.resolvedUrl("app/pages/PlaybackSetupPage.qml")
                    })
                    // разделы нижней навигации живут между переключениями: «Настройки» и «Поиск» пересоздавались
                    // на каждый тап, на телефоне это заметная пауза
                    // «Любимые» — тоже: сборка ~150 строк занимала до 0,75 с между тапом и переходом
                    keepAlive: ["home", "search", "library", "settings", "liked"]
                    pushOnReset: ["account", "playback", "liked", "releases", "radio", "folder"]
                    Component.onCompleted: {
                        Qt.callLater(router._prewarm)
                        const [name, id] = win.startPage.split(":")
                        if (!id) {
                            reset(name)
                            return
                        }
                        reset("home")
                        const open = () => {
                            if (["ready", "error", "premiumRequired"].indexOf(Auth.playbackState) < 0)
                                return
                            Auth.playbackChanged.disconnect(open)
                            push(name, ({ [name + "Id"]: id }))
                        }
                        Auth.playbackChanged.connect(open)
                        open()
                    }
                }
            }

            MobileMiniPlayer {
                id: miniPlayer
                suppressed: nowPlaying.holdsMini
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 8
                z: 3
                onOpenNowPlaying: center => nowPlaying.show(center)
            }
        }

        MobileNavBar {
            id: navBar
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            items: win.sections
            current: win.section
            z: 4
            onActivated: name => {
                if (nowPlaying.open)
                    nowPlaying.hide()
                router.reset(name)
            }
            // повторный тап по разделу: к его корню, а на корне — наверх страницы
            onReselected: name => {
                if (router.depth > 1 || router.rootName !== name) {
                    router.reset(name)
                    return
                }
                const page = router.currentItem
                if (page && page.flickable)
                    page.flickable.contentY = 0
                if (page && typeof page.focusSearch === "function")
                    page.focusSearch()
            }
        }

        NowPlaying {
            id: nowPlaying
            anchors.fill: parent
            compact: true
            collapseTarget: miniPlayer
            collapseCover: miniPlayer.coverItem
            z: 100
        }
    }

    // Смена цвета под новую обложку — тот же переход, что на десктопе; волна идёт от обложки
    ThemeWipe {
        id: themeWipe
        anchors.fill: parent
        source: scene
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: nowPlaying.open ? nowPlaying.coverItem : miniPlayer.coverItem
        z: 1000
        Component.onCompleted: Theme.wipe = themeWipe
    }
    Binding { target: Theme; property: "wipeDirection"; value: Player.direction }
    Binding { target: Player; property: "levelsWanted"; value: nowPlaying.wantsLevels }
    Binding {
        target: Theme
        property: "lowPower"
        value: Settings.lowPowerMode === "on" || (Settings.lowPowerMode !== "off" && win.softwareRendering)
    }
    // Приложение в фоне (экран выключен, другое приложение) — декоративные анимации стоят
    Binding {
        target: Theme; property: "calm"
        value: Qt.application.state !== Qt.ApplicationActive
    }

    // ---- Переходы и действия — как в Main.qml ----
    Connections {
        target: Catalog
        function onOpenRequested(name, props) {
            artistMenu.close()
            if (nowPlaying.open)
                nowPlaying.hide()
            const page = router.currentItem
            if (name === "search" && router.currentName === "search" && page && page.showQuery)
                page.showQuery(props.initialQuery)
            else if (name === "search")
                router.reset("search", props)
            else
                router.push(name, props)
        }
        // наведения на телефоне нет — карточка исполнителя не нужна
        function onPreviewRequested(id, x, y, w) {}
        function onPreviewCancelled() {}
        function onChooseArtist(refs) {
            artistMenu.refs = refs
            artistMenu.open(refs.map(r => ({ text: r.name, icon: "person" })), Qt.point(0, 0))
        }
        function onTrackMenuRequested(track, x, y, w, h) {
            const page = router.currentItem
            trackMenu.playlistId = !nowPlaying.open && router.currentName === "playlist" && page && page.canEdit ? page.playlistId : ""
            trackMenu.show(track, Qt.point(x, y), w > 0 && h > 0 ? Qt.size(w, h) : undefined)
        }
    }
    Connections {
        target: Library
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Player
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Devices
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    function notify(text, actionText, onAction) {
        snackbar.show(text, actionText ?? "", onAction ?? null)
    }
    function radioFrom(seedUri, title) {
        Radio.select(seedUri)
        Radio.playStation(seedUri, title)
    }
    function openSettings(section) {
        if (nowPlaying.open)
            nowPlaying.hide()
        router.reset("settings", section ? { section: section } : undefined)
    }

    function trackMenuAction(id, track, item) {
        if (id === "radio:track") {
            win.radioFrom("spotify:track:" + track.trackId, track.title ?? "")
        } else if (id.startsWith("radio:artist:")) {
            const artistId = id.slice("radio:artist:".length)
            const ref = (track.artistRefs ?? []).find(r => r.id === artistId)
            win.radioFrom("spotify:artist:" + artistId, ref ? ref.name : (track.artists ?? ""))
        } else if (id === "addqueue") {
            Player.addToQueue(track)
            snackbar.show(I18n.t("Added to queue"))
        } else if (id === "dislike") {
            Library.dislike(track)
        } else if (id === "like") {
            Library.toggleLike(track)
        } else if (id === "save") {
            const uri = track.isEpisode ? (track.uri ?? "") : "spotify:album:" + (track.albumId ?? "")
            const was = Library.isSaved(uri)
            Library.toggleSaved(uri)
            snackbar.show(was ? I18n.t("Removed from library") : I18n.t("Saved to library"))
        } else if (id === "removefromplaylist") {
            const page = router.currentItem
            if (router.currentName === "playlist" && page && typeof track.pos === "number")
                page.trackRemoved(track.pos)
        } else if (id === "repeat") {
            Player.cycleRepeat()
        } else if (id === "queue") {
            // очередь на телефоне — вкладка полноэкранного плеера
            nowPlaying.show(null)
            nowPlaying.tab = "queue"
            nowPlaying.sheetOpen = true
        } else if (id === "sound") {
            win.openSettings("playback")
        } else if (id === "playlist:new") {
            newPlaylistDialog.open(track)
        } else if (id.startsWith("playlist:")) {
            Library.addToPlaylist(track, id.slice("playlist:".length))
        } else if (id === "share:track") {
            Catalog.copyLink(Catalog.trackUrl(track))
            snackbar.show(I18n.t("Link copied"))
        } else if (id.startsWith("share:artist:")) {
            Catalog.copyLink(Catalog.artistUrl(id.slice("share:artist:".length)))
            snackbar.show(I18n.t("Link copied"))
        } else if (id === "goto:track") {
            Catalog.openAlbum(track.albumId, track.trackId)
        } else if (id === "goto:album") {
            Catalog.openAlbum(track.albumId)
        } else if (id === "goto:artist") {
            Catalog.openArtists(track.artistRefs ?? [])
        } else if (id === "goto:show") {
            Catalog.openShow(track.showId ?? "")
        }
    }

    Menu {
        id: artistMenu
        property var refs: []
        anchors.fill: parent
        z: 2000
        onPicked: i => Catalog.openArtist(refs[i].id)
    }
    TrackMenu {
        id: trackMenu
        z: 2000
        onTriggered: (id, track, item) => win.trackMenuAction(id, track, item)
    }
    NewPlaylistDialog {
        id: newPlaylistDialog
        z: 2100
    }
    ApiLimitDialog {
        z: 2120
    }
    // Обновление (Настройки → Обновления; новая версия — снекбаром с «Подробнее»)
    UpdateSheet {
        id: updateSheet
        z: 2150
    }
    Connections {
        target: Updater
        property string _told: ""
        function onStateChanged() {
            if (Updater.state === "available" && !Updater.skipped && _told !== Updater.version) {
                _told = Updater.version
                snackbar.show(I18n.t("Version {0} is available", Updater.version), I18n.t("Details"), () => Updater.showSheet(), 10000)
            }
        }
    }
    Snackbar {
        id: snackbar
        anchors.fill: parent
        // над мини-плеером и навигацией (в полноэкранном плеере — над полосой навигации системы)
        bottomMargin: nowPlaying.open ? Theme.safeBottom + 24
                    : navBar.height + (Player.hasTrack ? miniPlayer.height + 8 : 0) + 12
        z: 2200
    }
    Timer {
        interval: 2500
        running: CrashReport.pending
        onTriggered: {
            snackbar.show(I18n.t("Materialfy closed unexpectedly last time"), I18n.t("Report"), () => {
                Qt.openUrlExternally(CrashReport.issueUrl)
            }, 20000)
            CrashReport.dismiss()
        }
    }
}
