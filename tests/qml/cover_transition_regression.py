"""Check actual cover-buffer transitions without network/image decoding dependencies."""

import json
import os
import shutil
from pathlib import Path
from tempfile import TemporaryDirectory

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine
from PySide6.QtTest import QTest
import shiboken6

QML = Path(__file__).resolve().parents[2] / "src/materialfy/qml"
app = QGuiApplication([])
with TemporaryDirectory(prefix="materialfy-cover-test-") as directory:
    base = Path(directory)
    md = base / "Md3"
    core = base / "Materialfy/Core"
    md.mkdir()
    core.mkdir(parents=True)
    shutil.copy(QML / "app/NowPlayingCover.qml", base / "NowPlayingCover.qml")
    (md / "qmldir").write_text("module Md3\nsingleton Theme 1.0 Theme.qml\nMorphImage 1.0 MorphImage.qml\nMaskSpin 1.0 MaskSpin.qml\n")
    (md / "Theme.qml").write_text('''pragma Singleton
import QtQuick
QtObject {id:theme;property bool mobile:true;property bool reducedMotion:false;property bool lowPower:false
 property QtObject motion:QtObject {property int spatialDefault:280;property int effectsDefault:150;property int spatialSlow:theme.reducedMotion?0:380}
}''')
    (md / "MorphImage.qml").write_text('''import QtQuick
Item {property url source;property var shape;property bool eager;property real pulse;property int duration
 property string fallbackKind;property string fallbackArtist;property string fallbackTitle
 readonly property int status:source.toString().endsWith("slow")?Image.Loading:Image.Ready
 function jumpTo(value){shape=value}
}''')
    (md / "MaskSpin.qml").write_text("import QtQuick\nQtObject {property Item target;property bool spinning}")
    (core / "qmldir").write_text("module Materialfy.Core\nsingleton Player 1.0 Player.qml\n")
    (core / "Player.qml").write_text('''pragma Singleton
import QtQuick
QtObject {property string album:"Album";property string artist:"Artist";property string title:"Track"}''')
    scene = '''import QtQuick
import Md3
Window {width:360;height:640;visible:true
 NowPlayingCover {id:cover;anchors.fill:parent}
 function changeSource(value){cover.source=value}
 function reduceMotion(){Theme.reducedMotion=true}
 function setCoverVisible(value){cover.visible=value}
 function state(){return JSON.stringify({front:cover._front,layers:cover._layers.map(item=>({scale:item.scale,rotation:item.rotation,opacity:item.opacity,source:item.source.toString()}))})}
}'''
    engine = QQmlEngine()
    engine.addImportPath(str(base))
    component = QQmlComponent(engine)
    component.setData(scene.encode(), QUrl.fromLocalFile(str(base / "test.qml")))
    if component.isError():
        raise AssertionError("\n".join(error.toString() for error in component.errors()))
    window = component.create()
    assert window is not None
    try:
        QTest.qWait(60)
        window.changeSource("one")
        window.changeSource("two")
        state = json.loads(window.state())
        incoming = state["layers"][state["front"]]
        assert incoming["scale"] >= .95 and incoming["rotation"] == 0, "New cover shrank/spun excessively"
        QTest.qWait(70)
        state = json.loads(window.state())
        assert sum(item["opacity"] for item in state["layers"]) > .8, "Cover transition left a blank frame"
        window.changeSource("three")
        QTest.qWait(350)
        state = json.loads(window.state())
        incoming = state["layers"][state["front"]]
        assert incoming["source"].endswith("three") and incoming["scale"] == 1 and incoming["opacity"] == 1
        window.reduceMotion()
        window.changeSource("four")
        state = json.loads(window.state())
        incoming = state["layers"][state["front"]]
        assert incoming["scale"] == 1 and incoming["rotation"] == 0 and incoming["opacity"] == 1
        window.changeSource("slow")
        window.changeSource("newest")
        QTest.qWait(350)
        state = json.loads(window.state())
        assert state["layers"][state["front"]]["source"].endswith("newest"), "An old ready callback swapped the cover twice"
        window.changeSource("slow")
        window.changeSource("newest")
        QTest.qWait(1600)
        state = json.loads(window.state())
        assert state["layers"][state["front"]]["source"].endswith("newest"), "Returning to the current cover kept a stale pending load"
        window.changeSource("slow")
        window.setCoverVisible(False)
        window.changeSource("latest")
        QTest.qWait(1600)
        window.setCoverVisible(True)
        state = json.loads(window.state())
        incoming = state["layers"][state["front"]]
        assert incoming["source"].endswith("latest"), "Pending old cover replaced the latest cover while the player was hidden"
        print("PASS: gentle cover crossfade, rapid source replacement, reduced-motion swap without spatial animation", flush=True)
    finally:
        window.close()
        shiboken6.delete(window)
        shiboken6.delete(engine)
