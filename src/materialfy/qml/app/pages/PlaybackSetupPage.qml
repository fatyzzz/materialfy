import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// «Слушать на этом компьютере»: сессия librespot (scope streaming, ответ на 127.0.0.1:8898), после неё
// компьютер — устройство Spotify Connect. С Premium это и есть основной вход (включается при входе);
// отдельным шагом — только после входа без Premium (Web API). Без Web API «Отключить» = «Выйти».
// Слева — hero по Auth.playbackState, справа — плашка Premium / «Применить» и параметры движка.
// Уже 760 px — одна колонка.
Page {
    id: page
    title: I18n.t("Playback")
    showHeader: false

    property string pst: Auth.playbackState
    property bool premium: Auth.premium === true
    property bool signedIn: Auth.state === "signedIn"
    // вход только через сессию: отключить её — значит выйти
    readonly property bool sessionOnly: Auth.webState !== "ready" && Auth.personalState !== "ready"
    readonly property bool free: signedIn && (!premium || pst === "premiumRequired")
    readonly property string mode: !signedIn ? "signedOut"
                                 : free ? "free"
                                 : pst === "authorizing" ? "authorizing"
                                 : pst === "connecting" ? "connecting"
                                 : pst === "ready" ? "done"
                                 : pst === "error" ? "error" : "setup"
    readonly property bool wide: width - 2 * sideMargin >= 760
    readonly property bool _instant: Theme.reducedMotion === true
    readonly property string deviceName: Settings.deviceName !== "" ? Settings.deviceName
                                       : (Devices.thisDeviceName ?? "") !== "" ? Devices.thisDeviceName : "Materialfy"

    Component.onCompleted: Settings.refreshAudioDevices()

    function _later() {
        if (!page.router)
            return
        if (page.router.depth > 1)
            page.router.back()
        else
            page.router.reset("home")
    }
    function _cacheText(mb) {
        if (mb < 1024)
            return I18n.t("{0} MB", Math.round(mb))
        const tenths = Math.round(mb / 1024 * 10)
        return I18n.t("{0} GB", tenths % 10 ? I18n.decimal(tenths / 10, 1) : tenths / 10)
    }
    // Тексты ошибок переведены в Rust — подсказку выбираем по стабильному коду (Auth.playbackErrorCode)
    function _errorHint(text) {
        const code = text !== "" ? (Auth.playbackErrorCode ?? "") : ""
        if (code === "otherAccount") {
            const n = Auth.account.displayName ?? ""
            return (n !== "" ? I18n.t("Your library is open as {0}, but playback was confirmed by another account.", n) + " " : "")
                   + I18n.t("Sign out on accounts.spotify.com and try again.")
        }
        if (code === "portBusy" || text.indexOf("8898") >= 0)
            return I18n.t("Close the program listening on 127.0.0.1:8898 and try again.")
        if (code === "audioKey")
            return I18n.t("Spotify didn't provide a key for this track. Try another track or try again later.")
        return ""
    }

    // Строка статуса и действия меняются сквозным затуханием
    property string shownMode: mode
    onModeChanged: {
        if (_instant || !visible) {
            swap.stop()
            shownMode = mode
            status.opacity = 1
        } else {
            swap.restart()
        }
    }
    SequentialAnimation {
        id: swap
        NumberAnimation { target: status; property: "opacity"; to: 0; duration: Theme.motion.effectsFast; easing.type: Easing.InCubic }
        ScriptAction { script: page.shownMode = page.mode }
        NumberAnimation { target: status; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault; easing.type: Easing.OutCubic }
    }

    component DevRow: SettingRow {
        Layout.minimumHeight: 60
    }
    component RowDivider: Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: Qt.alpha(Theme.outlineVariant, 0.6)
    }

    // Узкое окно, Free: плашка — над hero
    PremiumNotice {
        visible: !page.wide && page.free
        Layout.fillWidth: true
    }

    GridLayout {
        Layout.fillWidth: true
        columns: page.wide ? 2 : 1
        columnSpacing: 20
        rowSpacing: 16

        // ================= Hero =================
        Rectangle {
            id: hero
            // Free в одну колонку: вместо hero — плашка Premium сверху
            visible: page.wide || !page.free
            Layout.alignment: Qt.AlignTop
            Layout.fillWidth: !page.wide
            // в две колонки hero тянется до низа колонки настроек, действия прижаты к низу
            Layout.fillHeight: page.wide
            Layout.preferredWidth: page.wide ? 400 : -1
            implicitHeight: heroCol.implicitHeight + 48
            radius: Theme.shape.extraLarge
            color: Theme.surfaceContainerHigh

            ColumnLayout {
                id: heroCol
                x: 24
                y: 24
                width: parent.width - 48
                height: Math.max(implicitHeight, parent.height - 48)
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    Item {
                        id: heroArt
                        Layout.preferredWidth: 96
                        Layout.preferredHeight: 96
                        readonly property bool done: page.shownMode === "done"
                        MorphShape {
                            anchors.fill: parent
                            shape: heroArt.done ? "sunny" : "cookie12"
                            duration: page._instant ? 0 : Theme.motion.spatialDefault
                            overshoot: 1.15
                            color: Theme.primaryContainer
                            opacity: 0.35
                            rotation: 10
                        }
                        MorphShape {
                            anchors.centerIn: parent
                            width: 72
                            height: 72
                            shape: heroArt.done ? "sunny" : "cookie9"
                            duration: page._instant ? 0 : Theme.motion.spatialDefault
                            overshoot: 1.15
                            color: heroArt.done ? Theme.primary : Theme.secondaryContainer
                            Behavior on color { enabled: !page._instant; ColorAnimation { duration: Theme.motion.effectsDefault } }
                            Icon {
                                anchors.centerIn: parent
                                name: heroArt.done ? "check" : "speaker"
                                size: 34
                                fill: 1
                                color: heroArt.done ? Theme.fgPrimary : Theme.fgSecondaryContainer
                            }
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4
                        Label {
                            Layout.fillWidth: true
                            text: page.shownMode === "done" ? I18n.t("Spotify Connect device") : "Spotify Premium"
                            type: "labelMedium"
                            font.capitalization: Font.AllUppercase
                            color: Theme.primary
                            wrapMode: Text.WordWrap
                            elide: Text.ElideNone
                        }
                        Label {
                            Layout.fillWidth: true
                            text: I18n.t("Listen on this computer")
                            type: "headlineSmall"
                            wrapMode: Text.WordWrap
                            elide: Text.ElideNone
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: page.mode === "done"
                          ? I18n.t("This computer, “{0}”, is in your Spotify devices list. You can move music here from your phone and back.", page.deviceName)
                          : page.mode === "signedOut"
                          ? I18n.t("Sign in with Spotify Premium — music on this computer turns on with the same sign-in.")
                          : I18n.t("Materialfy becomes a Spotify Connect device: music plays here, and your phone sees this computer in its device list. Confirm in your browser; after that your library also goes through this sign-in, with no Web API limits.")
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    lineHeightMode: Text.ProportionalHeight
                    lineHeight: 1.25
                }

                // Что даёт воспроизведение здесь
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    spacing: 10
                    visible: page.mode !== "done" && page.mode !== "signedOut"
                    Repeater {
                        model: page.mode === "done" || page.mode === "signedOut" ? [] : [
                            { icon: "high_quality", text: I18n.t("Up to 320 kbps, gapless") },
                            { icon: "devices", text: I18n.t("Remote: move music between your phone and computer") },
                            { icon: "lock", text: Qt.platform.os === "android"
                                                  ? I18n.t("No password needed: you confirm on the Spotify website, and the key is stored in the app's private storage")
                                                  : Qt.platform.os === "windows"
                                                  ? I18n.t("No password needed: you confirm on the Spotify website, and the key is stored in Credential Manager")
                                                  : I18n.t("No password needed: you confirm on the Spotify website, and the key is stored in the keychain") }
                        ]
                        RowLayout {
                            id: point
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 12
                            Icon { Layout.alignment: Qt.AlignTop; name: point.modelData.icon; size: 20; color: Theme.primary }
                            Label {
                                Layout.fillWidth: true
                                text: point.modelData.text
                                type: "bodyMedium"
                                wrapMode: Text.WordWrap
                                elide: Text.ElideNone
                            }
                        }
                    }
                }

                // Статус и действия по состоянию
                ColumnLayout {
                    id: status
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    visible: ["authorizing", "connecting", "done", "error"].indexOf(page.shownMode) >= 0
                    spacing: 12

                    // Ждём браузер / подключаемся / подключено
                    Rectangle {
                        visible: page.shownMode === "authorizing" || page.shownMode === "connecting" || page.shownMode === "done"
                        Layout.fillWidth: true
                        implicitHeight: statusRow.implicitHeight + 24
                        radius: Theme.shape.large
                        color: Theme.surfaceContainerHighest
                        RowLayout {
                            id: statusRow
                            x: 14
                            y: 12
                            width: parent.width - 28
                            spacing: 14
                            LoadingIndicator {
                                visible: page.shownMode !== "done"
                                size: 40
                                running: !Theme.calm && !page._instant
                            }
                            Icon {
                                visible: page.shownMode === "done"
                                name: Theme.mobile ? "smartphone" : "computer"
                                size: 24
                                fill: 1
                                color: Theme.primary
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    Layout.fillWidth: true
                                    text: page.shownMode === "authorizing" ? I18n.t("Confirm in your browser")
                                        : page.shownMode === "connecting" ? I18n.t("Connecting to Spotify Connect…")
                                        : I18n.t("{0} · this computer", page.deviceName)
                                    type: "titleSmall"
                                    wrapMode: Text.WordWrap
                                    elide: Text.ElideNone
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: page.shownMode === "authorizing" ? I18n.t("Permission to play in Spotify apps")
                                        : page.shownMode === "connecting" ? I18n.t("Registering device “{0}”", page.deviceName)
                                        : Player.localState === "failed" ? I18n.t("Couldn't connect — click “Apply” in the settings below")
                                        : Player.localState === "connecting" ? I18n.t("Connecting…")
                                        : I18n.t("Connected · {0} kbps", Settings.bitrate)
                                    type: "bodySmall"
                                    color: Player.localState === "failed" && page.shownMode === "done" ? Theme.error : Theme.fgSurfaceVariant
                                    wrapMode: Text.WordWrap
                                    elide: Text.ElideNone
                                }
                            }
                            IconButton {
                                visible: page.shownMode === "authorizing"
                                icon: "open_in_new"
                                style: "tonal"
                                onClicked: Auth.reopenBrowser()
                            }
                            Rectangle {
                                visible: page.shownMode === "done" && Player.localState !== "failed"
                                Layout.preferredWidth: onlineRow.implicitWidth + 20
                                Layout.preferredHeight: 32
                                radius: 16
                                color: Theme.surfaceContainerHighest
                                Row {
                                    id: onlineRow
                                    anchors.centerIn: parent
                                    spacing: 6
                                    Rectangle {
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: 8
                                        height: 8
                                        radius: 4
                                        color: Player.localState === "ready" ? Theme.primary : Theme.outline
                                    }
                                    Label {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: Player.localState === "ready" ? I18n.t("Online") : I18n.t("Waiting")
                                        type: "labelMedium"
                                        color: Theme.fgSurface
                                    }
                                }
                            }
                        }
                    }

                    // Ошибка
                    Rectangle {
                        visible: page.shownMode === "error"
                        Layout.fillWidth: true
                        implicitHeight: errRow.implicitHeight + 24
                        radius: Theme.shape.large
                        color: Theme.errorContainer
                        RowLayout {
                            id: errRow
                            x: 16
                            y: 12
                            width: parent.width - 32
                            spacing: 12
                            Icon { Layout.alignment: Qt.AlignTop; name: "error"; size: 20; color: Theme.fgErrorContainer }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                Label {
                                    Layout.fillWidth: true
                                    text: Auth.playbackError !== "" ? Auth.playbackError : I18n.t("Couldn't turn on playback")
                                    type: "titleSmall"
                                    color: Theme.fgErrorContainer
                                    wrapMode: Text.WordWrap
                                    elide: Text.ElideNone
                                }
                                Label {
                                    Layout.fillWidth: true
                                    visible: text !== ""
                                    text: page._errorHint(Auth.playbackError)
                                    type: "bodySmall"
                                    color: Theme.fgErrorContainer
                                    wrapMode: Text.WordWrap
                                    elide: Text.ElideNone
                                }
                            }
                        }
                    }
                }

                Item { Layout.fillHeight: true }

                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    spacing: 8
                    opacity: status.opacity

                    Button {
                        visible: page.shownMode === "setup" || page.shownMode === "free"
                        text: I18n.t("Allow playback")
                        icon: "open_in_new"
                        size: "m"
                        enabled: page.shownMode === "setup"
                        onClicked: Auth.authorizePlayback()
                    }
                    Button {
                        visible: page.shownMode === "error"
                        text: I18n.t("Retry")
                        icon: "refresh"
                        size: "m"
                        onClicked: Auth.authorizePlayback()
                    }
                    Button {
                        visible: page.shownMode === "signedOut"
                        text: I18n.t("Sign in")
                        icon: "person"
                        size: "m"
                        onClicked: page.router ? page.router.reset("account") : null
                    }
                    Button {
                        visible: page.shownMode === "authorizing"
                        text: I18n.t("Copy link")
                        icon: "content_copy"
                        style: "outlined"
                        onClicked: Auth.copyAuthUrl()
                    }
                    Button {
                        visible: page.shownMode === "authorizing" || page.shownMode === "connecting"
                        text: I18n.t("Cancel")
                        style: "text"
                        onClicked: Auth.cancelLogin()
                    }
                    Button {
                        visible: page.shownMode === "done"
                        readonly property bool here: Player.isLocal === true && Player.hasTrack
                        text: here ? I18n.t("Playing here") : I18n.t("Switch here")
                        icon: here ? "check" : "play_arrow"
                        enabled: !here && Player.localState === "ready"
                        onClicked: Player.transferHere()
                    }
                    Button {
                        visible: page.shownMode === "done"
                        text: page.sessionOnly ? I18n.t("Sign out") : I18n.t("Disconnect")
                        icon: "logout"
                        style: "outlined"
                        onClicked: Auth.disablePlayback()
                    }
                    Button {
                        visible: page.shownMode === "setup" || page.shownMode === "free" || page.shownMode === "error"
                        text: I18n.t("Later")
                        height: 56      // по центру строки рядом с кнопкой size "m"
                        style: "text"
                        onClicked: page._later()
                    }
                }
            }
        }

        // ================= Плашки и настройки =================
        ColumnLayout {
            Layout.alignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: 16

            PremiumNotice {
                visible: page.wide && page.free
                Layout.fillWidth: true
            }

            ApplyBar {
                Layout.fillWidth: true
                shown: page.mode === "done" && Settings.engineRestartPending === true
            }

            // Заголовок и плотная карточка строк с разделителями
            RowLayout {
                spacing: 8
                opacity: devCard.opacity
                Icon { name: "tune"; size: 20; color: Theme.primary }
                Label { text: I18n.t("Device"); type: "titleMedium"; color: Theme.primary }
            }
            Rectangle {
                id: devCard
                Layout.fillWidth: true
                implicitHeight: devRows.implicitHeight + 32
                radius: Theme.shape.extraLarge
                color: Theme.surfaceContainerHigh
                enabled: page.mode !== "free" && page.mode !== "signedOut"
                opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

                ColumnLayout {
                    id: devRows
                    x: 24
                    y: 16
                    width: parent.width - 48
                    spacing: 0

                    DevRow {
                        title: I18n.t("Name in Spotify Connect")
                        description: I18n.t("How this computer appears on phones and speakers")
                        TextField {
                            id: nameField
                            implicitWidth: Theme.mobile ? 280 : 240
                            trailingIcon: "edit"
                            onTrailingClicked: nameField.inputItem.forceActiveFocus()
                            leadingIcon: Theme.mobile ? "smartphone" : "computer"
                            text: Settings.deviceName
                            placeholderText: (Devices.thisDeviceName ?? "") !== "" ? Devices.thisDeviceName : "Materialfy"
                            maximumLength: 64
                            function commit() {
                                const v = text.trim()
                                if (v !== Settings.deviceName)
                                    Settings.deviceName = v
                            }
                            onAccepted: commit()
                            Connections {
                                target: nameField.inputItem
                                function onEditingFinished() { nameField.commit() }
                            }
                        }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Quality")
                        description: I18n.t("Stream bitrate, kbps")
                        ButtonGroup {
                            model: ["96", "160", "320"]
                            showCheck: true
                            autoSelect: false
                            currentIndex: [96, 160, 320].indexOf(Settings.bitrate)
                            onActivated: i => Settings.bitrate = [96, 160, 320][i]
                        }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Normalize volume")
                        description: I18n.t("Spotify normalization: quiet and loud tracks play at an even level")
                        Switch { checked: Settings.normalisation; onToggled: c => Settings.normalisation = c }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Gapless playback")
                        description: I18n.t("The next track starts without silence — for live albums and mixes")
                        Switch { checked: Settings.gapless; onToggled: c => Settings.gapless = c }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Autoplay")
                        description: I18n.t("When the queue ends, play similar tracks")
                        Switch { checked: Settings.autoplay; onToggled: c => Settings.autoplay = c }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Audio output")
                        MenuField {
                            implicitWidth: Theme.mobile ? 280 : 240
                            leadingIcon: "headphones"
                            model: [{ id: "", text: Theme.mobile ? I18n.t("System default") : I18n.t("System default output") }]
                                   .concat((Settings.audioDevices ?? []).map(d => ({ id: d, text: d })))
                            currentValue: Settings.audioDevice
                            onActivated: v => Settings.audioDevice = v
                        }
                    }
                    RowDivider { }
                    DevRow {
                        title: I18n.t("Audio cache")
                        description: I18n.t("Replays play without downloading")
                        Switch { checked: Settings.audioCache; onToggled: c => Settings.audioCache = c }
                    }
                    RowDivider {
                        visible: Settings.audioCache
                    }
                    DevRow {
                        visible: Settings.audioCache
                        title: I18n.t("Cache size")
                        Row {
                            spacing: 12
                            Slider {
                                wheelEnabled: false     // колесо прокручивает страницу, а не меняет настройку
                                id: cacheSlider
                                anchors.verticalCenter: parent.verticalCenter
                                width: 168
                                from: 256
                                to: 10240
                                stepSize: 256
                                trackHeight: 10
                                handleHeight: 28
                                value: Settings.audioCacheMb
                                valueText: v => page._cacheText(v)
                                // сохраняем при отпускании, а не на каждом шаге перетаскивания
                                property int _pending: Settings.audioCacheMb
                                onMoved: v => _pending = Math.round(v)
                                onPressedChanged: if (!pressed && _pending !== Settings.audioCacheMb) Settings.audioCacheMb = _pending
                            }
                            Label {
                                anchors.verticalCenter: parent.verticalCenter
                                width: 48
                                horizontalAlignment: Text.AlignRight
                                text: page._cacheText(cacheSlider.pressed ? cacheSlider._pending : cacheSlider.value)
                                type: "labelLarge"
                                color: Theme.fgSurfaceVariant
                            }
                        }
                    }
                }
            }
        }
    }
}
