import QtQuick
import Md3
import Materialfy.Core

// Подпись «cast · Играет на {имя}» на месте кодек-бейджа, пока играет другое устройство.
// Варианты: «{имя} · пауза», «Переносим на …», «{имя} не отвечает». brief — только имя (полоска мини-режима).
Row {
    id: root

    property bool brief: false
    // ширина, которую дала раскладка (мини-плеер): без неё подпись до 220 px вылезала из ряда под кнопку устройства
    property real maxWidth: -1
    // Имя держим и после пропажи устройства: снимок может уже не содержать его
    property string _name: ""
    readonly property string deviceName: (Player.device ?? {}).name ?? ""
    onDeviceNameChanged: if (deviceName !== "") _name = deviceName
    Component.onCompleted: if (deviceName !== "") _name = deviceName

    readonly property string targetId: Devices.transferringTo ?? ""
    readonly property bool moving: Player.transferring === true || targetId !== ""
    readonly property bool movingHere: moving && targetId !== "" && targetId === Devices.thisDeviceId
    readonly property bool lost: Player.deviceLost === true
    readonly property bool shown: Player.hasTrack && (Player.remote === true || lost || moving)

    function _targetName() {
        const m = Devices.devices
        const i = m && targetId !== "" ? m.find("id", targetId) : -1
        return i >= 0 ? (m.get(i).name ?? "") : ""
    }

    readonly property string text: {
        if (movingHere)
            return brief ? I18n.t("Here…") : I18n.t("Moving to this computer…")
        if (moving) {
            const t = _targetName()
            return t === "" ? I18n.t("Moving…") : brief ? t : I18n.t("Moving to “{0}”…", t)
        }
        const n = _name !== "" ? _name : I18n.t("another device")
        if (lost)
            return I18n.t("{0} isn’t responding", n)
        if (brief)
            return n
        return Player.playing ? I18n.t("Playing on {0}", n) : I18n.t("Paused on {0}", n)
    }
    readonly property color tint: lost ? Theme.error : moving ? Theme.fgSurfaceVariant : Theme.primary

    visible: opacity > 0.01
    opacity: shown ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    spacing: 3

    Icon {
        anchors.verticalCenter: parent.verticalCenter
        name: root.moving ? "sync" : root.lost ? "portable_wifi_off" : "cast"
        size: 14
        weight: 500
        color: root.tint
    }
    Label {
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(0, Math.min(implicitWidth, root.brief ? 140 : 220, root.maxWidth >= 0 ? root.maxWidth - 17 : Infinity))
        text: root.text
        type: "labelSmall"
        color: root.tint
        elide: Text.ElideRight
    }
}
