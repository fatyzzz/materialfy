"""Minimal Qt reproducer for delayed accessibility activation from a foreign thread.

The production Android helper resolves the same attached property during QML
component completion. This tests the Qt lifecycle workaround, not Android JNI.
"""
import json
import os
import subprocess
import sys
import threading

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

if len(sys.argv) == 1:
    results = {}
    for mode in ("unprimed", "primed"):
        result = subprocess.run([sys.executable, __file__, mode], check=True, capture_output=True, text=True)
        results[mode] = json.loads(result.stdout)
    assert results["primed"]["warnings"] == [], results
    assert results["primed"]["roles_valid"]
    print("PASS: Qt activation reproducer: %d foreign-thread warnings before, none after GUI attached-property creation" % len(results["unprimed"]["warnings"]))
    sys.exit(0)

from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QAccessible, QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine, QQmlProperty
from PySide6.QtTest import QTest
import shiboken6

app = QGuiApplication([])
messages = []
qInstallMessageHandler(lambda kind, context, message: messages.append(message))
engine = QQmlEngine()
component = QQmlComponent(engine)
component.setData(b'''import QtQuick
import QtQuick.Controls
Window {width:360;height:640;visible:true
 TextInput {id:input;text:"Example"}
 Popup {id:popup}
 function targets(){return [input,popup.contentItem.parent]}
}''', QUrl("file:///accessibility.qml"))
assert not component.isError(), [error.toString() for error in component.errors()]
window = component.create()
assert window is not None
try:
    QTest.qWait(30)
    valid = True
    if sys.argv[1] == "primed":
        context = QQmlEngine.contextForObject(window)
        for target in window.targets().toVariant():
            prop = QQmlProperty(target, "Accessible.role", context)
            valid = valid and prop.isValid()
            prop.read()
    messages.clear()
    # Deliberately reproduce the platform's delayed activation thread boundary.
    worker = threading.Thread(target=lambda: QAccessible.setActive(True))
    worker.start()
    while worker.is_alive():
        QTest.qWait(10)
    worker.join()
    warnings = [message for message in messages if "Cannot create children" in message]
    print(json.dumps({"roles_valid": valid, "warnings": warnings}), flush=True)
finally:
    QAccessible.setActive(False)
    window.close()
    shiboken6.delete(window)
    shiboken6.delete(engine)
