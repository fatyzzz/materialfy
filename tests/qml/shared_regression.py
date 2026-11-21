"""Run the shared controls and navigation regressions without a Spotify session."""

import os
import re
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QPoint, Qt, QUrl
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine
from PySide6.QtQuick import QQuickItem
from PySide6.QtTest import QTest


QML = Path(__file__).resolve().parents[2] / "src/materialfy/qml"
IMPORTS = """
import QtQuick
import Md3
import Materialfy.Core
import "Typography.js" as Typography
"""


def prepare_core(directory):
    core = directory / "Materialfy/Core"
    core.mkdir(parents=True)
    theme = (QML / "Md3/Theme.qml").read_text(encoding="utf-8")
    roles = dict.fromkeys(re.findall(r': "(\w+)"', theme))
    colors = ",".join(name + ': Qt.rgba(.2, .2, .2, 1)' for name in roles)
    singletons = {
        "ThemeEngine": f"property bool dark: false; property var colors: ({{{colors}}}); signal schemeChanged()",
        "Settings": 'property string animations: "reduced"; property string lowPowerMode: "auto"',
        "Translations": 'property string language: "en"; property var ru: ({}); property var pt: ({})',
        "Auth": 'property string state: "signedIn"',
        "IconMetrics": 'function glyph(name) { return "" } function ink(name, size) { return Qt.rect(0, 0, size, size) }',
        "Catalog": """
            property string searchQuery: "Last catalog search"
            property var calls: []
            function search(query, type) { calls = calls.concat([[query, type]]) }
        """,
    }
    for name, properties in singletons.items():
        (core / f"{name}.qml").write_text(
            "pragma Singleton\nimport QtQuick\nQtObject {\n" + properties + "\n}", encoding="utf-8"
        )
    (core / "qmldir").write_text(
        "module Materialfy.Core\n" + "".join(f"singleton {name} 1.0 {name}.qml\n" for name in singletons),
        encoding="utf-8",
    )


class SharedQmlTests(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        root = Path(self.directory.name)
        prepare_core(root)
        self.engine = QQmlEngine()
        self.warnings = []
        self.engine.warnings.connect(lambda errors: self.warnings.extend(error.toString() for error in errors))
        self.engine.addImportPath(str(root))
        self.engine.addImportPath(str(QML))

    def create(self, source, initial=None):
        self.component = QQmlComponent(self.engine)
        self.component.setData((IMPORTS + source).encode(), QUrl.fromLocalFile(str(QML / "Md3/test.qml")))
        obj = self.component.createWithInitialProperties(initial) if initial else self.component.create()
        self.assertIsNotNone(obj, self.component.errorString())
        self.obj = obj
        return obj

    def tearDown(self):
        self.assertEqual(self.warnings, [])

    def test_controls_use_same_activation_path_for_mouse_and_accessibility(self):
        window = self.create("""
            Window {
                width: 300; height: 160; visible: true
                property int activations: 0
                Button {
                    id: button
                    objectName: "testButton"
                    x: 20; y: 20; width: 130; height: 40
                    text: "Toggle"; checkable: true
                    onClicked: activations++
                }
                IconButton {
                    id: icon
                    checkable: true; autoToggle: false
                    onClicked: activations++
                }
                function pressIcon() {
                    icon.Accessible.pressAction()
                    return icon.checked
                }
                function pressButton() { button.Accessible.pressAction() }
            }
        """)
        button = window.findChild(QQuickItem, "testButton")
        QTest.qWait(30)
        self.assertEqual(button.property("implicitHeight"), 40)
        self.assertFalse(window.pressIcon())
        self.assertEqual(window.property("activations"), 1)
        window.pressButton()
        self.assertTrue(button.property("checked"))
        self.assertEqual(window.property("activations"), 2)
        QTest.mouseClick(window, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, QPoint(90, 40))
        self.assertFalse(button.property("checked"))
        self.assertEqual(window.property("activations"), 3)
        button.setProperty("enabled", False)
        window.pressButton()
        self.assertEqual(window.property("activations"), 3)
        window.close()

    def test_overlay_stack_and_typography(self):
        obj = self.create("""
            QtObject {
                id: root
                property string order: ""
                property QtObject first: QtObject {
                    id: firstOverlay
                    function close() { root.order += "1"; BackStack.pop(firstOverlay) }
                }
                property QtObject second: QtObject {
                    id: secondOverlay
                    function close() { root.order += "2"; BackStack.pop(secondOverlay) }
                }
                property QtObject skipped: QtObject { function back() { return false } }
                function closeOverlays() {
                    BackStack.push(first)
                    BackStack.push(second)
                    BackStack.push(second)
                    BackStack.push(skipped)
                    BackStack.back()
                    BackStack.back()
                    return !BackStack.hasOverlay && !BackStack.back()
                }
                function typeMetrics() {
                    const desktop = Theme.type.bodyLarge
                    const scaled = Typography.scaled(Theme.type, size => size * 2).bodyLarge
                    return [desktop.size, desktop.line, desktop.weight, desktop.tracking,
                            scaled.size, scaled.line, scaled.weight, scaled.tracking]
                }
            }
        """)
        self.assertTrue(obj.closeOverlays())
        self.assertEqual(obj.property("order"), "21")
        self.assertEqual(obj.typeMetrics().toVariant(), [16, 24, 400, 0.5, 32, 48, 400, 1])

    def test_search_route_uses_explicit_query_without_submitting_again(self):
        # Load the page's actual input and debounce logic without the result delegates.
        source = (QML / "app/pages/SearchPage.qml").read_text(encoding="utf-8")
        props = source[source.index("    property string initialQuery:"):source.index("    readonly property bool all:")]
        logic = source[source.index("    function focusSearch()"):source.index("    Flow {")]
        obj = self.create("""
            import QtQuick.Layouts
            Item {
                id: page
                width: 600
                property alias text: field.text
                function searchCount() { return Catalog.calls.length }
        """ + props + logic + "}", {"initialQuery": "Deep link"})
        self.assertIsInstance(obj, QQuickItem)
        self.assertEqual(obj.property("text"), "Deep link")
        obj.setProperty("filter", 1)
        self.assertEqual(obj.searchCount(), 1)
        obj.showQuery("Deep link")
        self.assertEqual(obj.property("filter"), 0)
        QTest.qWait(400)
        self.assertEqual(obj.searchCount(), 1)
        obj.setProperty("text", "Typed query")
        QTest.qWait(400)
        self.assertEqual(obj.searchCount(), 2)


if __name__ == "__main__":
    app = QGuiApplication(sys.argv[:1])
    unittest.main()
