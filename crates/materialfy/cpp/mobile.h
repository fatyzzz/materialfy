// Мобильная оболочка: Android-интеграция (MediaSession и уведомление плеера, системные панели, кнопка «назад»,
// ассеты APK) и синглтон MobileShell для QML. На десктопе — заглушки, чтобы мобильную раскладку можно было
// запускать и снимать (--mobile).
#pragma once

#include <QtCore/QObject>
#include <QtCore/QString>

#include <cstddef>
#include <cstdint>

namespace mf {

class MobileShell : public QObject
{
  Q_OBJECT
  // true — сборка под Android (раскладка телефона всегда, без окон и трея)
  Q_PROPERTY(bool android READ android CONSTANT)
  Q_PROPERTY(qreal fontScale READ fontScale NOTIFY fontScaleChanged)
  // Плеер в «острове» системы (Live Updates Android 16, остров vivo): есть ли где показать и включён ли
  Q_PROPERTY(bool islandSupported READ islandSupported CONSTANT)
  Q_PROPERTY(bool islandEnabled READ islandEnabled WRITE setIslandEnabled NOTIFY islandEnabledChanged)

public:
  explicit MobileShell(QObject* parent = nullptr);
  static MobileShell* instance();

  bool android() const;
  qreal fontScale() const;
  Q_INVOKABLE qreal textSize(qreal sp) const;
  Q_INVOKABLE void prepareAccessibility(QObject* item, QObject* qmlOwner);
  bool islandSupported() const;
  bool islandEnabled() const;
  void setIslandEnabled(bool on);

  // Состояние плеера → MediaSession и уведомление (Android). hasTrack=false — убрать уведомление;
  // local=false — играет другое устройство (Connect): без аудиофокуса и блокировок сна.
  Q_INVOKABLE void updateMedia(const QString& title,
                               const QString& artist,
                               const QString& album,
                               const QString& coverUrl,
                               qint64 durationMs,
                               qint64 positionMs,
                               bool playing,
                               bool hasTrack,
                               bool canPrevious,
                               bool canNext,
                               bool local,
                               bool clockRunning);
  // Сердце в карточке плеера системы (своя кнопка медиасессии): текущий трек в любимых
  Q_INVOKABLE void setMediaLiked(bool liked);
  // Вход ждёт ответа браузера: процесс не должен замёрзнуть в фоне (Android: SignInService)
  Q_INVOKABLE void setBrowserWait(bool waiting);
  // Иконки статус-бара и навигации: lightBars=true — тёмные значки на светлом фоне
  Q_INVOKABLE void setSystemBars(bool lightBars);
  // «Назад» на корне: свернуть приложение (музыка играет дальше), а не выходить
  Q_INVOKABLE void moveTaskToBack();
  // Короткий тактильный отклик (долгое нажатие, переключатели)
  Q_INVOKABLE void haptic();
  // Конец журнала приложения (MATERIALFY_LOG_FILE) — в системное окно «Поделиться» (Telegram, почта, буфер)
  Q_INVOKABLE void shareLog();

signals:
  void fontScaleChanged();
  void islandEnabledChanged();
  // Команды из уведомления, гарнитуры и экрана блокировки: "play", "pause", "toggle", "next", "previous",
  // "seek" (arg — мс), "stop"
  void mediaCommand(const QString& command, qint64 arg);

private:
  bool m_lightBars = false;
  bool m_browserWait = false;
  bool m_lightBarsSet = false;
};

// Android: вызывается из JNI (любой поток) — команда уходит в GUI-поток сигналом mediaCommand
void mobile_post_media_command(const QString& command, qint64 arg);

// Android: вернуть активность на передний план (после входа в браузере)
void mobile_bring_to_front();

// ---- для Rust (qt/ffi.rs) ----

// true — сборка под Android
bool is_android();
// Android: после QApplication — распаковать assets:/materialfy (qml/ и assets/) в <files>/materialfy-<version>
// (один раз на версию) и вернуть путь к каталогу qml. Пустая строка — не Android или ошибка.
QString android_prepare_assets(const QString& version);
// Android: JavaVM* и глобальная ссылка на Context — для ndk-context (cpal/AAudio). 0 — не Android.
std::size_t android_java_vm();
std::size_t android_context();
// Android: передать APK системному установщику. "" — окно установки показано, "permission" — нужно
// разрешение «Установка неизвестных приложений» (его настройки уже открыты), иначе — текст ошибки
QString android_install_apk(const QString& path);
// Открыть ссылку во внешнем браузере из любого потока (через GUI-поток: QDesktopServices)
void open_url_external(const QString& url);

} // namespace mf
