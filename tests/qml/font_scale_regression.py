"""Exercise real Theme typography and input/button layouts with a platform converter stub."""
import json
import os
import re
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
app = QGuiApplication([])
with TemporaryDirectory(prefix="materialfy-font-test-") as directory:
    base = Path(directory)
    md = base / "Md3"
    core = base / "Materialfy/Core"
    md.mkdir()
    core.mkdir(parents=True)
    files = ("Theme.qml", "Typography.js", "ColorMix.js", "Label.qml", "Button.qml",
             "Chip.qml", "SearchField.qml", "TextField.qml", "ButtonGroup.qml")
    for name in files:
        shutil.copy(QML / "Md3" / name, md / name)
    shutil.copy(QML / "app/MobileNavBar.qml", base / "MobileNavBar.qml")
    shutil.copy(QML / "app/SettingsNav.qml", base / "SettingsNav.qml")
    roles = re.findall(r': "(\w+)"', (md / "Theme.qml").read_text(encoding="utf-8-sig"))
    colors = ",".join(name + ':Qt.rgba(.2,.2,.2,1)' for name in dict.fromkeys(roles))
    (core / "ThemeEngine.qml").write_text('pragma Singleton\nimport QtQuick\nQtObject {property bool dark:false;property var colors:({' + colors + '});signal schemeChanged()}')
    (core / "Settings.qml").write_text('pragma Singleton\nimport QtQuick\nQtObject {property string animations:"reduced";property string lowPowerMode:"off";property bool systemReducedMotion:false}')
    (core / "MobileShell.qml").write_text('''pragma Singleton
import QtQuick
QtObject { property real fontScale:1
 function prepareAccessibility(item,owner){}
 // Deliberately nonlinear: headings and body text have different scaling ratios.
 function textSize(sp) {return sp<=18?sp*fontScale:sp+(fontScale-1)*9}
}''')
    (core / "qmldir").write_text("module Materialfy.Core\n" + "".join("singleton " + name + " 1.0 " + name + ".qml\n" for name in ("ThemeEngine", "Settings", "MobileShell")))
    stubs = {
        "I18n": 'pragma Singleton\nimport QtQuick\nQtObject {function t(value){return value}}',
        "Icon": 'import QtQuick\nItem {property string name;property real size:24;property color color;property bool fill:false;property real weight:400;property bool tight:false;implicitWidth:size;implicitHeight:size}',
        "IconButton": 'import QtQuick\nItem {property string size;property string icon;signal clicked();width:40;height:40}',
        "StateLayer": 'import QtQuick\nItem {property real radius;property real topLeftRadius;property real topRightRadius;property real bottomLeftRadius;property real bottomRightRadius;property color color;property bool ripple;property int cursorShape;property bool pressed:false;property bool containsMouse:false;signal clicked()}',
        "Elevation": 'import QtQuick\nQtObject {property Item target;property int level}',
    }
    for name, source in stubs.items():
        (md / (name + ".qml")).write_text(source)
    names = [n.removesuffix(".qml") for n in files if n.endswith(".qml")]
    (md / "qmldir").write_text("module Md3\n" + "".join(("singleton " if n in ("Theme", "I18n") else "") + n + " 1.0 " + n + ".qml\n" for n in names + list(stubs)))
    scene = '''import QtQuick
import Md3
import Materialfy.Core
Window {width:360;height:640;visible:true
 Label {id:label;type:"bodyLarge";text:"Large body"}
 Button {id:button;text:"Save"}
 Chip {id:chip;text:"Tracks"}
 SearchField {id:search}
 TextField {id:field;label:"Name"}
 SettingsNav {id:settingsNav;y:200;width:parent.width;sections:[{key:"appearance",text:"Оформление"}]}
 ButtonGroup {id:group;y:280;maxWidth:320;model:["Системные","Уменьшенные","Полные"]}
 MobileNavBar {id:nav;y:400;width:parent.width;items:[
   {name:"home",text:"Главная"},{name:"search",text:"Поиск"},
   {name:"library",text:"Библиотека"},{name:"settings",text:"Настройки"}]}
 function scale(value){Theme.mobile=true;MobileShell.fontScale=value}
 function desktop(){Theme.mobile=false}
 function captions(item,result){
   if(item.type==="labelMedium")result.push({size:item.font.pixelSize,bottom:item.mapToItem(nav,0,item.height).y})
   for(const child of item.children??[])captions(child,result)
 }
 function state(){let labels=[];captions(nav,labels);return JSON.stringify({body:Theme.type.bodyLarge,heading:Theme.type.headlineLarge,
   labelSize:label.font.pixelSize,labelHeight:label.implicitHeight,
   buttonHeight:button.implicitHeight,chipHeight:chip.implicitHeight,
   searchHeight:search.implicitHeight,fieldHeight:field.implicitHeight,
   navHeight:nav.implicitHeight,navLabels:labels,rowHeight:Theme.trackRowHeight(false),
   groupHeight:group.implicitHeight,settingsNavHeight:settingsNav.implicitHeight})}
}'''
    engine = QQmlEngine()
    engine.addImportPath(str(base))
    component = QQmlComponent(engine)
    component.setData(scene.encode(), QUrl.fromLocalFile(str(base / "test.qml")))
    assert not component.isError(), "\n".join(x.toString() for x in component.errors())
    window = component.create()
    assert window is not None
    try:
        QTest.qWait(50)
        window.scale(1)
        original = json.loads(window.state())
        assert original["body"]["size"] == 16 and original["body"]["line"] == 24
        window.scale(2)
        QTest.qWait(50)
        large = json.loads(window.state())
        assert large["body"]["size"] == 32 and large["heading"]["size"] == 41
        assert large["body"]["line"] == 48 and large["labelSize"] == 32
        assert large["buttonHeight"] >= 48, large
        assert large["chipHeight"] >= 48, large
        assert large["searchHeight"] >= large["body"]["line"] + 16, large
        assert large["fieldHeight"] >= large["body"]["line"] + 16, large
        assert len(large["navLabels"]) == 4 and all(label["size"] == 24 and label["bottom"] <= large["navHeight"] for label in large["navLabels"]), large
        assert large["rowHeight"] >= 108
        assert large["groupHeight"] >= 92 and large["settingsNavHeight"] >= 60, large
        window.desktop()
        assert json.loads(window.state())["body"]["size"] == 16
        print("PASS: live nonlinear text scaling, proportional line metrics, larger input/button bounds, desktop sizes preserved", flush=True)
    finally:
        window.close()
        shiboken6.delete(window)
        shiboken6.delete(engine)
