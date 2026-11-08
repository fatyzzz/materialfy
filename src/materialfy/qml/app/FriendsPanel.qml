import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Вкладка «Друзья» правой панели: карточка Jam и «Активность друзей» (что слушают те, на кого вы подписаны).
// Пока вкладка видна, активность перечитывается раз в минуту (сессия librespot, без Web API).
SmoothFlickable {
    id: root

    property bool shown: false
    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property var friends: Social.friends ?? []

    onShownChanged: if (shown && signedIn && Social.jamChecked !== true) Social.refreshJam()
    Component.onCompleted: if (shown && signedIn && Social.jamChecked !== true) Social.refreshJam()

    Timer {
        interval: 60000
        repeat: true
        triggeredOnStart: true
        running: root.shown && root.signedIn
        onTriggered: {
            Social.refreshFriends(false)
            root.now = Date.now()
        }
    }
    property real now: Date.now()

    wheelStep: Settings.wheelStep
    contentWidth: width
    contentHeight: body.implicitHeight + 16
    interactive: contentHeight > height

    ColumnLayout {
        id: body
        width: root.width
        spacing: 8

        // Не вошли — одна подпись вместо карточек
        EmptyState {
            visible: !root.signedIn
            Layout.fillWidth: true
            Layout.topMargin: 24
            icon: "group"
            title: I18n.t("Sign in to your account")
            text: I18n.t("Friend activity and Jams appear here after you sign in.")
        }

        JamCard {
            visible: root.signedIn
            Layout.fillWidth: true
        }

        // Шапка «Активность друзей»
        RowLayout {
            visible: root.signedIn
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.leftMargin: 8
            Layout.preferredHeight: 44
            spacing: 8
            Label {
                Layout.fillWidth: true
                text: root.signedIn && root.friends.length > 0
                      ? I18n.t("Friend activity — {0} listening", root.friends.length)
                      : I18n.t("Friend activity")
                type: "titleSmall"
            }
            Item {
                Layout.preferredWidth: 40
                Layout.preferredHeight: 40
                LoadingIndicator {
                    anchors.centerIn: parent
                    visible: Social.friendsLoading === true
                    running: visible && !Theme.calm
                    size: 28
                }
                IconButton {
                    anchors.centerIn: parent
                    visible: Social.friendsLoading !== true
                    icon: "refresh"
                    onClicked: Social.refreshFriends(true)
                }
            }
        }

        // Ошибка с «Повторить»
        ColumnLayout {
            visible: root.signedIn && (Social.friendsError ?? "") !== ""
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            spacing: 8
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Icon { name: "wifi_off"; size: 18; color: Theme.error }
                Label {
                    Layout.fillWidth: true
                    text: Social.friendsError ?? ""
                    type: "bodySmall"
                    color: Theme.error
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
            }
            Button {
                text: I18n.t("Retry")
                icon: "refresh"
                style: "tonal"
                size: "xs"
                onClicked: Social.refreshFriends(true)
            }
        }

        // Пусто
        ColumnLayout {
            visible: root.signedIn && Social.friendsLoaded === true && root.friends.length === 0
                     && (Social.friendsError ?? "") === ""
            Layout.fillWidth: true
            Layout.topMargin: 8
            spacing: 8
            MorphShape {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 64
                Layout.preferredHeight: 64
                shape: "cookie9"
                color: Theme.secondaryContainer
                Icon { anchors.centerIn: parent; name: "person_add"; size: 30; color: Theme.fgSecondaryContainer }
            }
            Label {
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                horizontalAlignment: Text.AlignHCenter
                text: I18n.t("No friend activity — follow friends in Spotify to see them here")
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
        }

        Repeater {
            model: root.signedIn ? root.friends : []
            FriendRow {
                required property var modelData
                Layout.fillWidth: true
                friend: modelData
                now: root.now
            }
        }
    }
}
