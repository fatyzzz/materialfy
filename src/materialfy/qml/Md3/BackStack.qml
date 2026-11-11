pragma Singleton
import QtQuick

// «Назад» системы (телефон): открытые меню, листы и диалоги закрываются раньше страниц, последний открытый —
// первым. Окно само встаёт сюда при открытии (push) и уходит при закрытии (pop); у окна back() (вернуть true,
// если закрыло что-то) или close().
QtObject {
    property var _items: []
    // Модальные листы остаются здесь до конца исчезновения: жест не должен перейти нижнему слою.
    readonly property bool hasOverlay: _items.length > 0

    function push(item) {
        _items = _items.filter(i => i !== item).concat([item])
    }
    function pop(item) {
        _items = _items.filter(i => i !== item)
    }
    // false — закрывать нечего
    function back() {
        while (_items.length > 0) {
            const top = _items[_items.length - 1]
            if (typeof top.back === "function") {
                if (top.back())
                    return true
            } else if (typeof top.close === "function") {
                top.close()
                return true
            }
            pop(top)
        }
        return false
    }
}
