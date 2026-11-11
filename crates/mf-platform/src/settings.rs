//! Настройки приложения: схема ключей (имена, типы, значения по умолчанию).
//!
//! Хранилище в формате QSettings (тот же, что пишет QSettings оболочки):
//! - Linux/macOS, а также при --config-dir на любой ОС: INI-файл (`[General]`, `ключ=значение`;
//!   bool — "true"/"false", числа — текстом, строки с запятой/спецсимволами — в кавычках, как пишет QSettings);
//! - Windows без --config-dir: реестр HKCU\Software\materialfy\Materialfy (строки, bool и float — REG_SZ,
//!   int — REG_DWORD; при чтении REG_SZ числа тоже принимаются).
//!
//! Чтение как `_read` в Python: нет значения или не приводится к типу — значение по умолчанию.
//! Запись как `_write`: если новое значение равно текущему — ничего не происходит и событие не шлётся.
//! Все методы потокобезопасны; запись на диск — сразу (синхронно, файл маленький).
//!
//! `get(WindowButtons)` всегда true: кнопки окна приложение рисует на всех ОС, настройки больше нет. Старое
//! значение в конфиге читается и сохраняется, но не влияет; сравнение при записи — с `get_stored`.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

/// windowX/windowY: положение ещё не сохранялось
pub const WINDOW_POS_UNSET: i64 = -1_000_000;

/// Все ключи (Spotify-версия).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingKey {
    // Внешний вид
    ThemeMode,
    SchemeVariant,
    AccentFromCover,
    CustomSeed,
    WindowButtons,
    TrackTransition,
    BackdropMode,
    NowPlayingBlur,
    NowPlayingDrift,
    AmbientBackground,
    CoverSpin,
    CoverPulse,
    CalmWhenInactive,
    /// текст песни: "normal" (классика) | "spicy" (крупная строка со сглаженным градиентом) | "karaoke" (текущая строка крупнее)
    LyricsMode,
    /// множитель размера шрифта текста (базовый размер задаёт место вызова)
    LyricsFontScale,
    /// затемняющая подложка за текстом: "none" | "soft" | "strong"
    LyricsScrim,
    /// выравнивание текста песни: "left" | "center" | "right"
    LyricsTextAlign,
    /// режим заполнения: "word" | "line" | "off"
    LyricsFillMode,
    /// фильтр видимых строк: "all" | "three" | "one" | "1"…"15"
    LyricsVisibleLines,
    /// множитель градиента и свечения (0..2)
    LyricsHighlight,
    /// масштаб активной строки (0.0 — по стилю, либо 1.0…1.35)
    LyricsActiveScale,
    /// масштаб неактивных строк (0.0 — по стилю, либо 0.8…1.0)
    LyricsIdleScale,
    /// амплитуда волны слов (0.0…2.0)
    LyricsWordBounce,
    /// непрозрачность неактивных строк (0.15…0.75)
    LyricsDimOpacity,
    /// длительность скролла строки в мс (-1 — по стилю, либо 200…800)
    LyricsScrollMs,
    /// задержка каскада соседних строк в мс (-1 — по стилю, либо 0…80)
    LyricsStaggerMs,
    /// степень размытия неактивных строк (0..1)
    LyricsBlurAmount,
    /// множитель межстрочного интервала (0.8..1.6)
    LyricsLineSpacing,
    /// анимации прокрутки и переходов текста
    LyricsAnimate,
    /// показывать метку источника текста
    LyricsShowSource,
    /// сдвиг синхронизации в миллисекундах (-500..500)
    LyricsSyncMs,
    /// "system" | "full" | "reduced" → Theme.reducedMotion
    Animations,
    /// режим для слабых ПК: "auto" (сам, когда кадры рисует процессор) | "on" | "off" → Theme.lowPower
    LowPowerMode,
    /// выбор графического процессора для рендеринга: "auto" | "integrated" | "discrete"
    GpuPreference,
    /// язык интерфейса: "system" (как в ОС) | "en" | "ru" | "pt" (mf_i18n::parse_setting); меняется на лету
    Language,
    /// масштаб интерфейса в процентах: 75 | 100 | 125 | 150 | 200 (QT_SCALE_FACTOR, после перезапуска)
    UiScale,
    // Раскладка
    RightPanelOpen,
    RightPanelTab,
    SidebarCollapsed,
    WindowWidth,
    WindowHeight,
    WindowX,
    WindowY,
    WindowMaximized,
    /// "grid" | "list"
    LibraryView,
    /// "recent" | "added" | "alpha" | "author" | "spotify"
    LibrarySort,
    /// скрытые полки главной через запятую (quick,recent,madeForYou,podcasts,topArtists,topTracks)
    HomeHidden,
    /// сцена «Радио» на главной
    HomeRadioHidden,
    /// в боковой панели вместо списка плейлистов — последние добавленные/обновлённые
    SidebarLibrary,
    // Воспроизведение (librespot)
    /// 96 | 160 | 320 кбит/с (смена — перезапуск движка)
    Bitrate,
    Normalisation,
    Gapless,
    Autoplay,
    /// 0..1
    Volume,
    /// имя устройства вывода; "" — по умолчанию системы
    AudioDevice,
    AudioCache,
    /// лимит аудиокэша librespot, МБ (256…10240)
    AudioCacheMb,
    /// пользователь включил воспроизведение на этом ПК (грант playback выдан)
    PlaybackAuthorized,
    /// скорость воспроизведения 0.5…2.0 (шаг 0.05), без смены высоты тона; меняется на лету
    PlaybackSpeed,
    /// кроссфейд между треками, секунды 0…12 (0 — выкл.); меняется на лету
    Crossfade,
    /// the DJ's spoken lines between its tracks; changes on the fly
    DjVoice,
    // Spotify Connect и сеть
    /// имя Connect-устройства; "" — «Materialfy · <имя ПК>»
    DeviceName,
    /// свой Client ID (личное приложение Web API); "" — нет
    WebClientId,
    /// окно с инструкцией, когда общий Client ID упёрся в лимит: true — «больше не показывать»
    ApiLimitTipDismissed,
    /// "off" | "system" | "http" | "socks"; пароль — в хранилище секретов (sf-auth, слот Proxy)
    ProxyMode,
    ProxyHost,
    ProxyPort,
    ProxyUsername,
    /// обложки и картинки грузить напрямую, мимо прокси (по умолчанию — через него, как всё остальное)
    CoversBypassProxy,
    // Локальные данные
    /// закреплённые URI (плейлисты/альбомы/станции), JSON-массив строк — get_string_list/set_string_list
    Pins,
    /// локальный «не рекомендовать»: URI треков, JSON-массив строк (фильтр радио)
    HiddenTracks,
    /// группа радио «Любой трек» (сиды, выбранные через поиск): JSON-массив RadioSeed
    RadioSeeds,
    // Поведение
    CloseToTray,
    WheelStep,
    /// плеер поверх всего вне главного окна: "off" | "island" (островок у верхнего края экрана) | "taskbar" (в панели задач
    /// Windows) | "both" (и то и другое, Windows)
    FloatingPlayer,
    /// вид островка и виджета: "black" (чёрный, как железо) | "color" (тёмный тон обложки со свечением)
    FloatingPlayerStyle,
    /// место виджета в панели задач: "tray" (у трея) | "left" | "appsLeft" | "appsRight" | доля 0..1 свободной ширины ("0.42")
    TaskbarWidgetPos,
    /// сколько раз под раскрытым островком показана подсказка с клавишами (до 3)
    FloatingKeyHints,
    FloatingHideFullscreen,
    FloatingHideMaximized,
    FloatingOffsetX,
    FloatingEdge,
    FloatingFreeX,
    FloatingFreeY,
    /// мониторы для островка: "" / "0" (основной) | "0,1" | "all"
    FloatingIslandScreens,
    /// мониторы для панели задач: "" / "0" (основной) | "0,1" | "all"
    TaskbarScreens,
    /// позиции на панели задач для каждого монитора: JSON {"0":"tray","1":"appsRight"}
    TaskbarPositions,
    // Updates
    /// check GitHub Releases for a new version on startup and every 6 h (no network when off)
    AutoUpdateCheck,
    /// "stable" | "beta" (beta also offers pre-releases)
    UpdateChannel,
    /// version the user chose to skip ("0.2.0"); "" — none
    SkippedVersion,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingType {
    Str,
    Bool,
    Int,
    Float,
}

impl SettingKey {
    pub const ALL: [SettingKey; 90] = [
        SettingKey::ThemeMode,
        SettingKey::SchemeVariant,
        SettingKey::AccentFromCover,
        SettingKey::CustomSeed,
        SettingKey::WindowButtons,
        SettingKey::TrackTransition,
        SettingKey::BackdropMode,
        SettingKey::NowPlayingBlur,
        SettingKey::NowPlayingDrift,
        SettingKey::AmbientBackground,
        SettingKey::CoverSpin,
        SettingKey::CoverPulse,
        SettingKey::CalmWhenInactive,
        SettingKey::LyricsMode,
        SettingKey::LyricsFontScale,
        SettingKey::LyricsScrim,
        SettingKey::LyricsTextAlign,
        SettingKey::LyricsFillMode,
        SettingKey::LyricsVisibleLines,
        SettingKey::LyricsHighlight,
        SettingKey::LyricsActiveScale,
        SettingKey::LyricsIdleScale,
        SettingKey::LyricsWordBounce,
        SettingKey::LyricsDimOpacity,
        SettingKey::LyricsScrollMs,
        SettingKey::LyricsStaggerMs,
        SettingKey::LyricsBlurAmount,
        SettingKey::LyricsLineSpacing,
        SettingKey::LyricsAnimate,
        SettingKey::LyricsShowSource,
        SettingKey::LyricsSyncMs,
        SettingKey::Animations,
        SettingKey::LowPowerMode,
        SettingKey::GpuPreference,
        SettingKey::Language,
        SettingKey::UiScale,
        SettingKey::RightPanelOpen,
        SettingKey::RightPanelTab,
        SettingKey::SidebarCollapsed,
        SettingKey::WindowWidth,
        SettingKey::WindowHeight,
        SettingKey::WindowX,
        SettingKey::WindowY,
        SettingKey::WindowMaximized,
        SettingKey::LibraryView,
        SettingKey::LibrarySort,
        SettingKey::HomeHidden,
        SettingKey::HomeRadioHidden,
        SettingKey::SidebarLibrary,
        SettingKey::Bitrate,
        SettingKey::Normalisation,
        SettingKey::Gapless,
        SettingKey::Autoplay,
        SettingKey::Volume,
        SettingKey::AudioDevice,
        SettingKey::AudioCache,
        SettingKey::AudioCacheMb,
        SettingKey::PlaybackAuthorized,
        SettingKey::PlaybackSpeed,
        SettingKey::Crossfade,
        SettingKey::DjVoice,
        SettingKey::DeviceName,
        SettingKey::WebClientId,
        SettingKey::ApiLimitTipDismissed,
        SettingKey::ProxyMode,
        SettingKey::ProxyHost,
        SettingKey::ProxyPort,
        SettingKey::ProxyUsername,
        SettingKey::CoversBypassProxy,
        SettingKey::Pins,
        SettingKey::HiddenTracks,
        SettingKey::RadioSeeds,
        SettingKey::CloseToTray,
        SettingKey::WheelStep,
        SettingKey::FloatingPlayer,
        SettingKey::FloatingPlayerStyle,
        SettingKey::TaskbarWidgetPos,
        SettingKey::FloatingKeyHints,
        SettingKey::FloatingHideFullscreen,
        SettingKey::FloatingHideMaximized,
        SettingKey::FloatingOffsetX,
        SettingKey::FloatingEdge,
        SettingKey::FloatingFreeX,
        SettingKey::FloatingFreeY,
        SettingKey::FloatingIslandScreens,
        SettingKey::TaskbarScreens,
        SettingKey::TaskbarPositions,
        SettingKey::AutoUpdateCheck,
        SettingKey::UpdateChannel,
        SettingKey::SkippedVersion,
    ];

    /// Имя ключа в хранилище и в QML ("themeMode", …).
    pub fn name(self) -> &'static str {
        use SettingKey::*;
        match self {
            ThemeMode => "themeMode",
            SchemeVariant => "schemeVariant",
            AccentFromCover => "accentFromCover",
            CustomSeed => "customSeed",
            WindowButtons => "windowButtons",
            TrackTransition => "trackTransition",
            BackdropMode => "backdropMode",
            NowPlayingBlur => "nowPlayingBlur",
            NowPlayingDrift => "nowPlayingDrift",
            AmbientBackground => "ambientBackground",
            CoverSpin => "coverSpin",
            CoverPulse => "coverPulse",
            CalmWhenInactive => "calmWhenInactive",
            LyricsMode => "lyricsMode",
            LyricsFontScale => "lyricsFontScale",
            LyricsScrim => "lyricsScrim",
            LyricsTextAlign => "lyricsTextAlign",
            LyricsFillMode => "lyricsFillMode",
            LyricsVisibleLines => "lyricsVisibleLines",
            LyricsHighlight => "lyricsHighlight",
            LyricsActiveScale => "lyricsActiveScale",
            LyricsIdleScale => "lyricsIdleScale",
            LyricsWordBounce => "lyricsWordBounce",
            LyricsDimOpacity => "lyricsDimOpacity",
            LyricsScrollMs => "lyricsScrollMs",
            LyricsStaggerMs => "lyricsStaggerMs",
            LyricsBlurAmount => "lyricsBlurAmount",
            LyricsLineSpacing => "lyricsLineSpacing",
            LyricsAnimate => "lyricsAnimate",
            LyricsShowSource => "lyricsShowSource",
            LyricsSyncMs => "lyricsSyncMs",
            Animations => "animations",
            LowPowerMode => "lowPowerMode",
            GpuPreference => "gpuPreference",
            Language => "language",
            UiScale => "uiScale",
            RightPanelOpen => "rightPanelOpen",
            RightPanelTab => "rightPanelTab",
            SidebarCollapsed => "sidebarCollapsed",
            WindowWidth => "windowWidth",
            WindowHeight => "windowHeight",
            WindowX => "windowX",
            WindowY => "windowY",
            WindowMaximized => "windowMaximized",
            LibraryView => "libraryView",
            LibrarySort => "librarySort",
            HomeHidden => "homeHidden",
            HomeRadioHidden => "homeRadioHidden",
            SidebarLibrary => "sidebarLibrary",
            Bitrate => "bitrate",
            Normalisation => "normalisation",
            Gapless => "gapless",
            Autoplay => "autoplay",
            Volume => "volume",
            AudioDevice => "audioDevice",
            AudioCache => "audioCache",
            AudioCacheMb => "audioCacheMb",
            PlaybackAuthorized => "playbackAuthorized",
            PlaybackSpeed => "playbackSpeed",
            Crossfade => "crossfade",
            DjVoice => "djVoice",
            DeviceName => "deviceName",
            WebClientId => "webClientId",
            ApiLimitTipDismissed => "apiLimitTipDismissed",
            ProxyMode => "proxyMode",
            ProxyHost => "proxyHost",
            ProxyPort => "proxyPort",
            ProxyUsername => "proxyUsername",
            CoversBypassProxy => "coversBypassProxy",
            Pins => "pins",
            HiddenTracks => "hiddenTracks",
            RadioSeeds => "radioSeeds",
            CloseToTray => "closeToTray",
            WheelStep => "wheelStep",
            FloatingPlayer => "floatingPlayer",
            FloatingPlayerStyle => "floatingPlayerStyle",
            TaskbarWidgetPos => "taskbarWidgetPos",
            FloatingKeyHints => "floatingKeyHints",
            FloatingHideFullscreen => "floatingHideFullscreen",
            FloatingHideMaximized => "floatingHideMaximized",
            FloatingOffsetX => "floatingOffsetX",
            FloatingEdge => "floatingEdge",
            FloatingFreeX => "floatingFreeX",
            FloatingFreeY => "floatingFreeY",
            FloatingIslandScreens => "floatingIslandScreens",
            TaskbarScreens => "taskbarScreens",
            TaskbarPositions => "taskbarPositions",
            AutoUpdateCheck => "autoUpdateCheck",
            UpdateChannel => "updateChannel",
            SkippedVersion => "skippedVersion",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }

    pub fn value_type(self) -> SettingType {
        match self.default_value() {
            SettingValue::Str(_) => SettingType::Str,
            SettingValue::Bool(_) => SettingType::Bool,
            SettingValue::Int(_) => SettingType::Int,
            SettingValue::Float(_) => SettingType::Float,
        }
    }

    pub fn default_value(self) -> SettingValue {
        use SettingKey::*;
        use SettingValue::{Bool, Float, Int, Str};
        match self {
            ThemeMode => Str("dark".into()),
            SchemeVariant => Str("content".into()),
            AccentFromCover => Bool(true),
            CustomSeed => Str("#8C6DFF".into()), // Ultraviolet, фирменный акцент Materialfy
            WindowButtons => Bool(false),
            TrackTransition => Str("wipe".into()),
            BackdropMode => Str("gauss".into()),
            NowPlayingBlur => Float(1.0),
            NowPlayingDrift => Float(0.5),
            AmbientBackground => Bool(true),
            CoverSpin => Bool(true),
            CoverPulse => Bool(true),
            CalmWhenInactive => Bool(true),
            LyricsMode => Str("normal".into()),
            LyricsFontScale => Float(1.0),
            LyricsScrim => Str("none".into()),
            LyricsTextAlign => Str("left".into()),
            LyricsFillMode => Str("word".into()),
            LyricsVisibleLines => Str("all".into()),
            LyricsHighlight => Float(1.0),
            LyricsActiveScale => Float(0.0),
            LyricsIdleScale => Float(0.0),
            LyricsWordBounce => Float(1.0),
            LyricsDimOpacity => Float(0.35),
            LyricsScrollMs => Int(-1),
            LyricsStaggerMs => Int(-1),
            LyricsBlurAmount => Float(1.0),
            LyricsLineSpacing => Float(1.0),
            LyricsAnimate => Bool(true),
            LyricsShowSource => Bool(true),
            LyricsSyncMs => Int(0),
            Animations => Str("system".into()),
            LowPowerMode => Str("auto".into()),
            GpuPreference => Str("auto".into()),
            Language => Str("system".into()),
            UiScale => Int(100),
            RightPanelOpen => Bool(false),
            RightPanelTab => Str("queue".into()),
            SidebarCollapsed => Bool(false),
            WindowWidth => Int(1280),
            WindowHeight => Int(800),
            WindowX => Int(WINDOW_POS_UNSET),
            WindowY => Int(WINDOW_POS_UNSET),
            WindowMaximized => Bool(false),
            LibraryView => Str("grid".into()),
            LibrarySort => Str("recent".into()),
            HomeHidden => Str(String::new()),
            HomeRadioHidden => Bool(false),
            SidebarLibrary => Bool(false),
            Bitrate => Int(320),
            Normalisation => Bool(true),
            Gapless => Bool(true),
            Autoplay => Bool(true),
            Volume => Float(0.7),
            AudioDevice => Str(String::new()),
            AudioCache => Bool(true),
            AudioCacheMb => Int(2048),
            PlaybackAuthorized => Bool(false),
            PlaybackSpeed => Float(1.0),
            Crossfade => Int(0),
            DjVoice => Bool(true),
            DeviceName => Str(String::new()),
            WebClientId => Str(String::new()),
            ApiLimitTipDismissed => Bool(false),
            ProxyMode => Str("system".into()),
            ProxyHost => Str(String::new()),
            ProxyPort => Str(String::new()),
            ProxyUsername => Str(String::new()),
            CoversBypassProxy => Bool(false),
            Pins => Str(String::new()),
            HiddenTracks => Str(String::new()),
            RadioSeeds => Str(String::new()),
            CloseToTray => Bool(true),
            WheelStep => Int(150),
            FloatingPlayer => Str("off".into()),
            FloatingPlayerStyle => Str("black".into()),
            TaskbarWidgetPos => Str("tray".into()),
            FloatingKeyHints => Int(0),
            FloatingHideFullscreen => Bool(true),
            FloatingHideMaximized => Bool(false),
            FloatingOffsetX => Int(0),
            FloatingEdge => Str("top".into()),
            FloatingFreeX => Int(WINDOW_POS_UNSET),
            FloatingFreeY => Int(WINDOW_POS_UNSET),
            FloatingIslandScreens => Str(String::new()),
            TaskbarScreens => Str(String::new()),
            TaskbarPositions => Str(String::new()),
            AutoUpdateCheck => Bool(true),
            UpdateChannel => Str("stable".into()),
            SkippedVersion => Str(String::new()),
        }
    }

    /// Допустимое значение ключа (значение уже приведено к типу ключа): bitrate — ближайшее из 96/160/320,
    /// audioCacheMb — 256…10240 с шагом 256. Остальные ключи — как есть.
    pub fn normalize(self, value: SettingValue) -> SettingValue {
        match (self, value) {
            (SettingKey::Bitrate, SettingValue::Int(kbps)) => SettingValue::Int(match kbps {
                ..=128 => 96,
                129..=240 => 160,
                _ => 320,
            }),
            (SettingKey::AudioCacheMb, SettingValue::Int(mb)) => {
                let mb = mb.clamp(AUDIO_CACHE_MB_MIN, AUDIO_CACHE_MB_MAX);
                let steps = (mb + AUDIO_CACHE_MB_STEP / 2) / AUDIO_CACHE_MB_STEP;
                SettingValue::Int((steps * AUDIO_CACHE_MB_STEP).clamp(AUDIO_CACHE_MB_MIN, AUDIO_CACHE_MB_MAX))
            }
            (SettingKey::PlaybackSpeed, SettingValue::Float(speed)) => {
                let speed = if speed.is_finite() { speed } else { 1.0 };
                SettingValue::Float(((speed.clamp(0.5, 2.0) * 20.0).round() / 20.0).clamp(0.5, 2.0))
            }
            (SettingKey::Crossfade, SettingValue::Int(seconds)) => SettingValue::Int(seconds.clamp(0, CROSSFADE_MAX_S)),
            (SettingKey::LyricsFontScale, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.7, 1.6) } else { 1.0 })
            }
            (SettingKey::LyricsHighlight, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 })
            }
            (SettingKey::LyricsActiveScale, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() && v >= 1.0 { v.clamp(1.0, 1.35) } else { 0.0 })
            }
            (SettingKey::LyricsIdleScale, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() && v > 0.0 { v.clamp(0.8, 1.0) } else { 0.0 })
            }
            (SettingKey::LyricsWordBounce, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 })
            }
            (SettingKey::LyricsDimOpacity, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.15, 0.75) } else { 0.35 })
            }
            (SettingKey::LyricsScrollMs, SettingValue::Int(ms)) => {
                SettingValue::Int(if ms == -1 { -1 } else { ms.clamp(200, 800) })
            }
            (SettingKey::LyricsStaggerMs, SettingValue::Int(ms)) => {
                SettingValue::Int(if ms == -1 { -1 } else { ms.clamp(0, 80) })
            }
            (SettingKey::LyricsBlurAmount, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 })
            }
            (SettingKey::LyricsLineSpacing, SettingValue::Float(v)) => {
                SettingValue::Float(if v.is_finite() { v.clamp(0.8, 1.6) } else { 1.0 })
            }
            (SettingKey::LyricsSyncMs, SettingValue::Int(ms)) => {
                SettingValue::Int(ms.clamp(-500, 500))
            }
            (SettingKey::UiScale, SettingValue::Int(percent)) => {
                // пресеты из настроек; значение из конфига вне пресетов — ближайший
                let percent = percent.clamp(50, 250);
                SettingValue::Int(
                    [75i64, 100, 125, 150, 200]
                        .into_iter()
                        .min_by_key(|preset| (preset - percent).abs())
                        .unwrap_or(100),
                )
            }
            (SettingKey::LyricsTextAlign, SettingValue::Str(s)) => {
                SettingValue::Str(match s.as_str() {
                    "center" => "center",
                    "right" => "right",
                    _ => "left",
                }.into())
            }
            (SettingKey::LyricsFillMode, SettingValue::Str(s)) => {
                SettingValue::Str(match s.as_str() {
                    "line" => "line",
                    "off" => "off",
                    _ => "word",
                }.into())
            }
            (SettingKey::LyricsVisibleLines, SettingValue::Str(s)) => {
                SettingValue::Str(match s.as_str() {
                    "three" => "three".into(),
                    "one" => "one".into(),
                    "all" => "all".into(),
                    other => match other.trim().parse::<u32>() {
                        Ok(n) if (1..=15).contains(&n) => other.trim().into(),
                        _ => "all".into(),
                    },
                })
            }
            (SettingKey::FloatingPlayer, SettingValue::Str(s)) => {
                SettingValue::Str(match s.as_str() {
                    "island" => "island",
                    "taskbar" => "taskbar",
                    "both" => "both",
                    _ => "off",
                }.into())
            }
            (SettingKey::FloatingEdge, SettingValue::Str(s)) => SettingValue::Str(
                match s.as_str() {
                    "bottom" => "bottom",
                    "left" => "left",
                    "right" => "right",
                    "free" => "free",
                    _ => "top",
                }
                .into(),
            ),
            (SettingKey::FloatingPlayerStyle, SettingValue::Str(s)) => {
                SettingValue::Str(if s == "color" { "color" } else { "black" }.into())
            }
            (SettingKey::TaskbarWidgetPos, SettingValue::Str(s)) => SettingValue::Str(
                if matches!(s.as_str(), "left" | "appsLeft" | "appsRight" | "tray") {
                    s
                } else {
                    match s.trim().parse::<f64>() {
                        Ok(f) if f.is_finite() => format!("{:.4}", f.clamp(0.0, 1.0)),
                        _ => "tray".into(),
                    }
                },
            ),
            (SettingKey::FloatingKeyHints, SettingValue::Int(n)) => SettingValue::Int(n.clamp(0, 3)),
            (SettingKey::GpuPreference, SettingValue::Str(s)) => {
                SettingValue::Str(match s.as_str() {
                    "integrated" => "integrated",
                    "discrete" => "discrete",
                    _ => "auto",
                }.into())
            }
            (SettingKey::UpdateChannel, SettingValue::Str(channel)) => {
                SettingValue::Str(if channel.trim().eq_ignore_ascii_case("beta") { "beta" } else { "stable" }.into())
            }
            (_, value) => value,
        }
    }
}

/// Самый длинный кроссфейд, секунды.
pub const CROSSFADE_MAX_S: i64 = 12;

/// Битрейты librespot, кбит/с.
pub const BITRATES: [i64; 3] = [96, 160, 320];
/// Лимит аудиокэша, МБ.
pub const AUDIO_CACHE_MB_MIN: i64 = 256;
pub const AUDIO_CACHE_MB_MAX: i64 = 10240;
pub const AUDIO_CACHE_MB_STEP: i64 = 256;

impl SettingValue {
    pub fn value_type(&self) -> SettingType {
        match self {
            SettingValue::Str(_) => SettingType::Str,
            SettingValue::Bool(_) => SettingType::Bool,
            SettingValue::Int(_) => SettingType::Int,
            SettingValue::Float(_) => SettingType::Float,
        }
    }

    /// Привести к типу, как `typ(value)` в Python (строковый bool из INI — `value.lower() == "true"`).
    /// None — не приводится.
    pub fn coerce(&self, to: SettingType) -> Option<SettingValue> {
        use SettingValue::*;
        Some(match (to, self) {
            (SettingType::Str, Str(s)) => Str(s.clone()),
            (SettingType::Str, Bool(b)) => Str(if *b { "True" } else { "False" }.into()),
            (SettingType::Str, Int(i)) => Str(i.to_string()),
            (SettingType::Str, Float(f)) => Str(py_float_str(*f)),
            (SettingType::Bool, Str(s)) => Bool(s.eq_ignore_ascii_case("true")),
            (SettingType::Bool, Bool(b)) => Bool(*b),
            (SettingType::Bool, Int(i)) => Bool(*i != 0),
            (SettingType::Bool, Float(f)) => Bool(*f != 0.0),
            (SettingType::Int, Str(s)) => Int(s.trim().parse().ok()?),
            (SettingType::Int, Bool(b)) => Int(i64::from(*b)),
            (SettingType::Int, Int(i)) => Int(*i),
            (SettingType::Int, Float(f)) => {
                if !f.is_finite() {
                    return None;
                }
                Int(f.trunc() as i64)
            }
            (SettingType::Float, Str(s)) => Float(s.trim().parse().ok()?),
            (SettingType::Float, Bool(b)) => Float(if *b { 1.0 } else { 0.0 }),
            (SettingType::Float, Int(i)) => Float(*i as f64),
            (SettingType::Float, Float(f)) => Float(*f),
        })
    }

    /// Текст для INI/REG_SZ — как QVariant::toString у QSettings.
    fn to_qt_string(&self) -> String {
        match self {
            SettingValue::Str(s) => s.clone(),
            SettingValue::Bool(b) => b.to_string(),
            SettingValue::Int(i) => i.to_string(),
            SettingValue::Float(f) => qt_float_str(*f),
        }
    }
}

/// JSON-массив строк; не JSON — список через запятую (пустые элементы отбрасываются).
fn parse_string_list(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if let Ok(list) = serde_json::from_str::<Vec<String>>(text) {
        return list;
    }
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// str(float) в Python: 1.0 → "1.0".
fn py_float_str(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

/// QString::number(d, 'g', FloatingPointShortest): 1.0 → "1", 0.7 → "0.7".
fn qt_float_str(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        f.to_string()
    }
}

/// Хранилище настроек. Клонируйте Arc<SettingsStore>.
pub struct SettingsStore {
    backend: Box<dyn Backend>,
    changes: broadcast::Sender<SettingKey>,
    write_lock: Mutex<()>,
}

/// Низкоуровневое хранилище строк/чисел (INI или реестр).
pub trait Backend: Send + Sync {
    fn read(&self, name: &str) -> Option<SettingValue>;
    fn write(&self, name: &str, value: &SettingValue) -> std::io::Result<()>;
}

impl SettingsStore {
    /// Хранилище по умолчанию для ОС; `config_override` — каталог из --config-dir.
    pub fn open(config_override: Option<PathBuf>) -> Arc<Self> {
        #[cfg(target_os = "windows")]
        if config_override.is_none() {
            return Self::with_backend(Box::new(registry::RegistryBackend::new()));
        }
        let path = crate::dirs::settings_ini_file(config_override.as_deref());
        Self::with_backend(Box::new(IniBackend::open(path)))
    }

    /// Хранилище в памяти (тесты).
    pub fn in_memory() -> Arc<Self> {
        Self::with_backend(Box::new(MemoryBackend::default()))
    }

    /// Своё хранилище (тесты, особые случаи).
    pub fn with_backend(backend: Box<dyn Backend>) -> Arc<Self> {
        let (changes, _) = broadcast::channel(64);
        Arc::new(Self { backend, changes, write_lock: Mutex::new(()) })
    }

    pub fn get(&self, key: SettingKey) -> SettingValue {
        if key == SettingKey::WindowButtons {
            return SettingValue::Bool(true);
        }
        self.get_stored(key)
    }

    /// Сохранённое значение без подмен (windowButtons всегда true в `get`).
    pub fn get_stored(&self, key: SettingKey) -> SettingValue {
        self.backend
            .read(key.name())
            .and_then(|value| value.coerce(key.value_type()))
            .map(|value| key.normalize(value))
            .unwrap_or_else(|| key.default_value())
    }

    pub fn get_str(&self, key: SettingKey) -> String {
        match self.get(key).coerce(SettingType::Str) {
            Some(SettingValue::Str(s)) => s,
            _ => String::new(),
        }
    }

    pub fn get_bool(&self, key: SettingKey) -> bool {
        matches!(self.get(key).coerce(SettingType::Bool), Some(SettingValue::Bool(true)))
    }

    pub fn get_int(&self, key: SettingKey) -> i64 {
        match self.get(key).coerce(SettingType::Int) {
            Some(SettingValue::Int(i)) => i,
            _ => 0,
        }
    }

    pub fn get_f64(&self, key: SettingKey) -> f64 {
        match self.get(key).coerce(SettingType::Float) {
            Some(SettingValue::Float(f)) => f,
            _ => 0.0,
        }
    }

    /// Список строк (Pins, HiddenTracks): JSON-массив; старый вид «a,b» тоже читается.
    pub fn get_string_list(&self, key: SettingKey) -> Vec<String> {
        parse_string_list(&self.get_str(key))
    }

    /// Записать список строк JSON-массивом; пустой список — пустая строка. true — изменилось.
    pub fn set_string_list<S: AsRef<str>>(&self, key: SettingKey, list: &[S]) -> bool {
        let text = if list.is_empty() {
            String::new()
        } else {
            let items: Vec<&str> = list.iter().map(AsRef::as_ref).collect();
            serde_json::to_string(&items).unwrap_or_default()
        };
        self.set(key, SettingValue::Str(text))
    }

    /// Записать; true — значение изменилось (и разослано подписчикам). Значение приводится к типу ключа
    /// и к допустимому диапазону (SettingKey::normalize).
    pub fn set(&self, key: SettingKey, value: SettingValue) -> bool {
        let Some(value) = value.coerce(key.value_type()).map(|v| key.normalize(v)) else {
            tracing::warn!("setting {}: value {value:?} does not match its type", key.name());
            return false;
        };
        {
            let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
            if self.get_stored(key) == value {
                return false;
            }
            if let Err(err) = self.backend.write(key.name(), &value) {
                tracing::warn!("setting {} not saved: {err}", key.name());
                return false;
            }
        }
        let _ = self.changes.send(key);
        if key == SettingKey::LyricsMode {
            // При смене стиля текста сбрасываем ручные параметры к значениям по умолчанию для стиля
            for reset_key in [
                SettingKey::LyricsActiveScale,
                SettingKey::LyricsWordBounce,
                SettingKey::LyricsDimOpacity,
                SettingKey::LyricsScrollMs,
                SettingKey::LyricsStaggerMs,
                SettingKey::LyricsIdleScale,
            ] {
                self.set(reset_key, reset_key.default_value());
            }
        }
        true
    }

    /// Изменения от любого источника (мост Settings, Player.volume, Wave.select…).
    pub fn subscribe(&self) -> broadcast::Receiver<SettingKey> {
        self.changes.subscribe()
    }
}

/// В памяти.
#[derive(Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, SettingValue>>,
}

impl Backend for MemoryBackend {
    fn read(&self, name: &str) -> Option<SettingValue> {
        self.values.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }

    fn write(&self, name: &str, value: &SettingValue) -> std::io::Result<()> {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_owned(), value.clone());
        Ok(())
    }
}

/// INI-файл в формате QSettings. Чужие секции и ключи сохраняются при перезаписи.
pub struct IniBackend {
    path: PathBuf,
    doc: Mutex<ini::Document>,
}

impl IniBackend {
    pub fn open(path: PathBuf) -> Self {
        let text = match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("settings {} not read: {err}", path.display());
                }
                String::new()
            }
        };
        Self { path, doc: Mutex::new(ini::Document::parse(&text)) }
    }
}

impl Backend for IniBackend {
    fn read(&self, name: &str) -> Option<SettingValue> {
        let doc = self.doc.lock().unwrap_or_else(|e| e.into_inner());
        let raw = doc.get(name)?;
        ini::decode_value(raw).map(SettingValue::Str)
    }

    fn write(&self, name: &str, value: &SettingValue) -> std::io::Result<()> {
        let mut doc = self.doc.lock().unwrap_or_else(|e| e.into_inner());
        let encoded = match value {
            SettingValue::Str(s) => ini::encode_string(&escape_at(s)),
            other => other.to_qt_string(),
        };
        doc.set(name, &encoded);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // атомарная замена: сначала временный файл рядом
        let tmp = self.path.with_extension("conf.tmp");
        std::fs::write(&tmp, doc.render())?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// QSettings: строка, начинающаяся с '@', пишется с удвоенным '@'.
fn escape_at(s: &str) -> String {
    if s.starts_with('@') { format!("@{s}") } else { s.to_owned() }
}

/// Обратное к escape_at и разбор служебных форм QSettings ("@Invalid()", "@ByteArray(...)").
fn unescape_at(s: String) -> Option<String> {
    if let Some(rest) = s.strip_prefix("@@") {
        return Some(format!("@{rest}"));
    }
    if let Some(inner) = s.strip_prefix("@ByteArray(").and_then(|r| r.strip_suffix(')')) {
        return Some(inner.to_owned());
    }
    if s.starts_with('@') && s.ends_with(')') && s[1..].chars().next().is_some_and(char::is_alphabetic) {
        // @Invalid(), @Variant(...) и прочие типы Qt — не наши значения
        return None;
    }
    Some(s)
}

mod ini {
    //! Разбор и запись INI так, как это делает QSettings (iniEscapedString / iniUnescapedStringList).

    #[derive(Debug, Default)]
    pub struct Document {
        /// Строки до первой секции (QSettings считает их General).
        preamble: Vec<String>,
        sections: Vec<Section>,
    }

    #[derive(Debug)]
    struct Section {
        name: String,
        lines: Vec<String>,
    }

    fn header_name(line: &str) -> Option<&str> {
        let t = line.trim();
        t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
    }

    fn split_entry(line: &str) -> Option<(&str, &str)> {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        Some((key.trim(), value))
    }

    impl Document {
        pub fn parse(text: &str) -> Self {
            let mut doc = Document::default();
            // продолжение строки обратным слешем QSettings не пишет — не поддерживаем
            for line in text.lines() {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(name) = header_name(line) {
                    doc.sections.push(Section { name: name.to_owned(), lines: Vec::new() });
                } else if let Some(section) = doc.sections.last_mut() {
                    section.lines.push(line.to_owned());
                } else {
                    doc.preamble.push(line.to_owned());
                }
            }
            doc
        }

        fn general_lines(&self) -> impl Iterator<Item = &String> {
            self.preamble.iter().chain(
                self.sections
                    .iter()
                    .filter(|s| s.name.eq_ignore_ascii_case("General"))
                    .flat_map(|s| s.lines.iter()),
            )
        }

        /// Сырое значение (после '=') ключа секции General; последнее вхождение побеждает.
        pub fn get(&self, key: &str) -> Option<&str> {
            self.general_lines()
                .filter_map(|line| split_entry(line))
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .last()
        }

        pub fn set(&mut self, key: &str, encoded: &str) {
            let entry = format!("{key}={encoded}");
            let replace = |lines: &mut Vec<String>| -> bool {
                let mut found = false;
                lines.retain_mut(|line| {
                    if split_entry(line).is_some_and(|(k, _)| k == key) {
                        if found {
                            return false;
                        }
                        found = true;
                        *line = entry.clone();
                    }
                    true
                });
                found
            };
            if replace(&mut self.preamble) {
                return;
            }
            for section in self.sections.iter_mut().filter(|s| s.name.eq_ignore_ascii_case("General")) {
                if replace(&mut section.lines) {
                    return;
                }
            }
            match self.sections.iter_mut().find(|s| s.name.eq_ignore_ascii_case("General")) {
                Some(section) => {
                    // перед хвостовыми пустыми строками секции
                    let at = section
                        .lines
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .map_or(0, |i| i + 1);
                    section.lines.insert(at, entry);
                }
                None => {
                    let mut lines = vec![entry];
                    if !self.sections.is_empty() {
                        lines.push(String::new());
                    }
                    self.sections.insert(0, Section { name: "General".into(), lines });
                }
            }
        }

        pub fn render(&self) -> String {
            let mut out = String::new();
            for line in &self.preamble {
                out.push_str(line);
                out.push('\n');
            }
            for section in &self.sections {
                out.push('[');
                out.push_str(&section.name);
                out.push_str("]\n");
                for line in &section.lines {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out
        }
    }

    /// iniEscapedString из qsettings.cpp.
    pub fn encode_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        let mut needs_quotes = false;
        let mut escape_next_if_digit = false;
        for ch in s.chars() {
            if matches!(ch, ';' | ',' | '=') {
                needs_quotes = true;
            }
            if escape_next_if_digit && ch.is_ascii_hexdigit() {
                out.push_str(&format!("\\x{:x}", ch as u32));
                continue;
            }
            escape_next_if_digit = false;
            match ch {
                '\0' => {
                    out.push_str("\\0");
                    escape_next_if_digit = true;
                }
                '\u{7}' => out.push_str("\\a"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{b}' => out.push_str("\\v"),
                '"' | '\\' => {
                    out.push('\\');
                    out.push(ch);
                }
                c if (c as u32) <= 0x1f => {
                    out.push_str(&format!("\\x{:x}", c as u32));
                    escape_next_if_digit = true;
                }
                c => out.push(c),
            }
        }
        if needs_quotes || out.starts_with(' ') || out.ends_with(' ') {
            format!("\"{out}\"")
        } else {
            out
        }
    }

    /// iniUnescapedStringList: кавычки, escape-последовательности, запятые вне кавычек — список.
    /// Список склеивается обратно через ',' (так его записала бы рука человека).
    pub fn decode_parts(raw: &str) -> Vec<String> {
        // (символ, защищён ли от обрезки пробелов)
        let mut parts: Vec<Vec<(char, bool)>> = vec![Vec::new()];
        let mut in_quotes = false;
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            let cur = parts.last_mut().expect("non-empty");
            match ch {
                '"' => in_quotes = !in_quotes,
                '\\' => {
                    let Some(next) = chars.next() else { break };
                    let c = match next {
                        'a' => '\u{7}',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'v' => '\u{b}',
                        'x' => {
                            let mut code = 0u32;
                            while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
                                code = code.saturating_mul(16).saturating_add(d);
                                chars.next();
                            }
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        '0'..='7' => {
                            let mut code = next.to_digit(8).unwrap_or(0);
                            while let Some(d) = chars.peek().and_then(|c| c.to_digit(8)) {
                                code = code.saturating_mul(8).saturating_add(d);
                                chars.next();
                            }
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        other => other,
                    };
                    cur.push((c, true));
                }
                ',' if !in_quotes => parts.push(Vec::new()),
                ';' if !in_quotes => break,
                c => cur.push((c, in_quotes)),
            }
        }
        parts
            .into_iter()
            .map(|part| {
                let start = part.iter().position(|&(c, p)| p || !c.is_whitespace()).unwrap_or(part.len());
                let end = part.iter().rposition(|&(c, p)| p || !c.is_whitespace()).map_or(start, |i| i + 1);
                part[start..end.max(start)].iter().map(|&(c, _)| c).collect()
            })
            .collect()
    }

    /// Значение строкой; None — служебное значение Qt (@Invalid() и т. п.).
    pub fn decode_value(raw: &str) -> Option<String> {
        super::unescape_at(decode_parts(raw).join(","))
    }
}

#[cfg(target_os = "windows")]
mod registry {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, RegType};

    use super::{Backend, SettingValue};

    const KEY_PATH: &str = r"Software\materialfy\Materialfy";

    /// HKCU\<path>; по умолчанию — ключ QSettings оболочки.
    pub struct RegistryBackend {
        path: String,
    }

    impl RegistryBackend {
        pub fn new() -> Self {
            Self::with_path(KEY_PATH)
        }

        pub fn with_path(path: &str) -> Self {
            Self { path: path.to_owned() }
        }
    }

    impl Backend for RegistryBackend {
        fn read(&self, name: &str) -> Option<SettingValue> {
            let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&self.path).ok()?;
            let raw = key.get_raw_value(name).ok()?;
            match raw.vtype {
                RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
                    let s: String = key.get_value(name).ok()?;
                    super::unescape_at(s).map(SettingValue::Str)
                }
                RegType::REG_MULTI_SZ => {
                    let list: Vec<String> = key.get_value(name).ok()?;
                    Some(SettingValue::Str(list.join(",")))
                }
                // QSettings читает REG_DWORD как int (со знаком)
                RegType::REG_DWORD => {
                    let v: u32 = key.get_value(name).ok()?;
                    Some(SettingValue::Int(i64::from(v as i32)))
                }
                RegType::REG_QWORD => {
                    let v: u64 = key.get_value(name).ok()?;
                    Some(SettingValue::Int(v as i64))
                }
                _ => None,
            }
        }

        fn write(&self, name: &str, value: &SettingValue) -> std::io::Result<()> {
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(&self.path)?;
            match value {
                SettingValue::Int(i) => match i32::try_from(*i) {
                    Ok(v) => key.set_value(name, &(v as u32)),
                    Err(_) => key.set_value(name, &(*i as u64)),
                },
                SettingValue::Str(s) => key.set_value(name, &super::escape_at(s)),
                other => key.set_value(name, &other.to_qt_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema() {
        assert_eq!(SettingKey::ALL.len(), 90);
        for key in SettingKey::ALL {
            assert_eq!(SettingKey::from_name(key.name()), Some(key));
            assert_eq!(key.default_value().value_type(), key.value_type());
        }
        assert_eq!(SettingKey::from_name("nope"), None);
        assert_eq!(SettingKey::Volume.default_value(), SettingValue::Float(0.7));
        assert_eq!(SettingKey::WheelStep.value_type(), SettingType::Int);
    }

    #[test]
    fn speed_and_crossfade_stay_in_range() {
        use SettingValue::*;
        assert_eq!(SettingKey::PlaybackSpeed.normalize(Float(1.23)), Float(1.25));
        assert_eq!(SettingKey::PlaybackSpeed.normalize(Float(9.0)), Float(2.0));
        assert_eq!(SettingKey::PlaybackSpeed.normalize(Float(f64::NAN)), Float(1.0));
        assert_eq!(SettingKey::Crossfade.normalize(Int(-3)), Int(0));
        assert_eq!(SettingKey::Crossfade.normalize(Int(40)), Int(CROSSFADE_MAX_S));
        assert_eq!(SettingKey::UiScale.normalize(Int(100)), Int(100));
        assert_eq!(SettingKey::UiScale.normalize(Int(110)), Int(100));
        assert_eq!(SettingKey::UiScale.normalize(Int(120)), Int(125));
        assert_eq!(SettingKey::UiScale.normalize(Int(1)), Int(75));
        assert_eq!(SettingKey::UiScale.normalize(Int(900)), Int(200));
        assert_eq!(SettingKey::LyricsTextAlign.normalize(Str("auto".into())), Str("left".into()));
        assert_eq!(SettingKey::LyricsTextAlign.normalize(Str("center".into())), Str("center".into()));
        assert_eq!(SettingKey::LyricsTextAlign.normalize(Str("right".into())), Str("right".into()));
        assert_eq!(SettingKey::LyricsFillMode.normalize(Str("auto".into())), Str("word".into()));
        assert_eq!(SettingKey::LyricsFillMode.normalize(Str("line".into())), Str("line".into()));
        assert_eq!(SettingKey::LyricsFillMode.normalize(Str("off".into())), Str("off".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("auto".into())), Str("all".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("three".into())), Str("three".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("one".into())), Str("one".into()));
        assert_eq!(SettingKey::FloatingPlayer.normalize(Str("island".into())), Str("island".into()));
        assert_eq!(SettingKey::FloatingPlayer.normalize(Str("taskbar".into())), Str("taskbar".into()));
        assert_eq!(SettingKey::FloatingPlayer.normalize(Str("both".into())), Str("both".into()));
        assert_eq!(SettingKey::FloatingPlayer.normalize(Str("notch".into())), Str("off".into()));
        assert_eq!(SettingKey::FloatingPlayerStyle.normalize(Str("color".into())), Str("color".into()));
        assert_eq!(SettingKey::FloatingPlayerStyle.normalize(Str("pink".into())), Str("black".into()));
        assert_eq!(SettingKey::FloatingEdge.normalize(Str("left".into())), Str("left".into()));
        assert_eq!(SettingKey::FloatingEdge.normalize(Str("up".into())), Str("top".into()));
        assert_eq!(SettingKey::TaskbarWidgetPos.normalize(Str("appsRight".into())), Str("appsRight".into()));
        assert_eq!(SettingKey::TaskbarWidgetPos.normalize(Str("0.5".into())), Str("0.5000".into()));
        assert_eq!(SettingKey::TaskbarWidgetPos.normalize(Str("7".into())), Str("1.0000".into()));
        assert_eq!(SettingKey::TaskbarWidgetPos.normalize(Str("middle".into())), Str("tray".into()));
        assert_eq!(SettingKey::FloatingKeyHints.normalize(Int(9)), Int(3));
        assert_eq!(SettingKey::GpuPreference.normalize(Str("auto".into())), Str("auto".into()));
        assert_eq!(SettingKey::GpuPreference.normalize(Str("integrated".into())), Str("integrated".into()));
        assert_eq!(SettingKey::GpuPreference.normalize(Str("discrete".into())), Str("discrete".into()));
        assert_eq!(SettingKey::GpuPreference.normalize(Str("custom".into())), Str("auto".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("1".into())), Str("1".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("5".into())), Str("5".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("15".into())), Str("15".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("0".into())), Str("all".into()));
        assert_eq!(SettingKey::LyricsVisibleLines.normalize(Str("16".into())), Str("all".into()));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(0.0)), Float(0.0));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(0.5)), Float(0.0));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(0.99)), Float(0.0));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(1.0)), Float(1.0));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(1.12)), Float(1.12));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(1.5)), Float(1.35));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(-0.2)), Float(0.0));
        assert_eq!(SettingKey::LyricsActiveScale.normalize(Float(f64::NAN)), Float(0.0));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(0.0)), Float(0.0));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(-0.2)), Float(0.0));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(f64::NAN)), Float(0.0));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(0.5)), Float(0.8));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(0.85)), Float(0.85));
        assert_eq!(SettingKey::LyricsIdleScale.normalize(Float(1.5)), Float(1.0));
        assert_eq!(SettingKey::LyricsWordBounce.normalize(Float(1.0)), Float(1.0));
        assert_eq!(SettingKey::LyricsWordBounce.normalize(Float(2.5)), Float(2.0));
        assert_eq!(SettingKey::LyricsWordBounce.normalize(Float(-0.5)), Float(0.0));
        assert_eq!(SettingKey::LyricsWordBounce.normalize(Float(f64::NAN)), Float(1.0));
        assert_eq!(SettingKey::LyricsDimOpacity.normalize(Float(0.35)), Float(0.35));
        assert_eq!(SettingKey::LyricsDimOpacity.normalize(Float(0.1)), Float(0.15));
        assert_eq!(SettingKey::LyricsDimOpacity.normalize(Float(0.9)), Float(0.75));
        assert_eq!(SettingKey::LyricsDimOpacity.normalize(Float(f64::NAN)), Float(0.35));
        assert_eq!(SettingKey::LyricsScrollMs.normalize(Int(-1)), Int(-1));
        assert_eq!(SettingKey::LyricsScrollMs.normalize(Int(100)), Int(200));
        assert_eq!(SettingKey::LyricsScrollMs.normalize(Int(450)), Int(450));
        assert_eq!(SettingKey::LyricsScrollMs.normalize(Int(1200)), Int(800));
        assert_eq!(SettingKey::LyricsStaggerMs.normalize(Int(-1)), Int(-1));
        assert_eq!(SettingKey::LyricsStaggerMs.normalize(Int(-5)), Int(0));
        assert_eq!(SettingKey::LyricsStaggerMs.normalize(Int(30)), Int(30));
        assert_eq!(SettingKey::LyricsStaggerMs.normalize(Int(150)), Int(80));
        assert_eq!(SettingKey::LyricsSyncMs.normalize(Int(-600)), Int(-500));
        assert_eq!(SettingKey::LyricsSyncMs.normalize(Int(0)), Int(0));
        assert_eq!(SettingKey::LyricsSyncMs.normalize(Int(250)), Int(250));
        assert_eq!(SettingKey::LyricsSyncMs.normalize(Int(800)), Int(500));
    }

    #[test]
    fn coerce_like_python() {
        use SettingValue::*;
        assert_eq!(Str("TRUE".into()).coerce(SettingType::Bool), Some(Bool(true)));
        assert_eq!(Str("1".into()).coerce(SettingType::Bool), Some(Bool(false)));
        assert_eq!(Int(1).coerce(SettingType::Bool), Some(Bool(true)));
        assert_eq!(Str(" 12 ".into()).coerce(SettingType::Int), Some(Int(12)));
        assert_eq!(Str("1.5".into()).coerce(SettingType::Int), None);
        assert_eq!(Float(1.9).coerce(SettingType::Int), Some(Int(1)));
        assert_eq!(Str("0.7".into()).coerce(SettingType::Float), Some(Float(0.7)));
        assert_eq!(Str("x".into()).coerce(SettingType::Float), None);
        assert_eq!(Int(4).coerce(SettingType::Str), Some(Str("4".into())));
        assert_eq!(Float(1.0).coerce(SettingType::Str), Some(Str("1.0".into())));
    }

    #[test]
    fn memory_store() {
        let store = SettingsStore::in_memory();
        let mut rx = store.subscribe();
        assert_eq!(store.get_str(SettingKey::ThemeMode), "dark");
        assert_eq!(store.get_int(SettingKey::WindowWidth), 1280);
        assert!((store.get_f64(SettingKey::Volume) - 0.7).abs() < 1e-12);
        assert!(!store.set(SettingKey::ThemeMode, SettingValue::Str("dark".into())));
        assert!(rx.try_recv().is_err());
        assert!(store.set(SettingKey::ThemeMode, SettingValue::Str("light".into())));
        assert_eq!(rx.try_recv().unwrap(), SettingKey::ThemeMode);
        assert_eq!(store.get_str(SettingKey::ThemeMode), "light");
        // приведение к типу ключа
        assert!(store.set(SettingKey::Bitrate, SettingValue::Float(160.0)));
        assert_eq!(store.get(SettingKey::Bitrate), SettingValue::Int(160));
        assert!(!store.set(SettingKey::Bitrate, SettingValue::Str("160".into())));
        assert!(!store.set(SettingKey::Volume, SettingValue::Str("громко".into())));
    }

    #[test]
    fn window_buttons() {
        let store = SettingsStore::in_memory();
        assert!(!store.get_stored(SettingKey::WindowButtons).eq(&SettingValue::Bool(true)));
        assert!(store.get_bool(SettingKey::WindowButtons));
        assert!(store.set(SettingKey::WindowButtons, SettingValue::Bool(true)));
        assert!(store.get_bool(SettingKey::WindowButtons));
        assert!(store.set(SettingKey::WindowButtons, SettingValue::Bool(false)));
        assert_eq!(store.get_stored(SettingKey::WindowButtons), SettingValue::Bool(false));
        assert!(store.get_bool(SettingKey::WindowButtons));
    }

    #[test]
    fn ini_escape() {
        assert_eq!(ini::encode_string("plain"), "plain");
        assert_eq!(ini::encode_string("a,b"), "\"a,b\"");
        assert_eq!(ini::encode_string(" pad"), "\" pad\"");
        assert_eq!(ini::encode_string("q\"\\"), "q\\\"\\\\");
        assert_eq!(ini::encode_string("x\ty"), "x\\ty");
        assert_eq!(ini::encode_string("\u{1}a"), "\\x1\\x61");
        assert_eq!(ini::encode_string("тест"), "тест");
        for s in ["plain", "a,b", " pad ", "q\"\\", "x\ty\n", "\u{1}abc", "тест; ок", "", "k=v"] {
            assert_eq!(ini::decode_value(&ini::encode_string(s)).as_deref(), Some(s), "{s:?}");
        }
        // как читает QSettings
        assert_eq!(ini::decode_value("  spaced  ").as_deref(), Some("spaced"));
        assert_eq!(ini::decode_value("a, b").as_deref(), Some("a,b"));
        assert_eq!(ini::decode_value("v ; comment").as_deref(), Some("v"));
        assert_eq!(ini::decode_value("@Invalid()"), None);
        assert_eq!(ini::decode_value("@@at").as_deref(), Some("@at"));
    }

    fn temp_ini(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("mf-platform-test-{}-{name}", std::process::id()))
            .join("materialfy")
            .join("Materialfy.conf")
    }

    #[test]
    fn ini_python_compat() {
        let path = temp_ini("compat");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // так файл пишет QSettings из Python-версии
        let python = "[General]\naccentFromCover=false\nbitrate=160\ncustomSeed=#FF0000\nvolume=0.35\n\
                      pins=\"spotify:a,spotify:b\"\nwindowWidth=oops\n\n[Other]\nkeep=me\n";
        std::fs::write(&path, python).unwrap();
        let store = SettingsStore::with_backend(Box::new(IniBackend::open(path.clone())));
        assert!(!store.get_bool(SettingKey::AccentFromCover));
        assert_eq!(store.get_int(SettingKey::Bitrate), 160);
        assert_eq!(store.get_str(SettingKey::CustomSeed), "#FF0000");
        assert!((store.get_f64(SettingKey::Volume) - 0.35).abs() < 1e-12);
        assert_eq!(store.get_str(SettingKey::Pins), "spotify:a,spotify:b");
        assert_eq!(store.get_int(SettingKey::WindowWidth), 1280);

        assert!(store.set(SettingKey::Volume, SettingValue::Float(0.5)));
        assert!(store.set(SettingKey::Pins, SettingValue::Str("a,b".into())));
        assert!(store.set(SettingKey::ThemeMode, SettingValue::Str("light".into())));
        assert!(store.set(SettingKey::NowPlayingBlur, SettingValue::Float(0.0)));
        assert!(store.set(SettingKey::CoverSpin, SettingValue::Bool(false)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("volume=0.5\n"), "{text}");
        assert!(text.contains("pins=\"a,b\"\n"), "{text}");
        assert!(text.contains("themeMode=light\n"), "{text}");
        assert!(text.contains("nowPlayingBlur=0\n"), "{text}");
        assert!(text.contains("coverSpin=false\n"), "{text}");
        assert!(text.contains("[Other]\nkeep=me\n"), "{text}");
        let general_end = text.find("[Other]").unwrap();
        assert!(text[..general_end].contains("themeMode=light"));

        // перечитать с диска
        let again = SettingsStore::with_backend(Box::new(IniBackend::open(path.clone())));
        assert_eq!(again.get_str(SettingKey::Pins), "a,b");
        assert!((again.get_f64(SettingKey::Volume) - 0.5).abs() < 1e-12);
        assert_eq!(again.get_str(SettingKey::ThemeMode), "light");
        assert!(!again.get_bool(SettingKey::CoverSpin));
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_python_compat() {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let path = format!(r"Software\materialfy\Materialfy-mf-platform-test-{}", std::process::id());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        {
            // так значения пишет QSettings из Python-версии
            let (key, _) = hkcu.create_subkey(&path).unwrap();
            key.set_value("nowPlayingBlur", &"1".to_string()).unwrap();
            key.set_value("accentFromCover", &"false".to_string()).unwrap();
            key.set_value("windowWidth", &1600u32).unwrap();
            key.set_value("bitrate", &"96".to_string()).unwrap();
        }
        let store = SettingsStore::with_backend(Box::new(registry::RegistryBackend::with_path(&path)));
        assert!((store.get_f64(SettingKey::NowPlayingBlur) - 1.0).abs() < 1e-12);
        assert!(!store.get_bool(SettingKey::AccentFromCover));
        assert_eq!(store.get_int(SettingKey::WindowWidth), 1600);
        assert_eq!(store.get_int(SettingKey::Bitrate), 96);
        assert!(store.set(SettingKey::WindowHeight, SettingValue::Int(700)));
        assert!(store.set(SettingKey::Volume, SettingValue::Float(0.25)));
        assert!(store.set(SettingKey::CoverPulse, SettingValue::Bool(false)));
        assert!(store.set(SettingKey::CustomSeed, SettingValue::Str("@x".into())));
        let key = hkcu.open_subkey(&path).unwrap();
        assert_eq!(key.get_value::<u32, _>("windowHeight").unwrap(), 700);
        assert_eq!(key.get_value::<String, _>("volume").unwrap(), "0.25");
        assert_eq!(key.get_value::<String, _>("coverPulse").unwrap(), "false");
        assert_eq!(key.get_value::<String, _>("customSeed").unwrap(), "@@x");
        assert_eq!(store.get_str(SettingKey::CustomSeed), "@x");
        assert_eq!(store.get_int(SettingKey::WindowHeight), 700);
        hkcu.delete_subkey_all(&path).unwrap();
    }

    #[test]
    fn engine_limits() {
        use SettingValue::Int;
        let store = SettingsStore::in_memory();
        assert_eq!(store.get_int(SettingKey::Bitrate), 320);
        for (input, want) in
            [(96, 96), (100, 96), (128, 96), (129, 160), (160, 160), (200, 160), (256, 320), (999, 320), (0, 96)]
        {
            assert_eq!(SettingKey::Bitrate.normalize(Int(input)), Int(want), "{input}");
        }
        assert!(store.set(SettingKey::Bitrate, Int(160)));
        assert!(!store.set(SettingKey::Bitrate, Int(150)));
        assert_eq!(store.get_int(SettingKey::Bitrate), 160);
        assert!(store.set(SettingKey::Bitrate, SettingValue::Str("96".into())));
        assert_eq!(store.get_int(SettingKey::Bitrate), 96);

        assert_eq!(store.get_int(SettingKey::AudioCacheMb), 2048);
        for (input, want) in
            [(0, 256), (256, 256), (300, 256), (400, 512), (1000, 1024), (10240, 10240), (99999, 10240)]
        {
            assert_eq!(SettingKey::AudioCacheMb.normalize(Int(input)), Int(want), "{input}");
        }
        assert!(store.set(SettingKey::AudioCacheMb, Int(5000)));
        assert_eq!(store.get_int(SettingKey::AudioCacheMb), 5120);
        // WheelStep — без ограничений, как было
        assert!(store.set(SettingKey::WheelStep, Int(37)));
        assert_eq!(store.get_int(SettingKey::WheelStep), 37);
        for key in SettingKey::ALL {
            assert_eq!(key.normalize(key.default_value()), key.default_value(), "{}", key.name());
        }
        // посторонние значения из хранилища тоже приводятся
        let backend = MemoryBackend::default();
        backend.write("bitrate", &SettingValue::Str("128".into())).unwrap();
        backend.write("audioCacheMb", &Int(1)).unwrap();
        let store = SettingsStore::with_backend(Box::new(backend));
        assert_eq!(store.get_int(SettingKey::Bitrate), 96);
        assert_eq!(store.get_int(SettingKey::AudioCacheMb), 256);
    }

    #[test]
    fn string_lists() {
        let store = SettingsStore::in_memory();
        assert!(store.get_string_list(SettingKey::Pins).is_empty());
        assert!(!store.set_string_list::<&str>(SettingKey::Pins, &[]));
        let pins = ["spotify:playlist:a", "spotify:album:b,c", "кавычка \"x\""];
        assert!(store.set_string_list(SettingKey::Pins, &pins));
        assert_eq!(
            store.get_str(SettingKey::Pins),
            r#"["spotify:playlist:a","spotify:album:b,c","кавычка \"x\""]"#
        );
        assert_eq!(store.get_string_list(SettingKey::Pins), pins);
        assert!(!store.set_string_list(SettingKey::Pins, &pins));
        assert!(store.set_string_list(SettingKey::Pins, &Vec::<String>::new()));
        assert_eq!(store.get_str(SettingKey::Pins), "");
        // старый вид — через запятую
        assert!(store.set(SettingKey::HiddenTracks, SettingValue::Str("spotify:track:1, spotify:track:2,".into())));
        assert_eq!(store.get_string_list(SettingKey::HiddenTracks), ["spotify:track:1", "spotify:track:2"]);

        // INI: JSON с запятыми и кавычками переживает перечитывание
        let path = temp_ini("lists");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let store = SettingsStore::with_backend(Box::new(IniBackend::open(path.clone())));
        assert!(store.set_string_list(SettingKey::Pins, &pins));
        let again = SettingsStore::with_backend(Box::new(IniBackend::open(path.clone())));
        assert_eq!(again.get_string_list(SettingKey::Pins), pins);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn language_follows_system_by_default() {
        let store = SettingsStore::in_memory();
        assert_eq!(store.get_str(SettingKey::Language), "system");
        // an explicit English differs from the default now, so it is stored and kept
        assert!(store.set(SettingKey::Language, SettingValue::Str("en".into())));
        assert_eq!(store.get_str(SettingKey::Language), "en");
        assert!(store.set(SettingKey::Language, SettingValue::Str("ru".into())));
        assert!(store.set(SettingKey::Language, SettingValue::Str("system".into())));
        assert!(!store.set(SettingKey::Language, SettingValue::Str("system".into())));
        assert_eq!(store.get_str(SettingKey::Language), "system");
    }

    #[test]
    fn update_settings() {
        let store = SettingsStore::in_memory();
        assert!(store.get_bool(SettingKey::AutoUpdateCheck));
        assert_eq!(store.get_str(SettingKey::UpdateChannel), "stable");
        assert_eq!(store.get_str(SettingKey::SkippedVersion), "");
        assert!(store.set(SettingKey::UpdateChannel, SettingValue::Str("Beta".into())));
        assert_eq!(store.get_str(SettingKey::UpdateChannel), "beta");
        // anything else falls back to stable
        assert!(store.set(SettingKey::UpdateChannel, SettingValue::Str("nightly".into())));
        assert_eq!(store.get_str(SettingKey::UpdateChannel), "stable");
        assert!(store.set(SettingKey::AutoUpdateCheck, SettingValue::Bool(false)));
        assert!(!store.get_bool(SettingKey::AutoUpdateCheck));
        assert!(store.set(SettingKey::SkippedVersion, SettingValue::Str("0.2.0".into())));
        assert_eq!(store.get_str(SettingKey::SkippedVersion), "0.2.0");
    }

    #[test]
    fn qml_names() {
        // name() = имя свойства QML; служебные pins/hiddenTracks свойств не имеют
        let names: Vec<&str> = SettingKey::ALL.iter().map(|k| k.name()).collect();
        for name in [
            "themeMode", "schemeVariant", "accentFromCover", "customSeed", "windowButtons", "trackTransition",
            "backdropMode", "nowPlayingBlur", "nowPlayingDrift", "ambientBackground", "coverSpin", "coverPulse",
            "calmWhenInactive", "lyricsMode", "lyricsFontScale", "lyricsScrim",
            "lyricsTextAlign", "lyricsFillMode", "lyricsVisibleLines", "lyricsHighlight",
            "lyricsActiveScale", "lyricsIdleScale", "lyricsWordBounce", "lyricsDimOpacity",
            "lyricsScrollMs", "lyricsStaggerMs", "lyricsBlurAmount",
            "lyricsLineSpacing", "lyricsAnimate", "lyricsShowSource", "lyricsSyncMs",
            "animations", "lowPowerMode", "gpuPreference", "rightPanelOpen", "rightPanelTab", "sidebarCollapsed", "windowWidth",
            "windowHeight", "windowX", "windowY", "windowMaximized", "closeToTray", "wheelStep", "floatingPlayer", "floatingPlayerStyle",
            "taskbarWidgetPos", "floatingKeyHints", "floatingHideFullscreen", "floatingHideMaximized", "floatingOffsetX", "floatingEdge", "floatingFreeX", "floatingFreeY", "libraryView",
            "librarySort", "homeHidden", "homeRadioHidden", "sidebarLibrary", "bitrate", "normalisation", "gapless", "autoplay", "volume", "audioDevice",
            "audioCache", "audioCacheMb", "playbackAuthorized", "deviceName", "webClientId", "apiLimitTipDismissed", "proxyMode", "proxyHost",
            "proxyPort", "proxyUsername", "coversBypassProxy", "pins", "hiddenTracks", "radioSeeds", "language", "uiScale", "autoUpdateCheck",
            "updateChannel", "skippedVersion",
        ] {
            assert!(names.contains(&name), "{name}");
        }
        assert_eq!(SettingKey::CustomSeed.default_value(), SettingValue::Str("#8C6DFF".into()));
        assert_eq!(SettingKey::Animations.default_value(), SettingValue::Str("system".into()));
        assert_eq!(SettingKey::Language.default_value(), SettingValue::Str("system".into()));
        assert_eq!(SettingKey::ProxyPort.value_type(), SettingType::Str);
        assert_eq!(SettingKey::AudioCacheMb.value_type(), SettingType::Int);
        assert_eq!(SettingKey::Normalisation.value_type(), SettingType::Bool);
        assert_eq!(SettingKey::LyricsScrollMs.default_value(), SettingValue::Int(-1));
        assert_eq!(SettingKey::LyricsStaggerMs.default_value(), SettingValue::Int(-1));
        assert_eq!(SettingKey::LyricsIdleScale.default_value(), SettingValue::Float(0.0));
        assert!(BITRATES.iter().any(|b| SettingKey::Bitrate.default_value() == SettingValue::Int(*b)));
    }

    #[test]
    fn lyrics_mode_change_resets_manual_adjustments() {
        let store = SettingsStore::in_memory();
        // Задаём кастомные значения для шести ручных регулировок текста
        store.set(SettingKey::LyricsActiveScale, SettingValue::Float(1.2));
        store.set(SettingKey::LyricsWordBounce, SettingValue::Float(1.5));
        store.set(SettingKey::LyricsDimOpacity, SettingValue::Float(0.6));
        store.set(SettingKey::LyricsScrollMs, SettingValue::Int(350));
        store.set(SettingKey::LyricsStaggerMs, SettingValue::Int(15));
        store.set(SettingKey::LyricsIdleScale, SettingValue::Float(0.85));

        // Изначальный режим — normal. Меняем на spicy: шесть регулировок должны сброситься к дефолтам стиля
        assert!(store.set(SettingKey::LyricsMode, SettingValue::Str("spicy".into())));
        assert_eq!(store.get(SettingKey::LyricsActiveScale), SettingValue::Float(0.0));
        assert_eq!(store.get(SettingKey::LyricsWordBounce), SettingValue::Float(1.0));
        assert_eq!(store.get(SettingKey::LyricsDimOpacity), SettingValue::Float(0.35));
        assert_eq!(store.get(SettingKey::LyricsScrollMs), SettingValue::Int(-1));
        assert_eq!(store.get(SettingKey::LyricsStaggerMs), SettingValue::Int(-1));
        assert_eq!(store.get(SettingKey::LyricsIdleScale), SettingValue::Float(0.0));

        // Пользователь настраивает параметры в новом режиме
        store.set(SettingKey::LyricsActiveScale, SettingValue::Float(1.15));
        store.set(SettingKey::LyricsWordBounce, SettingValue::Float(0.8));
        store.set(SettingKey::LyricsDimOpacity, SettingValue::Float(0.5));
        store.set(SettingKey::LyricsScrollMs, SettingValue::Int(400));
        store.set(SettingKey::LyricsStaggerMs, SettingValue::Int(25));
        store.set(SettingKey::LyricsIdleScale, SettingValue::Float(0.95));

        // Повторное применение того же режима (spicy -> spicy) НЕ должно сбрасывать настройки
        assert!(!store.set(SettingKey::LyricsMode, SettingValue::Str("spicy".into())));
        assert_eq!(store.get(SettingKey::LyricsActiveScale), SettingValue::Float(1.15));
        assert_eq!(store.get(SettingKey::LyricsWordBounce), SettingValue::Float(0.8));
        assert_eq!(store.get(SettingKey::LyricsDimOpacity), SettingValue::Float(0.5));
        assert_eq!(store.get(SettingKey::LyricsScrollMs), SettingValue::Int(400));
        assert_eq!(store.get(SettingKey::LyricsStaggerMs), SettingValue::Int(25));
        assert_eq!(store.get(SettingKey::LyricsIdleScale), SettingValue::Float(0.95));
    }

    #[test]
    fn ini_new_file() {
        let path = temp_ini("new");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let store = SettingsStore::with_backend(Box::new(IniBackend::open(path.clone())));
        assert!(store.set(SettingKey::WindowHeight, SettingValue::Int(900)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[General]\nwindowHeight=900\n");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
