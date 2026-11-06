import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Плавающий мини-плеер-пилюля (низ контент-панели).
Item {
    id: root

    readonly property Item coverItem: coverBox
    // узкое окно: одна строка — обложка, трек, «играть» + «дальше», лайк и «⋯»;
    // режимы, громкость и панели — в полноэкранном плеере
    property bool compact: false
    signal openNowPlaying(point coverCenter)   // центр обложки в координатах окна — отсюда растёт блоб

    implicitHeight: 72

    // В узкой пилюле место нужнее треку: по мере сужения уходят «⋯» (меню остаётся по ПКМ на треке),
    // громкость (она есть в полноэкранном плеере) и перемешивание с повтором
    // с запасом ±16 px: у самой границы кнопки не мигают туда-сюда
    property bool roomForMenu: width >= 880
    property bool roomForVolume: width >= 760
    property bool roomForModes: width >= 640
    // имя чужого устройства в кнопке Connect; уже — только значок (имя есть под названием трека)
    property bool roomForDeviceName: width >= 900
    onWidthChanged: {
        const room = (now, edge) => now ? width >= edge - 16 : width >= edge + 16
        roomForDeviceName = room(roomForDeviceName, 900)
        roomForMenu = room(roomForMenu, 880)
        roomForVolume = room(roomForVolume, 760)
        roomForModes = room(roomForModes, 640)
    }

    // Кнопка, которая уходит при сужении: место схлопывается анимацией, сама она гаснет и уменьшается
    component Fold: Item {
        id: fold
        property bool shown: true
        default property alias content: slot.data
        readonly property real full: slot.childrenRect.width
        implicitHeight: slot.childrenRect.height
        Layout.preferredWidth: shown ? full : 0
        visible: Layout.preferredWidth > 0.5
        clip: true
        Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
        Item {
            id: slot
            anchors.centerIn: parent
            width: childrenRect.width
            height: childrenRect.height
            opacity: fold.shown ? 1 : 0
            scale: fold.shown ? 1 : 0.6
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
        }
    }

    // Позиция приходит ~5 раз в секунду (у другого устройства — опросом); между обновлениями — плавно.
    readonly property alias progress: playerProgress.value
    PlayerProgress { id: playerProgress; active: seekBar.visible && !seekBar.dragging }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceContainerHighest
    }
    Elevation { target: bg; level: 2 }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 16
        spacing: 4

        // Управление
        // У выпуска подкаста вместо перемешивания и повтора — «−10 с» и «+30 с»
        Fold { shown: root.roomForModes && !root.compact; IconButton {
            icon: Player.isEpisode ? "replay_10" : "shuffle"
            checkable: !Player.isEpisode
            autoToggle: false
            checked: !Player.isEpisode && Player.shuffle
            enabled: Player.hasTrack && (Player.isEpisode || Player.source !== "radio")
            onClicked: {
                if (Player.isEpisode)
                    Player.seekRelative(-10000)
                else
                    Player.shuffle = !Player.shuffle
            }
        } }
        IconButton { icon: "skip_previous"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.previous() }
        Item {
            implicitWidth: 52
            implicitHeight: 52
            PlayButton {
                anchors.fill: parent
                size: 52
                playing: Player.playing
                loading: Player.buffering
                spin: Player.isLocal === true      // пульт не перерисовывает окно каждый кадр
                enabled: Player.hasTrack
                onClicked: Player.togglePlay()
            }
        }
        IconButton { icon: "skip_next"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.next() }
        Fold { shown: root.roomForModes && !root.compact; IconButton {
            icon: Player.isEpisode ? "forward_30" : Player.repeat === "one" ? "repeat_one" : "repeat"
            checkable: !Player.isEpisode
            autoToggle: false
            checked: !Player.isEpisode && Player.repeat !== "off"
            enabled: Player.hasTrack
            onClicked: Player.isEpisode ? Player.seekRelative(30000) : Player.cycleRepeat()
        } }

        // Трек
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            // в самой узкой раскладке текст не залезает под лайк
            clip: true

            // Колесо над обложкой и названием — громкость, как в мини-режиме: щелчок ±5 %, тачпад плавно
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    if (Player.remote === true && Player.canSetVolume === false)
                        return
                    const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
                    const d = touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05
                    Player.volume = Math.max(0, Math.min(1, Player.volume + d))
                    volumeHint.opacity = 1
                    volumeHintHide.restart()
                }
            }

            RowLayout {
                anchors.fill: parent
                spacing: 12

                Item {
                    id: coverBox
                    Layout.preferredWidth: 48
                    Layout.preferredHeight: 48
                    MorphImage {
                        id: miniCover
                        anchors.fill: parent
                        visible: Player.cover !== ""
                        source: Player.cover
                        fallbackKind: Player.album !== "" ? "album" : "track"
                        fallbackArtist: Player.artist.split(", ")[0]
                        fallbackTitle: Player.album !== "" ? Player.album : Player.title
                        shape: Player.playing ? "cookie12" : "softSquare"
                        MaskSpin { target: miniCover; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true }
                    }
                    MorphShape {
                        anchors.fill: parent
                        visible: Player.cover === ""
                        shape: "softSquare"
                        color: Theme.surfaceContainerHigh
                        Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 24; color: Theme.fgSurfaceVariant }
                    }
                    // громкость колесом: проценты поверх обложки, гаснут через секунду
                    Rectangle {
                        id: volumeHint
                        anchors.fill: parent
                        radius: Theme.shape.medium
                        color: Qt.alpha(Theme.surfaceContainerHighest, 0.88)
                        opacity: 0
                        visible: opacity > 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                        Label {
                            anchors.centerIn: parent
                            text: Math.round(Player.volume * 100) + "%"
                            type: "labelMedium"
                            weight: 600
                        }
                        Timer { id: volumeHintHide; interval: 1000; onTriggered: volumeHint.opacity = 0 }
                    }
                    // играет другое устройство — его значок в углу обложки
                    DeviceBadge {
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.rightMargin: -4
                        anchors.bottomMargin: -4
                        ringColor: Theme.surfaceContainerHighest
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Label {
                        Layout.fillWidth: true
                        text: Player.hasTrack ? Player.title : I18n.t("Nothing playing")
                        type: "titleSmall"
                    }
                    // Исполнитель · кодек или «Играет на …» (по правому краю — прямо над временем)
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        // Исполнители — по ссылке на каждого (карточка по наведению). Выпуск, ошибка, пустой
                        // плеер и тесная пилюля с чужим устройством остаются одной подписью
                        readonly property bool artistLinks: Player.errorText === "" && Player.hasTrack
                                                            && Player.isEpisode !== true
                                                            && !(remoteLabel.shown && !root.roomForMenu)
                        ArtistLinks {
                            Layout.fillWidth: true
                            visible: parent.artistLinks
                            // карточки по наведению тут нет: клик по имени сразу открывает исполнителя
                            preview: false
                            refs: Player.track ? (Player.track.artistRefs ?? []) : []
                            text: Player.artist
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                        }
                        LinkLabel {
                            Layout.fillWidth: true
                            visible: !parent.artistLinks && !(remoteLabel.shown && !root.roomForMenu && Player.errorText === "")
                            // у выпуска подкаста — ссылка на подкаст
                            linkEnabled: Player.errorText === "" && Player.hasTrack && Player.isEpisode === true
                                         && (Player.track.showId ?? "") !== ""
                            onClicked: Catalog.openShow(Player.track.showId ?? "")
                            text: Player.errorText !== "" ? Player.errorText
                                : Player.hasTrack ? Player.artist : I18n.t("Play a track, album or playlist")
                            type: "bodySmall"
                            color: Player.errorText !== "" ? Theme.error : Theme.fgSurfaceVariant
                        }
                        CodecBadge { Layout.alignment: Qt.AlignVCenter; Layout.bottomMargin: 3 }
                        RemoteLabel {
                            id: remoteLabel
                            Layout.alignment: Qt.AlignVCenter
                            Layout.maximumWidth: 220
                            brief: root.compact
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        visible: Player.hasTrack
                        spacing: 8
                        WavyProgress {
                            id: seekBar
                            Layout.fillWidth: true
                            Layout.preferredHeight: 14
                            thickness: 3
                            amplitude: 2
                            wavelength: 28
                            value: root.progress
                            wavy: Player.playing
                            interactive: true
                            onCommitted: v => Player.seek(v)
                        }
                        Label {
                            text: root._time(seekBar.dragging ? seekBar.dragValue * Player.durationMs : Player.positionMs)
                                  + " / " + root._time(Player.durationMs)
                            type: "labelSmall"
                            color: Theme.fgSurfaceVariant
                        }
                    }
                }
            }

            TapHandler {
                enabled: Player.hasTrack
                onTapped: root.openNowPlaying(coverBox.mapToItem(null, coverBox.width / 2, coverBox.height / 2))
            }
            TapHandler {
                id: trackMenuTap
                enabled: Player.hasTrack
                acceptedButtons: Qt.RightButton
                onTapped: (point) => {
                    const p = trackMenuTap.parent.mapToItem(null, point.position.x, point.position.y)
                    Catalog.requestTrackMenu(Player.track, p.x, p.y)
                }
            }
        }

        // Оценка: «не рекомендовать» — только в радио (там она влияет на подбор; сама переключает трек),
        // лайк — всегда, у выпуска — «сохранить» (закладка)
        Fold { shown: Player.source === "radio" && !Player.isEpisode && !root.compact; IconButton {
            icon: "thumb_down"
            enabled: Player.hasTrack
            onClicked: Library.dislike(Player.track)
        } }
        LikeButton { visible: !Player.isEpisode; track: Player.track }
        IconButton {
            visible: Player.isEpisode
            icon: "bookmark"
            checkable: true
            autoToggle: false
            checked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
            enabled: Player.uri !== ""
            onClicked: Library.toggleSaved(Player.uri)
        }
        Fold { shown: root.roomForMenu || root.compact; IconButton {
            id: miniMore
            icon: "more_horiz"
            iconSize: 30
            iconWeight: 700
            iconColor: enabled ? Theme.fgSurface : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
            enabled: Player.hasTrack
            onClicked: {
                const p = miniMore.mapToItem(null, 0, 0)
                Catalog.requestTrackMenuAt(Player.track, p.x, p.y, miniMore.width, miniMore.height)
            }
        } }
        Fold { shown: !root.compact; Item { width: 8; height: 8 } }

        // Панели и громкость
        Fold { shown: !root.compact; IconButton {
            icon: "title"
            checkable: true
            autoToggle: false
            checked: Settings.rightPanelOpen && Settings.rightPanelTab === "lyrics"
            onClicked: root._togglePanel("lyrics")
        } }
        // Очередь — ещё и цель для броска трека из списков: подсвечивается и чуть растёт
        Fold { shown: !root.compact; IconButton {
            id: queueButton
            icon: "queue_music"
            checkable: true
            autoToggle: false
            checked: (Settings.rightPanelOpen && Settings.rightPanelTab === "queue") || queueDrop.containsDrag
            scale: queueDrop.containsDrag ? 1.08 : 1
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
            onClicked: root._togglePanel("queue")
            DropArea {
                id: queueDrop
                anchors.fill: parent
                anchors.margins: -6
                keys: ["materialfy/track", "text/uri-list", "text/plain"]
                onDropped: drop => {
                    if (root._dropToQueue(drop))
                        drop.acceptProposedAction()
                }
            }
        } }
        // Устройства Spotify Connect (в узкой пилюле — тоже: перенос нужен и там)
        Fold { DeviceButton { iconOnly: root.compact || !root.roomForDeviceName } }
        Fold { shown: root.roomForVolume && !root.compact; Row {
            id: volumeRow
            // громкость играющего устройства; у части устройств её меняют только на них самих
            readonly property bool locked: Player.remote === true && Player.canSetVolume === false
            leftPadding: 8
            spacing: 4
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: volumeRow.locked ? "lock" : Player.volume > 0.5 ? "volume_up" : Player.volume > 0 ? "volume_down" : "volume_off"
                size: 20
            }
            Slider {
                anchors.verticalCenter: parent.verticalCenter
                width: 96
                trackHeight: 10
                handleHeight: 28
                enabled: !volumeRow.locked
                opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                value: Player.volume
                onMoved: v => Player.volume = v
            }
        } }
    }

    // Бросок на кнопку очереди: трек из списка (Drag.source с trackData() или track) или ссылка Spotify
    function _dropToQueue(drop) {
        const src = drop.source
        if (src) {
            const t = typeof src.trackData === "function" ? src.trackData() : src.track
            if (t && (t.uri || t.trackId)) {
                Player.addToQueue(t)
                return true
            }
        }
        const text = drop.hasUrls && drop.urls.length > 0 ? drop.urls[0].toString() : drop.hasText ? drop.text : ""
        const m = /(?:spotify:|open\.spotify\.com\/)(track|episode|album|playlist)[:\/]([A-Za-z0-9]+)/.exec(text.trim())
        if (m) {
            Player.addUriToQueue("spotify:" + m[1] + ":" + m[2])
            return true
        }
        return false
    }

    function _togglePanel(tab) {
        if (Settings.rightPanelOpen && Settings.rightPanelTab === tab) {
            Settings.rightPanelOpen = false
        } else {
            Settings.rightPanelTab = tab
            Settings.rightPanelOpen = true
        }
    }
}
