import QtQuick
import Md3
import Materialfy.Core

// Текст-ссылка (исполнитель, альбом): подчёркивается при наведении. Кликабелен только сам текст,
// а не пустое место справа — там клик достаётся строке под ним.
Label {
    id: root

    property bool linkEnabled: true
    // Карточка исполнителя при наведении (через Catalog — её показывает главное окно)
    property string previewArtistId
    signal clicked()

    font.underline: area.containsMouse && linkEnabled

    MouseArea {
        id: area
        width: Math.min(root.width, root.implicitWidth)
        height: root.height
        enabled: root.linkEnabled
        hoverEnabled: !Theme.mobile
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            hoverDelay.stop()
            Catalog.cancelPreview()
            root.clicked()
        }
        onContainsMouseChanged: {
            if (root.previewArtistId === "")
                return
            if (containsMouse) {
                hoverDelay.restart()
            } else {
                hoverDelay.stop()
                Catalog.cancelPreview()
            }
        }
    }
    Timer {
        id: hoverDelay
        interval: 600
        onTriggered: {
            const p = area.mapToItem(null, 0, area.height)
            Catalog.previewArtist(root.previewArtistId, p.x, p.y, area.width)
        }
    }
}
