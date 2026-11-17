import QtQuick
import QtQuick.Layouts
import Md3

// Заголовок секции страницы с необязательной кнопкой справа («Все», «Свернуть»).
RowLayout {
    id: root

    property string text
    property string actionText
    signal action()

    Layout.fillWidth: true
    Layout.topMargin: 8
    spacing: 12

    Label {
        Layout.fillWidth: true
        text: root.text
        type: Theme.mobile ? "titleLarge" : "headlineSmall"
        weight: 600
        // узкий экран: в две строки, а не с многоточием
        wrapMode: Theme.mobile ? Text.WordWrap : Text.NoWrap
        maximumLineCount: 3
    }
    Button {
        visible: root.actionText !== ""
        text: root.actionText
        style: "text"
        onClicked: root.action()
    }
}
