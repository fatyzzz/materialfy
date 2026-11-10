//! PlayerHandle — фасад плеера для сервисов и мостов (форма PlayerHandle из yamusic-md3 с поправками под Spotify).
//! Команды не блокируют: уходят в актор на tokio (mpsc), результат — в снимке/событиях.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use md3_model::{PlayContext, TrackItem};
use sf_api::ApiGateway;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, watch};

use crate::engine::{EngineConfig, Rootlist};
use crate::state::actor::{Actor, Command, Msg, Reconnect, Restart};
use crate::state::backend::{SpotifyBackend, WebBackend};
use crate::state::connect::{self, ConnectBackend};
use crate::state::core::Core;
use crate::state::host::EngineHost;
use crate::state::session::SessionData;
use crate::{
    Credentials, EngineStarted, LocalPlayback, PlayerConfig, PlayerError, PlayerEvent, PlayerSnapshot,
    PlayerSubscription, Repeat, Session,
};

/// Clone — дешёвый.
#[derive(Clone)]
pub struct PlayerHandle {
    inner: Arc<Inner>,
}

struct Inner {
    tx: mpsc::UnboundedSender<Msg>,
    snapshot: Arc<watch::Sender<PlayerSnapshot>>,
    events: broadcast::Sender<PlayerEvent>,
    host: Arc<EngineHost>,
    rt: Handle,
    session_file: Option<PathBuf>,
    connect: Arc<ConnectBackend>,
}

/// Английский ключ перевода; наружу — `mf_i18n::t(NOT_READY)`.
const NOT_READY: &str = "Playback on this computer is off";

impl PlayerHandle {
    /// Запустить актор на `rt`. Web API (опрос /me/player, удалённые команды, очередь) — через `api`.
    pub fn start(rt: &Handle, config: PlayerConfig, api: Arc<ApiGateway>) -> Result<Self, PlayerError> {
        let backend = Arc::new(SpotifyBackend::new(api, config.http.clone(), config.cover_dir.clone()));
        Ok(Self::with_backend(rt, config, backend))
    }

    /// Запуск с любым Web API (тесты подставляют фейковый).
    pub(crate) fn with_backend(rt: &Handle, config: PlayerConfig, backend: Arc<dyn WebBackend>) -> Self {
        let initial = PlayerSnapshot { volume: config.volume, direction: 1, current_index: -1, ..Default::default() };
        let snapshot = Arc::new(watch::channel(initial).0);
        let (events, _) = broadcast::channel(1024);
        let (tx, rx) = mpsc::unbounded_channel();
        let host = Arc::new(EngineHost::new(tx.clone()));
        // Connect (состояние, очередь, команды) — через сессию движка; Web API — запасной путь
        let connect = connect::wrap(backend, &host, &tx);
        let backend: Arc<dyn WebBackend> = connect.clone();
        let reconnect: Reconnect = {
            let host = host.clone();
            let rt = rt.clone();
            Arc::new(move |volume| host.reconnect(&rt, volume))
        };
        let restart: Restart = {
            let host = host.clone();
            let rt = rt.clone();
            Arc::new(move |config, volume, load| host.restart(&rt, Some(config), volume, load))
        };
        let core = Core::new(config.volume, config.session_file.is_some());
        let session_file = config.session_file.clone();
        {
            let snapshot = snapshot.clone();
            let events = events.clone();
            let tx = tx.clone();
            let session_file = session_file.clone();
            rt.spawn(async move {
                Actor::new(core, rx, tx, snapshot, events, backend, reconnect, restart, session_file).run().await;
            });
        }
        let handle = Self { inner: Arc::new(Inner { tx, snapshot, events, host, rt: rt.clone(), session_file, connect }) };
        handle.load_session();
        handle
    }

    pub(crate) fn connect(&self) -> &Arc<ConnectBackend> {
        &self.inner.connect
    }

    fn send(&self, command: Command) {
        let _ = self.inner.tx.send(Msg::Command(command));
    }

    fn load_session(&self) {
        let tx = self.inner.tx.clone();
        let path = self.inner.session_file.clone();
        self.inner.rt.spawn_blocking(move || {
            let data = path.as_deref().and_then(SessionData::load);
            let _ = tx.send(Msg::Restore(data));
        });
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.inner.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> PlayerSubscription {
        PlayerSubscription { snapshot: self.inner.snapshot.subscribe(), events: self.inner.events.subscribe() }
    }

    // --- локальный движок (librespot) ---------------------------------------------------------

    /// Поднять Engine (Session + Spirc) с кредами; вызывает AuthService после проверки Premium.
    /// Ok — устройство готово; reusable — сохранить в слот Playback.
    pub async fn connect_local(&self, credentials: Credentials, config: EngineConfig) -> Result<EngineStarted, String> {
        self.inner.host.connect(credentials, config, None).await
    }

    /// Перезапуск с новыми настройками (битрейт, нормализация, gapless, устройство вывода, кэш, имя)
    /// с подхватом контекста, трека, позиции, режимов и ручной очереди.
    pub fn restart_engine(&self, config: EngineConfig) {
        self.send(Command::RestartEngine(Box::new(config)));
    }

    /// Остановить локальный движок (выход, «Отключить воспроизведение»).
    pub fn disconnect_local(&self) {
        self.inner.host.disconnect();
    }

    pub fn local_playback(&self) -> LocalPlayback {
        self.inner.snapshot.borrow().local.clone()
    }

    /// Сессия librespot (Clone, Arc внутри) — для session_reads, станций, rootlist; None — движок не готов.
    pub fn session(&self) -> Option<Session> {
        self.inner.host.engine().map(|link| link.engine.session().clone())
    }

    /// id Connect-устройства этого ПК (hex(sha1(device_name))), если движок готов.
    pub fn this_device_id(&self) -> Option<String> {
        self.inner.host.engine().map(|link| link.engine.device_id().to_owned())
    }

    /// Текст Spotify (color-lyrics) через сессию; Ok(None) — нет текста.
    pub async fn lyrics_json(&self, track_uri: &str) -> Result<Option<serde_json::Value>, String> {
        let link = self.inner.host.engine().ok_or_else(|| mf_i18n::t(NOT_READY))?;
        link.engine.lyrics_json(track_uri).await.map_err(|error| format!("{error:#}"))
    }

    pub async fn rootlist(&self) -> Result<Rootlist, String> {
        let link = self.inner.host.engine().ok_or_else(|| mf_i18n::t(NOT_READY))?;
        link.engine.rootlist().await.map_err(|error| format!("{error:#}"))
    }

    // --- запуск ------------------------------------------------------------------------------

    /// Список из модели QML, начиная с row: context.context_uri есть — Load{context_uri, offset_uri};
    /// нет (поиск/фильтр/радио-список) — Load{uris (≤500 от row), offset}. Ручная очередь сохраняется.
    pub fn play_items(&self, items: Vec<TrackItem>, row: usize, context: PlayContext) {
        self.send(Command::PlayItems { items, row, context });
    }

    /// «Перемешать»: shuffle on и старт со случайного доступного трека.
    pub fn shuffle_from(&self, items: Vec<TrackItem>, context: PlayContext) {
        self.send(Command::ShuffleFrom { items, context });
    }

    /// Контекст по URI (плейлист/альбом/артист/шоу/станция), с трека offset_uri или с начала.
    pub fn play_context(&self, context: PlayContext, offset_uri: Option<String>, shuffle: Option<bool>) {
        self.send(Command::PlayContext { context, offset: offset_uri, shuffle });
    }

    /// Список URI с индекса (радио «играет показанные песни», история).
    pub fn play_uris(&self, uris: Vec<String>, index: usize, context: PlayContext) {
        self.send(Command::PlayUris { uris, index, context });
    }

    /// Запомнить строку: выбранный трек показывается сразу, даже когда список знает только его URI.
    pub fn remember(&self, track: TrackItem) {
        self.send(Command::Remember(Box::new(track)));
    }

    /// Выпуск подкаста с позиции (resume_ms).
    pub fn play_episode(&self, uri: String, position_ms: i64) {
        self.send(Command::PlayEpisode { uri, position_ms });
    }

    // --- очередь (queue.md) -----------------------------------------------------------------

    /// Совместимость: строка Player.queue по индексу = play_queue_row(uid этой строки).
    pub fn play_index(&self, index: usize) {
        self.send(Command::PlayIndex(index));
    }

    /// Клик по строке очереди = N×Next: строки выше снимаются, контекст продолжается (п. 5).
    pub fn play_queue_row(&self, uid: u64) {
        self.send(Command::PlayQueueRow(uid));
    }

    /// «В очередь»: после ручных, до контекста; двойной клик < 1,5 с — одна строка (п. 2).
    pub fn add_to_queue(&self, items: Vec<TrackItem>) {
        self.send(Command::AddToQueue(items));
    }

    /// «В очередь» по URI: трек/выпуск — одна строка; альбом — все треки по порядку целиком или никак.
    pub fn add_uri_to_queue(&self, uri: String) {
        self.send(Command::AddUriToQueue(uri));
    }

    /// Вставка в позицию «Далее в очереди» (только локально; иначе — в конец).
    pub fn insert_manual(&self, item: TrackItem, at: usize) {
        self.send(Command::InsertManual { item: Box::new(item), at });
    }

    /// Перестановка внутри «Далее в очереди» (только локально): `from`, `to` — индексы внутри секции,
    /// `to` — итоговое место строки (как ListChange::Move: remove(from), insert(to)).
    pub fn move_manual(&self, from: usize, to: usize) {
        self.send(Command::MoveManual { from, to });
    }

    /// «Очистить» — только ручные строки (п. 7).
    pub fn clear_manual_queue(&self) {
        self.send(Command::ClearManualQueue);
    }

    /// Перезапросить очередь (/me/player/queue).
    pub fn refresh_queue(&self) {
        self.send(Command::RefreshQueue);
    }

    // --- транспорт ---------------------------------------------------------------------------

    pub fn toggle_play(&self) {
        self.send(Command::TogglePlay);
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// У Spotify нет «стоп» — пауза.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Next сразу снимает верхнюю строку очереди (п. 4).
    pub fn next(&self) {
        self.send(Command::Next);
    }

    /// После 3 с — в начало трека (делает librespot / Web API), иначе предыдущий.
    pub fn previous(&self) {
        self.send(Command::Previous);
    }

    pub fn seek(&self, fraction: f64) {
        self.send(Command::Seek(fraction));
    }

    pub fn seek_ms(&self, ms: i64) {
        self.send(Command::SeekMs(ms));
    }

    /// ±мс от текущей позиции (выпуски: −10 с / +30 с).
    pub fn seek_relative(&self, delta_ms: i64) {
        self.send(Command::SeekRelative(delta_ms));
    }

    pub fn cycle_repeat(&self) {
        self.send(Command::CycleRepeat);
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Command::SetRepeat(repeat));
    }

    pub fn set_shuffle(&self, value: bool) {
        self.send(Command::SetShuffle(value));
    }

    /// Smart Shuffle (только локально и у плейлистов; иначе не действует).
    pub fn set_smart_shuffle(&self, value: bool) {
        self.send(Command::SetSmartShuffle(value));
    }

    /// 0..1; при перетаскивании — VolumePreview, через ~150 мс тишины — Volume (удалённо — PUT volume).
    pub fn set_volume(&self, value: f64) {
        self.send(Command::SetVolume(value.clamp(0.0, 1.0)));
    }

    pub fn set_levels_wanted(&self, value: bool) {
        self.send(Command::SetLevelsWanted(value));
    }

    /// Автоплей по окончании списка (autoplay_seed) — сразу, без перезапуска движка (зовёт glue).
    pub fn set_autoplay(&self, value: bool) {
        self.send(Command::SetAutoplay(value));
    }

    // --- Connect ---------------------------------------------------------------------------

    /// Перенести воспроизведение на этот ПК (librespot Transfer); устройство пропало — продолжить здесь
    /// с последнего удалённого снимка.
    pub fn transfer_here(&self, play: bool) {
        self.send(Command::TransferHere(play));
    }

    /// Перенести на устройство Connect (PUT /me/player).
    pub fn transfer_to(&self, device_id: String, play: bool) {
        self.send(Command::TransferTo(device_id, play));
    }

    /// Опросить /me/player сейчас.
    pub fn refresh_remote(&self) {
        self.send(Command::RefreshRemote);
    }

    /// Окно видно — опрос в обычном темпе; скрыто и ничего не играет — реже.
    pub fn set_app_active(&self, active: bool) {
        self.send(Command::SetAppActive(active));
    }

    // --- жизненный цикл ---------------------------------------------------------------------

    /// Старт: подхватить текущее воспроизведение аккаунта и сохранённую ручную очередь.
    pub fn restore_session(&self) {
        self.load_session();
    }

    /// Выход из аккаунта: забыть очередь, удалённое состояние и точку возобновления этого аккаунта.
    pub fn reset_account(&self) {
        self.send(Command::ResetAccount);
    }

    /// Вход скоро поднимет движок (true) или передумал (false): «Играть» до подключения ждёт движок,
    /// а не просит выбрать устройство (как playback_authorized в spotifast).
    pub fn expect_local(&self, expected: bool) {
        self.send(Command::ExpectLocal(expected));
    }

    /// Выход: сохранить сессию, остановить движок. Блокирует ≤ 2 с.
    pub fn shutdown(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self.inner.tx.send(Msg::Shutdown(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_millis(1500));
        }
        self.inner.host.shutdown();
    }
}
