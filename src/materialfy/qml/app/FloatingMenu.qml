import QtQuick
import Md3
import Materialfy.Core

// Меню по правому клику островка и виджета в панели задач: трек (любимые, плейлист, очередь, радио,
// ссылка, переходы), управление воспроизведением, окно. Действия с треком выполняет главное окно
// (Main.qml trackMenuAction) — те же, что в меню трека внутри приложения. Кладётся в окно: anchors.fill.
Item {
    id: root

    property var mainWindow: null
    readonly property alias opened: menu.opened

    function openAt(point) {
        menu.open(_items(), point)
    }

    function _playlistItems() {
        const out = [{ id: "playlist:new", text: I18n.t("New playlist…"), icon: "add" }]
        const model = Library.playlists
        const me = Auth.account ? (Auth.account.id ?? "") : ""
        const own = []
        for (let i = 0; model && i < model.count; i++) {
            const p = model.get(i)
            if ((me !== "" && p.ownerId === me) || p.canEdit === true)
                own.push({ id: "playlist:" + p.playlistId, text: p.title, icon: "queue_music" })
        }
        return own.length > 0 ? out.concat([{ separator: true }], own) : out
    }

    function _shareItems(track) {
        const refs = (track.artistRefs ?? []).filter(r => r && r.id)
        const out = [{ id: "share:track", text: track.isEpisode === true ? I18n.t("Copy episode link") : I18n.t("Copy track link"), icon: "content_copy" }]
        if (refs.length === 1)
            out.push({ id: "share:artist:" + refs[0].id, text: I18n.t("Copy artist link"), icon: "person" })
        else
            for (const r of refs)
                out.push({ id: "share:artist:" + r.id, text: I18n.t("Link to {0}", r.name), icon: "person" })
        return out
    }

    function _items() {
        const items = []
        if (Player.hasTrack) {
            const track = Player.track
            const episode = track.isEpisode === true
            const trackId = track.trackId ?? Player.trackId ?? ""
            const liked = Library.likesRevision >= 0 && trackId !== "" && Library.isLiked(trackId)
            if (!episode) {
                items.push({ id: "like", text: liked ? I18n.t("Remove from Liked Songs") : I18n.t("Save to Liked Songs"), icon: "favorite" })
                items.push({ id: "playlist", text: I18n.t("Add to playlist"), icon: "playlist_add", submenu: _playlistItems() })
            }
            items.push({ id: "addqueue", text: I18n.t("Add to queue"), icon: "add_to_queue" })
            if (!episode)
                items.push({ id: "radio:track", text: I18n.t("Track radio"), icon: "radio" })
            items.push({ id: "share", text: I18n.t("Share"), icon: "share", submenu: _shareItems(track) })
            if (!episode && (track.albumId ?? "") !== "")
                items.push({ id: "goto:album", text: I18n.t("Go to album"), icon: "album" })
            if (!episode && (track.artistRefs ?? []).some(r => r && r.id))
                items.push({ id: "goto:artist", text: I18n.t("Go to artist"), icon: "person" })
            items.push({ separator: true })
            items.push({ id: "shuffle", text: I18n.t("Shuffle playback"), icon: "shuffle", checked: Player.shuffle, enabled: Player.source !== "radio" })
            items.push({ id: "repeat", text: I18n.t("Repeat"), icon: Player.repeat === "one" ? "repeat_one" : "repeat", checked: Player.repeat !== "off" })
            items.push({ separator: true })
        }
        items.push({ id: "open", text: I18n.t("Open Materialfy"), icon: "open_in_new" })
        items.push({ id: "settings", text: I18n.t("Settings"), icon: "settings" })
        items.push({ id: "off", text: I18n.t("Turn off floating player"), icon: "visibility_off" })
        return items
    }

    Menu {
        id: menu
        anchors.fill: parent
        // меню больше окна островка: отдельным окном и на KDE Wayland, иначе обрезано окном и не ловит клики мимо маски
        windowed: Qt.platform.pluginName.indexOf("wayland") !== 0 || WindowShell.kwinAvailable()
        onTriggered: (id, item) => {
            switch (id) {
            case "open":
                WindowShell.raiseMain()
                break
            case "settings":
                WindowShell.raiseMain()
                if (root.mainWindow)
                    root.mainWindow.openSettings("appearance")
                break
            case "off":
                Settings.floatingPlayer = "off"
                break
            case "shuffle":
                Player.shuffle = !Player.shuffle
                break
            case "like":
            case "addqueue":
            case "radio:track":
            case "repeat":
                // делаются на месте — окно поднимать незачем
                if (root.mainWindow)
                    root.mainWindow.trackMenuAction(id, Player.track, item)
                break
            default:
                // переходы, новый плейлист — в главном окне; добавить в плейлист и ссылки — тоже через него
                if (id.startsWith("goto:") || id === "playlist:new")
                    WindowShell.raiseMain()
                if (root.mainWindow)
                    root.mainWindow.trackMenuAction(id, Player.track, item)
                break
            }
        }
    }
}
