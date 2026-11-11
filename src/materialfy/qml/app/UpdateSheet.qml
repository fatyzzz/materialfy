import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Окно обновления (Updater.showSheet() → sheetRequested): версия, дата, размер, заметки к выпуску
// (Markdown с GitHub, ссылки открываются в браузере), загрузка с волнистым прогрессом, проверка,
// «Перезапустить и обновить»; ошибки — с «Повторить» и страницей выпуска. Закрытие не прерывает загрузку:
// её видно на плашке в боковой панели.
Item {
    id: root

    property bool opened: false
    onVisibleChanged: visible ? BackStack.push(root) : BackStack.pop(root)
    Component.onDestruction: BackStack.pop(root)

    function open() {
        opened = true
        notesFlick.contentY = 0
        flick.contentY = 0
        card.forceActiveFocus()
    }
    function close() {
        if (Updater.state !== "installing")
            opened = false
    }

    readonly property string st: Updater.state
    readonly property bool hasOffer: Updater.version !== ""
    readonly property bool progressShown: st === "downloading" || st === "verifying" || st === "readyToInstall" || st === "installing"
    readonly property bool mac: Updater.installKind === "mac"

    // ---- форматирование ----
    readonly property real _mb: 1024 * 1024
    function mb(b) { return I18n.decimal(b / _mb, b >= 100 * _mb ? 0 : 1) }
    function sizeText(b) { return b >= 1024 * _mb ? I18n.bytes(b) : I18n.t("{0} MB", mb(b)) }
    function speedText(bps) {
        return bps < _mb ? I18n.t("{0} KB/s", Math.max(1, Math.round(bps / 1024))) : I18n.t("{0} MB/s", I18n.decimal(bps / _mb, 1))
    }
    function etaText(s) {
        if (s < 0)
            return ""
        if (s < 60)
            return I18n.t("{0} s left", Math.max(1, s))
        if (s < 3600)
            return I18n.t("{0} min left", Math.ceil(s / 60))
        return I18n.t("{0} h left", I18n.decimal(s / 3600, 1))
    }
    readonly property string progressDetail: {
        const parts = [I18n.t("{0} of {1}", mb(Updater.downloaded), sizeText(Updater.total))]
        if (Updater.speed > 0)
            parts.push(speedText(Updater.speed))
        const eta = etaText(Updater.eta)
        if (eta !== "")
            parts.push(eta)
        return parts.join(" · ")
    }

    readonly property string title: {
        switch (st) {
        case "checking": return I18n.t("Checking for updates…")
        case "upToDate": return I18n.t("You’re up to date")
        case "downloading": return I18n.t("Downloading the update")
        case "verifying": return I18n.t("Verifying the download")
        case "readyToInstall": return I18n.t("Ready to install")
        case "installing": return I18n.t("Installing the update…")
        case "error": return I18n.t("Update failed")
        default: return hasOffer ? I18n.t("Update available") : I18n.t("Updates")
        }
    }
    readonly property string headerShape: st === "readyToInstall" || st === "installing" ? "sunny"
                                        : st === "error" ? "cookie4"
                                        : st === "downloading" || st === "verifying" ? "flower6" : "cookie9"
    readonly property string headerIcon: st === "readyToInstall" || st === "installing" ? "restart_alt"
                                       : st === "error" ? "error"
                                       : st === "upToDate" ? "check"
                                       : st === "verifying" ? "check_circle" : "download"

    Connections {
        target: Updater
        function onSheetRequested() { root.open() }
    }

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
        width: Math.min(560, parent.width - 32)
        height: Math.min(content.implicitHeight + 48, parent.height - 32)
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        scale: Theme.reducedMotion || root.opened ? 1 : Theme.mobile ? 0.96 : 0.92
        opacity: root.opened ? 1 : 0
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Theme.mobile ? Easing.OutCubic : Easing.OutBack } }
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Behavior on height { enabled: root.opened; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
        Keys.onEscapePressed: root.close()

        Elevation { target: card; level: 3 }

        // клики по карточке не закрывают окно
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: wheel => wheel.accepted = false }

        Flickable {
            id: flick
            anchors.fill: parent
            anchors.margins: 24
            contentHeight: content.implicitHeight
            clip: contentHeight > height
            interactive: contentHeight > height
            boundsBehavior: Flickable.StopAtBounds

            ColumnLayout {
                id: content
                width: flick.width
                spacing: 16

                // ---- Шапка: фигура с иконкой, заголовок, версия ----
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Item {
                        Layout.preferredWidth: 56
                        Layout.preferredHeight: 56
                        Layout.alignment: Qt.AlignTop
                        MorphShape {
                            anchors.fill: parent
                            shape: root.headerShape
                            color: root.st === "error" ? Theme.errorContainer : Theme.primaryContainer
                            angle: root.st === "downloading" && !Theme.reducedMotion ? spin.angle : 0
                            QtObject {
                                id: spin
                                property real angle: 0
                                NumberAnimation on angle {
                                    running: root.opened && root.st === "downloading" && !Theme.reducedMotion
                                    from: 0; to: 360; duration: 6000; loops: Animation.Infinite
                                }
                            }
                        }
                        Icon {
                            anchors.centerIn: parent
                            name: root.headerIcon
                            size: 28
                            fill: 1
                            color: root.st === "error" ? Theme.fgErrorContainer : Theme.fgPrimaryContainer
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2
                        Label {
                            Layout.fillWidth: true
                            text: root.title
                            type: "titleLarge"
                            weight: 600
                            wrapMode: Text.Wrap
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: root.hasOffer
                            text: {
                                const parts = [Updater.prerelease ? I18n.t("Materialfy {0} (pre-release)", Updater.version)
                                                                  : I18n.t("Materialfy {0}", Updater.version)]
                                if (Updater.publishedAt !== "")
                                    parts.push(I18n.date(Updater.publishedAt))
                                if (Updater.size > 0)
                                    parts.push(root.sizeText(Updater.size))
                                return parts.join(" · ")
                            }
                            type: "bodyMedium"
                            color: Theme.fgSurfaceVariant
                            wrapMode: Text.Wrap
                        }
                        Label {
                            Layout.fillWidth: true
                            text: I18n.t("You have version {0}", Updater.currentVersion)
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                            wrapMode: Text.Wrap
                        }
                    }
                }

                // ---- Что нового ----
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.hasOffer && root.st !== "error"
                    spacing: 8

                    Label {
                        text: I18n.t("What’s new")
                        type: "titleSmall"
                        color: Theme.primary
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.min(notes.implicitHeight + 32, Math.max(96, root.height * 0.32))
                        radius: Theme.shape.large
                        color: Theme.surfaceContainer

                        Flickable {
                            id: notesFlick
                            anchors.fill: parent
                            anchors.margins: 16
                            contentHeight: notes.implicitHeight
                            clip: true
                            interactive: contentHeight > height
                            boundsBehavior: Flickable.StopAtBounds

                            Text {
                                id: notes
                                width: notesFlick.width
                                text: Updater.notes !== "" ? Updater.notes : I18n.t("No release notes for this version.")
                                textFormat: Updater.notes !== "" ? Text.MarkdownText : Text.PlainText
                                wrapMode: Text.Wrap
                                color: Theme.fgSurface
                                linkColor: Theme.primary
                                font.family: Theme.fontFamily
                                font.pixelSize: 14
                                font.variableAxes: Theme.fontAxes(14, 400)
                                lineHeight: 1.15
                                onLinkActivated: link => Qt.openUrlExternally(link)
                                HoverHandler { cursorShape: notes.hoveredLink !== "" ? Qt.PointingHandCursor : Qt.ArrowCursor }
                            }
                        }
                    }
                }

                // ---- Прогресс ----
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.progressShown
                    spacing: 6

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12
                        Label {
                            text: root.st === "downloading" ? Math.floor(Updater.progress * 100) + " %"
                                : root.st === "verifying" ? I18n.t("Verifying…")
                                : root.st === "installing" ? I18n.t("Restarting…")
                                : I18n.t("Downloaded and verified")
                            type: root.st === "downloading" ? "titleLarge" : "titleSmall"
                            color: Theme.fgSurface
                            font.features: ({ "tnum": 1 })
                        }
                        Label {
                            Layout.fillWidth: true
                            horizontalAlignment: Text.AlignRight
                            visible: root.st === "downloading"
                            text: root.progressDetail
                            type: "bodyMedium"
                            color: Theme.fgSurfaceVariant
                            font.features: ({ "tnum": 1 })
                            elide: Text.ElideLeft
                        }
                    }
                    WavyProgress {
                        Layout.fillWidth: true
                        value: root.st === "downloading" ? Updater.progress : 1
                        // волна бежит, пока что-то происходит; готово — прямая линия
                        wavy: root.st === "downloading" || root.st === "verifying" || root.st === "installing"
                        speed: root.st === "verifying" ? 2 : 1
                        Behavior on value { NumberAnimation { duration: 160 } }
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: root.st === "readyToInstall"
                              ? (Updater.note !== "" ? Updater.note
                                 : root.mac ? I18n.t("Open the disk image and drag Materialfy to Applications")
                                 : I18n.t("Materialfy will close, install the update and start again. It takes a few seconds"))
                              : root.st === "verifying" ? I18n.t("Making sure the file is exactly what was published")
                              : ""
                        type: "bodySmall"
                        color: Theme.fgSurfaceVariant
                        wrapMode: Text.Wrap
                    }
                }

                // ---- Ошибка ----
                Rectangle {
                    Layout.fillWidth: true
                    visible: root.st === "error"
                    implicitHeight: errorRow.implicitHeight + 24
                    radius: Theme.shape.large
                    color: Theme.errorContainer
                    RowLayout {
                        id: errorRow
                        x: 16
                        y: 12
                        width: parent.width - 32
                        spacing: 12
                        Icon { name: "error"; size: 20; color: Theme.fgErrorContainer; Layout.alignment: Qt.AlignTop }
                        Label {
                            Layout.fillWidth: true
                            text: Updater.errorText !== "" ? Updater.errorText : I18n.t("Something went wrong")
                            type: "bodyMedium"
                            color: Theme.fgErrorContainer
                            wrapMode: Text.Wrap
                        }
                    }
                }

                // ---- Эта копия не обновляется сама ----
                Label {
                    Layout.fillWidth: true
                    visible: root.st === "available" && !Updater.canInstall
                    text: Updater.installKind === "dev"
                          ? I18n.t("This is a development build, so it doesn’t update itself. Get the new version from the release page")
                          : I18n.t("This copy of Materialfy can’t update itself. Download the new version from the release page")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.Wrap
                }

                // ---- Кнопки ----
                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    layoutDirection: Qt.RightToLeft
                    spacing: 8

                    // available
                    Button {
                        visible: root.st === "available"
                        text: Updater.canInstall ? I18n.t("Update now") : I18n.t("Open release page")
                        icon: Updater.canInstall ? "download" : "open_in_new"
                        onClicked: Updater.canInstall ? Updater.download() : Updater.openReleasePage()
                    }
                    Button {
                        visible: root.st === "available"
                        text: I18n.t("Later")
                        style: "text"
                        onClicked: root.close()
                    }
                    Button {
                        visible: root.st === "available" && !Updater.skipped
                        text: I18n.t("Skip this version")
                        style: "text"
                        onClicked: { Updater.skip(); root.close() }
                    }

                    // downloading / verifying
                    Button {
                        visible: root.st === "downloading" || root.st === "verifying"
                        text: I18n.t("Continue in background")
                        style: "tonal"
                        onClicked: root.close()
                    }
                    Button {
                        visible: root.st === "downloading" || root.st === "verifying"
                        text: I18n.t("Cancel")
                        style: "text"
                        onClicked: Updater.cancel()
                    }

                    // ready
                    Button {
                        visible: root.st === "readyToInstall" || root.st === "installing"
                        enabled: root.st === "readyToInstall"
                        text: root.mac || Updater.installKind === "apk" ? I18n.t("Open installer") : I18n.t("Restart to update")
                        icon: root.mac ? "open_in_new" : "restart_alt"
                        onClicked: Updater.install()
                    }
                    Button {
                        visible: root.st === "readyToInstall"
                        text: I18n.t("Later")
                        style: "text"
                        onClicked: root.close()
                    }

                    // error
                    Button {
                        visible: root.st === "error" && Updater.retryable
                        text: I18n.t("Retry")
                        icon: "refresh"
                        onClicked: Updater.retry()
                    }
                    Button {
                        visible: root.st === "error"
                        text: I18n.t("Open release page")
                        icon: "open_in_new"
                        style: "text"
                        onClicked: Updater.openReleasePage()
                    }

                    // checking / up to date / error / idle
                    Button {
                        visible: root.st === "error" || root.st === "upToDate" || root.st === "checking" || root.st === "idle"
                        text: I18n.t("Close")
                        style: root.st === "error" ? "text" : "filled"
                        onClicked: root.close()
                    }
                }
            }
        }
    }
}
