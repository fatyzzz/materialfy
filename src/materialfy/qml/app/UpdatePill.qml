import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import Md3
import Materialfy.Core

// Плашка обновления в боковой панели (над карточкой аккаунта): «Доступно обновление · 0.2.0», во время
// загрузки — кольцо прогресса вокруг иконки и проценты, потом «Перезапустить и обновить». В рейле остаются
// иконка с кольцом (подписи гаснут, как у остальных пунктов). Клик — окно обновления (UpdateSheet).
// Пропущенная версия плашку не показывает.
Item {
    id: root

    property bool rail: false
    signal clicked()

    readonly property string st: Updater.state
    readonly property bool wanted: (st === "available" && !Updater.skipped) || st === "downloading" || st === "verifying"
                                   || st === "readyToInstall" || st === "installing"
                                   || (st === "error" && Updater.version !== "")
    readonly property bool failed: st === "error"

    readonly property string title: {
        switch (st) {
        case "downloading": return I18n.t("Downloading update")
        case "verifying": return I18n.t("Verifying update")
        case "readyToInstall": return I18n.t("Update ready")
        case "installing": return I18n.t("Installing update")
        case "error": return I18n.t("Update failed")
        default: return I18n.t("Update available")
        }
    }
    readonly property string detail: {
        switch (st) {
        case "downloading": return Math.floor(Updater.progress * 100) + " %"
        case "verifying": return I18n.t("Verifying…")
        case "readyToInstall": return Updater.installKind === "mac" || Updater.installKind === "apk" ? I18n.t("Open installer") : I18n.t("Restart to update")
        case "installing": return I18n.t("Restarting…")
        case "error": return I18n.t("Details and retry")
        default: return I18n.t("Version {0}", Updater.version)
        }
    }

    implicitHeight: wanted ? 56 : 0
    Layout.preferredHeight: implicitHeight
    Layout.topMargin: wanted ? 8 : 0
    visible: implicitHeight > 0.5
    opacity: wanted ? 1 : 0
    clip: true
    Behavior on implicitHeight { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

    Rectangle {
        id: bg
        width: parent.width
        height: 56
        radius: Theme.shape.large
        color: root.failed ? Theme.errorContainer : Theme.primaryContainer
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }

        // иконка на оси рейла (x = 40 от края панели) с кольцом прогресса
        Item {
            id: badge
            width: 40
            height: 40
            x: 28 - width / 2
            anchors.verticalCenter: parent.verticalCenter

            Shape {
                anchors.fill: parent
                visible: root.st === "downloading" || root.st === "verifying"
                preferredRendererType: Shape.CurveRenderer
                ShapePath {
                    strokeColor: Qt.alpha(Theme.fgPrimaryContainer, 0.16)
                    strokeWidth: 3
                    fillColor: "transparent"
                    PathAngleArc { centerX: 20; centerY: 20; radiusX: 18; radiusY: 18; startAngle: 0; sweepAngle: 360 }
                }
                ShapePath {
                    strokeColor: Theme.fgPrimaryContainer
                    strokeWidth: 3
                    fillColor: "transparent"
                    capStyle: ShapePath.RoundCap
                    PathAngleArc {
                        centerX: 20; centerY: 20; radiusX: 18; radiusY: 18
                        startAngle: -90
                        sweepAngle: root.st === "verifying" ? 360 : Math.max(4, 360 * Updater.progress)
                        Behavior on sweepAngle { NumberAnimation { duration: 160 } }
                    }
                }
            }
            Icon {
                anchors.centerIn: parent
                name: root.failed ? "error" : root.st === "readyToInstall" || root.st === "installing" ? "restart_alt" : "download"
                size: root.st === "downloading" || root.st === "verifying" ? 20 : 24
                fill: 1
                color: root.failed ? Theme.fgErrorContainer : Theme.fgPrimaryContainer
            }
            // точка «есть новое» в рейле, пока обновление только предлагается
            Rectangle {
                visible: root.st === "available" && root.rail
                width: 8
                height: 8
                radius: 4
                x: parent.width - 10
                y: 4
                color: Theme.tertiary
            }
        }

        Column {
            x: badge.x + badge.width + 10
            width: parent.width - x - 12
            anchors.verticalCenter: parent.verticalCenter
            opacity: root.rail ? 0 : 1
            visible: opacity > 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            Label {
                width: parent.width
                text: root.title
                type: "titleSmall"
                color: root.failed ? Theme.fgErrorContainer : Theme.fgPrimaryContainer
            }
            Label {
                width: parent.width
                text: root.detail
                type: "bodySmall"
                color: root.failed ? Theme.fgErrorContainer : Theme.fgPrimaryContainer
                opacity: 0.8
                font.features: ({ "tnum": 1 })
            }
        }

        StateLayer {
            anchors.fill: parent
            radius: Theme.shape.large
            color: root.failed ? Theme.fgErrorContainer : Theme.fgPrimaryContainer
            onClicked: root.clicked()
        }
    }
}
