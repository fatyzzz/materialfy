// Регистрация типов QML-модуля Materialfy.Core.
// Владелец — «bridges-player». Синглтоны создаются здесь, ДО движка, и живут до конца процесса.
// Имена типов — контракт с QML.
#include "register.h"

#include "mobile.h"

#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>

#include "materialfy/src/bridges/auth.cxxqt.h"
#include "materialfy/src/bridges/catalog.cxxqt.h"
#include "materialfy/src/bridges/covers.cxxqt.h"
#include "materialfy/src/bridges/crash_report.cxxqt.h"
#include "materialfy/src/bridges/data.cxxqt.h"
#include "materialfy/src/bridges/devices.cxxqt.h"
#include "materialfy/src/bridges/home.cxxqt.h"
#include "materialfy/src/bridges/library.cxxqt.h"
#include "materialfy/src/bridges/lyrics.cxxqt.h"
#include "materialfy/src/bridges/player.cxxqt.h"
#include "materialfy/src/bridges/radio.cxxqt.h"
#include "materialfy/src/bridges/releases.cxxqt.h"
#include "materialfy/src/bridges/settings.cxxqt.h"
#include "materialfy/src/bridges/social.cxxqt.h"
#include "materialfy/src/bridges/theme.cxxqt.h"
#include "materialfy/src/bridges/translations.cxxqt.h"
#include "materialfy/src/bridges/updater.cxxqt.h"
#include "materialfy/src/icons.cxxqt.h"
#include "materialfy/src/models/list_model.cxxqt.h"
#include "materialfy/src/models/track_filter.cxxqt.h"
#include "materialfy/src/winframe.cxxqt.h"

namespace mf {

namespace {
constexpr const char* URI = "Materialfy.Core";

template<typename T>
void
singleton(const char* name)
{
  auto* object = new T();
  QQmlEngine::setObjectOwnership(object, QQmlEngine::CppOwnership);
  qmlRegisterSingletonInstance<T>(URI, 1, 0, name, object);
}
} // namespace

void
register_qml_types()
{
  // Порядок важен: Settings и ThemeEngine первыми (их читают остальные при создании)
  singleton<Settings>("Settings");
  singleton<Translations>("Translations");
  singleton<ThemeEngine>("ThemeEngine");
  singleton<IconMetrics>("IconMetrics");
  singleton<Auth>("Auth");
  singleton<Library>("Library");
  singleton<Player>("Player");
  singleton<Devices>("Devices");
  singleton<Radio>("Radio");
  singleton<Home>("Home");
  singleton<Releases>("Releases");
  singleton<Social>("Social");
  singleton<Catalog>("Catalog");
  singleton<Covers>("Covers");
  singleton<CrashReport>("CrashReport");
  singleton<Lyrics>("Lyrics");
  singleton<Updater>("Updater");
  singleton<WindowShell>("WindowShell");
  singleton<MobileShell>("MobileShell");

  qmlRegisterType<TrackFilter>(URI, 1, 0, "TrackFilter");
  qmlRegisterUncreatableType<DictListModel>(URI, 1, 0, "DictListModel", QStringLiteral("the model is created in Rust"));
  qmlRegisterUncreatableType<AlbumData>(URI, 1, 0, "AlbumData", QStringLiteral("Catalog.album(id)"));
  qmlRegisterUncreatableType<ArtistData>(URI, 1, 0, "ArtistData", QStringLiteral("Catalog.artist(id)"));
  qmlRegisterUncreatableType<ShowData>(URI, 1, 0, "ShowData", QStringLiteral("Catalog.show(id)"));
  qmlRegisterUncreatableType<EpisodeData>(URI, 1, 0, "EpisodeData", QStringLiteral("Catalog.episode(id)"));
}

} // namespace mf
