import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// «Браузер не открылся?» — открыть ссылку ещё раз или скопировать её (адрес ответа — чип у «Отмена» на странице).
ColumnLayout {
    id: root

    signal copied()

    spacing: 8

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: grid.implicitHeight + 32
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh

        GridLayout {
            id: grid
            x: 20
            y: 16
            width: parent.width - 40
            columns: width >= 460 ? 2 : 1
            columnSpacing: 16
            rowSpacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Browser didn’t open?")
                    type: "titleSmall"
                }
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Open the page again or copy the link into the browser you want")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
            }
            // Flow: в узком окне кнопки переносятся (у RowLayout минимум — сумма кнопок)
            Flow {
                // в две колонки — своей ширины (одной строкой), в одну — на всю ширину с переносом
                Layout.fillWidth: grid.columns === 1
                spacing: 8
                Button {
                    text: I18n.t("Open again")
                    icon: "open_in_new"
                    style: "tonal"
                    onClicked: Auth.reopenBrowser()
                }
                Button {
                    text: copiedHint.running ? I18n.t("Copied") : I18n.t("Link")
                    icon: copiedHint.running ? "check" : "content_copy"
                    style: "outlined"
                    onClicked: {
                        Auth.copyAuthUrl()
                        copiedHint.restart()
                        root.copied()
                    }
                    Timer { id: copiedHint; interval: 1500 }
                }
            }
        }
    }
}
