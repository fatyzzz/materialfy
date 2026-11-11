// Сценарии навигации Router на настоящем мосту (Md3-синглтоны из приложения):
//   QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 \
//     target/release/materialfy --offline --new-instance --qml tests/qml/integration/RouterHarness.qml
// 1. Загрузка LibraryPage (дубликат метода prewarmEntry раньше валил весь файл).
// 2. reset("settings") из плейлиста: settings — keepAlive-страница, её живой экземпляр в глубине
//    стека; reset обязан заменить стек (depth 1, current settings), а не потеряться.
// 3. Клик во время hover-прогрева той же страницы: прогрев повышается до forward-перехода.
// Итог — SMOKE-RESULT PASS/FAIL.
import QtQuick
import QtQuick.Controls.Basic
import Md3
import Materialfy.Core
import "../../../src/materialfy/qml/app" as App
import "../../../src/materialfy/qml/app/pages" as Pages

ApplicationWindow {
    id: win
    width: 900
    height: 700
    visible: true

    property int _fails: 0
    function check(name, cond) {
        console.log(cond ? "SMOKE-PASS " + name : "SMOKE-FAIL " + name)
        if (!cond) _fails++
    }
    function finish() {
        console.log(_fails === 0 ? "SMOKE-RESULT PASS" : "SMOKE-RESULT FAIL")
        Qt.quit()
    }

    App.Router {
        id: router
        anchors.fill: parent
        keepAliveSlots: 6
        // settings обязана быть keepAlive-страницей: иначе сценарий 2 строит свежую страницу
        // и проходит даже с ломаным reset (in-stack _kept путь не проверяется)
        keepAlive: ["home", "settings"]
        pages: ({
            home: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/HomePage.qml")),
            playlist: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/PlaylistPage.qml")),
            settings: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/SettingsPage.qml")),
            library: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/LibraryPage.qml")),
        })
        Component.onCompleted: reset("home")
    }

    // Сценарий 1: LibraryPage грузится (мост Library настоящий)
    Timer { interval: 400; running: true; onTriggered: {
        let loaded = false
        try {
            const comp = Qt.createComponent(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/LibraryPage.qml"))
            loaded = comp.status === Component.Ready
            if (comp.status === Component.Error)
                console.log("LibraryPage error:", comp.errorString())
        } catch (e) {
            console.log("LibraryPage threw:", e)
        }
        check("library page compiles", loaded)

        // Сценарий 2: reset на keepAlive-страницу, чей экземпляр жив в глубине стека.
        // Предварительно открываем settings (уходит в _kept), возвращаемся на home, открываем
        // плейлист, затем reset("settings").
        router.reset("home")
        Qt.callLater(runScenario2)
    } }

    function runScenario2() {
        router.push("settings")
        timerToScenario2b.restart()
    }
    Timer { id: timerToScenario2b; interval: 1200; onTriggered: runScenario2b() }
    function runScenario2b() {
        // settings инкубировалась асинхронно и живёт в _kept; уходим на home и в плейлист
        router.reset("home")
        router.push("playlist", { playlistId: "r", playlistTitle: "R", cover: "" })
        timerToScenario2c.restart()
    }
    Timer { id: timerToScenario2c; interval: 900; onTriggered: runScenario2c() }
    function runScenario2c() {
        router.reset("settings")
        // инкубация settings асинхронная — check после её завершения
        timerToCheck2.restart()

    }
    Timer { id: timerToCheck2; interval: 800; onTriggered: {
        check("reset to in-stack keepAlive page replaces the stack",
              router.depth === 1 && router.currentName === "settings")
        startScenario3()
    } }
    function startScenario3() {
        router.reset("home")
        router.prewarmKeyed("playlist", { playlistId: "w", playlistTitle: "W", cover: "" })
        check("prewarm pending", router._pending !== null && router._pending.kind === "prewarm")
        router.push("playlist", { playlistId: "w", playlistTitle: "W", cover: "" })
        timerToScenario3.restart()
    }
    Timer { id: timerToScenario3; interval: 900; onTriggered: {
        check("click during prewarm lands on the page",
              router.currentName === "playlist" && router.depth >= 1)
        finish()
    } }
}
