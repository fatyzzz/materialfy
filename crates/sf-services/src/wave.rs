//! Радио Spotify (исполнитель «services-extra»; бывшая «Моя волна»). Станции только через сессию librespot:
//! session_reads::station("spotify:station:{track|artist|album|playlist}:ID") → ~50 треков, каждый запуск — новый
//! микс; играются ровно показанные треки списком URI с контекстом-станцией, дальше — autoplay librespot.
//! «Радио по вашей музыке» (play()) = станция от последнего/любимого трека. Группы-основы вместо групп Яндекса,
//! закрепления — первыми в группах. Локальный стоп-лист (Library.dislike) фильтрует треки.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use md3_model::{PlayContext, RadioSeed, RadioSeedGroup, TrackItem, contexts};
use sf_api::util::{seed_uri, station_seed, station_uri, uri_id, uri_kind};
use sf_api::{Operation, PlaylistAccess};
use sf_player::{PlayerEvent, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use mf_platform::{SettingKey, SettingValue, SettingsStore};

use crate::auth::{AuthEvent, AuthService};
use crate::data::{home_feed, route};
use crate::extra::seeds::{self, ANY_MAX, SeedSources, station_title};
use crate::history::HistoryService;
use crate::library::{FeedbackKind, LibraryEvent, LibraryService};
use crate::lock;

/// Ожидание станции от Spotify (reference backend.rs RADIO_TIMEOUT).
pub const RADIO_TIMEOUT: Duration = Duration::from_secs(20);
/// Сколько истории смотреть для сидов.
const HISTORY_FOR_SEEDS: usize = 50;
const LIKED_FOR_SEEDS: usize = 30;
/// Подпись сида (сессия или запасной Web API).
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(8);
/// Пачка событий подряд → одна пересборка сидов.
const SEEDS_DEBOUNCE: Duration = Duration::from_millis(150);

pub const LOCKED_TEXT: &str = "Turn on playback on this computer — radio needs a Spotify session";
const FAILED_TEXT: &str = "Couldn’t load the radio. Try again.";
const TIMEOUT_TEXT: &str = "Spotify is taking too long to respond. Try again.";
const NO_RADIO_TEXT: &str = "There’s no radio for this.";
const EMPTY_TEXT: &str = "Spotify returned no tracks for this station.";

/// Снимок для моста Radio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveSnapshot {
    /// играет станция этого сервиса (Player.source == "radio")
    pub active: bool,
    /// резолвится станция
    pub loading: bool,
    /// «Радио «…»»
    pub station_title: String,
    /// spotify:station:… текущей/открытой станции
    pub station_uri: String,
    /// сид выбранный на орбите (spotify:track|artist|album|playlist:ID)
    pub selected: String,
    /// треки открытой станции (список «Список треков»)
    pub tracks: Arc<Vec<TrackItem>>,
    /// группы сидов орбиты: recent | artists | collections | any
    pub seeds: Arc<Vec<RadioSeedGroup>>,
    /// нет локального воспроизведения/Premium — сиды неактивны
    pub locked: bool,
    pub error_text: String,
}

/// Что сделать с треками станции, когда они придут.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    Show,
    Play,
    /// новый микс; играть его, если эта станция и играет
    Refresh,
}

pub struct WaveService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    library: Arc<LibraryService>,
    history: Arc<HistoryService>,
    snapshot: watch::Sender<WaveSnapshot>,
    /// поколение запроса станции: принимается только последний ответ
    generation: AtomicU64,
    /// группа «Любой трек»: выбранные через поиск, новые первыми (SettingKey::RadioSeeds)
    any: Mutex<Vec<RadioSeed>>,
    settings: Arc<SettingsStore>,
    /// себя — для задач, запущенных из &self
    me: Weak<WaveService>,
}

impl WaveService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        settings: Arc<SettingsStore>,
    ) -> Arc<Self> {
        let any = load_any(&settings);
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth,
            player,
            library,
            history,
            snapshot: watch::channel(WaveSnapshot { locked: true, ..Default::default() }).0,
            generation: AtomicU64::new(0),
            any: Mutex::new(any),
            settings,
            me: me.clone(),
        });
        rt.spawn(listen(Arc::downgrade(&service)));
        service
    }

    /// Большая кнопка: станция выбранного сида (по умолчанию — последний прослушанный трек) или пауза/продолжение.
    pub fn play(&self) {
        let snap = self.snapshot();
        let player = self.player.snapshot();
        let playing_station = player.context.context_uri.clone().unwrap_or_default();
        let selected = if snap.selected.is_empty() { seeds::default_seed(&snap.seeds) } else { Some(snap.selected.clone()) };
        let wanted = selected.as_deref().and_then(station_uri);
        if snap.active && (wanted.is_none() || wanted.as_deref() == Some(playing_station.as_str())) {
            self.player.toggle_play();
            return;
        }
        match selected {
            Some(seed) => self.play_station(seed, String::new()),
            None => self.snapshot.send_modify(|s| s.error_text = mf_i18n::t("Play something first — the radio will start from your last track")),
        }
    }

    /// Радио от сида: "artist:ID" | "track:ID" | "album:ID" | "playlist:ID" | полный spotify:…-URI.
    pub fn play_station(&self, seed: String, title: String) {
        let Some((seed, station)) = normalize(&seed) else {
            self.snapshot.send_modify(|s| s.error_text = mf_i18n::t(NO_RADIO_TEXT));
            return;
        };
        let snap = self.snapshot();
        // открытая станция уже показала треки — играются ровно они
        if snap.station_uri == station && !snap.tracks.is_empty() && !snap.loading {
            let title = if title.is_empty() { snap.station_title.clone() } else { title };
            self.remember_any(&seed);
            self.snapshot.send_modify(|s| {
                s.selected = seed.clone();
                s.station_title = title.clone();
                s.error_text.clear();
            });
            self.start_playing(&station, &title, &snap.tracks);
            return;
        }
        self.request(seed, station, title, Then::Play);
    }

    /// Выбрать сид на орбите (без запуска); "any"-сид добавляется в группу «Любой трек».
    pub fn select(&self, seed_uri: String) {
        let Some((seed, _)) = normalize(&seed_uri) else { return };
        self.remember_any(&seed);
        self.snapshot.send_modify(|s| {
            s.selected = seed.clone();
            s.error_text.clear();
        });
    }

    /// Открыть станцию (загрузить треки без воспроизведения) — страница радио.
    pub fn open(&self, seed_uri: String) {
        let Some((seed, station)) = normalize(&seed_uri) else {
            self.snapshot.send_modify(|s| s.error_text = mf_i18n::t(NO_RADIO_TEXT));
            return;
        };
        let snap = self.snapshot();
        if snap.station_uri == station && (snap.loading || !snap.tracks.is_empty()) {
            return;
        }
        self.request(seed, station, String::new(), Then::Show);
    }

    /// «Пересобрать»: новый микс, старые треки видны до ответа.
    pub fn refresh(&self) {
        let snap = self.snapshot();
        let seed = station_seed(&snap.station_uri)
            .or_else(|| (!snap.selected.is_empty()).then(|| snap.selected.clone()))
            .or_else(|| seeds::default_seed(&snap.seeds));
        let Some((seed, station)) = seed.as_deref().and_then(normalize) else { return };
        let title = if snap.station_uri == station { snap.station_title.clone() } else { String::new() };
        self.request(seed, station, title, Then::Refresh);
    }

    /// «Сохранить как плейлист»: имя «Радио «…»», треки — показанные.
    pub fn save(&self) {
        let snap = self.snapshot();
        let uris: Vec<String> = snap.tracks.iter().map(TrackItem::play_uri).collect();
        if uris.is_empty() {
            return;
        }
        let title = if snap.station_title.is_empty() { station_title("") } else { snap.station_title.clone() };
        self.library.create_playlist_from(title, uris);
    }

    pub fn snapshot(&self) -> WaveSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<WaveSnapshot> {
        self.snapshot.subscribe()
    }

    // --- внутреннее ----------------------------------------------------------------------------

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    /// Сид не из групп орбиты попадает в «Любой трек» (подпись — из известного или с Web API).
    fn remember_any(&self, seed: &str) {
        let snap = self.snapshot();
        if seeds::find(&snap.seeds, seed).is_some() {
            return;
        }
        let known = self.with_sources(|sources| seeds::known_seed(sources, seed));
        let item = known.clone().unwrap_or_else(|| RadioSeed {
            uri: seed.to_owned(),
            label: mf_i18n::t(kind_label(seed)),
            cover: String::new(),
            kind: uri_kind(seed).unwrap_or("track").to_owned(),
        });
        {
            let mut any = lock(&self.any);
            any.retain(|s| s.uri != seed);
            any.insert(0, item);
            any.truncate(ANY_MAX);
            save_any(&self.settings, &any);
        }
        self.rebuild_seeds();
        if known.is_none() {
            let auth = self.auth.clone();
            let player = self.player.clone();
            let seed = seed.to_owned();
            let me = self.weak();
            self.rt.spawn(async move {
                let Some(found) = lookup(&auth, &player, &seed).await else { return };
                let Some(me) = me.upgrade() else { return };
                let mut any = lock(&me.any);
                if let Some(slot) = any.iter_mut().find(|s| s.uri == seed) {
                    *slot = found;
                    save_any(&me.settings, &any);
                }
                drop(any);
                me.rebuild_seeds();
            });
        }
    }

    fn weak(&self) -> Weak<Self> {
        self.me.clone()
    }

    fn with_sources<R>(&self, f: impl FnOnce(&SeedSources<'_>) -> R) -> R {
        let history: Vec<TrackItem> = self.history.recent(HISTORY_FOR_SEEDS).into_iter().map(|r| r.track).collect();
        let liked: Vec<TrackItem> = self.library.liked().into_iter().take(LIKED_FOR_SEEDS).collect();
        let contexts = self.history.recent_contexts(20);
        let playlists = self.library.playlists();
        let albums = self.library.albums();
        let artists = self.library.artists();
        let pins = self.library.pins();
        let any = lock(&self.any).clone();
        let sources = SeedSources {
            recent_tracks: &history,
            liked: &liked,
            recent_contexts: &contexts,
            playlists: &playlists,
            albums: &albums,
            artists: &artists,
            pins: &pins,
            any: &any,
        };
        f(&sources)
    }

    fn rebuild_seeds(&self) {
        let library = self.library.clone();
        let groups = self.with_sources(|sources| seeds::build(sources, &|uri| library.is_hidden(uri)));
        self.snapshot.send_if_modified(|s| {
            let mut changed = false;
            if *s.seeds != groups {
                s.seeds = Arc::new(groups);
                changed = true;
            }
            let lost = s.selected.is_empty() || seeds::find(&s.seeds, &s.selected).is_none();
            if lost {
                let fallback = seeds::default_seed(&s.seeds).unwrap_or_default();
                if s.selected != fallback {
                    s.selected = fallback;
                    changed = true;
                }
            }
            changed
        });
    }

    /// active/locked/станция по снимку плеера.
    fn follow_player(&self) {
        let player = self.player.snapshot();
        let active = player.source() == contexts::RADIO;
        let locked = !self.auth.is_signed_in() || !player.local.is_ready();
        let context_uri = player.context.context_uri.clone().unwrap_or_default();
        self.snapshot.send_if_modified(|s| {
            let mut changed = false;
            if s.active != active {
                s.active = active;
                changed = true;
            }
            if s.locked != locked {
                s.locked = locked;
                changed = true;
            }
            // станцию, запущенную не отсюда (восстановленная сессия), показываем, пока своей нет
            if active && s.station_uri.is_empty() && !s.loading && context_uri.starts_with("spotify:station:") {
                s.station_uri = context_uri.clone();
                s.station_title = player.context.title.clone();
                if let Some(seed) = station_seed(&context_uri) {
                    s.selected = seed;
                }
                changed = true;
            }
            changed
        });
    }

    fn request(&self, seed: String, station: String, title: String, then: Then) {
        self.remember_any(&seed);
        let generation = self.next_generation();
        let label_title = if title.is_empty() {
            let snap = self.snapshot();
            seeds::find(&snap.seeds, &seed).filter(|s| !is_kind_placeholder(&s.label, &s.uri)).map(|s| station_title(&s.label))
        } else {
            Some(title)
        };
        self.snapshot.send_modify(|s| {
            let same = s.station_uri == station;
            s.selected = seed.clone();
            s.loading = true;
            s.error_text.clear();
            if !same {
                s.station_uri = station.clone();
                s.tracks = Arc::new(Vec::new());
            }
            if let Some(t) = &label_title {
                s.station_title = t.clone();
            } else if !same {
                s.station_title = station_title("");
            }
        });
        let Some(me) = self.weak().upgrade() else { return };
        self.rt.spawn(async move {
            let resolve = resolve(&me.player, &station);
            let name = async {
                match &label_title {
                    Some(t) => Some(t.clone()),
                    None => lookup(&me.auth, &me.player, &seed).await.map(|s| station_title(&s.label)),
                }
            };
            let (result, name) = tokio::join!(resolve, name);
            if !me.current(generation) {
                return;
            }
            match result {
                Ok(tracks) => {
                    let library = me.library.clone();
                    let tracks: Vec<TrackItem> = tracks.into_iter().filter(|t| playable(t, &|u| library.is_hidden(u))).collect();
                    if tracks.is_empty() {
                        me.snapshot.send_modify(|s| {
                            s.loading = false;
                            s.error_text = mf_i18n::t(EMPTY_TEXT);
                        });
                        return;
                    }
                    let title = name.unwrap_or_else(|| me.snapshot().station_title);
                    let tracks = Arc::new(tracks);
                    me.snapshot.send_modify(|s| {
                        s.loading = false;
                        s.station_uri = station.clone();
                        s.station_title = title.clone();
                        s.tracks = tracks.clone();
                    });
                    let playing_this = {
                        let p = me.player.snapshot();
                        p.source() == contexts::RADIO && p.context.context_uri.as_deref() == Some(station.as_str())
                    };
                    if then == Then::Play || (then == Then::Refresh && playing_this) {
                        me.start_playing(&station, &title, &tracks);
                    }
                }
                Err(text) => me.snapshot.send_modify(|s| {
                    s.loading = false;
                    // неудачное обновление оставляет прежний микс на экране
                    s.error_text = text;
                }),
            }
        });
    }

    fn start_playing(&self, station: &str, title: &str, tracks: &[TrackItem]) {
        let uris: Vec<String> = tracks.iter().map(TrackItem::play_uri).collect();
        let cover = tracks.first().map(|t| t.cover.clone()).unwrap_or_default();
        let context = PlayContext::with_uri(contexts::RADIO, station).titled(title).with_cover(cover);
        // первая строка — сразу: экран переключается на неё, пока грузится звук
        if let Some(first) = tracks.first() {
            self.player.remember(first.clone());
        }
        self.player.play_uris(uris, 0, context);
    }

    fn drop_track(&self, uri: &str) {
        self.snapshot.send_if_modified(|s| {
            if !s.tracks.iter().any(|t| t.play_uri() == uri) {
                return false;
            }
            s.tracks = Arc::new(s.tracks.iter().filter(|t| t.play_uri() != uri).cloned().collect());
            true
        });
    }

    fn reset(&self) {
        self.next_generation();
        lock(&self.any).clear();
        save_any(&self.settings, &[]);
        self.snapshot.send_replace(WaveSnapshot { locked: true, ..Default::default() });
    }
}

/// Подписки: история/библиотека/вход/плеер → сиды и состояние.
async fn listen(weak: Weak<WaveService>) {
    let Some(me) = weak.upgrade() else { return };
    let mut history = me.history.subscribe();
    let mut library = me.library.subscribe();
    let mut auth = me.auth.subscribe();
    let mut player = me.player.subscribe().events;
    me.rebuild_seeds();
    me.follow_player();
    drop(me);
    let mut rebuild_at: Option<tokio::time::Instant> = None;
    loop {
        let deadline = rebuild_at;
        let wait = async move {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        let soon = || Some(tokio::time::Instant::now() + SEEDS_DEBOUNCE);
        tokio::select! {
            event = history.recv() => match event {
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => rebuild_at = rebuild_at.or_else(soon),
                Err(broadcast::error::RecvError::Closed) => break,
            },
            event = library.recv() => match event {
                Ok(LibraryEvent::Feedback { kind: FeedbackKind::Dislike, track }) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.drop_track(&track.play_uri());
                    rebuild_at = rebuild_at.or_else(soon);
                }
                Ok(
                    LibraryEvent::Playlists(_)
                    | LibraryEvent::Liked(_)
                    | LibraryEvent::Albums(_)
                    | LibraryEvent::Artists(_)
                    | LibraryEvent::PinsChanged
                    | LibraryEvent::EntriesChanged,
                )
                | Err(broadcast::error::RecvError::Lagged(_)) => rebuild_at = rebuild_at.or_else(soon),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            event = auth.recv() => {
                let Some(me) = weak.upgrade() else { break };
                match event {
                    Ok(AuthEvent::SignedOut) => me.reset(),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
                me.follow_player();
                rebuild_at = rebuild_at.or_else(soon);
            },
            event = player.recv() => match event {
                Ok(PlayerEvent::TrackChanged | PlayerEvent::QueueChanged | PlayerEvent::LocalChanged
                    | PlayerEvent::DeviceChanged | PlayerEvent::Queue(_))
                | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.follow_player();
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            () = wait => {
                rebuild_at = None;
                let Some(me) = weak.upgrade() else { break };
                me.rebuild_seeds();
            }
        }
    }
}

/// Сид и станция: принимает "track:ID", spotify:-URI сида или саму станцию.
fn normalize(seed: &str) -> Option<(String, String)> {
    let seed = seed.trim();
    let seed = station_seed(seed).or_else(|| seed_uri(seed))?;
    let station = station_uri(&seed)?;
    Some((seed, station))
}

/// Подпись-заглушка сида (English source; в UI — mf_i18n::t).
fn kind_label(seed: &str) -> &'static str {
    match uri_kind(seed) {
        Some("artist") => "Artist",
        Some("album") => "Album",
        Some("playlist") => "Playlist",
        _ => "Track",
    }
}

/// Подпись — заглушка kind_label на любом языке (заглушка сохраняется в настройках «Любой трек», язык мог смениться).
fn is_kind_placeholder(label: &str, seed: &str) -> bool {
    let en = kind_label(seed);
    label == en || mf_i18n::translated_in_any(en, label)
}

fn playable(track: &TrackItem, hidden: &dyn Fn(&str) -> bool) -> bool {
    track.available && !track.is_episode && !track.track_id.is_empty() && !hidden(&track.play_uri())
}

/// Треки станции через сессию librespot (у Web API станций нет).
async fn resolve(player: &PlayerHandle, station: &str) -> Result<Vec<TrackItem>, String> {
    let Some(session) = player.session() else {
        return Err(mf_i18n::t(LOCKED_TEXT));
    };
    match tokio::time::timeout(RADIO_TIMEOUT, sf_player::engine::session_reads::station(&session, station)).await {
        Ok(Ok(tracks)) => Ok(tracks.iter().map(sf_api::map::track_item).collect()),
        Ok(Err(e)) => {
            tracing::warn!("radio {station}: {e:#}");
            Err(mf_i18n::t(FAILED_TEXT))
        }
        Err(_) => Err(mf_i18n::t(TIMEOUT_TEXT)),
    }
}

/// Подпись и обложка сида (для «любого трека» из поиска/меню): через сессию (подписи GraphQL, без квоты
/// Web API), без неё — Web API с таймаутом (подпись не должна держать станцию).
async fn lookup(auth: &AuthService, player: &PlayerHandle, seed: &str) -> Option<RadioSeed> {
    let api = auth.api();
    let id = uri_id(seed)?.to_owned();
    let kind = uri_kind(seed)?;
    if let Some(session) = route::account_session(&api, player) {
        let (http, uris) = (api.http(), [seed.to_owned()]);
        let read = home_feed::lookup(&http, &session, &uris);
        match tokio::time::timeout(LOOKUP_TIMEOUT, read).await {
            Ok(Ok(cards)) => {
                if let Some(card) = cards.into_iter().find(|c| c.uri == seed && !c.title.is_empty()) {
                    return Some(RadioSeed { uri: seed.to_owned(), label: card.title, cover: card.cover, kind: kind.to_owned() });
                }
            }
            Ok(Err(error)) => tracing::debug!("seed label via session: {error:#}"),
            Err(_) => tracing::debug!("seed label via session: timeout"),
        }
    }
    tokio::time::timeout(LOOKUP_TIMEOUT, web_lookup(&api, seed, &id)).await.ok().flatten()
}

async fn web_lookup(api: &sf_api::ApiGateway, seed: &str, id: &str) -> Option<RadioSeed> {
    let id = id.to_owned();
    let make = |label: String, cover: String, kind: &str| RadioSeed { uri: seed.to_owned(), label, cover, kind: kind.to_owned() };
    let found = match uri_kind(seed)? {
        "track" => {
            let client = api.client_for(Operation::Catalog).await.ok()?;
            let t = sf_api::map::track_item(&client.track(&id).await.ok()?);
            make(t.title, t.cover, "track")
        }
        "album" => {
            let client = api.client_for(Operation::Catalog).await.ok()?;
            let a = sf_api::map::album_item(&client.album(&id).await.ok()?);
            make(a.title, a.cover, "album")
        }
        "artist" => {
            let client = api.client_for(Operation::Catalog).await.ok()?;
            let a = sf_api::map::artist_item(&client.artist(&id).await.ok()?);
            make(a.name, a.cover, "artist")
        }
        "playlist" => {
            let access = api.playlist_access(&id);
            let access = if access == PlaylistAccess::default() { PlaylistAccess::Unknown } else { access };
            let client = api.client_for(Operation::PlaylistMetadata(access)).await.ok()?;
            let p = sf_api::map::playlist_item(&client.playlist(&id).await.ok()?, None);
            make(p.title, p.cover, "playlist")
        }
        _ => return None,
    };
    (!found.label.is_empty()).then_some(found)
}

/// Группа «Любой трек» из настроек (битая запись — пустая группа).
fn load_any(settings: &SettingsStore) -> Vec<RadioSeed> {
    let mut any: Vec<RadioSeed> = serde_json::from_str(&settings.get_str(SettingKey::RadioSeeds)).unwrap_or_default();
    any.retain(|seed| !seed.uri.is_empty());
    any.truncate(ANY_MAX);
    any
}

fn save_any(settings: &SettingsStore, any: &[RadioSeed]) {
    let text = if any.is_empty() { String::new() } else { serde_json::to_string(any).unwrap_or_default() };
    settings.set(SettingKey::RadioSeeds, SettingValue::Str(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_normalize_to_stations() {
        let pair = |a: &str, b: &str| Some((a.to_owned(), b.to_owned()));
        assert_eq!(normalize("artist:1"), pair("spotify:artist:1", "spotify:station:artist:1"));
        assert_eq!(normalize("spotify:track:2"), pair("spotify:track:2", "spotify:station:track:2"));
        assert_eq!(normalize("spotify:station:album:3"), pair("spotify:album:3", "spotify:station:album:3"));
        assert_eq!(normalize("spotify:show:4"), None);
        assert_eq!(normalize("episode:5"), None);
    }

    #[test]
    fn hidden_and_unplayable_tracks_leave_the_station() {
        let track = |id: &str, available: bool| TrackItem {
            track_id: id.into(),
            uri: format!("spotify:track:{id}"),
            available,
            ..Default::default()
        };
        let hidden = |uri: &str| uri == "spotify:track:h";
        assert!(playable(&track("a", true), &hidden));
        assert!(!playable(&track("h", true), &hidden));
        assert!(!playable(&track("x", false), &hidden));
    }
}
