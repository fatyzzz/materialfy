"""Exercise DJ discovery, the real progress clock and cover swipe lifecycle in Qt."""
import os
import shutil
from pathlib import Path
from tempfile import TemporaryDirectory

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
from PySide6.QtTest import QTest
import shiboken6

QML = Path(__file__).resolve().parents[2] / "src/materialfy/qml"


def block(source, marker):
    start = source.index(marker)
    brace = source.index("{", start)
    depth = 0
    for end in range(brace, len(source)):
        depth += (source[end] == "{") - (source[end] == "}")
        if depth == 0:
            return source[start:end + 1]
    raise AssertionError(marker)


app = QGuiApplication([])
with TemporaryDirectory(prefix="materialfy-dj-") as directory:
    base = Path(directory)
    md = base / "Md3"
    core = base / "Materialfy/Core"
    md.mkdir(); core.mkdir(parents=True)
    (md / "qmldir").write_text("module Md3\nsingleton Theme 1.0 Theme.qml\nTickAnimation 1.0 TickAnimation.qml\n")
    (md / "Theme.qml").write_text('''pragma Singleton
import QtQuick
QtObject {property bool quiet:false;property var motion:({effectsDefault:1,spatialDefault:1,emphasizedDecelerate:[0,0,1,1,1,1]})}''')
    (md / "TickAnimation.qml").write_text('''import QtQuick
QtObject { property bool running:false;property real frameTime:0.1;signal triggered() }''')
    (core / "qmldir").write_text("module Materialfy.Core\nsingleton Player 1.0 Player.qml\nsingleton Home 1.0 Home.qml\nsingleton MobileShell 1.0 MobileShell.qml\n")
    (core / "Home.qml").write_text('''pragma Singleton
import QtQuick
QtObject {property var shelves:[]}''')
    (core / "MobileShell.qml").write_text('''pragma Singleton
import QtQuick
QtObject {function haptic(){}}''')
    (core / "Player.qml").write_text('''pragma Singleton
import QtQuick
QtObject {property bool playing:true;property bool narrating:false;property bool buffering:false
 property real position:0.2;property real positionMs:20000;property real durationMs:100000;property string uri:"old"
 property int nextCalls:0;property int previousCalls:0
 signal stateChanged();signal trackChanged();signal positionChanged()
 function next(){nextCalls++} function previous(){previousCalls++}
}'''.replace(';signal positionChanged()', ''))  # position already has an automatic notify signal
    shutil.copy(QML / "app/PlayerProgress.qml", base / "PlayerProgress.qml")
    home = (QML / "app/pages/HomePage.qml").read_text(encoding="utf-8-sig")
    player = (QML / "app/NowPlaying.qml").read_text(encoding="utf-8-sig")
    swipe = player[player.index('    property real _coverDx:'):player.index('    // большая обложка')]
    scene = '''import QtQuick
import Md3
import Materialfy.Core
Item {id:page;width:360;height:640
 property bool signedIn:true;property var djEntry:null;property var shelfIds:[]
 QtObject {id:shelfRepeater;property var loaders:[];function itemAt(i){if(!loaders[i])loaders[i]={};return loaders[i]}}
 HOME_CODE
 PlayerProgress {id:progress}
 Item {id:root;width:360;height:640
  Item {id:bigCover;width:300;property bool pending:false}
  SWIPE_CODE
 }
 function discover(){Home.shelves=[{key:"madeForYou",kind:"tiles",items:[{kind:"dj",uri:"DJ"},{kind:"playlist",uri:"mix"}]},{key:"djOnly",items:[{kind:"dj",uri:"DJ"}]}];_syncShelves();return djEntry.uri+":"+shelfIds.length+":"+shelfRepeater.itemAt(0).shelfData.items[0].uri}
 function signOut(){signedIn=false;_syncShelves();return djEntry===null&&shelfIds.length===0}
 function tick(){if(progress._tick.running)progress._tick.triggered();return progress.value}
 function voice(on){Player.narrating=on;Player.stateChanged()}
 function buffer(on){Player.buffering=on;Player.stateChanged()}
 function swipe(dir){root._coverSwipeAway(dir)}
 function metadataOnly(){Player.trackChanged()}
 function newTrack(){Player.uri="new";Player.trackChanged()}
 function restartSong(){Player.positionMs=0;Player.positionChanged()}
 function direction(){return root._coverDir}
 function calls(){return Player.nextCalls+Player.previousCalls}
}'''.replace("HOME_CODE", block(home, "function _syncShelves()")).replace("SWIPE_CODE", swipe)
    engine = QQmlEngine(); engine.addImportPath(str(base))
    component = QQmlComponent(engine)
    component.setData(scene.encode(), QUrl.fromLocalFile(str(base / "test.qml")))
    if component.isError():
        raise RuntimeError("\n".join(e.toString() for e in component.errors()))
    item = component.create(); assert item
    try:
        assert item.discover() == "DJ:1:mix", "DJ duplicated in ordinary shelves or empty shelf retained"
        assert item.signOut(), "DJ survived sign-out"
        assert item.tick() > 0.2
        item.voice(True)
        assert item.tick() == 0.2 and item.tick() == 0.2, "Track clock advanced during speech"
        item.voice(False); assert item.tick() > 0.2
        item.buffer(True); assert item.tick() == 0.2, "Track clock advanced while buffering"
        item.buffer(False)
        item.swipe(-1); QTest.qWait(30)
        assert item.calls() == 1
        item.metadataOnly(); QTest.qWait(30)
        assert item.direction() == -1, "Same-track metadata prematurely returned old cover"
        item.newTrack(); QTest.qWait(30)
        assert item.direction() == 0, "New song with same cover did not settle"
        item.swipe(1); QTest.qWait(30)
        item.restartSong(); QTest.qWait(30)
        assert item.direction() == 0, "Previous restarting same song left cover offscreen"
        assert item.calls() == 2, "A swipe dispatched more than one transport command"
        print("PASS: DJ discovery/deduplication/sign-out, narration/buffering clocks, metadata races and previous restart")
    finally:
        shiboken6.delete(item); shiboken6.delete(engine)
