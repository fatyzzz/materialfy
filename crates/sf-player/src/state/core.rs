//! Ядро «что показано сейчас» (перенос логики spotifast/src/app.rs без egui): цель Local/Remote,
//! очередь по правилам docs/_reference/queue.md, оптимистичные удержания (intent/pending/seq), опрос /me/player.
//! Синхронное и без ввода-вывода: команды и ответы меняют состояние, наружу уходят [`Effect`]; актор их исполняет.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use md3_model::{DeviceItem, PlayContext, QueueRow, QueueSection, TrackItem, contexts};
use sf_api::{ApiError, PlayRequest};
use sf_api::models::Device;
use sf_api::util::uri_kind;

use super::map;
use super::session::SessionData;
use crate::engine::{LoadSpec, LocalState, Playback, PlayerCommand, RepeatMode};
use crate::{
    ASSUMED_CONTEXT_HOLD_MS, INTENT_CEILING_MS, LocalPlayback, OPTIMISTIC_HOLD_MS, PLAYBACK_HOLD_MS,
    QUEUE_ADD_DEBOUNCE_MS, QUEUE_RECHECK_MS, QUEUE_STALE_RETRIES, REMOTE_POLL_ACTIVE_S, REMOTE_POLL_IDLE_S,
    REMOTE_POLL_UNSEEN_S, REMOTE_RECHECK_MS, RESTART_BEFORE_PREVIOUS_MS, Target, TrackEnded, TrackStarted,
};

pub(crate) const REMOTE_POLL_ACTIVE: Duration = Duration::from_secs(REMOTE_POLL_ACTIVE_S);
pub(crate) const REMOTE_POLL_IDLE: Duration = Duration::from_secs(REMOTE_POLL_IDLE_S);
pub(crate) const REMOTE_POLL_UNSEEN: Duration = Duration::from_secs(REMOTE_POLL_UNSEEN_S);
pub(crate) const REMOTE_FRESH: Duration = Duration::from_secs(45);
pub(crate) const OPTIMISTIC_HOLD: Duration = Duration::from_millis(OPTIMISTIC_HOLD_MS);
pub(crate) const PLAYBACK_HOLD: Duration = Duration::from_millis(PLAYBACK_HOLD_MS);
pub(crate) const INTENT_CEILING: Duration = Duration::from_millis(INTENT_CEILING_MS);
pub(crate) const ASSUMED_CONTEXT_HOLD: Duration = Duration::from_millis(ASSUMED_CONTEXT_HOLD_MS);
pub(crate) const REMOTE_RECHECK: Duration = Duration::from_millis(REMOTE_RECHECK_MS);
pub(crate) const QUEUE_RECHECK: Duration = Duration::from_millis(QUEUE_RECHECK_MS);
pub(crate) const QUEUE_ADD_DEBOUNCE: Duration = Duration::from_millis(QUEUE_ADD_DEBOUNCE_MS);
pub(crate) const SESSION_REFRESH: Duration = Duration::from_secs(30);
pub(crate) const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);
/// Тишина после последнего движения ползунка громкости до Volume (librespot сообщает Connect-клиентам).
pub(crate) const VOLUME_SETTLE: Duration = Duration::from_millis(150);
/// Сколько ждать подтверждения переноса на устройство.
pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(8);
/// Как часто перечитывать очередь, пока играет музыка и окно видно.
const QUEUE_REFRESH: Duration = Duration::from_secs(30);
const PENDING_ADD_LIFETIME: Duration = Duration::from_secs(30);
const TRACK_CACHE_LIMIT: usize = 2000;
const KNOWN_CONTEXTS: usize = 200;
/// Сколько ждать обещанный движок (ключи из хранилища, проверка /me, подключение librespot).
const LOCAL_EXPECT_HOLD: Duration = Duration::from_secs(45);
/// Сколько играет текущий трек, прежде чем тянуть следующую песню: сначала пусть скачается он сам.
const PRELOAD_AFTER_MS: u32 = 3_000;
/// Опросы без устройства, после которых удалённое устройство считается пропавшим.
const LOST_AFTER_POLLS: u8 = 2;
/// Smart Shuffle: /context-resolve с этой линзой отдаёт плейлист вместе с рекомендациями Spotify
/// (заголовком линза в context-resolve не работает, только параметром).
const SMART_LENS: &str = "?spotify-apply-lenses=enhance";

pub const MAX_LIST: usize = 500;

/// Команды Web API для удалённого устройства (и чтения для обоих режимов).
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteAction {
    Play,
    Pause,
    Next,
    Previous,
    Seek(u32),
    Volume(u8),
    Shuffle(bool),
    Repeat(RepeatMode),
}

impl RemoteAction {
    fn label(&self) -> String {
        match self {
            Self::Play => mf_i18n::t("Playback"),
            Self::Pause => mf_i18n::t("Pause"),
            Self::Next => mf_i18n::t("Next track"),
            Self::Previous => mf_i18n::t("Previous track"),
            Self::Seek(_) => mf_i18n::t("Seek"),
            Self::Volume(_) => mf_i18n::t("Volume"),
            Self::Shuffle(_) => mf_i18n::t("Shuffling"),
            Self::Repeat(_) => mf_i18n::t("Repeat"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiCall {
    Playback { seq: u64 },
    Queue { seq: u64 },
    Remote { action: RemoteAction, device: Option<String> },
    /// `shuffle` — сначала включить перемешивание, затем играть (одним упорядоченным обменом)
    Play { device: Option<String>, request: PlayRequest, shuffle: bool },
    Transfer { device: String, play: bool },
    AddMany { request: u64, uris: Vec<String>, device: Option<String> },
    /// все треки альбома по порядку (все страницы) или ошибка
    AlbumTracks { request: u64, id: String },
    /// строка трека/выпуска по URI: списки Web API не всегда кладут её в кэш ядра, а кликнутый трек нужен сразу
    TrackInfo { request: u64, uri: String },
}

#[derive(Clone, Debug)]
pub enum ApiReply {
    Playback { seq: u64, result: Result<Option<RemoteState>, ApiError> },
    Queue { seq: u64, result: Result<FetchedQueue, ApiError> },
    Remote { action: RemoteAction, result: Result<(), ApiError> },
    Transferred { device: String, name: Option<String>, result: Result<(), ApiError> },
    QueueBatchAdded { request: u64, added: usize, result: Result<(), ApiError> },
    AlbumTracks { request: u64, result: Result<Vec<TrackItem>, ApiError> },
    TrackInfo { request: u64, uri: String, result: Result<TrackItem, ApiError> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Engine(PlayerCommand),
    Api(ApiCall),
    Notice(String),
    /// пересоздать сессию librespot с подхватом воспроизведения
    Reconnect,
    /// скачать обложку текущего трека ("" — обложки нет)
    Cover(String),
    Save(Box<SessionData>),
    Started(Box<TrackStartedLite>),
    Ended(Box<TrackEndedLite>),
}

/// TrackStarted/TrackEnded без Debug/PartialEq-ограничений внешних типов.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackStartedLite {
    pub track: TrackItem,
    pub context: PlayContext,
    pub local: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackEndedLite {
    pub track: TrackItem,
    pub context: PlayContext,
    pub played: f64,
    pub end: f64,
    pub natural: bool,
    pub local: bool,
}

impl From<TrackStartedLite> for TrackStarted {
    fn from(value: TrackStartedLite) -> Self {
        Self { track: value.track, context: value.context, local: value.local }
    }
}

impl From<TrackEndedLite> for TrackEnded {
    fn from(value: TrackEndedLite) -> Self {
        Self {
            track: value.track,
            context: value.context,
            played: value.played,
            end: value.end,
            natural: value.natural,
            local: value.local,
        }
    }
}

/// Состояние /me/player (уже без serde-моделей, кроме устройства).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteState {
    pub device: Option<Device>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    /// канонический URI контекста
    pub context_uri: Option<String>,
    pub progress_ms: Option<u32>,
    pub is_playing: bool,
    pub item: Option<TrackItem>,
}

impl RemoteState {
    pub fn from_api(state: sf_api::models::PlaybackState) -> Self {
        Self {
            repeat: RepeatMode::from_api(&state.repeat_state),
            shuffle: state.shuffle_state,
            context_uri: state
                .context
                .as_ref()
                .filter(|context| !context.uri.is_empty())
                .map(|context| sf_api::link::canonical_context_uri(&context.uri)),
            progress_ms: state.progress_ms,
            is_playing: state.is_playing,
            item: state.item.as_ref().map(map::playable),
            device: state.device,
        }
    }

    pub fn device_id(&self) -> Option<&str> {
        self.device.as_ref().and_then(|device| device.id.as_deref())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FetchedQueue {
    pub current: Option<TrackItem>,
    pub next: Vec<TrackItem>,
    /// играющий контекст с названием, если источник его знает (карточка «Играет из …»)
    pub context: Option<PlayContext>,
    /// URI рекомендаций Smart Shuffle среди current/next
    pub recommended: Vec<String>,
}

impl FetchedQueue {
    pub fn from_api(queue: sf_api::models::Queue) -> Self {
        Self {
            current: queue.currently_playing.as_ref().map(map::playable),
            next: queue.queue.iter().map(map::playable).collect(),
            context: None,
            recommended: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteSnapshot {
    pub state: RemoteState,
    pub received_at: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QItem {
    pub uid: u64,
    pub track: TrackItem,
}

impl QItem {
    pub fn uri(&self) -> &str {
        &self.track.uri
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueData {
    pub current: Option<QItem>,
    pub next: Vec<QItem>,
}

/// Ручная очередь, которую вернуть в движок, перезапущенный по настройкам, когда он заиграет тот же трек.
struct Requeue {
    track: String,
    uris: Vec<String>,
    /// новый движок готов: состояния старого больше не считаются
    armed: bool,
}

struct AssumedContext {
    uri: String,
    shuffle: Option<bool>,
    at: Instant,
    /// список URI, показанный этим контекстом (радио): пока играет его трек, контекст держится и на паузе
    tracks: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrackConfirmation {
    Local,
    Remote { after_poll: u64, mismatches: u8 },
}

pub(crate) struct TrackIntent {
    pub uri: String,
    pub position_ms: u32,
    pub at: Instant,
    pub confirmation: TrackConfirmation,
    /// что играло здесь, когда трек выбрали: его обновления (догрузилась обложка, длительность) намерение не снимают
    pub from_uri: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct QueueShufflePending {
    current_uri: Option<String>,
    context_uris: Vec<String>,
    pub at: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingQueueAdd {
    pub item: TrackItem,
    pub at: Instant,
    /// позиция в manual_queue; сдвигается, когда уходит строка выше
    pub manual_index: usize,
    /// удалённая запись и место в ней, пока Spotify не принял эту копию
    pub write: Option<(u64, usize)>,
}

pub(crate) struct PendingAlbumQueue {
    label: String,
    target: Target,
}

/// Перенос воспроизведения: None — сюда (librespot Transfer), Some — на устройство.
#[derive(Clone, Debug)]
pub(crate) struct Transfer {
    pub to: Option<String>,
    pub name: String,
    pub at: Instant,
}

struct Listening {
    track: TrackItem,
    context: PlayContext,
    local: bool,
    played: Duration,
    since: Option<Instant>,
    position_ms: u32,
}

/// Что показано как текущее (плеер-бар, NowPlaying).
#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub local: bool,
    pub device: Option<DeviceItem>,
    pub track: TrackItem,
    pub duration_ms: u32,
    pub position_ms: u32,
    pub playing: bool,
    pub loading: bool,
    pub repeat: RepeatMode,
    /// 0..=65535
    pub volume: u16,
    pub can_control: bool,
    pub can_set_volume: bool,
    /// трек прошлой сессии на паузе
    pub resuming: bool,
}

impl NowPlaying {
    pub fn uri(&self) -> &str {
        &self.track.uri
    }
}

pub struct Core {
    // --- локальный движок ---
    pub local_ready: bool,
    pub local_playback: LocalPlayback,
    pub local_device_id: Option<String>,
    pub local_device_name: String,
    pub bitrate: u16,
    pub local: LocalState,
    pub(crate) local_transfer_sequence: Option<u64>,
    pub(crate) local_list: Option<Vec<String>>,
    pub autoplay: bool,
    unavailable_at: Vec<Instant>,
    last_unavailable_reconnect: Option<Instant>,
    /// громкость, которую движок ещё не подтвердил
    pub(crate) pending_local_volume: Option<(u16, Instant)>,
    pub(crate) pending_local_position: Option<(u32, Instant)>,
    /// громкость этого ПК (настройка), 0..=65535
    pub volume: u16,
    volume_settle: Option<(u16, Instant)>,

    // --- удалённое ---
    pub remote: Option<RemoteSnapshot>,
    pub(crate) remote_polled_at: Instant,
    pub(crate) remote_poll_pending: bool,
    pub(crate) remote_poll_seq: u64,
    pub(crate) remote_recheck_at: Option<Instant>,
    pub selected_device: Option<String>,
    pub app_active: bool,
    missing_polls: u8,
    pub device_lost: bool,
    lost: Option<RemoteSnapshot>,
    pub(crate) transfer: Option<Transfer>,
    known_devices: HashMap<String, String>,
    pub(crate) pending_remote_position: Option<(u32, Instant)>,
    pub(crate) pending_remote_volume: Option<(u8, Instant)>,

    // --- удержания ---
    pub(crate) intent_track: Option<TrackIntent>,
    pub(crate) optimistic_playing: Option<(bool, Instant)>,
    assumed_context: Option<AssumedContext>,
    pub(crate) shuffle_wanted: bool,
    pub(crate) shuffle_set_at: Option<Instant>,
    /// Smart Shuffle выбран (действует вместе с shuffle_wanted, только локально и у плейлистов)
    pub(crate) smart_wanted: bool,
    /// рекомендации Smart Shuffle в последней прочитанной очереди
    recommended: Vec<String>,
    pub(crate) queued_play: Option<PlayRequest>,
    /// вход обещал поднять движок (воспроизведение на ПК разрешено): запуск до подключения ждёт его
    local_expected: Option<Instant>,
    queue_start_pending: Option<Target>,
    last_now_playing_uri: Option<String>,
    last_now_playing_sequence: u64,
    /// контекст последнего запуска (подпись и обложка от вызывающего)
    context_info: PlayContext,
    /// подписи и обложки уже запускавшихся контекстов (по URI)
    known_contexts: HashMap<String, PlayContext>,

    // --- очередь ---
    pub queue: Option<QueueData>,
    pub(crate) queue_loading: bool,
    queue_fetched_at: Option<Instant>,
    pub(crate) queue_seq: u64,
    pub(crate) queue_recheck_at: Option<Instant>,
    pub(crate) queue_stale_retries: u32,
    queue_cleared: Option<(HashSet<String>, Instant)>,
    /// (трек, который играет, следующая песня) — что уже заказано вперёд: на новом треке вооружаемся заново
    preloaded: Option<(String, String)>,
    pub(crate) queue_shuffle_pending: Option<QueueShufflePending>,
    pub(crate) queue_reorder_pending: Option<Instant>,
    pub manual_queue: Vec<String>,
    pub(crate) pending_queue_adds: Vec<PendingQueueAdd>,
    pub(crate) pending_album_queues: HashMap<u64, PendingAlbumQueue>,
    pub(crate) pending_queue_batches: HashMap<u64, Target>,
    pub(crate) serial: u64,
    last_album_queue: Option<(String, Instant)>,
    next_uid: u64,
    current_uid: Option<(String, u64)>,

    // --- возобновление ---
    pub resume_context: Option<String>,
    pub resume_track: Option<String>,
    pub resume_position_ms: u32,
    pub resume_queue: Vec<String>,
    requeue: Option<Requeue>,
    resume_back: Vec<String>,
    pub session_dirty: bool,
    pub(crate) last_session_save: Instant,
    persist: bool,

    // --- прочее ---
    tracks: HashMap<String, TrackItem>,
    track_order: Vec<String>,
    /// Запрошенные строки по URI (последний запрос побеждает: старый ответ уже не нужен).
    info_requests: HashMap<String, u64>,
    pub error_text: String,
    pub levels_wanted: bool,
    pub direction: i32,
    listening: Option<Listening>,
    shown_uri: Option<String>,
    pub effects: Vec<Effect>,
}

impl Core {
    pub fn new(volume: f64, persist: bool) -> Self {
        let now = Instant::now();
        Self {
            local_ready: false,
            local_playback: LocalPlayback::Unavailable,
            local_device_id: None,
            local_device_name: String::new(),
            bitrate: 320,
            local: LocalState::default(),
            local_transfer_sequence: None,
            local_list: None,
            autoplay: true,
            unavailable_at: Vec::new(),
            last_unavailable_reconnect: None,
            pending_local_volume: None,
            pending_local_position: None,
            volume: crate::volume_to_u16(volume),
            volume_settle: None,
            remote: None,
            // первый опрос — сразу
            remote_polled_at: now.checked_sub(REMOTE_POLL_UNSEEN).unwrap_or(now),
            remote_poll_pending: false,
            remote_poll_seq: 0,
            remote_recheck_at: None,
            selected_device: None,
            app_active: true,
            missing_polls: 0,
            device_lost: false,
            lost: None,
            transfer: None,
            known_devices: HashMap::new(),
            pending_remote_position: None,
            pending_remote_volume: None,
            intent_track: None,
            optimistic_playing: None,
            assumed_context: None,
            shuffle_wanted: false,
            shuffle_set_at: None,
            smart_wanted: false,
            recommended: Vec::new(),
            queued_play: None,
            local_expected: None,
            queue_start_pending: None,
            last_now_playing_uri: None,
            last_now_playing_sequence: 0,
            context_info: PlayContext::default(),
            known_contexts: HashMap::new(),
            queue: None,
            queue_loading: false,
            queue_fetched_at: None,
            queue_seq: 0,
            queue_recheck_at: None,
            queue_stale_retries: 0,
            queue_cleared: None,
            preloaded: None,
            queue_shuffle_pending: None,
            queue_reorder_pending: None,
            manual_queue: Vec::new(),
            pending_queue_adds: Vec::new(),
            pending_album_queues: HashMap::new(),
            pending_queue_batches: HashMap::new(),
            serial: 0,
            last_album_queue: None,
            next_uid: 1,
            current_uid: None,
            resume_context: None,
            resume_track: None,
            resume_position_ms: 0,
            resume_queue: Vec::new(),
            requeue: None,
            resume_back: Vec::new(),
            session_dirty: false,
            last_session_save: now,
            persist,
            tracks: HashMap::new(),
            track_order: Vec::new(),
            info_requests: HashMap::new(),
            error_text: String::new(),
            levels_wanted: false,
            direction: 1,
            listening: None,
            shown_uri: None,
            effects: Vec::new(),
        }
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn notice(&mut self, text: impl Into<String>) {
        self.effects.push(Effect::Notice(text.into()));
    }

    fn engine(&mut self, command: PlayerCommand) {
        self.effects.push(Effect::Engine(command));
    }

    fn api(&mut self, call: ApiCall) {
        self.effects.push(Effect::Api(call));
    }

    fn uid(&mut self) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        uid
    }

    fn item(&mut self, track: TrackItem) -> QItem {
        QItem { uid: self.uid(), track }
    }

    // ---- кэш треков ---------------------------------------------------------------------------

    pub fn remember(&mut self, track: &TrackItem) {
        if track.uri.is_empty() {
            return;
        }
        let known = self.tracks.get(&track.uri);
        // строка-заглушка без имени не вытесняет известные данные
        if known.is_some_and(|known| !known.title.is_empty()) && track.title.is_empty() {
            return;
        }
        if known.is_none() {
            self.track_order.push(track.uri.clone());
            if self.track_order.len() > TRACK_CACHE_LIMIT {
                let drop = self.track_order.len() - TRACK_CACHE_LIMIT;
                for uri in self.track_order.drain(..drop) {
                    self.tracks.remove(&uri);
                }
            }
        }
        // пришла настоящая строка — запрос больше не нужен; безымянная заглушка его не отменяет
        if !track.title.is_empty() {
            self.info_requests.remove(&track.uri);
        }
        self.tracks.insert(track.uri.clone(), track.clone());
    }

    pub fn cached(&self, uri: &str) -> Option<&TrackItem> {
        self.tracks.get(uri)
    }

    // ---- цель и «что показано» -----------------------------------------------------------------

    pub fn remote_fresh(&self) -> Option<&RemoteSnapshot> {
        self.remote.as_ref().filter(|remote| remote.received_at.elapsed() < REMOTE_FRESH)
    }

    /// Куда идут команды: этот ПК или удалённое устройство.
    pub fn target(&self) -> Target {
        if self.local_ready && self.local.is_active() {
            return Target::Local;
        }
        if let Some(selected) = &self.selected_device
            && Some(selected.as_str()) != self.local_device_id.as_deref()
        {
            return Target::Remote(Some(selected.clone()));
        }
        if let Some(remote) = self.remote_fresh() {
            let device = remote.state.device_id();
            let is_local = device.is_some_and(|id| Some(id) == self.local_device_id.as_deref());
            if !is_local && (remote.state.is_playing || remote.state.item.is_some()) {
                return Target::Remote(device.map(str::to_owned));
            }
        }
        if self.local_ready { Target::Local } else { Target::Remote(None) }
    }

    #[cfg(test)]
    pub(crate) fn age_assumed_context(&mut self, by: Duration) {
        if let Some(assumed) = &mut self.assumed_context {
            assumed.at = assumed.at.checked_sub(by).unwrap_or(assumed.at);
        }
    }

    #[cfg(test)]
    pub(crate) fn age_intent(&mut self, by: Duration) {
        if let Some(intent) = &mut self.intent_track {
            intent.at = intent.at.checked_sub(by).unwrap_or(intent.at);
        }
    }

    /// Контекст, показанный как играющий, включая ещё не подтверждённый запуск.
    pub fn playing_context_uri(&self) -> Option<String> {
        let remote = self.remote.as_ref().and_then(|remote| remote.state.context_uri.clone());
        if let Some(assumed) = &self.assumed_context {
            let held = assumed.at.elapsed() < ASSUMED_CONTEXT_HOLD;
            // ответ опроса, начатого до запуска, ещё рассказывает старую историю
            let contradicted = self.remote.as_ref().is_some_and(|snapshot| {
                snapshot.received_at > assumed.at
                    && snapshot.state.context_uri.as_ref().is_some_and(|uri| *uri != assumed.uri)
            });
            let in_list = !assumed.tracks.is_empty()
                && self.now_playing_live().is_some_and(|now| assumed.tracks.contains(&now.track.uri));
            if held || (!contradicted && (self.believed_playing() || in_list)) {
                return (!assumed.uri.is_empty()).then(|| assumed.uri.clone());
            }
        }
        remote
    }

    /// Трек, показанный текущим, включая недавний неподтверждённый запуск.
    pub fn current_track_uri(&self) -> Option<String> {
        if self.intent_held()
            && let Some(intent) = &self.intent_track
        {
            return Some(intent.uri.clone());
        }
        self.now_playing().map(|now| now.track.uri)
    }

    /// Намерение ещё держится: короткое удержание, а пока движок грузит выбранный трек — до потолка (холодный
    /// старт бывает дольше шести секунд, и на экране появлялся прошлый трек, будто клик не сработал).
    fn intent_held(&self) -> bool {
        let Some(intent) = &self.intent_track else { return false };
        if self.queued_play.is_some() || intent.at.elapsed() < PLAYBACK_HOLD {
            return true;
        }
        intent.at.elapsed() < INTENT_CEILING && matches!(self.target(), Target::Local) && self.local.loading
    }

    pub(crate) fn expect_track(&mut self, uri: String, position_ms: u32) {
        self.want_track_info(&uri);
        let confirmation = match self.target() {
            Target::Local => TrackConfirmation::Local,
            Target::Remote(_) => TrackConfirmation::Remote { after_poll: self.remote_poll_seq, mismatches: 0 },
        };
        self.intent_track = Some(TrackIntent {
            uri,
            position_ms,
            at: Instant::now(),
            confirmation,
            from_uri: self.local.track.as_ref().map(|track| track.uri.clone()),
        });
    }

    /// Метки выбранного трека нужны сразу, а у списков Web API их нет в кэше: экран тогда переключался на
    /// безымянную строку только вместе со звуком. Один запрос на URI, повторный только после ответа.
    /// Строка очереди может быть безымянной заглушкой — такую спрашиваем, как и отсутствующую.
    fn want_track_info(&mut self, uri: &str) {
        let known = self.tracks.get(uri).is_some_and(|track| !track.title.is_empty());
        if uri.is_empty() || known || self.info_requests.contains_key(uri) {
            return;
        }
        let request = self.serial.wrapping_add(1);
        self.serial = request;
        self.info_requests.insert(uri.to_owned(), request);
        self.api(ApiCall::TrackInfo { request, uri: uri.to_owned() });
    }

    fn handle_track_info(&mut self, request: u64, uri: String, result: Result<TrackItem, ApiError>) {
        // пока ждали, трек уже пришёл с другой стороны — ответ не нужен
        if self.info_requests.get(&uri).copied() != Some(request) {
            return;
        }
        self.info_requests.remove(&uri);
        match result {
            Ok(track) => self.remember(&track),
            Err(error) => tracing::debug!("track {uri} is not described: {error}"),
        }
    }

    /// Локальный движок — источник истины: настоящая смена трека побеждает догадку очереди. Настоящая — это
    /// выбранный трек или любой другой, кроме игравшего до выбора (Spotify мог подменить трек версией для своего
    /// рынка). Обновление прежнего трека — не она: иначе на экране мелькал предыдущий трек, пока грузится новый.
    fn reconcile_local_track_intent(&mut self, reported: Option<&str>) {
        let settled = self.intent_track.as_ref().is_some_and(|intent| {
            matches!(intent.confirmation, TrackConfirmation::Local)
                && reported.is_some_and(|uri| uri == intent.uri || Some(uri) != intent.from_uri.as_deref())
        });
        if settled {
            self.intent_track = None;
        }
    }

    /// Опрос, начатый до команды, намерение не снимает; одно несовпадение терпим и переспрашиваем.
    pub(crate) fn reconcile_remote_track_intent(&mut self, poll: u64, reported: Option<&str>) {
        let mut settled = false;
        let mut recheck = false;
        if let Some(intent) = &mut self.intent_track
            && let TrackConfirmation::Remote { after_poll, mismatches } = &mut intent.confirmation
            && poll > *after_poll
        {
            if reported == Some(intent.uri.as_str()) || *mismatches > 0 {
                settled = true;
            } else {
                *after_poll = poll;
                *mismatches += 1;
                recheck = true;
            }
        }
        if settled {
            self.intent_track = None;
        } else if recheck {
            self.remote_recheck_at = Some(Instant::now() + REMOTE_RECHECK);
        }
    }

    pub fn believed_playing(&self) -> bool {
        if let Some((playing, at)) = self.optimistic_playing
            && at.elapsed() < PLAYBACK_HOLD
        {
            return playing;
        }
        self.now_playing().is_some_and(|now| now.playing)
    }

    pub fn now_playing(&self) -> Option<NowPlaying> {
        self.requested_track_preview().or_else(|| self.now_playing_live()).or_else(|| self.resume_preview())
    }

    fn requested_track_preview(&self) -> Option<NowPlaying> {
        if !self.intent_held() {
            return None;
        }
        let intent = self.intent_track.as_ref()?;
        let mut now = self.cached_track_preview(&intent.uri, intent.position_ms)?;
        let actual = self.now_playing_live();
        now.local = matches!(intent.confirmation, TrackConfirmation::Local);
        // Смена трека на этом ПК: пока движок его не отдал, играет не он — и показывать «играет» нельзя,
        // иначе строка идёт вперёд, а звука нет. Оптимистичный ответ нужен только паузе/продолжению
        // того же трека, который уже звучит.
        let switching = now.local && actual.as_ref().is_none_or(|live| live.track.uri != intent.uri);
        now.playing = if switching {
            false
        } else {
            match self.optimistic_playing {
                Some((playing, at)) if at.elapsed() < PLAYBACK_HOLD => playing,
                _ => self.queued_play.is_some() || actual.as_ref().is_some_and(|now| now.playing),
            }
        };
        now.loading = true;
        now.resuming = false;
        if let Some(actual) = actual {
            now.device = actual.device;
            now.repeat = actual.repeat;
            now.volume = actual.volume;
            now.can_control = actual.can_control;
            now.can_set_volume = actual.can_set_volume;
        } else if !now.local {
            now.device = self.remote_fresh().and_then(|remote| self.remote_device(&remote.state));
        } else {
            now.device = self.this_device(true);
        }
        Some(now)
    }

    /// Это устройство как DeviceItem (librespot на этом ПК).
    fn this_device(&self, active: bool) -> Option<DeviceItem> {
        let id = self.local_device_id.clone()?;
        Some(DeviceItem {
            id,
            name: self.local_device_name.clone(),
            kind: "computer".into(),
            is_active: active,
            is_this_computer: true,
            is_restricted: false,
            supports_volume: true,
            volume: i32::from(volume_to_percent(self.local.volume)),
            source: "api".into(),
        })
    }

    fn remote_device(&self, state: &RemoteState) -> Option<DeviceItem> {
        state.device.as_ref().map(|device| map::device(device, self.local_device_id.as_deref()))
    }

    /// Что устройство действительно играет, здесь или где-то ещё.
    pub(crate) fn now_playing_live(&self) -> Option<NowPlaying> {
        if self.local.is_active() && self.local_transfer_sequence != Some(self.local.track_sequence) {
            let track = self.local.track.as_ref()?;
            let item = map::local_track(track, self.tracks.get(&track.uri));
            let playing = match self.optimistic_playing {
                Some((playing, at)) if at.elapsed() < PLAYBACK_HOLD => playing,
                _ => self.local.playback == Playback::Playing,
            };
            let position_ms = match self.pending_local_position {
                Some((position, at)) if at.elapsed() < OPTIMISTIC_HOLD => {
                    if self.local.playback == Playback::Playing {
                        let elapsed = crate::engine::fx::track_elapsed_ms(at.elapsed());
                        position.saturating_add(elapsed).min(track.duration_ms.max(position))
                    } else {
                        position
                    }
                }
                _ => self.local.position_now(),
            };
            return Some(NowPlaying {
                local: true,
                device: self.this_device(true),
                duration_ms: track.duration_ms,
                track: item,
                position_ms,
                playing,
                // Playback держится Playing на время загрузки следующего трека; спиннер — по loading
                loading: self.local.loading,
                repeat: self.local.repeat,
                volume: self.local.volume,
                can_control: true,
                can_set_volume: true,
                resuming: false,
            });
        }
        let Some(remote) = self.remote_fresh() else {
            return self.lost_preview();
        };
        // снимок про этот ПК при пустом движке может описывать уже остановленное
        if self.local_device_id.is_some() && remote.state.device_id() == self.local_device_id.as_deref() {
            return None;
        }
        let item = remote.state.item.as_ref()?;
        let device = remote.state.device.as_ref();
        let playing = match self.optimistic_playing {
            Some((playing, at)) if at.elapsed() < PLAYBACK_HOLD => playing,
            _ => remote.state.is_playing,
        };
        let duration = item.duration_ms.max(0) as u32;
        let position = match self.pending_remote_position {
            Some((position, at)) if at.elapsed() < OPTIMISTIC_HOLD => position,
            _ => {
                let base = remote.state.progress_ms.unwrap_or(0);
                if remote.state.is_playing {
                    (u64::from(base) + remote.received_at.elapsed().as_millis() as u64).min(u64::from(duration))
                        as u32
                } else {
                    base
                }
            }
        };
        let percent = match self.pending_remote_volume {
            Some((volume, at)) if at.elapsed() < OPTIMISTIC_HOLD => volume,
            _ => device.and_then(|device| device.volume_percent).unwrap_or(50),
        };
        Some(NowPlaying {
            local: false,
            device: self.remote_device(&remote.state),
            track: item.clone(),
            duration_ms: duration,
            position_ms: position,
            playing,
            loading: false,
            repeat: remote.state.repeat,
            volume: percent_to_volume(percent),
            can_control: device.is_none_or(|device| !device.is_restricted),
            can_set_volume: device.is_none_or(|device| !device.is_restricted && device.supports_volume != Some(false)),
            resuming: false,
        })
    }

    /// Пропавшее устройство: последний трек на паузе, чтобы предложить продолжить здесь.
    fn lost_preview(&self) -> Option<NowPlaying> {
        if !self.device_lost {
            return None;
        }
        let lost = self.lost.as_ref()?;
        let item = lost.state.item.as_ref()?;
        Some(NowPlaying {
            local: false,
            device: self.remote_device(&lost.state),
            track: item.clone(),
            duration_ms: item.duration_ms.max(0) as u32,
            position_ms: self.lost_position(),
            playing: false,
            loading: false,
            repeat: lost.state.repeat,
            volume: self.volume,
            can_control: false,
            can_set_volume: false,
            resuming: false,
        })
    }

    fn lost_position(&self) -> u32 {
        self.lost.as_ref().map_or(0, |lost| {
            let base = lost.state.progress_ms.unwrap_or(0);
            if lost.state.is_playing {
                let duration = lost
                    .state
                    .item
                    .as_ref()
                    .map(|item| item.duration_ms.max(0) as u32)
                    .filter(|duration| *duration > 0)
                    .unwrap_or(u32::MAX);
                (u64::from(base) + lost.received_at.elapsed().as_millis() as u64).min(u64::from(duration)) as u32
            } else {
                base
            }
        })
    }

    fn resume_preview(&self) -> Option<NowPlaying> {
        let uri = self.resume_track.as_deref()?;
        self.cached_track_preview(uri, self.resume_position_ms)
    }

    fn cached_track_preview(&self, uri: &str, position_ms: u32) -> Option<NowPlaying> {
        let track = self.tracks.get(uri)?;
        let duration = track.duration_ms.max(0) as u32;
        Some(NowPlaying {
            local: true,
            device: None,
            track: track.clone(),
            duration_ms: duration,
            position_ms: if duration > 0 { position_ms.min(duration) } else { position_ms },
            playing: false,
            loading: false,
            repeat: RepeatMode::Off,
            volume: self.volume,
            can_control: true,
            can_set_volume: true,
            resuming: true,
        })
    }

    /// Плеер-бар показывает трек прошлой сессии без живого воспроизведения.
    pub fn resume_only(&self) -> bool {
        self.resume_track.is_some() && self.now_playing_live().is_none()
    }

    // ---- локальный движок -------------------------------------------------------------------------

    /// Движок скоро будет (старт с разрешённым воспроизведением) или уже не будет.
    pub fn expect_local(&mut self, expected: bool) {
        if expected {
            self.local_expected = Some(Instant::now());
            return;
        }
        self.local_expected = None;
        if !self.local_ready
            && !matches!(self.local_playback, LocalPlayback::Connecting)
            && self.queued_play.take().is_some()
        {
            self.queue_start_pending = None;
            self.intent_track = None;
            self.assumed_context = None;
            self.notice(mf_i18n::t("Choose a device or turn on playback on this computer"));
        }
    }

    fn local_coming(&self) -> bool {
        self.local_expected.is_some_and(|at| at.elapsed() < LOCAL_EXPECT_HOLD)
    }

    pub fn handle_playback(&mut self, status: LocalPlayback) {
        if matches!(status, LocalPlayback::Ready { .. } | LocalPlayback::Failed(_)) {
            self.local_expected = None;
        }
        match &status {
            LocalPlayback::Ready { .. } => {
                if let Some(requeue) = &mut self.requeue {
                    requeue.armed = true;
                }
            }
            LocalPlayback::Failed(_) | LocalPlayback::Unavailable => self.requeue = None,
            LocalPlayback::Connecting => {}
        }
        match &status {
            LocalPlayback::Ready { device_id } => {
                self.local_device_id = Some(device_id.clone());
                self.local_ready = true;
                self.error_text.clear();
                if let Some(request) = self.queued_play.take() {
                    self.play_request(request, false);
                }
            }
            LocalPlayback::Unavailable => {
                self.local_ready = false;
                self.local_device_id = None;
                self.local = LocalState::default();
            }
            LocalPlayback::Failed(message) => {
                self.local_ready = false;
                self.local = LocalState::default();
                if self.queued_play.take().is_some() {
                    self.intent_track = None;
                }
                self.error_text = message.clone();
                self.notice(mf_i18n::tf("Playback on this computer: {0}", &[message]));
            }
            LocalPlayback::Connecting => {}
        }
        self.local_playback = status;
    }

    pub fn handle_local(&mut self, state: LocalState) {
        if self.local_transfer_sequence.is_some_and(|sequence| sequence != state.track_sequence) {
            self.local_transfer_sequence = None;
            // перенос сюда подтверждён
            if self.transfer.as_ref().is_some_and(|transfer| transfer.to.is_none()) {
                self.transfer = None;
            }
        }
        let track_changed = state.track != self.local.track || state.track_sequence != self.local.track_sequence;
        let reconnected = state.connected && !self.local.connected;
        if state.shuffle != self.local.shuffle && self.shuffle_set_at.is_none_or(|at| at.elapsed() > Duration::from_secs(5))
        {
            // смена shuffle другим клиентом
            self.shuffle_wanted = state.shuffle;
        }
        if state.playback != self.local.playback {
            self.optimistic_playing = None;
        }
        self.reconcile_local_track_intent(state.track.as_ref().map(|track| track.uri.as_str()));
        let held_volume = self.held_local_volume(state.volume);
        if held_volume.is_none() {
            self.volume = state.volume;
        }
        if state.seek_sequence != self.local.seek_sequence || track_changed {
            self.pending_local_position = None;
        }
        if let Some(error) = &state.error
            && self.local.error.as_deref() != Some(error.as_str())
        {
            // Недоступный трек librespot пропускает сам (spirc: Unavailable текущего → следующий), и без
            // объяснения это выглядит как самовольная смена песни. Называем трек и говорим, что дальше.
            let text = match error.strip_prefix(crate::engine::player::UNAVAILABLE) {
                Some(uri) => {
                    let uri = uri.trim().trim_start_matches(':').trim();
                    let title = self.cached(uri).map(|track| track.title.clone()).unwrap_or_default();
                    if title.is_empty() {
                        mf_i18n::t("This track isn't available — moving on")
                    } else {
                        mf_i18n::tf("“{0}” isn't available — moving on", &[&title])
                    }
                }
                None => engine_error_text(error),
            };
            self.notice(text);
            // Испорченная сессия librespot лечится новой (воспроизведение подхватывается с той же позиции):
            // ключ не выдан — сразу (Spotify перестаёт выдавать ключи этой сессии, у новой они есть: в РФ без
            // VPN — минут через десять); «недоступно» — после трёх подряд (один бывает и по делу: регион).
            // Раньше проверялся старый текст ошибки, которого движок уже не пишет, — переподключения не было.
            let key_refused = error == crate::engine::player::AUDIO_KEY_REFUSED;
            if key_refused || error.starts_with(crate::engine::player::UNAVAILABLE) {
                let now = Instant::now();
                self.unavailable_at.retain(|at| now.duration_since(*at) < Duration::from_secs(20));
                self.unavailable_at.push(now);
                if (key_refused || self.unavailable_at.len() >= 3)
                    && self.last_unavailable_reconnect.is_none_or(|at| at.elapsed() > Duration::from_secs(30))
                {
                    self.unavailable_at.clear();
                    self.last_unavailable_reconnect = Some(now);
                    self.effects.push(Effect::Reconnect);
                    self.notice(mf_i18n::t("Spotify audio dropped out. Reconnecting playback…"));
                }
            }
        }
        if error_is_no_device(state.error.as_deref()) {
            self.error_text = mf_i18n::t("No playback device");
        } else if error_is_no_device(self.local.error.as_deref()) && state.error.is_none() {
            self.error_text.clear();
        }
        if let Some(seed) = autoplay_seed(self.local_list.as_deref(), self.autoplay, &self.local, &state) {
            tracing::info!("the list ended; continuing with Spotify autoplay from {seed}");
            self.local_list = None;
            self.engine(PlayerCommand::Load(LoadSpec {
                context_uri: Some(seed),
                play: true,
                autoplay: true,
                ..LoadSpec::default()
            }));
        }
        if state.is_active() {
            if self.device_lost {
                self.device_lost = false;
                self.lost = None;
            }
            if self.selected_device.is_some() && self.local_transfer_sequence.is_none() && track_changed {
                self.selected_device = None;
            }
        }
        self.local = state;
        if let Some(volume) = held_volume {
            self.local.volume = volume;
        }
        if track_changed {
            self.on_now_playing_changed();
        }
        if reconnected && let Some(request) = self.queued_play.take() {
            self.play_request(request, false);
        }
        self.requeue_after_restart();
    }

    /// Перезапуск движка по настройкам: что загрузить в новый (контекст или список, трек, позиция, режимы).
    /// None — пусть движок подхватит сам (один трек). Ручная очередь вернётся в новый движок на том же треке.
    pub(crate) fn prepare_restart(&mut self) -> Option<LoadSpec> {
        self.requeue = None;
        if !self.local.is_active() {
            return None;
        }
        let track = self.local.track.as_ref()?.uri.clone();
        let uris: Vec<String> = self
            .manual_queue
            .iter()
            .filter(|uri| matches!(uri_kind(uri), Some("track" | "episode")))
            .cloned()
            .collect();
        if !uris.is_empty() {
            self.requeue = Some(Requeue { track: track.clone(), uris, armed: false });
        }
        let mut spec = match &self.local_list {
            Some(list) if list.contains(&track) => {
                LoadSpec { uris: list.clone(), offset_uri: Some(track), ..LoadSpec::default() }
            }
            _ => {
                let context = self
                    .playing_context_uri()
                    .filter(|uri| !matches!(uri_kind(uri), Some("track" | "episode" | "station")))?;
                LoadSpec {
                    context_uri: Some(context),
                    offset_uri: Some(track),
                    shuffle: Some(self.shuffle_wanted),
                    ..LoadSpec::default()
                }
            }
        };
        spec.position_ms = self.local.position_now();
        spec.play = matches!(self.local.playback, Playback::Playing | Playback::Loading);
        spec.repeat = Some(self.local.repeat);
        Some(spec)
    }

    fn requeue_after_restart(&mut self) {
        let Some(requeue) = &self.requeue else { return };
        if !requeue.armed || !self.local.connected || !self.local.is_active() {
            return;
        }
        let Some(requeue) = self.requeue.take() else { return };
        if self.local.track.as_ref().is_some_and(|track| track.uri == requeue.track) {
            for uri in requeue.uris {
                self.engine(PlayerCommand::AddToQueue(uri));
            }
            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
        }
    }

    /// Громкость, которую движок ещё не подтвердил (пока держится удержание).
    fn held_local_volume(&mut self, reported: u16) -> Option<u16> {
        match self.pending_local_volume {
            Some((volume, at)) if volume != reported && at.elapsed() < OPTIMISTIC_HOLD => Some(volume),
            _ => {
                self.pending_local_volume = None;
                None
            }
        }
    }

    /// Трек сменился (подтверждённо): потребление очереди, точка возобновления, история.
    pub(crate) fn on_now_playing_changed(&mut self) {
        // очередь и сессия следуют за подтверждённым воспроизведением, а не за оптимистичным показом
        let Some(now) = self.now_playing_live() else {
            return;
        };
        let same_uri = self.last_now_playing_uri.as_deref() == Some(now.uri());
        let new_occurrence = now.local && self.last_now_playing_sequence != self.local.track_sequence;
        if same_uri && !new_occurrence {
            return;
        }
        self.session_dirty = true;
        let queue_already_updated = self.queue_start_pending.take().as_ref() == Some(&self.target());
        let repeating = same_uri && new_occurrence && now.repeat == RepeatMode::Track;
        // сохранённая очередь возвращается только с тем же треком
        if !self.resume_queue.is_empty() {
            let queued = std::mem::take(&mut self.resume_queue);
            if now.local && self.resume_track.as_deref() == Some(now.uri()) {
                for uri in queued {
                    self.manual_queue.push(uri.clone());
                    if uri.starts_with("spotify:track:") || uri.starts_with("spotify:episode:") {
                        self.engine(PlayerCommand::AddToQueue(uri));
                    }
                }
                self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
            }
        }
        // старт контекста может начать тот же трек, что голова ручной очереди, не потребляя её;
        // явные пропуски уже сняли свои строки — здесь только непрошеный переход
        if !queue_already_updated && !repeating {
            self.consume_manual_queue_head(&now.track.uri);
        }
        if !queue_already_updated
            && !repeating
            && let Some(queue) = &mut self.queue
        {
            let accounted =
                queue.current.as_ref().is_some_and(|item| item.uri() == now.uri()) && !(same_uri && new_occurrence);
            if !accounted && queue.next.first().is_some_and(|item| item.uri() == now.uri()) {
                let item = queue.next.remove(0);
                queue.current = Some(item);
            }
        }
        self.last_now_playing_uri = Some(now.uri().to_owned());
        if now.local {
            self.last_now_playing_sequence = self.local.track_sequence;
        }
        self.resume_context = self.playing_context_uri();
        self.resume_track = Some(now.uri().to_owned());
        self.resume_position_ms = 0;
        self.resume_back.clear();
        let track = now.track.clone();
        self.remember(&track);
        self.start_listening(&now);
        // свежая очередь при смене трека
        self.refresh_queue(true);
    }

    // ---- история ----------------------------------------------------------------------------------

    fn start_listening(&mut self, now: &NowPlaying) {
        self.finish_listening(false);
        let context = self.shown_context();
        self.listening = Some(Listening {
            track: now.track.clone(),
            context: context.clone(),
            local: now.local,
            played: Duration::ZERO,
            since: now.playing.then(Instant::now),
            position_ms: now.position_ms,
        });
        self.effects.push(Effect::Started(Box::new(TrackStartedLite {
            track: now.track.clone(),
            context,
            local: now.local,
        })));
    }

    fn finish_listening(&mut self, _closing: bool) {
        let Some(mut listening) = self.listening.take() else {
            return;
        };
        if let Some(since) = listening.since.take() {
            listening.played += since.elapsed();
        }
        let end = listening.track.duration_ms.max(0) as f64 / 1000.0;
        let natural = listening.track.duration_ms > 0
            && i64::from(listening.position_ms) + 3_000 >= listening.track.duration_ms;
        self.effects.push(Effect::Ended(Box::new(TrackEndedLite {
            track: listening.track,
            context: listening.context,
            played: listening.played.as_secs_f64(),
            end,
            natural,
            local: listening.local,
        })));
    }

    /// Учёт прослушивания: только подтверждённое воспроизведение.
    fn note_listening(&mut self) {
        let live = self.now_playing_live();
        let Some(listening) = &mut self.listening else {
            return;
        };
        let Some(live) = live.filter(|live| live.track.uri == listening.track.uri) else {
            if let Some(since) = listening.since.take() {
                listening.played += since.elapsed();
            }
            return;
        };
        listening.position_ms = live.position_ms;
        match (live.playing, listening.since) {
            (true, None) => listening.since = Some(Instant::now()),
            (false, Some(since)) => {
                listening.played += since.elapsed();
                listening.since = None;
            }
            _ => {}
        }
    }

    pub fn played_seconds(&self) -> f64 {
        self.listening.as_ref().map_or(0.0, |listening| {
            (listening.played + listening.since.map_or(Duration::ZERO, |since| since.elapsed())).as_secs_f64()
        })
    }

    // ---- таймеры ----------------------------------------------------------------------------------

    pub fn poll_interval(&self) -> Duration {
        let unseen = !self.app_active && !self.now_playing().is_some_and(|now| now.playing);
        // нигде ничего не играет: часто спрашивать не о чем (своё «Играть» опрашивает сразу, poll_remote_soon)
        let silent = !self.now_playing().is_some_and(|now| now.playing || now.loading);
        match self.target() {
            Target::Local if self.local.is_active() => REMOTE_POLL_IDLE,
            _ if unseen => REMOTE_POLL_UNSEEN,
            _ if silent => REMOTE_POLL_IDLE,
            _ => REMOTE_POLL_ACTIVE,
        }
    }

    /// Ближайший момент, когда tick что-то сделает.
    pub fn next_deadline(&self) -> Instant {
        let mut due = self.remote_polled_at + self.poll_interval();
        if self.remote_poll_pending {
            due = Instant::now() + REMOTE_POLL_UNSEEN;
        }
        for at in [
            self.remote_recheck_at,
            self.queue_recheck_at,
            self.volume_settle.map(|(_, at)| at + VOLUME_SETTLE),
            self.transfer.as_ref().map(|transfer| transfer.at + TRANSFER_TIMEOUT),
            self.session_dirty.then_some(self.last_session_save + SESSION_SAVE_DELAY),
        ]
        .into_iter()
        .flatten()
        {
            due = due.min(at);
        }
        due
    }

    /// Нужен ли частый пересчёт снимка (позиция идёт, держатся удержания).
    pub fn animating(&self) -> bool {
        self.now_playing().is_some_and(|now| now.playing || now.loading)
            || self.intent_track.is_some()
            || self.optimistic_playing.is_some()
            || self.assumed_context.as_ref().is_some_and(|assumed| assumed.at.elapsed() < ASSUMED_CONTEXT_HOLD)
            || self.pending_local_position.is_some()
            || self.pending_remote_position.is_some()
            || self.pending_remote_volume.is_some()
            || self.queued_play.is_some()
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        // движок вот-вот поднимется: состояние скоро придёт из кластера сессии, а не из Web API
        let engine_coming = !self.local_ready
            && (self.local_coming() || matches!(self.local_playback, LocalPlayback::Connecting));
        if !self.remote_poll_pending && !engine_coming && self.remote_polled_at.elapsed() >= self.poll_interval() {
            self.poll_remote();
        }
        if let Some(due) = self.remote_recheck_at
            && now >= due
        {
            self.remote_recheck_at = None;
            self.poll_remote();
        }
        if let Some(due) = self.queue_recheck_at
            && now >= due
        {
            self.queue_recheck_at = None;
            self.refresh_queue(true);
        }
        if let Some((volume, at)) = self.volume_settle
            && now >= at + VOLUME_SETTLE
        {
            self.volume_settle = None;
            self.settle_volume(volume);
        }
        if let Some(transfer) = &self.transfer
            && now >= transfer.at + TRANSFER_TIMEOUT
        {
            let name = if transfer.name.is_empty() { mf_i18n::t("Device") } else { transfer.name.clone() };
            self.transfer = None;
            self.local_transfer_sequence = None;
            self.notice(mf_i18n::tf("{0} didn't respond", &[&name]));
        }
        if let Some((_, at)) = self.optimistic_playing
            && at.elapsed() >= PLAYBACK_HOLD
        {
            self.optimistic_playing = None;
        }
        // Выбранный трек держится на экране, пока движок его грузит, а не 6 с по часам: холодный старт дольше,
        // и на экране появлялся прошлый трек, будто клик не сработал. Потолок — чтобы спиннер не висел вечно.
        if self.intent_track.is_some() && !self.intent_held() {
            self.intent_track = None;
        }
        if self.pending_local_position.is_some_and(|(_, at)| at.elapsed() >= OPTIMISTIC_HOLD) {
            self.pending_local_position = None;
        }
        if self.pending_remote_position.is_some_and(|(_, at)| at.elapsed() >= OPTIMISTIC_HOLD) {
            self.pending_remote_position = None;
        }
        if self.pending_remote_volume.is_some_and(|(_, at)| at.elapsed() >= OPTIMISTIC_HOLD) {
            self.pending_remote_volume = None;
        }
        // очередь могла измениться с другого устройства
        if self.app_active
            && !self.queue_loading
            && self.now_playing().is_some_and(|now| now.playing)
            && self.queue_fetched_at.is_none_or(|at| at.elapsed() > QUEUE_REFRESH)
        {
            self.refresh_queue(false);
        }
        self.note_listening();
        // точка возобновления движется, пока играет музыка
        if self.now_playing().is_some_and(|now| now.playing) && self.last_session_save.elapsed() > SESSION_REFRESH {
            self.session_dirty = true;
        }
        if self.session_dirty && self.last_session_save.elapsed() > SESSION_SAVE_DELAY {
            self.save_session();
        }
        self.preload_next_track();
        self.check_shown_track();
    }

    /// Новая обложка при смене показанного трека; направление анимации.
    fn check_shown_track(&mut self) {
        let shown = self.now_playing();
        let uri = shown.as_ref().map(|now| now.track.uri.clone());
        if uri == self.shown_uri {
            return;
        }
        self.shown_uri = uri;
        // пустой URL — обложки нет (тема возвращается к своей)
        let url = shown
            .map(|now| if now.track.cover_large.is_empty() { now.track.cover } else { now.track.cover_large })
            .unwrap_or_default();
        self.effects.push(Effect::Cover(url));
    }

    /// Тянет следующую песню очереди заранее, пока играет текущая: переход (или Next) тогда начинается без
    /// ожидания загрузки — librespot держит впереди только один трек, поэтому заказываем голову очереди, а на
    /// новом треке вооружаемся заново. На удалённом устройстве загрузкой распоряжается оно само.
    fn preload_next_track(&mut self) {
        if !matches!(self.target(), Target::Local) {
            return;
        }
        // движок, а не оптимистичный превью: пока трек ещё качается, «играет» врёт
        let Some(now) = self.now_playing_live() else {
            return;
        };
        // пока трек сам ещё качается, вперёд не заказываем — иначе следующая песня отбирает полосу
        // у той, что вот-вот зазвучит, и старт затягивается на ровном месте
        if !now.playing || now.loading || now.position_ms < PRELOAD_AFTER_MS {
            return;
        }
        let next = self
            .queue
            .as_ref()
            .and_then(|queue| queue.next.first())
            .map(|item| item.uri().to_owned())
            .filter(|uri| !uri.is_empty() && *uri != now.track.uri);
        let Some(next) = next else {
            return;
        };
        let current = now.track.uri.clone();
        let ordered = self.preloaded.as_ref().is_some_and(|(was, want)| *was == current && *want == next);
        if ordered {
            return;
        }
        self.preloaded = Some((current, next.clone()));
        self.engine(PlayerCommand::Preload(next));
    }

    pub(crate) fn poll_remote(&mut self) {
        self.remote_poll_pending = true;
        self.remote_polled_at = Instant::now();
        self.remote_poll_seq += 1;
        self.api(ApiCall::Playback { seq: self.remote_poll_seq });
    }

    /// Следующий опрос — не позже чем через 700 мс, каким бы ни был интервал.
    pub(crate) fn poll_remote_soon(&mut self) {
        let now = Instant::now();
        self.remote_polled_at = now
            .checked_sub(REMOTE_POLL_UNSEEN)
            .map_or(now, |at| at + Duration::from_millis(700));
    }

    pub fn refresh_queue(&mut self, force: bool) {
        if self.resume_only() && self.queue.is_some() {
            // нигде ничего не играет; на экране сохранённая очередь — запрос вернул бы меньше
            return;
        }
        if self.queue_loading && !force {
            return;
        }
        self.queue_loading = true;
        self.queue_fetched_at = Some(Instant::now());
        self.queue_seq += 1;
        self.api(ApiCall::Queue { seq: self.queue_seq });
    }

    // ---- очередь ----------------------------------------------------------------------------------

    /// Клик по строке «Далее»: играет сразу, строки выше уходят (N×Next).
    pub(crate) fn play_queue_item(&mut self, index: usize, uri: String) {
        if self.resume_only() {
            // живой очереди нет — играем показанные строки списком
            let uris: Vec<String> = self
                .queue
                .as_ref()
                .map(|queue| queue.next.iter().map(|item| item.uri().to_owned()).collect())
                .unwrap_or_default();
            if uris.is_empty() {
                return;
            }
            let index = uris.iter().position(|held| *held == uri).unwrap_or(index);
            let (uris, index) = cap_uris(&uris, index as u32);
            self.play_request(PlayRequest::tracks(uris).starting_at_index(index), false);
            return;
        }
        let mut skips = index + 1;
        let mut consumed: Vec<String> = Vec::new();
        if let Some(queue) = &mut self.queue {
            // клик называет песню: если строки сдвинулись, песня важнее номера
            let position = match queue.next.get(index) {
                Some(item) if item.uri() == uri => Some(index),
                _ => queue.next.iter().position(|item| item.uri() == uri),
            };
            let Some(position) = position else {
                self.refresh_queue(true);
                return;
            };
            skips = position + 1;
            let mut items: Vec<QItem> = queue.next.drain(..=position).collect();
            let chosen = items.pop().expect("the chosen row was just taken");
            consumed = items.iter().map(|item| item.uri().to_owned()).collect();
            consumed.push(chosen.uri().to_owned());
            let track = chosen.track.clone();
            queue.current = Some(chosen);
            self.tracks.entry(track.uri.clone()).or_insert(track);
        }
        for gone in &consumed {
            if let Some(at) = self.manual_queue.iter().position(|queued| queued == gone) {
                self.remove_manual_queue_row(at);
            }
        }
        self.expect_track(uri, 0);
        self.queue_start_pending = Some(self.target());
        self.optimistic_playing = Some((true, Instant::now()));
        self.direction = 1;
        match self.target() {
            Target::Local => {
                for _ in 0..skips {
                    self.engine(PlayerCommand::Next);
                }
            }
            Target::Remote(device) => {
                if device.is_none() && self.remote_fresh().is_none() {
                    self.remote(RemoteAction::Next, None);
                    return;
                }
                for _ in 0..skips {
                    self.remote(RemoteAction::Next, device.clone());
                }
            }
        }
    }

    /// Сколько первых строк «Далее» поставил пользователь (секция manual).
    pub fn queued_rows_len(&self) -> usize {
        let manual = if self.manual_queue.is_empty() && self.resume_only() {
            &self.resume_queue
        } else {
            &self.manual_queue
        };
        match &self.queue {
            Some(queue) => end_of_queued_rows(&queue.next, manual),
            None => 0,
        }
    }

    /// Только у локального плеера очередь можно переписать (ClearQueue + AddToQueue).
    pub fn queue_locally_reorderable(&self) -> bool {
        self.local.is_active() && matches!(self.target(), Target::Local)
    }

    pub fn can_clear_queue(&self) -> bool {
        self.queue_locally_reorderable() && self.queued_rows_len() > 0
    }

    /// «Очистить»: только ручные строки; контекст остаётся.
    pub fn clear_queue(&mut self) {
        if !matches!(self.target(), Target::Local) {
            return;
        }
        self.pending_album_queues.clear();
        self.last_album_queue = None;
        // каждая песня записана и в manual_queue, и в pending — берём большее из двух чисел
        let mut counts: HashMap<String, usize> = HashMap::new();
        for uri in &self.manual_queue {
            *counts.entry(uri.clone()).or_insert(0) += 1;
        }
        let mut pending: HashMap<&str, usize> = HashMap::new();
        for addition in &self.pending_queue_adds {
            *pending.entry(addition.item.uri.as_str()).or_insert(0) += 1;
        }
        for (uri, count) in pending {
            let held = counts.entry(uri.to_owned()).or_insert(0);
            *held = (*held).max(count);
        }
        if let Some(queue) = &mut self.queue {
            let mut left = counts.clone();
            queue.next.retain(|item| match left.get_mut(item.uri()) {
                Some(left) if *left > 0 => {
                    *left -= 1;
                    false
                }
                _ => true,
            });
        }
        let cleared: HashSet<String> = counts.into_keys().collect();
        if !self.manual_queue.is_empty() {
            self.session_dirty = true;
        }
        self.manual_queue.clear();
        self.pending_queue_adds.clear();
        if !cleared.is_empty() {
            self.queue_cleared = Some((cleared, Instant::now()));
        }
        self.engine(PlayerCommand::ClearQueue);
        // свежий запрос уберёт строки, добавленные с других устройств
        self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
        self.notice(mf_i18n::t("Queue cleared"));
    }

    /// Next: голова «Далее» сразу становится текущей (если очередь «заякорена» на текущий трек).
    fn pop_queue_head(&mut self) {
        // восстановленная или старая очередь ничего не говорит о том, куда попадёт Next
        let current = self.current_track_uri();
        let Some(queue) = &mut self.queue else {
            return;
        };
        let anchored = current.as_deref() == queue.current.as_ref().map(QItem::uri);
        if !anchored || queue.next.is_empty() {
            return;
        }
        let item = queue.next.remove(0);
        let uri = item.uri().to_owned();
        let track = item.track.clone();
        queue.current = Some(item);
        // у строки уже есть данные песни — плеер-бар назовёт её сразу
        self.tracks.entry(uri.clone()).or_insert(track);
        self.consume_manual_queue_head(&uri);
        self.queue_start_pending = Some(self.target());
        self.expect_track(uri, 0);
    }

    fn consume_manual_queue_head(&mut self, uri: &str) {
        if self.manual_queue.first().map(String::as_str) == Some(uri) {
            self.remove_manual_queue_row(0);
        }
    }

    fn remove_manual_queue_row(&mut self, index: usize) {
        self.manual_queue.remove(index);
        self.pending_queue_adds.retain_mut(|addition| {
            if addition.manual_index == index {
                return false;
            }
            if addition.manual_index > index {
                addition.manual_index -= 1;
            }
            true
        });
        self.session_dirty = true;
    }

    /// Ответ очереди старше последнего локального изменения?
    pub(crate) fn queue_fetch_is_stale(&self, fetched: &FetchedQueue) -> bool {
        if self.queue_stale_retries >= QUEUE_STALE_RETRIES {
            return false;
        }
        let fetched_current = fetched.current.as_ref().map(|item| item.uri.as_str());
        // недавний запуск или Next должен уже быть текущей строкой ответа
        if let Some(intent) = &self.intent_track
            && (intent.at.elapsed() < PLAYBACK_HOLD || self.queued_play.is_some())
            && fetched_current != Some(intent.uri.as_str())
        {
            return true;
        }
        // при локальном воспроизведении текущая строка ответа обязана совпасть с движком
        if self.local.is_active()
            && let Some(track) = &self.local.track
            && fetched_current.is_some_and(|uri| uri != track.uri)
        {
            return true;
        }
        // очищенная строка всё ещё сверху — очистка не дошла
        if let Some((cleared, at)) = &self.queue_cleared
            && at.elapsed() < PLAYBACK_HOLD
            && fetched.next.first().is_some_and(|item| cleared.contains(&item.uri))
        {
            return true;
        }
        // прежний порядок контекста сразу после переключения shuffle
        if let Some(pending) = &self.queue_shuffle_pending
            && pending.at.elapsed() < PLAYBACK_HOLD
            && fetched_current == pending.current_uri.as_deref()
        {
            let at = end_of_queued_tracks(&fetched.next, &self.manual_queue);
            let context = &fetched.next[at..];
            if context.len() == pending.context_uris.len()
                && context.iter().map(|item| item.uri.as_str()).eq(pending.context_uris.iter().map(String::as_str))
            {
                return true;
            }
        }
        // локальная перестановка оптимистична — ответ должен её догнать
        if let Some(at) = self.queue_reorder_pending
            && at.elapsed() < PLAYBACK_HOLD
            && !fetched
                .next
                .iter()
                .take(self.manual_queue.len())
                .map(|item| item.uri.as_str())
                .eq(self.manual_queue.iter().map(String::as_str))
        {
            return true;
        }
        false
    }

    /// Принятый ответ очереди с сохранением uid уже показанных строк.
    fn adopt_queue(&mut self, fetched: FetchedQueue) -> QueueData {
        let old: Vec<QItem> = self
            .queue
            .as_ref()
            .map(|queue| queue.current.iter().chain(queue.next.iter()).cloned().collect())
            .unwrap_or_default();
        let mut used = vec![false; old.len()];
        let mut cursor = 0;
        let mut take = |track: TrackItem, core: &mut Self| -> QItem {
            let found = (cursor..old.len())
                .find(|&i| !used[i] && old[i].track.uri == track.uri)
                .or_else(|| (0..old.len()).find(|&i| !used[i] && old[i].track.uri == track.uri));
            match found {
                Some(i) => {
                    used[i] = true;
                    cursor = i + 1;
                    // ответ может не знать имени, которое уже показано (оптимистичная строка)
                    let track = if track.title.is_empty() && !old[i].track.title.is_empty() {
                        old[i].track.clone()
                    } else {
                        track
                    };
                    QItem { uid: old[i].uid, track }
                }
                None => core.item(track),
            }
        };
        let current = fetched.current.map(|track| take(track, self));
        let next = fetched.next.into_iter().map(|track| take(track, self)).collect();
        QueueData { current, next }
    }

    fn handle_queue(&mut self, seq: u64, result: Result<FetchedQueue, ApiError>) {
        if seq != self.queue_seq {
            // новый запрос уже ушёл — этот ответ устарел
            return;
        }
        // пока идут записи в очередь, частичные чтения не принимаются
        let target = self.target();
        let writing_queue = self.pending_queue_batches.values().any(|batch| *batch == target);
        if writing_queue || result.as_ref().is_ok_and(|fetched| self.queue_fetch_is_stale(fetched)) {
            self.queue_stale_retries = self.queue_stale_retries.saturating_add(1);
            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
            return;
        }
        self.queue_stale_retries = 0;
        self.queue_loading = false;
        match result {
            Ok(mut fetched) => {
                if let Some(context) = fetched.context.take() {
                    self.learn_context(context);
                }
                self.queue_cleared = None;
                self.queue_shuffle_pending = None;
                self.queue_reorder_pending = None;
                for track in fetched.current.iter().chain(fetched.next.iter()) {
                    if !track.title.is_empty() {
                        self.remember(track);
                    }
                }
                self.recommended = std::mem::take(&mut fetched.recommended);
                let queue = self.adopt_queue(fetched);
                self.queue = Some(queue);
                self.reconcile_pending_queue();
            }
            Err(error) => tracing::debug!("queue unavailable: {error:#}"),
        }
    }

    // ---- добавление в очередь ------------------------------------------------------------------------

    /// «В очередь» одной песни (альбом — целиком).
    pub fn add_to_queue(&mut self, uri: String, label: String) {
        if uri_kind(&uri) == Some("album") {
            self.queue_album(uri, label);
            return;
        }
        if !matches!(uri_kind(&uri), Some("track" | "episode")) {
            self.notice(mf_i18n::t("Only a track, episode or album can be queued"));
            return;
        }
        if self.queued_moments_ago(&uri) {
            return;
        }
        self.queue_one(uri, label, true);
    }

    /// Тот же URI в пределах 1,5 с — это двойной клик, а не вторая просьба.
    fn queued_moments_ago(&mut self, uri: &str) -> bool {
        self.expire_pending_queue_adds();
        self.pending_queue_adds
            .iter()
            .any(|pending| pending.item.uri == uri && pending.at.elapsed() < QUEUE_ADD_DEBOUNCE)
    }

    fn queue_album(&mut self, uri: String, label: String) {
        if self
            .last_album_queue
            .as_ref()
            .is_some_and(|(previous, at)| *previous == uri && at.elapsed() < QUEUE_ADD_DEBOUNCE)
        {
            return;
        }
        let Some(id) = sf_api::util::uri_id(&uri).map(str::to_owned) else {
            return;
        };
        self.last_album_queue = Some((uri, Instant::now()));
        self.serial = self.serial.wrapping_add(1);
        let request = self.serial;
        self.pending_album_queues.insert(request, PendingAlbumQueue { label: label.clone(), target: self.target() });
        self.api(ApiCall::AlbumTracks { request, id });
        if label.is_empty() {
            self.notice(mf_i18n::t("Adding the album to the queue…"));
        } else {
            self.notice(mf_i18n::tf("Adding “{0}” to the queue…", &[&label]));
        }
    }

    fn receive_album_queue(&mut self, request: u64, result: Result<Vec<TrackItem>, ApiError>) {
        let Some(pending) = self.pending_album_queues.remove(&request) else {
            return;
        };
        if pending.target != self.target() {
            self.notice(mf_i18n::t("The playback device changed. Add the album to the queue again"));
            return;
        }
        match result {
            Ok(tracks) => self.queue_album_tracks(tracks, pending.label),
            Err(error) => {
                if pending.label.is_empty() {
                    self.notice(mf_i18n::tf("Couldn't add the album to the queue: {0}", &[&error]));
                } else {
                    self.notice(mf_i18n::tf("Couldn't add “{0}” to the queue: {1}", &[&pending.label, &error]));
                }
            }
        }
    }

    pub(crate) fn queue_album_tracks(&mut self, tracks: Vec<TrackItem>, label: String) {
        let pending_start = self.pending_queue_adds.len();
        let mut uris = Vec::new();
        for track in tracks {
            if !track.available || uri_kind(&track.uri) != Some("track") {
                continue;
            }
            self.remember(&track);
            let uri = track.uri.clone();
            self.show_queued_song(track);
            uris.push(uri);
        }
        if uris.is_empty() {
            self.notice(mf_i18n::t("This album has no tracks that can be queued"));
            return;
        }
        let count = uris.len();
        let songs = mf_i18n::n(count as i64, "{0} track", "{0} tracks");
        if label.is_empty() {
            self.notice(mf_i18n::tf("Queued {0} from the album", &[&songs]));
        } else {
            self.notice(mf_i18n::tf("Queued {0} from “{1}”", &[&songs, &label]));
        }
        if self.local.is_active() && self.target() == Target::Local {
            for uri in uris {
                self.engine(PlayerCommand::AddToQueue(uri));
            }
            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
        } else {
            self.write_queue_adds(uris, pending_start);
        }
    }

    /// Несколько песен за раз: повторы внутри пакета сохраняются, один тост.
    pub fn queue_many(&mut self, songs: Vec<TrackItem>) {
        let repeats: Vec<bool> = songs.iter().map(|song| self.queued_moments_ago(&song.uri)).collect();
        let mut count = 0;
        for (song, repeat) in songs.into_iter().zip(repeats) {
            if !repeat && matches!(uri_kind(&song.uri), Some("track" | "episode")) {
                self.remember(&song);
                let uri = song.uri.clone();
                let title = song.title.clone();
                self.queue_one(uri, title, false);
                count += 1;
            }
        }
        if count > 0 {
            self.queued_notice(count);
        }
    }

    fn queued_notice(&mut self, count: usize) {
        let songs = mf_i18n::n(count as i64, "{0} track", "{0} tracks");
        self.notice(mf_i18n::tf("Added to queue: {0}", &[&songs]));
    }

    /// librespot умеет только дописывать и очищать: перестановка = ClearQueue + AddToQueue каждой.
    fn resync_local_queue(&mut self) {
        self.engine(PlayerCommand::ClearQueue);
        for uri in self.manual_queue.clone() {
            if uri.starts_with("spotify:track:") || uri.starts_with("spotify:episode:") {
                self.engine(PlayerCommand::AddToQueue(uri));
            }
        }
        // запрос, ушедший до перестановки, устарел; каждая перестановка получает полный запас попыток
        self.queue_seq += 1;
        self.queue_stale_retries = 0;
        self.queue_reorder_pending = Some(Instant::now());
        self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
    }

    pub(crate) fn queue_one(&mut self, uri: String, label: String, announce: bool) {
        let pending_start = self.pending_queue_adds.len();
        let item = self.optimistic_queue_item(&uri, &label);
        let title = item.title.clone();
        self.show_queued_song(item);
        if announce {
            if title.is_empty() {
                self.notice(mf_i18n::t("Added to queue"));
            } else {
                self.notice(mf_i18n::tf("“{0}” added to queue", &[&title]));
            }
        }
        if self.local.is_active() && matches!(self.target(), Target::Local) {
            self.engine(PlayerCommand::AddToQueue(uri));
            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
            return;
        }
        self.write_queue_adds(vec![uri], pending_start);
    }

    fn show_queued_song(&mut self, item: TrackItem) {
        self.pending_queue_adds.push(PendingQueueAdd {
            item: item.clone(),
            at: Instant::now(),
            manual_index: self.manual_queue.len(),
            write: None,
        });
        let uri = item.uri.clone();
        if self.queue.is_none() && self.now_playing().is_some() {
            // очередь ещё не прочитана — ручная строка видна сразу
            self.queue = Some(QueueData::default());
        }
        if self.queue.is_some() {
            let row = self.item(item);
            let manual = &self.manual_queue;
            if let Some(queue) = &mut self.queue {
                let at = end_of_queued_rows(&queue.next, manual);
                queue.next.insert(at, row);
            }
        }
        self.manual_queue.push(uri);
        self.session_dirty = true;
    }

    fn write_queue_adds(&mut self, uris: Vec<String>, pending_start: usize) {
        let target = self.target();
        let device = match &target {
            Target::Local => self.local_device_id.clone(),
            Target::Remote(device) => device.clone(),
        };
        self.serial = self.serial.wrapping_add(1);
        let request = self.serial;
        for (index, addition) in self.pending_queue_adds[pending_start..].iter_mut().enumerate() {
            addition.write = Some((request, index));
        }
        self.pending_queue_batches.insert(request, target);
        self.api(ApiCall::AddMany { request, uris, device });
    }

    /// Откатить только отклонённые копии; принятые, более поздние и копии контекста остаются.
    fn remove_failed_queue_adds(&mut self, request: u64, added: usize) {
        let mut indexes: Vec<usize> = self
            .pending_queue_adds
            .iter()
            .filter(|addition| addition.write.is_some_and(|(write, index)| write == request && index >= added))
            .map(|addition| addition.manual_index)
            .collect();
        indexes.sort_unstable();
        for index in indexes.into_iter().rev() {
            let Some(uri) = self.manual_queue.get(index).cloned() else {
                continue;
            };
            let occurrence = self.manual_queue[..index].iter().filter(|other| **other == uri).count();
            let manual = &self.manual_queue;
            if let Some(queue) = &mut self.queue {
                let queued_len = end_of_queued_rows(&queue.next, manual);
                if let Some(row) = queue.next[..queued_len]
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.uri() == uri)
                    .nth(occurrence)
                    .map(|(row, _)| row)
                {
                    queue.next.remove(row);
                }
            }
            self.remove_manual_queue_row(index);
        }
    }

    /// Показать первые `queued_len` строк в порядке manual_queue — в том, что отдан движку.
    fn show_manual_queue_order(&mut self, queued_len: usize) {
        let Some(queue) = &mut self.queue else {
            return;
        };
        let queued_len = queued_len.min(queue.next.len());
        let mut shown: Vec<Option<QItem>> = queue.next.drain(..queued_len).map(Some).collect();
        let mut ordered = Vec::with_capacity(shown.len());
        for uri in &self.manual_queue {
            if let Some(item) = shown
                .iter_mut()
                .find(|item| item.as_ref().is_some_and(|item| item.uri() == uri))
                .and_then(Option::take)
            {
                ordered.push(item);
            }
        }
        ordered.extend(shown.into_iter().flatten());
        queue.next.splice(0..0, ordered);
    }

    /// Строка для оптимистичного показа: из кэша, из очереди или заглушка с подписью.
    fn optimistic_queue_item(&self, uri: &str, label: &str) -> TrackItem {
        if let Some(track) = self.tracks.get(uri) {
            return track.clone();
        }
        if let Some(item) = self
            .queue
            .as_ref()
            .and_then(|queue| queue.current.iter().chain(queue.next.iter()).find(|item| item.uri() == uri))
        {
            return item.track.clone();
        }
        TrackItem {
            track_id: sf_api::util::uri_id(uri).unwrap_or_default().to_owned(),
            uri: uri.to_owned(),
            title: label.to_owned(),
            available: true,
            is_episode: uri.starts_with("spotify:episode:"),
            ..TrackItem::default()
        }
    }

    pub(crate) fn expire_pending_queue_adds(&mut self) {
        if !self.pending_queue_batches.is_empty() {
            return;
        }
        self.pending_queue_adds.retain(|addition| addition.at.elapsed() < PENDING_ADD_LIFETIME);
    }

    /// Вернуть на место ожидающие добавления, которых нет в (запаздывающем) ответе.
    pub(crate) fn reconcile_pending_queue(&mut self) {
        self.expire_pending_queue_adds();
        if self.pending_queue_adds.is_empty() {
            return;
        }
        let Some(queue) = &self.queue else {
            return;
        };
        let fetched: HashSet<String> = queue.next.iter().map(|item| item.uri().to_owned()).collect();
        // уже пришедшая или уже играющая песня больше не ожидается
        let current = self.current_track_uri();
        self.pending_queue_adds
            .retain(|addition| !fetched.contains(&addition.item.uri) && current.as_deref() != Some(addition.item.uri.as_str()));
        let missing: Vec<TrackItem> = self.pending_queue_adds.iter().map(|addition| addition.item.clone()).collect();
        let rows: Vec<QItem> = missing.into_iter().map(|track| self.item(track)).collect();
        let manual = &self.manual_queue;
        if let Some(queue) = &mut self.queue {
            let at = end_of_queued_rows(&queue.next, manual);
            for row in rows.into_iter().rev() {
                queue.next.insert(at, row);
            }
        }
    }

    /// Перестановка внутри «Далее в очереди»: `to` — итоговый индекс (как ListChange::Move).
    pub fn move_manual(&mut self, from: usize, to: usize) {
        if !self.queue_locally_reorderable() {
            return;
        }
        let queued_len = self.queued_rows_len();
        if from >= queued_len || to >= queued_len || from == to {
            return;
        }
        if from < self.manual_queue.len() {
            let uri = self.manual_queue.remove(from);
            let at = to.min(self.manual_queue.len());
            self.manual_queue.insert(at, uri);
            // ожидающие добавления сдвигаются так же, как строки, на которые они указывают
            for addition in &mut self.pending_queue_adds {
                if addition.manual_index == from {
                    addition.manual_index = at;
                } else if from < addition.manual_index && addition.manual_index <= at {
                    addition.manual_index -= 1;
                } else if at <= addition.manual_index && addition.manual_index < from {
                    addition.manual_index += 1;
                }
            }
            self.show_manual_queue_order(queued_len);
        }
        self.session_dirty = true;
        self.resync_local_queue();
    }

    /// Вставка в позицию «Далее в очереди» (только локально, иначе — в конец, как «В очередь»).
    pub fn insert_manual(&mut self, items: Vec<TrackItem>, position: usize) {
        if !self.queue_locally_reorderable() {
            self.queue_many(items);
            return;
        }
        let position = position.min(self.queued_rows_len());
        let repeats: Vec<bool> = items.iter().map(|item| self.queued_moments_ago(&item.uri)).collect();
        let mut inserted = 0;
        for (item, repeat) in items.into_iter().zip(repeats) {
            if repeat || !matches!(uri_kind(&item.uri), Some("track" | "episode")) {
                continue;
            }
            self.remember(&item);
            let uri = item.uri.clone();
            let at = (position + inserted).min(self.manual_queue.len());
            for addition in &mut self.pending_queue_adds {
                if addition.manual_index >= at {
                    addition.manual_index += 1;
                }
            }
            self.pending_queue_adds.push(PendingQueueAdd {
                item: item.clone(),
                at: Instant::now(),
                manual_index: at,
                write: None,
            });
            let row = self.item(item);
            if let Some(queue) = &mut self.queue {
                let at = at.min(queue.next.len());
                queue.next.insert(at, row);
            }
            self.manual_queue.insert(at, uri);
            inserted += 1;
        }
        if inserted > 0 {
            self.session_dirty = true;
            self.queued_notice(inserted);
            self.resync_local_queue();
        }
    }

    // ---- запуск ----------------------------------------------------------------------------------

    /// Запуск списка/контекста. `shuffle_first` — включить перемешивание до старта.
    pub fn play_request(&mut self, request: PlayRequest, shuffle_first: bool) {
        let mut request = request;
        self.queue_shuffle_pending = None;
        self.device_lost = false;
        self.lost = None;
        if shuffle_first {
            self.shuffle_wanted = true;
            self.shuffle_set_at = Some(Instant::now());
            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
        }
        let shuffle = shuffle_first || self.shuffle_wanted;
        if request.offset_uri.is_none()
            && request.offset_position.is_none()
            && request.uris.is_empty()
            && let Some(context) = request.context_uri.as_deref()
            && !shuffle
            && context.starts_with("spotify:playlist:")
        {
            // плейлист по порядку — с начала, а не с места, где его оставил плеер
            request.offset_position = Some(0);
        }
        let mut keys: Vec<String> = Vec::new();
        if let Some(offset) = &request.offset_uri {
            keys.push(offset.clone());
        }
        match request.offset_position {
            Some(position) => {
                if let Some(uri) = request.uris.get(position as usize) {
                    keys.push(uri.clone());
                }
            }
            None if request.offset_uri.is_none() => {
                if let Some(first) = request.uris.first() {
                    keys.push(first.clone());
                }
            }
            None => {}
        }
        if let Some(context) = &request.context_uri
            && matches!(uri_kind(context), Some("track" | "episode"))
        {
            keys.push(context.clone());
        }
        let expected = keys.iter().find(|key| key.contains(":track:") || key.contains(":episode:")).cloned();
        match expected {
            Some(uri) => self.expect_track(uri, request.position_ms),
            None => self.intent_track = None,
        }
        // контекст сразу показан играющим; Spotify догонит
        self.assumed_context = Some(AssumedContext {
            uri: request.context_uri.clone().unwrap_or_default(),
            shuffle: shuffle.then_some(true),
            at: Instant::now(),
            tracks: Vec::new(),
        });
        self.queue_start_pending = Some(self.target());
        self.direction = 1;
        match self.target() {
            Target::Local if !self.local.connected => {
                // движок переподключается — запрос ждёт его
                self.queued_play = Some(request);
            }
            Target::Local => {
                self.queued_play = None;
                let mut load = local_load(&request, shuffle);
                load.repeat = Some(self.local.repeat);
                if shuffle
                    && self.smart_wanted
                    && let Some(uri) = load.context_uri.as_mut()
                    && smart_capable(uri)
                {
                    uri.push_str(SMART_LENS);
                }
                self.local_list = load.context_uri.is_none().then(|| load.uris.clone());
                let shuffle_after = shuffle && load.shuffle.is_none() && !load.uris.is_empty();
                self.engine(PlayerCommand::Load(load));
                if shuffle_after {
                    self.engine(PlayerCommand::Shuffle(true));
                }
                self.optimistic_playing = Some((true, Instant::now()));
            }
            Target::Remote(Some(device)) => {
                self.queued_play = None;
                self.api(ApiCall::Play { device: Some(device), request, shuffle });
                self.optimistic_playing = Some((true, Instant::now()));
            }
            Target::Remote(None) => {
                // никогда не просим Spotify играть «нигде»: ждём движок или просим выбрать устройство
                if matches!(self.local_playback, LocalPlayback::Connecting) || self.local_coming() {
                    self.queued_play = Some(request);
                } else {
                    self.queue_start_pending = None;
                    self.queued_play = None;
                    self.intent_track = None;
                    self.assumed_context = None;
                    self.notice(mf_i18n::t("Choose a device or turn on playback on this computer"));
                }
            }
        }
    }

    /// Контекст, который показывать для запуска (подпись/обложка от вызывающего).
    pub fn set_context_info(&mut self, context: PlayContext) {
        if let Some(uri) = context.context_uri.clone().filter(|uri| !uri.is_empty())
            && (!context.title.is_empty() || !context.cover.is_empty())
        {
            if self.known_contexts.len() >= KNOWN_CONTEXTS {
                self.known_contexts.clear();
            }
            self.known_contexts.insert(uri, context.clone());
        }
        self.context_info = context;
    }

    /// Название контекста, узнанное вместе с очередью: дополняет известное, не перекрывая подпись вызывающего.
    pub(crate) fn learn_context(&mut self, context: PlayContext) {
        let Some(uri) = context.context_uri.clone().filter(|uri| !uri.is_empty()) else {
            return;
        };
        if context.title.is_empty() {
            return;
        }
        if let Some(known) = self.known_contexts.get_mut(&uri) {
            if known.title.is_empty() {
                known.title = context.title;
            }
            if known.cover.is_empty() {
                known.cover = context.cover;
            }
            return;
        }
        if self.known_contexts.len() >= KNOWN_CONTEXTS {
            self.known_contexts.clear();
        }
        self.known_contexts.insert(uri, context);
    }

    /// Список треков из модели QML начиная с `row`.
    pub fn play_items(&mut self, items: Vec<TrackItem>, row: usize, context: PlayContext) {
        let Some(chosen) = items.get(row).cloned() else {
            return;
        };
        for item in &items {
            self.remember(item);
        }
        let station = context.kind == contexts::RADIO;
        match context.context_uri.clone() {
            Some(uri) if !station && !uri.is_empty() => {
                self.set_context_info(context);
                self.play_request(PlayRequest::context(uri).starting_at_uri(chosen.play_uri()), false);
            }
            _ => {
                let (uris, index) = playable_list(&items, row);
                if uris.is_empty() {
                    self.notice(mf_i18n::t("This track isn't available"));
                    return;
                }
                let (uris, index) = cap_uris(&uris, index as u32);
                let assumed = context.context_uri.clone().filter(|uri| !uri.is_empty());
                self.set_context_info(context);
                self.play_request(PlayRequest::tracks(uris.clone()).starting_at_index(index), false);
                if let Some(uri) = assumed
                    && self.assumed_context.is_some()
                {
                    // радио играет показанные песни, а показан контекстом — станция
                    self.assumed_context = Some(AssumedContext {
                        uri,
                        shuffle: self.shuffle_wanted.then_some(true),
                        at: Instant::now(),
                        tracks: uris,
                    });
                }
            }
        }
    }

    /// «Перемешать»: shuffle on и старт со случайного доступного трека.
    pub fn shuffle_from(&mut self, items: Vec<TrackItem>, context: PlayContext) {
        let available: Vec<usize> = (0..items.len()).filter(|&i| playable(&items[i])).collect();
        if available.is_empty() {
            self.notice(mf_i18n::t("Nothing to shuffle"));
            return;
        }
        let row = available[fastrand::usize(..available.len())];
        for item in &items {
            self.remember(item);
        }
        let chosen = items[row].play_uri();
        match context.context_uri.clone().filter(|uri| !uri.is_empty() && context.kind != contexts::RADIO) {
            Some(uri) => {
                self.set_context_info(context);
                self.play_request(PlayRequest::context(uri).starting_at_uri(chosen), true);
            }
            None => {
                let (uris, index) = playable_list(&items, row);
                let (uris, index) = cap_uris(&uris, index as u32);
                self.set_context_info(context);
                self.play_request(PlayRequest::tracks(uris).starting_at_index(index), true);
            }
        }
    }

    /// Контекст по URI с трека `offset_uri` или с начала; `shuffle` — задать режим перед стартом.
    pub fn play_context(&mut self, context: PlayContext, offset_uri: Option<String>, shuffle: Option<bool>) {
        let Some(uri) = context.context_uri.clone().filter(|uri| !uri.is_empty()) else {
            return;
        };
        let mut request = PlayRequest::context(uri);
        request.offset_uri = offset_uri.filter(|offset| !offset.is_empty());
        if shuffle == Some(false) && self.shuffle_wanted {
            self.shuffle_wanted = false;
            self.shuffle_set_at = Some(Instant::now());
        }
        self.set_context_info(context);
        self.play_request(request, shuffle == Some(true));
    }

    pub fn play_uris(&mut self, uris: Vec<String>, index: usize, context: PlayContext) {
        let uris: Vec<String> = uris.into_iter().filter(|uri| !uri.is_empty()).collect();
        if uris.is_empty() {
            return;
        }
        let (uris, index) = cap_uris(&uris, index.min(uris.len() - 1) as u32);
        let assumed = context.context_uri.clone().filter(|uri| !uri.is_empty());
        self.set_context_info(context);
        self.play_request(PlayRequest::tracks(uris.clone()).starting_at_index(index), false);
        if let Some(uri) = assumed
            && self.assumed_context.is_some()
        {
            // список играет как URI, а показан контекстом (радио, страница)
            self.assumed_context = Some(AssumedContext {
                uri,
                shuffle: self.shuffle_wanted.then_some(true),
                at: Instant::now(),
                tracks: uris,
            });
        }
    }

    /// Выпуск с позиции; играющий выпуск не отправляется на старую точку.
    pub fn play_episode(&mut self, uri: String, position_ms: i64) {
        let mut request = PlayRequest::tracks(vec![uri.clone()]).starting_at_index(0);
        if self.now_playing().is_none_or(|now| now.track.uri != uri) && position_ms > 0 {
            request.position_ms = position_ms.clamp(0, i64::from(u32::MAX)) as u32;
        }
        self.set_context_info(PlayContext::new(contexts::EPISODE));
        self.play_request(request, false);
    }

    /// Продолжить трек прошлой сессии.
    fn resume_last(&mut self) -> bool {
        let Some(track) = self.resume_track.clone() else {
            return false;
        };
        let mut request = match self.resume_context.clone() {
            Some(context) => PlayRequest::context(context).starting_at_uri(track),
            None => PlayRequest::tracks(vec![track]),
        };
        request.position_ms = self.resume_position_ms;
        self.play_request(request, false);
        true
    }

    /// Пропавшее устройство: продолжить здесь с последнего удалённого снимка.
    fn resume_lost_here(&mut self) -> bool {
        let Some(lost) = self.lost.clone() else {
            return false;
        };
        let Some(item) = lost.state.item.clone() else {
            return false;
        };
        if !self.local_ready {
            self.notice(mf_i18n::t("Playback on this computer is off"));
            return true;
        }
        let position = self.lost_position();
        self.selected_device = None;
        self.remember(&item);
        let mut request = match lost.state.context_uri.clone() {
            Some(context) => PlayRequest::context(context).starting_at_uri(item.uri.clone()),
            None => PlayRequest::tracks(vec![item.uri.clone()]),
        };
        request.position_ms = position;
        self.device_lost = false;
        self.lost = None;
        self.remote = None;
        self.play_request(request, false);
        true
    }

    // ---- транспорт ----------------------------------------------------------------------------------

    pub fn toggle_play(&mut self) {
        if self.device_lost && self.resume_lost_here() {
            return;
        }
        // пока грузится следующий трек, строка показывает «не играет», но движок ещё играет: клик — пауза
        let playing = self.now_playing().map(|now| {
            now.playing
                || (now.local
                    && now.loading
                    && match self.optimistic_playing {
                        Some((playing, at)) if at.elapsed() < PLAYBACK_HOLD => playing,
                        _ => self.queued_play.is_some() || self.local.playback == Playback::Playing,
                    })
        });
        match self.target() {
            Target::Local => {
                if self.local.is_active() {
                    self.engine(PlayerCommand::Toggle);
                } else {
                    // локально ничего не играет: продолжить здесь удалённое или прошлую сессию
                    if let Some(remote) = self.remote_fresh()
                        && let Some(item) = &remote.state.item
                    {
                        let mut request = match &remote.state.context_uri {
                            Some(context) => PlayRequest::context(context.clone()).starting_at_uri(item.uri.clone()),
                            None => PlayRequest::tracks(vec![item.uri.clone()]),
                        };
                        request.position_ms = remote.state.progress_ms.unwrap_or(0);
                        self.play_request(request, false);
                        return;
                    }
                    if !self.resume_last() {
                        self.notice(mf_i18n::t("Choose something to play"));
                    }
                    return;
                }
            }
            Target::Remote(device) => {
                if device.is_none() && self.remote_fresh().is_none() {
                    // нигде ничего не известно (свежий старт до готовности движка) — прошлая сессия
                    if !self.resume_last() {
                        self.notice(mf_i18n::t("Choose a track, album or playlist"));
                    }
                    return;
                }
                if playing == Some(true) {
                    self.remote(RemoteAction::Pause, device);
                } else {
                    self.remote(RemoteAction::Play, device);
                }
            }
        }
        if let Some(playing) = playing {
            self.optimistic_playing = Some((!playing, Instant::now()));
        }
    }

    pub fn play(&mut self) {
        if !self.believed_playing() {
            self.toggle_play();
        }
    }

    pub fn pause(&mut self) {
        if self.believed_playing() {
            self.toggle_play();
        }
    }

    pub fn next(&mut self) {
        if self.resume_only() {
            self.step_resume(true);
            return;
        }
        // голова очереди сразу становится текущей; без цели — не трогаем
        let target = self.target();
        if !matches!(target, Target::Remote(None)) || self.remote_fresh().is_some() {
            self.pop_queue_head();
        }
        self.direction = 1;
        match target {
            Target::Local => self.engine(PlayerCommand::Next),
            Target::Remote(device) => self.remote(RemoteAction::Next, device),
        }
    }

    pub fn previous(&mut self) {
        if self.resume_only() {
            if i64::from(self.resume_position_ms) > RESTART_BEFORE_PREVIOUS_MS {
                self.resume_position_ms = 0;
                self.session_dirty = true;
            } else {
                self.step_resume(false);
            }
            return;
        }
        // Next назвал свою цель заранее; Previous её отменяет
        self.intent_track = None;
        self.queue_start_pending = Some(self.target());
        self.direction = -1;
        match self.target() {
            Target::Local => self.engine(PlayerCommand::Previous),
            Target::Remote(device) => self.remote(RemoteAction::Previous, device),
        }
    }

    /// Шаг по сохранённой очереди без воспроизведения (Next/Previous до первого Play).
    fn step_resume(&mut self, forward: bool) -> bool {
        let Some(current) = self.resume_track.clone() else {
            return false;
        };
        if forward {
            let Some(queue) = &mut self.queue else {
                return false;
            };
            if queue.next.is_empty() {
                return false;
            }
            let next = queue.next.remove(0);
            let uri = next.uri().to_owned();
            let track = next.track.clone();
            queue.current = Some(next);
            self.remember(&track);
            if self.resume_queue.first() == Some(&uri) {
                self.resume_queue.remove(0);
            }
            self.resume_back.push(current);
            self.resume_track = Some(uri);
        } else {
            let Some(previous) = self.resume_back.pop() else {
                return false;
            };
            let Some(track) = self.tracks.get(&previous).cloned() else {
                return false;
            };
            let current_row = self.queue.as_mut().and_then(|queue| queue.current.take());
            let row = self.item(track);
            if let Some(queue) = &mut self.queue {
                if let Some(old) = current_row {
                    queue.next.insert(0, old);
                }
                queue.current = Some(row);
            }
            self.resume_track = Some(previous);
            self.direction = -1;
        }
        self.resume_position_ms = 0;
        self.session_dirty = true;
        true
    }

    pub fn seek(&mut self, position_ms: u32) {
        // под треком прошлой сессии ползунок двигает точку, с которой начнётся Play
        if self.now_playing_live().is_none() && self.resume_track.is_some() {
            self.resume_position_ms = position_ms;
            self.session_dirty = true;
            return;
        }
        match self.target() {
            Target::Local => {
                self.pending_local_position = Some((position_ms, Instant::now()));
                self.engine(PlayerCommand::Seek(position_ms));
            }
            Target::Remote(device) => {
                self.pending_remote_position = Some((position_ms, Instant::now()));
                self.remote(RemoteAction::Seek(position_ms), device);
            }
        }
    }

    pub fn seek_relative(&mut self, delta_ms: i64) {
        if let Some(now) = self.now_playing() {
            let position = (i64::from(now.position_ms) + delta_ms).clamp(0, i64::from(now.duration_ms.max(1)));
            self.seek(position as u32);
        }
    }

    pub fn can_set_volume(&self) -> bool {
        self.now_playing().is_none_or(|now| now.can_set_volume)
    }

    /// Громкость: локально сразу (VolumePreview), Volume и удалённый PUT — после 150 мс тишины.
    pub fn set_volume(&mut self, volume: u16) {
        match self.target() {
            Target::Local => {
                self.local.volume = volume;
                self.volume = volume;
                self.pending_local_volume = Some((volume, Instant::now()));
                self.engine(PlayerCommand::VolumePreview(volume));
            }
            Target::Remote(_) if !self.can_set_volume() => return,
            Target::Remote(_) => {
                self.pending_remote_volume = Some((volume_to_percent(volume), Instant::now()));
            }
        }
        self.volume_settle = Some((volume, Instant::now()));
    }

    fn settle_volume(&mut self, volume: u16) {
        match self.target() {
            Target::Local => {
                self.pending_local_volume = Some((volume, Instant::now()));
                self.engine(PlayerCommand::Volume(volume));
            }
            Target::Remote(_) if !self.can_set_volume() => {}
            Target::Remote(device) => {
                let percent = volume_to_percent(volume);
                self.pending_remote_volume = Some((percent, Instant::now()));
                self.remote(RemoteAction::Volume(percent), device);
            }
        }
    }

    fn note_shuffle_pending(&mut self) {
        let current = self.current_track_uri();
        if let Some(queue) = &self.queue {
            let at = end_of_queued_rows(&queue.next, &self.manual_queue);
            let context_uris: Vec<String> = queue.next[at..].iter().map(|item| item.uri().to_owned()).collect();
            if context_uris.len() > 1 && context_uris.iter().any(|uri| *uri != context_uris[0]) {
                self.queue_shuffle_pending =
                    Some(QueueShufflePending { current_uri: current, context_uris, at: Instant::now() });
            } else {
                self.queue_shuffle_pending = None;
            }
        }
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        let was_smart = self.smart_shuffle();
        if !shuffle {
            self.smart_wanted = false;
        }
        self.shuffle_wanted = shuffle;
        self.shuffle_set_at = Some(Instant::now());
        self.session_dirty = true;
        // рекомендации живут в самом контексте — без них контекст грузится заново
        if was_smart && !shuffle && self.reload_smart_context() {
            return;
        }
        if let Some(assumed) = &mut self.assumed_context {
            assumed.shuffle = Some(shuffle);
        }
        self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
        self.note_shuffle_pending();
        match self.target() {
            Target::Local => {
                self.local.shuffle = shuffle;
                self.engine(PlayerCommand::Shuffle(shuffle));
            }
            // без устройства режим просто запоминается для следующего Play
            Target::Remote(None) if self.remote_fresh().is_none() => {}
            Target::Remote(device) => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.state.shuffle = shuffle;
                }
                self.remote(RemoteAction::Shuffle(shuffle), device);
            }
        }
    }

    /// Smart Shuffle можно включить: играет плейлист на этом ПК (удалённым устройствам линзу не передать).
    pub fn smart_shuffle_available(&self) -> bool {
        self.target() == Target::Local && self.playing_context_uri().is_some_and(|uri| smart_capable(&uri))
    }

    pub fn smart_shuffle(&self) -> bool {
        self.smart_wanted && self.shuffle_wanted && self.smart_shuffle_available()
    }

    pub fn set_smart_shuffle(&mut self, smart: bool) {
        if smart == self.smart_shuffle() || (smart && !self.smart_shuffle_available()) {
            return;
        }
        self.smart_wanted = smart;
        self.shuffle_wanted = true;
        self.shuffle_set_at = Some(Instant::now());
        self.session_dirty = true;
        self.reload_smart_context();
    }

    /// Тот же контекст с рекомендациями или без них, с текущего трека и места; если играет рекомендация,
    /// в обычном плейлисте её нет — тогда с начала.
    fn reload_smart_context(&mut self) -> bool {
        let (Some(context), Some(now)) = (self.playing_context_uri(), self.now_playing()) else {
            return false;
        };
        let mut request = PlayRequest::context(context);
        if !self.recommended.contains(&now.track.uri) {
            request = request.starting_at_uri(now.track.uri);
            request.position_ms = now.position_ms;
        }
        self.play_request(request, false);
        true
    }

    pub fn repeat(&self) -> RepeatMode {
        self.now_playing().map(|now| now.repeat).unwrap_or(self.local.repeat)
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        match self.target() {
            Target::Local => {
                self.local.repeat = mode;
                self.engine(PlayerCommand::Repeat(mode));
            }
            Target::Remote(device) => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.state.repeat = mode;
                }
                if device.is_none() && self.remote_fresh().is_none() {
                    self.local.repeat = mode;
                    return;
                }
                self.remote(RemoteAction::Repeat(mode), device);
            }
        }
    }

    /// Удалённая команда; без активного устройства запрос не шлётся.
    fn remote(&mut self, action: RemoteAction, device: Option<String>) {
        if device.is_none() && self.remote_fresh().is_none() {
            self.notice(mf_i18n::t("Nothing is playing. Choose something to play first"));
            return;
        }
        self.api(ApiCall::Remote { action, device });
    }

    // ---- Connect ---------------------------------------------------------------------------------------

    /// Перенести воспроизведение на этот ПК.
    pub fn transfer_here(&mut self, _play: bool) {
        if self.device_lost {
            // забирать нечего: продолжить с последнего удалённого снимка
            self.resume_lost_here();
            return;
        }
        if !self.local_ready {
            self.notice(mf_i18n::t("Playback on this computer is off"));
            return;
        }
        self.selected_device = None;
        if !self.local.is_active() {
            // Connect переносит всё состояние активного устройства, включая очередь
            let from = self
                .remote_fresh()
                .and_then(|remote| remote.state.device.as_ref())
                .map(|device| device.name.clone())
                .unwrap_or_default();
            self.queue_start_pending = Some(Target::Local);
            self.local_transfer_sequence = Some(self.local.track_sequence);
            self.local_list = None;
            self.resume_queue.clear();
            self.queued_play = None;
            self.intent_track = None;
            self.assumed_context = None;
            self.optimistic_playing = None;
            self.transfer = Some(Transfer { to: None, name: from, at: Instant::now() });
            self.engine(PlayerCommand::Transfer);
        }
        self.poll_remote_soon();
    }

    /// Перенести на устройство Connect (PUT /me/player).
    pub fn transfer_to(&mut self, device: String, play: bool) {
        if Some(device.as_str()) == self.local_device_id.as_deref() {
            self.transfer_here(play);
            return;
        }
        let play = play || self.believed_playing();
        let name = self.known_devices.get(&device).cloned().unwrap_or_default();
        self.selected_device = Some(device.clone());
        self.device_lost = false;
        self.lost = None;
        self.transfer = Some(Transfer { to: Some(device.clone()), name, at: Instant::now() });
        self.api(ApiCall::Transfer { device, play });
    }

    pub fn set_app_active(&mut self, active: bool) {
        if active && !self.app_active {
            self.poll_remote_soon();
        }
        self.app_active = active;
    }

    /// Выход из аккаунта: всё, что относится к аккаунту, забыть.
    pub fn reset_account(&mut self) {
        self.pending_album_queues.clear();
        self.pending_queue_batches.clear();
        self.pending_queue_adds.clear();
        self.last_album_queue = None;
        self.manual_queue.clear();
        self.queue = None;
        self.queue_seq += 1;
        self.remote = None;
        self.remote_poll_seq += 1;
        self.remote_poll_pending = false;
        self.selected_device = None;
        self.device_lost = false;
        self.lost = None;
        self.transfer = None;
        self.intent_track = None;
        self.optimistic_playing = None;
        self.assumed_context = None;
        self.queued_play = None;
        self.local_expected = None;
        self.resume_track = None;
        self.resume_context = None;
        self.resume_queue.clear();
        self.requeue = None;
        self.session_dirty = true;
    }

    // ---- ответы Web API --------------------------------------------------------------------------------

    pub fn handle_api(&mut self, reply: ApiReply) {
        match reply {
            ApiReply::Playback { seq, result } => self.handle_playback_state(seq, result),
            ApiReply::Queue { seq, result } => self.handle_queue(seq, result),
            ApiReply::Remote { action, result } => {
                match result {
                    Ok(()) => {
                        self.remote_recheck_at = Some(Instant::now() + REMOTE_RECHECK);
                        if matches!(action, RemoteAction::Shuffle(_)) {
                            self.queue_recheck_at = Some(Instant::now() + QUEUE_RECHECK);
                            if let Some(pending) = &mut self.queue_shuffle_pending {
                                pending.at = Instant::now();
                            } else {
                                self.note_shuffle_pending();
                            }
                        }
                    }
                    Err(error) => {
                        self.optimistic_playing = None;
                        self.pending_remote_position = None;
                        self.pending_remote_volume = None;
                        if matches!(action, RemoteAction::Play | RemoteAction::Next | RemoteAction::Previous)
                            && self
                                .intent_track
                                .as_ref()
                                .is_some_and(|intent| matches!(intent.confirmation, TrackConfirmation::Remote { .. }))
                        {
                            self.intent_track = None;
                        }
                        let hint = if error.status() == Some(404) {
                            self.mark_device_lost();
                            format!(" {}", mf_i18n::t("Choose a device first."))
                        } else {
                            String::new()
                        };
                        self.notice(format!("{}: {error}.{hint}", action.label()));
                    }
                }
                self.poll_remote_soon();
            }
            ApiReply::Transferred { device, name, result } => {
                if let Some(name) = name.filter(|name| !name.is_empty()) {
                    self.known_devices.insert(device.clone(), name.clone());
                    if let Some(transfer) = &mut self.transfer
                        && transfer.to.as_deref() == Some(device.as_str())
                    {
                        transfer.name = name;
                    }
                }
                match result {
                    Ok(()) => {
                        if self.transfer.as_ref().is_some_and(|transfer| transfer.to.as_deref() == Some(&device)) {
                            self.selected_device = Some(device);
                        }
                        self.poll_remote_soon();
                    }
                    Err(error) => {
                        if self.transfer.as_ref().is_some_and(|transfer| transfer.to.as_deref() == Some(&device)) {
                            self.transfer = None;
                            self.selected_device = None;
                        }
                        self.notice(mf_i18n::tf("Couldn't switch device: {0}", &[&error]));
                    }
                }
            }
            ApiReply::QueueBatchAdded { request, added, result } => {
                if self.pending_queue_batches.remove(&request).as_ref() != Some(&self.target()) {
                    return;
                }
                if result.is_err() {
                    self.remove_failed_queue_adds(request, added);
                }
                for addition in &mut self.pending_queue_adds {
                    if addition.write.is_some_and(|(write, _)| write == request) {
                        addition.write = None;
                        addition.at = Instant::now();
                    }
                }
                if let Err(error) = result {
                    self.notice(mf_i18n::tf("Couldn't add to queue: {0}", &[&error]));
                }
                self.refresh_queue(true);
            }
            ApiReply::AlbumTracks { request, result } => self.receive_album_queue(request, result),
            ApiReply::TrackInfo { request, uri, result } => self.handle_track_info(request, uri, result),
        }
    }

    fn mark_device_lost(&mut self) {
        if self.device_lost {
            return;
        }
        let previous = self.remote.clone().filter(|remote| {
            remote.state.item.is_some() && remote.state.device_id() != self.local_device_id.as_deref()
        });
        if let Some(previous) = previous {
            self.device_lost = true;
            self.lost = Some(previous);
            self.remote = None;
            self.selected_device = None;
        }
    }

    fn handle_playback_state(&mut self, seq: u64, result: Result<Option<RemoteState>, ApiError>) {
        if seq != self.remote_poll_seq {
            // опоздавший старый опрос описывает прошлое
            return;
        }
        self.remote_poll_pending = false;
        let state = match result {
            Ok(state) => state,
            Err(error) => {
                if error.status() == Some(404) {
                    self.mark_device_lost();
                }
                tracing::debug!("playback state unavailable: {error:#}");
                return;
            }
        };
        // устройство пропало? два опроса подряд без него
        let showing_remote = self.remote.as_ref().is_some_and(|remote| {
            remote.state.item.is_some() && remote.state.device_id() != self.local_device_id.as_deref()
        });
        let has_device = state.as_ref().is_some_and(|state| state.device.is_some());
        if showing_remote && !has_device && !self.local.is_active() {
            self.missing_polls += 1;
            if self.missing_polls < LOST_AFTER_POLLS {
                self.remote_recheck_at = Some(Instant::now() + REMOTE_RECHECK);
                return;
            }
            self.missing_polls = 0;
            self.mark_device_lost();
            return;
        }
        self.missing_polls = 0;
        if has_device {
            self.device_lost = false;
            self.lost = None;
        }
        let previous_uri = self.remote.as_ref().and_then(|remote| remote.state.item.as_ref().map(|item| item.uri.clone()));
        let previous_shuffle = self.remote.as_ref().map(|remote| remote.state.shuffle);
        self.remote = state.map(|state| RemoteSnapshot { state, received_at: Instant::now() });
        if let (Some(previous), Some(current)) = (previous_shuffle, self.remote.as_ref().map(|remote| remote.state.shuffle))
            && previous != current
            && self.shuffle_set_at.is_none_or(|at| at.elapsed() > Duration::from_secs(5))
        {
            // смена shuffle с другого устройства
            self.shuffle_wanted = current;
        }
        if let Some(device) = self.remote.as_ref().and_then(|remote| remote.state.device.clone())
            && let Some(id) = device.id.clone()
        {
            self.known_devices.insert(id.clone(), device.name.clone());
            // перенос подтверждён: активным стало нужное устройство
            if let Some(transfer) = &self.transfer
                && transfer.to.as_deref() == Some(id.as_str())
                && device.is_active
            {
                self.transfer = None;
            }
        }
        if let Some(item) = self.remote.as_ref().and_then(|remote| remote.state.item.clone()) {
            self.remember(&item);
        }
        let uri = self.remote.as_ref().and_then(|remote| remote.state.item.as_ref().map(|item| item.uri.clone()));
        self.reconcile_remote_track_intent(seq, uri.as_deref());
        if let Some((wanted, _)) = self.optimistic_playing
            && self.remote.as_ref().is_some_and(|remote| remote.state.is_playing == wanted)
            && !self.local.is_active()
        {
            self.optimistic_playing = None;
        }
        if uri != previous_uri {
            self.on_now_playing_changed();
        }
    }

    // ---- сессия ----------------------------------------------------------------------------------

    pub fn session_data(&mut self) -> SessionData {
        if let Some(now) = self.now_playing() {
            self.resume_context = self.playing_context_uri();
            self.resume_track = Some(now.track.uri.clone());
            self.resume_position_ms = now.position_ms;
        }
        let track = self.resume_track.as_ref().and_then(|uri| self.tracks.get(uri)).cloned();
        let context = self.shown_context();
        SessionData {
            version: SessionData::VERSION,
            context_uri: self.resume_context.clone(),
            context,
            track,
            position_ms: self.resume_position_ms,
            // ни разу не возобновлённая очередь переносится дальше как есть
            manual: if self.resume_queue.is_empty() { self.manual_queue.clone() } else { self.resume_queue.clone() },
            rows: self
                .queue
                .as_ref()
                .map(|queue| queue.next.iter().take(30).map(|item| item.track.clone()).collect())
                .unwrap_or_default(),
            shuffle: self.shuffle_wanted,
        }
    }

    pub fn save_session(&mut self) {
        self.session_dirty = false;
        self.last_session_save = Instant::now();
        if !self.persist {
            return;
        }
        let data = self.session_data();
        self.effects.push(Effect::Save(Box::new(data)));
    }

    pub fn restore_session(&mut self, data: SessionData) {
        let Some(track) = data.track.filter(|track| !track.uri.is_empty()) else {
            return;
        };
        if self.now_playing_live().is_some() {
            return;
        }
        self.remember(&track);
        for row in &data.rows {
            self.remember(row);
        }
        self.resume_track = Some(track.uri.clone());
        self.resume_context = data.context_uri.filter(|uri| !uri.is_empty());
        self.resume_position_ms = data.position_ms;
        self.resume_queue = data.manual;
        self.shuffle_wanted = data.shuffle;
        self.set_context_info(data.context);
        let current = self.item(track);
        let next: Vec<QItem> = data.rows.into_iter().map(|row| self.item(row)).collect();
        self.queue = Some(QueueData { current: Some(current), next });
    }

    // ---- снимок ------------------------------------------------------------------------------------

    /// Контекст для карточки «Играет из …».
    pub fn shown_context(&self) -> PlayContext {
        let uri = self.playing_context_uri().or_else(|| self.resume_only().then(|| self.resume_context.clone()).flatten());
        match uri {
            Some(uri) => {
                // Запуск по одному URI (плитка «Слушать», Player.playContext) приходит без подписи —
                // такой context_info не перекрывает известную подпись и название играющего альбома
                let started = self.context_info.context_uri.as_deref() == Some(uri.as_str());
                if started && !self.context_info.title.is_empty() {
                    return self.context_info.clone();
                }
                if let Some(known) = self.known_contexts.get(&uri) {
                    return known.clone();
                }
                let mut context = if started { self.context_info.clone() } else { map::context_from_uri(&uri) };
                if context.kind == contexts::ALBUM
                    && let Some(now) = self.now_playing()
                    && sf_api::util::uri_id(&uri) == Some(now.track.album_id.as_str())
                {
                    context.title = now.track.album.clone();
                    if context.cover.is_empty() {
                        context.cover = now.track.cover.clone();
                    }
                }
                context
            }
            None => {
                // список без контекста (поиск, история) — подпись от вызывающего, пока список играет
                let list = self.local_list.is_some()
                    || self.assumed_context.as_ref().is_some_and(|assumed| {
                        assumed.uri.is_empty() && assumed.at.elapsed() < ASSUMED_CONTEXT_HOLD
                    });
                if list && self.context_info.context_uri.is_none() {
                    self.context_info.clone()
                } else {
                    PlayContext::default()
                }
            }
        }
    }

    /// Строки Player.queue: current → manual → context.
    pub fn queue_rows(&mut self) -> Vec<QueueRow> {
        let mut rows = Vec::new();
        let now = self.now_playing();
        let queued = self.queued_rows_len();
        let queue_current = self.queue.as_ref().and_then(|queue| queue.current.clone());
        if let Some(now) = &now {
            let uid = match &queue_current {
                Some(item) if item.uri() == now.track.uri => item.uid,
                _ => match &self.current_uid {
                    Some((uri, uid)) if *uri == now.track.uri => *uid,
                    _ => {
                        let uid = self.uid();
                        self.current_uid = Some((now.track.uri.clone(), uid));
                        uid
                    }
                },
            };
            let recommended = self.recommended.contains(&now.track.uri);
            rows.push(QueueRow { section: QueueSection::Current, uid, recommended, track: now.track.clone() });
        } else if let Some(item) = &queue_current {
            let recommended = self.recommended.contains(&item.track.uri);
            rows.push(QueueRow { section: QueueSection::Current, uid: item.uid, recommended, track: item.track.clone() });
        }
        if let Some(queue) = &self.queue {
            for (index, item) in queue.next.iter().enumerate() {
                let section = if index < queued { QueueSection::Manual } else { QueueSection::Context };
                let recommended = section == QueueSection::Context && self.recommended.contains(&item.track.uri);
                rows.push(QueueRow { section, uid: item.uid, recommended, track: item.track.clone() });
            }
        }
        rows
    }

    /// Найти строку «Далее» по uid (индекс в queue.next и URI).
    pub fn find_row(&self, uid: u64) -> Option<(usize, String)> {
        let queue = self.queue.as_ref()?;
        queue.next.iter().position(|item| item.uid == uid).map(|index| (index, queue.next[index].track.uri.clone()))
    }

    pub fn transferring(&self) -> bool {
        self.transfer.is_some()
    }

    pub fn transfer_target(&self) -> Option<String> {
        self.transfer.as_ref().map(|transfer| transfer.to.clone().unwrap_or_else(|| self.local_device_id.clone().unwrap_or_default()))
    }
}

// ---- свободные функции (перенос из app.rs) -----------------------------------------------------

/// Индекс после ручных строк и до строк контекста.
pub fn end_of_queued_rows(rows: &[QItem], manual: &[String]) -> usize {
    end_of(rows.iter().map(QItem::uri), manual)
}

fn end_of_queued_tracks(rows: &[TrackItem], manual: &[String]) -> usize {
    end_of(rows.iter().map(|row| row.uri.as_str()), manual)
}

fn end_of<'a>(rows: impl Iterator<Item = &'a str>, manual: &[String]) -> usize {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for uri in manual {
        *counts.entry(uri.as_str()).or_insert(0) += 1;
    }
    let mut at = 0;
    for uri in rows {
        match counts.get_mut(uri) {
            Some(left) if *left > 0 => {
                *left -= 1;
                at += 1;
            }
            _ => break,
        }
    }
    at
}

fn playable(item: &TrackItem) -> bool {
    item.available && !item.play_uri().is_empty() && matches!(uri_kind(&item.play_uri()), Some("track" | "episode"))
}

/// Только играбельные строки; индекс выбранной (или следующей доступной) в этом списке.
fn playable_list(items: &[TrackItem], row: usize) -> (Vec<String>, usize) {
    let mut uris = Vec::new();
    let mut index = None;
    for (i, item) in items.iter().enumerate() {
        if !playable(item) {
            continue;
        }
        if i >= row && index.is_none() {
            index = Some(uris.len());
        }
        uris.push(item.play_uri());
    }
    let index = index.unwrap_or(0);
    (uris, index)
}

/// Контексты, которые Spotify дополняет рекомендациями (Smart Shuffle). «Любимые треки» с линзой
/// context-resolve отвечает 404 (проверено 2026-11).
fn smart_capable(uri: &str) -> bool {
    uri.starts_with("spotify:playlist:") && !uri.contains('?')
}

/// Один трек — как контекст (чтобы автоплей шёл от него); список с выбранной строкой не грузится перемешанным.
pub fn local_load(request: &PlayRequest, shuffle: bool) -> LoadSpec {
    let single_song = request.context_uri.is_none() && request.uris.len() == 1 && request.uris[0].contains(":track:");
    if single_song {
        return LoadSpec {
            context_uri: Some(request.uris[0].clone()),
            position_ms: request.position_ms,
            play: true,
            ..LoadSpec::default()
        };
    }
    // librespot перемешивает список и потом не находит выбранную строку — грузим ровно, shuffle после
    let chosen = request.offset_uri.is_some() || request.offset_position.is_some();
    let list = request.context_uri.is_none();
    LoadSpec {
        context_uri: request.context_uri.clone(),
        uris: request.uris.clone(),
        offset_uri: request.offset_uri.clone(),
        offset_index: request.offset_position,
        position_ms: request.position_ms,
        play: true,
        shuffle: (shuffle && !(list && chosen)).then_some(true),
        repeat: None,
        autoplay: false,
    }
}

/// Последний трек простого списка доигран — сид автоплея.
pub fn autoplay_seed(list: Option<&[String]>, autoplay: bool, before: &LocalState, after: &LocalState) -> Option<String> {
    if !autoplay || !after.connected || after.playback != Playback::Stopped || before.playback != Playback::Playing {
        return None;
    }
    let track = before.track.as_ref()?;
    if list?.last() != Some(&track.uri) || track.duration_ms == 0 {
        return None;
    }
    let near_end = before.position_now() + 3_000 >= track.duration_ms;
    near_end.then(|| track.uri.clone())
}

/// Не больше 500 URI, начиная с выбранной строки.
pub fn cap_uris(uris: &[String], index: u32) -> (Vec<String>, u32) {
    if uris.len() <= MAX_LIST {
        return (uris.to_vec(), index);
    }
    let start = (index as usize).min(uris.len().saturating_sub(1));
    let end = (start + MAX_LIST).min(uris.len());
    (uris[start..end].to_vec(), 0)
}

pub fn volume_to_percent(volume: u16) -> u8 {
    ((f64::from(volume) / f64::from(u16::MAX)) * 100.0).round() as u8
}

pub fn percent_to_volume(percent: u8) -> u16 {
    ((f64::from(percent.min(100)) / 100.0) * f64::from(u16::MAX)).round() as u16
}

fn error_is_no_device(error: Option<&str>) -> bool {
    error.is_some_and(|error| error == crate::engine::sink::NO_DEVICE)
}

/// Ошибка движка (LocalState.error — непереведённый английский, его сравнивает ядро) → текст интерфейса.
fn engine_error_text(error: &str) -> String {
    use crate::engine::{player, sink};
    if error == sink::NO_DEVICE {
        return mf_i18n::t("No playback device");
    }
    if error.starts_with("This item isn't available") {
        return mf_i18n::t("This track isn't available");
    }
    if error == player::AUDIO_KEY_REFUSED {
        return mf_i18n::t(player::AUDIO_KEY_REFUSED);
    }
    if error == sink::OUTPUT_STOPPED {
        return mf_i18n::t(sink::OUTPUT_STOPPED);
    }
    if let Some(rest) = error.strip_prefix(player::UNAVAILABLE) {
        return format!("{}{rest}", mf_i18n::t(player::UNAVAILABLE));
    }
    if let Some(rest) = error.strip_prefix(sink::OPEN_FAILED) {
        return format!("{}{rest}", mf_i18n::t(sink::OPEN_FAILED));
    }
    error.to_owned()
}
