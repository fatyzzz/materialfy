"""Exercise real player lifecycle code and SheetMenu pointer gestures offscreen."""

import os
import shutil
from pathlib import Path
from tempfile import TemporaryDirectory

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QPoint, Qt, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine
from PySide6.QtQuick import QQuickItem
from PySide6.QtTest import QTest
import shiboken6

REPO = Path(__file__).resolve().parents[2]
QML = REPO / "src/materialfy/qml"


def block(source, marker):
    start = source.index(marker)
    brace = source.index("{", start)
    depth = 0
    for end in range(brace, len(source)):
        depth += (source[end] == "{") - (source[end] == "}")
        if depth == 0:
            return source[start:end + 1]
    raise AssertionError("Unclosed QML block: " + marker)


def visual_items(item):
    yield item
    for child in item.childItems():
        yield from visual_items(child)


app = QGuiApplication([])
with TemporaryDirectory(prefix="materialfy-motion-test-") as directory:
    base = Path(directory)
    md = base / "Md3"
    md.mkdir()
    shutil.copy(QML / "Md3/BackStack.qml", md / "BackStack.qml")
    sheet_source = (QML / "Md3/SheetMenu.qml").read_text(encoding="utf-8-sig")
    sheet_source = sheet_source.replace("id: sheet", 'id: sheet\n        objectName: "test-sheet"', 1)
    (md / "SheetMenu.qml").write_text(sheet_source, encoding="utf-8")
    (md / "Theme.qml").write_text('''pragma Singleton
import QtQuick
QtObject { id:theme
 property var type:({titleMedium:{line:24},bodySmall:{line:16},bodyLarge:{line:24}})
 property bool mobile:true; property bool lowPower:false; property bool reducedMotion:false
 property color scrim:"black";property color surfaceContainerLow:"white";property color fgSurface:"black";property color fgSurfaceVariant:"gray";property color outlineVariant:"gray";property color primary:"purple"
 property int safeTop:24;property int safeBottom:16
 property QtObject motion: QtObject {
  property int spatialFast:theme.reducedMotion?0:200;property int spatialDefault:theme.reducedMotion?0:280;property int effectsDefault:150
  property var emphasized:[0.2,0,0,1,1,1];property var emphasizedDecelerate:[0.05,0.7,0.1,1,1,1];property var emphasizedAccelerate:[0.3,0,0.8,0.15,1,1];property var springDefault:emphasized
 }
 property var shape:({extraLarge:28,medium:12});property var stateLayer:({disabledContent:0.38})
}''', encoding="utf-8")
    stubs = {
        "MorphImage": "Image {property string shape;property string fallbackTitle}",
        "Label": "Text {property string type;property int weight}",
        "Icon": "Item {property string name;property int size;property color color}",
        "StateLayer": "MouseArea {}",
    }
    lines = ["module Md3", "singleton Theme 1.0 Theme.qml", "singleton BackStack 1.0 BackStack.qml", "SheetMenu 1.0 SheetMenu.qml"]
    for name, source in stubs.items():
        (md / f"{name}.qml").write_text("import QtQuick\n" + source, encoding="utf-8")
        lines.append(f"{name} 1.0 {name}.qml")
    (md / "qmldir").write_text("\n".join(lines) + "\n", encoding="utf-8")

    source = (QML / "app/NowPlaying.qml").read_text(encoding="utf-8-sig")
    pieces = [block(source, marker) for marker in [
        "function show(from)", "function hide()",
        "ParallelAnimation {\n        id: dismiss", "NumberAnimation {\n        id: dropBack",
    ]]
    scene = '''import QtQuick
import Md3
Window { id:window; width:360;height:640;visible:true
 Item { id:root;anchors.fill:parent
  property bool open:true;property bool sheetOpen:false;property real _dropY:0;property bool _miniReleased:false
  property real collapse:0;property bool _swipeClosing:false;property bool _tabsWarmed:true;property var _windowShapes:["square"];property point origin:Qt.point(0,0)
  QtObject {id:blob;function jumpTo(shape){}}
  Timer {id:tabsWarmup;interval:1}
  QtObject {id:collapseIn;function stop(){}}
  QtObject {id:collapseBack;function stop(){}}
  PLAYER_CODE
 }
 SheetMenu {id:menu;anchors.fill:parent;z:2}
 function closePlayer(){root.hide()}
 function openPlayer(){root.show(null)}
 function isOpen(){return root.open}
 function dropY(){return root._dropY}
 function miniReleased(){return root._miniReleased}
 function reduceMotion(value){Theme.reducedMotion=value}
 function showMenu(){menu.open([{text:"One"},{text:"Two"},{text:"Three"}])}
 function menuOpen(){return menu.opened}
 function closeMenu(){menu.close()}
 function overlays(){return BackStack.hasOverlay}
}'''.replace("PLAYER_CODE", "\n".join(pieces))
    engine = QQmlEngine()
    engine.addImportPath(str(base))
    component = QQmlComponent(engine)
    component.setData(scene.encode("utf-8"), QUrl.fromLocalFile(str(base / "test.qml")))
    if component.isError():
        raise AssertionError("\n".join(error.toString() for error in component.errors()))
    window = component.create()
    assert window is not None
    try:
        QTest.qWait(80)
        window.closePlayer()
        QTest.qWait(120)
        before = window.dropY()
        assert before > 0 and window.miniReleased()
        window.openPlayer()
        assert abs(window.dropY() - before) < 1, "Reopen snapped the player"
        QTest.qWait(350)
        assert window.isOpen() and window.dropY() == 0 and not window.miniReleased(), "Old dismiss completed after reopen"
        window.reduceMotion(True)
        window.closePlayer()
        assert not window.isOpen() and window.dropY() == 0
        window.openPlayer()
        assert window.isOpen()
        window.reduceMotion(False)
        print("PASS: reopening cancels dismissal without a jump; reduced-motion close is immediate", flush=True)

        window.showMenu()
        QTest.qWait(350)
        sheet = next(item for item in visual_items(window.contentItem()) if item.objectName() == "test-sheet")
        for distance in [35, 100]:
            start = QPoint(180, int(sheet.y() + 14))
            QTest.mousePress(window, Qt.LeftButton, Qt.NoModifier, start)
            for dy in range(15, distance + 1, 5):
                QTest.mouseMove(window, start + QPoint(0, dy), 15)
            before = sheet.y()
            QTest.mouseRelease(window, Qt.LeftButton, Qt.NoModifier, start + QPoint(0, distance))
            assert sheet.y() >= before - 1, "Releasing the grip snapped the sheet upward"
            QTest.qWait(350)
            if distance == 35:
                assert window.menuOpen() and window.overlays()
            else:
                assert not window.menuOpen() and not window.overlays()
        print("PASS: short drag settles smoothly; dismiss continues from release position; overlay guard clears", flush=True)
        window.reduceMotion(True)
        window.showMenu()
        QTest.qWait(30)
        before = sheet.y()
        window.closeMenu()
        QTest.qWait(50)
        assert abs(sheet.y() - before) < 1 and sheet.opacity() < 1, "Reduced-motion sheet still slides on exit"
        QTest.qWait(150)
        assert not window.overlays()
        print("PASS: reduced-motion sheet fades without an automatic slide", flush=True)
    finally:
        window.close()
        shiboken6.delete(window)
        shiboken6.delete(engine)
