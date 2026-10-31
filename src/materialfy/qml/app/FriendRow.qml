import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Строка «Активности друзей» (как правая панель Spotify для компьютера): аватар, имя, когда; трек · исполнитель
// (ссылки), откуда играл; при наведении — «Играть». friend — ключи FriendActivity из Social.friends.
Item {
    id: root

    property var friend: ({})
    // «сейчас» — меньше 10 минут назад (так же I18n.relative пишет «Сейчас»)
    readonly property bool live: (friend.timestamp ?? 0) > 0 && now - friend.timestamp < 10 * 60 * 1000
    property real now: Date.now()       // обновляет подпись «N мин назад» вместе с опросом панели

    implicitWidth: 320
    implicitHeight: content.implicitHeight + 16

    function play() {
        const track = friend.trackUri ?? ""
        if (track === "")
            return
        // метаданные есть тут же: строка переключается сразу, не дожидаясь ни Web API, ни звука
        Player.rememberTrack({
            uri: track, trackId: track.split(":").pop(), title: friend.track ?? "",
            artists: friend.artist ?? "", album: friend.album ?? "", albumId: (friend.albumUri ?? "").split(":").pop() ?? "",
            cover: friend.cover ?? "", isEpisode: friend.isEpisode === true
        })
        const context = friend.contextUri ?? ""
        if (context !== "" && !friend.isEpisode)
            Player.playContext(context, track)
        else if (friend.isEpisode)
            Player.playEpisode(track, 0)
        else
            Player.playUris([track], 0)
    }

    // двойной щелчок — играть, как у строк треков
    StateLayer {
        id: hit
        anchors.fill: parent
        radius: Theme.shape.medium
        onDoubleClicked: root.play()
    }

    RowLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 8
        anchors.rightMargin: 8
        spacing: 12

        Item {
            Layout.alignment: Qt.AlignTop
            Layout.preferredWidth: 44
            Layout.preferredHeight: 44
            Avatar {
                anchors.centerIn: parent
                size: 44
                name: root.friend.name ?? ""
                source: root.friend.avatar ?? ""
            }
            // «слушает сейчас»
            Rectangle {
                visible: root.live
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                width: 14
                height: 14
                radius: 7
                color: Theme.primary
                border.width: 2
                border.color: Theme.surfaceContainer
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1

            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Label {
                    Layout.fillWidth: true
                    text: root.friend.name ?? ""
                    type: "titleSmall"
                }
                Icon {
                    visible: root.live
                    name: "graphic_eq"
                    size: 16
                    color: Theme.primary
                }
                Label {
                    visible: !root.live
                    text: { void root.now; return I18n.relative(root.friend.timestamp ?? 0) }
                    type: "labelSmall"
                    color: Theme.fgSurfaceVariant
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 0
                LinkLabel {
                    Layout.maximumWidth: implicitWidth
                    Layout.fillWidth: true
                    text: root.friend.track ?? ""
                    type: "bodyMedium"
                    linkEnabled: (root.friend.albumUri ?? "") !== ""
                    onClicked: Catalog.openUri(root.friend.albumUri)
                }
                Label {
                    visible: (root.friend.artist ?? "") !== ""
                    text: " · "
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
                LinkLabel {
                    visible: (root.friend.artist ?? "") !== ""
                    Layout.fillWidth: true
                    text: root.friend.artist ?? ""
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    linkEnabled: (root.friend.artistUri ?? "").startsWith("spotify:artist:")
                                 || (root.friend.artistUri ?? "").startsWith("spotify:show:")
                    previewArtistId: (root.friend.artistUri ?? "").startsWith("spotify:artist:")
                                     ? root.friend.artistUri.split(":")[2] : ""
                    onClicked: Catalog.openUri(root.friend.artistUri)
                }
            }

            RowLayout {
                visible: (root.friend.context ?? "") !== ""
                Layout.fillWidth: true
                spacing: 4
                Icon {
                    name: {
                        const uri = root.friend.contextUri ?? ""
                        return uri.startsWith("spotify:album:") ? "album"
                             : uri.startsWith("spotify:artist:") ? "person"
                             : "playlist_play"
                    }
                    size: 14
                    color: Theme.fgSurfaceVariant
                }
                LinkLabel {
                    Layout.fillWidth: true
                    text: root.friend.context ?? ""
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                    linkEnabled: (root.friend.contextUri ?? "") !== ""
                    onClicked: Catalog.openUri(root.friend.contextUri)
                }
            }
        }

        IconButton {
            Layout.alignment: Qt.AlignVCenter
            icon: "play_arrow"
            style: "tonal"
            visible: hit.containsMouse || hovered
            enabled: (root.friend.trackUri ?? "") !== ""
            onClicked: root.play()
        }
    }
}
