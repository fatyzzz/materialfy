import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Плашка «Нужен Spotify Premium» (информация, а не ошибка — tertiaryContainer).
// compact — сжатый вид для меню устройств.
Rectangle {
    id: root

    property bool compact: false

    implicitWidth: compact ? 340 : 480
    implicitHeight: (compact ? small : full).implicitHeight + (compact ? 24 : 32)
    radius: compact ? Theme.shape.large : Theme.shape.extraLarge
    color: Theme.tertiaryContainer

    function _premium() { Qt.openUrlExternally("https://www.spotify.com/premium") }

    // ---- Сжатый вид ----
    RowLayout {
        id: small
        visible: root.compact
        x: 14
        y: 12
        width: parent.width - 28
        spacing: 12
        Icon {
            Layout.alignment: Qt.AlignTop
            name: "workspace_premium"
            size: 22
            fill: 1
            weight: 500
            color: Theme.fgTertiaryContainer
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Label {
                Layout.fillWidth: true
                text: I18n.t("Spotify Premium required")
                type: "titleSmall"
                color: Theme.fgTertiaryContainer
            }
            Label {
                Layout.fillWidth: true
                text: I18n.t("Switching devices and listening here require Premium")
                type: "bodySmall"
                color: Theme.fgTertiaryContainer
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
            RowLayout {
                Layout.leftMargin: -8
                spacing: 0
                TextAction { text: I18n.t("Learn more"); onClicked: root._premium() }
                TextAction { text: I18n.t("Check again"); onClicked: Auth.refreshProfile() }
            }
        }
    }

    // ---- Полный вид ----
    ColumnLayout {
        id: full
        visible: !root.compact
        x: 20
        y: 16
        width: parent.width - 40
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            spacing: 16
            MorphShape {
                Layout.preferredWidth: 48
                Layout.preferredHeight: 48
                Layout.alignment: Qt.AlignTop
                shape: "cookie9"
                color: Theme.fgTertiaryContainer
                Icon {
                    anchors.centerIn: parent
                    name: "workspace_premium"
                    size: 26
                    fill: 1
                    weight: 500
                    color: Theme.tertiaryContainer
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Spotify Premium required")
                    type: "titleMedium"
                    color: Theme.fgTertiaryContainer
                }
                Label {
                    Layout.fillWidth: true
                    text: {
                        const name = Auth.account.displayName ?? ""
                        return (name !== "" ? I18n.t("You're signed in as {0} (Spotify Free).", name) : I18n.t("A Free account gives you your library, search and playlists."))
                               + " " + I18n.t("Spotify allows listening on a computer and switching devices only with Premium.")
                    }
                    type: "bodyMedium"
                    color: Theme.fgTertiaryContainer
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    lineHeightMode: Text.ProportionalHeight
                    lineHeight: 1.2
                }
            }
        }

        // Что работает, а что нет
        // чипы и кнопки — в колонке текста, правее значка
        Flow {
            Layout.fillWidth: true
            // узкая плашка (телефон 320–380): без отступа под иконку, иначе кнопка шире плашки
            Layout.leftMargin: root.width < 380 ? 0 : 64
            spacing: 6
            Repeater {
                model: [
                    { ok: true, text: I18n.t("Media library") }, { ok: true, text: I18n.t("Search") }, { ok: true, text: I18n.t("Playlists") },
                    { ok: false, text: I18n.t("Playback") }, { ok: false, text: I18n.t("Connect remote") }
                ]
                Rectangle {
                    id: cap
                    required property var modelData
                    width: capRow.implicitWidth + 16
                    height: 24
                    radius: Theme.shape.small
                    color: Qt.alpha(Theme.fgTertiaryContainer, 0.14)
                    Row {
                        id: capRow
                        anchors.centerIn: parent
                        spacing: 4
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: cap.modelData.ok ? "check" : "close"
                            size: 14
                            color: Theme.fgTertiaryContainer
                        }
                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            text: cap.modelData.text
                            type: "labelSmall"
                            color: Theme.fgTertiaryContainer
                        }
                    }
                }
            }
        }

        Flow {
            Layout.fillWidth: true
            // узкая плашка (телефон 320–380): без отступа под иконку, иначе кнопка шире плашки
            Layout.leftMargin: root.width < 380 ? 0 : 64
            spacing: 8
            TextAction { filled: true; icon: "open_in_new"; text: I18n.t("Get Premium"); onClicked: root._premium() }
            TextAction { icon: "refresh"; text: I18n.t("Check again"); onClicked: Auth.refreshProfile() }
        }
    }

    // Кнопки цвета плашки (Md3/Button красит в primary, на tertiaryContainer это спорит с плашкой)
    component TextAction: Item {
        id: act
        property string text
        property string icon
        property bool filled: false
        signal clicked()
        implicitWidth: actRow.implicitWidth + (filled ? 40 : 16)
        implicitHeight: filled || icon !== "" ? 40 : 32
        Rectangle {
            anchors.fill: parent
            visible: act.filled
            radius: actHit.pressed ? 8 : height / 2
            color: Theme.fgTertiaryContainer
            Behavior on radius { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
        }
        Row {
            id: actRow
            anchors.centerIn: parent
            spacing: 8
            Icon {
                visible: act.icon !== ""
                anchors.verticalCenter: parent.verticalCenter
                tight: true
                name: act.icon
                size: 20
                color: act.filled ? Theme.tertiaryContainer : Theme.fgTertiaryContainer
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: act.text
                type: "labelLarge"
                color: act.filled ? Theme.tertiaryContainer : Theme.fgTertiaryContainer
            }
        }
        StateLayer {
            id: actHit
            anchors.fill: parent
            radius: act.filled ? (pressed ? 8 : height / 2) : height / 2
            color: act.filled ? Theme.tertiaryContainer : Theme.fgTertiaryContainer
            onClicked: act.clicked()
        }
    }
}
