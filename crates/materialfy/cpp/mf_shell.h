// Мелкие помощники Qt, которых нет в cxx-qt-lib. Владелец — агент «shell».
#pragma once

#include <QtCore/QObject>
#include <QtCore/QRectF>
#include <QtCore/QString>
#include <QtCore/QStringList>
#include <QtGui/QColor>
#include <QtQml/QQmlApplicationEngine>

#include <cstdint>

#include "rust/cxx.h"

namespace mf {

// QApplication (Qt Widgets: Qt.labs.platform берёт из него трей и меню на всех ОС).
// Аргументы копируются и живут до конца процесса.
void app_create(rust::Slice<const rust::String> args);
int app_exec();
// Удалить QApplication в конце run (после движка и остановки фоновых потоков)
void app_destroy();
void app_quit();
void app_set_names(const QString& app, const QString& org, const QString& desktop);
// Переменная окружения, которую видит и Qt (на Windows он читает копию C-рантайма, а не окружение процесса)
void set_qt_env(const QString& name, const QString& value);
// Windows и Linux: падение в нативном коде (Qt, драйвер): код или сигнал и цепочка вызовов в файл crash.log
void install_native_crash_log(const QString& path);
void app_set_quit_on_last_window_closed(bool value);
void app_set_window_icon(const QString& path);
// QFontDatabase::addApplicationFont; -1 — ошибка
int app_add_font(const QString& path);
// Qt's own strings (dialogs, context menus): qtbase_<code>.qm from translations/ next to the exe or Qt's
// TranslationsPath; "en" removes the translator. Also sets the default QLocale.
void app_set_qt_language(const QString& code);
// The OS UI languages, most preferred first (QLocale::system().uiLanguages(), BCP 47: "ru-RU", "en-US"…).
// Not affected by app_set_qt_language (QLocale::setDefault).
QStringList system_ui_languages();

// Буфер обмена (Auth.copyToClipboard, Catalog.copyLink)
void copy_to_clipboard(const QString& text);

// Объект, отданный в QML из слота, иначе станет JavaScriptOwnership и попадёт под сборщик мусора
void set_cpp_ownership(QObject* object);

// Системная тема: true — тёмная (QStyleHints::colorScheme() != Light)
bool system_color_scheme_dark();
// Подписка на QStyleHints::colorSchemeChanged → Rust system_dark_changed(bool)
void watch_color_scheme();
// Подписка на QGuiApplication::applicationStateChanged → Rust app_active_changed(bool) (опрос удалённого плеера)
void watch_app_state();
// GUI-поток жив: раз в секунду gui_heartbeat() (single_instance: зависшее окно не «показываем»)
void start_gui_heartbeat();
// Каталог в файловом менеджере (Settings.openCacheFolder)
bool open_local_folder(const QString& path);

// Загрузка QML: движок, контекстные свойства, начальные свойства корня
void engine_add_import_path(QQmlApplicationEngine& engine, const QString& path);
void engine_set_context_bool(QQmlApplicationEngine& engine, const QString& name, bool value);
// Создать все синглтоны модуля заранее (как qmlRegisterSingletonInstance в Python-версии)
void engine_touch_singletons(QQmlApplicationEngine& engine, const QStringList& names);
bool engine_has_root(const QQmlApplicationEngine& engine);
// Первый корневой объект (QQuickWindow) или nullptr
QObject* engine_root_object(const QQmlApplicationEngine& engine);
// Дисковый кэш картинок (images.py): у каждого NAM свой QNetworkDiskCache на общий каталог, только HTTP/1.1
void engine_install_caching_nam(QQmlApplicationEngine& engine, const QString& dir, std::int64_t max_bytes);
// Прокси для этих NAM (потокобезопасно, действует на следующие запросы):
// 0 — без прокси, 1 — системный, 2 — HTTP, 3 — SOCKS5, 4 — заблокировать (неверные настройки)
void set_image_proxy(int mode, const QString& host, int port, const QString& user, const QString& password);

// Есть ли у объекта свойство (property(name).isValid(), как `is not None` в app.py)
bool object_has_property(QObject* object, const QString& name);
// Окно, которое показывают AppRequest::RaiseWindow (держится через QPointer)
void set_main_window(QObject* window);
// Потокобезопасно: выполнить в GUI-потоке show/raise/requestActivate главного окна или выход
void post_raise_window();
void post_quit();
// Через delay мс: grabWindow().save(path) и выход
void schedule_screenshot(QObject* window, const QString& path, int delay_ms);
// Hidden to the tray or minimized for a few seconds: drop the scene graph and the graphics device
// (QQuickWindow::releaseResources with non-persistent graphics), collect JS garbage, compact the heaps and
// trim the working set. Showing the window rebuilds everything on the first frame.
void install_memory_trim(QObject* window);
// Software rendering (llvmpipe, WARP, SwiftShader, Qt Quick software backend: the CPU draws every frame): once the
// scene graph is up, the window gets softwareRendering = true and drops its decorative effects.
// MATERIALFY_SOFTWARE_RENDERING=1 / 0 forces it either way.
void watch_software_rendering(QObject* window);

// Самый быстрый монитор — основным (display.py). Строка для лога.
QString prefer_fastest_screen();

// IconMetrics.ink: смещение центра контура глифа и его размер (icons.py, _measure)
QRectF icon_measure(const QString& text, int size);

} // namespace mf
