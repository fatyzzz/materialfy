//! Плеер Spotify без Qt. Верхний слой (исполнитель «queue-state»): PlayerHandle в форме yamusic-md3 — команды,
//! снимок PlayerSnapshot (watch) + события PlayerEvent (broadcast), слияние локального librespot и удалённого
//! Connect-устройства (Target), очередь по правилам spotifast/docs/_reference/queue.md, оптимистичные
//! удержания. Нижний слой — модуль [`engine`] (исполнитель «engine»).
//!
//! Потоки: всё на общем tokio-рантайме приложения (Handle из оболочки); librespot сам заводит поток плеера,
//! fastframe-audio — поток устройства. Наружу — только watch/broadcast; Qt будят мосты (CxxQtThread::queue).

pub mod engine;
pub mod handle;
/// JSON pushes of the dealer (social-connect) for sf-services
pub mod pushes;
/// внутреннее «queue-state»
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};
use tokio::sync::{broadcast, watch};

pub use engine::sink::output_devices;
pub use engine::{EngineConfig, LoadSpec, LocalState, RepeatMode, Rootlist, RootlistEntry};
pub use handle::PlayerHandle;
pub use librespot_core::Session;
pub use librespot_core::spclient::SpClient;
pub use librespot_core::authentication::Credentials;

/// Интервалы опроса /me/player (spotifast app.rs: REMOTE_POLL_*), с.
pub const REMOTE_POLL_ACTIVE_S: u64 = 4;
pub const REMOTE_POLL_IDLE_S: u64 = 20;
pub const REMOTE_POLL_UNSEEN_S: u64 = 30;
/// Удержания оптимистичного состояния (AGENTS.md), мс.
pub const OPTIMISTIC_HOLD_MS: u64 = 2500;
pub const PLAYBACK_HOLD_MS: u64 = 6000;
/// Сколько выбранный трек держится на экране, пока движок его грузит (холодный старт бывает дольше 6 с).
pub const INTENT_CEILING_MS: u64 = 20_000;
pub const ASSUMED_CONTEXT_HOLD_MS: u64 = 8000;
pub const REMOTE_RECHECK_MS: u64 = 1200;
pub const QUEUE_RECHECK_MS: u64 = 700;
pub const QUEUE_STALE_RETRIES: u32 = 6;
pub const QUEUE_ADD_DEBOUNCE_MS: u64 = 1500;
pub const RESTART_BEFORE_PREVIOUS_MS: i64 = 3000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    /// повтор контекста (Spotify "context")
    All,
    /// повтор трека (Spotify "track")
    One,
}

impl Repeat {
    /// "off" | "all" | "one" (значение Player.repeat в QML)
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "all" => Some(Self::All),
            "one" => Some(Self::One),
            _ => None,
        }
    }

    pub fn to_engine(self) -> RepeatMode {
        match self {
            Self::Off => RepeatMode::Off,
            Self::All => RepeatMode::Context,
            Self::One => RepeatMode::Track,
        }
    }

    pub fn from_engine(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => Self::Off,
            RepeatMode::Context => Self::All,
            RepeatMode::Track => Self::One,
        }
    }

    /// off → all → one → off
    pub fn cycled(self) -> Self {
        Self::from_engine(self.to_engine().next())
    }
}

/// Куда идут команды: локальный librespot или устройство Connect (Web API; None — активное/неизвестно).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Local,
    Remote(Option<String>),
}

/// Состояние локального воспроизведения (librespot на этом ПК).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocalPlayback {
    /// движок не запущен (нет разрешения/Premium или выключено)
    #[default]
    Unavailable,
    Connecting,
    Ready { device_id: String },
    Failed(String),
}

impl LocalPlayback {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Итог connect_local: id устройства и переиспользуемые креды librespot (сохранить в слот Playback).
pub struct EngineStarted {
    pub device_id: String,
    pub reusable: Option<Credentials>,
}

#[derive(Clone)]
pub struct PlayerConfig {
    /// mf_platform::dirs::cache_subdir("covers"): обложка текущего трека → cover_file
    pub cover_dir: PathBuf,
    /// очередь «Далее в очереди» и точка возобновления (queue.md п. 9); None — не сохранять (режим снимков)
    pub session_file: Option<PathBuf>,
    /// громкость 0..1 из настроек
    pub volume: f64,
    /// общий HTTP-клиент (обложки)
    pub http: sf_api::Http,
}

/// Снимок всех свойств Player для QML (обновляется актором до рассылки событий).
#[derive(Clone, Debug, Default)]
pub struct PlayerSnapshot {
    /// текущий трек/выпуск (None — нет); hasTrack/trackId/title/artist/album/cover/coverLarge/uri/track
    pub track: Option<TrackItem>,
    /// "OGG 320" у локального воспроизведения, "" у удалённого
    pub codec: String,
    /// путь скачанной обложки (тема из обложки, SMTC); "" — нет
    pub cover_file: String,
    pub playing: bool,
    pub buffering: bool,
    /// The DJ is talking between tracks on this computer (Player.narrating).
    pub narrating: bool,
    /// с экстраполяцией между событиями/опросами
    pub position_ms: i64,
    pub duration_ms: i64,
    /// 0..1
    pub volume: f64,
    pub shuffle: bool,
    /// Smart Shuffle: перемешивание с рекомендациями Spotify (shuffle тоже true)
    pub smart_shuffle: bool,
    /// Smart Shuffle можно включить (плейлист на этом ПК)
    pub smart_shuffle_available: bool,
    pub repeat: Repeat,
    /// индекс строки section=current в queue, -1 — нет
    pub current_index: i32,
    pub error_text: String,
    /// +1 — вперёд, -1 — назад (анимация смены трека)
    pub direction: i32,
    /// «дыхание» обложки 0..1 (только локально и пока levels_wanted)
    pub level: f64,
    pub levels_wanted: bool,
    pub context: PlayContext,
    /// Сейчас + Далее в очереди + Далее из контекста; изменения по месту — PlayerEvent::Queue
    pub queue: Arc<Vec<QueueRow>>,
    /// сколько секунд текущего трека реально проиграно (история)
    pub played_seconds: f64,

    // --- Spotify Connect ---
    pub target: Target,
    /// активное устройство (None — ничего не играет нигде)
    pub device: Option<DeviceItem>,
    /// играет этот ПК (librespot)
    pub is_local: bool,
    pub local: LocalPlayback,
    /// разрешено ли управлять (!device.is_restricted)
    pub can_control: bool,
    pub can_set_volume: bool,
    /// идёт перенос воспроизведения (сюда или туда)
    pub transferring: bool,
    /// удалённое устройство пропало (2 опроса без него или 404)
    pub device_lost: bool,
    /// «Ещё N треков» в секции контекста
    pub context_remaining: i32,
    /// число строк section=manual
    pub manual_count: i32,
    /// «Очистить» доступно (только локально и есть ручные строки)
    pub can_clear_queue: bool,
    /// перетаскивание внутри «Далее в очереди» (только локально)
    pub queue_reorderable: bool,
}

impl PlayerSnapshot {
    pub fn has_track(&self) -> bool {
        self.track.is_some()
    }

    /// Player.source: context.type ("radio" | "liked" | "playlist" | …)
    pub fn source(&self) -> &str {
        &self.context.kind
    }

    pub fn position(&self) -> f64 {
        if self.duration_ms > 0 { self.position_ms as f64 / self.duration_ms as f64 } else { 0.0 }
    }

    pub fn is_episode(&self) -> bool {
        self.track.as_ref().is_some_and(|t| t.is_episode)
    }
}

/// Изменение очереди — мост повторяет его в модели, чтобы список не терял прокрутку.
#[derive(Clone, Debug)]
pub enum QueueChange {
    Reset(Arc<Vec<QueueRow>>),
    Insert { at: usize, rows: Vec<QueueRow> },
    Remove { at: usize, count: usize },
    Move { from: usize, to: usize },
}

#[derive(Clone, Debug)]
pub struct TrackStarted {
    pub track: TrackItem,
    pub context: PlayContext,
    pub local: bool,
}

#[derive(Clone, Debug)]
pub struct TrackEnded {
    pub track: TrackItem,
    pub context: PlayContext,
    /// секунды
    pub played: f64,
    pub end: f64,
    /// доигран сам, а не пропущен
    pub natural: bool,
    pub local: bool,
}

/// События — по одному на каждый Qt-сигнал Player + служебные.
#[derive(Clone, Debug)]
pub enum PlayerEvent {
    TrackChanged,
    StateChanged,
    PositionChanged,
    VolumeChanged,
    ModeChanged,
    QueueChanged,
    CoverFileChanged,
    ErrorChanged,
    LevelChanged,
    /// устройство/цель/перенос/remote изменились
    DeviceChanged,
    /// локальный движок: подключение/готов/ошибка
    LocalChanged,
    Queue(QueueChange),
    TrackStarted(TrackStarted),
    TrackEnded(TrackEnded),
    /// короткое сообщение пользователю («Очередь очищена», «Pixel 9 не ответил»)
    Notice(String),
}

pub struct PlayerSubscription {
    pub snapshot: watch::Receiver<PlayerSnapshot>,
    pub events: broadcast::Receiver<PlayerEvent>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("{0}")]
    Engine(String),
    #[error("Spotify Premium required")]
    PremiumRequired,
    #[error("no playback device")]
    NoDevice,
}

/// Подпись кодека: Ogg Vorbis с битрейтом librespot.
pub fn codec_label(bitrate_kbps: u16) -> String {
    format!("OGG {bitrate_kbps}")
}

/// Громкость librespot (0..=65535) ↔ 0..1 и ↔ проценты (spotifast app.rs volume_to_percent/percent_to_volume).
pub fn volume_to_u16(volume: f64) -> u16 {
    (volume.clamp(0.0, 1.0) * f64::from(u16::MAX)).round() as u16
}

pub fn volume_from_u16(volume: u16) -> f64 {
    f64::from(volume) / f64::from(u16::MAX)
}
