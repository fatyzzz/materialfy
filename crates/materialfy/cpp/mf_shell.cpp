#include "mf_shell.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QLibraryInfo>
#include <QtCore/QLocale>
#include <QtCore/QMetaObject>
#include <QtCore/QMutex>
#include <QtCore/QPointer>
#include <QtCore/QTimer>
#include <QtCore/QTranslator>
#include <QtCore/QUrl>
#include <QtGui/QClipboard>
#include <QtGui/QDesktopServices>
#include <QtGui/QFont>
#include <QtGui/QFontDatabase>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtGui/QImage>
#include <QtGui/QPainter>
#include <QtGui/QPixmapCache>
#include <QtGui/QScreen>
#include <QtGui/QStyleHints>
#include <QtGui/QWindow>
#include <QtNetwork/QNetworkAccessManager>
#include <QtNetwork/QNetworkDiskCache>
#include <QtNetwork/QNetworkProxy>
#include <QtNetwork/QNetworkReply>
#include <QtNetwork/QNetworkRequest>
#include <QtQml/QQmlContext>
#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>
#include <QtQml/QQmlNetworkAccessManagerFactory>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGRendererInterface>
#if QT_CONFIG(opengl)
#include <QtGui/QOpenGLContext>
#include <QtGui/QOpenGLFunctions>
#endif
#include <QtWidgets/QApplication>

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cwchar>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#if defined(Q_OS_LINUX)
#include <dlfcn.h>
#endif

#if defined(Q_OS_WIN)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <shobjidl.h>
#endif

#if defined(Q_OS_LINUX) && defined(__GLIBC__)
#include <malloc.h>
#endif

#include "materialfy/src/qt/ffi.cxx.h"

namespace mf {

namespace {
std::vector<std::string>&
arg_storage()
{
  static std::vector<std::string> storage;
  return storage;
}
std::vector<char*>&
argv_storage()
{
  static std::vector<char*> argv;
  return argv;
}
int argc_storage = 0;
// Удаляется в app_destroy — после движка и остановки tokio
QApplication* app = nullptr;
QPointer<QWindow> main_window;

// Картинки Spotify: один и тот же путь /image/<id> отдают несколько CDN. Основной i.scdn.co в РФ не открывается
// или отвечает по 8+ с — его адреса переписываем на зеркала (тот же список — sf_api::image_hosts). Берём текущее
// рабочее зеркало; таймаут или сетевая ошибка переводят всех на следующее (404 — нет картинки, хост ни при чём).
const char* const spotify_image_hosts[] = { "image-cdn-fa.spotifycdn.com", "image-cdn-ak.spotifycdn.com",
                                            "image-cdn.spotifycdn.com", "i2o.scdn.co" };
constexpr int spotify_image_host_count = 4;
// и прежние адреса: кэш обложек, скачанных до зеркал, лежит под i.scdn.co
const char* const spotify_image_known_hosts[] = { "i.scdn.co", "image-cdn-fa.spotifycdn.com", "image-cdn-ak.spotifycdn.com",
                                                  "image-cdn.spotifycdn.com", "i2o.scdn.co" };
std::atomic<int> spotify_image_host{ 0 };

bool
is_spotify_image_host(const QString& host)
{
  for (const char* known : spotify_image_known_hosts)
    if (host == QLatin1String(known))
      return true;
  return false;
}

// Только HTTP/1.1: по HTTP/2 сервер обложек отвечает REFUSED_STREAM, когда страница
// разом просит десятки картинок, и часть обложек не появляется.
class Http1Nam : public QNetworkAccessManager
{
public:
  using QNetworkAccessManager::QNetworkAccessManager;

protected:
  QNetworkReply* createRequest(Operation op, const QNetworkRequest& request, QIODevice* data) override
  {
    QNetworkRequest copy(request);
    copy.setAttribute(QNetworkRequest::Http2AllowedAttribute, false);
    // зависшее соединение не держит очередь (на хост их всего шесть): через 10 с — ошибка и повтор в QML
    copy.setTransferTimeout(10000);
    QUrl url = copy.url();
    if (op != GetOperation || !is_spotify_image_host(url.host()) || !url.path().startsWith(QLatin1String("/image/")))
      return QNetworkAccessManager::createRequest(op, copy, data);

    // проверка запасных обложек: сеть, где ни одно зеркало Spotify не открывается (и кэш не выручает)
    static const bool no_spotify_images = qEnvironmentVariableIsSet("MATERIALFY_TEST_NO_SPOTIFY_IMAGES");
    if (no_spotify_images) {
      url.setHost(QStringLiteral("spotify-images.invalid"));
      copy.setUrl(url);
      copy.setAttribute(QNetworkRequest::CacheLoadControlAttribute, QNetworkRequest::AlwaysNetwork);
      return QNetworkAccessManager::createRequest(op, copy, data);
    }
    // картинка по id не меняется: из кэша — без перепроверки; уже скачанное по любому из адресов не качаем
    copy.setAttribute(QNetworkRequest::CacheLoadControlAttribute, QNetworkRequest::PreferCache);
    if (QAbstractNetworkCache* disk = cache()) {
      for (const char* known : spotify_image_known_hosts) {
        QUrl cached = url;
        cached.setHost(QLatin1String(known));
        if (disk->metaData(cached).isValid()) {
          copy.setUrl(cached);
          return QNetworkAccessManager::createRequest(op, copy, data);
        }
      }
    }
    const int host = spotify_image_host.load();
    url.setHost(QLatin1String(spotify_image_hosts[host]));
    copy.setUrl(url);
    QNetworkReply* reply = QNetworkAccessManager::createRequest(op, copy, data);
    QObject::connect(reply, &QNetworkReply::finished, reply, [reply, host] {
      const auto error = reply->error();
      const bool host_failed = error == QNetworkReply::OperationCanceledError // таймаут
                               || error == QNetworkReply::TimeoutError || error == QNetworkReply::HostNotFoundError
                               || error == QNetworkReply::ConnectionRefusedError
                               || error == QNetworkReply::RemoteHostClosedError
                               || error == QNetworkReply::SslHandshakeFailedError
                               || error == QNetworkReply::TemporaryNetworkFailureError
                               || error == QNetworkReply::UnknownNetworkError
                               || error == QNetworkReply::ServiceUnavailableError
                               || error == QNetworkReply::InternalServerError;
      if (host_failed) {
        int expected = host;
        spotify_image_host.compare_exchange_strong(expected, (host + 1) % spotify_image_host_count);
      }
    });
    return reply;
  }
};

// Прокси картинок из настроек «Сеть»: общий для всех NAM, читается на каждый запрос
QMutex proxy_mutex;
int proxy_mode = 1;
QNetworkProxy proxy_manual;

class SettingsProxyFactory : public QNetworkProxyFactory
{
public:
  QList<QNetworkProxy> queryProxy(const QNetworkProxyQuery& query) override
  {
    int mode;
    QNetworkProxy manual;
    {
      QMutexLocker lock(&proxy_mutex);
      mode = proxy_mode;
      manual = proxy_manual;
    }
    switch (mode) {
      case 0:
        return { QNetworkProxy(QNetworkProxy::NoProxy) };
      case 2:
      case 3:
        return { manual };
      case 4:
        // неверные настройки: тип не подходит для HTTP — запрос падает с ProxyNotFoundError, а не идёт напрямую
        return { QNetworkProxy(QNetworkProxy::FtpCachingProxy) };
      default:
        return systemProxyForQuery(query);
    }
  }
};

// QML создаёт NAM в своих потоках загрузки — у каждого свой QNetworkDiskCache на общий каталог
class CachingNamFactory : public QQmlNetworkAccessManagerFactory
{
public:
  CachingNamFactory(QString dir, qint64 bytes)
    : m_dir(std::move(dir))
    , m_bytes(bytes)
  {
  }

  QNetworkAccessManager* create(QObject* parent) override
  {
    auto* nam = new Http1Nam(parent);
    auto* disk = new QNetworkDiskCache(nam);
    disk->setCacheDirectory(m_dir);
    disk->setMaximumCacheSize(m_bytes);
    nam->setCache(disk);
    nam->setProxyFactory(new SettingsProxyFactory);
    return nam;
  }

private:
  const QString m_dir;
  const qint64 m_bytes;
};
} // namespace

void
app_create(rust::Slice<const rust::String> args)
{
  auto& storage = arg_storage();
  storage.clear();
  for (const auto& a : args)
    storage.emplace_back(std::string(a));
  auto& argv = argv_storage();
  argv.clear();
  for (auto& s : storage)
    argv.push_back(s.data());
  argv.push_back(nullptr);
  argc_storage = static_cast<int>(storage.size());
  app = new QApplication(argc_storage, argv.data());
}

int
app_exec()
{
  return QApplication::exec();
}

void
app_destroy()
{
  delete app;
  app = nullptr;
}

void
app_quit()
{
  QCoreApplication::quit();
}

#if defined(Q_OS_WIN)
namespace {
wchar_t crash_log_path[MAX_PATH * 2] = {};

// Модуль и смещение адреса: «Qt6Qml.dll+0x1216b7» — как в журнале событий Windows
void
describe_address(void* address, char* out, size_t size)
{
  HMODULE module = nullptr;
  wchar_t name[MAX_PATH] = L"?";
  if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                         reinterpret_cast<LPCWSTR>(address),
                         &module) &&
      module) {
    wchar_t full[MAX_PATH];
    if (GetModuleFileNameW(module, full, MAX_PATH)) {
      const wchar_t* base = wcsrchr(full, L'\\');
      wcsncpy_s(name, base ? base + 1 : full, _TRUNCATE);
    }
  }
  const auto offset = reinterpret_cast<uintptr_t>(address) - reinterpret_cast<uintptr_t>(module);
  _snprintf_s(out, size, _TRUNCATE, "%ls+0x%llx", name, static_cast<unsigned long long>(offset));
}

LONG WINAPI
on_unhandled_exception(EXCEPTION_POINTERS* info)
{
  FILE* file = nullptr;
  if (_wfopen_s(&file, crash_log_path, L"ab") == 0 && file) {
    char where[MAX_PATH + 32];
    describe_address(info->ExceptionRecord->ExceptionAddress, where, sizeof where);
    fprintf(file, "=== native crash · Materialfy · exception 0x%08lx at %s\n",
            static_cast<unsigned long>(info->ExceptionRecord->ExceptionCode), where);
    void* frames[48];
    const USHORT count = RtlCaptureStackBackTrace(0, 48, frames, nullptr);
    for (USHORT i = 0; i < count; ++i) {
      describe_address(frames[i], where, sizeof where);
      fprintf(file, "  %2u %s\n", static_cast<unsigned>(i), where);
    }
    fclose(file);
  }
  return EXCEPTION_CONTINUE_SEARCH; // дальше — как обычно (отчёт Windows, журнал событий)
}
} // namespace
#elif defined(Q_OS_LINUX)
#include <execinfo.h>
#include <fcntl.h>
#include <signal.h>
#include <unistd.h>

#include <cstdio>
#include <cstring>

namespace {
char crash_log_path[4096] = {};

// Обработчик сигнала: только open/write/backtrace, без Qt и без выделения памяти в куче
void
on_fatal_signal(int signal_number)
{
  const int fd = open(crash_log_path, O_WRONLY | O_CREAT | O_APPEND, 0644);
  if (fd >= 0) {
    char header[96];
    const int length = snprintf(header, sizeof header, "=== native crash · Materialfy · signal %d\n", signal_number);
    const auto written = write(fd, header, length > 0 ? static_cast<size_t>(length) : 0);
    Q_UNUSED(written);
    void* frames[64];
    const int count = backtrace(frames, 64);
    backtrace_symbols_fd(frames, count, fd);
    close(fd);
  }
  // SA_RESETHAND уже вернул обработчик по умолчанию: повтор сигнала даёт обычное падение и дамп
  raise(signal_number);
}
} // namespace
#endif

void
install_native_crash_log(const QString& path)
{
#if defined(Q_OS_WIN)
  wcsncpy_s(crash_log_path, reinterpret_cast<const wchar_t*>(path.utf16()), _TRUNCATE);
  SetUnhandledExceptionFilter(on_unhandled_exception);
#elif defined(Q_OS_LINUX)
  const QByteArray utf8 = path.toUtf8();
  std::strncpy(crash_log_path, utf8.constData(), sizeof crash_log_path - 1);
  // первый вызов backtrace подгружает libgcc_s и может выделить память: делаем это до падения
  void* warmup[1];
  backtrace(warmup, 1);
  struct sigaction action {};
  action.sa_handler = on_fatal_signal;
  sigemptyset(&action.sa_mask);
  action.sa_flags = SA_RESETHAND;
  for (const int signal_number : {SIGSEGV, SIGBUS, SIGABRT, SIGFPE, SIGILL}) {
    sigaction(signal_number, &action, nullptr);
  }
#else
  Q_UNUSED(path);
#endif
}

void
set_qt_env(const QString& name, const QString& value)
{
#if defined(Q_OS_WIN)
  _wputenv_s(reinterpret_cast<const wchar_t*>(name.utf16()), reinterpret_cast<const wchar_t*>(value.utf16()));
#else
  qputenv(name.toUtf8().constData(), value.toUtf8());
#endif
}

void
app_set_names(const QString& name, const QString& org, const QString& desktop)
{
  QCoreApplication::setApplicationName(name);
  QCoreApplication::setOrganizationName(org);
  QGuiApplication::setDesktopFileName(desktop);
#if defined(Q_OS_WIN)
  // AppUserModelID = имя приложения (как у ярлыков из materialfy.iss): группировка на панели задач
  SetCurrentProcessExplicitAppUserModelID(reinterpret_cast<const wchar_t*>(name.utf16()));
#endif
}

void
app_set_quit_on_last_window_closed(bool value)
{
  QGuiApplication::setQuitOnLastWindowClosed(value);
}

void
app_set_window_icon(const QString& path)
{
  QGuiApplication::setWindowIcon(QIcon(path));
}

int
app_add_font(const QString& path)
{
  return QFontDatabase::addApplicationFont(path);
}

void
app_set_qt_language(const QString& code)
{
  static QTranslator* translator = nullptr;
  if (translator) {
    QCoreApplication::removeTranslator(translator);
    delete translator;
    translator = nullptr;
  }
  const bool english = code.isEmpty() || code == QLatin1String("en");
  QLocale::setDefault(english ? QLocale(QLocale::English, QLocale::UnitedStates) : QLocale(code));
  if (english)
    return;
  // windeployqt --translations <code> puts qtbase_<code>.qm into translations/ next to the exe;
  // a development build finds it in the Qt installation.
  const QString name = QStringLiteral("qtbase_") + code;
  auto* t = new QTranslator();
  const QStringList dirs{ QCoreApplication::applicationDirPath() + QStringLiteral("/translations"),
                          QLibraryInfo::path(QLibraryInfo::TranslationsPath) };
  for (const auto& dir : dirs) {
    if (t->load(name, dir)) {
      QCoreApplication::installTranslator(t);
      translator = t;
      return;
    }
  }
  delete t;
}

QStringList
system_ui_languages()
{
  return QLocale::system().uiLanguages(QLocale::TagSeparator::Dash);
}

void
copy_to_clipboard(const QString& text)
{
  // Windows: буфер бывает занят другим процессом (история буфера и т. п.) — OleSetClipboard
  // отказывает; повторяем с паузой. Более новое копирование отменяет повторы старого.
  static int generation = 0;
  struct Attempt
  {
    static void run(const QString& text, int gen, int left)
    {
      auto* clipboard = QGuiApplication::clipboard();
      if (!clipboard || gen != generation)
        return;
      clipboard->setText(text);
      if (left > 0 && !clipboard->ownsClipboard())
        QTimer::singleShot(80, qApp, [text, gen, left] { run(text, gen, left - 1); });
    }
  };
  Attempt::run(text, ++generation, 4);
}

void
set_cpp_ownership(QObject* object)
{
  if (object)
    QQmlEngine::setObjectOwnership(object, QQmlEngine::CppOwnership);
}

bool
system_color_scheme_dark()
{
  return QGuiApplication::styleHints()->colorScheme() != Qt::ColorScheme::Light;
}

void
watch_color_scheme()
{
  QObject::connect(QGuiApplication::styleHints(), &QStyleHints::colorSchemeChanged, qApp, [](Qt::ColorScheme scheme) {
    system_dark_changed(scheme != Qt::ColorScheme::Light);
  });
}

#if defined(Q_OS_WIN) && defined(_M_X64)
namespace {
std::atomic<long long> gui_tick_ms{ 0 };
HANDLE gui_thread = nullptr;

long long
monotonic_ms()
{
  return static_cast<long long>(GetTickCount64());
}

// Стек приостановленного потока: адреса возврата по таблицам раскрутки x64 (без dbghelp и без выделения памяти,
// пока поток стоит — он может держать блокировку кучи)
USHORT
capture_thread_stack(HANDLE thread, void** frames, USHORT capacity)
{
  CONTEXT context{};
  context.ContextFlags = CONTEXT_FULL;
  if (!GetThreadContext(thread, &context))
    return 0;
  USHORT count = 0;
  while (count < capacity && context.Rip) {
    frames[count++] = reinterpret_cast<void*>(context.Rip);
    DWORD64 image_base = 0;
    PRUNTIME_FUNCTION function = RtlLookupFunctionEntry(context.Rip, &image_base, nullptr);
    if (!function) {
      // листовая функция: адрес возврата на вершине стека
      context.Rip = *reinterpret_cast<DWORD64*>(context.Rsp);
      context.Rsp += 8;
      continue;
    }
    void* handler_data = nullptr;
    DWORD64 establisher = 0;
    RtlVirtualUnwind(UNW_FLAG_NHANDLER, image_base, context.Rip, function, &context, &handler_data, &establisher, nullptr);
  }
  return count;
}

// Сторож: GUI-поток не отметился 10 с — окно «не отвечает». Его стек — в crash.log, один раз на зависание.
void
watch_gui_thread()
{
  bool reported = false;
  for (;;) {
    Sleep(2000);
    const long long silent = monotonic_ms() - gui_tick_ms.load();
    if (silent < 10000) {
      reported = false;
      continue;
    }
    if (reported || !gui_thread || !crash_log_path[0])
      continue;
    reported = true;
    // Несколько снимков через полсекунды, пока поток молчит: долгая работа видна в каждом, случайное — в одном
    constexpr int max_samples = 12;
    constexpr USHORT depth = 64;
    static void* samples[max_samples][depth];
    USHORT counts[max_samples] = {};
    int taken = 0;
    while (taken < max_samples && monotonic_ms() - gui_tick_ms.load() >= 10000) {
      if (SuspendThread(gui_thread) != static_cast<DWORD>(-1)) {
        counts[taken] = capture_thread_stack(gui_thread, samples[taken], depth);
        ResumeThread(gui_thread);
        ++taken;
      }
      Sleep(500);
    }
    FILE* file = nullptr;
    if (_wfopen_s(&file, crash_log_path, L"ab") == 0 && file) {
      fprintf(file, "=== hang · Materialfy · the GUI thread has not responded for %lld s, %d samples\n",
              (monotonic_ms() - gui_tick_ms.load()) / 1000, taken);
      char where[MAX_PATH + 32];
      // сводка: в скольких снимках встречается адрес (верхние 40 по частоте)
      std::vector<std::pair<void*, int>> hits;
      for (int s = 0; s < taken; ++s)
        for (USHORT i = 0; i < counts[s]; ++i) {
          auto it = std::find_if(hits.begin(), hits.end(), [&](const auto& h) { return h.first == samples[s][i]; });
          if (it == hits.end())
            hits.emplace_back(samples[s][i], 1);
          else if (std::none_of(samples[s], samples[s] + i, [&](void* a) { return a == samples[s][i]; }))
            ++it->second;
        }
      std::stable_sort(hits.begin(), hits.end(), [](const auto& a, const auto& b) { return a.second > b.second; });
      fprintf(file, "  most frequent frames (samples containing it):\n");
      for (size_t i = 0; i < hits.size() && i < 40; ++i) {
        describe_address(hits[i].first, where, sizeof where);
        fprintf(file, "    %2d/%d %s\n", hits[i].second, taken, where);
      }
      for (int s = 0; s < taken; ++s) {
        fprintf(file, "  sample %d:\n", s);
        for (USHORT i = 0; i < counts[s]; ++i) {
          describe_address(samples[s][i], where, sizeof where);
          fprintf(file, "    %2u %s\n", static_cast<unsigned>(i), where);
        }
      }
      fclose(file);
    }
  }
}
} // namespace
#endif

void
start_gui_heartbeat()
{
  auto* timer = new QTimer(qApp);
  timer->setInterval(1000);
#if defined(Q_OS_WIN) && defined(_M_X64)
  QObject::connect(timer, &QTimer::timeout, qApp, [] {
    gui_tick_ms.store(monotonic_ms());
    gui_heartbeat();
  });
  gui_tick_ms.store(monotonic_ms());
  DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &gui_thread,
                  THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, 0);
  std::thread(watch_gui_thread).detach();
#else
  QObject::connect(timer, &QTimer::timeout, qApp, [] { gui_heartbeat(); });
#endif
  timer->start();
  gui_heartbeat();
}

void
watch_app_state()
{
  QObject::connect(qApp, &QGuiApplication::applicationStateChanged, qApp, [](Qt::ApplicationState state) {
    app_active_changed(state == Qt::ApplicationActive);
  });
}

bool
open_local_folder(const QString& path)
{
  return QDesktopServices::openUrl(QUrl::fromLocalFile(path));
}

void
engine_add_import_path(QQmlApplicationEngine& engine, const QString& path)
{
  engine.addImportPath(path);
}

void
engine_set_context_bool(QQmlApplicationEngine& engine, const QString& name, bool value)
{
  engine.rootContext()->setContextProperty(name, value);
}

void
engine_touch_singletons(QQmlApplicationEngine& engine, const QStringList& names)
{
  for (const auto& name : names)
    engine.singletonInstance<QObject*>(QStringLiteral("Materialfy.Core"), name);
}

bool
engine_has_root(const QQmlApplicationEngine& engine)
{
  return !engine.rootObjects().isEmpty();
}

QObject*
engine_root_object(const QQmlApplicationEngine& engine)
{
  const auto roots = engine.rootObjects();
  return roots.isEmpty() ? nullptr : roots.first();
}

void
engine_install_caching_nam(QQmlApplicationEngine& engine, const QString& dir, std::int64_t max_bytes)
{
  // Фабрика должна жить дольше движка — не удаляется
  engine.setNetworkAccessManagerFactory(new CachingNamFactory(dir, static_cast<qint64>(max_bytes)));
}

void
set_image_proxy(int mode, const QString& host, int port, const QString& user, const QString& password)
{
  QNetworkProxy manual;
  // SOCKS5 по умолчанию разрешает имена на стороне прокси (как socks5h в sf-auth)
  if (mode == 2 || mode == 3)
    manual = QNetworkProxy(mode == 3 ? QNetworkProxy::Socks5Proxy : QNetworkProxy::HttpProxy,
                           host,
                           static_cast<quint16>(port),
                           user,
                           password);
  QMutexLocker lock(&proxy_mutex);
  proxy_mode = mode;
  proxy_manual = manual;
}

bool
object_has_property(QObject* object, const QString& name)
{
  return object && object->property(name.toUtf8().constData()).isValid();
}

void
set_main_window(QObject* window)
{
  main_window = qobject_cast<QWindow*>(window);
}

void
post_raise_window()
{
  if (!QCoreApplication::instance())
    return;
  QMetaObject::invokeMethod(
    QCoreApplication::instance(),
    [] {
      if (!main_window)
        return;
      main_window->show();
      // свёрнутое окно show() не разворачивает
      if (main_window->windowStates() & Qt::WindowMinimized)
        main_window->setWindowStates(main_window->windowStates() & ~Qt::WindowMinimized);
      main_window->raise();
      main_window->requestActivate();
    },
    Qt::QueuedConnection);
}

void
post_quit()
{
  if (!QCoreApplication::instance())
    return;
  QMetaObject::invokeMethod(QCoreApplication::instance(), [] { QCoreApplication::quit(); }, Qt::QueuedConnection);
}

void
schedule_screenshot(QObject* window, const QString& path, int delay_ms)
{
  QPointer<QQuickWindow> target = qobject_cast<QQuickWindow*>(window);
  QTimer::singleShot(delay_ms, qApp, [target, path] {
    if (target && !target->grabWindow().save(path))
      qWarning("could not save screenshot %s", qPrintable(path));
    QCoreApplication::quit();
  });
}

namespace {
// Return freed heap memory to the OS and shrink the working set (what Task Manager shows as "Memory")
void
trim_process_memory()
{
#if defined(Q_OS_WIN)
  HEAP_OPTIMIZE_RESOURCES_INFORMATION info{ HEAP_OPTIMIZE_RESOURCES_CURRENT_VERSION, 0 };
  HeapSetInformation(nullptr, HeapOptimizeResources, &info, sizeof(info));
  HeapCompact(GetProcessHeap(), 0);
  SetProcessWorkingSetSize(GetCurrentProcess(), static_cast<SIZE_T>(-1), static_cast<SIZE_T>(-1));
#elif defined(Q_OS_LINUX) && defined(__GLIBC__)
  malloc_trim(0);
#endif
}

// Qt Quick Shapes (curve renderer) do not rebuild their geometry when the scene graph comes back after
// releaseResources(): the new node stays empty until a path changes. Emitting QQuickPath::changed marks
// every ShapePath dirty, and the next frame tessellates it again.
void
refresh_shapes(QQuickItem* item)
{
  if (item->inherits("QQuickShape")) {
    for (QObject* child : item->children()) {
      if (child->inherits("QQuickShapePath"))
        QMetaObject::invokeMethod(child, "changed", Qt::DirectConnection);
    }
  }
  for (QQuickItem* child : item->childItems())
    refresh_shapes(child);
}
} // namespace

void
install_memory_trim(QObject* object)
{
  auto* window = qobject_cast<QQuickWindow*>(object);
  if (!window)
    return;
  // Without these releaseResources() keeps the scene graph and the D3D/Vulkan/GL device of a hidden window
  window->setPersistentGraphics(false);
  window->setPersistentSceneGraph(false);
  // A short minimize (switching windows) does not tear the scene down
  auto* settle = new QTimer(window);
  settle->setSingleShot(true);
  settle->setInterval(3000);
  QObject::connect(settle, &QTimer::timeout, window, [window] {
    const auto v = window->visibility();
    if (v != QWindow::Hidden && v != QWindow::Minimized)
      return;
    window->releaseResources();
    if (auto* engine = qmlEngine(window)) {
      engine->collectGarbage();
      engine->trimComponentCache();
    }
    QPixmapCache::clear();
    trim_process_memory();
  });
  // Every scene graph after the first one follows a release (ours or Qt's own on hide)
  auto inits = std::make_shared<int>(0);
  QObject::connect(
    window,
    &QQuickWindow::sceneGraphInitialized,
    window,
    [window, inits] {
      if ((*inits)++ > 0)
        refresh_shapes(window->contentItem());
    },
    Qt::QueuedConnection);
  QObject::connect(window, &QWindow::visibilityChanged, settle, [settle](QWindow::Visibility v) {
    if (v == QWindow::Hidden || v == QWindow::Minimized)
      settle->start();
    else
      settle->stop();
  });
}

namespace {
bool
is_software_device_name(QString name)
{
  name = name.toLower();
  return name.contains(QLatin1String("llvmpipe")) || name.contains(QLatin1String("softpipe")) ||
         name.contains(QLatin1String("swiftshader")) || name.contains(QLatin1String("lavapipe")) ||
         name.contains(QLatin1String("basic render driver")) || name.contains(QLatin1String("gdi generic"));
}

// Поток рендера, сразу после создания графики окна: рисует ли кадры процессор
bool
detect_software_rendering(QQuickWindow* window)
{
  QSGRendererInterface* ri = window->rendererInterface();
  if (!ri)
    return false;
  switch (ri->graphicsApi()) {
    case QSGRendererInterface::Software:
      return true;
#if QT_CONFIG(opengl)
    case QSGRendererInterface::OpenGL: {
      QOpenGLContext* ctx = QOpenGLContext::currentContext();
      if (!ctx)
        return false;
      const auto* renderer = reinterpret_cast<const char*>(ctx->functions()->glGetString(GL_RENDERER));
      return renderer && is_software_device_name(QString::fromLatin1(renderer));
    }
#endif
#if defined(Q_OS_WIN)
    case QSGRendererInterface::Direct3D11: {
      auto* device =
        static_cast<ID3D11Device*>(ri->getResource(window, QSGRendererInterface::DeviceResource));
      if (!device)
        return false;
      bool software = false;
      IDXGIDevice* dxgi = nullptr;
      if (SUCCEEDED(device->QueryInterface(__uuidof(IDXGIDevice), reinterpret_cast<void**>(&dxgi)))) {
        IDXGIAdapter* adapter = nullptr;
        if (SUCCEEDED(dxgi->GetAdapter(&adapter))) {
          IDXGIAdapter1* adapter1 = nullptr;
          if (SUCCEEDED(adapter->QueryInterface(__uuidof(IDXGIAdapter1), reinterpret_cast<void**>(&adapter1)))) {
            DXGI_ADAPTER_DESC1 desc{};
            if (SUCCEEDED(adapter1->GetDesc1(&desc)))
              software = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) ||
                         is_software_device_name(QString::fromWCharArray(desc.Description));
            adapter1->Release();
          }
          adapter->Release();
        }
        dxgi->Release();
      }
      return software;
    }
#endif
    default:
      return false;
  }
}
} // namespace

void
watch_software_rendering(QObject* object)
{
  auto* window = qobject_cast<QQuickWindow*>(object);
  if (!window)
    return;
  const QString forced = qEnvironmentVariable("MATERIALFY_SOFTWARE_RENDERING");
  if (!forced.isEmpty()) {
    window->setProperty("softwareRendering", forced != QLatin1String("0"));
    return;
  }
  // sceneGraphInitialized приходит в потоке рендера, где контекст OpenGL текущий; свойство окна — в GUI-потоке
  QObject::connect(
    window,
    &QQuickWindow::sceneGraphInitialized,
    window,
    [window] {
      const bool software = detect_software_rendering(window);
      QMetaObject::invokeMethod(
        window, [window, software] { window->setProperty("softwareRendering", software); }, Qt::QueuedConnection);
    },
    Qt::DirectConnection);
}

QString
prefer_fastest_screen()
{
#if defined(Q_OS_LINUX)
  // Qt Quick берёт период vsync у primaryScreen(); на Wayland это может быть 60-герцовый монитор,
  // и анимации на 144 Гц идут по таймеру 60 Гц. Публичного API нет — экспортируемая функция
  // QWindowSystemInterface::handlePrimaryScreenChanged (её зовут платформенные плагины).
  const auto screens = QGuiApplication::screens();
  if (screens.size() < 2)
    return QStringLiteral("primary screen: single monitor");
  QScreen* primary = QGuiApplication::primaryScreen();
  QScreen* fastest = *std::max_element(
    screens.begin(), screens.end(), [](QScreen* a, QScreen* b) { return a->refreshRate() < b->refreshRate(); });
  if (fastest == primary || fastest->refreshRate() <= primary->refreshRate() + 0.5)
    return QStringLiteral("primary screen: %1 is already the fastest").arg(primary->name());

  using SetPrimary = void (*)(void*);
  auto set_primary = reinterpret_cast<SetPrimary>(
    dlsym(RTLD_DEFAULT, "_ZN22QWindowSystemInterface26handlePrimaryScreenChangedEP15QPlatformScreen"));
  if (!set_primary)
    return QStringLiteral("primary screen: could not hook into Qt (no handlePrimaryScreenChanged)");
  void* platform_screen = fastest->handle();
  if (!platform_screen)
    return QStringLiteral("primary screen: the screen has no platform object");
  const QString before =
    QStringLiteral("%1 (%2 Hz)").arg(primary->name()).arg(qRound(primary->refreshRate()));
  set_primary(platform_screen);
  QScreen* now = QGuiApplication::primaryScreen();
  return QStringLiteral("primary screen: %1 → %2 (%3 Hz)")
    .arg(before, now->name())
    .arg(qRound(now->refreshRate()));
#else
  return QStringLiteral("primary screen: refresh-rate switching is not supported on this OS");
#endif
}

QRectF
icon_measure(const QString& text, int size)
{
  if (text.isEmpty() || size <= 0)
    return {};
  QFont font(QStringLiteral("Material Symbols Rounded"));
  font.setPixelSize(size);
  font.setVariableAxis(QFont::Tag("FILL"), 1);
  font.setVariableAxis(QFont::Tag("opsz"), static_cast<float>(std::clamp(size, 20, 48)));
  const QFontMetricsF metrics(font);
  const qreal advance = metrics.horizontalAdvance(text);
  const qreal height = metrics.height();
  const int w = static_cast<int>(advance) + 8;
  const int h = static_cast<int>(height) + 8;
  QImage image(w, h, QImage::Format_Alpha8);
  image.fill(0);
  {
    QPainter painter(&image);
    painter.setFont(font);
    painter.setPen(Qt::black);
    painter.drawText(QRectF(4, 4, advance, height), Qt::AlignLeft | Qt::AlignTop, text);
  }

  constexpr uchar threshold = 96;
  int top = -1, bottom = -1, left = -1, right = -1;
  for (int y = 0; y < h; ++y) {
    const uchar* row = image.constScanLine(y);
    int first = -1, last = -1;
    for (int x = 0; x < w; ++x) {
      if (row[x] > threshold) {
        if (first < 0)
          first = x;
        last = x;
      }
    }
    if (first < 0)
      continue;
    if (top < 0)
      top = y;
    bottom = y;
    left = left < 0 ? first : std::min(left, first);
    right = right < 0 ? last : std::max(right, last);
  }
  if (top < 0)
    return {};
  const qreal cx = (left + right + 1) / 2.0 - (4 + advance / 2);
  const qreal cy = (top + bottom + 1) / 2.0 - (4 + height / 2);
  return QRectF(cx, cy, right - left + 1, bottom - top + 1);
}

} // namespace mf
