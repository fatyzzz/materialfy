import QtQuick
import QtQuick.Layouts
import Md3

// Нижняя навигация MD3 (телефон): 3–5 разделов, у активного — иконка залита и стоит на «пилюле»
// secondaryContainer. Под панелью — отступ навигационной полосы системы (Theme.safeBottom).
// items: [{ name, icon, text }]; current — имя активного раздела; activated(name) — тап (повторный тап по
// активному разделу — reselected: вернуться к его корню / прокрутить вверх).
Rectangle {
    id: root

    property var items: []
    property string current
    signal activated(string name)
    signal reselected(string name)

    implicitHeight: 80 + Theme.safeBottom
    color: Theme.surfaceContainer

    // клики сквозь панель не проходят
    MouseArea { anchors.fill: parent; onWheel: wheel => wheel.accepted = true }

    RowLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 80
        anchors.leftMargin: 8
        anchors.rightMargin: 8
        spacing: 8

        Repeater {
            model: root.items

            Item {
                id: tab
                required property var modelData
                readonly property bool active: root.current === modelData.name
                Layout.fillWidth: true
                Layout.fillHeight: true
                activeFocusOnTab: true
                Accessible.role: Accessible.PageTab
                Accessible.name: modelData.text
                Accessible.selectable: true
                Accessible.selected: tab.active
                // Android-мост Qt передаёт checked, но не selected: активная вкладка должна озвучиваться.
                Accessible.checkable: true
                Accessible.checked: tab.active
                Accessible.onPressAction: tab.activate()
                Keys.onReturnPressed: tab.activate()
                Keys.onSpacePressed: tab.activate()

                function activate() {
                    if (active)
                        root.reselected(modelData.name)
                    else
                        root.activated(modelData.name)
                }

                // Выбор раздела: пилюля раскрывается от центра с пружинным перелётом, иконка коротко
                // «подпрыгивает» (MD3 Expressive); нажатие чуть поджимает иконку
                onActiveChanged: if (active && Theme.reducedMotion !== true) pop.restart()
                SequentialAnimation {
                    id: pop
                    NumberAnimation { target: icon; property: "popScale"; to: 1.06; duration: Theme.motion.press; easing.type: Easing.OutCubic }
                    NumberAnimation { target: icon; property: "popScale"; to: 1; duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic }
                }

                Rectangle {
                    id: pill
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 12
                    height: 32
                    width: tab.active ? 64 : 24
                    radius: height / 2
                    color: Theme.secondaryContainer
                    opacity: tab.active ? 1 : 0
                    Behavior on width { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                }
                Icon {
                    id: icon
                    property real popScale: 1
                    anchors.centerIn: pill
                    name: tab.modelData.icon
                    size: 24
                    fill: tab.active ? 1 : 0
                    weight: tab.active ? 500 : 400
                    color: tab.active ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                    scale: popScale * (tapArea.pressed ? 0.88 : 1)
                    Behavior on fill { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
                    Behavior on scale { enabled: !pop.running; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.springFast } }
                }
                Label {
                    Accessible.ignored: true
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.top: pill.bottom
                    anchors.topMargin: 4
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    text: tab.modelData.text
                    type: "labelMedium"
                    // длинная подпись («Configurações» на 360 px) — мельче, а не с многоточием
                    fontSizeMode: Text.HorizontalFit
                    minimumPixelSize: 9
                    weight: tab.active ? 700 : 500
                    color: tab.active ? Theme.fgSurface : Theme.fgSurfaceVariant
                    Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
                }
                // Касание — вся высота колонки; волна — на месте пилюли, как у системной навигации
                Rectangle {
                    x: (parent.width - 64) / 2
                    y: 12
                    width: 64
                    height: 32
                    radius: 16
                    color: Theme.fgSurface
                    opacity: tapArea.pressed ? Theme.stateLayer.pressed : tab.activeFocus ? Theme.stateLayer.focus : 0
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                }
                MouseArea {
                    id: tapArea
                    anchors.fill: parent
                    onClicked: tab.activate()
                }
            }
        }
    }
}
