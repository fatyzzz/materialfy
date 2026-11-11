import os, shutil, faulthandler
faulthandler.enable()
from pathlib import Path
from tempfile import TemporaryDirectory
os.environ['QT_QPA_PLATFORM']='offscreen'
from PySide6.QtCore import QUrl, QPointF
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
from PySide6.QtQuick import QQuickItem
from PySide6.QtTest import QTest
workspace=TemporaryDirectory(prefix='materialfy-queue-test-')
base=Path(workspace.name)
md=base/'Md3'; core=base/'Materialfy/Core'
md.mkdir(parents=True,exist_ok=True);core.mkdir(parents=True,exist_ok=True)
repo=Path(__file__).resolve().parents[2]/'src/materialfy/qml'
shutil.copy(repo/'app/QueueView.qml',base/'QueueView.qml')
def module(path,name,types):
    lines=['module '+name]
    for n,(single,src) in types.items():
        (path/(n+'.qml')).write_text(src,encoding='utf-8')
        lines.append(('singleton ' if single else '')+n+' 1.0 '+n+'.qml')
    (path/'qmldir').write_text('\n'.join(lines)+'\n')
module(md,'Md3',{
'Theme':(True,'''pragma Singleton
import QtQuick
QtObject { function trackRowHeight(compact){return compact?56:64} property bool mobile:true; property bool reducedMotion:true;property color primary:"red"; property var shape:({large:12});property var motion:({spatialFast:0,spatialDefault:0,effectsFast:0,effectsDefault:0,emphasized:[0.2,0,0.2,1,1,1],emphasizedDecelerate:[0.2,0,0.2,1,1,1]}) }'''),
'Button':(False,'import QtQuick\nItem {property string text;property string icon;property string style;implicitHeight:40;implicitWidth:100;signal clicked()}'),
'EmptyState':(False,'import QtQuick\nItem {property string text;property string icon;property string shape;property string title;property bool animated}'),
'SmoothScroll':(False,'import QtQuick\nItem {property var flickable;property real wheelStep}'),
'Snackbar':(False,'import QtQuick\nItem {property real bottomMargin;function show(a,b,c){}}'),
})
module(core,'Materialfy.Core',{
'Player':(True,'''pragma Singleton
import QtQuick
QtObject {
 property bool remote:false; property bool isLocal:true;property bool hasTrack:true;property bool deviceLost:false;property bool canClearQueue:true;property bool queueReorderable:true
 property int manualCount:0;property int contextRemaining:0;property string contextTitle:"Album";property string contextUri:"";property var device:({name:"Phone"});property var queueContext:({uri:"album"})
 property var queue: ListModel { signal rowsChanged()
  function find(key,value) { for(let i=0;i<count;i++)if(get(i)[key]===value)return i;return -1 }
 }
 function setup(manual,context) {
  queue.clear();queue.append({uid:"current",title:"Current",section:"current"})
  for(let i=0;i<manual;i++)queue.append({uid:"manual"+i,title:"Manual"+i,section:"manual"})
  for(let i=0;i<context;i++)queue.append({uid:"context"+i,title:"Context"+i,section:"context"})
  manualCount=manual;contextRemaining=context;queue.rowsChanged()
 }
 function moveManual(from,to) {queue.move(from+1,to+1,1);queue.rowsChanged()}
 function clearManualQueue() {if(manualCount>0)queue.remove(1,manualCount);manualCount=0;queue.rowsChanged()}
 function addToQueue(t) {}
 function insertManual(t,i) {}
 function playQueueRow(uid) {}
}'''),
'I18n':(True,'pragma Singleton\nimport QtQuick\nQtObject {function t(s,v){return s.replace("{0}",v??"")};function n(n,s,p){return (n===1?s:p).replace("{0}",n)}}'.replace('};function','} function')),
'Settings':(True,'pragma Singleton\nimport QtQuick\nQtObject {property int wheelStep:48}'),
'Catalog':(True,'pragma Singleton\nimport QtQuick\nQtObject {function openUri(uri){}}'),
})
stubs={
'ConnectBanner':'Item {property string variant;implicitHeight:0}',
'QueueContextCard':'Item {property var context;signal clicked();implicitHeight:48;height:implicitHeight}',
'QueueDropZone':'Item {property bool active;implicitHeight:40;height:implicitHeight}',
'QueueSectionHeader':'Item {objectName:"qa-header";property string title;property int count;property string actionIcon;property string actionText;property string note;signal action();implicitHeight:32;height:implicitHeight}',
'QueueRow':'Item {objectName:"qa-row-"+(track?.uid??"");property var track;property bool current;property bool compact;property bool draggable;property bool reserveHandle;property bool dragging;signal activated();signal dragStarted();signal dragMoved(real dy);signal dragFinished()}',
}
for n,src in stubs.items():(base/(n+'.qml')).write_text('import QtQuick\n'+src)
qml='''import QtQuick
import Materialfy.Core
Window {width:360;height:800;visible:true
 QueueView {id:v;anchors.fill:parent;compact:true}
 function setup(m,c){Player.setup(m,c)}
 function manualRows(){return v.manualRows}
 function startDrag(){v.beginDrag(0);v.updateDrag(v.rowHeight)}
 function finishDrag(){v.endDrag()}
 function dropIndex(y){return v._dropIndexAt(180,y)}
}'''
app=QGuiApplication([]);engine=QQmlEngine();engine.addImportPath(str(base));c=QQmlComponent(engine)
c.setData(qml.encode(),QUrl.fromLocalFile(str(base/'test.qml')))
if c.isError():raise RuntimeError('\n'.join(e.toString() for e in c.errors()))
w=c.create();assert w
def headers():
    items=[o for o in visual_items() if o.objectName()=='qa-header' and o.isVisible() and painted(o)]
    return sorted([(round(o.mapToScene(QPointF()).y(),1),o.property('title')) for o in items])
def visual_items():
    result=[];pending=[w.contentItem()]
    while pending:
        item=pending.pop();result.append(item);pending.extend(item.childItems())
    return result
def painted(item):
    opacity=1
    while item is not None:
        opacity*=item.opacity();item=item.parentItem()
    return opacity>.01
def run_checks():
    for m in [0,1,3,2]*20:
        w.setup(m,4);QTest.qWait(50)
        expected=['Now playing']+(['Next in queue'] if m else [])+['Next from “Album”']
        actual=[title for y,title in headers()]
        assert actual==expected,(m,headers())
        assert w.manualRows()==m
    print('PASS: real QueueView keeps correct headers through 80 model resets/inserts/removals',flush=True)
    w.setup(2,4);QTest.qWait(40)
    first=next(o for o in visual_items() if o.objectName()=='qa-row-manual0' and painted(o))
    middle=first.mapToScene(QPointF()).y()+28
    assert w.dropIndex(middle-1)==0 and w.dropIndex(middle+1)==1, 'Drop offset ignores header/context card'
    print('PASS: drop positions include context card and section header height',flush=True)
    before=headers();w.startDrag();QTest.qWait(5)
    row=next((o for o in visual_items() if o.objectName()=='qa-row-manual0'),None)
    assert row is not None
    assert headers()==before,'Dragging moved the section header'
    assert round(row.y(),1)==88.0,('Row must include header height + drag offset',row.y())
    w.finishDrag();QTest.qWait(80)
    assert [title for y,title in headers()]==['Now playing','Next in queue','Next from “Album”']
    print('PASS: manual drag keeps heading stationary and preserves headings after reorder',flush=True)

try:
    run_checks()
finally:
    w.close()
    import shiboken6
    shiboken6.delete(w)
    shiboken6.delete(engine)
    QTest.qWait(10)
    workspace.cleanup()
