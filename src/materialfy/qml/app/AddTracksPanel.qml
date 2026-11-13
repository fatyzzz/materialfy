import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// «Добавить треки» в свой плейлист: справа 328 px (поверхность как у правой панели), в узком окне — шторка
// снизу на 72 % высоты над затемнением. Источники: поиск по Spotify, «Недавние» (Library.recent),
// «Любимые» (Library.liked). Трек добавляется в конец (Library.addTracks); дубликат помечен «уже есть»,
// добавить его можно ещё раз. added(track) — странице: мигнуть строкой и предложить «Отменить».
Item {
    id: root

    required property string playlistId
    property string playlistTitle
    property var playlistModel: null        // треки плейлиста — для пометки «уже есть»
    property bool opened: false
    property bool compact: false
    property real bottomInset: 104          // над плавающим мини-плеером
    readonly property real panelWidth: 328

    signal closeRequested()
    signal added(var track)

    property string source: "recent"        // recent | liked
    property string query
    property var _addedNow: ({})            // uri → true: добавлено из панели в этот раз
    property var _present: ({})             // uri → true: уже в плейлисте

    readonly property bool searching: query.trim() !== ""
    readonly property var listModel: searching ? Catalog.searchTracks
        : source === "liked" ? Library.liked
        : (Library.recent ? Library.recent : null)

    function focusSearch() { search.inputItem.forceActiveFocus() }

    function _rebuildPresent() {
        const set = {}
        const m = root.playlistModel
        for (let i = 0; m && i < m.count; i++) {
            const u = m.get(i).uri
            if (u)
                set[u] = true
        }
        _present = set
    }
    Connections {
        target: root.playlistModel
        ignoreUnknownSignals: true
        function onCountChanged() { root._rebuildPresent() }
        function onModelReset() { root._rebuildPresent() }
    }
    onPlaylistModelChanged: _rebuildPresent()
    onOpenedChanged: if (opened) { _addedNow = ({}); _rebuildPresent() }

    Timer {
        id: searchDelay
        interval: 300
        onTriggered: if (root.searching) Catalog.search(root.query.trim(), "tracks")
    }
    onQueryChanged: searchDelay.restart()

    function _add(track) {
        if (!track || !track.uri)
            return
        Library.addTracks(root.playlistId, [track.uri])
        const a = Object.assign({}, _addedNow)
        a[track.uri] = true
        _addedNow = a
        root.added(track)
    }

    visible: opened || sheet.opacity > 0

    // Затемнение под шторкой (только в узком окне)
    Rectangle {
        anchors.fill: parent
        visible: root.compact && opacity > 0
        color: Theme.scrim
        opacity: root.opened && root.compact ? 0.32 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onClicked: root.closeRequested(); onWheel: w => w.accepted = true }
    }

    Rectangle {
        id: sheet
        readonly property real shift: root.opened ? 0 : (root.compact ? 24 : 16)
        width: root.compact ? parent.width : root.panelWidth
        height: root.compact ? parent.height * (Theme.mobile ? 0.8 : 0.72) : parent.height - root.bottomInset - 8
        x: root.compact ? 0 : parent.width - width - 8 + shift
        y: root.compact ? parent.height - height + shift : 8
        Behavior on x { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
        Behavior on y { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
        opacity: root.opened ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.reducedMotion === true ? 0 : Theme.motion.effectsDefault } }
        radius: Theme.shape.extraLarge
        bottomLeftRadius: root.compact ? 0 : Theme.shape.extraLarge
        bottomRightRadius: root.compact ? 0 : Theme.shape.extraLarge
        color: Theme.surfaceContainerLow

        Elevation { target: sheet; level: root.compact ? 3 : 1 }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: w => w.accepted = true }

        ColumnLayout {
            anchors.fill: parent
            anchors.topMargin: 16
            // телефон: над полосой навигации системы
            anchors.bottomMargin: Theme.mobile ? Theme.safeBottom : 0
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 20
                Layout.rightMargin: 8
                spacing: 8
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Label { text: I18n.t("Add tracks"); type: "titleMedium" }
                    Label {
                        Layout.fillWidth: true
                        text: I18n.t("to “{0}” · at the end", root.playlistTitle)
                        type: "bodySmall"
                        color: Theme.fgSurfaceVariant
                    }
                }
                IconButton { icon: "close"; onClicked: root.closeRequested() }
            }

            SearchField {
                id: search
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                implicitHeight: 40
                placeholder: I18n.t("Find a track on Spotify")
                text: root.query
                onTextChanged: root.query = text
                Keys.onEscapePressed: root.query !== "" ? (root.query = "") : root.closeRequested()
            }

            Row {
                Layout.leftMargin: 16
                spacing: 8
                Chip { text: I18n.t("Recent"); selected: !root.searching && root.source === "recent"; onClicked: { root.query = ""; root.source = "recent" } }
                Chip { text: I18n.t("Liked"); selected: !root.searching && root.source === "liked"; onClicked: { root.query = ""; root.source = "liked" } }
            }

            ListView {
                id: results
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.leftMargin: 8
                Layout.rightMargin: 8
                clip: true
                model: root.listModel
                boundsBehavior: Theme.mobile ? Flickable.OvershootBounds : Flickable.StopAtBounds
                acceptedButtons: Theme.mobile ? Qt.LeftButton : Qt.NoButton
                reuseItems: true
                delegate: Item {
                    id: row
                    required property int index
                    required property var model
                    readonly property string uri: model.uri ?? ""
                    readonly property bool present: root._present[uri] === true
                    readonly property bool addedNow: root._addedNow[uri] === true
                    width: ListView.view.width
                    height: 56
                    opacity: model.available === false ? Theme.stateLayer.disabledContent : 1

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 4
                        spacing: 12
                        MorphImage {
                            Layout.preferredWidth: 40
                            Layout.preferredHeight: 40
                            source: row.model.cover ?? ""
                            shape: rowHover.hovered ? "cookie9" : "softSquare"
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Label { Layout.fillWidth: true; text: row.model.title ?? ""; type: "bodyMedium"; weight: 500 }
                            Label { Layout.fillWidth: true; text: row.model.artists ?? ""; type: "bodySmall"; color: Theme.fgSurfaceVariant }
                        }
                        Label {
                            visible: row.present && !row.addedNow
                            text: I18n.t("already added")
                            type: "labelSmall"
                            color: Theme.tertiary
                        }
                        IconButton {
                            icon: row.addedNow ? "check" : "add"
                            size: "xs"
                            style: row.addedNow ? "filled" : row.present ? "standard" : "tonal"
                            enabled: row.uri !== "" && row.model.available !== false
                            onClicked: root._add(results.model.get(row.index))
                        }
                    }
                    HoverHandler { id: rowHover }
                }
                SmoothScroll { flickable: results; wheelStep: Settings.wheelStep }

                Column {
                    anchors.centerIn: parent
                    width: parent.width - 32
                    spacing: 8
                    visible: results.count === 0 && !(root.listModel && root.listModel.loading)
                             && !(root.searching && (Catalog.searchState === "loading" || searchDelay.running))
                    Icon { anchors.horizontalCenter: parent.horizontalCenter; name: root.searching ? "search_off" : "music_note"; size: 32; color: Theme.fgSurfaceVariant }
                    Label {
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                        text: root.searching ? I18n.t("Nothing found")
                            : root.source === "liked" ? I18n.t("No Liked Songs yet") : I18n.t("Recently played tracks will appear here")
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                }
                LoadingIndicator {
                    anchors.centerIn: parent
                    visible: results.count === 0 && ((root.listModel && root.listModel.loading)
                             || (root.searching && (Catalog.searchState === "loading" || searchDelay.running)))
                }
            }
        }
    }
}
