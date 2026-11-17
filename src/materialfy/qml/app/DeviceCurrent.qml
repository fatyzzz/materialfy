import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Текущее устройство в меню устройств: иконка в «печеньке», имя, «тип · играет / на паузе»,
// громкость устройства (у запрещённых и без supports_volume — строка с замком).
Rectangle {
    id: root

    property var device: ({})           // DEVICE_KEYS активного устройства
    readonly property var _d: device ?? {}
    readonly property bool volumeLocked: _d.isRestricted === true || _d.supportsVolume === false
                                         || (Player.remote === true && Player.canSetVolume === false)

    DeviceBadge.Info { id: info; type: root._d.type ?? "" }

    implicitWidth: 340
    implicitHeight: body.implicitHeight + 18
    radius: Theme.shape.large
    color: Theme.secondaryContainer

    // Другое устройство — содержимое меняется сквозным затуханием, иконка морфится из круга
    property string _shownId: _d.id ?? ""
    onDeviceChanged: {
        const id = _d.id ?? ""
        if (id === _shownId)
            return
        _shownId = id
        if (Theme.reducedMotion === true || !visible)
            return
        swap.restart()
    }
    SequentialAnimation {
        id: swap
        NumberAnimation { target: body; property: "opacity"; to: 0.2; duration: Theme.motion.effectsFast / 2 }
        ScriptAction { script: { shape.jumpTo("circle"); shape.shape = "cookie9" } }
        NumberAnimation { target: body; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault }
    }

    ColumnLayout {
        id: body
        x: 12
        y: 12
        width: parent.width - 24
        spacing: 4

        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            MorphShape {
                id: shape
                Layout.preferredWidth: 40
                Layout.preferredHeight: 40
                shape: "cookie9"
                duration: Theme.motion.spatialFast
                color: Theme.primary
                Icon {
                    anchors.centerIn: parent
                    name: root._d.isThisComputer ? (Theme.mobile ? "smartphone" : "computer") : info.icon
                    size: 22
                    fill: 1
                    weight: 500
                    color: Theme.fgPrimary
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Label {
                    Layout.fillWidth: true
                    text: root._d.name ?? ""
                    type: "titleSmall"
                    color: Theme.fgSecondaryContainer
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 4
                    Icon {
                        name: Player.playing ? "graphic_eq" : "pause"
                        size: 16
                        color: Theme.fgSecondaryContainer
                    }
                    Label {
                        Layout.fillWidth: true
                        text: Player.playing ? I18n.t("{0} · playing", root._d.isThisComputer ? I18n.t("This computer") : info.label)
                            : I18n.t("{0} · on pause", root._d.isThisComputer ? I18n.t("This computer") : info.label)
                        type: "bodySmall"
                        color: Theme.fgSecondaryContainer
                        opacity: 0.9
                    }
                }
            }
        }

        // Громкость: у активного устройства она же Player.volume (плеер сам шлёт её устройству)
        RowLayout {
            visible: !root.volumeLocked
            Layout.fillWidth: true
            Layout.leftMargin: 4
            spacing: 8
            Icon {
                name: vol.value > 0.5 ? "volume_up" : vol.value > 0 ? "volume_down" : "volume_off"
                size: 20
                color: Theme.fgSecondaryContainer
            }
            Slider {
                id: vol
                Layout.fillWidth: true
                trackHeight: 10
                handleHeight: 28
                showValue: false
                inactiveColor: Qt.alpha(Theme.fgSecondaryContainer, 0.22)
                value: Player.volume
                onMoved: v => Player.volume = v
            }
            Label {
                Layout.preferredWidth: 40
                horizontalAlignment: Text.AlignRight
                text: Math.round(vol.value * 100) + " %"
                type: "labelMedium"
                color: Theme.fgSecondaryContainer
            }
        }
        RowLayout {
            visible: root.volumeLocked
            Layout.fillWidth: true
            Layout.leftMargin: 4
            Layout.topMargin: 4
            Layout.bottomMargin: 2
            spacing: 8
            Icon { name: "lock"; size: 18; color: Theme.fgSecondaryContainer }
            Label {
                Layout.fillWidth: true
                text: I18n.t("This device's volume can only be changed on the device itself")
                type: "bodySmall"
                color: Theme.fgSecondaryContainer
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
        }
    }
}
