import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Строка трека: номер · обложка · название/исполнители · альбом · (добавил/добавлено) · (прослушивания) · лайк ·
// длительность.
// editable — режим правки плейлиста: ручка перетаскивания слева, «убрать» вместо сердца и ⋯.
Item {
    id: root

    property int number: 0
    property string trackId
    property string title
    property string version
    property string artists
    property var artistRefs: []        // [{ id, name }] — для перехода к исполнителю
    property string albumId
    property string album
    property string cover
    property bool showCover: true
    property string coverLarge
    property string uri
    property bool isEpisode: false
    property string showId
    property int durationMs: 0
    property bool explicit: false
    property bool available: true
    // прослушивания на Spotify (0 — неизвестно); колонка — только где showPlays (альбом, исполнитель)
    // и строка достаточно широкая
    property real playCount: 0
    property bool showPlays: false
    property bool _playsRoom: width > 600
    property bool liked: false
    property bool current: false       // играет сейчас
    // выбранный трек ещё грузится (движок не отдал его): строка уже его, а звука пока нет
    property bool loading: Player.buffering
    property bool showAlbum: true
    property bool wide: showAlbum && _wideAuto
    // колонка альбома появляется с запасом ±20 px, чтобы не мигать у самой границы при растягивании
    property bool _wideAuto: width > 720
    onWidthChanged: {
        _wideAuto = _wideAuto ? width > 700 : width > 740
        _playsRoom = _playsRoom ? width > 580 : width > 620
        _sized = width > 0
    }
    property real _albumT: wide ? 1 : 0
    // Колонка выезжает, когда строку растягивают; первая ширина (строку в раскладке создают нулевой) — сразу,
    // иначе каждая новая строка полсекунды анимировала бы колонку и перекладывалась в каждом кадре
    property bool _sized: false
    Component.onCompleted: _sized = width > 0
    Behavior on _albumT { enabled: root._sized; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
    property var track: null           // полный словарь трека для меню; без него — собирается из полей
    // позиция в плейлисте (для «Убрать из этого плейлиста»); -1 — неизвестна (например, в поиске)
    // и в словарь для меню тогда не попадает: Main.qml различает «нет» и число
    property int pos: -1
    property bool highlighted: false   // вспышка «перейти к треку» / после добавления

    // Плейлист: кто и когда добавил. addedColumn: "" — нет колонки, "by" — «Добавил», "at" — «Добавлено»
    property string addedAt
    property string addedBy
    property string addedColumn
    property bool editable: false
    property bool dragging: false      // строка поднята и едет за курсором

    signal activated()                 // клик — воспроизвести
    signal likeClicked()               // клик по сердцу
    signal removeClicked()             // «убрать из плейлиста» (editable)
    signal dragStarted()
    signal dragMoved(real dy, real sceneY)
    signal dragFinished()

    implicitHeight: 64

    component ExplicitMark: Rectangle {
        implicitWidth: 16
        implicitHeight: 16
        radius: 4
        color: Theme.surfaceContainerHighest
        Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
    }

    function trackData() {
        if (track)
            return track
        const data = {
            trackId: trackId, albumId: albumId, title: title, version: version, artists: artists,
            artistRefs: artistRefs ?? [], album: album, cover: cover, coverLarge: coverLarge, durationMs: durationMs,
            explicit: explicit, available: available, uri: uri, isEpisode: isEpisode, showId: showId,
            addedAt: addedAt, addedBy: addedBy, playCount: playCount
        }
        if (pos >= 0)
            data.pos = pos
        return data
    }

    function openMenu(item, x, y) {
        const p = item.mapToItem(null, x, y)
        Catalog.requestTrackMenu(trackData(), p.x, p.y)
    }
    // меню у кнопки «⋯»: под ней, правым краем к её правому краю
    function openMenuAt(button) {
        const p = button.mapToItem(null, 0, 0)
        Catalog.requestTrackMenuAt(trackData(), p.x, p.y, button.width, button.height)
    }

    function _time(ms) {
        const s = Math.round(ms / 1000)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    function _date(iso) {
        if (!iso)
            return ""
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return ""
        const now = new Date()
        const day = 24 * 3600 * 1000
        const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
        if (d.getTime() >= start)
            return I18n.t("Today")
        if (d.getTime() >= start - day)
            return I18n.t("Yesterday")
        return I18n.shortDate(d)
    }

    readonly property bool _compactAdded: width < 600
    readonly property real _editT: editable ? 1 : 0

    // ---- Телефон: свайп вправо — «в очередь» (как в Spotify). Строка едет за пальцем, под ней значок;
    // после порога отпустить — трек встаёт в очередь (играет следующим после уже добавленных) ----
    property real _swipeX: 0
    readonly property real _queueAt: 88
    readonly property bool _queueArmed: _swipeX >= _queueAt
    on_QueueArmedChanged: if (queueSwipe.active) MobileShell.haptic()
    NumberAnimation {
        id: swipeBack
        target: root; property: "_swipeX"; to: 0
        duration: Theme.motion.spatialDefault
        easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }
    Item {
        id: queueReveal
        visible: root._swipeX > 0
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: Math.max(0, root._swipeX)
        clip: true
        Rectangle {
            anchors.fill: parent
            radius: Theme.shape.large
            color: root._queueArmed ? Theme.primary : Theme.surfaceContainerHighest
            Behavior on color { ColorAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            x: Math.min(24, Math.max(8, (parent.width - size) / 2))
            name: "add_to_queue"
            size: 24
            color: root._queueArmed ? Theme.fgPrimary : Theme.fgSurfaceVariant
            scale: root._queueArmed ? 1.15 : 1
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
        }
    }

    Rectangle {
        id: bg
        anchors.fill: parent
        transform: Translate { x: root._swipeX }
        radius: Theme.shape.large
        color: root.dragging ? Theme.surfaceContainerHighest
             : root.current && !root.editable ? Qt.alpha(Theme.primary, 0.12) : "transparent"
    }
    // тень поднятой строки (Elevation кладётся рядом с bg — внутрь строки, сдвигается вместе с ней)
    Elevation { target: bg; level: root.dragging ? 2 : 0 }

    // Вспышка и всё, что видно только в правке или под курсором, создаётся по требованию: строк в списке
    // десятки, а мигает, правится или наведена одна
    Loader {
        anchors.fill: parent
        active: root.highlighted
        sourceComponent: Rectangle {
            id: flash
            radius: Theme.shape.large
            color: Theme.primary
            opacity: 0
            SequentialAnimation {
                running: Theme.reducedMotion !== true
                loops: 2
                NumberAnimation { target: flash; property: "opacity"; to: 0.18; duration: Theme.motion.effectsDefault }
                NumberAnimation { target: flash; property: "opacity"; to: 0; duration: Theme.motion.effectsDefault * 2 }
            }
            // reduced motion — без вспышки: ровная подсветка, пока строка помечена
            states: State {
                when: Theme.reducedMotion === true
                PropertyChanges { flash.opacity: 0.12 }
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        transform: Translate { x: root._swipeX }
        anchors.leftMargin: (Theme.mobile ? 8 : 12) - 8 * root._editT
        anchors.rightMargin: Theme.mobile ? 0 : 16
        spacing: Theme.mobile ? 12 : 16
        // недоступный в регионе трек приглушён, как в очереди (QueueRow)
        opacity: root.available ? 1 : Theme.stateLayer.disabledContent

        // Ручка перетаскивания: ширина 0 → 28 при входе в правку
        Item {
            id: handle
            Layout.preferredWidth: root.editable ? 28 : 0
            Layout.rightMargin: root.editable ? 0 : -16
            Layout.fillHeight: true
            Behavior on Layout.preferredWidth { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            Behavior on Layout.rightMargin { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            // с первого входа в правку и остаётся (иконка гаснет при выходе)
            Loader {
                anchors.fill: parent
                active: root.editable || item !== null
                sourceComponent: Item {
                    Icon {
                        anchors.centerIn: parent
                        name: "drag_indicator"
                        size: 22
                        color: handleHover.hovered || root.dragging ? Theme.fgSurface : Theme.fgSurfaceVariant
                        opacity: root.editable ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    }
                    HoverHandler { id: handleHover; cursorShape: root.dragging ? Qt.ClosedHandCursor : Qt.OpenHandCursor; enabled: root.editable }
                    DragHandler {
                        enabled: root.editable
                        target: null
                        yAxis.enabled: true
                        xAxis.enabled: false
                        grabPermissions: PointerHandler.CanTakeOverFromAnything
                        onActiveChanged: active ? root.dragStarted() : root.dragFinished()
                        onTranslationChanged: if (active) root.dragMoved(translation.y, centroid.scenePosition.y)
                    }
                }
            }
        }

        // телефон: номер рядом с обложкой лишний — его 40 px нужнее названию (как в Spotify)
        Label {
            visible: root.number > 0 && !(Theme.mobile && root.showCover && root.cover !== "")
            Layout.preferredWidth: 28
            text: root.number
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }

        // выбранный трек ещё грузится — индикатор MD3 поверх обложки; строка уже переключилась на него
        Item {
            visible: root.showCover
            Layout.preferredWidth: 48
            Layout.preferredHeight: 48
            MorphImage {
                anchors.fill: parent
                source: root.cover
                fallbackKind: root.album !== "" ? "album" : "track"
                fallbackArtist: root.artists.split(", ")[0]
                fallbackTitle: root.album !== "" ? root.album : root.title
                shape: root.current && !root.editable ? "cookie9" : "softSquare"
            }
            LoadingIndicator {
                anchors.centerIn: parent
                size: 34
                color: Theme.primary
                visible: root.current && root.loading
                running: visible && root.visible
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.preferredWidth: 3
            spacing: 2
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)   // дробный масштаб: ширина в раскладке округляется вниз
                    text: root.title
                    type: "bodyLarge"
                    weight: 500
                    color: root.current ? Theme.primary : Theme.fgSurface
                }
                Label {
                    visible: root.version !== ""
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)
                    text: root.version
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
                ExplicitMark { visible: root.explicit && !Theme.mobile }
                Item { Layout.fillWidth: true }
            }
            // телефон: «E» — перед исполнителями, как в Spotify: строке названия нужна вся ширина
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                ExplicitMark { visible: root.explicit && Theme.mobile }
                ArtistLinks {
                    Layout.fillWidth: true
                    refs: root.artistRefs ?? []
                    text: root.artists
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
            }
        }

        LinkLabel {
            visible: root._albumT > 0.01
            opacity: root._albumT
            Layout.fillWidth: true
            Layout.preferredWidth: 2 * root._albumT
            linkEnabled: root.albumId !== ""
            onClicked: Catalog.openAlbum(root.albumId)
            text: root.album
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }

        // «Добавил» (аватар + имя) / «Добавлено» (дата); в узкой строке — только аватар, а дату прячем:
        // место нужнее названию
        RowLayout {
            visible: root.addedColumn !== "" && !(root.addedColumn === "at" && root._compactAdded)
            Layout.preferredWidth: root.addedColumn === "by" && root._compactAdded ? 28 : 132
            Layout.maximumWidth: Layout.preferredWidth
            spacing: 8
            Avatar {
                visible: root.addedColumn === "by" && root.addedBy !== ""
                size: 24
                name: root.addedBy
            }
            Label {
                visible: !(root.addedColumn === "by" && root._compactAdded)
                Layout.fillWidth: true
                text: root.addedColumn === "by" ? root.addedBy : root._date(root.addedAt)
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
            }
        }

        // Прослушивания: полное число с разделителями разрядов
        Label {
            visible: root.showPlays && root._playsRoom
            Layout.preferredWidth: 112
            text: root.playCount > 0 ? I18n.grouped(root.playCount) : ""
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
            font.features: ({ "tnum": 1 })
        }

        // Сердце и ⋯ — в просмотре; «убрать» — в правке (перекрёстное растворение)
        Item {
            // телефон: «⋯» видна всегда (наведения нет), сердце — только у лайкнутых, кнопка «⋯» крупнее
            readonly property bool heartShown: !Theme.mobile || root.liked
            implicitWidth: root.editable ? 32 : Theme.mobile ? (heartShown ? 32 + 4 : 0) + 40 : 32 + 16 + 32
            implicitHeight: Theme.mobile ? 40 : 32

            Row {
                visible: opacity > 0
                opacity: 1 - root._editT
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                spacing: Theme.mobile ? 4 : 16
                anchors.verticalCenter: parent.verticalCenter

                // Сердце: залитое у лайкнутых, контур — при наведении на строку
                Item {
                    visible: parent.parent.heartShown
                    width: 32
                    height: 32
                    anchors.verticalCenter: parent.verticalCenter
                    Icon {
                        anchors.centerIn: parent
                        visible: root.available
                        name: "favorite"
                        size: 20
                        fill: root.liked ? 1 : 0
                        color: root.liked ? Theme.primary : heartArea.containsMouse ? Theme.fgSurface : Theme.fgSurfaceVariant
                        opacity: root.liked || rowHover.hovered ? 1 : 0
                        scale: heartArea.pressed ? 0.85 : 1
                        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                    }
                    StateLayer {
                        id: heartArea
                        anchors.fill: parent
                        radius: width / 2
                        visible: root.available && !root.editable && (root.liked || rowHover.hovered)
                        onClicked: root.likeClicked()
                    }
                }
                // «⋯» — с первого наведения на строку
                Loader {
                    width: Theme.mobile ? 40 : 32
                    height: Theme.mobile ? 40 : 32
                    active: Theme.mobile || rowHover.hovered || item !== null
                    sourceComponent: IconButton {
                        id: moreButton
                        icon: Theme.mobile ? "more_vert" : "more_horiz"
                        size: Theme.mobile ? "s" : "xs"
                        iconSize: Theme.mobile ? 24 : 26
                        iconWeight: Theme.mobile ? 500 : 700
                        enabled: !root.editable
                        opacity: Theme.mobile || rowHover.hovered || pressed ? 1 : 0
                        onClicked: root.openMenuAt(moreButton)
                    }
                }
            }
            // «убрать» — с первого входа в правку
            Loader {
                anchors.right: parent.right
                width: 32
                height: 32
                active: root.editable || item !== null
                visible: opacity > 0
                opacity: root._editT
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                sourceComponent: IconButton {
                    icon: "remove_circle"
                    size: "xs"
                    iconColor: Theme.fgSurfaceVariant
                    onClicked: root.removeClicked()
                }
            }
        }

        Label {
            // телефон: длительность — в меню «О треке»; место нужнее названию
            visible: !Theme.mobile
            Layout.preferredWidth: 44
            text: root._time(root.durationMs)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }
    }

    HoverHandler { id: rowHover }

    // Слой поверх строки: нажатие видит первым и держит пассивно (тап и кнопки работают), жест забирает,
    // когда палец поехал вправо; вертикальный — остаётся списку
    Item {
        anchors.fill: parent
        z: 10
        DragHandler {
            id: queueSwipe
            target: null
            enabled: Theme.mobile && !root.editable && root.available && root.uri !== ""
            yAxis.enabled: false
            // translation обнуляется раньше, чем приходит active=false: решение — по _swipeX
            onTranslationChanged: {
                if (!active)
                    return
                const x = Math.max(0, translation.x)
                // за порогом — с сопротивлением
                root._swipeX = x <= root._queueAt ? x : root._queueAt + (x - root._queueAt) * 0.3
            }
            onActiveChanged: {
                if (active)
                    swipeBack.stop()
            }
            onGrabChanged: transition => {
                const released = transition === PointerDevice.UngrabExclusive
                if (!released && transition !== PointerDevice.CancelGrabExclusive)
                    return
                if (released && root._queueArmed)
                    Player.addToQueue(root.trackData())
                swipeBack.restart()
            }
        }
    }

    // Перетаскивание в очередь (вне правки)
    QueueDragSource {
        anchors.fill: parent
        z: -2
        source: root
        enabled: !root.editable && root.available && root.uri !== ""
        title: root.title
        cover: root.cover
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad   // касание — не правый клик: у него нет кнопок, TapHandler принял бы любой тап
        onTapped: (point) => root.openMenu(root, point.position.x, point.position.y)
    }

    // Под содержимым: клики по сердцу достаются ему, по остальной строке — сюда
    StateLayer {
        z: -1
        anchors.fill: parent
        radius: Theme.shape.large
        dragged: root.dragging
        onClicked: if (!root.editable) root.activated()
        // телефон: долгое нажатие — меню трека (на десктопе — правая кнопка)
        onPressAndHold: mouse => {
            if (Theme.mobile && !root.editable) {
                MobileShell.haptic()
                root.openMenu(root, mouse.x, mouse.y)
            } else {
                mouse.accepted = false
            }
        }
    }
}
