import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Страница со списком треков (виртуализированный ListView): шапка прокручивается вместе со списком.
// Поиск по списку (кнопка в шапке или Ctrl+F): пока в поле есть текст, список показывает найденное,
// и «Слушать», «Перемешать», клик по треку ставят в очередь именно найденное.
// editing — правка своего плейлиста: строки с ручкой и «убрать», перестановка (moveRequested),
// шапка — editHero вместо обычной.
Item {
    id: page

    property var router
    property string title
    property string subtitle
    property string description
    property bool showHeader: true
    property var model: null                  // DictListModel треков
    property string emptyIcon: "music_note"
    property string emptyTitle: I18n.t("Nothing here")
    property string emptyText: ""
    property string emptyActionText: ""       // кнопка под пустым списком (например, «Повторить»)
    property string emptyActionIcon: ""
    signal emptyAction()
    property Component headerExtra: null       // доп. содержимое шапки (кнопки и т. п.)
    property Component heroMeta: null          // строка под подзаголовком (владелец, бейджи)
    property Component heroNote: null          // подсказка под кнопками
    property Component footerExtra: null       // под списком (например, «+ Добавить треки»)
    // Шапка-«герой»: обложка + надзаголовок + название + «N треков · длительность» + Слушать/Перемешать
    property string overline                   // «ПЛЕЙЛИСТ», «КОЛЛЕКЦИЯ»
    property string heroImage                  // обложка (url, лучше coverLarge); если пусто — фигура с иконкой
    property string heroIcon: "queue_music"
    property var heroShape: "cookie9"
    property var heroArtistRefs: []            // исполнители-ссылки под названием (альбом)
    property bool showPlayButtons: true
    readonly property bool scrolled: list.contentY - list.originY > 48

    // Правка плейлиста
    property bool editing: false
    property Component editHero: null
    property string addedColumn                // "" / "by" / "at" — колонка «Добавил»/«Добавлено»
    property bool showPlays: false             // колонка прослушиваний (альбом)
    property bool showAlbum: true              // колонка альбома (на странице альбома не нужна)
    property bool rowCovers: true              // обложки в строках (у альбома на телефоне — номер вместо одинаковых обложек)
    property real contentRightInset: 0         // место справа под боковую панель (AddTracksPanel)
    Behavior on contentRightInset { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
    readonly property alias listView: list

    property bool searchOpen: false
    property string searchText
    // запрос уходит в фильтр с задержкой: полный сброс модели на каждый символ дороги
    onSearchTextChanged: searchDebounce.restart()
    readonly property var shownModel: model ? filter : null   // то, что в списке: всё или найденное

    signal trackActivated(int row)
    signal trackRemoved(int row)               // «убрать» в режиме правки (row — индекс в model)
    signal moveRequested(int from, int to)

    onEditingChanged: if (editing) closeSearch()

    // Прокрутить к треку и мигнуть им, когда список загрузится (один раз)
    property string highlightTrackId
    property string _flashId
    property int _flashRow: -1
    function _revealTrack() {
        if (highlightTrackId === "" || !shownModel || shownModel.count === 0)
            return
        for (let i = 0; i < shownModel.count; ++i) {
            if (shownModel.get(i).trackId === highlightTrackId) {
                list.userScrolled = true
                list.positionViewAtIndex(i, ListView.Center)
                _flashId = highlightTrackId
                highlightTrackId = ""
                flashReset.restart()
                return
            }
        }
    }
    // Мигнуть строкой по индексу (после добавления в конец / возврата удалённой)
    function flashRow(row) {
        if (row < 0)
            return
        _flashRow = row
        list.userScrolled = true
        list.positionViewAtIndex(row, ListView.Contain)
        flashReset.restart()
    }
    onHighlightTrackIdChanged: Qt.callLater(_revealTrack)
    Connections {
        target: page.shownModel
        ignoreUnknownSignals: true
        function onCountChanged() { Qt.callLater(page._revealTrack) }
        function onModelReset() { Qt.callLater(page._revealTrack) }
    }
    Timer { id: flashReset; interval: Theme.reducedMotion === true ? 1000 : 1600; onTriggered: { page._flashId = ""; page._flashRow = -1 } }

    // Ctrl+F: открыть поиск по списку. false — искать тут нечего или поле уже в фокусе
    // (тогда повторный Ctrl+F уходит в общий поиск).
    function findInPage() {
        if (!model || model.count === 0 || !showHeader || editing)
            return false
        const field = list.headerItem ? list.headerItem.searchField : null
        if (searchOpen && field && field.inputItem.activeFocus)
            return false
        searchOpen = true
        list.userScrolled = false
        list.positionViewAtBeginning()
        Qt.callLater(() => list.headerItem && list.headerItem.searchField && list.headerItem.searchField.inputItem.forceActiveFocus())
        return true
    }
    function closeSearch() {
        searchOpen = false
        searchText = ""
    }
    // «Назад» системы (телефон): сначала закрыть поиск по списку; true — обработано здесь
    function handleBack() {
        if (!searchOpen)
            return false
        closeSearch()
        return true
    }

    TrackFilter {
        id: filter
        source: page.model
        // query ставит searchDebounce: привязка к page.searchText сбрасывала модель на каждый символ
    }
    Timer {
        id: searchDebounce
        interval: 200
        onTriggered: filter.query = page.searchText
    }

    function _duration(ms) {
        const m = Math.round(ms / 60000)
        return m >= 60 ? I18n.t("{0} h {1} min", Math.floor(m / 60), m % 60) : I18n.t("{0} min", m)
    }

    // Медленно плывущие фигуры за шапкой. Слой вне ListView (иначе его clip режет фигуры по краю
    // списка) и заходит под прозрачную верхнюю панель; сверху и снизу фигуры растворяются,
    // поэтому видимой рамки нет. Едет вместе с шапкой, стоит, когда она уехала из виду.
    // Декоративное движение шапки стартует, когда страница доехала и список встал: открытие и так
    // самый тяжёлый момент (создание страницы и строк, переход), не добавляем к нему перерисовки
    property bool _settled: false
    Timer { interval: 900; running: page.visible && !page._settled; onTriggered: page._settled = true }

    HeroShapes {
        width: page.width - page.contentRightInset
        // телефон: на узком экране фигуры за крупной обложкой — серое пятно, а не фон
        active: page.showHeader && page.visible && Theme.reducedMotion !== true && !Theme.mobile
        running: page._settled
        headerHeight: list.headerItem ? list.headerItem.height : 320
        scroll: list.contentY - list.originY
        contentX: list.x
        contentWidth: list.width
        seed: page.title
    }

    ReorderListView {
        id: list
        anchors.fill: parent
        anchors.leftMargin: Theme.mobile ? 8 : 16
        anchors.rightMargin: (Theme.mobile ? 8 : 16) + page.contentRightInset
        model: page.shownModel
        clip: true
        boundsBehavior: Theme.mobile ? Flickable.OvershootBounds : Flickable.StopAtBounds
        acceptedButtons: Theme.mobile ? Qt.LeftButton : Qt.NoButton
        reuseItems: true
        cacheBuffer: 640
        reorderable: page.editing && !filter.active
        interactive: !dragActive
        onMoveRequested: (from, to) => page.moveRequested(from, to)

        // удаление в правке: строка гаснет, соседи подтягиваются
        remove: Transition {
            enabled: page.editing && Theme.reducedMotion !== true
            NumberAnimation { property: "opacity"; to: 0; duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic }
        }
        removeDisplaced: Transition {
            enabled: page.editing && Theme.reducedMotion !== true
            NumberAnimation { property: "y"; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }

        // ListView не сдвигает contentY, когда шапка меняет высоту (подзаголовок «N треков»
        // появляется после загрузки) или модель сбрасывается, — и заголовок оказывается обрезан.
        // Пока пользователь не прокручивал, держим список у самого верха.
        property bool userScrolled: false
        function keepTop() { if (!userScrolled) Qt.callLater(() => contentY = originY) }
        onMovementStarted: userScrolled = true   // касание
        Connections {
            target: page.shownModel
            ignoreUnknownSignals: true
            function onModelReset() { if (!list._settling && list.dragFrom < 0) { list.userScrolled = false; list.keepTop() } }
        }

        header: Item {
            id: headerItem
            readonly property var searchField: heroLoader.item ? heroLoader.item.searchField ?? null : null
            width: list.width
            implicitHeight: (page.showHeader ? heroLoader.implicitHeight : 0) + 56
            onImplicitHeightChanged: list.keepTop()

            Loader {
                id: heroLoader
                visible: page.showHeader
                x: Theme.mobile ? 8 : 16
                y: 24     // запас сверху: фигура за обложкой больше неё и не должна упираться в край списка
                width: parent.width - 2 * x
                sourceComponent: page.editing && page.editHero ? page.editHero : defaultHero
            }
        }

        delegate: TrackRow {
            id: row
            required property int index
            required property var model
            width: ListView.view.width
            number: index + 1
            trackId: model.trackId
            title: model.title
            version: model.version
            artists: model.artists
            artistRefs: model.artistRefs
            albumId: model.albumId
            album: model.album
            cover: model.cover
            showCover: page.rowCovers
            coverLarge: model.coverLarge ?? ""
            uri: model.uri ?? ""
            isEpisode: model.isEpisode ?? false
            showId: model.showId ?? ""
            durationMs: model.durationMs
            explicit: model.explicit
            available: model.available
            addedAt: model.addedAt ?? ""
            addedBy: model.addedBy ?? ""
            addedColumn: page.addedColumn
            playCount: model.playCount ?? 0
            showPlays: page.showPlays
            showAlbum: page.showAlbum
            editable: list.reorderable || (page.editing && !filter.active)
            dragging: list.dragFrom === index
            liked: Library.likesRevision >= 0 && Library.isLiked(model.trackId)
            current: model.trackId === Player.trackId
            highlighted: (page._flashId !== "" && model.trackId === page._flashId) || page._flashRow === index
            // pos — позиция в самом списке (для «Убрать из этого плейлиста»); при поиске неизвестна.
            // Словарь для меню TrackRow собирает сам, лениво (trackData()) — get(index) целиком
            // (18 ролей в QVariantMap на каждую строку) здесь больше не нужен
            pos: filter.active ? -1 : index
            z: dragging ? 10 : 0
            transform: Translate {
                y: row.dragging ? list.dragOffset : list.shiftFor(row.index)
                Behavior on y {
                    enabled: !row.dragging && list.shiftAnimated && Theme.reducedMotion !== true
                    NumberAnimation { duration: Theme.motion.effectsSlow; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
                }
            }
            onActivated: {
                page.trackActivated(index)
                Player.playFrom(page.shownModel, index)
            }
            onLikeClicked: Library.toggleLike(page.shownModel.get(index))
            onRemoveClicked: page.trackRemoved(index)
            onDragStarted: list.beginDrag(index)
            onDragMoved: (dy, sceneY) => list.updateDrag(dy, sceneY)
            onDragFinished: list.endDrag()
        }

        footer: Item {
            id: footerBox
            width: list.width
            height: (Theme.mobile ? 96 : 120) + (page.model && page.model.loading && page.model.count > 0 ? 64 : 0) + extraLoader.height
            Loader {
                id: extraLoader
                width: parent.width
                active: page.footerExtra !== null
                sourceComponent: page.footerExtra
            }
            LoadingIndicator {
                anchors.horizontalCenter: parent.horizontalCenter
                y: extraLoader.height
                visible: page.model && page.model.loading && page.model.count > 0
                // Длинный плейлист грузится страницами секундами; крутящийся индикатор перерисовывает окно
                // с частотой монитора, поэтому крутится, только когда низ списка на экране
                running: footerBox.y < list.contentY + list.height
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                y: extraLoader.height + 8
                width: Math.min(parent.width - 32, 520)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                visible: filter.active && filter.count === 0 && !(page.model && page.model.loading)
                // цитируем filter.query, а не searchText: текст должен совпадать с тем, что реально отражает список
                text: I18n.t("Nothing in this list matches “{0}”", filter.query.trim())
                type: "bodyLarge"
                color: Theme.fgSurfaceVariant
            }
        }
    }

    SmoothScroll { flickable: list; wheelStep: Settings.wheelStep; onUserScrolled: list.userScrolled = true }

    Component {
        id: defaultHero
        // Телефон: обложка крупно по центру, под ней подписи и кнопки; десктоп — обложка слева от подписей
        GridLayout {
            id: hero
            readonly property alias searchField: searchField
            columns: Theme.mobile ? 1 : 2
            rowSpacing: 20
            columnSpacing: page.width < 560 ? 18 : 28
            Behavior on columnSpacing { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }

            // Обложка или фигура с иконкой; за обложкой — фигура побольше
            Item {
                Layout.preferredWidth: Theme.mobile ? Math.round(Math.min(232, page.width * 0.56))
                                     : page.width > 760 ? 184 : page.width < 560 ? 96 : 128
                Layout.preferredHeight: Layout.preferredWidth
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Layout.alignment: Theme.mobile ? Qt.AlignHCenter : Qt.AlignTop
                // фигура за обложкой на 30 % больше — ей нужно место сверху
                Layout.topMargin: Theme.mobile ? 20 : 0
                MorphShape {
                    id: coverBackdrop
                    visible: page.heroImage !== ""
                    anchors.centerIn: parent
                    width: parent.width * 1.3
                    height: width
                    shape: "cookie9"
                    color: Theme.surfaceContainerHighest
                }
                TickAnimation {
                    running: coverBackdrop.visible && page.visible && page._settled && !Theme.quiet && Theme.reducedMotion !== true
                             && list.contentY - list.originY < 300
                    onTriggered: coverBackdrop.rotation = (coverBackdrop.rotation + frameTime * 6) % 360
                }
                MorphImage {
                    anchors.fill: parent
                    visible: page.heroImage !== ""
                    source: page.heroImage
                    shape: coverHover.hovered ? "cookie12" : "cookie9"
                    HoverHandler { id: coverHover }
                }
                MorphShape {
                    anchors.fill: parent
                    visible: page.heroImage === ""
                    shape: page.heroShape
                    color: Theme.primaryContainer
                    Icon {
                        anchors.centerIn: parent
                        name: page.heroIcon
                        size: parent.width * 0.38
                        fill: 1
                        color: Theme.fgPrimaryContainer
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Theme.mobile ? Qt.AlignTop : Qt.AlignBottom
                Layout.topMargin: Theme.mobile ? 8 : 0
                spacing: 4
                Label {
                    visible: page.overline !== ""
                    text: page.overline
                    type: "labelLarge"
                    color: Theme.primary
                    font.letterSpacing: 1.2
                }
                Label {
                    Layout.fillWidth: true
                    text: page.title
                    type: page.width > 760 ? "displayMedium" : page.width < 560 ? "headlineMedium" : "displaySmall"
                    weight: 700
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                }
                Label {
                    visible: page.description !== ""
                    Layout.fillWidth: true
                    text: page.description
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }
                Flow {
                    visible: (page.heroArtistRefs ?? []).length > 0
                    Layout.fillWidth: true
                    spacing: 6
                    Repeater {
                        model: page.heroArtistRefs
                        LinkLabel {
                            required property var modelData
                            required property int index
                            text: modelData.name + (index < page.heroArtistRefs.length - 1 ? "," : "")
                            type: "titleLarge"
                            weight: 600
                            onClicked: Catalog.openArtist(modelData.id)
                        }
                    }
                }
                Label {
                    visible: text !== "" && page.heroMeta === null
                    Layout.fillWidth: true
                    text: page.subtitle + (page.model && page.model.totalDurationMs > 0 && page.subtitle !== ""
                                           ? "  ·  " + page._duration(page.model.totalDurationMs) : "")
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
                Loader {
                    Layout.fillWidth: true
                    active: page.heroMeta !== null
                    visible: active
                    sourceComponent: page.heroMeta
                }
                // в узком окне кнопки переносятся, а не распирают шапку
                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.mobile ? 10 : 14
                    spacing: Theme.mobile ? 8 : 12
                    visible: page.model !== null && (page.model.count > 0 || page.headerExtra !== null)
                    Button {
                        visible: page.showPlayButtons
                        text: I18n.t("Play")
                        icon: "play_arrow"
                        size: Theme.mobile ? "s" : "m"
                        enabled: page.shownModel !== null && page.shownModel.count > 0
                        onClicked: Player.playFrom(page.shownModel, 0)
                    }
                    Button {
                        visible: page.showPlayButtons && !Theme.mobile
                        text: I18n.t("Shuffle play")
                        icon: "shuffle"
                        style: "tonal"
                        size: "m"
                        enabled: page.shownModel !== null && page.shownModel.count > 0
                        onClicked: Player.shuffleFrom(page.shownModel)
                    }
                    // телефон: перемешивание — круглой кнопкой, иначе «Сохранить» и поиск уезжали на вторую строку
                    IconButton {
                        visible: page.showPlayButtons && Theme.mobile
                        Accessible.name: I18n.t("Shuffle play")
                        icon: "shuffle"
                        style: "tonal"
                        size: "s"
                        enabled: page.shownModel !== null && page.shownModel.count > 0
                        onClicked: Player.shuffleFrom(page.shownModel)
                    }
                    Loader {
                        active: page.headerExtra !== null
                        sourceComponent: page.headerExtra
                    }
                    IconButton {
                        visible: page.model !== null && page.model.count > 0
                        icon: "search"
                        size: Theme.mobile ? "s" : "m"
                        style: page.searchOpen ? "tonal" : "standard"
                        checkable: true
                        autoToggle: false
                        checked: page.searchOpen
                        onClicked: page.searchOpen ? page.closeSearch() : page.findInPage()
                    }
                }
                Loader {
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    active: page.heroNote !== null
                    visible: active
                    sourceComponent: page.heroNote
                }
                RowLayout {
                    visible: page.searchOpen
                    Layout.fillWidth: true
                    Layout.topMargin: 10
                    spacing: 16
                    SearchField {
                        id: searchField
                        Layout.fillWidth: true
                        Layout.maximumWidth: 420
                        placeholder: I18n.t("Find in list")
                        text: page.searchText
                        onTextChanged: page.searchText = text
                        Keys.onEscapePressed: page.closeSearch()
                    }
                    Label {
                        visible: filter.active
                        text: I18n.t("{0} of {1}", filter.count, page.model ? page.model.count : 0)
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                    Item { Layout.fillWidth: true }
                }
            }
        }
    }

    // Первая загрузка / пусто
    LoadingIndicator {
        anchors.centerIn: parent
        size: 64
        contained: true
        visible: page.model !== null && page.model.loading && page.model.count === 0
    }
    EmptyState {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.horizontalCenterOffset: -page.contentRightInset / 2
        y: Math.max((list.headerItem ? list.headerItem.height - (list.contentY - list.originY) : 0) + 8,
                    (parent.height - implicitHeight) / 2)
        width: Math.min(parent.width - (Theme.mobile ? 32 : 64) - page.contentRightInset, 420)
        visible: page.model !== null && !page.model.loading && page.model.count === 0 && page.footerExtra === null
        icon: page.emptyIcon
        title: page.emptyTitle
        text: page.emptyText
        actionText: page.emptyActionText
        actionIcon: page.emptyActionIcon
        onAction: page.emptyAction()
    }
}
