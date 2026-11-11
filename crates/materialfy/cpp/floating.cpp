// Плеер вне главного окна: островок у верхнего края экрана (FloatingIsland.qml) и виджет в панели задач
// Windows (TaskbarPlayer.qml). Окна создаёт QML, здесь — то, чего из QML не сделать: стиль окна Win32,
// маска ввода, встраивание в Shell_TrayWnd, клавиши без фокуса, курсор вне своего окна.
//
// Панель задач: окно становится дочерним Shell_TrayWnd (через QWindow::fromWinId + setParent — Qt сам
// ставит WS_CHILD вместо WS_POPUP и знает, что окно дочернее: координаты мыши и mapToGlobal верны). Место —
// у области уведомлений (TrayNotifyWnd), у левого края или у значков приложений: их границы — из UI Automation
// (кнопки Windows 11 нарисованы XAML, своих окон у них нет). Окно другого процесса как родитель — законно, но
// Explorer при перезапуске уносит дочерние окна с собой: тогда floating_taskbar_place отвечает -1, и QML создаёт
// окно заново. Не встроилось (Windhawk перекрашивает панель, второй монитор, другая оболочка) — окно над панелью,
// которым владеет сама панель: Windows держит его над ней и опускает вместе с ней под полноэкранное приложение.
#include "floating.h"

#include <QtCore/QEvent>
#include <QtGui/QCursor>
#include <QtGui/QGuiApplication>
#include <QtGui/QPainterPath>
#include <QtGui/QRegion>
#include <QtGui/QScreen>
#include <QtGui/QWindow>

namespace mf {

namespace {

// Маска окна в логических пикселях: часть фигуры выше края окна отрезана (у островка верх прямой)
QRegion
mask_region(int x, int y, int w, int h, int radius)
{
  if (radius <= 0)
    return QRegion(x, y, w, h);
  QPainterPath path;
  path.addRoundedRect(x, y, w, h, radius, radius);
  return QRegion(path.toFillPolygon().toPolygon());
}

} // namespace

void
floating_set_mask(QObject* object, int x, int y, int w, int h)
{
  floating_set_mask_rounded(object, x, y, w, h, 0);
}

} // namespace mf

#if defined(_WIN32)

#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <objbase.h>
#include <UIAutomation.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <tlhelp32.h>
#include <wrl/client.h>

#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtCore/QVariant>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <cwchar>
#include <string>
#include <utility>

using Microsoft::WRL::ComPtr;

namespace mf {

namespace {

// HWND окна — только у платформы windows: у offscreen (снимки) winId — просто счётчик, не дескриптор
HWND
native(QWindow* window)
{
  if (!window || QGuiApplication::platformName() != QLatin1String("windows"))
    return nullptr;
  return reinterpret_cast<HWND>(window->winId());
}

// Отпустить родителя: сначала окно — сиротой, потом удаляем родительскую обёртку QWindow. Регион сбрасывается
// только при смене родителя: иначе раз в 2 с окно мелькало бы прямоугольником до новой маски
void
release_host(QWindow* window)
{
  if (!window)
    return;
  QObject* p = window->parent();
  if (!p)
    return;
  if (HWND hwnd = native(window))
    SetWindowRgn(hwnd, nullptr, TRUE);
  window->setParent(nullptr);
  delete p;
}

int
px(double logical, double scale)
{
  return static_cast<int>(std::lround(logical * scale));
}

bool
mouse_held()
{
  const auto down = [](int vk) { return (GetAsyncKeyState(vk) & 0x8000) != 0; };
  return down(VK_LBUTTON) || down(VK_RBUTTON) || down(VK_MBUTTON);
}

// ---- Мониторы ----

// Экран окна: screenIndex из QML (у виджета — номер монитора из настройки), иначе экран самого окна
QScreen*
target_screen(QWindow* window)
{
  const auto screens = QGuiApplication::screens();
  if (window) {
    bool ok = false;
    const int idx = window->property("screenIndex").toInt(&ok);
    if (ok && idx >= 0 && idx < screens.size())
      return screens.at(idx);
    if (window->screen())
      return window->screen();
  }
  return QGuiApplication::primaryScreen();
}

struct MonitorByName
{
  std::wstring name;
  HMONITOR found = nullptr;
};

BOOL CALLBACK
monitor_by_name(HMONITOR monitor, HDC, LPRECT, LPARAM param)
{
  auto* find = reinterpret_cast<MonitorByName*>(param);
  MONITORINFOEXW info{};
  info.cbSize = sizeof(info);
  if (GetMonitorInfoW(monitor, &info) && find->name == info.szDevice) {
    find->found = monitor;
    return FALSE;
  }
  return TRUE;
}

// Монитор Windows для экрана Qt: по имени устройства («\\.\DISPLAY2»), иначе по центру экрана. Qt хранит начало
// экрана в физических пикселях, а размер — в логических: центр — начало + половина размера × масштаб
HMONITOR
monitor_for(QScreen* screen)
{
  if (!screen)
    return MonitorFromPoint(POINT{ 0, 0 }, MONITOR_DEFAULTTOPRIMARY);
  MonitorByName find{ screen->name().toStdWString() };
  EnumDisplayMonitors(nullptr, nullptr, monitor_by_name, reinterpret_cast<LPARAM>(&find));
  if (find.found)
    return find.found;
  const QRect g = screen->geometry();
  const double dpr = screen->devicePixelRatio();
  const POINT center{ g.x() + px(g.width(), dpr) / 2, g.y() + px(g.height(), dpr) / 2 };
  return MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
}

bool
is_shell_window(HWND hwnd)
{
  wchar_t cls[64] = {};
  GetClassNameW(hwnd, cls, 64);
  return std::wcscmp(cls, L"Progman") == 0 || std::wcscmp(cls, L"WorkerW") == 0
         || std::wcscmp(cls, L"Shell_TrayWnd") == 0 || std::wcscmp(cls, L"Shell_SecondaryTrayWnd") == 0;
}

// Окно закрывает монитор целиком (не развёрнуто — то закрывает рабочую область без панели)
bool
covers_monitor(HWND hwnd, HMONITOR monitor)
{
  hwnd = hwnd ? GetAncestor(hwnd, GA_ROOT) : nullptr;
  if (!hwnd || !IsWindowVisible(hwnd) || IsIconic(hwnd) || is_shell_window(hwnd))
    return false;
  if (MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) != monitor)
    return false;
  MONITORINFO info{};
  info.cbSize = sizeof(info);
  if (!GetMonitorInfoW(monitor, &info))
    return false;
  RECT r{};
  if (FAILED(DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &r, sizeof(r))))
    GetWindowRect(hwnd, &r);
  const RECT& m = info.rcMonitor;
  return r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
}

// Развёрнутое окно на этом мониторе (кнопка «развернуть»: занимает рабочую область, панель задач остаётся)
bool
maximized_on_monitor(HWND hwnd, HMONITOR monitor)
{
  hwnd = hwnd ? GetAncestor(hwnd, GA_ROOT) : nullptr;
  if (!hwnd || !IsWindowVisible(hwnd) || IsIconic(hwnd) || is_shell_window(hwnd) || !IsZoomed(hwnd))
    return false;
  return MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) == monitor;
}

// ---- Панель задач и места на ней (физические пиксели экрана) ----

struct Bar
{
  HWND tray = nullptr;
  RECT rect{};
  double scale = 1;
  LONG notify_left = 0; // левый край области уведомлений
};

// true — есть горизонтальная панель на мониторе окна
bool
read_bar(QWindow* window, Bar& bar)
{
  static const std::wstring tray_class =
    qEnvironmentVariable("MATERIALFY_TASKBAR_CLASS", QStringLiteral("Shell_TrayWnd")).toStdWString();

  const HMONITOR monitor = monitor_for(target_screen(window));
  HWND tray = FindWindowW(tray_class.c_str(), nullptr);
  const bool primary = tray && MonitorFromWindow(tray, MONITOR_DEFAULTTONEAREST) == monitor;
  if (!primary) {
    // панель второго монитора («Показывать панель задач на всех дисплеях»); её нет — виджету некуда
    tray = nullptr;
    HWND sec = nullptr;
    while ((sec = FindWindowExW(nullptr, sec, L"Shell_SecondaryTrayWnd", nullptr)) != nullptr) {
      if (MonitorFromWindow(sec, MONITOR_DEFAULTTONEAREST) == monitor) {
        tray = sec;
        break;
      }
    }
  }
  if (!tray || !GetWindowRect(tray, &bar.rect))
    return false;
  if (bar.rect.bottom - bar.rect.top > bar.rect.right - bar.rect.left)
    return false; // вертикальная панель (Windows 10, сторонние оболочки)
  bar.tray = tray;
  const UINT dpi = GetDpiForWindow(tray);
  bar.scale = (dpi ? dpi : 96) / 96.0;
  // у второй панели области уведомлений нет — только часы
  bar.notify_left = bar.rect.right - px(primary ? 200 : 60, bar.scale);
  RECT notify{};
  HWND notify_wnd = primary ? FindWindowExW(tray, nullptr, L"TrayNotifyWnd", nullptr) : nullptr;
  if (notify_wnd && GetWindowRect(notify_wnd, &notify) && notify.right > notify.left && notify.left > bar.rect.left)
    bar.notify_left = notify.left;
  return true;
}

// Панель видна: не спрятана (автоскрытие уводит её за край экрана, оставляя пару пикселей)
bool
bar_shown(const Bar& bar)
{
  if (!bar.tray || !IsWindowVisible(bar.tray))
    return false;
  MONITORINFO info{};
  info.cbSize = sizeof(info);
  if (!GetMonitorInfoW(MonitorFromWindow(bar.tray, MONITOR_DEFAULTTONEAREST), &info))
    return true;
  RECT shown{};
  IntersectRect(&shown, &bar.rect, &info.rcMonitor);
  return (shown.bottom - shown.top) * 4 >= (bar.rect.bottom - bar.rect.top) * 3;
}

// Кнопки панели через UI Automation: «Пуск», поиск, представление задач и значки приложений — одна группа
// (по центру или слева), «Виджеты» (погода) — слева от неё. false — UIA нет или кнопок не нашлось.
// Обход дерева Explorer — межпроцессный: свойства берутся одним запросом (кэш UIA), результат живёт 3 с
// (виджет переставляется раз в 2 с, а значки сдвигаются, только когда открывают и закрывают программы)
struct Buttons
{
  HWND tray = nullptr;
  RECT bar{};
  ULONGLONG tick = 0;
  bool found = false;
  RECT apps{};
  RECT widgets{};
} buttons_cache;

bool
taskbar_buttons(HWND tray, const RECT& bar_rect, RECT& apps, RECT& widgets)
{
  const ULONGLONG now = GetTickCount64();
  Buttons& c = buttons_cache;
  if (c.tray == tray && EqualRect(&c.bar, &bar_rect) && now - c.tick < 3000) {
    if (c.found) {
      apps = c.apps;
      widgets = c.widgets;
    }
    return c.found;
  }
  c = Buttons{ tray, bar_rect, now, false, {}, {} };

  static ComPtr<IUIAutomation> uia;
  if (!uia) {
    // Qt уже инициализировал COM в GUI-потоке (OLE для перетаскивания); второй вызов безвреден
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(CoCreateInstance(__uuidof(CUIAutomation), nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&uia))))
      return false;
  }
  ComPtr<IUIAutomationElement> root;
  if (FAILED(uia->ElementFromHandle(tray, &root)) || !root)
    return false;
  VARIANT button{};
  button.vt = VT_I4;
  button.lVal = UIA_ButtonControlTypeId;
  ComPtr<IUIAutomationCondition> condition;
  ComPtr<IUIAutomationCacheRequest> request;
  if (FAILED(uia->CreatePropertyCondition(UIA_ControlTypePropertyId, button, &condition))
      || FAILED(uia->CreateCacheRequest(&request)))
    return false;
  request->AddProperty(UIA_AutomationIdPropertyId);
  request->AddProperty(UIA_ClassNamePropertyId);
  request->AddProperty(UIA_BoundingRectanglePropertyId);
  ComPtr<IUIAutomationElementArray> found;
  if (FAILED(root->FindAllBuildCache(TreeScope_Descendants, condition.Get(), request.Get(), &found)) || !found)
    return false;
  int count = 0;
  found->get_Length(&count);
  bool any = false;
  RECT group{};
  RECT weather{};
  for (int i = 0; i < count; ++i) {
    ComPtr<IUIAutomationElement> element;
    if (FAILED(found->GetElement(i, &element)) || !element)
      continue;
    BSTR id = nullptr;
    BSTR cls = nullptr;
    element->get_CachedAutomationId(&id);
    element->get_CachedClassName(&cls);
    const std::wstring automation_id = id ? id : L"";
    const std::wstring class_name = cls ? cls : L"";
    SysFreeString(id);
    SysFreeString(cls);
    RECT r{};
    if (FAILED(element->get_CachedBoundingRectangle(&r)) || r.right <= r.left)
      continue;
    if (automation_id == L"WidgetsButton") {
      weather = r;
    } else if (automation_id == L"StartButton" || automation_id == L"SearchButton" || automation_id == L"TaskViewButton"
               || class_name == L"Taskbar.TaskListButtonAutomationPeer" || class_name.find(L"SearchBox") != std::wstring::npos) {
      if (!any)
        group = r;
      group.left = std::min(group.left, r.left);
      group.right = std::max(group.right, r.right);
      any = true;
    }
  }
  c.found = any;
  c.apps = group;
  c.widgets = weather;
  if (any) {
    apps = group;
    widgets = weather;
  }
  return any;
}

// Места для левого края окна шириной w: «left», «appsLeft», «appsRight», «tray» и свободный отрезок [lo, hi]
struct Places
{
  LONG lo = 0;
  LONG hi = 0;
  std::array<std::pair<const char*, LONG>, 4> anchors{};
};

Places
places(const Bar& bar, int w, bool with_buttons)
{
  const LONG gap = px(8, bar.scale);
  Places p;
  p.lo = bar.rect.left + gap;
  p.hi = bar.notify_left - gap - w;
  const LONG center = (bar.rect.left + bar.rect.right) / 2;
  RECT apps{ center - px(240, bar.scale), 0, center + px(240, bar.scale), 0 }; // без UIA — примерно
  RECT widgets{};
  const bool found = with_buttons && taskbar_buttons(bar.tray, bar.rect, apps, widgets);
  if (found && widgets.right > widgets.left && widgets.right < apps.left)
    p.lo = std::max(p.lo, widgets.right + gap);
  LONG left = p.lo;
  // значки слева (не по центру): «после Пуска и поиска» — сразу за группой
  if (found && apps.left - p.lo < w + gap)
    left = apps.right + gap;
  auto clamp = [&](LONG x) { return std::max(p.lo, std::min(x, p.hi)); };
  p.anchors = { { { "left", clamp(left) },
                  { "appsLeft", clamp(apps.left - gap - w) },
                  { "appsRight", clamp(apps.right + gap) },
                  { "tray", p.hi } } };
  return p;
}

LONG
position_x(const Places& p, const QString& pos)
{
  for (const auto& [name, x] : p.anchors)
    if (pos == QLatin1String(name))
      return x;
  bool ok = false;
  const double f = pos.toDouble(&ok);
  if (!ok || !std::isfinite(f))
    return p.hi;
  return p.lo + static_cast<LONG>(std::lround(std::clamp(f, 0.0, 1.0) * (p.hi - p.lo)));
}

// Окно — встроенным (координаты панели) или над панелью (координаты экрана) на месте x, по центру панели
int
move_window(QWindow* window, HWND hwnd, const Bar& bar, bool embed, LONG x, int w, int h)
{
  const LONG y = (bar.rect.top + bar.rect.bottom - h) / 2;
  const int r = h / 2;
  if (embed) {
    QWindow* cur_parent = window ? qobject_cast<QWindow*>(window->parent()) : nullptr;
    if (!cur_parent || native(cur_parent) != bar.tray) {
      release_host(window);
      QWindow* next = QWindow::fromWinId(reinterpret_cast<WId>(bar.tray));
      if (next && window)
        window->setParent(next);
    }
  }
  if (embed && GetParent(hwnd) == bar.tray) {
    POINT origin{ x, y };
    ScreenToClient(bar.tray, &origin);
    // HWND_TOP — над содержимым панели (XAML-мостом Windows 11), иначе окно под ним не видно
    SetWindowPos(hwnd, HWND_TOP, origin.x, origin.y, w, h, SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, 2 * r, 2 * r), TRUE);
    return 1;
  }
  // Над панелью: окном владеет сама панель. Владеемое окно Windows всегда держит над владельцем (клик по панели
  // его не прячет) и вместе с ним снимает «поверх всех», когда впереди полноэкранное приложение. Qt при каждом
  // показе окна сбрасывает владельца — ставим заново при каждом вызове
  release_host(window);
  if (reinterpret_cast<HWND>(GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT)) != bar.tray)
    SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, reinterpret_cast<LONG_PTR>(bar.tray));
  SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_NOOWNERZORDER);
  SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, 2 * r, 2 * r), TRUE);
  return 2;
}

// Масштаб окна на панели: DPI самой панели × масштаб интерфейса. Не devicePixelRatio окна — до переезда
// на панель он берётся с экрана, где окно было, и на мониторах с разным DPI первое размещение ошибается.
double
window_scale(const Bar& bar)
{
  const double user = qEnvironmentVariable("QT_SCALE_FACTOR", QStringLiteral("1")).toDouble();
  return bar.scale * (user > 0 ? user : 1.0);
}

int
size_h(const Bar& bar, int height, double scale)
{
  // Высота окна — с масштабом интерфейса (window_scale), предел по высоте панели — в пикселях панели (bar.scale).
  return std::min(px(height, scale), static_cast<int>(bar.rect.bottom - bar.rect.top) - 2 * px(4, bar.scale));
}

// Перетаскивание: где схватили (от левого края окна) и места на панели — на всё время перетаскивания
struct Drag
{
  LONG grab = 0;
  Places places;
  bool embed = true;
} drag;

// ---- Клавиши без фокуса ----

HHOOK key_hook = nullptr;
QPointer<QWindow> key_window;
HWND key_hwnd = nullptr;
std::array<bool, 6> key_down{}; // нажатия, которые мы забрали: их отпускание тоже не отдаём
POINT key_cursor{};
ULONGLONG key_active = 0; // когда мышь последний раз двигалась над окном (или нажали стрелку)
constexpr ULONGLONG key_idle_ms = 6000;

int
key_code(DWORD vk)
{
  switch (vk) {
    case VK_LEFT: return 1;
    case VK_RIGHT: return 2;
    case VK_UP: return 3;
    case VK_DOWN: return 4;
    case VK_SPACE: return 5;
    default: return 0;
  }
}

bool
cursor_over_key_window()
{
  POINT p{};
  if (!key_hwnd || !GetCursorPos(&p))
    return false;
  if (p.x != key_cursor.x || p.y != key_cursor.y) {
    key_cursor = p;
    key_active = GetTickCount64();
  }
  // WindowFromPoint учитывает регион окна: прозрачная часть островка — не наша
  HWND under = WindowFromPoint(p);
  return under == key_hwnd || IsChild(key_hwnd, under);
}

LRESULT CALLBACK
key_proc(int code, WPARAM wparam, LPARAM lparam)
{
  if (code == HC_ACTION) {
    const auto* key = reinterpret_cast<const KBDLLHOOKSTRUCT*>(lparam);
    const int k = key_code(key->vkCode);
    const bool down = wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN;
    if (k && !down && key_down[k]) {
      key_down[k] = false;
      return 1;
    }
    const auto held = [](int vk) { return (GetAsyncKeyState(vk) & 0x8000) != 0; };
    // Мышь давно не двигалась — её бросили над островком и печатают в другом окне: пробел и стрелки — им
    if (k && down && key_window && !(key->flags & LLKHF_INJECTED) && !held(VK_CONTROL) && !held(VK_MENU)
        && !held(VK_LWIN) && !held(VK_RWIN) && cursor_over_key_window()
        && GetTickCount64() - key_active < key_idle_ms) {
      // Автоповтор: громкость идёт шагами дальше, трек и пауза — один раз на нажатие
      const bool repeat = key_down[k];
      key_down[k] = true;
      if (k != 5)
        key_active = GetTickCount64(); // стрелками пользуются осознанно; пробел срок не продлевает
      if (!repeat || k == 3 || k == 4)
        QMetaObject::invokeMethod(key_window.data(), "floatingKey", Qt::QueuedConnection, Q_ARG(QVariant, QVariant(k)));
      return 1;
    }
  }
  return CallNextHookEx(nullptr, code, wparam, lparam);
}

bool
process_running(const wchar_t* exe)
{
  HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snap == INVALID_HANDLE_VALUE)
    return false;
  PROCESSENTRY32W entry{};
  entry.dwSize = sizeof(entry);
  bool found = false;
  for (BOOL ok = Process32FirstW(snap, &entry); ok && !found; ok = Process32NextW(snap, &entry))
    found = _wcsicmp(entry.szExeFile, exe) == 0;
  CloseHandle(snap);
  return found;
}

} // namespace

bool
floating_tool_window(QObject* object)
{
  HWND hwnd = native(qobject_cast<QWindow*>(object));
  if (!hwnd)
    return false;
  // Qt ставит то же по флагам Tool и WindowDoesNotAcceptFocus; здесь — наверняка: без WS_EX_APPWINDOW окна нет
  // ни в панели задач, ни в Alt+Tab, с WS_EX_NOACTIVATE клик по нему не уводит фокус у окна, где печатают
  const LONG_PTR ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
  const LONG_PTR want = (ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) & ~LONG_PTR(WS_EX_APPWINDOW);
  if (want != ex) {
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
    SetWindowPos(hwnd, nullptr, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
  }
  // Форму рисует QML; Windows 11 не скругляет окно сам и не прячет его при Aero Peek
  constexpr DWORD corner_preference = 33; // DWMWA_WINDOW_CORNER_PREFERENCE
  const int corners = 1;                  // DWMWCP_DONOTROUND
  DwmSetWindowAttribute(hwnd, corner_preference, &corners, sizeof(corners));
  const BOOL exclude = TRUE;
  DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &exclude, sizeof(exclude));
  return true;
}

bool
floating_fullscreen_on(QObject* object)
{
  const HMONITOR monitor = monitor_for(target_screen(qobject_cast<QWindow*>(object)));
  // Игра в исключительном полноэкранном режиме и режим презентации: Windows говорит о них без монитора, а
  // соседние мониторы и их панели задач видны как обычно (CS 4:3 на основном — виджет на втором остаётся).
  // Такое окно всегда на переднем плане — прячемся, только если оно на нашем мониторе
  QUERY_USER_NOTIFICATION_STATE state{};
  if (SUCCEEDED(SHQueryUserNotificationState(&state))
      && (state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE)) {
    const HWND front = GetForegroundWindow();
    if (front && MonitorFromWindow(front, MONITOR_DEFAULTTONEAREST) == monitor)
      return true;
  }
  // Остальное (видео и браузер на весь экран, игры в окне без рамки) — только на своём мониторе: окно на переднем
  // плане или то, что видно в его центре (переднее окно может быть и на другом мониторе)
  if (covers_monitor(GetForegroundWindow(), monitor))
    return true;
  MONITORINFO info{};
  info.cbSize = sizeof(info);
  if (!GetMonitorInfoW(monitor, &info))
    return false;
  const POINT center{ (info.rcMonitor.left + info.rcMonitor.right) / 2, (info.rcMonitor.top + info.rcMonitor.bottom) / 2 };
  return covers_monitor(WindowFromPoint(center), monitor);
}

bool
floating_maximized_on(QObject* object)
{
  const HMONITOR monitor = monitor_for(target_screen(qobject_cast<QWindow*>(object)));
  if (maximized_on_monitor(GetForegroundWindow(), monitor))
    return true;
  // переднее окно может быть на другом мониторе — смотрим, что видно в центре нашего
  MONITORINFO info{};
  info.cbSize = sizeof(info);
  if (!GetMonitorInfoW(monitor, &info))
    return false;
  const POINT center{ (info.rcMonitor.left + info.rcMonitor.right) / 2, (info.rcMonitor.top + info.rcMonitor.bottom) / 2 };
  return maximized_on_monitor(WindowFromPoint(center), monitor);
}

int
floating_cursor_in(QObject* object, int x, int y, int w, int h)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || w <= 0 || h <= 0)
    return 0;
  const QPoint p = window->mapFromGlobal(QCursor::pos(window->screen()));
  if (!QRect(x, y, w, h).contains(p))
    return 0;
  return mouse_held() ? 2 : 1;
}

bool
floating_windhawk_active()
{
  // Мод мог остаться на диске после удаления Windhawk: важно, что сам Windhawk запущен. Раз в 30 с — его могут
  // включить и выключить, не перезапуская нас
  static ULONGLONG checked = 0;
  static bool active = false;
  const ULONGLONG now = GetTickCount64();
  if (checked == 0 || now - checked > 30000) {
    checked = now;
    const DWORD attr = GetFileAttributesW(L"C:\\ProgramData\\Windhawk\\Engine\\ModsWritable\\mod-storage\\windows-11-taskbar-styler");
    active = attr != INVALID_FILE_ATTRIBUTES && process_running(L"windhawk.exe");
  }
  return active;
}

void
floating_set_mask_rounded(QObject* object, int x, int y, int w, int h, int radius)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window)
    return;
  HWND hwnd = native(window);

  if (w <= 0 || h <= 0) {
    window->setMask(QRegion());
    if (hwnd)
      SetWindowRgn(hwnd, nullptr, TRUE);
    return;
  }

  window->setMask(mask_region(x, y, w, h, radius));
  if (hwnd) {
    // Свой регион в физических пикселях: у QRegion из многоугольника лесенка на скруглениях при масштабе 125 %+
    const double scale = window->devicePixelRatio();
    const int rx1 = px(x, scale);
    const int ry1 = px(y, scale);
    const int rx2 = rx1 + px(w, scale);
    const int ry2 = ry1 + px(h, scale);
    const int r = px(radius, scale);
    SetWindowRgn(hwnd, r > 0 ? CreateRoundRectRgn(rx1, ry1, rx2 + 1, ry2 + 1, 2 * r, 2 * r) : CreateRectRgn(rx1, ry1, rx2, ry2), TRUE);
  }
}

void
floating_set_click_through(QObject* object, bool enable)
{
  // Флаг Qt: он же ставит WS_EX_LAYERED с непрозрачностью (без неё слоёное окно невидимо) и WS_EX_TRANSPARENT.
  // Стили Qt пересчитывает из флагов — свои (WS_EX_NOACTIVATE и др.) ставим заново
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || window->flags().testFlag(Qt::WindowTransparentForInput) == enable)
    return;
  window->setFlag(Qt::WindowTransparentForInput, enable);
  floating_tool_window(window);
}

int
floating_taskbar_place(QObject* object, int width, int height, bool embed, const QString& pos)
{
  auto* window = qobject_cast<QWindow*>(object);
  HWND hwnd = native(window);
  if (!hwnd)
    return 0;
  if (!IsWindow(hwnd)) {
    // HWND ушёл вместе со старой панелью: окно — сиротой, чтобы удаление обёртки его не задело
    release_host(window);
    return -1;
  }
  Bar bar;
  if (!read_bar(window, bar)) {
    release_host(window);
    return 0;
  }
  const double scale = window_scale(bar);
  const int w = px(width, scale);
  const int h = size_h(bar, height, scale);
  // кнопки панели нужны только местам у значков и у левого края — у трея хватает TrayNotifyWnd
  const Places p = places(bar, w, pos != QLatin1String("tray"));
  if (h <= 0 || p.hi < p.lo) {
    // узкий экран: виджет залез бы на «Пуск» и значки
    release_host(window);
    return 0;
  }
  // Windhawk перекрашивает панель XAML-слоями поверх дочерних окон; у панели второго монитора то же —
  // там виджет над панелью
  const bool primary = bar.tray == FindWindowW(L"Shell_TrayWnd", nullptr);
  const bool use_embed = embed && primary && !floating_windhawk_active();
  if (!use_embed && !bar_shown(bar)) {
    // панель спрятана (автоскрытие): висеть над пустым местом незачем
    release_host(window);
    return 0;
  }
  return move_window(window, hwnd, bar, use_embed, position_x(p, pos), w, h);
}

QString
floating_taskbar_drag(QObject* object, int width, int height, int phase)
{
  auto* window = qobject_cast<QWindow*>(object);
  HWND hwnd = native(window);
  Bar bar;
  POINT cursor{};
  RECT now{};
  if (!hwnd || !IsWindow(hwnd) || !read_bar(window, bar) || !GetCursorPos(&cursor) || !GetWindowRect(hwnd, &now))
    return {};
  const double scale = window_scale(bar);
  const int w = px(width, scale);
  if (phase == 0) {
    drag.grab = cursor.x - now.left;
    drag.places = places(bar, w, true);
    drag.embed = GetParent(hwnd) == bar.tray;
  }
  const Places& p = drag.places;
  if (p.hi < p.lo)
    return {};
  LONG x = std::max(p.lo, std::min(cursor.x - drag.grab, p.hi));
  QString pos;
  const LONG snap = px(24, bar.scale);
  LONG best = snap + 1;
  for (const auto& [name, ax] : p.anchors) {
    if (std::abs(x - ax) <= snap && std::abs(x - ax) < best) {
      best = std::abs(x - ax);
      pos = QString::fromLatin1(name);
    }
  }
  if (!pos.isEmpty())
    x = position_x(p, pos);
  else
    pos = QString::number(p.hi > p.lo ? double(x - p.lo) / double(p.hi - p.lo) : 1.0, 'f', 4);
  move_window(window, hwnd, bar, drag.embed, x, w, size_h(bar, height, scale));
  return pos;
}

void
floating_taskbar_detach(QObject* object)
{
  release_host(qobject_cast<QWindow*>(object));
}

bool
floating_key_hook(QObject* object, bool on)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!on) {
    // снимает только своё окно: островок и виджет могли смениться местами
    if (key_hook && (!window || window == key_window.data() || !key_window)) {
      UnhookWindowsHookEx(key_hook);
      key_hook = nullptr;
      key_window = nullptr;
      key_hwnd = nullptr;
      key_down = {};
    }
    return true;
  }
  HWND hwnd = native(window);
  if (!hwnd)
    return false;
  key_window = window;
  key_hwnd = hwnd;
  GetCursorPos(&key_cursor);
  key_active = GetTickCount64();
  if (!key_hook)
    key_hook = SetWindowsHookExW(WH_KEYBOARD_LL, key_proc, GetModuleHandleW(nullptr), 0);
  return key_hook != nullptr;
}

} // namespace mf

#else

namespace mf {

#if !defined(__APPLE__) // macOS — floating_mac.mm
namespace {
// Wayland не отдаёт глобальный курсор: после ухода мыши QCursor::pos() остаётся на последней точке внутри окна.
// Вход и выход указателя ловим сами и кладём в свойство окна, его читает floating_cursor_in.
class PointerTracker : public QObject
{
public:
  using QObject::QObject;
  bool eventFilter(QObject* watched, QEvent* event) override
  {
    if (event->type() == QEvent::Enter)
      watched->setProperty("mfPointerIn", true);
    else if (event->type() == QEvent::Leave)
      watched->setProperty("mfPointerIn", false);
    return false;
  }
};
} // namespace

bool
floating_tool_window(QObject* object)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || !QGuiApplication::platformName().startsWith(QLatin1String("wayland")))
    return false;
  window->installEventFilter(new PointerTracker(window));
  return true;
}
#endif

bool
floating_fullscreen_on(QObject* window)
{
  (void)window;
  return false;
}

bool
floating_maximized_on(QObject* window)
{
  (void)window;
  return false;
}

int
floating_cursor_in(QObject* object, int x, int y, int w, int h)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || w <= 0 || h <= 0)
    return 0;
  // X11 и macOS отдают глобальный курсор; Wayland — последний известный нам, поэтому вне окна он не в счёт
  if (QGuiApplication::platformName().startsWith(QLatin1String("wayland")) && !window->property("mfPointerIn").toBool())
    return 0;
  const QPoint p = window->mapFromGlobal(QCursor::pos(window->screen()));
  if (!QRect(x, y, w, h).contains(p))
    return 0;
  return QGuiApplication::mouseButtons() != Qt::NoButton ? 2 : 1;
}

int
floating_taskbar_place(QObject* window, int width, int height, bool embed, const QString& pos)
{
  (void)window;
  (void)width;
  (void)height;
  (void)embed;
  (void)pos;
  return 0;
}

QString
floating_taskbar_drag(QObject* window, int width, int height, int phase)
{
  (void)window;
  (void)width;
  (void)height;
  (void)phase;
  return {};
}

void
floating_taskbar_detach(QObject* window)
{
  (void)window;
}

bool
floating_key_hook(QObject* window, bool on)
{
  (void)window;
  (void)on;
  return false;
}

void
floating_set_click_through(QObject* object, bool enable)
{
  // X11/macOS: прозрачность для мыши — флаг Qt
  if (auto* window = qobject_cast<QWindow*>(object))
    window->setFlag(Qt::WindowTransparentForInput, enable);
}

bool
floating_windhawk_active()
{
  return false;
}

void
floating_set_mask_rounded(QObject* object, int x, int y, int w, int h, int radius)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window)
    return;
  window->setMask(w > 0 && h > 0 ? mask_region(x, y, w, h, radius) : QRegion());
}

} // namespace mf

#endif
