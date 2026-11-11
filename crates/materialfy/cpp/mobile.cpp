#include "mobile.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtCore/QDirIterator>
#include <QtCore/QFile>
#include <QtCore/QFileInfo>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtCore/QStandardPaths>
#include <QtCore/QUrl>
#include <QtCore/QtLogging>

#include <cstdio>
#include <QtGui/QDesktopServices>

#if defined(Q_OS_ANDROID)
#include <QtCore/QJniEnvironment>
#include <QtCore/QJniObject>
#include <QtCore/qcoreapplication_platform.h>
#include <jni.h>
#endif

namespace mf {

namespace {
QPointer<MobileShell> g_instance;

#if defined(Q_OS_ANDROID)
constexpr const char* BRIDGE = "org/materialfy/MobileBridge";

QJniObject
context_object()
{
  return QNativeInterface::QAndroidApplication::context();
}

QJniObject
jstr(const QString& text)
{
  return QJniObject::fromString(text);
}

// org.materialfy.MobileBridge.onMediaCommand(String, long) — из Java (MediaSession, кнопки уведомления)
void JNICALL
native_media_command(JNIEnv* env, jclass, jstring command, jlong arg)
{
  const char* chars = env->GetStringUTFChars(command, nullptr);
  const QString text = QString::fromUtf8(chars);
  env->ReleaseStringUTFChars(command, chars);
  mobile_post_media_command(text, static_cast<qint64>(arg));
}

void
register_natives()
{
  static bool done = false;
  if (done)
    return;
  done = true;
  QJniEnvironment env;
  const JNINativeMethod methods[] = {
    { "onMediaCommand", "(Ljava/lang/String;J)V", reinterpret_cast<void*>(native_media_command) },
  };
  if (!env.registerNativeMethods(BRIDGE, methods, 1))
    qWarning("materialfy: could not register MobileBridge natives");
}

// Рекурсивно скопировать каталог из assets:/ (QFile умеет читать APK-ассеты через движок платформы)
bool
copy_tree(const QString& from, const QString& to)
{
  QDir().mkpath(to);
  bool ok = true;
  QDirIterator it(from, QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot, QDirIterator::Subdirectories);
  while (it.hasNext()) {
    const QString path = it.next();
    const QString rel = path.mid(from.size());
    const QString target = to + rel;
    if (it.fileInfo().isDir()) {
      QDir().mkpath(target);
      continue;
    }
    QDir().mkpath(QFileInfo(target).absolutePath());
    QFile::remove(target);
    if (!QFile::copy(path, target)) {
      qWarning("materialfy: could not extract %s", qPrintable(rel));
      ok = false;
    } else {
      QFile::setPermissions(target, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    }
  }
  return ok;
}
#endif
} // namespace

MobileShell::MobileShell(QObject* parent)
  : QObject(parent)
{
  g_instance = this;
#if defined(Q_OS_ANDROID)
  register_natives();
#endif
}

MobileShell*
MobileShell::instance()
{
  return g_instance.data();
}

bool
MobileShell::android() const
{
  return is_android();
}

bool
MobileShell::islandSupported() const
{
#if defined(Q_OS_ANDROID)
  return QJniObject::callStaticMethod<jboolean>(BRIDGE, "islandSupported", "()Z");
#else
  return false;
#endif
}

bool
MobileShell::islandEnabled() const
{
#if defined(Q_OS_ANDROID)
  return QJniObject::callStaticMethod<jboolean>(
    BRIDGE, "islandEnabled", "(Landroid/content/Context;)Z", context_object().object());
#else
  return false;
#endif
}

void
MobileShell::setIslandEnabled(bool on)
{
#if defined(Q_OS_ANDROID)
  if (on == islandEnabled())
    return;
  QJniObject::callStaticMethod<void>(
    BRIDGE, "setIslandEnabled", "(Landroid/content/Context;Z)V", context_object().object(), static_cast<jboolean>(on));
  QJniEnvironment env;
  env.checkAndClearExceptions();
  emit islandEnabledChanged();
#else
  Q_UNUSED(on)
#endif
}

void
MobileShell::updateMedia(const QString& title,
                         const QString& artist,
                         const QString& album,
                         const QString& coverUrl,
                         qint64 durationMs,
                         qint64 positionMs,
                         bool playing,
                         bool hasTrack,
                         bool canPrevious,
                         bool canNext,
                         bool local)
{
#if defined(Q_OS_ANDROID)
  QJniObject::callStaticMethod<void>(
    BRIDGE,
    "updateMedia",
    "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JJZZZZZ)V",
    context_object().object(),
    jstr(title).object<jstring>(),
    jstr(artist).object<jstring>(),
    jstr(album).object<jstring>(),
    jstr(coverUrl).object<jstring>(),
    static_cast<jlong>(durationMs),
    static_cast<jlong>(positionMs),
    static_cast<jboolean>(playing),
    static_cast<jboolean>(hasTrack),
    static_cast<jboolean>(canPrevious),
    static_cast<jboolean>(canNext),
    static_cast<jboolean>(local));
  QJniEnvironment env;
  env.checkAndClearExceptions();
#else
  Q_UNUSED(title)
  Q_UNUSED(artist)
  Q_UNUSED(album)
  Q_UNUSED(coverUrl)
  Q_UNUSED(durationMs)
  Q_UNUSED(positionMs)
  Q_UNUSED(playing)
  Q_UNUSED(hasTrack)
  Q_UNUSED(canPrevious)
  Q_UNUSED(canNext)
  Q_UNUSED(local)
#endif
}

void
MobileShell::setSystemBars(bool lightBars)
{
  if (m_lightBarsSet && m_lightBars == lightBars)
    return;
  m_lightBars = lightBars;
  m_lightBarsSet = true;
#if defined(Q_OS_ANDROID)
  QNativeInterface::QAndroidApplication::runOnAndroidMainThread([lightBars] {
    QJniObject::callStaticMethod<void>(BRIDGE,
                                       "setSystemBars",
                                       "(Landroid/content/Context;Z)V",
                                       context_object().object(),
                                       static_cast<jboolean>(lightBars));
    QJniEnvironment env;
    env.checkAndClearExceptions();
  });
#endif
}

void
MobileShell::setBrowserWait(bool waiting)
{
  if (m_browserWait == waiting)
    return;
  m_browserWait = waiting;
#if defined(Q_OS_ANDROID)
  QJniObject::callStaticMethod<void>(
    BRIDGE, "setBrowserWait", "(Landroid/content/Context;Z)V", context_object().object(), static_cast<jboolean>(waiting));
  QJniEnvironment env;
  env.checkAndClearExceptions();
#endif
}

void
MobileShell::moveTaskToBack()
{
#if defined(Q_OS_ANDROID)
  QNativeInterface::QAndroidApplication::runOnAndroidMainThread([] {
    QJniObject::callStaticMethod<void>(
      BRIDGE, "moveTaskToBack", "(Landroid/content/Context;)V", context_object().object());
    QJniEnvironment env;
    env.checkAndClearExceptions();
  });
#endif
}

void
MobileShell::setMediaLiked(bool liked)
{
#if defined(Q_OS_ANDROID)
  QJniObject::callStaticMethod<void>(BRIDGE, "setMediaLiked", "(Z)V", static_cast<jboolean>(liked));
  QJniEnvironment env;
  env.checkAndClearExceptions();
#else
  Q_UNUSED(liked);
#endif
}

void
MobileShell::haptic()
{
#if defined(Q_OS_ANDROID)
  QNativeInterface::QAndroidApplication::runOnAndroidMainThread([] {
    QJniObject::callStaticMethod<void>(
      BRIDGE, "haptic", "(Landroid/content/Context;)V", context_object().object());
    QJniEnvironment env;
    env.checkAndClearExceptions();
  });
#endif
}

void
MobileShell::shareLog()
{
#if defined(Q_OS_ANDROID)
  // Хвост журнала: прошлый запуск (.old) + этот. Не больше ~150 КБ — Intent идёт через Binder (лимит ~1 МБ на всё)
  constexpr qint64 LIMIT = 150 * 1024;
  const QString path = qEnvironmentVariable("MATERIALFY_LOG_FILE");
  QByteArray data;
  for (const QString& file : { path + QStringLiteral(".old"), path }) {
    QFile f(file);
    if (f.open(QIODevice::ReadOnly))
      data += f.readAll();
  }
  if (data.size() > LIMIT) {
    data = data.right(LIMIT);
    const qsizetype nl = data.indexOf('\n');
    if (nl >= 0)
      data = data.mid(nl + 1);
  }
  const QString text = QString::fromUtf8(data);
  QNativeInterface::QAndroidApplication::runOnAndroidMainThread([text] {
    QJniObject::callStaticMethod<void>(BRIDGE,
                                       "shareText",
                                       "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                                       context_object().object(),
                                       jstr(QStringLiteral("materialfy.log")).object<jstring>(),
                                       jstr(text).object<jstring>());
    QJniEnvironment env;
    env.checkAndClearExceptions();
  });
#endif
}

#if defined(Q_OS_ANDROID)
namespace {
// Сообщения Qt и ошибки QML — в stderr: на Android его читает поток Rust (main.rs) и пишет в logcat и в журнал
void
log_to_stderr(QtMsgType type, const QMessageLogContext& context, const QString& message)
{
  const char* level = type == QtDebugMsg     ? "debug"
                      : type == QtInfoMsg    ? "info"
                      : type == QtWarningMsg ? "warning"
                      : type == QtCriticalMsg ? "critical"
                                              : "fatal";
  const QByteArray text = message.toUtf8();
  if (context.file && context.line > 0)
    std::fprintf(stderr, "Qt %s: %s (%s:%d)\n", level, text.constData(), context.file, context.line);
  else
    std::fprintf(stderr, "Qt %s: %s\n", level, text.constData());
  std::fflush(stderr);
}
}
#endif

void
mobile_post_media_command(const QString& command, qint64 arg)
{
  QCoreApplication* app = QCoreApplication::instance();
  if (!app)
    return;
  QMetaObject::invokeMethod(app, [command, arg] {
    if (MobileShell* shell = MobileShell::instance())
      emit shell->mediaCommand(command, arg);
  }, Qt::QueuedConnection);
}

void
mobile_bring_to_front()
{
#if defined(Q_OS_ANDROID)
  QJniObject::callStaticMethod<void>(
    BRIDGE, "bringToFront", "(Landroid/content/Context;)V", context_object().object());
  QJniEnvironment env;
  env.checkAndClearExceptions();
#endif
}

bool
is_android()
{
#if defined(Q_OS_ANDROID)
  return true;
#else
  return false;
#endif
}

QString
android_prepare_assets(const QString& version)
{
#if defined(Q_OS_ANDROID)
  register_natives();
  qInstallMessageHandler(log_to_stderr);
  const QString base = QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
  const QString root = base + QStringLiteral("/bundle-") + version;
  const QString stamp = root + QStringLiteral("/.complete");
  // В метке — версия и метка сборки APK (build-id из build_apk.sh): переустановка той же версии (сборка ветки,
  // отладка) тоже распаковывает новый QML, а не остаётся на прежнем
  QString build = version;
  {
    QFile id(QStringLiteral("assets:/materialfy/build-id"));
    if (id.open(QIODevice::ReadOnly))
      build += QLatin1Char('+') + QString::fromUtf8(id.readAll()).trimmed();
  }
  bool fresh = false;
  {
    QFile marker(stamp);
    fresh = marker.open(QIODevice::ReadOnly) && QString::fromUtf8(marker.readAll()) == build;
  }
  if (!fresh) {
    // старые версии — прочь
    QDir baseDir(base);
    for (const QString& old : baseDir.entryList({ QStringLiteral("bundle-*") }, QDir::Dirs | QDir::NoDotAndDotDot))
      QDir(base + QLatin1Char('/') + old).removeRecursively();
    bool ok = copy_tree(QStringLiteral("assets:/materialfy/qml"), root + QStringLiteral("/qml"));
    ok = copy_tree(QStringLiteral("assets:/materialfy/assets"), root + QStringLiteral("/assets")) && ok;
    if (!ok)
      return {};
    QFile marker(stamp);
    if (marker.open(QIODevice::WriteOnly))
      marker.write(build.toUtf8());
  }
  return root + QStringLiteral("/qml");
#else
  Q_UNUSED(version)
  return {};
#endif
}

std::size_t
android_java_vm()
{
#if defined(Q_OS_ANDROID)
  return reinterpret_cast<std::size_t>(QJniEnvironment::javaVM());
#else
  return 0;
#endif
}

std::size_t
android_context()
{
#if defined(Q_OS_ANDROID)
  static jobject global = nullptr;
  if (!global) {
    QJniEnvironment env;
    global = env->NewGlobalRef(context_object().object());
  }
  return reinterpret_cast<std::size_t>(global);
#else
  return 0;
#endif
}

QString
android_install_apk(const QString& path)
{
#if defined(Q_OS_ANDROID)
  // поток Rust: QJniEnvironment сам прикрепляет его к JVM
  QJniObject result = QJniObject::callStaticObjectMethod("org/materialfy/ApkInstaller",
                                                         "install",
                                                         "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                                                         context_object().object(),
                                                         jstr(path).object<jstring>());
  QJniEnvironment env;
  if (env.checkAndClearExceptions())
    return QStringLiteral("installer exception");
  return result.isValid() ? result.toString() : QStringLiteral("installer unavailable");
#else
  Q_UNUSED(path)
  return QStringLiteral("not android");
#endif
}

void
open_url_external(const QString& url)
{
  QCoreApplication* app = QCoreApplication::instance();
  if (!app)
    return;
  QMetaObject::invokeMethod(app, [url] { QDesktopServices::openUrl(QUrl(url)); }, Qt::QueuedConnection);
}

} // namespace mf
