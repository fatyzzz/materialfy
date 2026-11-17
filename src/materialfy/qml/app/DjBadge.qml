import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// The "DJ" pill on the cover while Spotify's DJ talks between tracks on this computer.
// Placed in a corner of the cover with anchors, like DeviceBadge.
Rectangle {
    id: root

    readonly property bool shown: Player.narrating === true

    implicitWidth: Math.min(260, parent ? parent.width : 260, row.implicitWidth + 20)
    implicitHeight: Math.max(32, Theme.type.labelLarge.line + 12)
    radius: height / 2
    color: Theme.tertiaryContainer
    scale: shown ? 1 : (Theme.reducedMotion ? 1 : 0.94)
    opacity: shown ? 1 : 0
    visible: opacity > 0.01
    Accessible.role: Accessible.StaticText
    Accessible.name: Player.playing ? I18n.t("DJ is speaking") : I18n.t("DJ paused")
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
    Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        spacing: 6
        Icon {
            Layout.alignment: Qt.AlignVCenter
            tight: true
            name: "mic"
            size: 18
            color: Theme.fgTertiaryContainer
        }
        Label {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            text: Player.playing ? I18n.t("DJ is speaking") : I18n.t("DJ paused")
            type: "labelLarge"
            weight: 600
            color: Theme.fgTertiaryContainer
        }
    }

    HoverHandler { id: hover }
    Rectangle {
        visible: hover.hovered
        anchors.top: parent.bottom
        anchors.topMargin: 6
        anchors.left: parent.left
        width: tip.implicitWidth + 20
        height: tip.implicitHeight + 12
        radius: Theme.shape.small
        color: Theme.inverseSurface
        Label {
            id: tip
            anchors.centerIn: parent
            text: I18n.t("The DJ is talking. Turn the voice off in Settings → Playback")
            type: "bodySmall"
            color: Theme.fgInverseSurface
        }
    }
}
