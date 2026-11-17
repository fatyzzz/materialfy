import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Мини-плеер на всё окно (окно сжато меньше обычной раскладки). Высокое окно — «карточка»
// (обложка над названием или слева от него, если окно широкое), низкое — «полоска» в одну строку.
// Фон тянет окно, двойной клик — развернуть обратно.
Item {
    id: root

    required property Window window
    signal expandRequested()

    readonly property Item coverItem: strip ? stripCover : cardCover
    readonly property bool strip: height < 200
    // Широкая невысокая карточка: обложка слева, остальное справа
    readonly property bool landscape: !strip && width > height * 1.35

    // Позиция приходит ~5 раз в секунду; между обновлениями — плавно (как в мини-плеере)
    readonly property alias progress: playerProgress.value
    PlayerProgress {
        id: playerProgress
        active: (cardSeek.visible || stripSeek.visible) && !cardSeek.dragging && !stripSeek.dragging
    }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    // ---- Фон: размытая обложка + вуаль цвета темы (как в полноэкранном плеере) ----
    Rectangle { anchors.fill: parent; color: Theme.surface }
    TrackBackdrop {
        anchors.fill: parent
        visible: Player.cover !== ""
        source: Player.cover
        mode: Settings.backdropMode
        blur: Settings.nowPlayingBlur
        flow: Settings.nowPlayingDrift
        followTheme: Settings.accentFromCover
        running: root.visible && Player.playing
    }
    // вуаль на ударе чуть тоньше — переливающийся фон «вспыхивает» в такт
    Rectangle {
        anchors.fill: parent
        color: Theme.surface
        opacity: Player.cover === "" ? 0 : (Theme.dark ? 0.6 : 0.52) - (Settings.coverPulse ? root.beat * 0.16 : 0)
    }

    // «Дыхание» в такт, как у полноэкранного плеера: уровень звука плеера (запрашивает NowPlaying и в режиме mini,
    // только при локальном воспроизведении); доля над скользящим средним — удар
    property real avgLevel: 0
    property real beat: 0
    Behavior on beat { NumberAnimation { duration: 110; easing.type: Easing.OutQuad } }
    Connections {
        target: Player
        enabled: root.visible
        function onLevelChanged() {
            root.avgLevel += (Player.level - root.avgLevel) * 0.06
            const b = Math.max(0, Math.min(1, (Player.level - root.avgLevel) * 4))
            // значимые сдвиги только: каждый старт Behavior перерисовывает всё мини-окно,
            // а события уровня приходят 30–60 раз в секунду
            if (Math.abs(b - root.beat) > 0.04)
                root.beat = b
        }
    }
    Connections {
        target: Player
        function onStateChanged() { if (!Player.playing) root.beat = 0 }
    }

    // Перетаскивание окна за фон (кнопки и полоса перемотки забирают нажатие сами)
    DragHandler {
        property bool manual: false
        target: null
        grabPermissions: PointerHandler.TakeOverForbidden
        onActiveChanged: if (active) manual = !root.window.startSystemMove()
        onCentroidChanged: if (active && manual && root.window.visibility === Window.Windowed) {
            root.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
            root.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
        }
    }
    TapHandler {
        acceptedButtons: Qt.LeftButton
        onDoubleTapped: root.expandRequested()
    }
    TapHandler {
        id: menuTap
        // без трека меню тоже нужно: в нём перемешивание, повтор, «поверх всех окон», «развернуть»
        acceptedButtons: Qt.RightButton
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad   // касание — не правый клик: у него нет кнопок, TapHandler принял бы любой тап
        onTapped: point => {
            const p = root.mapToItem(null, point.position.x, point.position.y)
            Catalog.requestTrackMenu(Player.track, p.x, p.y)
        }
    }

    // ---- Общие куски ----
    component Cover: Item {
        id: cover
        property real size: 120
        implicitWidth: size
        implicitHeight: size
        MorphImage {
            id: img
            anchors.fill: parent
            visible: Player.cover !== ""
            // крупная карточка — обложка 640 px, полоска — обычная
            source: cover.size > 160 && Player.coverLarge !== "" ? Player.coverLarge : Player.cover
            fallbackKind: Player.album !== "" ? "album" : "track"
            fallbackArtist: Player.artist.split(", ")[0]
            fallbackTitle: Player.album !== "" ? Player.album : Player.title
            shape: Player.playing ? "cookie12" : "softSquare"
            pulse: Settings.coverPulse && Player.isLocal === true ? root.beat : 0
            MaskSpin { target: img; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true }
        }
        MorphShape {
            anchors.fill: parent
            visible: Player.cover === ""
            shape: cover.size >= 96 ? "cookie9" : "softSquare"
            color: Theme.secondaryContainer
            Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: Math.max(20, cover.size * 0.32); color: Theme.fgSecondaryContainer }
        }
        // играет другое устройство — его значок в углу обложки
        DeviceBadge {
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.rightMargin: cover.size >= 96 ? cover.size * 0.06 : -4
            anchors.bottomMargin: cover.size >= 96 ? cover.size * 0.06 : -4
            size: cover.size >= 96 ? 34 : 22
            ringColor: Theme.surface
        }
    }

    component PlayControl: Item {
        id: pc
        property real size: 56
        implicitWidth: size
        implicitHeight: size
        PlayButton {
            anchors.fill: parent
            size: pc.size
            spin: Player.isLocal === true
            // без трека — запустить радио по выбранному сиду
            playing: Player.hasTrack ? Player.playing : Radio.active && Player.playing
            enabled: Player.hasTrack || (Auth.state === "signedIn" && !Radio.locked)
            onClicked: Player.hasTrack ? Player.togglePlay() : Radio.play()
        }
        LoadingIndicator {
            anchors.centerIn: parent
            size: pc.size * 1.15
            color: Theme.primary
            visible: Player.buffering || Radio.loading
            opacity: 0.5
        }
    }

    // Пилюля в углу: булавка «поверх всех окон» видна всегда (клик закрепляет и открепляет), «свернуть»,
    // «развернуть» (обратно в обычную раскладку) и «закрыть» — при наведении. «Развернуть» есть и в меню по ПКМ
    // и по двойному клику, «Поверх всех окон» — и в меню. Где закрепить нельзя (Wayland), пилюля — по наведению.
    HoverHandler { id: hover }
    Rectangle {
        id: chrome
        readonly property bool pinned: root.window.pinnedOnTop === true
        readonly property bool canPin: root.window.canPin !== false
        readonly property bool shown: hover.hovered || canPin
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 10               // мимо зоны ресайза у края окна
        z: 10
        width: chromeRow.implicitWidth + 4
        height: chromeRow.implicitHeight + 4
        radius: height / 2
        color: Qt.alpha(Theme.surfaceContainerHigh, 0.88)
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Behavior on width { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
        clip: true

        Row {
            id: chromeRow
            anchors.centerIn: parent
            spacing: 0
            IconButton {
                visible: chrome.canPin
                icon: "push_pin"; size: "xs"
                checkable: true; autoToggle: false; checked: chrome.pinned
                iconColor: checked ? Theme.fgSecondaryContainer : Theme.fgSurface
                onClicked: root.window.pinnedOnTop = !chrome.pinned
            }
            IconButton {
                visible: hover.hovered
                icon: "remove"; size: "xs"; iconColor: Theme.fgSurface
                Accessible.role: Accessible.Button; Accessible.name: I18n.t("Minimize")
                onClicked: root.window.showMinimized()
            }
            IconButton {
                visible: hover.hovered
                icon: "open_in_full"; size: "xs"; iconColor: Theme.fgSurface
                Accessible.role: Accessible.Button; Accessible.name: I18n.t("Expand")
                onClicked: root.expandRequested()
            }
            IconButton {
                visible: hover.hovered
                icon: "close"; size: "xs"; iconColor: hovered ? Theme.error : Theme.fgSurface
                Accessible.role: Accessible.Button; Accessible.name: I18n.t("Close")
                onClicked: root.window.close()
            }
        }
    }

    readonly property string titleText: Player.hasTrack ? Player.title : I18n.t("Nothing playing")
    readonly property string subtitleText: Player.errorText !== "" ? Player.errorText
        : Player.hasTrack ? Player.artist : Auth.state === "signedIn" ? I18n.t("Start a radio") : I18n.t("Sign in to listen")

    // ---- Карточка ----
    Item {
        id: card
        anchors.fill: parent
        visible: !root.strip

        GridLayout {
            anchors.fill: parent
            anchors.margins: 16
            anchors.topMargin: 16
            columns: root.landscape ? 2 : 1
            columnSpacing: 16
            rowSpacing: 10

            Cover {
                id: cardCover
                Layout.alignment: Qt.AlignHCenter | Qt.AlignVCenter
                // «карточка»: обложка забирает всё, что осталось над названием и кнопками
                // справа колонке нужно не меньше 230 px: название, время и «назад / слушать / вперёд»
                size: root.landscape
                      ? Math.max(64, Math.min(card.height - 32, card.width * 0.42, card.width - 32 - 16 - 230))
                      : Math.max(56, Math.min(card.width - 48, card.height - 40 - 16 - 172, 320))
            }

            ColumnLayout {
                id: cardInfo
                Layout.fillWidth: true
                // ровно оставшееся место: длинная подпись или ряд кнопок не распирают колонку за край окна
                Layout.preferredWidth: 0
                Layout.minimumWidth: 0
                Layout.fillHeight: root.landscape
                Layout.topMargin: root.landscape ? 24 : 0     // под кнопками окна
                spacing: 6

                Item { Layout.fillHeight: true; visible: root.landscape }

                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: root.landscape ? Text.AlignLeft : Text.AlignHCenter
                    text: root.titleText
                    type: card.width >= 340 ? "titleLarge" : "titleMedium"
                    weight: 600
                    elide: Text.ElideRight
                }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: root.landscape ? Text.AlignLeft : Text.AlignHCenter
                    text: root.subtitleText
                    type: "bodyMedium"
                    color: Player.errorText !== "" ? Theme.error : Theme.fgSurfaceVariant
                    elide: Text.ElideRight
                }
                // пульт: «Играет на …»
                RemoteLabel {
                    visible: shown
                    Layout.alignment: root.landscape ? Qt.AlignLeft : Qt.AlignHCenter
                    Layout.maximumWidth: cardInfo.width
                    brief: cardInfo.width < 300
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    visible: Player.hasTrack
                    spacing: 8
                    Label {
                        text: root._time(cardSeek.dragging ? cardSeek.dragValue * Player.durationMs : Player.positionMs)
                        type: "labelSmall"
                        color: Theme.fgSurfaceVariant
                    }
                    WavyProgress {
                        id: cardSeek
                        Layout.fillWidth: true
                        Layout.preferredHeight: 18
                        thickness: 4
                        amplitude: 2.5
                        wavelength: 30
                        value: root.progress
                        wavy: Player.playing
                        interactive: true
                        onCommitted: v => Player.seek(v)
                    }
                    Label {
                        text: root._time(Player.durationMs)
                        type: "labelSmall"
                        color: Theme.fgSurfaceVariant
                    }
                }

                RowLayout {
                    Layout.alignment: root.landscape ? Qt.AlignLeft : Qt.AlignHCenter
                    spacing: card.width >= 340 ? 8 : 2
                    IconButton {
                        visible: Player.source === "radio" && !Player.isEpisode && card.width >= 300
                        icon: "thumb_down"
                        iconColor: Theme.fgSurface
                        enabled: Player.hasTrack
                        onClicked: Library.dislike(Player.track)
                    }
                    IconButton { icon: "skip_previous"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.previous() }
                    PlayControl { size: card.width >= 340 ? 60 : 52 }
                    IconButton { icon: "skip_next"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.next() }
                    LikeButton { track: Player.track; visible: card.width >= 260 && !Player.isEpisode }
                    IconButton {
                        visible: card.width >= 260 && Player.isEpisode
                        icon: "bookmark"
                        checkable: true
                        autoToggle: false
                        checked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
                        onClicked: Library.toggleSaved(Player.uri)
                    }
                }

                Item { Layout.fillHeight: true; visible: root.landscape }
            }
        }
    }

    // ---- Полоска ----
    Item {
        id: stripBox
        anchors.fill: parent
        visible: root.strip

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 14           // кнопки — мимо зоны ресайза у правого края
            anchors.topMargin: 6
            anchors.bottomMargin: 14
            spacing: 6

            Cover {
                id: stripCover
                Layout.alignment: Qt.AlignVCenter
                size: Math.max(40, Math.min(stripBox.height - 28, 72, stripBox.width * 0.2))
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 4
                Layout.alignment: Qt.AlignVCenter
                spacing: 0
                Label {
                    Layout.fillWidth: true
                    text: root.titleText
                    type: "titleSmall"
                    weight: 600
                    elide: Text.ElideRight
                }
                Label {
                    Layout.fillWidth: true
                    visible: !stripRemote.shown
                    text: root.subtitleText
                    type: "bodySmall"
                    color: Player.errorText !== "" ? Theme.error : Theme.fgSurfaceVariant
                    elide: Text.ElideRight
                }
                RemoteLabel {
                    id: stripRemote
                    visible: shown
                    brief: true
                }
            }
            IconButton { icon: "skip_previous"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.previous() }
            PlayControl { size: 48 }
            IconButton { icon: "skip_next"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.next() }
            LikeButton { track: Player.track; visible: stripBox.width >= 420 && !Player.isEpisode }
        }

        // Перемотка — тонкой волной по нижнему краю
        WavyProgress {
            id: stripSeek
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 8          // нижние 6 px — зона ресайза окна
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            height: 12
            visible: Player.hasTrack
            thickness: 3
            amplitude: 1.5
            wavelength: 24
            value: root.progress
            wavy: Player.playing
            interactive: true
            onCommitted: v => Player.seek(v)
        }
    }

    // Колесо над мини-плеером — громкость: щелчок колеса ±5 %, тачпад — плавно; плашка с процентами
    WheelHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: event => {
            const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
            const d = touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05
            root.nudgeVolume(d)
        }
    }
    function nudgeVolume(d) {
        // у части удалённых устройств громкость меняется только на них самих
        if (Player.remote === true && Player.canSetVolume === false)
            return
        Player.volume = Math.max(0, Math.min(1, Player.volume + d))
        volumeHint.opacity = 1
        volumeHintHide.restart()
    }
    // Стрелки в мини-плеере (окно в фокусе): ←/→ — трек, ↑/↓ — громкость. В большом окне стрелки листают списки,
    // там для этого Ctrl+стрелки
    Shortcut { enabled: root.visible; sequence: "Left"; onActivated: Player.previous() }
    Shortcut { enabled: root.visible; sequence: "Right"; onActivated: Player.next() }
    Shortcut { enabled: root.visible; sequence: "Up"; onActivated: root.nudgeVolume(0.05) }
    Shortcut { enabled: root.visible; sequence: "Down"; onActivated: root.nudgeVolume(-0.05) }
    Timer { id: volumeHintHide; interval: 1000; onTriggered: volumeHint.opacity = 0 }
    Rectangle {
        id: volumeHint
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: 10
        z: 11
        width: volumeRow.implicitWidth + 24
        height: 32
        radius: 16
        color: Qt.alpha(Theme.surfaceContainerHigh, 0.92)
        opacity: 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Row {
            id: volumeRow
            anchors.centerIn: parent
            spacing: 6
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: Player.volume > 0.5 ? "volume_up" : Player.volume > 0 ? "volume_down" : "volume_off"
                size: 18
                color: Theme.fgSurface
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: Math.round(Player.volume * 100) + " %"
                type: "labelLarge"
                color: Theme.fgSurface
            }
        }
    }
}
