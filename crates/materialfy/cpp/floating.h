#pragma once

#include <QtCore/QObject>
#include <QtCore/QString>

namespace mf {

// Плеер вне главного окна (настройка floatingPlayer): островок у верхнего края экрана и виджет в панели задач.
// window — QWindow* (Window из QML). Вне Windows — заглушки (false / 0 / ""), кроме масок и floating_cursor_in.

// Окно-островок: не в панели задач и Alt+Tab, не забирает фокус (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
// DWM не скругляет углы. macOS (floating_mac.mm): над строкой меню, на всех рабочих столах, размер выреза экрана —
// в свойства окна notchW / notchH. false — другая ОС или окна нет.
bool floating_tool_window(QObject* window);

// Где окно ловит мышь и рисуется: прямоугольник в логических пикселях окна; w <= 0 — всё окно.
// Остальная (прозрачная) часть окна пропускает клики к окнам под ним (Windows — регион окна, X11 — XShape).
// radius > 0 — со скруглёнными углами; часть фигуры выше y = 0 отрезает край окна (островок: верх прямой).
void floating_set_mask(QObject* window, int x, int y, int w, int h);
void floating_set_mask_rounded(QObject* window, int x, int y, int w, int h, int radius);

// На мониторе окна полноэкранное приложение (видео, игра, браузер по F11, презентация): плееру поверх него не место.
// Смотрит на окно на переднем плане именно этого монитора — игра на втором мониторе первый не трогает.
bool floating_fullscreen_on(QObject* window);

// На мониторе окна развёрнутое окно (кнопка «развернуть»): островок по настройке прячется и под ним. Windows —
// переднее окно или то, что видно в центре монитора; на KDE Wayland это делает скрипт KWin (kwin.rs), вне их — false.
bool floating_maximized_on(QObject* window);

// Курсор над прямоугольником (логические px окна; окно может быть и прозрачным для мыши): 0 — нет,
// 1 — да, 2 — да, но зажата кнопка мыши (тащат окно или выделяют — островок не вылезает).
int floating_cursor_in(QObject* window, int x, int y, int w, int h);

// Виджет в панели задач Windows: встроить окно дочерним в Shell_TrayWnd и поставить на место.
// width × height — размер в логических пикселях (масштаб окна — его devicePixelRatio, то есть DPI панели ×
// масштаб интерфейса; высота не больше панели без полей, а предел считается в пикселях панели),
// embed: false — сразу запасной вариант. pos — место (настройка taskbarWidgetPos): "tray" — у области уведомлений,
// "left" — у левого края (после «Пуска» и поиска, если значки слева), "appsLeft" / "appsRight" — слева / справа
// от значков приложений, или доля 0..1 свободной ширины панели ("0.42").
// Возвращает 1 — встроено, 2 — запасной вариант (окно над панелью, принадлежит ей), 0 — некуда (нет панели, она
// вертикальная, узкая или сейчас спрятана), -1 — окно потеряно (Explorer перезапустился и забрал дочернее окно
// с собой) — его надо создать заново. Вызывается раз в 2 с: заодно ловит смену размера и DPI панели.
int floating_taskbar_place(QObject* window, int width, int height, bool embed, const QString& pos);

// Перетаскивание виджета вдоль панели за курсором: phase 0 — начало (запоминает, где схватили), 1 — шаг,
// 2 — конец. В пределах 24 px окно липнет к местам «left», «appsLeft», «appsRight», «tray». Возвращает место
// в формате taskbarWidgetPos ("" — не вышло).
QString floating_taskbar_drag(QObject* window, int width, int height, int phase);

// Вынуть окно из панели задач (перед удалением окна)
void floating_taskbar_detach(QObject* window);

// Клавиши без фокуса: пока курсор над окном, ← → ↑ ↓ и пробел (без Ctrl, Alt, Win) не доходят до активного окна,
// а вызывают у window QML-функцию floatingKey(code): 1 ←, 2 →, 3 ↑, 4 ↓, 5 пробел. Windows — низкоуровневый
// хук клавиатуры (WH_KEYBOARD_LL) только на время наведения: on — поставить, false — снять. Курсор ушёл с окна
// или лежит без движения дольше 6 с (мышь бросили у края и печатают) — клавиши снова идут как обычно.
// Вне Windows — false (фокус не забираем).
bool floating_key_hook(QObject* window, bool on);

// Пропуск кликов через окно (WS_EX_TRANSPARENT): true — клики идут в окна под ним
void floating_set_click_through(QObject* window, bool enable);

// Запущен Windhawk со стилем панели задач (windows-11-taskbar-styler): XAML панели перекрывает дочерние окна,
// виджет тогда — окно над панелью
bool floating_windhawk_active();

} // namespace mf
