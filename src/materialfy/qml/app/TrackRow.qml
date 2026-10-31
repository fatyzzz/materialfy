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

    Rectangle {
        id: bg
        anchors.fill: parent
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
        anchors.leftMargin: 12 - 8 * root._editT
        anchors.rightMargin: 16
        spacing: 16
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

        Label {
            visible: root.number > 0
            Layout.preferredWidth: 28
            text: root.number
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }

        MorphImage {
            Layout.preferredWidth: 48
            Layout.preferredHeight: 48
            source: root.cover
            fallbackKind: root.album !== "" ? "album" : "track"
            fallbackArtist: root.artists.split(", ")[0]
            fallbackTitle: root.album !== "" ? root.album : root.title
            shape: root.current && !root.editable ? "cookie9" : "softSquare"
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
                Rectangle {
                    visible: root.explicit
                    implicitWidth: 16
                    implicitHeight: 16
                    radius: 4
                    color: Theme.surfaceContainerHighest
                    Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
                }
                Item { Layout.fillWidth: true }
            }
            ArtistLinks {
                Layout.fillWidth: true
                refs: root.artistRefs ?? []
                text: root.artists
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
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
            implicitWidth: root.editable ? 32 : 32 + 16 + 32
            implicitHeight: 32

            Row {
                visible: opacity > 0
                opacity: 1 - root._editT
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                spacing: 16

                // Сердце: залитое у лайкнутых, контур — при наведении на строку
                Item {
                    width: 32
                    height: 32
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
                    width: 32
                    height: 32
                    active: rowHover.hovered || item !== null
                    sourceComponent: IconButton {
                        id: moreButton
                        icon: "more_horiz"
                        size: "xs"
                        iconSize: 26
                        iconWeight: 700
                        enabled: !root.editable
                        opacity: rowHover.hovered || pressed ? 1 : 0
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
            Layout.preferredWidth: 44
            text: root._time(root.durationMs)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }
    }

    HoverHandler { id: rowHover }

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
        onTapped: (point) => root.openMenu(root, point.position.x, point.position.y)
    }

    // Под содержимым: клики по сердцу достаются ему, по остальной строке — сюда
    StateLayer {
        z: -1
        anchors.fill: parent
        radius: Theme.shape.large
        dragged: root.dragging
        onClicked: if (!root.editable) root.activated()
    }
}
