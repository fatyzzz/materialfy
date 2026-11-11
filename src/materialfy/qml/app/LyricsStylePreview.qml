import QtQuick

// Превью в настройках — это настоящий LyricsView со своим текстом и часами:
// тот же шейдер заливки, масштаб активной строки, размытие и паузы, а не отдельная имитация.
Item {
    id: root

    property real baseFontSize: 20
    property real position: 0
    implicitHeight: 168
    clip: true

    LyricsView {
        id: view
        anchors.fill: parent
        clip: true
        baseFontSize: root.baseFontSize
        previewMode: true
        previewLines: root.demo
        previewPos: root.position
        showSource: false
    }

    // Runs only while the card is really on screen (the Settings page is kept alive by the router).
    // FrameAnimation + elapsed wall time keeps the wave smooth on 120/144 Hz displays too.
    property real _lastTick: 0
    readonly property bool onScreen: {
        let item = root
        while (item) {
            if (!item.visible)
                return false
            item = item.parent
        }
        return true
    }

    FrameAnimation {
        running: root.onScreen && view.live
        onTriggered: {
            const now = Date.now()
            const dt = root._lastTick > 0 ? Math.min(0.1, (now - root._lastTick) / 1000) : 0.016
            root._lastTick = now
            root.position += dt
            if (root.position > 12.4) {
                root.position = 0
                view.layoutRevision++
            }
            view.tick()
        }
    }

    property var demo: [
        {
            t: 0.0, e: 2.2, i: false,
            w: [
                { x: "When ", s: 0.0, e: 0.42 },
                { x: "the ", s: 0.42, e: 0.70 },
                { x: "music ", s: 0.70, e: 1.18 },
                { x: "starts ", s: 1.18, e: 1.62 },
                { x: "to ", s: 1.62, e: 1.82 },
                { x: "play", s: 1.82, e: 2.2 }
            ]
        },
        {
            t: 2.2, e: 5.0, i: false,
            w: [
                { x: "Every ", s: 2.2, e: 2.7 },
                { x: "rhythm ", s: 2.7, e: 3.35, p: [[2, 2.7, 2.95], [4, 2.95, 3.35]] },
                { x: "guides ", s: 3.35, e: 3.95 },
                { x: "the ", s: 3.95, e: 4.25 },
                { x: "way", s: 4.25, e: 5.0 }
            ]
        },
        {
            t: 5.0, e: 7.4, i: false,
            w: [
                { x: "Singing ", s: 5.0, e: 5.6 },
                { x: "through ", s: 5.6, e: 6.1 },
                { x: "the ", s: 6.1, e: 6.35 },
                { x: "night ", s: 6.35, e: 6.85 },
                { x: "and ", s: 6.85, e: 7.1 },
                { x: "day", s: 7.1, e: 7.4 }
            ]
        },
        {
            t: 7.4, e: 9.6, i: false,
            w: [
                { x: "A ", s: 7.4, e: 7.6 },
                { x: "melody ", s: 7.6, e: 8.25, p: [[3, 7.6, 7.85], [3, 7.85, 8.25]] },
                { x: "that ", s: 8.25, e: 8.6 },
                { x: "never ", s: 8.6, e: 9.1 },
                { x: "fades", s: 9.1, e: 9.6 }
            ]
        },
        {
            t: 9.6, e: 12.2, i: false,
            w: [
                { x: "It ", s: 9.6, e: 9.9 },
                { x: "carries ", s: 9.9, e: 10.5 },
                { x: "us ", s: 10.5, e: 10.9 },
                { x: "away", s: 10.9, e: 12.2 }
            ]
        }
    ]
}
