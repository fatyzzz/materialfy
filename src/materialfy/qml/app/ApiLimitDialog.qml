import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Общий Client ID упёрся в лимит Spotify, а своего приложения нет: окно с инструкцией, как подключить своё
// (бесплатно, пара минут). Само открывается раз за запуск, пока не нажали «Больше не показывать»;
// open() — вручную. Client ID подключается прямо здесь: браузер → «Разрешить» → готово.
Item {
    id: root

    property bool opened: false
    onVisibleChanged: visible ? BackStack.push(root) : BackStack.pop(root)
    Component.onDestruction: BackStack.pop(root)
    property bool _shownThisRun: false
    // Client ID отправлен: ждём браузер и показываем, чем кончилось
    property bool _connecting: false

    readonly property bool _wanted: Auth.state === "signedIn" && Settings.webApiLimit !== ""
        && Auth.personalState !== "ready" && Auth.personalState !== "authorizing"
        && !Settings.apiLimitTipDismissed && !_shownThisRun

    function open() {
        _shownThisRun = true
        _connecting = Auth.personalState === "authorizing"
        field.text = Auth.personalClientId
        opened = true
    }
    function close() { opened = false }
    function connect() {
        const id = field.text.trim()
        if (id === "")
            return
        _connecting = true
        Auth.setPersonalClientId(id)
    }

    // лимит должен продержаться пару секунд: короткая заминка окна не стоит
    Timer {
        interval: 2500
        running: root._wanted && !root.opened
        onTriggered: if (root._wanted) root.open()
    }
    Connections {
        target: Auth
        function onPersonalChanged() {
            if (root._connecting && Auth.personalState === "ready")
                doneTimer.restart()
        }
    }
    Timer { id: doneTimer; interval: 1800; onTriggered: { root._connecting = false; root.close() } }

    readonly property string _st: !_connecting ? "" : Auth.personalState   // "authorizing" | "ready" | "error"

    anchors.fill: parent
    visible: opened || scrim.opacity > 0

    Rectangle {
        id: scrim
        anchors.fill: parent
        color: Qt.alpha(Theme.scrim, 0.32)
        opacity: root.opened ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onClicked: root.close()
            onWheel: wheel => {}
        }
    }

    Rectangle {
        id: card
        anchors.centerIn: parent
        width: Math.min(520, parent.width - 48)
        height: Math.min(flick.contentHeight + 48, parent.height - 48)
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        scale: Theme.reducedMotion || root.opened ? 1 : Theme.mobile ? 0.96 : 0.92
        opacity: root.opened ? 1 : 0
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Theme.mobile ? Easing.OutCubic : Easing.OutBack } }
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Keys.onEscapePressed: root.close()

        Elevation { target: card; level: 3 }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons }

        Flickable {
            id: flick
            anchors.fill: parent
            anchors.margins: 24
            contentWidth: width
            contentHeight: content.implicitHeight
            boundsBehavior: Flickable.StopAtBounds
            clip: true

            ColumnLayout {
                id: content
                width: flick.width
                spacing: 16

                Icon {
                    Layout.alignment: Qt.AlignHCenter
                    name: "key"
                    size: 24
                    color: Theme.secondary
                }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: I18n.t("Spotify is limiting requests")
                    type: "headlineSmall"
                    wrapMode: Text.WordWrap
                }
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Materialfy shares one Spotify developer app with many people, and Spotify has slowed it down. Playback and most pages go through your account and keep working; search, some artist pages and library changes may wait. Your own free app removes the limit for good and takes about two minutes.")
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
                Label {
                    Layout.fillWidth: true
                    visible: Settings.webApiLimit !== ""
                    text: Settings.webApiLimit
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }

                // ---- 1. Кабинет разработчика ----
                Step {
                    number: 1
                    title: I18n.t("Open the Spotify developer dashboard and sign in with your Spotify account")
                    Button {
                        text: I18n.t("Open dashboard")
                        icon: "open_in_new"
                        style: "tonal"
                        size: "xs"
                        onClicked: Qt.openUrlExternally("https://developer.spotify.com/dashboard")
                    }
                }

                // ---- 2. Приложение и redirect URI ----
                Step {
                    number: 2
                    title: I18n.t("Create an app with any name and description. Add this address to Redirect URIs, tick Web API and save")
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: uriRow.implicitHeight + 12
                        radius: Theme.shape.medium
                        color: Theme.surfaceContainerHighest
                        RowLayout {
                            id: uriRow
                            anchors.fill: parent
                            anchors.leftMargin: 14
                            anchors.rightMargin: 4
                            spacing: 8
                            Label {
                                Layout.fillWidth: true
                                text: Auth.personalRedirectUri
                                type: "bodyMedium"
                                font.family: "Consolas"
                            }
                            IconButton {
                                id: copyUri
                                property bool done: false
                                icon: done ? "check" : "content_copy"
                                onClicked: {
                                    Auth.copyToClipboard(Auth.personalRedirectUri)
                                    done = true
                                    copiedTimer.restart()
                                }
                                Timer { id: copiedTimer; interval: 1600; onTriggered: copyUri.done = false }
                            }
                        }
                    }
                }

                // ---- 3. Client ID ----
                Step {
                    number: 3
                    title: I18n.t("Open the app’s settings, copy its Client ID and paste it here")
                    TextField {
                        id: field
                        Layout.fillWidth: true
                        label: "Client ID"
                        placeholderText: I18n.t("32 characters from the Spotify developer dashboard")
                        leadingIcon: "key"
                        error: root._st === "error"
                        supportingText: root._st === "error" ? I18n.t("Couldn’t connect your personal app") : ""
                        onAccepted: root.connect()
                    }
                }

                // как идёт подключение
                RowLayout {
                    Layout.fillWidth: true
                    visible: root._st === "authorizing" || root._st === "ready"
                    spacing: 12
                    LoadingIndicator { visible: root._st === "authorizing"; size: 28 }
                    Icon { visible: root._st === "ready"; name: "check_circle"; size: 24; color: Theme.primary }
                    Label {
                        Layout.fillWidth: true
                        text: root._st === "ready" ? I18n.t("Done: requests now go through your app")
                                                   : I18n.t("Confirm access in your browser")
                        type: "bodyMedium"
                        wrapMode: Text.WordWrap
                    }
                }

                // Flow справа налево: на телефоне три кнопки не помещаются в ряд — переносятся, а не распирают
                // диалог шире экрана (у RowLayout минимальная ширина — сумма кнопок)
                Flow {
                    Layout.fillWidth: true
                    layoutDirection: Qt.RightToLeft
                    spacing: 8
                    Button {
                        text: I18n.t("Connect")
                        enabled: field.text.trim() !== "" && root._st !== "authorizing"
                        onClicked: root.connect()
                    }
                    Button { text: I18n.t("Later"); style: "text"; onClicked: root.close() }
                    Button {
                        text: I18n.t("Don’t show again")
                        style: "text"
                        onClicked: {
                            Settings.apiLimitTipDismissed = true
                            root.close()
                        }
                    }
                }
            }
        }
    }

    // Шаг: номер в круге, текст и что под ним
    component Step: RowLayout {
        property int number
        property string title
        default property alias body: below.data
        Layout.fillWidth: true
        spacing: 14
        Rectangle {
            Layout.alignment: Qt.AlignTop
            implicitWidth: 28
            implicitHeight: 28
            radius: 14
            color: Theme.primary
            Label {
                anchors.centerIn: parent
                text: number
                type: "labelLarge"
                color: Theme.fgPrimary
            }
        }
        ColumnLayout {
            id: below
            Layout.fillWidth: true
            spacing: 8
            Label {
                Layout.fillWidth: true
                text: title
                type: "bodyMedium"
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
        }
    }
}
