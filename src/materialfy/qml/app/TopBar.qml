import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Верхняя панель контента: назад, заголовок прокрученной страницы, кнопки страницы, кнопки окна.
// Пустое место тянет окно (startSystemMove), двойной клик — развернуть.
Item {
    id: root

    required property var router
    required property Window window
    // узкое окно: боковая панель спрятана в шторку, открывается этой кнопкой
    property bool showMenuButton: false
    // дополнительные кнопки справа (телефон: новинки и аккаунт на главной)
    property Component trailing: null
    signal menuClicked()

    implicitHeight: Theme.mobile ? 56 : 64

    readonly property var page: router.currentItem
    // заголовок страницы с собственной шапкой (heroTitle: true) появляется только после прокрутки
    readonly property bool showTitle: page && (page.scrolled || !(page.showHeader || page.heroTitle === true)) && (page.title ?? "") !== ""

    // Перетаскивание окна. Windows: startSystemMove работает, когда у окна есть системное меню
    // (его возвращает нативная рамка оболочки). Если система отказала — двигаем окно сами.
    DragHandler {
        property bool manual: false
        // телефон: окно не двигается, а жест мешал бы прокрутке
        enabled: !Theme.mobile
        target: null
        grabPermissions: PointerHandler.TakeOverForbidden
        onActiveChanged: if (active) manual = !root.window.startSystemMove()
        onCentroidChanged: if (active && manual && root.window.visibility === Window.Windowed) {
            root.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
            root.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
        }
    }
    TapHandler {
        enabled: !Theme.mobile
        acceptedButtons: Qt.LeftButton
        onDoubleTapped: windowButtons.toggleMaximized()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.mobile ? 4 : 16
        anchors.rightMargin: Theme.mobile ? 8 : 16
        spacing: 8

        // кнопка шторки въезжает и раздвигает заголовок, а не появляется рывком
        Item {
            Layout.preferredWidth: root.showMenuButton ? menuButton.implicitWidth : 0
            Layout.rightMargin: root.showMenuButton ? 0 : -8
            Layout.preferredHeight: menuButton.implicitHeight
            Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            Behavior on Layout.rightMargin { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            IconButton {
                id: menuButton
                icon: "menu"
                enabled: root.showMenuButton
                opacity: root.showMenuButton ? 1 : 0
                scale: root.showMenuButton ? 1 : 0.6
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                Behavior on scale { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.OutBack } }
                onClicked: root.menuClicked()
            }
        }

        // Без «назад» кнопка схлопывается, и заголовок стоит у левого края
        Item {
            Layout.preferredWidth: back.enabled ? back.implicitWidth : 0
            // заголовок встаёт по левому краю содержимого страницы (отступ 32; на телефоне — 16)
            Layout.rightMargin: back.enabled || root.showMenuButton ? 0 : Theme.mobile ? -4 : 8
            Layout.preferredHeight: back.implicitHeight
            Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
            Behavior on Layout.rightMargin { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
            clip: true
            IconButton {
                id: back
                icon: "arrow_back"
                enabled: root.router.depth > 1
                opacity: enabled ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: root.router.back()
            }
        }

        Label {
            Layout.fillWidth: true
            // уступает место кнопкам страницы (поиск по медиатеке на телефоне)
            Layout.preferredWidth: 1
            Layout.minimumWidth: 0
            text: root.page ? root.page.title : ""
            type: "titleLarge"
            weight: 500
            opacity: root.showTitle ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        }

        // Кнопки страницы (поиск по библиотеке, сортировка, вид) — page.toolbar: Component.
        // Выгруженный Loader сохраняет implicitWidth прежних кнопок: без явной ширины заголовок следующей
        // страницы обрезался («Лю…»), хотя справа пусто. Скрытый — ещё и без интервала раскладки.
        Loader {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: item ? item.implicitWidth : 0
            visible: active
            active: !!root.page && !!root.page.toolbar
            sourceComponent: root.page ? root.page.toolbar ?? null : null
        }

        Loader {
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: item ? item.implicitWidth : 0
            visible: active
            active: root.trailing !== null
            sourceComponent: root.trailing
        }

        WindowButtons {
            id: windowButtons
            visible: !Theme.mobile
            window: root.window
        }
    }
}
