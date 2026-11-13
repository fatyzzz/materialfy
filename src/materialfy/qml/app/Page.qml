import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Базовая страница: плавная прокрутка, крупный заголовок, отступ снизу под плавающий мини-плеер.
// Содержимое кладётся прямо внутрь: Page { title: "…"; Label { … } } — оно попадает в ColumnLayout.
Item {
    id: page

    property var router
    property string title
    property string subtitle
    property bool showHeader: true
    property real sideMargin: Theme.mobile ? 16 : 32
    // под плавающим мини-плеером (на телефоне он ниже и уже)
    property real bottomInset: Theme.mobile ? 96 : 120
    default property alias content: column.data
    // Слой под содержимым, вне прокручиваемой области — её clip его не режет (фон шапки, см. HeroShapes)
    property alias underlay: underlayHost.data
    readonly property alias flickable: flick
    readonly property bool scrolled: flick.contentY > 48
    // телефон: «потянуть вниз — обновить» (страница включает и задаёт refreshing / onPullRefresh)
    property bool pullToRefresh: false
    property bool refreshing: false
    signal pullRefresh()

    Item { id: underlayHost; anchors.fill: parent }

    PullToRefresh {
        flickable: flick
        enabled: page.pullToRefresh
        busy: page.refreshing
        onRefresh: page.pullRefresh()
    }

    SmoothFlickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.implicitHeight + 8 + page.bottomInset
        wheelStep: Settings.wheelStep

        ColumnLayout {
            id: column
            x: page.sideMargin
            y: 8
            width: flick.width - 2 * page.sideMargin
            spacing: Theme.mobile ? 20 : 24

            ColumnLayout {
                visible: page.showHeader
                Layout.fillWidth: true
                spacing: 4
                Label {
                    Layout.fillWidth: true
                    text: page.title
                    type: Theme.mobile ? "headlineMedium" : "displaySmall"
                    weight: 600
                }
                Label {
                    visible: page.subtitle !== ""
                    Layout.fillWidth: true
                    text: page.subtitle
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
            }
        }
    }
}
