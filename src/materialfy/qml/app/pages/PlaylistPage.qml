import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Плейлист: просмотр, а у своего / совместного — правка прямо на странице (название, описание, видимость,
// обложка, порядок, удаление и добавление треков). Каждое действие сразу уходит в Spotify и отменяется
// из Snackbar («Вернуть» / «Отменить»).
TrackListPage {
    id: page

    property string playlistId
    property string playlistTitle
    property string cover

    // Library.playlistInfo — метод: перечитываем по сигналам моста
    property var info: ({})
    property int _rev: 0
    function _reload() {
        info = playlistId !== "" ? Library.playlistInfo(playlistId) : ({})
        _rev++
    }
    Connections {
        target: Library
        function onPlaylistInfoChanged(id) { if (id === page.playlistId) page._reload() }
        function onEntriesChanged() { page._reload() }
    }

    readonly property string myId: Auth.account ? (Auth.account.id ?? "") : ""
    readonly property bool canEdit: _rev >= 0 && playlistId !== "" && Library.canEdit(playlistId)
    readonly property bool own: (info.ownerId ?? "") !== "" && info.ownerId === myId
    readonly property bool collaborative: info.collaborative === true
    readonly property string uri: (info.uri ?? "") !== "" ? info.uri : (playlistId !== "" ? "spotify:playlist:" + playlistId : "")
    property bool panelOpen: false
    readonly property bool compactPanel: width < 760

    title: (info.title ?? "") !== "" ? info.title : playlistTitle
    overline: collaborative ? I18n.t("COLLABORATIVE PLAYLIST") : I18n.t("PLAYLIST")
    description: info.description ?? ""
    heroImage: (info.cover ?? "") !== "" ? info.cover : cover
    // Модель треков: стабильный экземпляр по id + счётчик владельцев у моста. playlistTracks зовём
    // один раз на открытие (guard _acquiredId — повторные оценки binding'ов счётчик не надувают),
    // releasePlaylist — на скрытие (Router прячет ушедшую страницу), смену id и уничтожение:
    // последний release отменяет идущее чтение; возврат на страницу перечитывает кэш кусками.
    // Шапка (playlistInfo) независима и не releasing'ся.
    property string _acquiredId: ""
    // Скрытие страницы (Router прячет на каждом push, сцена — в мини-режиме) снимает владение:
    // идущее чтение отменяется, но модель и строки остаются — «назад» не пересоздаёт список
    // во время анимации и сохраняет прокрутку. Модель обнуляется при смене id и уничтожении.
    function _syncModel() {
        if (playlistId === _acquiredId)
            return
        _releaseModel()
        if (playlistId !== "" && visible) {
            _acquiredId = playlistId
            model = Library.playlistTracks(playlistId)
        }
    }
    function _releaseOwnership() {
        if (_acquiredId === "")
            return
        Library.releasePlaylist(_acquiredId)
        _acquiredId = ""
    }
    function _releaseModel() {
        _releaseOwnership()
        model = null
    }
    Component.onCompleted: { _reload(); _syncModel() }
    onPlaylistIdChanged: { _reload(); _syncModel() }
    onVisibleChanged: {
        if (visible)
            _syncModel()
        else
            _releaseOwnership()
    }
    Component.onDestruction: _releaseModel()
    emptyIcon: "queue_music"
    emptyTitle: I18n.t("This playlist has no tracks")
    emptyText: canEdit ? I18n.t("Tap “Edit playlist” to add tracks") : ""
    addedColumn: collaborative ? "by" : (model && model.count > 0 && (model.get(0).addedAt ?? "") !== "" ? "at" : "")
    contentRightInset: editing && panelOpen && !compactPanel ? addPanel.panelWidth + 8 : 0
    onEditingChanged: if (!editing) panelOpen = false
    // «Назад» системы (телефон): панель добавления → правка → поиск по списку
    function handleBack() {
        if (panelOpen) {
            panelOpen = false
            return true
        }
        if (editing) {
            editing = false
            return true
        }
        if (searchOpen) {
            closeSearch()
            return true
        }
        return false
    }
    onCanEditChanged: if (!canEdit) editing = false

    function notify(text, actionText, onAction) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, actionText ?? "", onAction ?? null)
    }

    // ---- Удаление с «Вернуть»: удалённые за один показ Snackbar возвращаются вместе ----
    // Web API снимает все вхождения uri, поэтому одно удаление — группа [{uri, pos}] по возрастанию pos
    property var _removed: []               // группы в порядке удаления
    Timer { id: undoWindow; interval: 5000; onTriggered: page._removed = [] }
    onTrackRemoved: row => {
        const t = model.get(row)
        if (!t || !t.uri)
            return
        const group = []
        for (let i = 0; i < model.count; i++) {
            if (i === row || model.get(i).uri === t.uri)
                group.push({ uri: t.uri, pos: i })
        }
        Library.removeTracks(playlistId, [{ uri: t.uri, pos: row }])
        _removed = _removed.concat([group])
        undoWindow.restart()
        const n = _removed.reduce((a, g) => a + g.length, 0)
        notify(n > 1 ? I18n.n(n, "Removed {0} track", "Removed {0} tracks") : I18n.t("Removed “{0}” from playlist", t.title ?? ""), I18n.t("Undo"), () => page._restore())
    }
    function _restore() {
        const list = _removed
        _removed = []
        undoWindow.stop()
        // группы — в обратном порядке удаления, внутри группы — по возрастанию pos, подряд идущие одним вызовом
        let first = -1
        for (let g = list.length - 1; g >= 0; g--) {
            const group = list[g]
            for (let i = 0; i < group.length;) {
                let j = i + 1
                while (j < group.length && group[j].pos === group[j - 1].pos + 1)
                    j++
                Library.insertTracks(playlistId, group.slice(i, j).map(x => x.uri), group[i].pos)
                first = first < 0 ? group[i].pos : Math.min(first, group[i].pos)
                i = j
            }
        }
        if (first >= 0)
            _flashAfterChange(first)
    }

    // мигнуть строкой, когда модель обновится (добавление/возврат приходят из сервиса)
    property int _pendingFlash: -1
    function _flashAfterChange(row) {
        _pendingFlash = row
        flashFallback.restart()
    }
    Timer { id: flashFallback; interval: 1200; onTriggered: page._pendingFlash = -1 }
    Connections {
        target: page.model
        ignoreUnknownSignals: true
        enabled: page._pendingFlash >= 0
        function onCountChanged() { const r = page._pendingFlash; page._pendingFlash = -1; Qt.callLater(() => page.flashRow(Math.min(r, page.model.count - 1))) }
        function onModelReset() { const r = page._pendingFlash; page._pendingFlash = -1; Qt.callLater(() => page.flashRow(Math.min(r, page.model.count - 1))) }
    }

    onMoveRequested: (from, to) => {
        const t = model.get(from)
        Library.moveTracks(playlistId, from, to)
        notify(I18n.t("“{0}” → position {1}", t ? t.title ?? "" : "", to + 1), I18n.t("Revert"), () => Library.moveTracks(page.playlistId, to, from))
    }

    // ---- Шапка: владелец, «N треков, длительность», видимость ----
    heroMeta: Flow {
        spacing: 8
        RowLayout {
            visible: (page.info.owner ?? "") !== ""
            spacing: 8
            height: 28
            Avatar { size: 24; name: page.info.owner ?? "" }
            Label {
                text: (page.info.owner ?? "") + (page.own ? I18n.t(" (you)") : "")
                type: "titleSmall"
            }
        }
        Label {
            height: 28
            verticalAlignment: Text.AlignVCenter
            visible: page.model !== null && page.model.count > 0
            text: !visible ? "" : "·  " + I18n.n(page.model.count, "{0} track", "{0} tracks")
                  + (page.model.totalDurationMs > 0 ? ", " + page._duration(page.model.totalDurationMs) : "")
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
        Rectangle {
            visible: page.own
            implicitWidth: visRow.implicitWidth + 16
            implicitHeight: 28
            radius: 8
            color: Theme.surfaceContainerHigh
            Row {
                id: visRow
                anchors.centerIn: parent
                spacing: 4
                Icon { name: page.info.public === true ? "public" : "lock"; size: 16; anchors.verticalCenter: parent.verticalCenter }
                Label { text: page.info.public === true ? I18n.t("Public") : I18n.t("Private"); type: "labelMedium"; color: Theme.fgSurfaceVariant; anchors.verticalCenter: parent.verticalCenter }
            }
        }
        Rectangle {
            visible: page.canEdit && !page.own
            implicitWidth: editRow.implicitWidth + 16
            implicitHeight: 28
            radius: 8
            color: Qt.alpha(Theme.primary, 0.12)
            Row {
                id: editRow
                anchors.centerIn: parent
                spacing: 4
                Icon { name: "edit"; size: 16; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
                Label { text: I18n.t("You can edit"); type: "labelMedium"; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
            }
        }
    }

    // телефон: радио и «поделиться» — в меню «ещё», кнопки поменьше: иначе ряд шапки переносился на две строки
    headerExtra: Row {
        spacing: 4
        IconButton {
            visible: !Theme.mobile
            icon: "radio"
            size: "m"
            // станции идут через сессию воспроизведения (как «Радио исполнителя»)
            enabled: page.uri !== "" && !Radio.locked
            onClicked: {
                Radio.playStation(page.uri, page.title)
                page.notify(I18n.t("Playlist radio: “{0}”", page.title))
            }
        }
        IconButton {
            visible: page.canEdit
            icon: "edit"
            size: Theme.mobile ? "s" : "m"
            onClicked: page.editing = true
        }
        IconButton {
            visible: !page.canEdit && !page.own && page.playlistId !== ""
            anchors.verticalCenter: parent.verticalCenter
            readonly property bool followed: page.info.followed === true
            Accessible.name: followed ? I18n.t("Following") : I18n.t("Follow")
            icon: followed ? "library_add_check" : "library_add"
            style: followed ? "tonal" : "standard"
            size: Theme.mobile ? "s" : "m"
            checkable: true
            autoToggle: false
            checked: followed
            onClicked: {
                if (followed) {
                    Library.unfollowPlaylist(page.playlistId)
                    page.notify(I18n.t("Unfollowed “{0}”", page.title), I18n.t("Undo"), () => Library.followPlaylist(page.playlistId))
                } else {
                    Library.followPlaylist(page.playlistId)
                }
            }
        }
        IconButton {
            visible: !Theme.mobile
            icon: "share"
            size: "m"
            enabled: page.uri !== ""
            onClicked: {
                Catalog.copyLink(Catalog.uriUrl(page.uri))
                page.notify(I18n.t("Link copied"))
            }
        }
        IconButton {
            id: moreButton
            icon: Theme.mobile ? "more_vert" : "more_horiz"
            size: Theme.mobile ? "s" : "m"
            onClicked: {
                const p = moreButton.mapToItem(plMenu, 0, moreButton.height)
                plMenu.open(page._menuItems(), p)
            }
        }
    }

    heroNote: page.collaborative ? collabNote : null
    Component {
        id: collabNote
        Rectangle {
            implicitHeight: noteRow.implicitHeight + 24
            radius: Theme.shape.large
            color: Theme.surfaceContainerHigh
            RowLayout {
                id: noteRow
                x: 16
                y: 12
                width: parent.width - 32
                spacing: 12
                Icon { Layout.alignment: Qt.AlignTop; name: "group"; size: 20; color: Theme.primary }
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Collaborators can only be changed in the Spotify app. Here you can see who added what.")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                }
            }
        }
    }

    editHero: PlaylistEditHeader {
        playlistId: page.playlistId
        info: page.info
        cover: page.heroImage
        own: page.own
        panelOpen: page.panelOpen
        pageWidth: page.width - page.contentRightInset
        onDone: page.editing = false
        onAddTracks: { page.panelOpen = !page.panelOpen; if (page.panelOpen) Qt.callLater(addPanel.focusSearch) }
        onNotify: text => page.notify(text)
    }

    footerExtra: page.editing ? addRow : null
    Component {
        id: addRow
        Item {
            width: parent ? parent.width : 0
            height: 72
            Rectangle {
                anchors.fill: parent
                anchors.topMargin: 8
                radius: Theme.shape.large
                color: "transparent"
                border.width: 1
                border.color: Theme.outlineVariant
                Row {
                    anchors.centerIn: parent
                    spacing: 8
                    Icon { name: "add"; size: 20; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
                    Label { text: I18n.t("Add tracks"); type: "labelLarge"; color: Theme.primary; anchors.verticalCenter: parent.verticalCenter }
                }
                StateLayer {
                    anchors.fill: parent
                    radius: Theme.shape.large
                    color: Theme.primary
                    onClicked: { page.panelOpen = true; Qt.callLater(addPanel.focusSearch) }
                }
            }
        }
    }

    // ---- Меню «⋯» ----
    function _menuItems() {
        const items = Theme.mobile ? [
            { id: "radio", text: I18n.t("Playlist radio"), icon: "radio", enabled: page.uri !== "" && !Radio.locked },
            { id: "share", text: I18n.t("Copy link"), icon: "share", enabled: page.uri !== "" },
            { separator: true }
        ] : []
        items.push(
            { id: "queue", text: I18n.t("Add to queue"), icon: "add_to_queue", enabled: page.uri !== "" },
            { id: "pin", text: (Library.pinsRevision >= 0 && Library.isPinned(page.uri)) ? I18n.t("Unpin") : I18n.t("Pin in library"),
              icon: "push_pin" })
        if (page.canEdit)
            items.push({ id: "edit", text: I18n.t("Edit playlist"), icon: "edit" })
        if (page.own)
            items.push({ separator: true }, { id: "delete", text: I18n.t("Delete playlist"), icon: "delete" })
        else if (page.info.followed === true)
            items.push({ separator: true }, { id: "unfollow", text: I18n.t("Unfollow"), icon: "remove_circle" })
        return items
    }
    Menu {
        id: plMenu
        anchors.fill: parent
        z: 50
        header: ({ title: page.title, subtitle: I18n.t("Playlist"), image: page.heroImage })
        onTriggered: (id, item) => {
            if (id === "radio") {
                Radio.playStation(page.uri, page.title)
                page.notify(I18n.t("Playlist radio: “{0}”", page.title))
            } else if (id === "share") {
                Catalog.copyLink(Catalog.uriUrl(page.uri))
                page.notify(I18n.t("Link copied"))
            } else if (id === "queue") {
                Player.addUriToQueue(page.uri)
                page.notify(I18n.t("Playlist added to queue"))
            } else if (id === "pin") {
                const was = Library.isPinned(page.uri)
                Library.togglePin(page.uri)
                page.notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
            } else if (id === "edit") {
                page.editing = true
            } else if (id === "delete" || id === "unfollow") {
                Library.unfollowPlaylist(page.playlistId)
                page.notify(id === "delete" ? I18n.t("Playlist deleted") : I18n.t("Unfollowed “{0}”", page.title),
                            I18n.t("Undo"), () => Library.followPlaylist(page.playlistId))
                if (id === "delete" && page.router)
                    page.router.back()
            }
        }
    }

    AddTracksPanel {
        id: addPanel
        // телефон: лист поверх всего окна (над мини-плеером и нижней навигацией)
        parent: Theme.mobile && page.Window.window ? page.Window.window.contentItem : page
        anchors.fill: parent
        z: Theme.mobile ? 1500 : 20
        playlistId: page.playlistId
        playlistTitle: page.title
        playlistModel: page.model
        opened: page.editing && page.panelOpen
        compact: page.compactPanel
        onCloseRequested: page.panelOpen = false
        onAdded: track => {
            const pos = page.model ? page.model.count : 0
            page._flashAfterChange(pos)
            page.notify(I18n.t("Added “{0}” to the end", track.title ?? ""), I18n.t("Revert"),
                        () => Library.removeTracks(page.playlistId, [{ uri: track.uri, pos: pos }]))
        }
    }
}
