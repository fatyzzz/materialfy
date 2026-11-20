//! Spotify Connect через сессию librespot, без Web API: кластер connect-state (GET /connect-state/v1/cluster
//! и пуши дилера hm://connect-state/v1/cluster) даёт состояние удалённого плеера, очередь и список устройств;
//! команды другим устройствам уходят так же, как у настольного клиента:
//! POST /connect-state/v1/player/command/from/{это}/to/{цель}, громкость — PUT …/connect/volume/from/…/to/…,
//! перенос — POST …/connect/transfer/from/{активное}/to/{цель}.
//! Очередь своего устройства (играет этот ПК) берётся прямо из Spirc — без сети, кроме данных треков.
//! Web API остаётся запасным путём: движок не запущен или сессия не ответила.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use librespot_core::SpotifyUri;
use librespot_core::dealer::protocol::Message;
use librespot_protocol::connect::{Cluster, ClusterUpdate, DeviceInfo};
use librespot_protocol::devices::DeviceType;
use librespot_protocol::player::{PlayerState, ProvidedTrack};
use md3_model::{PlayContext, TrackItem};
use protobuf::Message as _;
use reqwest::Method;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use sf_api::models::Device;
use sf_api::util::{uri_id, uri_kind};
use sf_api::PlayRequest;
use tokio::sync::{mpsc, watch};

use super::actor::{Command, Msg};
use super::backend::{BoxFuture, WebBackend};
use super::core::{ApiCall, ApiReply, FetchedQueue, RemoteAction, RemoteState};
use super::host::EngineHost;
use super::map;
use crate::engine::RepeatMode;
use crate::engine::player::ClusterPushes;
use crate::{PlayerHandle, Session};

/// Кластер из пуша считается верным столько; дольше — перечитываем (дилер мог тихо отвалиться).
const CLUSTER_FRESH: Duration = Duration::from_secs(30);
/// Сколько следующих строк очереди отдавать (как /me/player/queue — первые строки «Далее»).
const QUEUE_LIMIT: usize = 30;
const TRACK_CACHE_LIMIT: usize = 1000;
/// Сколько названий контекстов помнить (карточка «Играет из …»).
const CONTEXT_CACHE_LIMIT: usize = 200;
const CLUSTER_URI: &str = "hm://connect-state/v1/cluster";
const PROTOBUF: &str = "application/x-protobuf";

/// Сессия движка и id его Connect-устройства (отправитель команд).
#[derive(Clone)]
pub(crate) struct Link {
    pub session: Session,
    pub device_id: String,
    /// состояние плеера этого устройства из Spirc; None — устройство не активно
    pub local: watch::Receiver<Option<Arc<PlayerState>>>,
    /// пуши кластера, на которые движок подписался до запуска дилера (забирает первый)
    pub pushes: Arc<Mutex<Option<ClusterPushes>>>,
}

/// Текущая сессия движка; None — движок не запущен.
pub(crate) type LinkSource = Arc<dyn Fn() -> Option<Link> + Send + Sync>;
/// Что-то изменилось: `remote` — кластер (пуш дилера), `queue` — очередь.
pub(crate) type Changed = Arc<dyn Fn(bool, bool) + Send + Sync>;

struct Stamped {
    cluster: Arc<Cluster>,
    at: Instant,
    session_id: String,
}

#[derive(Default)]
struct State {
    /// сессия, на пуши которой подписаны
    listening: Option<String>,
    cluster: Option<Stamped>,
    tracks: HashMap<String, TrackItem>,
    track_order: VecDeque<String>,
    /// сессия, за состоянием Spirc которой следим
    watching: Option<String>,
    /// названия и обложки контекстов по URI; None — у контекста нет названия
    contexts: HashMap<String, Option<(String, String)>>,
    /// когда опрос состояния в последний раз ушёл в Web API из-за отказа сессии
    web_poll_at: Option<Instant>,
}

/// Опрос состояния и очереди при сбое сессии идёт в Web API не чаще этого: кластер обычно оживает сам,
/// а каждый такой запрос тратит общую квоту.
const WEB_POLL_FALLBACK_EVERY: Duration = Duration::from_secs(60);

/// WebBackend поверх сессии: чтения и команды Connect идут через librespot, остальное и отказы — в Web API.
pub struct ConnectBackend {
    web: Arc<dyn WebBackend>,
    link: LinkSource,
    changed: Changed,
    state: Mutex<State>,
    generation: watch::Sender<u64>,
    me: Weak<ConnectBackend>,
}

impl ConnectBackend {
    pub(crate) fn new(web: Arc<dyn WebBackend>, link: LinkSource, changed: Changed) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            web,
            link,
            changed,
            state: Mutex::new(State::default()),
            generation: watch::channel(0).0,
            me: me.clone(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Растёт с каждым пушем кластера (список устройств в меню обновляется сам).
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.generation.subscribe()
    }

    /// Устройства из кластера; None — сессии нет или она не ответила (тогда — Web API).
    pub async fn devices(&self) -> Option<Vec<Device>> {
        let link = (self.link)()?;
        match self.cluster(&link).await {
            Ok((cluster, _)) => Some(devices(&cluster)),
            Err(error) => {
                tracing::debug!("devices via the session unavailable: {error}");
                None
            }
        }
    }

    /// Громкость устройства 0..100 через connect-state; None — сессии нет.
    pub async fn set_volume(&self, device: &str, percent: u8) -> Option<Result<(), String>> {
        let link = (self.link)()?;
        Some(send_volume(&link, device, percent).await)
    }

    async fn run(self: Arc<Self>, link: Link, call: ApiCall) -> ApiReply {
        self.watch_local(&link);
        let result = match &call {
            ApiCall::Playback { seq } => {
                self.playback(&link).await.map(|state| ApiReply::Playback { seq: *seq, result: Ok(state) })
            }
            ApiCall::Queue { seq } => self.queue(&link).await.map(|queue| ApiReply::Queue { seq: *seq, result: Ok(queue) }),
            ApiCall::Remote { action, device } => self
                .remote(&link, action, device.as_deref())
                .await
                .map(|()| ApiReply::Remote { action: action.clone(), result: Ok(()) }),
            ApiCall::Play { device, request, shuffle } => self
                .play(&link, device.as_deref(), request, *shuffle)
                .await
                .map(|()| ApiReply::Remote { action: RemoteAction::Play, result: Ok(()) }),
            ApiCall::Transfer { device, play } => self.transfer(&link, device, *play).await.map(|name| {
                ApiReply::Transferred { device: device.clone(), name, result: Ok(()) }
            }),
            ApiCall::AddMany { request, uris, device } => {
                return self.add_many(&link, *request, uris, device.clone()).await;
            }
            // треки альбома — из метаданных сессии (без квоты Web API)
            ApiCall::AlbumTracks { request, id } => {
                match crate::engine::session_reads::album_with_tracks(&link.session, id).await {
                    Ok((_, tracks)) => Ok(ApiReply::AlbumTracks {
                        request: *request,
                        result: Ok(tracks.iter().map(super::map::track).collect()),
                    }),
                    Err(error) => Err(format!("album tracks via the session: {error:?}")),
                }
            }
            // метаданные одного трека — Web API: их нет в кластере устройства
            ApiCall::TrackInfo { .. } => return self.web.call(call).await,
        };
        match result {
            Ok(reply) => reply,
            Err(error) => {
                if let ApiCall::Playback { seq } | ApiCall::Queue { seq } = &call {
                    let mut state = self.lock();
                    if state.web_poll_at.is_some_and(|at| at.elapsed() < WEB_POLL_FALLBACK_EVERY) {
                        tracing::debug!("the session route failed ({error}); the Web API was asked recently");
                        let failed = sf_api::ApiError::Network(error);
                        return match &call {
                            ApiCall::Playback { .. } => ApiReply::Playback { seq: *seq, result: Err(failed) },
                            _ => ApiReply::Queue { seq: *seq, result: Err(failed) },
                        };
                    }
                    state.web_poll_at = Some(Instant::now());
                }
                tracing::debug!("the session route failed ({error}); falling back to the Web API");
                self.web.call(call).await
            }
        }
    }

    // ---- кластер ------------------------------------------------------------------------------------

    /// Свежий кластер этой сессии (из пуша) или новый GET; второе — оценка серверного «сейчас», мс.
    async fn cluster(&self, link: &Link) -> Result<(Arc<Cluster>, i64), String> {
        let session_id = link.session.session_id();
        self.listen(link, &session_id);
        {
            let state = self.lock();
            if let Some(stamped) = &state.cluster
                && stamped.session_id == session_id
                && state.listening.as_deref() == Some(session_id.as_str())
                && stamped.at.elapsed() < CLUSTER_FRESH
            {
                return Ok((stamped.cluster.clone(), server_now(&stamped.cluster, stamped.at)));
            }
        }
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(PROTOBUF));
        let bytes = link
            .session
            .spclient()
            .request(&Method::GET, "/connect-state/v1/cluster", Some(headers), None)
            .await
            .map_err(|error| error.to_string())?;
        let cluster = Arc::new(Cluster::parse_from_bytes(&bytes).map_err(|error| error.to_string())?);
        let at = Instant::now();
        self.lock().cluster = Some(Stamped { cluster: cluster.clone(), at, session_id });
        Ok((cluster.clone(), server_now(&cluster, at)))
    }

    /// Подписаться на пуши кластера этой сессии (один раз на сессию).
    fn listen(&self, link: &Link, session_id: &str) {
        {
            let mut state = self.lock();
            if state.listening.as_deref() == Some(session_id) {
                return;
            }
            state.listening = Some(session_id.to_owned());
        }
        // подписка движка до запуска дилера; позже (её уже забрали) — новая у запущенного дилера
        let early = link.pushes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        let subscribed = match early {
            Some(updates) => Ok(updates),
            None => link.session.dealer().listen_for(CLUSTER_URI, Message::from_raw::<ClusterUpdate>),
        };
        let updates = match subscribed {
            Ok(updates) => updates,
            Err(error) => {
                tracing::debug!("cluster pushes unavailable: {error}");
                self.lock().listening = None;
                return;
            }
        };
        let me = self.me.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let mut updates = updates;
            while let Some(update) = updates.next().await {
                let Some(me) = me.upgrade() else { return };
                let mut update = match update {
                    Ok(update) => update,
                    Err(error) => {
                        tracing::debug!("couldn't read a cluster push: {error}");
                        continue;
                    }
                };
                let Some(cluster) = update.cluster.take() else { continue };
                me.adopt(cluster, &session_id);
            }
            if let Some(me) = me.upgrade() {
                let mut state = me.lock();
                if state.listening.as_deref() == Some(session_id.as_str()) {
                    state.listening = None;
                }
            }
        });
    }

    fn adopt(&self, cluster: Cluster, session_id: &str) {
        tracing::debug!(
            "cluster push: active device {:?}, playing {}",
            cluster.active_device_id,
            is_playing(&cluster.player_state)
        );
        let queue = {
            let mut state = self.lock();
            let queue = state.cluster.as_ref().is_none_or(|old| queue_key(&old.cluster) != queue_key(&cluster));
            state.cluster =
                Some(Stamped { cluster: Arc::new(cluster), at: Instant::now(), session_id: session_id.to_owned() });
            queue
        };
        self.generation.send_modify(|generation| *generation += 1);
        (self.changed)(true, queue);
    }

    /// Следить за состоянием Spirc этой сессии (один раз на сессию): смена трека, очереди, перемешивания
    /// или контекста перечитывает очередь.
    fn watch_local(&self, link: &Link) {
        let session_id = link.session.session_id();
        {
            let mut state = self.lock();
            if state.watching.as_deref() == Some(session_id.as_str()) {
                return;
            }
            state.watching = Some(session_id.clone());
        }
        let mut local = link.local.clone();
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut last = None;
            while local.changed().await.is_ok() {
                let key = local.borrow_and_update().as_deref().map(local_queue_key);
                if key == last {
                    continue;
                }
                last = key;
                let Some(me) = me.upgrade() else { return };
                (me.changed)(false, true);
            }
            // движок остановлен
            if let Some(me) = me.upgrade() {
                let mut state = me.lock();
                if state.watching.as_deref() == Some(session_id.as_str()) {
                    state.watching = None;
                }
            }
        });
    }

    // ---- чтения -------------------------------------------------------------------------------------

    async fn playback(&self, link: &Link) -> Result<Option<RemoteState>, String> {
        let (cluster, now) = self.cluster(link).await?;
        let uri = &cluster.player_state.track.uri;
        let tracks = if cluster.active_device_id.is_empty() || uri.is_empty() {
            HashMap::new()
        } else {
            self.tracks(link, std::slice::from_ref(uri)).await
        };
        Ok(remote_state(&cluster, now, &tracks))
    }

    /// Играет этот ПК — очередь из Spirc (без сети); иначе — из кластера.
    async fn queue(&self, link: &Link) -> Result<FetchedQueue, String> {
        let local = link.local.borrow().clone().filter(|state| !state.track.uri.is_empty());
        if let Some(state) = local {
            return Ok(self.fetched(link, &state).await);
        }
        let (cluster, _) = self.cluster(link).await?;
        if cluster.active_device_id.is_empty() {
            return Ok(FetchedQueue::default());
        }
        Ok(self.fetched(link, &cluster.player_state).await)
    }

    /// Очередь и контекст из состояния плеера; данные треков и название контекста — через сессию (с кэшем).
    async fn fetched(&self, link: &Link, state: &PlayerState) -> FetchedQueue {
        let (current, next) = queue_from_state(state, QUEUE_LIMIT);
        let wanted: Vec<String> = current.iter().chain(next.iter()).cloned().collect();
        let (tracks, context) = futures::join!(self.tracks(link, &wanted), self.context(link, state));
        FetchedQueue {
            current: current.map(|uri| track_item(&uri, &tracks, state.duration)),
            next: next.iter().map(|uri| track_item(uri, &tracks, 0)).collect(),
            context,
            recommended: recommended(state),
        }
    }

    /// Контекст для карточки «Играет из …»: название и обложка из кэша или через сессию (название из
    /// метаданных плеера важнее).
    async fn context(&self, link: &Link, state: &PlayerState) -> Option<PlayContext> {
        let uri = context_uri(state)?;
        let mut context = map::context_from_uri(&uri);
        let cached = self.lock().contexts.get(&uri).cloned();
        let named = match cached {
            Some(named) => named,
            None => match crate::engine::session_reads::context_name(&link.session, &uri).await {
                Ok(named) => {
                    let named = named.map(|(title, images)| {
                        let cover = sf_api::models::pick_image(&images, sf_api::map::COVER_PX).unwrap_or_default();
                        (title, cover.to_owned())
                    });
                    let mut cache = self.lock();
                    if cache.contexts.len() >= CONTEXT_CACHE_LIMIT {
                        cache.contexts.clear();
                    }
                    cache.contexts.insert(uri.clone(), named.clone());
                    named
                }
                Err(error) => {
                    tracing::debug!("context name via the session unavailable: {error:?}");
                    None
                }
            },
        };
        if let Some((title, cover)) = named {
            context.title = title;
            context.cover = cover;
        }
        if let Some(title) = context_title(state) {
            context.title = title;
        }
        Some(context)
    }

    /// Данные треков по URI: из кэша, недостающее — одним пакетным запросом через сессию.
    async fn tracks(&self, link: &Link, uris: &[String]) -> HashMap<String, TrackItem> {
        let mut found = HashMap::new();
        let mut missing = Vec::new();
        {
            let state = self.lock();
            for uri in uris {
                match state.tracks.get(uri) {
                    Some(item) => {
                        found.insert(uri.clone(), item.clone());
                    }
                    None => {
                        if let Ok(parsed) = SpotifyUri::from_uri(uri) {
                            missing.push(parsed);
                        }
                    }
                }
            }
        }
        if missing.is_empty() {
            return found;
        }
        match crate::engine::session_reads::metadata(&link.session, missing.iter()).await {
            Ok(items) => {
                let mut state = self.lock();
                for (uri, item) in items {
                    let item = map::playable(&item);
                    if !state.tracks.contains_key(&uri) {
                        state.track_order.push_back(uri.clone());
                    }
                    state.tracks.insert(uri.clone(), item.clone());
                    found.insert(uri, item);
                }
                while state.track_order.len() > TRACK_CACHE_LIMIT {
                    if let Some(old) = state.track_order.pop_front() {
                        state.tracks.remove(&old);
                    }
                }
            }
            Err(error) => tracing::debug!("track data via the session unavailable: {error:?}"),
        }
        found
    }

    // ---- команды ------------------------------------------------------------------------------------

    async fn remote(&self, link: &Link, action: &RemoteAction, device: Option<&str>) -> Result<(), String> {
        let (cluster, _) = self.cluster(link).await?;
        let active = cluster.active_device_id.as_str();
        let target = device.filter(|device| !device.is_empty()).unwrap_or(active);
        if target.is_empty() {
            return Err("no active device".into());
        }
        match action {
            RemoteAction::Volume(percent) => send_volume(link, target, *percent).await,
            // «играть» на неактивном устройстве в Web API — перенос с воспроизведением
            RemoteAction::Play if target != active => {
                send_transfer(link, if active.is_empty() { target } else { active }, target, true).await
            }
            // только изменившийся флаг: librespot пересобирает «Далее» на каждом repeating_context
            RemoteAction::Repeat(mode) => {
                send_command(link, target, &repeat_body(*mode, repeat(&cluster.player_state))).await
            }
            action => send_command(link, target, &action_body(action)).await,
        }
    }

    async fn play(&self, link: &Link, device: Option<&str>, request: &PlayRequest, shuffle: bool) -> Result<(), String> {
        let target = match device.filter(|device| !device.is_empty()) {
            Some(device) => device.to_owned(),
            None => self.cluster(link).await?.0.active_device_id.clone(),
        };
        if target.is_empty() {
            return Err("no active device".into());
        }
        let body = play_body(request, shuffle).ok_or("nothing to start")?;
        send_command(link, &target, &body).await
    }

    /// Перенос; Ok — имя устройства из кластера (если известно).
    async fn transfer(&self, link: &Link, device: &str, play: bool) -> Result<Option<String>, String> {
        let (cluster, _) = self.cluster(link).await?;
        let name = cluster.device.get(device).map(|info| info.name.clone()).filter(|name| !name.is_empty());
        let from = if cluster.active_device_id.is_empty() { device } else { cluster.active_device_id.as_str() };
        send_transfer(link, from, device, play).await?;
        Ok(name)
    }

    /// По одному треку, по порядку; на отказе остаток уходит через Web API.
    async fn add_many(&self, link: &Link, request: u64, uris: &[String], device: Option<String>) -> ApiReply {
        let target = match device.as_deref().filter(|device| !device.is_empty()) {
            Some(device) => Ok(device.to_owned()),
            None => self.cluster(link).await.map(|(cluster, _)| cluster.active_device_id.clone()),
        };
        let mut added = 0;
        if let Ok(target) = target.as_ref()
            && !target.is_empty()
        {
            for uri in uris {
                if let Err(error) = send_command(link, target, &queue_body(uri)).await {
                    tracing::debug!("queueing via the session failed ({error}); the rest goes through the Web API");
                    break;
                }
                added += 1;
            }
        }
        if added == uris.len() {
            return ApiReply::QueueBatchAdded { request, added, result: Ok(()) };
        }
        let rest = uris[added..].to_vec();
        match self.web.call(ApiCall::AddMany { request, uris: rest, device }).await {
            ApiReply::QueueBatchAdded { request, added: more, result } => {
                ApiReply::QueueBatchAdded { request, added: added + more, result }
            }
            other => other,
        }
    }
}

/// Обернуть Web API: сессия — из движка хозяина; пуш кластера — внеочередной опрос (и очередь, если сменилась).
pub(crate) fn wrap(
    web: Arc<dyn WebBackend>,
    host: &Arc<EngineHost>,
    tx: &mpsc::UnboundedSender<Msg>,
) -> Arc<ConnectBackend> {
    let link: LinkSource = {
        let host = Arc::downgrade(host);
        Arc::new(move || {
            let engine = host.upgrade()?.engine()?;
            Some(Link {
                session: engine.engine.session().clone(),
                device_id: engine.engine.device_id().to_owned(),
                local: engine.engine.playback_state(),
                pushes: engine.engine.cluster_pushes(),
            })
        })
    };
    let changed: Changed = {
        let tx = tx.clone();
        Arc::new(move |remote, queue| {
            if remote {
                let _ = tx.send(Msg::Command(Command::RefreshRemote));
            }
            if queue {
                let _ = tx.send(Msg::Command(Command::RefreshQueue));
            }
        })
    };
    ConnectBackend::new(web, link, changed)
}

impl PlayerHandle {
    /// Устройства Spotify Connect из кластера сессии; None — движок не запущен или сессия не ответила.
    pub async fn connect_devices(&self) -> Option<Vec<Device>> {
        self.connect().devices().await
    }

    /// Громкость устройства через connect-state; None — движка нет (тогда — Web API).
    pub async fn connect_volume(&self, device: &str, percent: u8) -> Option<Result<(), String>> {
        self.connect().set_volume(device, percent).await
    }

    /// Счётчик пушей кластера: устройства или их состояние изменились.
    pub fn connect_changes(&self) -> watch::Receiver<u64> {
        self.connect().changes()
    }
}

impl WebBackend for ConnectBackend {
    fn call(&self, call: ApiCall) -> BoxFuture<ApiReply> {
        match ((self.link)(), self.me.upgrade()) {
            (Some(link), Some(me)) => Box::pin(me.run(link, call)),
            _ => self.web.call(call),
        }
    }

    fn fetch_cover(&self, url: String) -> BoxFuture<Result<std::path::PathBuf, String>> {
        self.web.fetch_cover(url)
    }
}

// ---- запросы spclient ---------------------------------------------------------------------------------

async fn send(link: &Link, method: Method, path: String, body: &Value) -> Result<(), String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let body = body.to_string();
    link.session
        .spclient()
        .request(&method, &path, Some(headers), Some(body.as_bytes()))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn send_command(link: &Link, to: &str, body: &Value) -> Result<(), String> {
    send(link, Method::POST, format!("/connect-state/v1/player/command/from/{}/to/{to}", link.device_id), body).await
}

async fn send_volume(link: &Link, to: &str, percent: u8) -> Result<(), String> {
    let path = format!("/connect-state/v1/connect/volume/from/{}/to/{to}", link.device_id);
    send(link, Method::PUT, path, &json!({ "volume": volume_from_percent(percent) })).await
}

async fn send_transfer(link: &Link, from: &str, to: &str, play: bool) -> Result<(), String> {
    send(link, Method::POST, format!("/connect-state/v1/connect/transfer/from/{from}/to/{to}"), &transfer_body(play)).await
}

// ---- чистые преобразования (тестируются без сети) --------------------------------------------------------

/// Серверное «сейчас» по метке кластера и времени его получения; без метки — часы этого ПК.
fn server_now(cluster: &Cluster, received: Instant) -> i64 {
    if cluster.server_timestamp_ms > 0 {
        cluster.server_timestamp_ms + received.elapsed().as_millis() as i64
    } else {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |now| now.as_millis() as i64)
    }
}

pub(crate) fn is_playing(state: &PlayerState) -> bool {
    state.is_playing && !state.is_paused
}

/// Позиция сейчас: на паузе — как записано, при игре — плюс прошедшее с отметки (не дальше длительности).
pub(crate) fn position_ms(state: &PlayerState, server_now_ms: i64) -> u32 {
    let mut position = state.position_as_of_timestamp.max(0);
    if is_playing(state) && state.timestamp > 0 {
        let speed = if state.playback_speed > 0.0 { state.playback_speed } else { 1.0 };
        position += ((server_now_ms - state.timestamp).max(0) as f64 * speed) as i64;
    }
    if state.duration > 0 {
        position = position.min(state.duration);
    }
    position.clamp(0, i64::from(u32::MAX)) as u32
}

pub(crate) fn repeat(state: &PlayerState) -> RepeatMode {
    let options = &state.options;
    if options.repeating_track {
        RepeatMode::Track
    } else if options.repeating_context {
        RepeatMode::Context
    } else {
        RepeatMode::Off
    }
}

/// Названия типов, как их пишет Web API (список устройств показывает иконку по ним).
pub(crate) fn kind_name(kind: DeviceType) -> &'static str {
    match kind {
        DeviceType::COMPUTER => "Computer",
        DeviceType::TABLET => "Tablet",
        DeviceType::SMARTPHONE => "Smartphone",
        DeviceType::SPEAKER => "Speaker",
        DeviceType::TV => "TV",
        DeviceType::AVR => "AVR",
        DeviceType::STB => "STB",
        DeviceType::AUDIO_DONGLE => "AudioDongle",
        DeviceType::GAME_CONSOLE => "GameConsole",
        DeviceType::CAST_VIDEO => "CastVideo",
        DeviceType::CAST_AUDIO => "CastAudio",
        DeviceType::AUTOMOBILE => "Automobile",
        DeviceType::SMARTWATCH => "Smartwatch",
        DeviceType::CHROMEBOOK => "Chromebook",
        DeviceType::CAR_THING => "CarThing",
        DeviceType::HOME_THING => "HomeThing",
        DeviceType::UNKNOWN | DeviceType::UNKNOWN_SPOTIFY | DeviceType::OBSERVER => "Unknown",
    }
}

pub(crate) fn volume_percent(volume: u32) -> u8 {
    ((u64::from(volume.min(65535)) * 100 + 32767) / 65535) as u8
}

pub(crate) fn volume_from_percent(percent: u8) -> u16 {
    ((u32::from(percent.min(100)) * 65535 + 50) / 100) as u16
}

pub(crate) fn device(id: &str, info: &DeviceInfo, active: &str) -> Device {
    let capabilities = &info.capabilities;
    let volume = !capabilities.disable_volume;
    Device {
        id: Some(id.to_owned()),
        name: info.name.clone(),
        is_active: !active.is_empty() && id == active,
        is_restricted: false,
        volume_percent: volume.then(|| volume_percent(info.volume)),
        supports_volume: Some(volume),
        kind: kind_name(info.device_type.enum_value_or_default()).to_owned(),
    }
}

/// Видимые устройства кластера (скрытые — нет), по имени.
pub(crate) fn devices(cluster: &Cluster) -> Vec<Device> {
    let mut list: Vec<Device> = cluster
        .device
        .iter()
        .filter(|(_, info)| !info.capabilities.hidden)
        .map(|(id, info)| device(id, info, &cluster.active_device_id))
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    list
}

/// Строка очереди, которую показывают (не разделитель, не скрытая, не служебная, не недоступная).
fn visible(track: &ProvidedTrack) -> bool {
    !track.uri.is_empty()
        && track.uri != "spotify:delimiter"
        && !track.uri.starts_with("spotify:meta:")
        && track.provider != "unavailable"
        && track.metadata.get("hidden").map(String::as_str) != Some("true")
}

/// Поставлена вручную («В очередь»), а не из контекста или автоплея.
fn queued(track: &ProvidedTrack) -> bool {
    track.provider == "queue" || track.metadata.get("is_queued").map(String::as_str) == Some("true")
}

/// URI текущего трека и первых `limit` следующих: ручные строки первыми, затем контекст и автоплей.
pub(crate) fn queue_from_state(state: &PlayerState, limit: usize) -> (Option<String>, Vec<String>) {
    let current = Some(state.track.uri.clone()).filter(|uri| !uri.is_empty());
    let (manual, rest): (Vec<&ProvidedTrack>, Vec<&ProvidedTrack>) =
        state.next_tracks.iter().filter(|track| visible(track)).partition(|track| queued(track));
    let next = manual.into_iter().chain(rest).take(limit).map(|track| track.uri.clone()).collect();
    (current, next)
}

/// Треки, которые Smart Shuffle добавил в контекст от себя.
fn recommended(state: &PlayerState) -> Vec<String> {
    std::iter::once(&*state.track)
        .chain(state.next_tracks.iter())
        .filter(|track| track.metadata.get("provider").map(String::as_str) == Some("enhanced_recommendation"))
        .map(|track| track.uri.clone())
        .collect()
}

/// URI текущего трека и первых `limit` следующих у активного устройства кластера.
pub(crate) fn queue_uris(cluster: &Cluster, limit: usize) -> (Option<String>, Vec<String>) {
    if cluster.active_device_id.is_empty() {
        return (None, Vec::new());
    }
    queue_from_state(&cluster.player_state, limit)
}

/// Что сравнивать, чтобы понять, сменилась ли очередь своего устройства.
fn local_queue_key(state: &PlayerState) -> (String, Vec<String>, String) {
    let (current, next) = queue_from_state(state, QUEUE_LIMIT);
    (current.unwrap_or_default(), next, state.context_uri.clone())
}

/// Название контекста, если плеер его знает (метаданные контекста от Spotify).
fn context_title(state: &PlayerState) -> Option<String> {
    ["context_description", "title"]
        .iter()
        .filter_map(|key| state.context_metadata.get(*key))
        .map(|title| title.trim())
        .find(|title| !title.is_empty())
        .map(str::to_owned)
}

/// Что сравнивать, чтобы понять, сменилась ли очередь.
fn queue_key(cluster: &Cluster) -> (String, String, Vec<String>) {
    let (current, next) = queue_uris(cluster, QUEUE_LIMIT);
    (cluster.active_device_id.clone(), current.unwrap_or_default(), next)
}

/// Трек из кэша данных или минимальный (URI, id, длительность), пока данных нет.
fn track_item(uri: &str, tracks: &HashMap<String, TrackItem>, duration_ms: i64) -> TrackItem {
    let mut item = tracks.get(uri).cloned().unwrap_or_else(|| TrackItem {
        track_id: uri_id(uri).unwrap_or_default().to_owned(),
        uri: uri.to_owned(),
        is_episode: uri_kind(uri) == Some("episode"),
        available: true,
        ..TrackItem::default()
    });
    if item.duration_ms <= 0 && duration_ms > 0 {
        item.duration_ms = duration_ms;
    }
    item
}

/// Контекст, который показал бы Web API (у списка без контекста — нет).
fn context_uri(state: &PlayerState) -> Option<String> {
    // у Smart Shuffle в URI контекста остаётся параметр линзы
    let uri = state.context_uri.split('?').next().unwrap_or_default();
    (uri.starts_with("spotify:") && uri != "spotify:web-api" && !uri.starts_with("spotify:internal:"))
        .then(|| sf_api::link::canonical_context_uri(uri))
}

/// Состояние в форме /me/player; None — ничего не активно (как 204).
pub(crate) fn remote_state(cluster: &Cluster, server_now_ms: i64, tracks: &HashMap<String, TrackItem>) -> Option<RemoteState> {
    let active = cluster.active_device_id.as_str();
    if active.is_empty() {
        return None;
    }
    let state = &cluster.player_state;
    let device = match cluster.device.get(active) {
        Some(info) => device(active, info, active),
        None => Device { id: Some(active.to_owned()), is_active: true, ..Device::default() },
    };
    let item = Some(state.track.uri.as_str()).filter(|uri| !uri.is_empty()).map(|uri| track_item(uri, tracks, state.duration));
    Some(RemoteState {
        device: Some(device),
        repeat: repeat(state),
        shuffle: state.options.shuffling_context,
        context_uri: context_uri(state),
        progress_ms: Some(position_ms(state, server_now_ms)),
        is_playing: is_playing(state),
        item,
    })
}

fn command(endpoint: &str, fields: Value) -> Value {
    let mut command = json!({ "endpoint": endpoint, "logging_params": {} });
    if let (Some(command), Value::Object(fields)) = (command.as_object_mut(), fields) {
        command.extend(fields);
    }
    json!({ "command": command })
}

/// Тело команды плеера (громкость — отдельный путь, см. send_volume).
pub(crate) fn action_body(action: &RemoteAction) -> Value {
    match action {
        RemoteAction::Play => command("resume", json!({})),
        RemoteAction::Pause => command("pause", json!({})),
        RemoteAction::Next => command("skip_next", json!({})),
        // как Web API: всегда предыдущий трек, а не начало текущего
        RemoteAction::Previous => command("skip_prev", json!({ "options": { "allow_seeking": false } })),
        RemoteAction::Seek(position) => command("seek_to", json!({ "value": position, "position": position })),
        RemoteAction::Shuffle(on) => command("set_shuffling_context", json!({ "value": on })),
        RemoteAction::Repeat(mode) => command(
            "set_options",
            json!({
                "repeating_context": *mode == RepeatMode::Context,
                "repeating_track": *mode == RepeatMode::Track,
            }),
        ),
        RemoteAction::Volume(percent) => json!({ "volume": volume_from_percent(*percent) }),
    }
}

/// set_options только с флагами, которые меняются (ничего не меняется — оба).
pub(crate) fn repeat_body(mode: RepeatMode, current: RepeatMode) -> Value {
    let context = mode == RepeatMode::Context;
    let track = mode == RepeatMode::Track;
    let mut fields = serde_json::Map::new();
    if context != (current == RepeatMode::Context) {
        fields.insert("repeating_context".into(), json!(context));
    }
    if track != (current == RepeatMode::Track) {
        fields.insert("repeating_track".into(), json!(track));
    }
    if fields.is_empty() {
        return action_body(&RemoteAction::Repeat(mode));
    }
    command("set_options", Value::Object(fields))
}

pub(crate) fn queue_body(uri: &str) -> Value {
    command("add_to_queue", json!({ "track": { "uri": uri, "metadata": { "is_queued": "true" }, "provider": "queue" } }))
}

/// «resume» — играть после переноса, «restore» — как было на прежнем устройстве.
pub(crate) fn transfer_body(play: bool) -> Value {
    json!({ "transfer_options": { "restore_paused": if play { "resume" } else { "restore" } } })
}

/// Команда play для запроса Web API: контекст по URI или список треков страницей; None — пустой запрос.
pub(crate) fn play_body(request: &PlayRequest, shuffle: bool) -> Option<Value> {
    // одиночный трек/выпуск играет как список из одного (как в PlayRequest::body)
    let single = request
        .context_uri
        .as_deref()
        .filter(|uri| matches!(uri_kind(uri), Some("track" | "episode")))
        .map(|context| request.offset_uri.as_deref().unwrap_or(context).to_owned());
    let mut skip_to = serde_json::Map::new();
    let context = if let Some(item) = single {
        json!({ "uri": "", "url": "", "pages": [{ "tracks": [{ "uri": item }] }] })
    } else if let Some(uri) = &request.context_uri {
        if let Some(offset) = &request.offset_uri {
            skip_to.insert("track_uri".into(), json!(offset));
        } else if let Some(index) = request.offset_position {
            skip_to.insert("track_index".into(), json!(index));
        }
        json!({ "uri": uri, "url": format!("context://{uri}") })
    } else if !request.uris.is_empty() {
        let index = match (&request.offset_uri, request.offset_position) {
            (Some(offset), _) => request.uris.iter().position(|uri| uri == offset),
            (None, Some(index)) => Some(index as usize).filter(|index| *index < request.uris.len()),
            (None, None) => None,
        };
        if let Some(index) = index {
            skip_to.insert("track_index".into(), json!(index));
            skip_to.insert("track_uri".into(), json!(request.uris[index]));
        }
        let tracks: Vec<Value> = request.uris.iter().map(|uri| json!({ "uri": uri })).collect();
        json!({ "uri": "", "url": "", "pages": [{ "tracks": tracks }] })
    } else {
        return None;
    };
    let mut options = serde_json::Map::new();
    if !skip_to.is_empty() {
        options.insert("skip_to".into(), Value::Object(skip_to));
    }
    if request.position_ms > 0 {
        options.insert("seek_to".into(), json!(request.position_ms));
    }
    if shuffle {
        options.insert("player_options_override".into(), json!({ "shuffling_context": true }));
    }
    Some(command(
        "play",
        json!({
            "context": context,
            "play_origin": { "feature_identifier": "materialfy" },
            "options": Value::Object(options),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::connect::Capabilities;
    use librespot_protocol::player::ContextPlayerOptions;
    use protobuf::{EnumOrUnknown, MessageField};

    fn provided(uri: &str) -> ProvidedTrack {
        ProvidedTrack { uri: uri.into(), ..Default::default() }
    }

    fn info(name: &str, kind: DeviceType, volume: u32) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            device_type: EnumOrUnknown::new(kind),
            volume,
            capabilities: MessageField::some(Capabilities { volume_steps: 64, ..Default::default() }),
            ..Default::default()
        }
    }

    fn cluster() -> Cluster {
        let mut hidden = provided("spotify:track:hidden");
        hidden.metadata.insert("hidden".into(), "true".into());
        let state = PlayerState {
            timestamp: 1_000_000,
            context_uri: "spotify:user:alice:playlist:37i9dQZF1DXbIbVYph0Zr5".into(),
            track: MessageField::some(provided("spotify:track:now")),
            position_as_of_timestamp: 10_000,
            duration: 200_000,
            is_playing: true,
            is_paused: false,
            playback_speed: 1.0,
            options: MessageField::some(ContextPlayerOptions { shuffling_context: true, ..Default::default() }),
            next_tracks: vec![
                provided("spotify:track:q1"),
                provided("spotify:delimiter"),
                hidden,
                provided("spotify:episode:e1"),
                provided("spotify:meta:page:3"),
            ],
            ..Default::default()
        };
        let mut cluster = Cluster {
            active_device_id: "phone".into(),
            server_timestamp_ms: 1_005_000,
            player_state: MessageField::some(state),
            ..Default::default()
        };
        cluster.device.insert("phone".into(), info("Pixel", DeviceType::SMARTPHONE, 32768));
        cluster.device.insert("pc".into(), info("Materialfy · PC", DeviceType::COMPUTER, 65535));
        let mut ghost = info("Hidden", DeviceType::SPEAKER, 0);
        ghost.capabilities.mut_or_insert_default().hidden = true;
        cluster.device.insert("ghost".into(), ghost);
        cluster
    }

    #[test]
    fn the_remote_state_comes_from_the_active_player() {
        let cluster = cluster();
        let mut tracks = HashMap::new();
        tracks.insert(
            "spotify:track:now".to_owned(),
            TrackItem { uri: "spotify:track:now".into(), title: "Now".into(), ..Default::default() },
        );
        let state = remote_state(&cluster, 1_005_000, &tracks).unwrap();
        assert_eq!(state.device_id(), Some("phone"));
        let device = state.device.as_ref().unwrap();
        assert_eq!((device.name.as_str(), device.kind.as_str(), device.is_active), ("Pixel", "Smartphone", true));
        assert_eq!(device.volume_percent, Some(50));
        assert!(state.is_playing);
        assert!(state.shuffle);
        assert_eq!(state.repeat, RepeatMode::Off);
        // 10 с на отметке + 5 с с её момента
        assert_eq!(state.progress_ms, Some(15_000));
        assert_eq!(state.context_uri.as_deref(), Some("spotify:playlist:37i9dQZF1DXbIbVYph0Zr5"));
        let item = state.item.unwrap();
        assert_eq!(item.title, "Now");
        // длительность — из кластера, если данные трека её не знают
        assert_eq!(item.duration_ms, 200_000);
    }

    #[test]
    fn nothing_active_reads_as_no_playback() {
        let mut cluster = cluster();
        cluster.active_device_id.clear();
        assert_eq!(remote_state(&cluster, 0, &HashMap::new()), None);
        assert_eq!(queue_uris(&cluster, 10), (None, Vec::new()));
    }

    #[test]
    fn a_paused_player_keeps_its_position_and_a_playing_one_stops_at_the_end() {
        let mut state = cluster().player_state.unwrap();
        state.is_paused = true;
        assert!(!is_playing(&state));
        assert_eq!(position_ms(&state, 9_999_999), 10_000);
        state.is_paused = false;
        assert_eq!(position_ms(&state, 9_999_999), 200_000);
        // часы позади отметки не уводят позицию назад
        assert_eq!(position_ms(&state, 0), 10_000);
    }

    #[test]
    fn repeat_reads_track_before_context() {
        let mut state = PlayerState::default();
        assert_eq!(repeat(&state), RepeatMode::Off);
        state.options.mut_or_insert_default().repeating_context = true;
        assert_eq!(repeat(&state), RepeatMode::Context);
        state.options.mut_or_insert_default().repeating_track = true;
        assert_eq!(repeat(&state), RepeatMode::Track);
    }

    #[test]
    fn the_queue_skips_delimiters_hidden_and_page_markers() {
        let cluster = cluster();
        let (current, next) = queue_uris(&cluster, 10);
        assert_eq!(current.as_deref(), Some("spotify:track:now"));
        assert_eq!(next, vec!["spotify:track:q1".to_owned(), "spotify:episode:e1".to_owned()]);
        assert_eq!(queue_uris(&cluster, 1).1, vec!["spotify:track:q1".to_owned()]);
        let item = track_item("spotify:episode:e1", &HashMap::new(), 0);
        assert!(item.is_episode);
        assert_eq!(item.track_id, "e1");
    }

    fn from(uri: &str, provider: &str) -> ProvidedTrack {
        ProvidedTrack { uri: uri.into(), provider: provider.into(), ..Default::default() }
    }

    /// Состояние Spirc этого ПК: альбом, две ручные строки (одна — после контекстной), маркеры и автоплей.
    fn local_state() -> PlayerState {
        let mut hidden = from("spotify:track:hidden", "context");
        hidden.metadata.insert("hidden".into(), "true".into());
        let mut flagged = from("spotify:track:q2", "context");
        flagged.metadata.insert("is_queued".into(), "true".into());
        PlayerState {
            context_uri: "spotify:album:2noRn2Aes5aoNVsU6iWThc".into(),
            track: MessageField::some(from("spotify:track:now", "context")),
            duration: 320_000,
            next_tracks: vec![
                from("spotify:track:q1", "queue"),
                from("spotify:track:c1", "context"),
                flagged,
                hidden,
                from("spotify:track:gone", "unavailable"),
                from("spotify:meta:page:1", "context"),
                from("spotify:track:c2", "context"),
                from("spotify:delimiter", "context"),
                from("spotify:track:auto", "autoplay"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn the_local_queue_puts_queued_tracks_first_and_drops_markers() {
        let state = local_state();
        let (current, next) = queue_from_state(&state, 10);
        assert_eq!(current.as_deref(), Some("spotify:track:now"));
        assert_eq!(
            next,
            ["spotify:track:q1", "spotify:track:q2", "spotify:track:c1", "spotify:track:c2", "spotify:track:auto"]
        );
        assert_eq!(queue_from_state(&state, 3).1, ["spotify:track:q1", "spotify:track:q2", "spotify:track:c1"]);
        assert_eq!(queue_from_state(&PlayerState::default(), 10), (None, Vec::new()));
    }

    #[test]
    fn the_local_queue_key_follows_track_queue_shuffle_and_context_but_not_position() {
        let before = local_state();
        let mut after = before.clone();
        after.position_as_of_timestamp = 61_000;
        after.timestamp = 99;
        assert_eq!(local_queue_key(&before), local_queue_key(&after));
        // перемешивание переставляет контекстные строки
        after.next_tracks.swap(1, 6);
        assert_ne!(local_queue_key(&before), local_queue_key(&after));
        let mut added = before.clone();
        added.next_tracks.insert(0, from("spotify:track:new", "queue"));
        assert_ne!(local_queue_key(&before), local_queue_key(&added));
        let mut next = before.clone();
        next.track = MessageField::some(from("spotify:track:q1", "queue"));
        next.next_tracks.remove(0);
        assert_ne!(local_queue_key(&before), local_queue_key(&next));
        let mut other = before.clone();
        other.context_uri = "spotify:playlist:p".into();
        assert_ne!(local_queue_key(&before), local_queue_key(&other));
    }

    #[test]
    fn the_context_title_comes_from_the_players_metadata_when_known() {
        let mut state = local_state();
        assert_eq!(context_title(&state), None);
        assert_eq!(context_uri(&state).as_deref(), Some("spotify:album:2noRn2Aes5aoNVsU6iWThc"));
        state.context_metadata.insert("context_description".into(), "  ".into());
        state.context_metadata.insert("title".into(), "Discovery".into());
        assert_eq!(context_title(&state).as_deref(), Some("Discovery"));
        state.context_metadata.insert("context_description".into(), "Discovery (Deluxe)".into());
        assert_eq!(context_title(&state).as_deref(), Some("Discovery (Deluxe)"));
        // список без контекста Web API не называет
        state.context_uri = "spotify:web-api".into();
        assert_eq!(context_uri(&state), None);
    }

    #[test]
    fn smart_shuffle_keeps_the_playlist_uri_and_marks_its_recommendations() {
        let mut state = local_state();
        state.context_uri = "spotify:playlist:p?spotify-apply-lenses=enhance".into();
        assert_eq!(context_uri(&state).as_deref(), Some("spotify:playlist:p"));
        let mut rec = provided("spotify:track:rec");
        rec.metadata.insert("provider".into(), "enhanced_recommendation".into());
        state.next_tracks = vec![provided("spotify:track:own"), rec];
        assert_eq!(recommended(&state), ["spotify:track:rec"]);
    }

    #[test]
    fn a_new_queue_or_track_changes_the_queue_key() {
        let before = cluster();
        let mut after = before.clone();
        after.player_state.mut_or_insert_default().position_as_of_timestamp = 99;
        assert_eq!(queue_key(&before), queue_key(&after));
        after.player_state.mut_or_insert_default().next_tracks.insert(0, provided("spotify:track:new"));
        assert_ne!(queue_key(&before), queue_key(&after));
    }

    #[test]
    fn devices_leave_out_hidden_ones_and_carry_volume() {
        let list = devices(&cluster());
        let names: Vec<&str> = list.iter().map(|device| device.name.as_str()).collect();
        assert_eq!(names, ["Materialfy · PC", "Pixel"]);
        assert_eq!(list[0].volume_percent, Some(100));
        assert!(!list[0].is_active && list[1].is_active);
        let mut silent = info("Fixed", DeviceType::AVR, 100);
        silent.capabilities.mut_or_insert_default().disable_volume = true;
        let fixed = device("avr", &silent, "");
        assert_eq!((fixed.volume_percent, fixed.supports_volume), (None, Some(false)));
        assert_eq!(fixed.kind, "AVR");
    }

    #[test]
    fn volume_converts_both_ways() {
        assert_eq!(volume_from_percent(0), 0);
        assert_eq!(volume_from_percent(100), 65535);
        assert_eq!(volume_from_percent(250), 65535);
        for percent in 0..=100 {
            assert_eq!(volume_percent(u32::from(volume_from_percent(percent))), percent);
        }
    }

    #[test]
    fn commands_carry_their_endpoint_and_logging_params() {
        let body = action_body(&RemoteAction::Seek(61_000));
        assert_eq!(body, json!({ "command": { "endpoint": "seek_to", "value": 61000, "position": 61000, "logging_params": {} } }));
        assert_eq!(action_body(&RemoteAction::Play)["command"]["endpoint"], "resume");
        assert_eq!(action_body(&RemoteAction::Next)["command"]["endpoint"], "skip_next");
        assert_eq!(action_body(&RemoteAction::Previous)["command"]["options"]["allow_seeking"], false);
        assert_eq!(action_body(&RemoteAction::Shuffle(true))["command"]["value"], true);
        let repeat = action_body(&RemoteAction::Repeat(RepeatMode::Track));
        assert_eq!(repeat["command"]["repeating_track"], true);
        assert_eq!(repeat["command"]["repeating_context"], false);
        let only_track = repeat_body(RepeatMode::Track, RepeatMode::Off)["command"].clone();
        assert_eq!(only_track, json!({ "endpoint": "set_options", "repeating_track": true, "logging_params": {} }));
        let both = repeat_body(RepeatMode::Track, RepeatMode::Context)["command"].clone();
        assert_eq!((both["repeating_context"].clone(), both["repeating_track"].clone()), (json!(false), json!(true)));
        assert_eq!(repeat_body(RepeatMode::Off, RepeatMode::Off), action_body(&RemoteAction::Repeat(RepeatMode::Off)));
        let queued = queue_body("spotify:track:x");
        assert_eq!(queued["command"]["track"]["provider"], "queue");
        assert_eq!(queued["command"]["logging_params"], json!({}));
        assert_eq!(transfer_body(true)["transfer_options"]["restore_paused"], "resume");
        assert_eq!(transfer_body(false)["transfer_options"]["restore_paused"], "restore");
    }

    #[test]
    fn play_loads_a_context_at_a_track_with_shuffle() {
        let request = PlayRequest {
            position_ms: 5_000,
            ..PlayRequest::context("spotify:album:a").starting_at_uri("spotify:track:t")
        };
        let body = play_body(&request, true).unwrap();
        let command = &body["command"];
        assert_eq!(command["endpoint"], "play");
        assert_eq!(command["context"]["uri"], "spotify:album:a");
        assert_eq!(command["context"]["url"], "context://spotify:album:a");
        assert_eq!(command["options"]["skip_to"]["track_uri"], "spotify:track:t");
        assert_eq!(command["options"]["seek_to"], 5000);
        assert_eq!(command["options"]["player_options_override"]["shuffling_context"], true);
        let indexed = play_body(&PlayRequest::context("spotify:playlist:p").starting_at_index(3), false).unwrap();
        assert_eq!(indexed["command"]["options"], json!({ "skip_to": { "track_index": 3 } }));
    }

    #[test]
    fn play_sends_a_list_or_a_single_item_as_a_page() {
        let uris = vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()];
        let body = play_body(&PlayRequest::tracks(uris.clone()).starting_at_uri("spotify:track:b"), false).unwrap();
        let command = &body["command"];
        assert_eq!(command["context"]["pages"][0]["tracks"][1]["uri"], "spotify:track:b");
        assert_eq!(command["options"]["skip_to"]["track_index"], 1);
        let single = play_body(&PlayRequest::context("spotify:track:solo"), false).unwrap();
        assert_eq!(single["command"]["context"]["pages"][0]["tracks"], json!([{ "uri": "spotify:track:solo" }]));
        assert_eq!(single["command"]["options"], json!({}));
        assert_eq!(play_body(&PlayRequest::default(), false), None);
    }
}
