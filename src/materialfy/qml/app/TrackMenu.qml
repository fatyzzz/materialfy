import QtQuick
import Md3
import Materialfy.Core

// Меню трека (ПКМ или «⋯»): радио, очередь, плейлисты, ссылки, переходы.
// Один экземпляр на окно: show(track, point); действия разбирает владелец через triggered(id, track, item).
// playlistId — свой плейлист, открытый на странице (пункт «Убрать из этого плейлиста», нужен track.pos).
// В мини-плеере сверху — то, чего в нём нет (перемешивание, повтор, поверх окон, развернуть);
// эти пункты меню выполняет само, как и «О треке» (окно TrackInfoDialog живёт здесь же).
Item {
    id: root

    property var track: ({})
    property string playlistId
    readonly property bool opened: menu.opened

    signal triggered(string id, var track, var item)

    readonly property string _id: track.trackId ?? ""
    readonly property bool _episode: track.isEpisode === true
    readonly property bool _isCurrent: Player.hasTrack && _id !== "" && Player.trackId === _id
    readonly property var _refs: (track.artistRefs ?? []).filter(r => r && r.id)
    readonly property var _win: root.Window.window
    readonly property bool _mini: !!_win && _win.layoutMode === "mini"

    // size — размер кнопки «⋯» с левым верхним углом в point (меню встаёт у неё); без него — у точки
    function show(t, point, size) {
        track = t ?? ({})
        if (_id === "" && (track.uri ?? "") === "" && !_mini)
            return
        menu.open(_items(), point, size)
    }
    function close() { menu.close() }
    // «назад» системы (телефон): true — меню было открыто и закрылось (или вернулось из подменю)
    function back() { return infoDialog.visible ? (infoDialog.close(), true) : menu.back() }

    function _playlistItems() {
        const out = [{ id: "playlist:new", text: I18n.t("New playlist…"), icon: "add" }]
        const model = Library.playlists
        const me = Auth.account ? (Auth.account.id ?? "") : ""
        const own = []
        for (let i = 0; model && i < model.count; i++) {
            const p = model.get(i)
            if (!((me !== "" && p.ownerId === me) || p.canEdit === true))
                continue
            own.push({ id: "playlist:" + p.playlistId, text: p.title, icon: "queue_music" })
        }
        if (own.length > 0)
            out.push({ separator: true })
        return out.concat(own)
    }

    function _shareItems() {
        const out = [{ id: "share:track", text: _episode ? I18n.t("Copy episode link") : I18n.t("Copy track link"), icon: "content_copy" }]
        if (_refs.length === 1)
            out.push({ id: "share:artist:" + _refs[0].id, text: I18n.t("Copy artist link"), icon: "person" })
        else
            for (const r of _refs)
                out.push({ id: "share:artist:" + r.id, text: I18n.t("Link to {0}", r.name), icon: "person" })
        return out
    }

    function _radioItems() {
        if (_episode)
            return []
        const out = [{ id: "radio:track", text: I18n.t("Track radio"), icon: "radio" }]
        if (_refs.length === 1)
            out.push({ id: "radio:artist:" + _refs[0].id, text: I18n.t("Artist radio"), icon: "person" })
        else if (_refs.length > 1)
            out.push({ id: "radio:artists", text: I18n.t("Artist radio"), icon: "person",
                       submenu: _refs.map(r => ({ id: "radio:artist:" + r.id, text: r.name, icon: "person" })) })
        return out
    }

    function _miniItems() {
        const onTop = _win && "pinnedOnTop" in _win && _win.canPin !== false
        const items = [
            { id: "mini:shuffle", text: I18n.t("Shuffle playback"), icon: "shuffle", checked: Player.shuffle,
              enabled: Player.hasTrack && Player.source !== "radio" },
            { id: "mini:repeat", text: I18n.t("Repeat"), icon: Player.repeat === "one" ? "repeat_one" : "repeat",
              checked: Player.repeat !== "off", enabled: Player.hasTrack }
        ]
        if (onTop)
            items.push({ id: "mini:ontop", text: I18n.t("Always on top"), icon: "push_pin", checked: !!_win.pinnedOnTop })
        if (_win && typeof _win.expandFromMini === "function")
            items.push({ id: "mini:expand", text: I18n.t("Expand"), icon: "open_in_full" })
        return items
    }

    function _miniAction(id) {
        if (id === "mini:shuffle")
            Player.shuffle = !Player.shuffle
        else if (id === "mini:repeat")
            Player.cycleRepeat()
        else if (id === "mini:ontop" && _win && "pinnedOnTop" in _win)
            _win.pinnedOnTop = !_win.pinnedOnTop
        else if (id === "mini:expand" && _win && typeof _win.expandFromMini === "function")
            _win.expandFromMini()
    }

    // Телефон: сначала то, ради чего меню открывают чаще всего (лайк, плейлист, очередь), затем переходы,
    // радио и редкое — в конце. Лист на весь экран: важное не должно уходить за край
    function _mobileItems() {
        const hasAlbum = (track.albumId ?? "") !== ""
        const liked = Library.likesRevision >= 0 && _id !== "" && Library.isLiked(_id)
        const items = []
        if (!_episode)
            items.push({ id: "like", text: liked ? I18n.t("Remove from Liked Songs") : I18n.t("Save to Liked Songs"), icon: "favorite" },
                       { id: "playlist", text: I18n.t("Add to playlist"), icon: "playlist_add", submenu: _playlistItems() })
        items.push({ id: "addqueue", text: I18n.t("Add to queue"), icon: "add_to_queue", enabled: (track.uri ?? "") !== "" })
        if (!_episode && root.playlistId !== "" && typeof track.pos === "number")
            items.push({ id: "removefromplaylist", text: I18n.t("Remove from this playlist"), icon: "remove_circle" })
        if (_isCurrent)
            items.push({ id: "repeat", text: I18n.t("Repeat this"), icon: Player.repeat === "one" ? "repeat_one" : "repeat",
                         checked: Player.repeat !== "off" })
        if (_isCurrent && Player.source === "radio")
            items.push({ id: "dislike", text: I18n.t("Don't recommend"), icon: "thumb_down" })
        items.push({ separator: true })
        if (_episode)
            items.push({ id: "goto:show", text: I18n.t("Go to podcast"), icon: "podcasts", enabled: (track.showId ?? "") !== "" })
        else
            items.push({ id: "goto:album", text: I18n.t("Go to album"), icon: "album", enabled: hasAlbum },
                       { id: "goto:artist", text: I18n.t("Go to artist"), icon: "person", enabled: _refs.length > 0 })
        const radio = _radioItems()
        if (radio.length > 0) {
            items.push({ separator: true })
            for (const r of radio)
                items.push(r)
        }
        items.push({ separator: true })
        if (!_episode && hasAlbum) {
            const saved = Library.savedRevision >= 0 && Library.isSaved("spotify:album:" + track.albumId)
            items.push({ id: "save", text: saved ? I18n.t("Remove album from library") : I18n.t("Save album"), icon: saved ? "library_add_check" : "library_add" })
        } else if (_episode) {
            const saved = Library.savedRevision >= 0 && Library.isSaved(track.uri ?? "")
            items.push({ id: "save", text: saved ? I18n.t("Remove episode from library") : I18n.t("Save episode"), icon: saved ? "library_add_check" : "library_add" })
        }
        items.push({ id: "share", text: I18n.t("Share"), icon: "share", submenu: _shareItems() })
        if (!_episode && _id !== "")
            items.push({ id: "info", text: I18n.t("Track info"), icon: "info" })
        items.push({ id: "queue", text: I18n.t("Open queue"), icon: "queue_music" })
        return items
    }

    function _items() {
        if (Theme.mobile && !_mini && (_id !== "" || (track.uri ?? "") !== ""))
            return _mobileItems()
        const mini = _mini ? _miniItems() : []
        if (_id === "" && (track.uri ?? "") === "")
            return mini
        if (mini.length > 0)
            mini.push({ separator: true })
        const hasAlbum = (track.albumId ?? "") !== ""
        const liked = Library.likesRevision >= 0 && _id !== "" && Library.isLiked(_id)
        let items = mini.concat(_radioItems())
        if (items.length > mini.length)
            items.push({ separator: true })
        items.push({ id: "addqueue", text: I18n.t("Add to queue"), icon: "add_to_queue", enabled: (track.uri ?? "") !== "" })
        // в мини-режиме повтор уже в верхней группе
        if (_isCurrent && !_mini)
            items.push({ id: "repeat", text: I18n.t("Repeat this"), icon: Player.repeat === "one" ? "repeat_one" : "repeat",
                         checked: Player.repeat !== "off" })
        if (_isCurrent && Player.source === "radio")
            items.push({ id: "dislike", text: I18n.t("Don't recommend"), icon: "thumb_down" })
        items.push(
            { id: "queue", text: I18n.t("Open queue"), icon: "queue_music" },
            { separator: true }
        )
        if (!_episode) {
            items.push({ id: "like", text: liked ? I18n.t("Remove from Liked Songs") : I18n.t("Save to Liked Songs"), icon: "favorite" },
                       { id: "playlist", text: I18n.t("Add to playlist"), icon: "playlist_add", submenu: _playlistItems() })
            if (root.playlistId !== "" && typeof track.pos === "number")
                items.push({ id: "removefromplaylist", text: I18n.t("Remove from this playlist"), icon: "remove_circle" })
            if (hasAlbum) {
                const saved = Library.savedRevision >= 0 && Library.isSaved("spotify:album:" + track.albumId)
                items.push({ id: "save", text: saved ? I18n.t("Remove album from library") : I18n.t("Save album"), icon: saved ? "library_add_check" : "library_add" })
            }
        } else {
            const saved = Library.savedRevision >= 0 && Library.isSaved(track.uri ?? "")
            items.push({ id: "save", text: saved ? I18n.t("Remove episode from library") : I18n.t("Save episode"), icon: saved ? "library_add_check" : "library_add" })
        }
        items.push({ id: "share", text: I18n.t("Share"), icon: "share", submenu: _shareItems() })
        if (!_episode && _id !== "" && !_mini)
            items.push({ id: "info", text: I18n.t("Track info"), icon: "info" })
        items.push({ separator: true })
        if (_episode)
            items.push({ id: "goto:show", text: I18n.t("Go to podcast"), icon: "podcasts", enabled: (track.showId ?? "") !== "" })
        else
            items.push(
                { id: "goto:album", text: I18n.t("Go to album"), icon: "album", enabled: hasAlbum },
                { id: "goto:artist", text: I18n.t("Go to artist"), icon: "person", enabled: _refs.length > 0 }
            )
        return items
    }

    anchors.fill: parent

    Menu {
        id: menu
        anchors.fill: parent
        header: root._id !== "" || (root.track.uri ?? "") !== ""
                ? { title: root.track.title ?? "", subtitle: root.track.artists ?? "", image: root.track.cover ?? "" } : null
        onTriggered: (id, item) => {
            if (id.startsWith("mini:"))
                root._miniAction(id)
            else if (id === "info")
                infoDialog.open(root.track)
            else
                root.triggered(id, root.track, item)
        }
    }

    TrackInfoDialog {
        id: infoDialog
        z: 1
    }
}
