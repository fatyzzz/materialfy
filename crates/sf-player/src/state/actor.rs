//! Актор плеера: mpsc команд → ядро → снимок в watch, затем события в broadcast (по одному на Qt-сигнал).
//! Сетевые команды управления идут по одной (порядок важен), чтения — параллельно.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
use md3_model::QueueRow;
use md3_model::{PlayContext, QueueSection, TrackItem};
use tokio::sync::{broadcast, mpsc, watch};

use super::backend::{LocalEngine, WebBackend};
use super::core::{ApiCall, ApiReply, Core, Effect};
use super::diff;
use super::session::SessionData;
use crate::engine::{EngineConfig, EngineEvent, LoadSpec};
#[cfg(test)]
use crate::QueueChange;
use crate::{LocalPlayback, PlayerEvent, PlayerSnapshot, Repeat, Target, TrackEnded, codec_label};

/// Пока что-то движется — снимок пересчитывается так часто.
const FRAME: Duration = Duration::from_millis(250);
/// «Дыхание» обложки — ~30 Гц.
const LEVEL_FRAME: Duration = Duration::from_millis(33);
const IDLE: Duration = Duration::from_secs(5);
/// Шаг позиции в снимке во время игры, мс.
const POSITION_STEP_MS: i64 = 200;

pub(crate) enum LocalMsg {
    Connecting,
    Ready { engine: Arc<dyn LocalEngine>, device_id: String, name: String, bitrate: u16, autoplay: bool },
    Failed(String),
    Unavailable,
}

#[derive(Debug)]
pub(crate) enum Command {
    PlayItems { items: Vec<TrackItem>, row: usize, context: PlayContext },
    ShuffleFrom { items: Vec<TrackItem>, context: PlayContext },
    PlayContext { context: PlayContext, offset: Option<String>, shuffle: Option<bool> },
    PlayUris { uris: Vec<String>, index: usize, context: PlayContext },
    /// Строка для оптимистичной сверки: списки присылают только URI.
    Remember(Box<TrackItem>),
    PlayEpisode { uri: String, position_ms: i64 },
    PlayIndex(usize),
    PlayQueueRow(u64),
    AddToQueue(Vec<TrackItem>),
    AddUriToQueue(String),
    InsertManual { item: Box<TrackItem>, at: usize },
    MoveManual { from: usize, to: usize },
    ClearManualQueue,
    RefreshQueue,
    TogglePlay,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(f64),
    SeekMs(i64),
    SeekRelative(i64),
    CycleRepeat,
    SetRepeat(Repeat),
    SetShuffle(bool),
    SetVolume(f64),
    SetLevelsWanted(bool),
    SetAutoplay(bool),
    TransferHere(bool),
    TransferTo(String, bool),
    RefreshRemote,
    SetAppActive(bool),
    ResetAccount,
    ExpectLocal(bool),
    /// перезапуск движка с новыми настройками, с подхватом контекста и ручной очереди
    RestartEngine(Box<EngineConfig>),
}

pub(crate) enum Msg {
    Command(Command),
    Engine(Box<EngineEvent>),
    Api(ApiReply),
    Local(LocalMsg),
    Cover { url: String, result: Result<PathBuf, String> },
    Restore(Option<SessionData>),
    /// ответ — конец трека, который ещё играл (его TrackEnded иначе не наступил бы)
    Shutdown(std::sync::mpsc::Sender<Option<TrackEnded>>),
}

/// Как пересоздать сессию librespot (громкость — для нового движка).
pub(crate) type Reconnect = Arc<dyn Fn(u16) + Send + Sync>;
/// Перезапуск движка с настройками; LoadSpec — что загрузить в новый (None — подхват движком).
pub(crate) type Restart = Arc<dyn Fn(EngineConfig, u16, Option<LoadSpec>) + Send + Sync>;

pub(crate) struct Actor {
    pub core: Core,
    rx: mpsc::UnboundedReceiver<Msg>,
    tx: mpsc::UnboundedSender<Msg>,
    snapshot: Arc<watch::Sender<PlayerSnapshot>>,
    events: broadcast::Sender<PlayerEvent>,
    backend: Arc<dyn WebBackend>,
    control: mpsc::UnboundedSender<ApiCall>,
    /// записи очереди (альбом — десятки POST) — своя упорядоченная очередь, транспорт за ними не ждёт
    queue_writes: mpsc::UnboundedSender<ApiCall>,
    engine: Option<Arc<dyn LocalEngine>>,
    reconnect: Reconnect,
    restart: Restart,
    session_file: Option<PathBuf>,
    published: PlayerSnapshot,
    cover_url: String,
    cover_file: String,
    level: f64,
    restored: bool,
    pending: Vec<PlayerEvent>,
}

impl Actor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Core,
        rx: mpsc::UnboundedReceiver<Msg>,
        tx: mpsc::UnboundedSender<Msg>,
        snapshot: Arc<watch::Sender<PlayerSnapshot>>,
        events: broadcast::Sender<PlayerEvent>,
        backend: Arc<dyn WebBackend>,
        reconnect: Reconnect,
        restart: Restart,
        session_file: Option<PathBuf>,
    ) -> Self {
        let published = snapshot.borrow().clone();
        // управление — строго по порядку; записи очереди — по порядку между собой
        let control = ordered(backend.clone(), tx.clone());
        let queue_writes = ordered(backend.clone(), tx.clone());
        Self {
            core,
            rx,
            tx,
            snapshot,
            events,
            backend,
            control,
            queue_writes,
            engine: None,
            reconnect,
            restart,
            session_file,
            published,
            cover_url: String::new(),
            cover_file: String::new(),
            level: 0.0,
            restored: false,
            pending: Vec::new(),
        }
    }

    pub async fn run(mut self) {
        loop {
            let wait = self.wait();
            match tokio::time::timeout(wait, self.rx.recv()).await.ok() {
                Some(None) => break,
                Some(Some(Msg::Shutdown(done))) => {
                    let _ = done.send(self.shutdown());
                    break;
                }
                Some(Some(message)) => self.handle(message),
                None => {}
            }
            // всё, что уже пришло, — одним снимком
            while let Ok(message) = self.rx.try_recv() {
                if let Msg::Shutdown(done) = message {
                    let _ = done.send(self.shutdown());
                    return;
                }
                self.handle(message);
            }
            self.step();
        }
    }

    /// Обработать всё, что уже пришло (для тестов).
    #[cfg(test)]
    pub fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            self.handle(message);
        }
    }

    fn wait(&self) -> Duration {
        if self.levels_active() {
            return LEVEL_FRAME;
        }
        if self.core.animating() {
            return FRAME;
        }
        self.core.next_deadline().saturating_duration_since(Instant::now()).clamp(Duration::from_millis(10), IDLE)
    }

    fn levels_active(&self) -> bool {
        self.core.levels_wanted && self.engine.is_some() && self.core.local.is_active() && self.published.playing
    }

    /// Таймеры ядра, эффекты, снимок, события.
    pub fn step(&mut self) {
        self.core.tick();
        self.run_effects();
        self.level = if self.levels_active() { self.engine.as_ref().map_or(0.0, |engine| engine.level()) } else { 0.0 };
        self.publish();
    }

    pub fn handle(&mut self, message: Msg) {
        match message {
            Msg::Command(command) => self.command(command),
            Msg::Engine(event) => match *event {
                EngineEvent::State(state) => self.core.handle_local(state),
                EngineEvent::SessionEnded => {
                    let mut state = self.core.local.clone();
                    state.connected = false;
                    self.core.handle_local(state);
                    (self.reconnect)(self.core.volume);
                }
            },
            Msg::Api(reply) => self.core.handle_api(reply),
            Msg::Local(local) => self.local(local),
            Msg::Cover { url, result } => {
                if url == self.cover_url {
                    self.cover_file = match result {
                        Ok(path) => path.to_string_lossy().into_owned(),
                        Err(error) => {
                            tracing::debug!("cover not downloaded: {error:#}");
                            String::new()
                        }
                    };
                }
            }
            Msg::Restore(data) => {
                if !self.restored {
                    self.restored = true;
                    if let Some(data) = data {
                        self.core.restore_session(data);
                    }
                }
                self.core.poll_remote_soon();
            }
            Msg::Shutdown(_) => {}
        }
    }

    fn local(&mut self, local: LocalMsg) {
        match local {
            LocalMsg::Connecting => self.core.handle_playback(LocalPlayback::Connecting),
            LocalMsg::Ready { engine, device_id, name, bitrate, autoplay } => {
                // движок мог подключиться уже после запроса уровней — синхронизируем гейт tap'а
                engine.set_levels_wanted(self.core.levels_wanted);
                self.engine = Some(engine);
                self.core.local_device_name = name;
                self.core.bitrate = bitrate;
                self.core.autoplay = autoplay;
                self.core.handle_playback(LocalPlayback::Ready { device_id });
                self.core.poll_remote_soon();
            }
            LocalMsg::Failed(text) => {
                self.engine = None;
                self.core.handle_playback(LocalPlayback::Failed(text));
            }
            LocalMsg::Unavailable => {
                self.engine = None;
                self.core.handle_playback(LocalPlayback::Unavailable);
            }
        }
    }

    fn command(&mut self, command: Command) {
        let core = &mut self.core;
        match command {
            Command::PlayItems { items, row, context } => core.play_items(items, row, context),
            Command::ShuffleFrom { items, context } => core.shuffle_from(items, context),
            Command::PlayContext { context, offset, shuffle } => core.play_context(context, offset, shuffle),
            Command::PlayUris { uris, index, context } => core.play_uris(uris, index, context),
            Command::Remember(item) => core.remember(&item),
            Command::PlayEpisode { uri, position_ms } => core.play_episode(uri, position_ms),
            Command::PlayIndex(index) => {
                // индекс строки Player.queue: 0 — текущая
                let rows = core.queue_rows();
                if let Some(row) = rows.get(index)
                    && row.section != QueueSection::Current
                {
                    let uid = row.uid;
                    if let Some((at, uri)) = core.find_row(uid) {
                        core.play_queue_item(at, uri);
                    }
                }
            }
            Command::PlayQueueRow(uid) => {
                if let Some((at, uri)) = core.find_row(uid) {
                    core.play_queue_item(at, uri);
                }
            }
            Command::AddToQueue(items) => match items.len() {
                0 => {}
                1 => {
                    let item = items.into_iter().next().expect("one item");
                    core.remember(&item);
                    let uri = item.play_uri();
                    core.add_to_queue(uri, item.title);
                }
                _ => core.queue_many(items),
            },
            Command::AddUriToQueue(uri) => core.add_to_queue(uri, String::new()),
            Command::InsertManual { item, at } => core.insert_manual(vec![*item], at),
            Command::MoveManual { from, to } => core.move_manual(from, to),
            Command::ClearManualQueue => core.clear_queue(),
            Command::RefreshQueue => core.refresh_queue(true),
            Command::TogglePlay => core.toggle_play(),
            Command::Play => core.play(),
            Command::Pause | Command::Stop => core.pause(),
            Command::Next => core.next(),
            Command::Previous => core.previous(),
            Command::Seek(fraction) => {
                if let Some(now) = core.now_playing() {
                    let position = (fraction.clamp(0.0, 1.0) * f64::from(now.duration_ms)).round() as u32;
                    core.seek(position);
                }
            }
            Command::SeekMs(ms) => core.seek(ms.clamp(0, i64::from(u32::MAX)) as u32),
            Command::SeekRelative(delta) => core.seek_relative(delta),
            Command::CycleRepeat => {
                let mode = core.repeat().next();
                core.set_repeat(mode);
            }
            Command::SetRepeat(repeat) => core.set_repeat(repeat.to_engine()),
            Command::SetShuffle(value) => core.set_shuffle(value),
            Command::SetVolume(value) => core.set_volume(crate::volume_to_u16(value)),
            Command::SetLevelsWanted(value) => {
                core.levels_wanted = value;
                // движок узнаёт о гейте сразу: буфер tap'а не наполняется, пока уровень не нужен
                if let Some(engine) = &self.engine {
                    engine.set_levels_wanted(value);
                }
            }
            Command::SetAutoplay(value) => core.autoplay = value,
            Command::TransferHere(play) => core.transfer_here(play),
            Command::TransferTo(device, play) => core.transfer_to(device, play),
            Command::RefreshRemote => core.poll_remote(),
            Command::SetAppActive(active) => core.set_app_active(active),
            Command::ResetAccount => core.reset_account(),
            Command::ExpectLocal(expected) => core.expect_local(expected),
            Command::RestartEngine(config) => {
                let resume = core.prepare_restart();
                (self.restart)(*config, core.volume, resume);
            }
        }
    }

    fn run_effects(&mut self) {
        for effect in self.core.take_effects() {
            match effect {
                Effect::Engine(command) => match &self.engine {
                    Some(engine) => {
                        if let Err(error) = engine.command(command) {
                            tracing::warn!("engine command failed: {error:#}");
                        }
                    }
                    None => tracing::debug!("engine not ready, command {command:?} skipped"),
                },
                Effect::Api(call) => self.api(call),
                Effect::Notice(text) => self.pending.push(PlayerEvent::Notice(text)),
                Effect::Reconnect => (self.reconnect)(self.core.volume),
                Effect::Cover(url) if url.is_empty() => {
                    self.cover_url.clear();
                    self.cover_file.clear();
                }
                Effect::Cover(url) if url == self.cover_url && !self.cover_file.is_empty() => {}
                Effect::Cover(url) => {
                    self.cover_url = url.clone();
                    let backend = self.backend.clone();
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let result = backend.fetch_cover(url.clone()).await;
                        let _ = tx.send(Msg::Cover { url, result });
                    });
                }
                Effect::Save(data) => {
                    if let Some(path) = self.session_file.clone() {
                        tokio::task::spawn_blocking(move || {
                            if let Err(error) = data.save(&path) {
                                tracing::warn!("player session not saved: {error:#}");
                            }
                        });
                    }
                }
                Effect::Started(started) => self.pending.push(PlayerEvent::TrackStarted((*started).into())),
                Effect::Ended(ended) => self.pending.push(PlayerEvent::TrackEnded((*ended).into())),
            }
        }
    }

    pub(crate) fn api(&mut self, call: ApiCall) {
        match call {
            // чтения — параллельно
            ApiCall::Playback { .. } | ApiCall::Queue { .. } | ApiCall::AlbumTracks { .. } | ApiCall::TrackInfo { .. } => {
                let backend = self.backend.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let reply = backend.call(call).await;
                    let _ = tx.send(Msg::Api(reply));
                });
            }
            call @ ApiCall::AddMany { .. } => {
                let _ = self.queue_writes.send(call);
            }
            call => {
                let _ = self.control.send(call);
            }
        }
    }

    pub fn build(&mut self) -> PlayerSnapshot {
        let core = &mut self.core;
        let now = core.now_playing();
        let rows = core.queue_rows();
        let queue = if rows.as_slice() == self.published.queue.as_slice() {
            self.published.queue.clone()
        } else {
            Arc::new(rows)
        };
        let target = core.target();
        let context_remaining = queue.iter().filter(|row| row.section == QueueSection::Context).count() as i32;
        let manual_count = queue.iter().filter(|row| row.section == QueueSection::Manual).count() as i32;
        let live = now.as_ref().filter(|now| !now.resuming);
        let is_local = live.is_some_and(|now| now.local) || (target == Target::Local && core.local.is_active());
        let volume = now.as_ref().map_or(core.volume, |now| if now.resuming { core.volume } else { now.volume });
        PlayerSnapshot {
            track: now.as_ref().map(|now| now.track.clone()),
            codec: if is_local && now.is_some() { codec_label(core.bitrate) } else { String::new() },
            cover_file: self.cover_file.clone(),
            playing: now.as_ref().is_some_and(|now| now.playing),
            buffering: now.as_ref().is_some_and(|now| now.loading),
            narrating: is_local && core.local.narrating,
            position_ms: now.as_ref().map_or(0, |now| i64::from(now.position_ms)),
            duration_ms: now.as_ref().map_or(0, |now| i64::from(now.duration_ms)),
            volume: f64::from(volume) / f64::from(u16::MAX),
            shuffle: core.shuffle_wanted,
            repeat: Repeat::from_engine(now.as_ref().map_or(core.local.repeat, |now| now.repeat)),
            current_index: if queue.first().is_some_and(|row| row.section == QueueSection::Current) { 0 } else { -1 },
            error_text: core.error_text.clone(),
            direction: core.direction,
            level: self.level,
            levels_wanted: core.levels_wanted,
            context: core.shown_context(),
            played_seconds: core.played_seconds(),
            device: live.and_then(|now| now.device.clone()),
            target,
            is_local,
            local: core.local_playback.clone(),
            can_control: live.is_none_or(|now| now.can_control),
            can_set_volume: live.is_none_or(|now| now.can_set_volume),
            transferring: core.transferring(),
            device_lost: core.device_lost,
            context_remaining,
            manual_count,
            can_clear_queue: core.can_clear_queue(),
            queue_reorderable: core.queue_locally_reorderable(),
            queue,
        }
    }

    /// Снимок — в watch, затем события (снимок обновляется до рассылки).
    pub fn publish(&mut self) {
        let mut next = self.build();
        // кадры «дыхания» идут ~30 Гц; позиция в QML — не чаще ~5 раз в секунду
        let old = &self.published;
        if old.track == next.track
            && old.playing
            && next.playing
            && old.duration_ms == next.duration_ms
            && (0..POSITION_STEP_MS).contains(&(next.position_ms - old.position_ms))
        {
            next.position_ms = old.position_ms;
        }
        let old = std::mem::replace(&mut self.published, next.clone());
        let mut events = Vec::new();
        if old.track != next.track || old.codec != next.codec {
            events.push(PlayerEvent::TrackChanged);
        }
        if old.playing != next.playing || old.buffering != next.buffering || old.narrating != next.narrating {
            events.push(PlayerEvent::StateChanged);
        }
        if old.position_ms != next.position_ms || old.duration_ms != next.duration_ms {
            events.push(PlayerEvent::PositionChanged);
        }
        if old.volume != next.volume {
            events.push(PlayerEvent::VolumeChanged);
        }
        if old.shuffle != next.shuffle || old.repeat != next.repeat {
            events.push(PlayerEvent::ModeChanged);
        }
        let queue_changed = !Arc::ptr_eq(&old.queue, &next.queue) && old.queue != next.queue;
        if queue_changed {
            for change in diff::queue_changes(&old.queue, &next.queue) {
                events.push(PlayerEvent::Queue(change));
            }
        }
        if queue_changed
            || old.current_index != next.current_index
            || old.context != next.context
            || old.context_remaining != next.context_remaining
            || old.manual_count != next.manual_count
            || old.can_clear_queue != next.can_clear_queue
            || old.queue_reorderable != next.queue_reorderable
        {
            events.push(PlayerEvent::QueueChanged);
        }
        if old.cover_file != next.cover_file {
            events.push(PlayerEvent::CoverFileChanged);
        }
        if old.error_text != next.error_text {
            events.push(PlayerEvent::ErrorChanged);
        }
        if old.level != next.level || old.levels_wanted != next.levels_wanted {
            events.push(PlayerEvent::LevelChanged);
        }
        if old.target != next.target
            || old.device != next.device
            || old.is_local != next.is_local
            || old.can_control != next.can_control
            || old.can_set_volume != next.can_set_volume
            || old.transferring != next.transferring
            || old.device_lost != next.device_lost
            || old.local != next.local
        {
            events.push(PlayerEvent::DeviceChanged);
        }
        if old.local != next.local {
            events.push(PlayerEvent::LocalChanged);
        }
        let changed = !events.is_empty() || old.played_seconds != next.played_seconds;
        if changed {
            self.snapshot.send_replace(next);
        }
        for event in events.into_iter().chain(self.pending.drain(..)) {
            let _ = self.events.send(event);
        }
    }

    /// Сохранить сессию и закрыть текущее прослушивание: события после выхода уже никто не получит,
    /// поэтому конец трека возвращается вызывающему.
    pub(crate) fn shutdown(&mut self) -> Option<TrackEnded> {
        self.core.save_session();
        self.core.finish_listening(true);
        let mut ended = None;
        for effect in self.core.take_effects() {
            match (effect, self.session_file.as_ref()) {
                (Effect::Save(data), Some(path)) => {
                    if let Err(error) = data.save(path) {
                        tracing::warn!("player session not saved: {error:#}");
                    }
                }
                (Effect::Ended(last), _) => ended = Some((*last).into()),
                _ => {}
            }
        }
        self.engine = None;
        ended
    }
}

/// Задача, выполняющая вызовы строго по одному в порядке отправки.
fn ordered(backend: Arc<dyn WebBackend>, tx: mpsc::UnboundedSender<Msg>) -> mpsc::UnboundedSender<ApiCall> {
    let (sender, mut calls) = mpsc::unbounded_channel::<ApiCall>();
    tokio::spawn(async move {
        while let Some(call) = calls.recv().await {
            let reply = backend.call(call).await;
            if tx.send(Msg::Api(reply)).is_err() {
                break;
            }
        }
    });
    sender
}

/// Порядок строк (для тестов).
#[cfg(test)]
pub(crate) fn uris(rows: &[QueueRow]) -> Vec<(QueueSection, String)> {
    rows.iter().map(|row| (row.section, row.track.uri.clone())).collect()
}

/// Применить изменения очереди, как это делает мост (для тестов).
#[cfg(test)]
pub(crate) fn apply_changes(rows: &mut Vec<QueueRow>, changes: &[QueueChange]) {
    diff::apply(rows, changes);
}
