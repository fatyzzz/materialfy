import QtQuick
import Md3
import Materialfy.Core

// Исполнители через запятую: каждый — своя ссылка. Наведение на имя показывает карточку этого
// исполнителя, клик открывает его страницу — без меню выбора, когда исполнителей несколько.
// Если имена не влезают в строку, остаётся прежняя подпись с «…»: резать имя пополам хуже.
Item {
    id: root

    // [{ id, name }]; пусто — просто подпись text (выпуск, старые строки без ссылок)
    property var refs: []
    property string text: ""
    property string type: "bodyMedium"
    property color color: Theme.fgSurfaceVariant
    property bool centered: false

    readonly property var _refs: refs ?? []
    readonly property bool fits: width <= 0 || _names.advanceWidth <= width
    // имена из refs должны совпадать с подписью: иначе показываем строку как есть, без ссылок
    readonly property bool _matches: text === "" || _names.text === text
    readonly property bool linked: _refs.length > 0 && fits && _matches
    // ширина содержимого — без оглядки на width: иначе в раскладке получается петля биндингов
    readonly property real naturalWidth: _refs.length > 0 ? _names.advanceWidth : _label.advanceWidth

    implicitWidth: naturalWidth
    implicitHeight: Math.ceil(_probe.implicitHeight)
    clip: true

    Label {
        id: _probe
        visible: false
        text: "X"
        type: root.type
    }
    TextMetrics {
        id: _names
        font: _probe.font
        text: root._refs.map(r => r.name ?? "").join(", ")
    }
    TextMetrics {
        id: _label
        font: _probe.font
        text: root.text
    }

    Label {
        anchors.fill: parent
        visible: !root.linked
        text: root.text
        type: root.type
        color: root.color
        elide: Text.ElideRight
    }

    Row {
        visible: root.linked
        anchors.top: parent.top
        anchors.left: root.centered ? undefined : parent.left
        anchors.horizontalCenter: root.centered ? parent.horizontalCenter : undefined
        spacing: 0

        Repeater {
            model: root._refs

            Row {
                required property var modelData
                required property int index

                Label {
                    visible: index > 0
                    text: ", "
                    type: root.type
                    color: root.color
                }
                LinkLabel {
                    text: modelData.name ?? ""
                    type: root.type
                    color: root.color
                    linkEnabled: (modelData.id ?? "") !== ""
                    previewArtistId: modelData.id ?? ""
                    onClicked: Catalog.openArtist(modelData.id ?? "")
                }
            }
        }
    }
}
