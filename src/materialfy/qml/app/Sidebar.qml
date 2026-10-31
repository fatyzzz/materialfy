import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Левая навигация. compact — рейл из иконок (узкое окно или свёрнуто вручную).
//
// Геометрия одна для обоих режимов: поля 12 px, все иконки (знак, разделы, обложки плейлистов, аватар)
// центрированы на оси x = 40 — это центр рейла шириной 80. При сворачивании анимируется только ширина
// (её ведёт Main.qml), подписи гаснут, высоты не меняются — поэтому ничего не прыгает.
Item {
    id: root

    required property var router
    property bool compact: false
    readonly property bool signedIn: Auth.state === "signedIn"
    signal newPlaylistRequested()
    // Шторка (узкое окно): в раскладке ширины не занимает, по openDrawer() выезжает поверх контента
    property bool drawer: false
    // Открыта поверх контента: шторка (drawer) или раскрытый рейл, когда рейл навязан шириной окна
    property bool drawerOpen: false
    // Рейл из-за ширины окна (а не свёрнут вручную): кнопка меню раскрывает панель поверх контента
    readonly property bool forcedRail: compact && !drawer && !Settings.sidebarCollapsed
    // Закрывается анимацией — до её конца карточка шторки ещё видна
    property bool closing: false
    // Переход в шторку: место в раскладке сжимается анимацией (Main.qml), а рейл уезжает влево вместе с ним
    readonly property bool collapsing: drawer && !drawerOpen && !closing && width > 0.5
    // Панель поверх контента (карточка шторки, затемнение, своя ширина 264)
    readonly property bool floating: drawer ? !collapsing : (drawerOpen || closing)
    readonly property bool rail: (compact && !drawer && !drawerOpen) || collapsing

    function openDrawer() { closeTimer.stop(); closing = false; drawerOpen = true; panel.forceActiveFocus() }
    function closeDrawer() {
        if (!drawerOpen)
            return
        closing = true
        drawerOpen = false
        closeTimer.restart()
    }
    function go(name, props) { closeDrawer(); router.reset(name, props) }
    // Меню строки (ПКМ): закрепить/открепить в «Библиотеке». Закреплённые — первыми в списках.
    property var _menuEntry: null
    function openRowMenu(entry, point) {
        _menuEntry = entry
        const uri = entry.uri ?? ""
        if (uri === "" || entry.kind === "folder")
            return
        const pinned = Library.pinsRevision >= 0 && Library.isPinned(uri)
        rowMenu.open([{ id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin to top"), icon: "push_pin" }],
                     rowMenu.mapFromItem(null, point.x, point.y))
    }
    function _menuAction(id) {
        const entry = _menuEntry
        if (!entry || id !== "pin")
            return
        const uri = entry.uri ?? ""
        const was = Library.isPinned(uri)
        Library.togglePin(uri)
        const w = root.Window.window
        if (w && typeof w.notify === "function")
            w.notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
    }

    // Строка боковой панели: плейлист/папка — страницей приложения, остальное — карточкой каталога
    function openEntry(entry) {
        const kind = entry.kind ?? ""
        closeDrawer()
        if (kind === "folder")
            router.reset("folder", { folderId: entry.id, folderTitle: entry.title ?? "" })
        else if (kind === "liked")
            router.reset("liked")
        else if (kind === "playlist")
            router.reset("playlist", { playlistId: entry.id, playlistTitle: entry.title ?? "", cover: entry.cover ?? "" })
        else if (kind === "album")
            Catalog.openAlbum(entry.id)
        else if (kind === "artist")
            Catalog.openArtist(entry.id)
        else if (kind === "show")
            Catalog.openShow(entry.id)
    }
    onDrawerChanged: { drawerOpen = false; closing = false }
    onCompactChanged: if (!compact && !drawer) { drawerOpen = false; closing = false }
    Timer { id: closeTimer; interval: Theme.motion.spatialDefault + 40; onTriggered: root.closing = false }

    readonly property real expandedWidth: 264
    readonly property real railWidth: 80
    readonly property real margin: 12
    readonly property real iconInset: 14     // квадрат иконки 28 px: 12 + 14 + 14 = 40 — ось рейла
    readonly property real labelWidth: expandedWidth - 2 * margin - iconInset - 28 - 8 - 12

    readonly property var sections: [
        { name: "home", icon: "home", text: I18n.t("Home") },
        { name: "search", icon: "search", text: I18n.t("Search") },
        { name: "releases", icon: "new_releases", text: I18n.t("New releases") },
        { name: "library", icon: "library_music", text: I18n.t("Library") },
        { name: "liked", icon: "favorite", text: I18n.t("Liked Songs") }
    ]

    // Плейлисты и папки верхнего уровня rootlist (порядок Spotify; закреплённые — первыми, их ставит сервис).
    // Раскрытая папка (стрелка справа) показывает свои плейлисты и вложенные папки прямо в списке, с отступом;
    // клик по самой папке открывает её страницей (FolderPage). Раскрытые папки помнятся, пока открыто приложение.
    // Режим «Библиотека»: вместо дерева плейлистов — последние добавленные/обновлённые строки всех
    // разделов (плейлисты, альбомы, исполнители, подкасты и «Любимые»). Папок в этой сводке нет.
    readonly property bool libraryMode: Settings.sidebarLibrary === true
    readonly property var _recent: signedIn && libraryMode ? Library.recentEntries(40) : null
    readonly property var _source: signedIn && !libraryMode ? Library.items("playlists", "all", "spotify") : null
    property var playlists: []
    property var recentItems: []
    property var expandedFolders: ({})      // id папки → true
    readonly property int indentStep: 16
    readonly property int maxDepth: 4         // глубже — без доп. отступа (подпись не должна исчезнуть)
    function toggleFolder(id) {
        const next = Object.assign({}, expandedFolders)
        if (next[id])
            delete next[id]
        else
            next[id] = true
        expandedFolders = next
        _rebuild()
    }
    function _rebuild() {
        if (libraryMode) {
            const rows = []
            const recent = _recent
            for (let i = 0; recent && i < recent.count; i++)
                rows.push(Object.assign({}, recent.get(i), { depth: 0 }))
            recentItems = rows
            return
        }
        const out = []
        const m = _source
        // «Любимые» — отдельным пунктом выше; дети папок — внутри папки (если папки в списке есть)
        const folders = new Set()
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "folder")
                folders.add(e.id)
        }
        const seen = new Set()
        // в рейле вложенных строк нет: иконки держат одну ось
        const addChildren = (id, depth) => {
            if (root.rail || !root.expandedFolders[id] || seen.has(id))
                return
            seen.add(id)
            const children = Library.folderChildren(id)
            for (let i = 0; children && i < children.count; i++) {
                const c = Object.assign({}, children.get(i), { depth: depth })
                out.push(c)
                if (c.kind === "folder")
                    addChildren(c.id, depth + 1)
            }
        }
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "liked" || folders.has(e.folderId ?? ""))
                continue
            out.push(Object.assign({}, e, { depth: 0 }))
            if (e.kind === "folder")
                addChildren(e.id, 1)
        }
        playlists = out
    }
    on_SourceChanged: _rebuild()
    onRailChanged: _rebuild()
    Connections {
        target: root._source
        ignoreUnknownSignals: true
        function onCountChanged() { root._rebuild() }
        function onModelReset() { root._rebuild() }
        function onDataChanged() { root._rebuild() }
    }
    Connections {
        target: root._recent
        ignoreUnknownSignals: true
        function onCountChanged() { root._rebuild() }
        function onModelReset() { root._rebuild() }
        function onDataChanged() { root._rebuild() }
    }
    // режим переключили в настройках — список меняется целиком
    onLibraryModeChanged: _rebuild()
    // содержимое папок меняется вместе с деревом (обложки, названия, перенос в Spotify)
    Connections {
        target: Library
        function onEntriesChanged() {
            if (root.libraryMode || Object.keys(root.expandedFolders).length > 0)
                root._rebuild()
        }
        // закрепили/открепили строку — сводка переставляется
        function onPinsChanged() { if (root.libraryMode) root._rebuild() }
    }

    implicitWidth: drawer ? 0 : compact ? railWidth : expandedWidth
    clip: !drawer && !floating
    z: drawer || floating ? 50 : 0

    component FadeLabel: Label {
        opacity: root.rail ? 0 : 1
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    // Затемнение под шторкой — на всё окно (координаты пересчитываются при открытии)
    Rectangle {
        id: scrim
        property point origin: Qt.point(0, 0)
        visible: opacity > 0
        x: -origin.x
        y: -origin.y
        width: root.Window.width
        height: root.Window.height
        color: Theme.scrim
        opacity: root.drawerOpen ? 0.32 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onClicked: root.closeDrawer(); onWheel: w => w.accepted = true }
    }
    onDrawerOpenChanged: if (drawerOpen) scrim.origin = root.mapToItem(null, 0, 0)

    Item {
        id: panel
        width: root.drawer ? (root.collapsing ? root.railWidth : root.expandedWidth)
             : root.drawerOpen ? root.expandedWidth : root.width
        height: root.height
        x: !root.drawer ? 0 : root.drawerOpen ? 0 : root.collapsing ? root.width - root.railWidth : -width - 24
        visible: !root.drawer || root.drawerOpen || root.collapsing || root.closing
        clip: root.floating && !root.drawer
        // шторка выезжает слева; раскрытый рейл растёт вширь из рейла (MD3: rail → drawer)
        Behavior on x {
            enabled: root.drawer && (root.drawerOpen || root.closing)
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
        }
        Behavior on width {
            enabled: !root.drawer && root.compact
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
        Keys.onEscapePressed: root.closeDrawer()

        // Пустые места панели (вокруг логотипа, между пунктами, под плейлистами) тянут окно, как верхняя
        // панель; двойной клик — развернуть. Пункты, прокрутка и карточка аккаунта забирают нажатие сами.
        DragHandler {
            property bool manual: false
            target: null
            grabPermissions: PointerHandler.TakeOverForbidden
            onActiveChanged: if (active) manual = !root.Window.window.startSystemMove()
            onCentroidChanged: if (active && manual && root.Window.window.visibility === Window.Windowed) {
                root.Window.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
                root.Window.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
            }
        }
        TapHandler {
            acceptedButtons: Qt.LeftButton
            onDoubleTapped: {
                const w = root.Window.window
                w.visibility = w.visibility === Window.Maximized ? Window.Windowed : Window.Maximized
            }
        }

        // Карточка шторки: скруглён правый край (MD3 modal navigation drawer)
        Rectangle {
            id: drawerBg
            visible: root.floating
            anchors.fill: parent
            anchors.leftMargin: -Theme.shape.extraLarge
            radius: Theme.shape.extraLarge
            color: Theme.surfaceContainerLow
        }
        Elevation { target: drawerBg; level: 1 }
        MouseArea { visible: root.floating; anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: w => w.accepted = true }

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 12
        anchors.bottomMargin: 12
        anchors.leftMargin: root.margin
        anchors.rightMargin: root.margin
        spacing: 2

        // Шапка: знак «Волна-M» на оси рейла + надпись. Знак — переключатель свернуть / развернуть
        // (наведение превращает его в кнопку); в узком окне — раскрыть панель поверх контента.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            Layout.bottomMargin: 8

            SidebarLogo {
                objectName: "sidebarLogo"
                x: 40 - root.margin - markSize / 2
                anchors.verticalCenter: parent.verticalCenter
                markSize: 40
                expanded: !root.rail
                onActivated: {
                    if (root.drawer || root.drawerOpen)
                        root.closeDrawer()
                    else if (root.forcedRail)
                        root.openDrawer()
                    else
                        Settings.sidebarCollapsed = !Settings.sidebarCollapsed
                }
            }
        }

        Repeater {
            model: root.sections
            NavItem {
                required property var modelData
                Layout.fillWidth: true
                compact: root.rail
                iconInset: root.iconInset
                labelWidth: root.labelWidth
                icon: modelData.icon
                text: modelData.text
                // до входа разделы неактивны (38 %): смотреть в них нечего; «Главная» — с приглашением войти
                enabled: root.signedIn || modelData.name === "home"
                opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                // «Библиотека» не подсвечена, пока внутри открыта папка — её подсвечивает пункт папки
                selected: root.router.rootName === modelData.name
                          && !(modelData.name === "library" && (root.router.current.props.folderId ?? "") !== "")
                onClicked: root.go(modelData.name)
            }
        }

        // Заголовок секции: подпись гаснет, волнистый разделитель остаётся и в рейле
        Item {
            id: sectionHeader
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.preferredHeight: 32

            FadeLabel {
                id: sectionLabel
                x: root.iconInset
                anchors.verticalCenter: parent.verticalCenter
                text: root.libraryMode ? I18n.t("Recently added") : I18n.t("Playlists")
                type: "labelMedium"
                color: Theme.fgSurfaceVariant
            }
            WavyDivider {
                // в режиме «Библиотека» кнопки «+» нет — линия идёт до самого края
                x: root.rail ? root.iconInset : sectionLabel.x + sectionLabel.implicitWidth + 10
                width: parent.width - x
                anchors.verticalCenter: parent.verticalCenter
                Behavior on x { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            }
            IconButton {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                icon: "add"
                size: "xs"
                iconColor: Theme.primary
                enabled: root.signedIn
                onClicked: root.newPlaylistRequested()
                // в режиме «Библиотека» список не плейлистов: кнопка «новый плейлист» тут не к месту
                opacity: root.rail || root.libraryMode ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            }
        }

        FadeLabel {
            visible: opacity > 0 && !root.signedIn
            Layout.preferredWidth: root.labelWidth + 28 + 8
            Layout.leftMargin: root.iconInset
            Layout.topMargin: 4
            text: root.libraryMode ? I18n.t("Sign in to see your library") : I18n.t("Sign in to see your playlists")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
        FadeLabel {
            visible: opacity > 0 && root.signedIn && !root.libraryMode
                     && root._source !== null && !root._source.loading && root.playlists.length === 0
            Layout.preferredWidth: root.labelWidth + 28 + 8
            Layout.leftMargin: root.iconInset
            Layout.topMargin: 4
            text: I18n.t("No playlists yet. Create one with +")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
        LoadingIndicator {
            visible: root.signedIn && !root.libraryMode
                     && root._source !== null && root._source.loading && root.playlists.length === 0
            Layout.leftMargin: 40 - root.margin - size / 2
            Layout.topMargin: 8
            size: 32
        }

        // Плейлисты пользователя (или сводка «Библиотеки»): своя прокрутка, если строк много
        ListView {
            id: playlistList
            visible: root.signedIn && count > 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.topMargin: 4
            clip: true
            spacing: 2
            boundsBehavior: Flickable.StopAtBounds
            acceptedButtons: Qt.NoButton
            model: root.libraryMode ? root.recentItems : root.playlists
            delegate: NavItem {
                id: row
                required property var modelData
                readonly property string kind: modelData.kind ?? ""
                readonly property bool folder: kind === "folder"
                readonly property bool expanded: folder && root.expandedFolders[modelData.id] === true
                readonly property real indent: Math.min(modelData.depth ?? 0, root.maxDepth) * root.indentStep
                width: ListView.view.width
                compact: root.rail
                iconInset: root.iconInset + indent
                labelWidth: root.labelWidth - indent - (folder ? 28 : 0)
                text: modelData.title ?? ""
                // в сводке у каждой строки свой значок: обложки у альбомов и подкастов тоже есть
                icon: folder ? (expanded ? "folder_open" : "folder")
                      : kind === "album" ? "album"
                      : kind === "artist" ? "person"
                      : kind === "show" ? "podcasts"
                      : kind === "liked" ? "favorite" : "queue_music"
                imageSource: folder ? "" : (modelData.cover ?? "")
                // у папки справа — своя кнопка-стрелка (ниже); закреплённый — булавка
                trailingIcon: !folder && modelData.pinned === true ? "push_pin" : ""
                trailingColor: Theme.primary
                selected: folder ? root.router.currentName === "folder" && (root.router.current.props.folderId ?? "") === modelData.id
                          : kind === "playlist"
                            ? root.router.currentName === "playlist" && (root.router.current.props.playlistId ?? "") === modelData.id
                          : kind === "album" ? root.router.currentName === "album" && (root.router.current.props.albumId ?? "") === modelData.id
                          : kind === "artist" ? root.router.currentName === "artist" && (root.router.current.props.artistId ?? "") === modelData.id
                          : kind === "show" ? root.router.currentName === "show" && (root.router.current.props.showId ?? "") === modelData.id
                          : kind === "liked" ? root.router.rootName === "liked" : false
                onClicked: root.openEntry(modelData)
                onRightClicked: point => root.openRowMenu(modelData, point)

                // Стрелка папки: раскрыть/свернуть прямо в списке (поворот на 90° — MD3 expand)
                IconButton {
                    visible: row.folder && opacity > 0
                    opacity: root.rail ? 0 : 1
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                    anchors.right: parent.right
                    anchors.rightMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    z: 2
                    size: "xs"
                    icon: "chevron_right"
                    iconColor: Theme.fgSurfaceVariant
                    rotation: row.expanded ? 90 : 0
                    Behavior on rotation { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
                    onClicked: root.toggleFolder(row.modelData.id)
                    Accessible.name: row.expanded ? I18n.t("Collapse folder") : I18n.t("Expand folder")
                }
            }
            SmoothScroll { flickable: playlistList; wheelStep: Settings.wheelStep; showScrollBar: false }
        }

        Item { Layout.fillHeight: true; visible: !playlistList.visible }

        // Доступное обновление и ход загрузки (окно обновления — UpdateSheet в Main.qml)
        UpdatePill {
            Layout.fillWidth: true
            rail: root.rail
            onClicked: { root.closeDrawer(); Updater.showSheet() }
        }

        // В рейле настройки — отдельным пунктом; в развёрнутом виде — кнопкой в карточке аккаунта
        NavItem {
            visible: root.rail
            Layout.fillWidth: true
            Layout.topMargin: 8
            compact: root.rail
            iconInset: root.iconInset
            labelWidth: root.labelWidth
            icon: "settings"
            text: I18n.t("Settings")
            selected: root.router.rootName === "settings"
            onClicked: root.go("settings")
        }

        // Карточка аккаунта: аватар на оси рейла, одна высота в обоих режимах
        Item {
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.preferredHeight: 64

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.large
                color: root.router.rootName === "account" || !root.signedIn ? Theme.secondaryContainer : Theme.surfaceContainer
                Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
            }

            Avatar {
                id: avatar
                x: 40 - root.margin - size / 2
                anchors.verticalCenter: parent.verticalCenter
                size: 40
                name: root.signedIn ? (Auth.account.displayName ?? "") : ""
                source: root.signedIn ? (Auth.account.avatar ?? "") : ""
            }

            Column {
                x: avatar.x + avatar.width + 12
                anchors.verticalCenter: parent.verticalCenter
                width: root.expandedWidth - 2 * root.margin - x - 12 - settingsButton.width
                opacity: root.rail ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                Label {
                    width: parent.width
                    text: root.signedIn ? (Auth.account.displayName ?? "") : I18n.t("Sign in")
                    type: "titleSmall"
                    color: root.signedIn ? Theme.fgSurface : Theme.fgSecondaryContainer
                }
                Label {
                    width: parent.width
                    text: root.signedIn ? (Auth.playbackState === "authorizing" ? I18n.t("Waiting for browser…")
                                          : Auth.premium ? "Premium" : "Spotify Free")
                        : Auth.state === "awaitingBrowser" ? I18n.t("Waiting for browser…")
                        : Auth.state === "checking" || Auth.state === "connecting" ? I18n.t("Signing in…")
                        : I18n.t("Spotify account")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }
            }

            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.large
                onClicked: root.go("account")
            }

            IconButton {
                id: settingsButton
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                icon: "settings"
                checkable: true
                autoToggle: false
                checked: root.router.rootName === "settings"
                opacity: root.rail ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: root.go("settings")
            }
        }
    }
    }

    // ПКМ по строке: закрепить/открепить
    Menu {
        id: rowMenu
        anchors.fill: parent
        z: 60
        onTriggered: (id, item) => root._menuAction(id)
    }
}
