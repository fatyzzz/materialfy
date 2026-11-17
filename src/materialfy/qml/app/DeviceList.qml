import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Список устройств Spotify Connect: «Слушать на» + обновить, текущее устройство с громкостью,
// «Другие устройства», подпись про Cast. Общий для вкладки правой панели (DevicePanel) и листа снизу
// в узком окне (DeviceSheet). Список запрашивают они сами при открытии; здесь — кнопка «обновить».
ColumnLayout {
    id: content

    property bool touch: false          // лист: действия видны кнопками, наведения нет
    signal closeRequested()

    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property bool free: signedIn && Auth.premium !== true
    readonly property var active: Devices.active ?? {}
    readonly property bool hasActive: (active.id ?? "") !== ""
    readonly property string movingTo: Devices.transferringTo ?? ""

    // Этот компьютер: из списка устройств, а если его там нет — по имени из моста
    readonly property var here: {
        const m = Devices.devices
        void Devices.active
        const i = m && m.count > 0 ? m.find("isThisComputer", true) : -1
        if (i >= 0)
            return m.get(i)
        return { id: Devices.thisDeviceId ?? "", name: Devices.thisDeviceName || I18n.t("This computer"), type: Theme.mobile ? "smartphone" : "computer",
                 isActive: false, isThisComputer: true, isRestricted: false, supportsVolume: true, volume: -1, source: "api" }
    }
    readonly property bool hereActive: hasActive && active.isThisComputer === true
    readonly property bool needsSetup: !free && Auth.playbackState !== "ready" && Player.localState !== "ready"
    readonly property bool showHere: signedIn && !hereActive
    readonly property int others: {
        const m = Devices.devices
        void Devices.active
        if (!m)
            return 0
        let n = 0
        for (let i = 0; i < m.count; ++i) {
            const d = m.get(i)
            if (!d.isActive && !d.isThisComputer)
                ++n
        }
        return n
    }

    spacing: 0

    // Шапка
    RowLayout {
        Layout.fillWidth: true
        Layout.preferredHeight: 44
        Layout.leftMargin: 12
        Layout.rightMargin: 4
        spacing: 8
        Label {
            Layout.fillWidth: true
            text: I18n.t("Play on")
            type: "titleSmall"
        }
        Item {
            Layout.preferredWidth: 40
            Layout.preferredHeight: 40
            LoadingIndicator {
                running: !Theme.calm
                anchors.centerIn: parent
                visible: Devices.loading === true
                size: 28
            }
            IconButton {
                anchors.centerIn: parent
                visible: Devices.loading !== true
                icon: "refresh"
                enabled: content.signedIn
                onClicked: Devices.refresh()
            }
        }
    }

    PremiumNotice {
        visible: content.free
        Layout.fillWidth: true
        Layout.topMargin: 2
        Layout.bottomMargin: 6
        compact: true
    }

    DeviceCurrent {
        visible: content.hasActive
        Layout.fillWidth: true
        Layout.topMargin: 2
        Layout.bottomMargin: 6
        device: content.active
    }

    // Нигде не играет и выбрать не из чего (пока идёт запрос — «Ищем устройства…»)
    ColumnLayout {
        id: emptyBox
        readonly property bool looking: content.signedIn && Devices.loading === true
        visible: !content.hasActive && content.others === 0
        Layout.fillWidth: true
        Layout.topMargin: 8
        Layout.bottomMargin: 8
        spacing: 8
        MorphShape {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 64
            Layout.preferredHeight: 64
            shape: "cookie9"
            color: Theme.secondaryContainer
            Icon { anchors.centerIn: parent; visible: !emptyBox.looking; name: "devices_off"; size: 30; color: Theme.fgSecondaryContainer }
            LoadingIndicator {
                anchors.centerIn: parent
                visible: emptyBox.looking
                running: visible && !Theme.calm
                size: 36
                color: Theme.fgSecondaryContainer
            }
        }
        Label {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            text: emptyBox.looking ? I18n.t("Looking for devices…")
                : content.signedIn ? I18n.t("No active devices") : I18n.t("Sign in to your account")
            type: "titleSmall"
        }
        Label {
            visible: !emptyBox.looking
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            horizontalAlignment: Text.AlignHCenter
            text: content.signedIn
                  ? I18n.t("Open Spotify on a phone or speaker and it will show up here. Or start playing music on this computer.")
                  : I18n.t("Spotify Connect devices will appear here after you sign in.")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
        }
    }

    Label {
        visible: content.showHere || content.others > 0
        Layout.fillWidth: true
        Layout.leftMargin: 12
        Layout.topMargin: 6
        Layout.bottomMargin: 4
        text: I18n.t("Other devices")
        type: "labelMedium"
        color: Theme.fgSurfaceVariant
    }

    // Этот компьютер: «Слушать здесь» или «Настроить воспроизведение здесь»
    DeviceRow {
        visible: content.showHere
        Layout.fillWidth: true
        device: content.here
        touch: content.touch
        setup: content.needsSetup
        disabled: content.free
        busy: content.movingTo !== "" && content.movingTo === (content.here.id ?? "")
        onActivated: {
            if (content.needsSetup) {
                content.closeRequested()
                Catalog.openRequested("playback", {})
            } else {
                Devices.transferHere()
            }
        }
    }

    Repeater {
        model: Devices.devices
        DeviceRow {
            id: row
            required property var model
            readonly property var dev: ({
                id: model.id, name: model.name, type: model.type, isActive: model.isActive,
                isThisComputer: model.isThisComputer, isRestricted: model.isRestricted,
                supportsVolume: model.supportsVolume, volume: model.volume, source: model.source
            })
            visible: !dev.isActive && !dev.isThisComputer
            Layout.fillWidth: true
            device: dev
            touch: content.touch
            disabled: content.free
            busy: content.movingTo !== "" && content.movingTo === dev.id
            onActivated: dev.source === "zeroconf" ? Devices.activateReceiver(dev.name) : Devices.transfer(dev.id)
        }
    }

    Label {
        visible: content.signedIn && (Devices.errorText ?? "") !== ""
        Layout.fillWidth: true
        Layout.leftMargin: 12
        Layout.rightMargin: 12
        Layout.topMargin: 4
        text: Devices.errorText ?? ""
        type: "bodySmall"
        color: Theme.error
        wrapMode: Text.WordWrap
        elide: Text.ElideNone
    }

    // Подпись про Google Cast
    Rectangle {
        Layout.fillWidth: true
        Layout.topMargin: 6
        implicitHeight: 1
        color: Theme.outlineVariant
    }
    RowLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 12
        Layout.rightMargin: 12
        Layout.topMargin: 8
        Layout.bottomMargin: 6
        spacing: 8
        Icon { Layout.alignment: Qt.AlignTop; name: "info"; size: 16; color: Theme.fgSurfaceVariant }
        Label {
            Layout.fillWidth: true
            text: I18n.t("Google Cast speakers only appear here once the Spotify app has already turned them on")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
        }
    }
}
