pragma Singleton
import QtQuick

// Open overlays handle Back in reverse order. Owners unregister when closed or destroyed.
QtObject {
    property var _items: []
    readonly property bool hasOverlay: _items.length > 0

    function push(item) {
        _items = _items.filter(entry => entry !== item).concat([item])
    }

    function pop(item) {
        _items = _items.filter(entry => entry !== item)
    }

    function back() {
        while (hasOverlay) {
            const item = _items[_items.length - 1]
            if (item) {
                if (typeof item.back === "function") {
                    if (item.back())
                        return true
                } else if (typeof item.close === "function") {
                    item.close()
                    return true
                }
            }
            pop(item)
        }
        return false
    }
}
