//! Библиотека (исполнитель «services-core»): «Любимые треки» (liked.rs: кэш 15 мин, намерения поверх лагающих
//! чтений), плейлисты из rootlist сессии (папки, права, шапки), редактирование плейлиста (сразу через сессию —
//! data/playlist_session.rs, Web API — запасной путь; откат по ошибке),
//! альбомы/исполнители/подкасты/выпуски, локальные закрепления и «не рекомендовать».
//! Правила перенесены из spotifast (src/app.rs).
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use md3_model::{
    AccountId, AlbumItem, ArtistItem, EpisodeItem, LibraryEntry, ListChange, PlaylistId, PlaylistItem, ShowItem,
    TrackItem, contexts,
};
use sf_api::models::{self, SavedTrack};
use sf_api::{ApiClient, ApiError, Operation, map};
use sf_player::{PlayerEvent, PlayerHandle, Rootlist, RootlistEntry};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use mf_platform::{SettingKey, SettingsStore};

use crate::auth::{AuthEvent, AuthService};
use crate::data::liked::{self, LikedSongs};
use crate::data::paged::PagedList;
use crate::data::text::list_change;
use crate::data::collection_session::{self, LikedFeed};
use crate::data::{known, library_cache, playlist_cache, playlist_session, podcast_session, route};
use sf_player::engine::session_playlists::Change;
use crate::history::HistoryService;
use crate::lock;

/// Страница Liked Songs у Web API.
pub const CHUNK: usize = 50;

/// Запись в плейлист — не больше 100 URI за запрос.
const WRITE_CHUNK: usize = 100;
/// Потолок страниц одного плейлиста (по 50).
const MAX_PLAYLIST_PAGES: u32 = 200;
/// Потолок страниц раздела библиотеки.
const MAX_SECTION_PAGES: u32 = 100;
/// Паузы перед самостоятельными повторами, когда раздел не загрузился ни через сессию, ни через
/// Web API (сессия не ответила вовремя, Web API под 429): пустой список без повтора — тупик.
const AUTO_RETRIES: [std::time::Duration; 2] =
    [std::time::Duration::from_secs(15), std::time::Duration::from_secs(60)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackKind {
    Like,
    Unlike,
    /// локальный «не рекомендовать» (у Spotify нет API)
    Dislike,
}

/// Раздел библиотеки для items(kind, …).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryKind {
    #[default]
    Playlists,
    Albums,
    Artists,
    Shows,
}

impl LibraryKind {
    /// "playlists" | "albums" | "artists" | "shows"
    pub fn from_name(name: &str) -> Self {
        match name {
            "albums" => Self::Albums,
            "artists" => Self::Artists,
            "shows" | "podcasts" => Self::Shows,
            _ => Self::Playlists,
        }
    }
}

/// Шапка плейлиста (PlaylistPage): описание, владелец, видимость, права, длительность.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaylistInfo {
    pub item: PlaylistItem,
    pub total_duration_ms: i64,
    pub followed: bool,
}

// события короткоживущие (broadcast), упаковка трека в Box только усложнила бы сопоставление в мостах
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum LibraryEvent {
    Playlists(ListChange<PlaylistItem>),
    Liked(ListChange<TrackItem>),
    PlaylistTracks { id: PlaylistId, change: ListChange<TrackItem> },
    /// изменилась шапка плейлиста (название/описание/обложка/видимость/права)
    PlaylistInfo(PlaylistId),
    Albums(ListChange<AlbumItem>),
    Artists(ListChange<ArtistItem>),
    Shows(ListChange<ShowItem>),
    Episodes(ListChange<EpisodeItem>),
    /// дерево rootlist/закрепления/сортировка — перечитать entries()
    EntriesChanged,
    LikesChanged,
    /// сохранённость альбомов/шоу/выпусков/подписки
    SavedChanged,
    ErrorChanged,
    PinsChanged,
    Notice(String),
    /// для радио (стоп-лист) и истории
    Feedback { kind: FeedbackKind, track: TrackItem },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    Playlists,
    Albums,
    Artists,
    Shows,
    Episodes,
}

#[derive(Default)]
struct PlaylistPage {
    generation: u64,
    header: Option<models::Playlist>,
    rows: Vec<TrackItem>,
    snapshot: String,
    complete: bool,
    loading: bool,
    /// идущее чтение: отменяется закрытием экрана или явным обновлением (записи это не трогает)
    read: Option<tokio::task::AbortHandle>,
    pending_writes: usize,
    /// после записи перечитать (ошибка или чтение во время записи)
    reload_after_write: bool,
    /// snapshot_id из ответа последней записи: шапка со старым snapshot — устаревшая (перепроверка ≤ 3 раз)
    expected_snapshot: Option<String>,
}

/// Паузы перед перечитыванием шапки, пока Spotify не подтвердит snapshot записи.
const SNAPSHOT_RECHECKS: [std::time::Duration; 3] = [
    std::time::Duration::from_millis(1500),
    std::time::Duration::from_secs(3),
    std::time::Duration::from_secs(6),
];

/// Что делать при открытии плейлиста (записи уже нет: её проверяет вызывающий).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenAction {
    /// кэш уже на месте и показан в модели — ни Reset, ни перечитывания
    Idle,
    /// чтение уже идёт: не дублируем его и не сбрасываем модель, новому экрану остаётся Loading(true)
    Loading,
    /// перечитать
    Fetch,
}

impl PlaylistPage {
    /// Шапка этого экрана уже сверена со snapshot: страница не может устареть незаметно.
    fn header_confirmed(&self) -> bool {
        self.header.as_ref().is_some_and(|h| {
            let snapshot = h.snapshot_id.clone().unwrap_or_default();
            !snapshot.is_empty() && snapshot == self.snapshot
        })
    }

    fn open_action(&self, force: bool) -> OpenAction {
        if self.loading {
            return if force { OpenAction::Fetch } else { OpenAction::Loading };
        }
        if !force && self.complete && !self.reload_after_write && self.header_confirmed() {
            OpenAction::Idle
        } else {
            OpenAction::Fetch
        }
    }
}

/// Тот же аккаунт и то же поколение страницы: ответ чтения ещё «свой».
fn same_read(state: &State, id: &PlaylistId, generation: u64, page_generation: u64) -> bool {
    state.generation == generation
        && state.pages.get(id).is_some_and(|page| page.generation == page_generation)
}

/// То же, и записи не начались: их оптимистичные строки перечитывание не трогает.
fn read_is_current(state: &State, id: &PlaylistId, generation: u64, page_generation: u64) -> bool {
    same_read(state, id, generation, page_generation)
        && state.pages.get(id).is_some_and(|page| page.pending_writes == 0)
}

/// Кэш плейлиста и его перезаливка в модель уходят кусками: первый — Reset, дальше — Append.
/// Один гигантский Reset не готовится и не применяется целиком; порядок и повторы строк сохраняются.
fn cached_events(id: &PlaylistId, rows: &[TrackItem]) -> Vec<LibraryEvent> {
    rows.chunks(CHUNK)
        .enumerate()
        .map(|(i, chunk)| {
            let change =
                if i == 0 { ListChange::Reset(chunk.to_vec()) } else { ListChange::Append(chunk.to_vec()) };
            LibraryEvent::PlaylistTracks { id: id.clone(), change }
        })
        .collect()
}

#[derive(Default)]
struct State {
    account: Option<String>,
    generation: u64,
    error_text: String,

    liked: LikedSongs,
    liked_view: PagedList<SavedTrack>,
    liked_rows: Vec<TrackItem>,
    liked_loading: bool,
    liked_running: bool,
    liked_uris: HashSet<String>,
    liked_recordings: HashSet<String>,
    likes_revision: u64,

    playlists_raw: Vec<models::Playlist>,
    playlists: Vec<PlaylistItem>,
    playlists_loading: bool,
    rootlist: Option<Rootlist>,
    rootlist_loading: bool,
    /// список плейлистов взят из rootlist сессии — Web API его уже не перезаписывает
    session_list: bool,
    /// что загрузилось без сессии librespot (вход раньше движка, через Web API) — повторить, когда сессия встанет
    liked_without_session: bool,
    sections_without_session: HashSet<Section>,
    /// самостоятельных повторов после ошибки за этот вход; повтор ждёт своей паузы
    auto_retries: usize,
    auto_retry_pending: bool,
    pages: HashMap<PlaylistId, PlaylistPage>,

    albums: Vec<(String, AlbumItem)>,
    artists: Vec<ArtistItem>,
    shows: Vec<(String, ShowItem)>,
    episodes: Vec<EpisodeItem>,
    progress: HashMap<Section, (u32, u32)>,
    saved: HashSet<String>,
    saved_intents: HashMap<String, bool>,
    saved_revision: u64,

    pins: Vec<String>,
    pins_revision: u64,
    hidden: HashSet<String>,

    /// загруженные обложки, ещё не подтверждённые Spotify (id плейлиста →)
    uploaded_covers: HashMap<String, UploadedCover>,
    /// плейлисты, чья обложка — наш коллаж из первых строк (своей нет): после правок он пересобирается
    mosaics: HashSet<String>,
    cover_requests: u64,
}

struct UploadedCover {
    request: u64,
    /// file:// локальной копии ("" — копия не записалась)
    local_url: String,
    /// адреса картинок до загрузки и отвергнутые сравнением
    previous_urls: Vec<String>,
}

/// Перепроверки обложки после загрузки (Spotify обрабатывает картинку не сразу).
const COVER_RECHECKS: [std::time::Duration; 3] =
    [std::time::Duration::from_secs(3), std::time::Duration::from_secs(8), std::time::Duration::from_secs(20)];

/// Картинка целиком (≤ 20 МБ), через общий клиент с прокси.
async fn fetch_bytes(http: &sf_api::Http, url: &str) -> Result<Vec<u8>, String> {
    let client = http.client()?;
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.without_url().to_string())?;
    if response.content_length().is_some_and(|length| length > 20 * 1024 * 1024) {
        return Err("image is too large".to_owned());
    }
    response.bytes().await.map(|bytes| bytes.to_vec()).map_err(|error| error.without_url().to_string())
}

pub struct LibraryService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    history: Arc<HistoryService>,
    settings: Arc<SettingsStore>,
    events: broadcast::Sender<LibraryEvent>,
    state: Mutex<State>,
    me: Weak<LibraryService>,
}

fn read_list(settings: &SettingsStore, key: SettingKey) -> Vec<String> {
    settings.get_string_list(key)
}

fn write_list(settings: &SettingsStore, key: SettingKey, list: &[String]) {
    settings.set_string_list(key, list);
}

fn now_seconds() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// Строка плейлиста; локальный файл — недоступная строка (позиции совпадают с сервером).
fn playlist_row(item: &models::PlaylistItem, collaborative: bool) -> Option<TrackItem> {
    let playable = item.playable()?;
    let mut row = match playable {
        models::PlayableItem::Track(track) if track.is_local || item.is_local => TrackItem {
            title: track.name.clone(),
            artists: track.artist_names(),
            uri: track.uri.clone(),
            duration_ms: i64::from(track.duration_ms),
            available: false,
            ..Default::default()
        },
        other => map::playable_item(other)?,
    };
    row.added_at = item.added_at.clone().unwrap_or_default();
    if collaborative {
        row.added_by = item.added_by.as_ref().and_then(|user| user.id.clone()).unwrap_or_default();
    }
    Some(row)
}

fn duration(rows: &[TrackItem]) -> i64 {
    rows.iter().map(|row| row.duration_ms.max(0)).sum()
}

fn uri_kind(uri: &str) -> &str {
    sf_api::util::uri_kind(uri).unwrap_or("")
}

/// Папка rootlist и её прямые дети.
#[derive(Clone, Debug, Default)]
struct Folder {
    id: String,
    name: String,
    parent: String,
    /// "playlist:<uri>" | "folder:<id>"
    children: Vec<String>,
}

/// Дерево rootlist: корень и папки.
fn folder_tree(rootlist: &Rootlist) -> (Vec<String>, HashMap<String, Folder>) {
    let mut root = Vec::new();
    let mut folders: HashMap<String, Folder> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    for entry in &rootlist.entries {
        match entry {
            RootlistEntry::Playlist(uri) => {
                let key = format!("playlist:{uri}");
                match stack.last() {
                    Some(parent) => folders.entry(parent.clone()).or_default().children.push(key),
                    None => root.push(key),
                }
            }
            // папка внутри самой себя или уже встреченная дала бы цикл (и бесконечную рекурсию в collect_placed):
            // её содержимое идёт в уже известную папку, а при вложении в себя — в текущую
            RootlistEntry::FolderStart { id, .. } if stack.contains(id) || folders.get(id).is_some_and(|f| !f.id.is_empty()) => {
                let into = if stack.contains(id) { stack.last().cloned().unwrap_or_default() } else { id.clone() };
                stack.push(into);
            }
            RootlistEntry::FolderStart { id, name } => {
                let parent = stack.last().cloned().unwrap_or_default();
                let key = format!("folder:{id}");
                match stack.last() {
                    Some(parent) => folders.entry(parent.clone()).or_default().children.push(key),
                    None => root.push(key),
                }
                let folder = folders.entry(id.clone()).or_default();
                folder.id = id.clone();
                folder.name = name.clone();
                folder.parent = parent;
                stack.push(id.clone());
            }
            RootlistEntry::FolderEnd => {
                stack.pop();
            }
        }
    }
    (root, folders)
}

/// Сводка «последнее добавленное/обновлённое» из строк всех разделов: без папок, новые сверху.
fn merge_recent(mut all: Vec<LibraryEntry>, limit: usize) -> Vec<LibraryEntry> {
    all.retain(|entry| entry.kind != "folder");
    // «последнее касание»: у плейлиста это добавление, у исполнителя — прослушивание
    let touched = |entry: &LibraryEntry| entry.added_at.clone().max(entry.played_at.clone());
    all.sort_by(|a, b| touched(b).cmp(&touched(a)));
    all.truncate(limit);
    all
}

/// Сортировка строк экрана «Библиотека» (закреплённые — первыми в порядке закрепления).
fn sort_entries(entries: &mut [LibraryEntry], sort: &str, pins: &[String]) {
    let pin_rank = |entry: &LibraryEntry| pins.iter().position(|uri| *uri == entry.uri).unwrap_or(usize::MAX);
    let liked_first = |entry: &LibraryEntry| entry.kind != "liked";
    match sort {
        "recent" => entries.sort_by(|a, b| {
            (liked_first(a), pin_rank(a))
                .cmp(&(liked_first(b), pin_rank(b)))
                .then_with(|| a.played_at.is_empty().cmp(&b.played_at.is_empty()))
                .then_with(|| b.played_at.cmp(&a.played_at))
        }),
        "added" => entries.sort_by(|a, b| {
            (liked_first(a), pin_rank(a))
                .cmp(&(liked_first(b), pin_rank(b)))
                .then_with(|| a.added_at.is_empty().cmp(&b.added_at.is_empty()))
                .then_with(|| b.added_at.cmp(&a.added_at))
        }),
        "alpha" => entries.sort_by(|a, b| {
            (liked_first(a), pin_rank(a))
                .cmp(&(liked_first(b), pin_rank(b)))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }),
        "author" => entries.sort_by(|a, b| {
            (liked_first(a), pin_rank(a))
                .cmp(&(liked_first(b), pin_rank(b)))
                .then_with(|| a.owner.to_lowercase().cmp(&b.owner.to_lowercase()))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }),
        _ => entries.sort_by_key(|entry| (liked_first(entry), pin_rank(entry))),
    }
}

impl State {
    fn account_id(&self) -> Option<AccountId> {
        self.account.clone().map(AccountId::new)
    }

    fn owns(&self, playlist: &models::Playlist) -> bool {
        self.account.as_deref().is_some_and(|me| playlist.owned_by(me))
    }

    fn can_edit_raw(&self, playlist: &models::Playlist) -> bool {
        self.owns(playlist)
            || playlist.collaborative
            || self.rootlist.as_ref().is_some_and(|root| root.editable.contains(&playlist.uri))
    }

    fn playlist_item(&self, playlist: &models::Playlist) -> PlaylistItem {
        let mut item = map::playlist_item(playlist, self.account_id().as_ref());
        item.can_edit = self.can_edit_raw(playlist);
        item.pinned = self.pins.contains(&item.uri);
        if let Some(upload) = self.uploaded_covers.get(&playlist.id).filter(|upload| !upload.local_url.is_empty()) {
            item.cover = upload.local_url.clone();
        }
        item
    }

    fn raw(&self, id: &str) -> Option<&models::Playlist> {
        self.playlists_raw.iter().find(|pl| pl.id == id)
    }

    /// Пересобрать строки плейлистов; изменение для модели.
    fn rebuild_playlists(&mut self) -> Option<LibraryEvent> {
        let rows: Vec<PlaylistItem> = self.playlists_raw.iter().map(|pl| self.playlist_item(pl)).collect();
        let change = list_change(&self.playlists, &rows);
        self.playlists = rows;
        change.map(LibraryEvent::Playlists)
    }

    fn header(&self, id: &str) -> Option<models::Playlist> {
        let page = self.pages.get(id).and_then(|page| page.header.clone());
        match (page, self.raw(id)) {
            (Some(mut header), Some(listed)) => {
                header.fill_from(listed);
                Some(header)
            }
            (Some(header), None) => Some(header),
            (None, listed) => listed.cloned(),
        }
    }

    fn playlist_info(&self, id: &str) -> Option<PlaylistInfo> {
        let header = self.header(id)?;
        let mut item = self.playlist_item(&header);
        let page = self.pages.get(id);
        if let Some(page) = page.filter(|page| page.complete) {
            item.track_count = page.rows.len() as i64;
        }
        Some(PlaylistInfo {
            item,
            total_duration_ms: page.map(|page| duration(&page.rows)).unwrap_or(0),
            followed: self.raw(id).is_some() || self.owns(&header),
        })
    }

    /// Пересобрать «Любимые треки» из намерений; события для моделей.
    fn sync_liked(&mut self) -> Vec<LibraryEvent> {
        let mut events = Vec::new();
        self.liked.sync_view(&mut self.liked_view);
        let rows: Vec<TrackItem> = self.liked_view.items.iter().map(map::saved_track).collect();
        if let Some(change) = list_change(&self.liked_rows, &rows) {
            events.push(LibraryEvent::Liked(change));
        }
        self.liked_rows = rows;
        let uris: HashSet<String> = self.liked_view.items.iter().map(|item| item.track.uri.clone()).collect();
        if uris != self.liked_uris {
            self.liked_recordings =
                self.liked_view.items.iter().filter_map(|item| item.track.recording_key()).collect();
            known::remember_recordings(self.liked_view.items.iter().map(|item| &item.track));
            self.liked_uris = uris;
            self.likes_revision += 1;
            events.push(LibraryEvent::LikesChanged);
            events.push(LibraryEvent::EntriesChanged);
        }
        let loading = self.liked.loading() || self.liked_running;
        if loading != self.liked_loading {
            self.liked_loading = loading;
            events.push(LibraryEvent::Liked(ListChange::Loading(loading)));
        }
        events
    }

    fn is_liked(&self, uri: &str) -> bool {
        if let Some(intent) = self.liked.intent(uri) {
            return intent;
        }
        if self.liked_uris.contains(uri) {
            return true;
        }
        known::recording(uri).is_some_and(|key| self.liked_recordings.contains(&key))
    }

    fn is_saved(&self, uri: &str) -> bool {
        if let Some(intent) = self.saved_intents.get(uri) {
            return *intent;
        }
        match uri_kind(uri) {
            "playlist" => {
                let id = sf_api::util::uri_id(uri).unwrap_or_default();
                self.raw(id).is_some()
            }
            "track" => self.is_liked(uri),
            _ => self.saved.contains(uri),
        }
    }

    /// Сохранённое — из прочитанных разделов; намерение снимается, когда чтение его подтвердило
    /// (снятие — только по полному разделу).
    fn rebuild_saved(&mut self, complete: bool) {
        let mut saved: HashSet<String> = self.albums.iter().map(|(_, item)| item.uri.clone()).collect();
        saved.extend(self.artists.iter().map(|item| item.uri.clone()));
        saved.extend(self.shows.iter().map(|(_, item)| item.uri.clone()));
        saved.extend(self.episodes.iter().map(|item| item.uri.clone()));
        self.saved_intents.retain(|uri, want| {
            matches!(uri_kind(uri), "playlist" | "track")
                || !((*want && saved.contains(uri)) || (!*want && complete && !saved.contains(uri)))
        });
        self.saved = saved;
    }

    fn set_error(&mut self, text: String) -> Option<LibraryEvent> {
        (self.error_text != text).then(|| {
            self.error_text = text;
            LibraryEvent::ErrorChanged
        })
    }

    fn playlist_entry(&self, item: &PlaylistItem, folder: &str, times: &HashMap<String, String>) -> LibraryEntry {
        let own = self.account.as_deref().is_some_and(|me| me == item.owner_id);
        let owner = if own { mf_i18n::t("You") } else { item.owner.clone() };
        let mut subtitle = mf_i18n::tf("Playlist · {0}", &[&owner]);
        if item.track_count > 0 {
            subtitle.push_str(&format!(" · {}", mf_i18n::n(item.track_count, "{0} track", "{0} tracks")));
        }
        LibraryEntry {
            kind: "playlist".to_owned(),
            id: item.playlist_id.clone(),
            uri: item.uri.clone(),
            title: item.title.clone(),
            subtitle,
            cover: item.cover.clone(),
            covers: if item.cover.is_empty() { Vec::new() } else { vec![item.cover.clone()] },
            owner: item.owner.clone(),
            owner_id: item.owner_id.clone(),
            can_edit: item.can_edit,
            read_only: !item.can_edit,
            pinned: item.pinned,
            track_count: item.track_count,
            added_at: String::new(),
            played_at: times.get(&item.uri).cloned().unwrap_or_default(),
            folder_id: folder.to_owned(),
        }
    }

    fn folder_entry(&self, folder: &Folder, folders: &HashMap<String, Folder>) -> LibraryEntry {
        let mut covers = Vec::new();
        let mut count = 0;
        for child in &folder.children {
            if let Some(uri) = child.strip_prefix("playlist:") {
                count += 1;
                if covers.len() < 4
                    && let Some(item) = self.playlists.iter().find(|item| item.uri == uri)
                    && !item.cover.is_empty()
                {
                    covers.push(item.cover.clone());
                }
            } else if let Some(id) = child.strip_prefix("folder:") {
                count += folders.get(id).map_or(0, |f| f.children.len());
            }
        }
        LibraryEntry {
            kind: "folder".to_owned(),
            id: folder.id.clone(),
            uri: format!("spotify:folder:{}", folder.id),
            title: if folder.name.is_empty() { mf_i18n::t("Folder") } else { folder.name.clone() },
            subtitle: mf_i18n::tf("Folder · {0}", &[&mf_i18n::n(count as i64, "{0} playlist", "{0} playlists")]),
            cover: covers.first().cloned().unwrap_or_default(),
            covers,
            read_only: true,
            pinned: self.pins.contains(&format!("spotify:folder:{}", folder.id)),
            folder_id: folder.parent.clone(),
            ..Default::default()
        }
    }

    fn liked_entry(&self, times: &HashMap<String, String>) -> LibraryEntry {
        let uri = contexts::liked_uri(self.account.as_deref().unwrap_or_default());
        let count = self.liked_view.total.map(i64::from).unwrap_or(self.liked_rows.len() as i64);
        LibraryEntry {
            kind: "liked".to_owned(),
            id: "liked".to_owned(),
            title: mf_i18n::t("Liked Songs"),
            subtitle: mf_i18n::tf("Playlist · {0}", &[&mf_i18n::n(count, "{0} track", "{0} tracks")]),
            owner: mf_i18n::t("You"),
            owner_id: self.account.clone().unwrap_or_default(),
            can_edit: false,
            read_only: true,
            pinned: true,
            track_count: count,
            played_at: times.get(&uri).cloned().unwrap_or_default(),
            uri,
            ..Default::default()
        }
    }

    fn owner_matches(&self, item: &PlaylistItem, owner: &str) -> bool {
        let own = self.account.as_deref().is_some_and(|me| me == item.owner_id);
        let collab = !own && (item.collaborative || item.can_edit);
        match owner {
            "mine" => own,
            "collab" => collab,
            "followed" => !own && !collab,
            _ => true,
        }
    }
}

impl LibraryService {
    /// SignedIn → refresh, SignedOut → очистка (поздние ответы старого аккаунта отбрасываются).
    /// history — сортировка «Недавно прослушанные» и LibraryEntry.played_at (recent_contexts).
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        history: Arc<HistoryService>,
        settings: Arc<SettingsStore>,
    ) -> Arc<Self> {
        let state = State {
            pins: read_list(&settings, SettingKey::Pins),
            hidden: read_list(&settings, SettingKey::HiddenTracks).into_iter().collect(),
            ..Default::default()
        };
        let service = Arc::new_cyclic(|me| Self {
            rt,
            auth,
            player,
            history,
            settings,
            events: broadcast::channel(1024).0,
            state: Mutex::new(state),
            me: me.clone(),
        });
        service.spawn_listeners();
        service
    }

    fn spawn_listeners(&self) {
        let mut auth = self.auth.subscribe();
        let me = self.me.clone();
        self.rt.spawn(async move {
            loop {
                let event = auth.recv().await;
                let Some(service) = me.upgrade() else { return };
                match event {
                    Ok(AuthEvent::SignedIn(account)) => service.switch_account(Some(account.id)),
                    Ok(AuthEvent::SignedOut) => service.switch_account(None),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        service.switch_account(service.auth.account().map(|a| a.id));
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        let mut player = self.player.subscribe().events;
        let me = self.me.clone();
        self.rt.spawn(async move {
            loop {
                match player.recv().await {
                    Ok(PlayerEvent::LocalChanged) => {
                        let Some(service) = me.upgrade() else { return };
                        if !service.player.local_playback().is_ready() {
                            continue;
                        }
                        // аккаунт сессии вход ставит по тому же событию — может и чуть позже этого
                        // слушателя; без него перечитывание через сессию молча не начнётся
                        service.spawn(|service| async move {
                            let api = service.auth.api();
                            for _ in 0..20 {
                                if route::account_session(&api, &service.player).is_some() {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            }
                            service.reload_with_session();
                            let wanted = {
                                let state = lock(&service.state);
                                // список с диска (library_cache) — тоже повод: свежий rootlist заменит его
                                state.account.is_some() && (state.rootlist.is_none() || !state.session_list)
                                    && !state.rootlist_loading
                            };
                            if wanted {
                                service.load_rootlist();
                            }
                        });
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn spawn<F>(&self, task: impl FnOnce(Arc<Self>) -> F + Send + 'static)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let me = self.me.clone();
        self.rt.spawn(async move {
            if let Some(service) = me.upgrade() {
                task(service).await;
            }
        });
    }

    /// Фоновое обновление: запасной Web API уступает страницам и не идёт в сеть, пока он ограничен.
    fn spawn_background<F>(&self, task: impl FnOnce(Arc<Self>) -> F + Send + 'static)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawn(move |service| sf_api::limits::background(task(service)));
    }

    fn send(&self, events: impl IntoIterator<Item = LibraryEvent>) {
        for event in events {
            let _ = self.events.send(event);
        }
    }

    fn notice(&self, text: impl Into<String>) {
        let _ = self.events.send(LibraryEvent::Notice(text.into()));
    }

    fn error_text_of(&self, error: &ApiError) -> String {
        self.auth.note_api_error(error);
        route::api_error_text(error)
    }

    async fn client(&self, operation: Operation) -> Result<Arc<ApiClient>, ApiError> {
        self.auth.api().client_for(operation).await
    }

    fn generation(&self) -> u64 {
        lock(&self.state).generation
    }

    fn current(&self, generation: u64) -> bool {
        lock(&self.state).generation == generation
    }

    /// Новый аккаунт (или выход): всё старое — сбросить, поздние ответы — отбросить.
    fn switch_account(&self, account: Option<String>) {
        let events = {
            let mut state = lock(&self.state);
            if state.account == account && account.is_some() {
                drop(state);
                self.load_all(false);
                return;
            }
            let generation = state.generation + 1;
            let opened: Vec<PlaylistId> = state.pages.keys().cloned().collect();
            let pins = std::mem::take(&mut state.pins);
            let hidden = std::mem::take(&mut state.hidden);
            let revisions = (state.likes_revision, state.saved_revision, state.pins_revision);
            *state = State {
                account: account.clone(),
                generation,
                pins,
                hidden,
                likes_revision: revisions.0 + 1,
                saved_revision: revisions.1 + 1,
                pins_revision: revisions.2,
                ..Default::default()
            };
            let mut events: Vec<LibraryEvent> = opened
                .into_iter()
                .map(|id| LibraryEvent::PlaylistTracks { id, change: ListChange::Reset(Vec::new()) })
                .collect();
            events.extend([
                LibraryEvent::Playlists(ListChange::Reset(Vec::new())),
                LibraryEvent::Liked(ListChange::Reset(Vec::new())),
                LibraryEvent::Albums(ListChange::Reset(Vec::new())),
                LibraryEvent::Artists(ListChange::Reset(Vec::new())),
                LibraryEvent::Shows(ListChange::Reset(Vec::new())),
                LibraryEvent::Episodes(ListChange::Reset(Vec::new())),
                LibraryEvent::EntriesChanged,
                LibraryEvent::LikesChanged,
                LibraryEvent::SavedChanged,
                LibraryEvent::ErrorChanged,
            ]);
            events
        };
        self.send(events);
        if account.is_some() {
            self.hydrate_from_cache();
            self.load_all(false);
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LibraryEvent> {
        self.events.subscribe()
    }

    pub fn refresh(&self) {
        lock(&self.state).auto_retries = 0;
        self.load_all(true);
    }

    /// Раздел не загрузился ни через сессию, ни через Web API: ошибка (с «Повторить») уже на экране,
    /// а через паузу — повтор сам, не больше AUTO_RETRIES раз за вход.
    fn retry_later(&self, generation: u64) {
        let pause = {
            let mut state = lock(&self.state);
            if state.generation != generation || state.auto_retry_pending {
                return;
            }
            let Some(&pause) = AUTO_RETRIES.get(state.auto_retries) else { return };
            state.auto_retries += 1;
            state.auto_retry_pending = true;
            pause
        };
        self.spawn(move |service| async move {
            tokio::time::sleep(pause).await;
            {
                let mut state = lock(&service.state);
                if state.generation != generation {
                    return;
                }
                state.auto_retry_pending = false;
            }
            tracing::debug!("library: retrying what didn't load");
            service.load_all(false);
        });
    }

    fn load_all(&self, force: bool) {
        if lock(&self.state).account.is_none() {
            return;
        }
        if let Some(event) = lock(&self.state).set_error(String::new()) {
            self.send([event]);
        }
        self.load_liked(force);
        self.load_playlists();
        self.load_rootlist();
        for section in [Section::Albums, Section::Artists, Section::Shows, Section::Episodes] {
            self.load_section(section);
        }
        let ids: Vec<PlaylistId> = lock(&self.state).pages.keys().cloned().collect();
        if force {
            for id in ids {
                // явное обновление: идущие чтения перезапускаются, готовый кэш перечитывается
                self.refresh_playlist(id);
            }
        }
    }

    /// Сессия встала: что загрузилось без неё (через Web API), перечитать через неё — один раз; плейлисты
    /// дочитывает load_rootlist. Раздел, который ещё грузится, повторится по своему окончанию.
    fn reload_with_session(&self) {
        if route::account_session(&self.auth.api(), &self.player).is_none() {
            return;
        }
        let (liked, sections) = {
            let mut state = lock(&self.state);
            if state.account.is_none() {
                return;
            }
            let liked = !state.liked_running && std::mem::take(&mut state.liked_without_session);
            let idle: Vec<Section> =
                state.sections_without_session.iter().filter(|s| !state.progress.contains_key(*s)).copied().collect();
            for section in &idle {
                state.sections_without_session.remove(section);
            }
            (liked, idle)
        };
        if liked {
            self.load_liked(true);
        }
        for section in sections {
            self.load_section(section);
        }
    }

    // --- «Любимые треки» ---------------------------------------------------------------------

    fn load_liked(&self, force: bool) {
        let (generation, account) = {
            let mut state = lock(&self.state);
            if state.liked_running {
                return;
            }
            let Some(account) = state.account.clone() else { return };
            state.liked_running = true;
            (state.generation, account)
        };
        self.spawn_background(move |service| async move {
            let path = mf_platform::dirs::liked_cache_file(&account);
            let checked = lock(&service.state).liked.cache_checked;
            if !checked {
                lock(&service.state).liked.cache_loading = true;
                let cache = liked::read(&path, &account).await;
                let events = {
                    let mut state = lock(&service.state);
                    if state.generation != generation {
                        return;
                    }
                    if let Some(cache) = cache {
                        state.liked.restore(cache);
                    }
                    state.liked.cache_loading = false;
                    state.liked.cache_checked = true;
                    state.sync_liked()
                };
                service.send(events);
            }
            {
                let mut state = lock(&service.state);
                if force || !state.liked.fresh(now_seconds()) {
                    state.liked.start_refresh(generation);
                }
            }
            let events = lock(&service.state).sync_liked();
            service.send(events);
            // сначала коллекция сессии (без квоты Web API); не смогла — дальше Web API с того же места
            let mut feed: Option<LikedFeed> = None;
            let mut feed_opened = false;
            loop {
                let Some(offset) = lock(&service.state).liked.request_offset() else { break };
                if !feed_opened {
                    feed_opened = true;
                    route::wait_account_session(&service.auth.api(), &service.player, route::STARTUP_SESSION_WAIT).await;
                    feed = LikedFeed::open(&service.auth.api(), &service.player).await;
                    if feed.is_none() && route::account_session(&service.auth.api(), &service.player).is_none() {
                        let mut state = lock(&service.state);
                        if state.generation == generation {
                            state.liked_without_session = true;
                        }
                    }
                }
                let from_session = match feed.as_mut() {
                    Some(open) => open.next_page(offset).await,
                    None => None,
                };
                if from_session.is_none() {
                    feed = None;
                }
                let result = match from_session {
                    Some(page) => Ok(page),
                    None => async {
                        service.client(Operation::UserData).await?.saved_tracks(offset, CHUNK as u32).await
                    }
                    .await,
                };
                let (events, cache) = {
                    let mut state = lock(&service.state);
                    if state.generation != generation {
                        return;
                    }
                    match result {
                        Ok(page) => {
                            state.liked.absorb(offset, page, now_seconds());
                            let cache = state.liked.checkpoint(account.clone(), false);
                            (state.sync_liked(), cache)
                        }
                        Err(error) => {
                            drop(state);
                            let text = service.error_text_of(&error);
                            let mut state = lock(&service.state);
                            state.liked.fail(text.clone());
                            let mut events = state.sync_liked();
                            events.extend(state.set_error(mf_i18n::tf("Couldn’t load Liked Songs: {0}", &[&text])));
                            state.liked_running = false;
                            events.extend(state.sync_liked());
                            drop(state);
                            service.send(events);
                            // сессия встала, пока шёл Web API, — повторить через неё
                            service.reload_with_session();
                            service.retry_later(generation);
                            return;
                        }
                    }
                };
                service.send(events);
                if let Some(cache) = cache
                    && let Err(error) = liked::write(&path, &cache).await
                {
                    tracing::debug!("Liked Songs cache not written: {error:#}");
                }
            }
            let (events, cache) = {
                let mut state = lock(&service.state);
                if state.generation != generation {
                    return;
                }
                state.liked_running = false;
                let cache = state.liked.checkpoint(account.clone(), true);
                (state.sync_liked(), cache)
            };
            service.send(events);
            if let Some(cache) = cache
                && let Err(error) = liked::write(&path, &cache).await
            {
                tracing::debug!("Liked Songs cache not written: {error:#}");
            }
            service.reload_with_session();
        });
    }

    fn checkpoint_liked(&self) {
        let (account, cache) = {
            let mut state = lock(&self.state);
            let Some(account) = state.account.clone() else { return };
            let cache = state.liked.checkpoint(account.clone(), true);
            (account, cache)
        };
        if let Some(cache) = cache {
            self.rt.spawn(async move {
                let path = mf_platform::dirs::liked_cache_file(&account);
                if let Err(error) = liked::write(&path, &cache).await {
                    tracing::debug!("Liked Songs cache not written: {error:#}");
                }
            });
        }
    }

    pub fn liked(&self) -> Vec<TrackItem> {
        lock(&self.state).liked_rows.clone()
    }
    pub fn liked_loading(&self) -> bool {
        lock(&self.state).liked_loading
    }
    /// По id трека или URI (рыночные дубли одной записи — один лайк).
    pub fn is_liked(&self, track_id_or_uri: &str) -> bool {
        lock(&self.state).is_liked(&known::track_uri(track_id_or_uri))
    }
    pub fn likes_revision(&self) -> u64 {
        lock(&self.state).likes_revision
    }

    /// Оптимистично: UI сразу, откат + Notice при ошибке.
    pub fn toggle_like(&self, track: TrackItem) {
        if track.is_episode {
            self.toggle_saved(track.play_uri());
            return;
        }
        let uri = track.play_uri();
        known::remember([&track]);
        let (target, uris, events) = {
            let mut state = lock(&self.state);
            if state.account.is_none() {
                drop(state);
                self.notice(mf_i18n::t("Sign in to your account"));
                return;
            }
            let target = !state.is_liked(&uri);
            let mut uris = vec![uri.clone()];
            if !target && let Some(key) = known::recording(&uri) {
                for item in &state.liked_view.items {
                    if item.track.recording_key().as_deref() == Some(key.as_str()) && !uris.contains(&item.track.uri) {
                        uris.push(item.track.uri.clone());
                    }
                }
            }
            for other in &uris {
                let source = if *other == uri {
                    known::track_from_item(&track)
                } else {
                    state
                        .liked_view
                        .items
                        .iter()
                        .find(|item| item.track.uri == *other)
                        .map(|item| item.track.clone())
                        .unwrap_or_else(|| known::track_from_item(&track))
                };
                state.liked.change(other.clone(), target, source);
            }
            (target, uris, state.sync_liked())
        };
        self.send(events);
        let kind = if target { FeedbackKind::Like } else { FeedbackKind::Unlike };
        self.send([LibraryEvent::Feedback { kind, track }]);
        let generation = self.generation();
        self.spawn(move |service| async move {
            let result = collection_session::write(&service.auth.api(), &service.player, &uris, target).await;
            if !service.current(generation) {
                return;
            }
            let ok = result.is_ok();
            let events = {
                let mut state = lock(&service.state);
                for uri in &uris {
                    state.liked.confirm(uri, target, ok);
                }
                state.sync_liked()
            };
            service.send(events);
            match result {
                Ok(()) => service.checkpoint_liked(),
                Err(error) => {
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Liked Songs didn’t change: {0}", &[&text]));
                }
            }
        });
    }

    /// Локальный стоп-лист (HiddenTracks) + Feedback{Dislike}; если трек играет — next().
    pub fn dislike(&self, track: TrackItem) {
        let uri = track.play_uri();
        let list = {
            let mut state = lock(&self.state);
            if !state.hidden.insert(uri.clone()) {
                None
            } else {
                let mut list: Vec<String> = state.hidden.iter().cloned().collect();
                list.sort();
                Some(list)
            }
        };
        if let Some(list) = list {
            write_list(&self.settings, SettingKey::HiddenTracks, &list);
        }
        self.send([LibraryEvent::Feedback { kind: FeedbackKind::Dislike, track }]);
        let playing = self.player.snapshot().track.is_some_and(|current| current.play_uri() == uri);
        if playing {
            self.player.next();
        }
    }

    pub fn is_hidden(&self, uri: &str) -> bool {
        lock(&self.state).hidden.contains(uri)
    }

    // --- плейлисты ---------------------------------------------------------------------------

    fn load_playlists(&self) {
        let generation = {
            let mut state = lock(&self.state);
            if state.playlists_loading || state.account.is_none() {
                return;
            }
            state.playlists_loading = true;
            state.rootlist_loading = true;
            state.progress.insert(Section::Playlists, (0, 0));
            state.generation
        };
        self.send([LibraryEvent::Playlists(ListChange::Loading(true))]);
        self.spawn_background(move |service| async move {
            let api = service.auth.api();
            // сначала rootlist сессии: шапки, папки и права одним чтением, без квоты Web API
            if let Some(library) = playlist_session::library(&api, &service.player, playlist_session::SESSION_WAIT).await {
                service.adopt_library(library, generation);
                return;
            }
            {
                let mut state = lock(&service.state);
                if state.generation != generation {
                    return;
                }
                state.rootlist_loading = false;
                state.session_list = false;
            }
            let mut raw: Vec<models::Playlist> = Vec::new();
            let mut offset = 0;
            let mut failure = None;
            for page_index in 0..MAX_SECTION_PAGES {
                let result = async { service.client(Operation::PlaylistLibrary).await?.my_playlists(offset, 50).await }.await;
                let page = match result {
                    Ok(page) => page,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                api.observe_playlists(page.items.iter());
                let next = page.next_offset();
                let total_pages = page.total.div_ceil(50).max(1);
                raw.extend(page.items);
                let events = {
                    let mut state = lock(&service.state);
                    if state.generation != generation || state.session_list {
                        return;
                    }
                    if offset == 0 || raw.len() >= state.playlists_raw.len() {
                        state.playlists_raw = raw.clone();
                    }
                    state.progress.insert(Section::Playlists, (page_index + 1, total_pages));
                    let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
                    events.push(LibraryEvent::EntriesChanged);
                    events.push(LibraryEvent::SavedChanged);
                    events
                };
                service.send(events);
                match next {
                    Some(next) if next > offset => offset = next,
                    _ => break,
                }
            }
            let events = {
                let mut state = lock(&service.state);
                if state.generation != generation || state.session_list {
                    return;
                }
                state.playlists_loading = false;
                state.progress.remove(&Section::Playlists);
                if failure.is_none() {
                    state.playlists_raw = raw;
                }
                let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
                events.push(LibraryEvent::Playlists(ListChange::Loading(false)));
                events.push(LibraryEvent::EntriesChanged);
                events.push(LibraryEvent::SavedChanged);
                events
            };
            service.send(events);
            if let Some(error) = failure {
                let text = service.error_text_of(&error);
                let event = lock(&service.state).set_error(mf_i18n::tf("Couldn’t load playlists: {0}", &[&text]));
                service.send(event);
                service.retry_later(generation);
            }
        });
    }

    /// Список плейлистов, дерево папок и права (приглашения) — из rootlist сессии librespot (сессия
    /// подключилась, когда список уже взял Web API).
    fn load_rootlist(&self) {
        let generation = {
            let mut state = lock(&self.state);
            if state.rootlist_loading || state.account.is_none() || self.player.session().is_none() {
                return;
            }
            state.rootlist_loading = true;
            state.generation
        };
        self.spawn(move |service| async move {
            let api = service.auth.api();
            match playlist_session::library(&api, &service.player, std::time::Duration::ZERO).await {
                Some(library) => service.adopt_library(library, generation),
                None => {
                    let mut state = lock(&service.state);
                    if state.generation == generation {
                        state.rootlist_loading = false;
                    }
                }
            }
        });
    }

    /// Принять список из rootlist: чего у сессии нет (имя владельца, коллаж), берётся из прежних шапок;
    /// затем коллажи для плейлистов без своей обложки.
    fn adopt_library(&self, library: playlist_session::Library, generation: u64) {
        let (events, coverless, cached) = {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return;
            }
            let mut raw = library.playlists;
            for playlist in &mut raw {
                if let Some(listed) = state.raw(&playlist.id) {
                    playlist.fill_from(listed);
                }
            }
            self.auth.api().observe_playlists(raw.iter());
            let coverless: Vec<String> =
                raw.iter().filter(|pl| pl.images.is_empty() && pl.track_total() > 0).map(|pl| pl.id.clone()).collect();
            state.playlists_raw = raw;
            // дерево уходит на диск: когда сессия в следующий раз будет подниматься, библиотека уже видна
            let cached = library_cache::CachedLibrary::new(
                library.rootlist.entries.clone(),
                state.playlists_raw.clone(),
            );
            state.rootlist = Some(library.rootlist);
            state.rootlist_loading = false;
            state.session_list = true;
            let was_loading = std::mem::take(&mut state.playlists_loading);
            state.progress.remove(&Section::Playlists);
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            if was_loading {
                events.push(LibraryEvent::Playlists(ListChange::Loading(false)));
            }
            events.extend([LibraryEvent::EntriesChanged, LibraryEvent::SavedChanged]);
            let ids: Vec<PlaylistId> = state.pages.keys().cloned().collect();
            events.extend(ids.into_iter().map(LibraryEvent::PlaylistInfo));
            (events, coverless, cached)
        };
        self.send(events);
        if !coverless.is_empty() {
            self.spawn(move |service| async move { service.fill_mosaics(coverless, generation).await });
        }
        self.spawn(move |service| async move {
            let Some(account) = lock(&service.state).account.clone() else { return };
            let file = library_cache::path(&mf_platform::dirs::playlist_cache_dir(&account));
            let _ = tokio::task::spawn_blocking(move || library_cache::write(&file, &cached)).await;
        });
    }

    /// Пока сессия поднимается (это может занять десятки секунд), показать дерево плейлистов с диска.
    /// Свежий rootlist из сессии придёт позже и заменит его.
    fn hydrate_from_cache(&self) {
        let (account, generation) = {
            let state = lock(&self.state);
            let Some(account) = state.account.clone() else { return };
            if state.rootlist.is_some() {
                return;
            }
            (account, state.generation)
        };
        let file = library_cache::path(&mf_platform::dirs::playlist_cache_dir(&account));
        self.spawn(move |service| async move {
            let cached = tokio::task::spawn_blocking(move || library_cache::read(&file)).await.ok().flatten();
            let Some(cached) = cached else { return };
            service.adopt_cached(cached, generation);
        });
    }

    /// Принять кэш как список из сессии: те же события, дерево без прав на добавление (их даёт сессия).
    fn adopt_cached(&self, cached: library_cache::CachedLibrary, generation: u64) {
        let events = {
            let mut state = lock(&self.state);
            // сессия успела ответить первой — её список свежее кэша
            if state.generation != generation || state.rootlist.is_some() {
                return;
            }
            let playlists = cached.playlists;
            self.auth.api().observe_playlists(playlists.iter());
            state.playlists_raw = playlists;
            state.rootlist = Some(Rootlist { entries: cached.rootlist, ..Default::default() });
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::EntriesChanged);
            events
        };
        self.send(events);
    }

    /// Коллажи обложек (как у Web API) для плейлистов без своей обложки — через сессию.
    async fn fill_mosaics(&self, ids: Vec<String>, generation: u64) {
        use futures::StreamExt as _;
        let api = self.auth.api();
        let mut covers = futures::stream::iter(ids)
            .map(|id| {
                let (api, player) = (&api, &self.player);
                async move { (playlist_session::mosaic(api, player, &id).await, id) }
            })
            .buffer_unordered(4);
        while let Some((images, id)) = covers.next().await {
            if images.is_empty() {
                continue;
            }
            let events = {
                let mut state = lock(&self.state);
                if state.generation != generation {
                    return;
                }
                let generated = state.mosaics.contains(&id);
                if let Some(listed) =
                    state.playlists_raw.iter_mut().find(|pl| pl.id == id && (pl.images.is_empty() || generated || spotify_mosaic(&pl.images)))
                {
                    listed.images = images.clone();
                }
                state.mosaics.insert(id.clone());
                let id = PlaylistId::new(id);
                if let Some(header) = state.pages.get_mut(&id).and_then(|page| page.header.as_mut())
                    && (header.images.is_empty() || generated)
                {
                    header.images = images;
                }
                let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
                events.push(LibraryEvent::EntriesChanged);
                events.push(LibraryEvent::PlaylistInfo(id));
                events
            };
            self.send(events);
        }
    }

    /// Плейлист без своей обложки: коллаж из обложек первых строк `rows` (как у Web API) — и заново
    /// после правок, когда первые строки сменились. Своя обложка (картинка в шапке) и только что
    /// загруженная не трогаются.
    fn refresh_mosaic(&self, id: &PlaylistId, rows: &[TrackItem]) {
        let events = {
            let mut state = lock(&self.state);
            if state.uploaded_covers.contains_key(id.as_str()) {
                return;
            }
            let Some(header) = state.pages.get(id).and_then(|page| page.header.as_ref()) else { return };
            let generated = state.mosaics.contains(id.as_str());
            if !header.images.is_empty() && !generated && !spotify_mosaic(&header.images) {
                return;
            }
            let images = sf_player::engine::session_playlists::mosaic_from(
                rows.iter().map(|row| if row.cover_large.is_empty() { row.cover.as_str() } else { row.cover_large.as_str() }),
            );
            let listed = state.raw(id.as_str()).map(|listed| listed.images.clone());
            let changed = match &listed {
                Some(current) if current.is_empty() || generated || spotify_mosaic(current) => *current != images,
                Some(_) => return,
                None => header.images != images,
            };
            if !changed {
                return;
            }
            if let Some(listed) = state.playlists_raw.iter_mut().find(|pl| pl.id == id.as_str()) {
                listed.images = images.clone();
            }
            if let Some(header) = state.pages.get_mut(id).and_then(|page| page.header.as_mut()) {
                header.images = images;
            }
            state.mosaics.insert(id.as_str().to_owned());
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::EntriesChanged);
            events.push(LibraryEvent::PlaylistInfo(id.clone()));
            events
        };
        self.send(events);
    }

    pub fn playlists(&self) -> Vec<PlaylistItem> {
        lock(&self.state).playlists.clone()
    }
    pub fn playlists_loading(&self) -> bool {
        lock(&self.state).playlists_loading
    }

    /// Открыть плейлист (сессия librespot для чужих, Web API для своих) → PlaylistTracks.
    /// Без дублей: чтение уже идёт — не перезапускается; кэш уже показан — ни Reset, ни сети.
    pub fn load_playlist(&self, id: PlaylistId) {
        self.open_playlist(id, false);
    }

    /// Явное обновление (кнопка «Обновить», перечитывание после правок): идущее чтение отменяется.
    pub fn refresh_playlist(&self, id: PlaylistId) {
        self.open_playlist(id, true);
    }

    fn open_playlist(&self, id: PlaylistId, force: bool) {
        let (generation, page_generation, cached_needed, account, action) = {
            let mut state = lock(&self.state);
            let Some(account) = state.account.clone() else { return };
            let generation = state.generation;
            let page = state.pages.entry(id.clone()).or_default();
            if page.pending_writes > 0 {
                page.reload_after_write = true;
                return;
            }
            let action = page.open_action(force);
            if action == OpenAction::Fetch {
                if let Some(read) = page.read.take() {
                    read.abort();
                }
                page.generation += 1;
                page.loading = true;
            }
            (generation, page.generation, page.rows.is_empty(), account, action)
        };
        // новому экрану (и модели, созданной позже чтения) нужно видеть, что чтение идёт; при Idle
        // чтения нет — погасить зависший loading, если он остался от пропущенных событий
        let loading = action != OpenAction::Idle;
        self.send([LibraryEvent::PlaylistTracks { id: id.clone(), change: ListChange::Loading(loading) }]);
        if action == OpenAction::Loading {
            return;
        }
        let me = self.me.clone();
        let task_id = id.clone();
        let read = self.rt.spawn(async move {
            if let Some(service) = me.upgrade() {
                service.fetch_playlist(task_id, generation, page_generation, cached_needed, account).await;
            }
        });
        // чтение запоминается под своей генерацией: заменить его может только более новое
        let mut state = lock(&self.state);
        if let Some(page) = state.pages.get_mut(&id)
            && page.generation == page_generation
        {
            page.read = Some(read.abort_handle());
        } else {
            drop(state);
            read.abort();
        }
    }

    /// Экран закрылся: отменить чтение, которое больше никому не нужно (записи и воспроизведение не трогает).
    pub fn cancel_playlist(&self, id: &PlaylistId) {
        let mut state = lock(&self.state);
        let Some(page) = state.pages.get_mut(id) else { return };
        if page.pending_writes > 0 {
            return;
        }
        if !page.loading {
            page.read = None;
            return;
        }
        if let Some(read) = page.read.take() {
            read.abort();
        }
        page.generation += 1;
        page.loading = false;
        // loading при complete=true — это явное обновление (force) или перечитывание после правки;
        // отмена не должна прятать намерение: иначе следующее открытие покажет старый кэш как Idle
        page.complete = false;
        let event = LibraryEvent::PlaylistTracks { id: id.clone(), change: ListChange::Loading(false) };
        drop(state);
        self.send([event]);
    }

    /// Модель моста может не видеть готовый кэш сервиса (новый мост, пропущенные события): перезалить
    /// его кусками. События уходят в очередь под блокировкой состояния — чтение, начавшееся параллельно,
    /// допишет свои куски после, устаревший Reset не догонит свежие строки.
    pub fn replay_playlist(&self, id: &PlaylistId) {
        let state = lock(&self.state);
        let Some(page) = state.pages.get(id) else { return };
        if page.rows.is_empty() {
            return;
        }
        let events = cached_events(id, &page.rows);
        self.send(events);
    }

    async fn fetch_playlist(
        self: Arc<Self>,
        id: PlaylistId,
        generation: u64,
        page_generation: u64,
        cached_needed: bool,
        account: String,
    ) {
        let service = self;
        // быстрая проверка перед ожиданием сети; принятие шапок/страниц/финала сверяется ещё раз под тем же блоком
        let still = |service: &Self| {
            let state = lock(&service.state);
            read_is_current(&state, &id, generation, page_generation)
        };
        let cache_file = playlist_cache::path(&mf_platform::dirs::playlist_cache_dir(&account), id.as_str());
        if cached_needed {
            let file = cache_file.clone();
            let cached = tokio::task::spawn_blocking(move || playlist_cache::read(&file)).await.ok().flatten();
            if let Some(cached) = cached {
                if !still(&service) {
                    return;
                }
                known::remember(cached.items.iter());
                // кэш уходит кусками (первый Reset, дальше Append), а не одним гигантским Reset
                {
                    let mut state = lock(&service.state);
                    if !read_is_current(&state, &id, generation, page_generation) {
                        return;
                    }
                    let page = state.pages.entry(id.clone()).or_default();
                    page.rows = cached.items.clone();
                    page.snapshot = cached.snapshot;
                    let events = cached_events(&id, &cached.items);
                    // под той же блокировкой: replay шлёт синхронно под lock — окно между
                    // unlock и send дало бы дубли поверх свежих строк
                    service.send(events);
                };
            }
        }
        let api = service.auth.api();
        let finish = |service: &Self, error: Option<String>| {
            let mut events = Vec::new();
            {
                let mut state = lock(&service.state);
                if !read_is_current(&state, &id, generation, page_generation) {
                    return;
                }
                if let Some(page) = state.pages.get_mut(&id) {
                    page.loading = false;
                    page.read = None;
                }
            }
            events.push(LibraryEvent::PlaylistTracks { id: id.clone(), change: ListChange::Loading(false) });
            if let Some(error) = error {
                events.push(LibraryEvent::Notice(error));
            }
            service.send(events);
        };
        let mut rechecks = SNAPSHOT_RECHECKS.iter();
        let header = loop {
            let header = match route::playlist(&api, &service.player, id.as_str()).await {
                Ok(header) => header,
                Err(error) => {
                    service.auth.note_api_error(&error);
                    if still(&service) {
                        finish(&service, Some(route::playlist_error_text(&error)));
                    }
                    return;
                }
            };
            if !still(&service) {
                return;
            }
            // ответ со snapshot до нашей записи не должен вернуть строки, которые запись уже изменила
            let confirmed = {
                let mut state = lock(&service.state);
                if !read_is_current(&state, &id, generation, page_generation) {
                    return;
                }
                let page = state.pages.entry(id.clone()).or_default();
                match &page.expected_snapshot {
                    Some(expected) if header.snapshot_id.as_deref() != Some(expected.as_str()) => false,
                    _ => {
                        page.expected_snapshot = None;
                        true
                    }
                }
            };
            if confirmed {
                break header;
            }
            let Some(delay) = rechecks.next() else {
                if let Some(page) = lock(&service.state).pages.get_mut(&id) {
                    page.expected_snapshot = None;
                }
                finish(&service, Some(mf_i18n::t("Spotify hasn’t confirmed the playlist changes yet — refresh a bit later")));
                return;
            };
            tokio::time::sleep(*delay).await;
            if !still(&service) {
                return;
            }
        };
        let snapshot = header.snapshot_id.clone().unwrap_or_default();
        let total = header.track_total();
        let collaborative = header.collaborative;
        let up_to_date = {
            let mut state = lock(&service.state);
            if !read_is_current(&state, &id, generation, page_generation) {
                return;
            }
            let page = state.pages.entry(id.clone()).or_default();
            page.header = Some(header);
            !snapshot.is_empty() && page.snapshot == snapshot && page.rows.len() == total as usize
        };
        service.send([LibraryEvent::PlaylistInfo(id.clone())]);
        if up_to_date {
            let rows = {
                let mut state = lock(&service.state);
                let page = state.pages.entry(id.clone()).or_default();
                page.complete = true;
                page.rows.clone()
            };
            service.send([LibraryEvent::PlaylistInfo(id.clone())]);
            service.refresh_mosaic(&id, &rows);
            finish(&service, None);
            return;
        }
        let progressive = lock(&service.state).pages.get(&id).is_none_or(|page| page.rows.is_empty());
        let mut rows: Vec<TrackItem> = Vec::new();
        let mut offset = 0;
        for _ in 0..MAX_PLAYLIST_PAGES {
            let page = match route::playlist_items(&api, &service.player, id.as_str(), offset).await {
                Ok(page) => page,
                Err(error) => {
                    service.auth.note_api_error(&error);
                    if still(&service) {
                        finish(&service, Some(route::playlist_error_text(&error)));
                    }
                    return;
                }
            };
            if !still(&service) {
                return;
            }
            let next = page.next_offset();
            let added: Vec<TrackItem> = page.items.iter().filter_map(|item| playlist_row(item, collaborative)).collect();
            known::remember(added.iter());
            if progressive {
                let change = if rows.is_empty() { ListChange::Reset(added.clone()) } else { ListChange::Append(added.clone()) };
                {
                    let mut state = lock(&service.state);
                    if !read_is_current(&state, &id, generation, page_generation) {
                        return;
                    }
                    let page = state.pages.entry(id.clone()).or_default();
                    if offset == 0 {
                        page.rows.clear();
                    }
                    page.rows.extend(added.iter().cloned());
                    service.send([LibraryEvent::PlaylistTracks { id: id.clone(), change }]);
                }
            }
            rows.extend(added);
            match next {
                Some(next) if next > offset => offset = next,
                _ => break,
            }
        }
        {
            let mut state = lock(&service.state);
            if !read_is_current(&state, &id, generation, page_generation) {
                return;
            }
            let page = state.pages.entry(id.clone()).or_default();
            let change = list_change(&page.rows, &rows);
            page.rows = rows.clone();
            page.snapshot = snapshot.clone();
            page.complete = true;
            page.read = None;
            if let Some(change) = change {
                service.send([LibraryEvent::PlaylistTracks { id: id.clone(), change }]);
            }
        };
        service.send([LibraryEvent::PlaylistInfo(id.clone())]);
        service.refresh_mosaic(&id, &rows);
        finish(&service, None);
        if !snapshot.is_empty() {
            let cache = playlist_cache::CachedPlaylist::new(snapshot, rows);
            tokio::task::spawn_blocking(move || playlist_cache::write(&cache_file, &cache));
        }
    }

    pub fn playlist_tracks(&self, id: &PlaylistId) -> Option<Vec<TrackItem>> {
        lock(&self.state).pages.get(id).map(|page| page.rows.clone())
    }
    pub fn playlist_info(&self, id: &PlaylistId) -> Option<PlaylistInfo> {
        lock(&self.state).playlist_info(id.as_str())
    }
    /// Свой, совместный или по приглашению (rootlist).
    pub fn can_edit(&self, id: &PlaylistId) -> bool {
        let state = lock(&self.state);
        state.header(id.as_str()).is_some_and(|header| state.can_edit_raw(&header))
    }

    fn playlist_title(&self, id: &PlaylistId) -> String {
        lock(&self.state).header(id.as_str()).map(|header| header.name).unwrap_or_default()
    }

    /// Строки для оптимистичной вставки: известные или дочитанные из каталога.
    async fn resolve_rows(&self, uris: &[String], hint: Option<&TrackItem>) -> Vec<TrackItem> {
        let mut rows = Vec::with_capacity(uris.len());
        let unknown: Vec<String> = uris
            .iter()
            .filter(|uri| hint.is_none_or(|hint| hint.play_uri() != **uri) && known::get(uri).is_none())
            .cloned()
            .collect();
        let mut from_session = self.session_rows(&unknown).await;
        let mut client = None;
        for uri in uris {
            if let Some(hint) = hint.filter(|hint| hint.play_uri() == *uri) {
                rows.push(hint.clone());
                continue;
            }
            if let Some(row) = known::get(uri) {
                rows.push(row);
                continue;
            }
            if let Some(row) = from_session.remove(uri) {
                known::remember([&row]);
                rows.push(row);
                continue;
            }
            if client.is_none() {
                client = self.client(Operation::Catalog).await.ok();
            }
            let id = sf_api::util::uri_id(uri).unwrap_or_default();
            let fetched = match (&client, uri_kind(uri)) {
                (Some(client), "track") => client.track(id).await.ok().map(|track| map::track_item(&track)),
                (Some(client), "episode") => {
                    client.episode(id).await.ok().map(|episode| map::episode_item(&episode, None).to_track())
                }
                _ => None,
            };
            let row = fetched.unwrap_or_else(|| TrackItem { uri: uri.clone(), available: true, ..Default::default() });
            known::remember([&row]);
            rows.push(row);
        }
        rows
    }

    /// Треки и выпуски по URI одной-двумя пачками метаданных сессии (без квоты Web API); чего нет — пусто.
    async fn session_rows(&self, uris: &[String]) -> HashMap<String, TrackItem> {
        let mut rows = HashMap::new();
        if uris.is_empty() {
            return rows;
        }
        let Some(session) = route::account_session(&self.auth.api(), &self.player) else { return rows };
        let tracks: Vec<String> = uris.iter().filter(|uri| uri_kind(uri) == "track").cloned().collect();
        let episodes: Vec<String> = uris.iter().filter(|uri| uri_kind(uri) == "episode").cloned().collect();
        let read_tracks = async {
            if tracks.is_empty() {
                return None;
            }
            sf_player::engine::session_collection::tracks(&session, &tracks).await.ok()
        };
        let read_episodes = async {
            if episodes.is_empty() {
                return None;
            }
            sf_player::engine::session_podcasts::episodes(&session, &episodes).await.ok()
        };
        let read = async { tokio::join!(read_tracks, read_episodes) };
        let Ok((tracks, episodes)) = tokio::time::timeout(std::time::Duration::from_secs(15), read).await else { return rows };
        for (uri, track) in tracks.unwrap_or_default() {
            rows.insert(uri, map::track_item(&track));
        }
        for episode in episodes.unwrap_or_default() {
            rows.insert(episode.uri.clone(), map::episode_item(&episode, None).to_track());
        }
        rows
    }

    /// Начать запись: оптимистичные строки (если плейлист загружен) и счётчик записей; delta — изменение
    /// числа треков для незагруженного плейлиста.
    fn begin_write(&self, id: &PlaylistId, delta: i64, edit: impl FnOnce(&mut Vec<TrackItem>)) -> Vec<LibraryEvent> {
        let mut state = lock(&self.state);
        let mut events = Vec::new();
        let page = state.pages.entry(id.clone()).or_default();
        page.pending_writes += 1;
        if page.loading {
            // идущее чтение прервётся записью — перечитать после неё
            page.reload_after_write = true;
        }
        if page.complete || !page.rows.is_empty() {
            let mut rows = page.rows.clone();
            edit(&mut rows);
            if let Some(change) = list_change(&page.rows, &rows) {
                events.push(LibraryEvent::PlaylistTracks { id: id.clone(), change });
            }
            page.rows = rows;
        }
        let exact = page.complete.then_some(page.rows.len() as u32);
        let recount = |playlist: &mut models::Playlist| {
            let total = exact.unwrap_or_else(|| (i64::from(playlist.track_total()) + delta).max(0) as u32);
            playlist.tracks = Some(models::TrackCount { total });
            playlist.items_count = None;
        };
        if let Some(listed) = state.playlists_raw.iter_mut().find(|pl| pl.id == id.as_str()) {
            recount(listed);
        }
        if let Some(header) = state.pages.get_mut(id).and_then(|page| page.header.as_mut()) {
            recount(header);
        }
        events.extend(state.rebuild_playlists());
        events.push(LibraryEvent::PlaylistInfo(id.clone()));
        events.push(LibraryEvent::EntriesChanged);
        events
    }

    /// Закончить запись: новый snapshot или перечитать.
    fn end_write(&self, id: &PlaylistId, result: Result<Option<String>, ApiError>, success: Option<String>) {
        let mosaic: Option<Option<Vec<TrackItem>>>;
        let reload = {
            let mut state = lock(&self.state);
            let page = state.pages.entry(id.clone()).or_default();
            page.pending_writes = page.pending_writes.saturating_sub(1);
            match &result {
                Ok(snapshot) => {
                    if let Some(snapshot) = snapshot.clone().filter(|s| !s.is_empty()) {
                        page.expected_snapshot = Some(snapshot.clone());
                        page.snapshot = snapshot.clone();
                        if let Some(header) = &mut page.header {
                            header.snapshot_id = Some(snapshot);
                        }
                    }
                }
                Err(_) => {
                    page.reload_after_write = true;
                    page.expected_snapshot = None;
                }
            }
            let reload = page.pending_writes == 0 && std::mem::take(&mut page.reload_after_write);
            // коллаж из первых строк — по строкам после правки (известны все) или заново через сессию
            mosaic = (result.is_ok() && page.pending_writes == 0 && !reload).then(|| page.complete.then(|| page.rows.clone()));
            if page.pending_writes == 0 && !reload && page.complete && !page.snapshot.is_empty() {
                let account = state.account.clone().unwrap_or_default();
                let page = &state.pages[id];
                let cache = playlist_cache::CachedPlaylist::new(page.snapshot.clone(), page.rows.clone());
                let file = playlist_cache::path(&mf_platform::dirs::playlist_cache_dir(&account), id.as_str());
                self.rt.spawn_blocking(move || playlist_cache::write(&file, &cache));
            }
            reload
        };
        match result {
            Ok(_) => {
                if let Some(text) = success {
                    self.notice(text);
                }
            }
            Err(error) => {
                let text = self.error_text_of(&error);
                self.notice(mf_i18n::tf("Playlist didn’t change: {0}", &[&text]));
            }
        }
        match mosaic {
            Some(Some(rows)) => self.refresh_mosaic(id, &rows),
            Some(None) => {
                let (wanted, generation) = {
                    let state = lock(&self.state);
                    let wanted = !state.uploaded_covers.contains_key(id.as_str())
                        && state
                            .raw(id.as_str())
                            .is_some_and(|listed| {
                                listed.images.is_empty() || state.mosaics.contains(id.as_str()) || spotify_mosaic(&listed.images)
                            });
                    (wanted, state.generation)
                };
                if wanted {
                    let ids = vec![id.as_str().to_owned()];
                    self.spawn(move |service| async move { service.fill_mosaics(ids, generation).await });
                }
            }
            None => {}
        }
        if reload {
            // после записи: кэш уже не «актуален» — перечитать принудительно
            self.refresh_playlist(id.clone());
        }
    }

    fn add_rows(
        &self,
        id: PlaylistId,
        uris: Vec<String>,
        position: Option<usize>,
        hint: Option<TrackItem>,
        unique: bool,
    ) {
        if uris.is_empty() {
            return;
        }
        let generation = self.generation();
        self.spawn(move |service| async move {
            let title = service.playlist_title(&id);
            if unique {
                let (complete, present) = {
                    let state = lock(&service.state);
                    let page = state.pages.get(&id);
                    (
                        page.is_some_and(|page| page.complete),
                        page.is_some_and(|page| page.rows.iter().any(|row| row.play_uri() == uris[0])),
                    )
                };
                let duplicate = if present {
                    true
                } else if complete {
                    false
                } else {
                    let api = service.auth.api();
                    match playlist_session::duplicates(&api, &service.player, id.as_str(), &uris).await {
                        Some(found) => found.is_ok_and(|found| !found.is_empty()),
                        None => {
                            let access = api.playlist_access(id.as_str());
                            match api.client_for(Operation::PlaylistItems(access)).await {
                                Ok(client) => {
                                    client.playlist_duplicates(id.as_str(), &uris).await.is_ok_and(|found| !found.is_empty())
                                }
                                Err(_) => false,
                            }
                        }
                    }
                };
                if duplicate {
                    service.notice(if title.is_empty() {
                        mf_i18n::t("This track is already in the playlist")
                    } else {
                        mf_i18n::tf("This track is already in “{0}”", &[&title])
                    });
                    return;
                }
            }
            let rows = service.resolve_rows(&uris, hint.as_ref()).await;
            if !service.current(generation) {
                return;
            }
            let count = rows.len();
            let events = service.begin_write(&id, count as i64, |list| match position {
                Some(at) => {
                    let at = at.min(list.len());
                    list.splice(at..at, rows);
                }
                None => list.extend(rows),
            });
            service.send(events);
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let add = Change::Add { uris: uris.clone(), at: position.map(|at| at as u32) };
            let result = match playlist_session::change(&api, &service.player, id.as_str(), add).await {
                Some(result) => result,
                None => {
                    async {
                        let client = api.client_for(Operation::PlaylistMutation(access)).await?;
                        let mut snapshot = None;
                        for (index, chunk) in uris.chunks(WRITE_CHUNK).enumerate() {
                            let at = position.map(|at| (at + index * WRITE_CHUNK) as u32);
                            snapshot = client.add_playlist_items(id.as_str(), chunk, at).await?.or(snapshot);
                        }
                        Ok(snapshot)
                    }
                    .await
                }
            };
            if !service.current(generation) {
                return;
            }
            let success = match (count, title.is_empty()) {
                (1, false) => mf_i18n::tf("Added to “{0}”", &[&title]),
                (1, true) => mf_i18n::t("Added to playlist"),
                (n, false) => mf_i18n::tf("{0} added to “{1}”", &[&mf_i18n::n(n as i64, "{0} track", "{0} tracks"), &title]),
                (n, true) => mf_i18n::tf("{0} added to playlist", &[&mf_i18n::n(n as i64, "{0} track", "{0} tracks")]),
            };
            service.end_write(&id, result, Some(success));
        });
    }

    pub fn add_to_playlist(&self, track: TrackItem, id: PlaylistId) {
        known::remember([&track]);
        let uri = track.play_uri();
        self.add_rows(id, vec![uri], None, Some(track), true);
    }
    /// Добавить в конец (AddTracksPanel); дубли разрешены.
    pub fn add_tracks(&self, id: PlaylistId, uris: Vec<String>) {
        self.add_rows(id, uris, None, None, false);
    }
    /// Вставить на позицию (Web API `position`; «Вернуть» после удаления строк).
    pub fn insert_tracks(&self, id: PlaylistId, uris: Vec<String>, position: usize) {
        self.add_rows(id, uris, Some(position), None, false);
    }

    /// (uri, позиция) — по snapshot_id. Web API снимает все вхождения URI — строки снимаются так же.
    pub fn remove_tracks(&self, id: PlaylistId, rows: Vec<(String, usize)>) {
        let mut uris: Vec<String> = Vec::new();
        for (uri, _) in rows {
            if !uri.is_empty() && !uris.contains(&uri) {
                uris.push(uri);
            }
        }
        if uris.is_empty() {
            return;
        }
        let delta = -(uris.len() as i64);
        let events = self.begin_write(&id, delta, |list| list.retain(|row| !uris.contains(&row.play_uri())));
        self.send(events);
        let generation = self.generation();
        self.spawn(move |service| async move {
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let mut snapshot = {
                let state = lock(&service.state);
                state.pages.get(&id).map(|page| page.snapshot.clone()).filter(|s| !s.is_empty())
            };
            let remove = Change::Remove { uris: uris.clone() };
            let result = match playlist_session::change(&api, &service.player, id.as_str(), remove).await {
                Some(result) => result,
                None => {
                    async {
                        let client = api.client_for(Operation::PlaylistMutation(access)).await?;
                        for chunk in uris.chunks(WRITE_CHUNK) {
                            snapshot =
                                client.remove_playlist_items(id.as_str(), chunk, snapshot.as_deref()).await?.or(snapshot);
                        }
                        Ok(snapshot)
                    }
                    .await
                }
            };
            if service.current(generation) {
                service.end_write(&id, result, None);
            }
        });
    }

    pub fn move_tracks(&self, id: PlaylistId, from: usize, to: usize) {
        let len = lock(&self.state).pages.get(&id).map_or(0, |page| page.rows.len());
        if from == to || from >= len || to >= len {
            return;
        }
        let events = self.begin_write(&id, 0, |list| {
            let row = list.remove(from);
            list.insert(to, row);
        });
        self.send(events);
        let generation = self.generation();
        let insert_before = if to > from { to + 1 } else { to } as u32;
        self.spawn(move |service| async move {
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let (snapshot, alone) = {
                let state = lock(&service.state);
                let page = state.pages.get(&id);
                (
                    page.map(|page| page.snapshot.clone()).filter(|s| !s.is_empty()),
                    page.is_some_and(|page| page.pending_writes == 1),
                )
            };
            // позиции — в строках, которые видел пользователь: их ревизия — база (сервер перебазирует),
            // если до переноса нет других незаконченных записей
            let r#move = Change::Move { from: from as u32, insert_before, snapshot: snapshot.clone().filter(|_| alone) };
            let result = match playlist_session::change(&api, &service.player, id.as_str(), r#move).await {
                Some(result) => result,
                None => {
                    async {
                        api.client_for(Operation::PlaylistMutation(access))
                            .await?
                            .reorder_playlist(id.as_str(), from as u32, insert_before, snapshot.as_deref())
                            .await
                    }
                    .await
                }
            };
            if service.current(generation) {
                service.end_write(&id, result, None);
            }
        });
    }

    /// Новый плейлист в начале библиотеки.
    async fn create(&self, title: &str) -> Result<models::Playlist, ApiError> {
        let api = self.auth.api();
        let playlist = match playlist_session::create(&api, &self.player, title, false).await {
            Some(created) => {
                let mut playlist = created?;
                playlist.owner.display_name =
                    self.auth.account().map(|account| account.display_name).filter(|name| !name.is_empty());
                playlist
            }
            None => self.client(Operation::PlaylistCreation).await?.create_playlist(title, false, "").await?,
        };
        api.observe_playlist(&playlist);
        Ok(playlist)
    }

    fn adopt_created(&self, playlist: &models::Playlist, generation: u64) -> bool {
        let events = {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return false;
            }
            state.playlists_raw.insert(0, playlist.clone());
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::EntriesChanged);
            events.push(LibraryEvent::SavedChanged);
            events
        };
        self.send(events);
        true
    }

    pub fn create_playlist(&self, title: String, track: Option<TrackItem>) {
        let title = if title.trim().is_empty() { mf_i18n::t("New playlist") } else { title.trim().to_owned() };
        let generation = self.generation();
        self.spawn(move |service| async move {
            match service.create(&title).await {
                Ok(playlist) => {
                    if !service.adopt_created(&playlist, generation) {
                        return;
                    }
                    let id = PlaylistId::new(playlist.id.clone());
                    {
                        let mut state = lock(&service.state);
                        let page = state.pages.entry(id.clone()).or_default();
                        page.header = Some(playlist.clone());
                        page.complete = true;
                        page.snapshot = playlist.snapshot_id.clone().unwrap_or_default();
                    }
                    service.notice(mf_i18n::tf("Playlist “{0}” created", &[&title]));
                    if let Some(track) = track {
                        let uri = track.play_uri();
                        service.add_rows(id, vec![uri], None, Some(track), false);
                    }
                }
                Err(error) => {
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Couldn’t create playlist: {0}", &[&text]));
                }
            }
        });
    }

    /// Создать из списка URI (очередь, радио).
    pub fn create_playlist_from(&self, title: String, uris: Vec<String>) {
        let title = if title.trim().is_empty() { mf_i18n::t("New playlist") } else { title.trim().to_owned() };
        let generation = self.generation();
        self.spawn(move |service| async move {
            let result = async {
                let mut playlist = service.create(&title).await?;
                let api = service.auth.api();
                let add = Change::Add { uris: uris.clone(), at: None };
                match playlist_session::change(&api, &service.player, &playlist.id, add).await {
                    Some(result) => playlist.snapshot_id = result?.or(playlist.snapshot_id),
                    None => {
                        let client = service.client(Operation::PlaylistMutation(sf_api::PlaylistAccess::Owned)).await?;
                        for chunk in uris.chunks(WRITE_CHUNK) {
                            if let Some(snapshot) = client.add_playlist_items(&playlist.id, chunk, None).await? {
                                playlist.snapshot_id = Some(snapshot);
                            }
                        }
                    }
                }
                playlist.tracks = Some(models::TrackCount { total: uris.len() as u32 });
                Ok::<_, ApiError>(playlist)
            }
            .await;
            match result {
                Ok(playlist) => {
                    if service.adopt_created(&playlist, generation) {
                        service.notice(mf_i18n::tf("Playlist “{0}” created · {1}", &[&title, &mf_i18n::n(uris.len() as i64, "{0} track", "{0} tracks")]));
                    }
                }
                Err(error) => {
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Couldn’t create playlist: {0}", &[&text]));
                }
            }
        });
    }

    /// Поменять шапку оптимистично; ошибка — вернуть как было.
    fn edit_details(&self, id: PlaylistId, name: Option<String>, description: Option<String>, public: Option<bool>) {
        let change = |playlist: &mut models::Playlist| {
            if let Some(name) = &name {
                playlist.name = name.clone();
            }
            if let Some(description) = &description {
                playlist.description = Some(description.clone());
            }
            if let Some(public) = public {
                playlist.public = Some(public);
            }
        };
        let (before, events) = {
            let mut state = lock(&self.state);
            let before = (
                state.raw(id.as_str()).cloned(),
                state.pages.get(&id).and_then(|page| page.header.clone()),
            );
            if let Some(listed) = state.playlists_raw.iter_mut().find(|pl| pl.id == id.as_str()) {
                change(listed);
            }
            if let Some(header) = state.pages.get_mut(&id).and_then(|page| page.header.as_mut()) {
                change(header);
            }
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::PlaylistInfo(id.clone()));
            events.push(LibraryEvent::EntriesChanged);
            (before, events)
        };
        self.send(events);
        let generation = self.generation();
        self.spawn(move |service| async move {
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            // название/описание — шапка плейлиста, видимость — строка rootlist; любая часть не вышла через
            // сессию — всё через Web API (запись повторяемая)
            let in_session = async {
                if name.is_some() || description.is_some() {
                    let details = Change::Details { name: name.clone(), description: description.clone(), collaborative: None };
                    if let Err(error) = playlist_session::change(&api, &service.player, id.as_str(), details).await? {
                        return Some(Err(error));
                    }
                }
                match public {
                    Some(public) => playlist_session::set_public(&api, &service.player, id.as_str(), public).await,
                    None => Some(Ok(())),
                }
            };
            let result = match in_session.await {
                Some(result) => result,
                None => {
                    async {
                        api.client_for(Operation::PlaylistMutation(access))
                            .await?
                            .update_playlist(id.as_str(), name.as_deref(), description.as_deref(), public)
                            .await
                    }
                    .await
                }
            };
            if !service.current(generation) {
                return;
            }
            if let Err(error) = result {
                let events = {
                    let mut state = lock(&service.state);
                    if let (Some(old), Some(listed)) =
                        (before.0, state.playlists_raw.iter_mut().find(|pl| pl.id == id.as_str()))
                    {
                        *listed = old;
                    }
                    if let (Some(old), Some(page)) = (before.1, state.pages.get_mut(&id)) {
                        page.header = Some(old);
                    }
                    let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
                    events.push(LibraryEvent::PlaylistInfo(id.clone()));
                    events.push(LibraryEvent::EntriesChanged);
                    events
                };
                service.send(events);
                let text = service.error_text_of(&error);
                service.notice(mf_i18n::tf("Playlist didn’t change: {0}", &[&text]));
            }
        });
    }

    pub fn rename_playlist(&self, id: PlaylistId, title: String) {
        let title = title.trim().to_owned();
        if title.is_empty() {
            self.notice(mf_i18n::t("The name can’t be empty"));
            return;
        }
        self.edit_details(id, Some(title), None, None);
    }
    pub fn set_description(&self, id: PlaylistId, text: String) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            self.notice(mf_i18n::t("Spotify doesn’t allow clearing the description completely"));
            return;
        }
        self.edit_details(id, None, Some(text), None);
    }
    pub fn set_public(&self, id: PlaylistId, public: bool) {
        self.edit_details(id, None, None, Some(public));
    }

    /// JPEG ≤ 256 КБ (sf_api::playlist_cover), путь из FileDialog QML. После загрузки обложка видна сразу
    /// (локальная копия); картинка Spotify принимается, только когда её пиксели совпали с выбранной
    /// (до 3 перепроверок, как в spotifast).
    pub fn upload_cover(&self, id: PlaylistId, path: std::path::PathBuf) {
        let path = match path.to_str().and_then(|text| text.strip_prefix("file:///").or(text.strip_prefix("file://"))) {
            Some(rest) if cfg!(windows) => std::path::PathBuf::from(rest),
            Some(rest) => std::path::PathBuf::from(format!("/{}", rest.trim_start_matches('/'))),
            None => path,
        };
        let generation = self.generation();
        self.spawn(move |service| async move {
            let cover = tokio::task::spawn_blocking(move || sf_api::playlist_cover::read(&path)).await;
            let cover = match cover {
                Ok(Ok(cover)) => cover,
                Ok(Err(text)) => return service.notice(text),
                Err(_) => return service.notice(mf_i18n::t("Couldn’t read the image — choose a JPEG or PNG")),
            };
            let previous_urls: Vec<String> = lock(&service.state)
                .header(id.as_str())
                .map(|header| header.images.iter().map(|image| image.url.clone()).collect())
                .unwrap_or_default();
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let result = match playlist_session::upload_cover(&api, &service.player, id.as_str(), cover.jpeg.to_vec()).await {
                Some(result) => result.map(|_| ()),
                None => {
                    async {
                        api.client_for(Operation::PlaylistMutation(access))
                            .await?
                            .upload_playlist_cover(id.as_str(), &cover.encoded)
                            .await
                    }
                    .await
                }
            };
            if !service.current(generation) {
                return;
            }
            if let Err(error) = result {
                service.auth.note_api_error(&error);
                let text = service.error_text_of(&error);
                service.notice(mf_i18n::tf("Couldn’t upload the cover: {0}", &[&text]));
                return;
            }
            let local_url = {
                let jpeg = cover.jpeg.clone();
                let name = cover.uri.trim_start_matches("bytes://playlist-cover-").to_owned();
                let file = mf_platform::dirs::cache_subdir("covers").join(format!("upload-{name}"));
                tokio::task::spawn_blocking(move || {
                    std::fs::create_dir_all(file.parent()?).ok()?;
                    std::fs::write(&file, &jpeg[..]).ok()?;
                    reqwest::Url::from_file_path(&file).ok().map(String::from)
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
            };
            let request = {
                let mut state = lock(&service.state);
                state.cover_requests += 1;
                let request = state.cover_requests;
                state.uploaded_covers.insert(id.as_str().to_owned(), UploadedCover { request, local_url, previous_urls });
                request
            };
            service.send_playlist_cover(&id, None);
            service.notice(mf_i18n::t("Playlist cover updated"));
            service.confirm_cover(id, generation, request, cover).await;
        });
    }

    /// Перечитывать шапку, пока Spotify не отдаст картинку с теми же пикселями (≤ 3 раз).
    async fn confirm_cover(&self, id: PlaylistId, generation: u64, request: u64, cover: sf_api::playlist_cover::Cover) {
        let api = self.auth.api();
        let pending = |service: &Self| {
            let state = lock(&service.state);
            state.generation == generation
                && state.uploaded_covers.get(id.as_str()).is_some_and(|upload| upload.request == request)
        };
        for delay in COVER_RECHECKS {
            tokio::time::sleep(delay).await;
            if !pending(self) {
                return;
            }
            let Ok(header) = route::playlist(&api, &self.player, id.as_str()).await else { continue };
            let previous = match lock(&self.state).uploaded_covers.get(id.as_str()) {
                Some(upload) if upload.request == request => upload.previous_urls.clone(),
                _ => return,
            };
            let fresh = !header.images.is_empty() && header.images.iter().all(|image| !previous.contains(&image.url));
            if !fresh {
                continue;
            }
            let url = header.images[0].url.clone();
            let matched = match fetch_bytes(&api.http(), &url).await {
                Ok(bytes) => {
                    let cover = cover.clone();
                    tokio::task::spawn_blocking(move || cover.matches_remote(&bytes))
                        .await
                        .unwrap_or_else(|_| Err("comparison failed".to_owned()))
                }
                Err(error) => Err(error),
            };
            if !pending(self) {
                return;
            }
            match matched {
                Ok(true) => {
                    lock(&self.state).uploaded_covers.remove(id.as_str());
                    self.send_playlist_cover(&id, Some(header));
                    return;
                }
                Ok(false) => {
                    if let Some(upload) = lock(&self.state).uploaded_covers.get_mut(id.as_str()) {
                        upload.previous_urls.extend(header.images.iter().map(|image| image.url.clone()));
                    }
                }
                // проверить не удалось — остаётся загруженная обложка
                Err(error) => {
                    tracing::debug!("playlist cover not verified: {error}");
                    return;
                }
            }
        }
    }

    /// Обложка в списке и шапке: подтверждённая шапка Spotify или локальная копия загрузки.
    fn send_playlist_cover(&self, id: &PlaylistId, header: Option<models::Playlist>) {
        let events = {
            let mut state = lock(&self.state);
            if let Some(header) = header {
                if let Some(listed) = state.playlists_raw.iter_mut().find(|pl| pl.id == id.as_str()) {
                    listed.images = header.images.clone();
                }
                state.pages.entry(id.clone()).or_default().header = Some(header);
            }
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::PlaylistInfo(id.clone()));
            events.push(LibraryEvent::EntriesChanged);
            events
        };
        self.send(events);
    }

    pub fn follow_playlist(&self, id: PlaylistId) {
        let uri = id.uri();
        {
            let mut state = lock(&self.state);
            state.saved_intents.insert(uri.clone(), true);
            state.saved_revision += 1;
        }
        self.send([LibraryEvent::SavedChanged, LibraryEvent::PlaylistInfo(id.clone())]);
        let generation = self.generation();
        self.spawn(move |service| async move {
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let result = match playlist_session::follow(&api, &service.player, id.as_str()).await {
                Some(result) => result,
                None => {
                    async { api.client_for(Operation::PlaylistMutation(access)).await?.follow_playlist(id.as_str()).await }
                        .await
                }
            };
            if !service.current(generation) {
                return;
            }
            let header = lock(&service.state).header(id.as_str());
            let events = {
                let mut state = lock(&service.state);
                state.saved_intents.remove(&uri);
                state.saved_revision += 1;
                let mut events = vec![LibraryEvent::SavedChanged, LibraryEvent::PlaylistInfo(id.clone())];
                if result.is_ok()
                    && let Some(header) = header
                    && state.raw(id.as_str()).is_none()
                {
                    state.playlists_raw.insert(0, header);
                    events.extend(state.rebuild_playlists());
                    events.push(LibraryEvent::EntriesChanged);
                }
                events
            };
            service.send(events);
            match result {
                Ok(()) => service.notice(mf_i18n::t("Playlist added to your library")),
                Err(error) => {
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Couldn’t follow: {0}", &[&text]));
                }
            }
        });
    }

    /// Отписаться (для своего — «удалить»).
    pub fn unfollow_playlist(&self, id: PlaylistId) {
        let uri = id.uri();
        let (removed, own, events) = {
            let mut state = lock(&self.state);
            let index = state.playlists_raw.iter().position(|pl| pl.id == id.as_str());
            let removed = index.map(|index| (index, state.playlists_raw.remove(index)));
            let own = removed.as_ref().is_some_and(|(_, pl)| state.owns(pl));
            state.saved_intents.insert(uri.clone(), false);
            state.saved_revision += 1;
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.extend([LibraryEvent::EntriesChanged, LibraryEvent::SavedChanged, LibraryEvent::PlaylistInfo(id.clone())]);
            (removed, own, events)
        };
        self.send(events);
        let pins_changed = {
            let mut state = lock(&self.state);
            let before = state.pins.len();
            state.pins.retain(|pin| *pin != uri);
            (state.pins.len() != before).then(|| {
                state.pins_revision += 1;
                state.pins.clone()
            })
        };
        if let Some(pins) = pins_changed {
            write_list(&self.settings, SettingKey::Pins, &pins);
            self.send([LibraryEvent::PinsChanged]);
        }
        let generation = self.generation();
        self.spawn(move |service| async move {
            let api = service.auth.api();
            let access = api.playlist_access(id.as_str());
            let result = match playlist_session::unfollow(&api, &service.player, id.as_str()).await {
                Some(result) => result,
                None => {
                    async { api.client_for(Operation::PlaylistMutation(access)).await?.unfollow_playlist(id.as_str()).await }
                        .await
                }
            };
            if !service.current(generation) {
                return;
            }
            let events = {
                let mut state = lock(&service.state);
                state.saved_intents.remove(&uri);
                state.saved_revision += 1;
                let mut events = vec![LibraryEvent::SavedChanged, LibraryEvent::PlaylistInfo(id.clone())];
                if result.is_err()
                    && let Some((index, playlist)) = removed
                    && state.raw(id.as_str()).is_none()
                {
                    let index = index.min(state.playlists_raw.len());
                    state.playlists_raw.insert(index, playlist);
                    events.extend(state.rebuild_playlists());
                    events.push(LibraryEvent::EntriesChanged);
                }
                events
            };
            service.send(events);
            match result {
                Ok(()) => service.notice(if own { mf_i18n::t("Playlist deleted") } else { mf_i18n::t("Playlist removed from your library") }),
                Err(error) => {
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Playlist is still in your library: {0}", &[&text]));
                }
            }
        });
    }

    // --- альбомы, исполнители, подкасты --------------------------------------------------

    fn load_section(&self, section: Section) {
        let generation = {
            let mut state = lock(&self.state);
            if state.account.is_none() || state.progress.contains_key(&section) {
                return;
            }
            state.progress.insert(section, (0, 0));
            state.generation
        };
        self.spawn_background(move |service| async move {
            let result = service.fetch_section(section, generation).await;
            let events = {
                let mut state = lock(&service.state);
                if state.generation != generation {
                    return;
                }
                state.progress.remove(&section);
                let mut events = vec![LibraryEvent::SavedChanged, LibraryEvent::EntriesChanged];
                if let Err(error) = &result {
                    drop(state);
                    let text = service.error_text_of(error);
                    let mut state = lock(&service.state);
                    events.extend(state.set_error(mf_i18n::tf("Your library didn’t fully load: {0}", &[&text])));
                }
                events
            };
            service.send(events);
            if result.is_err() {
                service.retry_later(generation);
            }
            // сессия встала, пока шёл Web API, — повторить через неё
            service.reload_with_session();
        });
    }

    /// Обновить раздел из готовых строк; событие для модели.
    fn put_section(
        &self,
        section: Section,
        generation: u64,
        (done, pages, last): (u32, u32, bool),
        apply: impl FnOnce(&mut State) -> Option<LibraryEvent>,
    ) -> bool {
        let events = {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return false;
            }
            state.progress.insert(section, (done, pages.max(done)));
            let mut events: Vec<LibraryEvent> = apply(&mut state).into_iter().collect();
            state.rebuild_saved(last);
            state.saved_revision += 1;
            events.push(LibraryEvent::SavedChanged);
            events.push(LibraryEvent::EntriesChanged);
            events
        };
        self.send(events);
        true
    }

    async fn fetch_section(&self, section: Section, generation: u64) -> Result<(), ApiError> {
        route::wait_account_session(&self.auth.api(), &self.player, route::STARTUP_SESSION_WAIT).await;
        if self.session_section(section, generation).await {
            return Ok(());
        }
        if route::account_session(&self.auth.api(), &self.player).is_none() {
            let mut state = lock(&self.state);
            if state.generation == generation {
                state.sections_without_session.insert(section);
            }
        }
        let client = self.client(Operation::UserData).await?;
        match section {
            Section::Albums => {
                let mut rows: Vec<(String, AlbumItem)> = Vec::new();
                let mut offset = 0;
                for done in 1..=MAX_SECTION_PAGES {
                    let page = client.saved_albums(offset, 50).await?;
                    let next = page.next_offset();
                    let pages = page.total.div_ceil(50);
                    rows.extend(page.items.iter().map(|saved| {
                        let mut item = map::album_item(&saved.album);
                        item.saved = true;
                        (saved.added_at.clone().unwrap_or_default(), item)
                    }));
                    let snapshot = rows.clone();
                    let last = !matches!(next, Some(n) if n > offset);
                    if !self.put_section(section, generation, (done, pages, last), |state| {
                        if !last && snapshot.len() < state.albums.len() {
                            return None;
                        }
                        let old: Vec<AlbumItem> = state.albums.iter().map(|(_, item)| item.clone()).collect();
                        let new: Vec<AlbumItem> = snapshot.iter().map(|(_, item)| item.clone()).collect();
                        state.albums = snapshot;
                        list_change(&old, &new).map(LibraryEvent::Albums)
                    }) {
                        return Ok(());
                    }
                    if last {
                        break;
                    }
                    offset = next.unwrap_or_default();
                }
            }
            Section::Shows => {
                let mut shows: Vec<(String, ShowItem)> = Vec::new();
                let mut offset = 0;
                for done in 1..=MAX_SECTION_PAGES {
                    let page = client.saved_shows(offset, 50).await?;
                    let next = page.next_offset();
                    let pages = page.total.div_ceil(50);
                    shows.extend(page.items.iter().map(|saved| {
                        let mut item = map::show_item(&saved.show);
                        item.saved = true;
                        (saved.added_at.clone().unwrap_or_default(), item)
                    }));
                    if !matches!(next, Some(n) if n > offset) || done == MAX_SECTION_PAGES {
                        // аудиокниги librespot не играет
                        if let Some(session) = self.player.session() {
                            let uris: Vec<String> = shows.iter().map(|(_, show)| show.uri.clone()).collect();
                            if let Ok(books) =
                                sf_player::engine::session_reads::audiobook_shows(&session, &uris).await
                            {
                                shows.retain(|(_, show)| !books.contains(&show.uri));
                            }
                        }
                        let snapshot = shows.clone();
                        self.put_section(section, generation, (done, pages, true), |state| {
                            let old: Vec<ShowItem> = state.shows.iter().map(|(_, item)| item.clone()).collect();
                            let new: Vec<ShowItem> = snapshot.iter().map(|(_, item)| item.clone()).collect();
                            state.shows = snapshot;
                            list_change(&old, &new).map(LibraryEvent::Shows)
                        });
                        break;
                    }
                    offset = next.unwrap_or_default();
                }
            }
            Section::Episodes => {
                let mut episodes: Vec<EpisodeItem> = Vec::new();
                let mut offset = 0;
                for done in 1..=MAX_SECTION_PAGES {
                    let page = client.saved_episodes(offset, 50).await?;
                    let next = page.next_offset();
                    let pages = page.total.div_ceil(50);
                    episodes.extend(page.items.iter().map(|saved| {
                        let mut item = map::episode_item(&saved.episode, None);
                        item.saved = true;
                        item
                    }));
                    let snapshot = episodes.clone();
                    let last = !matches!(next, Some(n) if n > offset);
                    if !self.put_section(section, generation, (done, pages, last), |state| {
                        if !last && snapshot.len() < state.episodes.len() {
                            return None;
                        }
                        let change = list_change(&state.episodes, &snapshot);
                        state.episodes = snapshot;
                        change.map(LibraryEvent::Episodes)
                    }) {
                        return Ok(());
                    }
                    if last {
                        break;
                    }
                    offset = next.unwrap_or_default();
                }
            }
            Section::Artists => {
                let mut artists: Vec<ArtistItem> = Vec::new();
                let mut after: Option<String> = None;
                for done in 1..=MAX_SECTION_PAGES {
                    let page = client.followed_artists(after.as_deref(), 50).await?;
                    let pages = page.total.map_or(done, |total| total.div_ceil(50));
                    let count = page.items.len();
                    artists.extend(page.items.iter().map(map::artist_item));
                    after = page.cursors.as_ref().and_then(|cursors| cursors.after.clone()).filter(|_| page.next.is_some());
                    let last = after.is_none() || count == 0;
                    let snapshot = artists.clone();
                    if !self.put_section(section, generation, (done, pages, last), |state| {
                        if !last && snapshot.len() < state.artists.len() {
                            return None;
                        }
                        let change = list_change(&state.artists, &snapshot);
                        state.artists = snapshot;
                        change.map(LibraryEvent::Artists)
                    }) {
                        return Ok(());
                    }
                    if last {
                        break;
                    }
                }
            }
            Section::Playlists => {}
        }
        Ok(())
    }

    /// Альбомы и подписки — из коллекции сессии одним чтением; false — спросить Web API.
    async fn session_section(&self, section: Section, generation: u64) -> bool {
        let (api, player) = (self.auth.api(), &self.player);
        match section {
            Section::Albums => {
                let Some(saved) =
                    collection_session::read(&api, player, "albums", |s| async move { collection_session::saved_albums(&s).await })
                        .await
                else {
                    return false;
                };
                let rows: Vec<(String, AlbumItem)> = saved
                    .iter()
                    .map(|saved| {
                        let mut item = map::album_item(&saved.album);
                        item.saved = true;
                        (saved.added_at.clone().unwrap_or_default(), item)
                    })
                    .collect();
                self.put_section(section, generation, (1, 1, true), |state| {
                    let old: Vec<AlbumItem> = state.albums.iter().map(|(_, item)| item.clone()).collect();
                    let new: Vec<AlbumItem> = rows.iter().map(|(_, item)| item.clone()).collect();
                    state.albums = rows;
                    list_change(&old, &new).map(LibraryEvent::Albums)
                });
                true
            }
            Section::Artists => {
                let Some(followed) = collection_session::read(&api, player, "followed artists", |s| async move {
                    collection_session::followed_artists(&s).await
                })
                .await
                else {
                    return false;
                };
                let artists: Vec<ArtistItem> = followed.iter().map(map::artist_item).collect();
                self.put_section(section, generation, (1, 1, true), |state| {
                    let change = list_change(&state.artists, &artists);
                    state.artists = artists;
                    change.map(LibraryEvent::Artists)
                });
                true
            }
            _ => self.session_podcasts(section, generation).await,
        }
    }

    /// Подкасты (набор «show», без аудиокниг) и «Ваши выпуски» («listenlater») — из коллекции сессии;
    /// false — спросить Web API.
    async fn session_podcasts(&self, section: Section, generation: u64) -> bool {
        let (api, player) = (self.auth.api(), &self.player);
        match section {
            Section::Shows => {
                let Some(saved) =
                    collection_session::read(&api, player, "podcasts", |s| async move { podcast_session::saved_shows(&s).await })
                        .await
                else {
                    return false;
                };
                let rows: Vec<(String, ShowItem)> = saved
                    .iter()
                    .map(|(added_at, show)| {
                        let mut item = map::show_item(show);
                        item.saved = true;
                        (added_at.clone(), item)
                    })
                    .collect();
                self.put_section(section, generation, (1, 1, true), |state| {
                    let old: Vec<ShowItem> = state.shows.iter().map(|(_, item)| item.clone()).collect();
                    let new: Vec<ShowItem> = rows.iter().map(|(_, item)| item.clone()).collect();
                    state.shows = rows;
                    list_change(&old, &new).map(LibraryEvent::Shows)
                });
                true
            }
            Section::Episodes => {
                let Some(saved) = collection_session::read(&api, player, "your episodes", |s| async move {
                    podcast_session::saved_episodes(&s).await
                })
                .await
                else {
                    return false;
                };
                let episodes: Vec<EpisodeItem> = saved
                    .iter()
                    .map(|episode| {
                        let mut item = map::episode_item(episode, None);
                        item.saved = true;
                        item
                    })
                    .collect();
                self.put_section(section, generation, (1, 1, true), |state| {
                    let change = list_change(&state.episodes, &episodes);
                    state.episodes = episodes;
                    change.map(LibraryEvent::Episodes)
                });
                true
            }
            _ => false,
        }
    }

    pub fn albums(&self) -> Vec<AlbumItem> {
        lock(&self.state).albums.iter().map(|(_, item)| item.clone()).collect()
    }
    pub fn artists(&self) -> Vec<ArtistItem> {
        lock(&self.state).artists.clone()
    }
    pub fn shows(&self) -> Vec<ShowItem> {
        lock(&self.state).shows.iter().map(|(_, item)| item.clone()).collect()
    }
    pub fn episodes(&self) -> Vec<EpisodeItem> {
        lock(&self.state).episodes.clone()
    }
    /// Альбом/шоу/выпуск/исполнитель (подписка) сохранён.
    pub fn is_saved(&self, uri: &str) -> bool {
        lock(&self.state).is_saved(uri)
    }
    pub fn saved_revision(&self) -> u64 {
        lock(&self.state).saved_revision
    }

    /// Сохранить/удалить (альбом, шоу, выпуск) или подписаться/отписаться (исполнитель).
    pub fn toggle_saved(&self, uri: String) {
        let kind = uri_kind(&uri).to_owned();
        match kind.as_str() {
            "playlist" => {
                let id = PlaylistId::from_uri_or_id(&uri);
                if self.is_saved(&uri) { self.unfollow_playlist(id) } else { self.follow_playlist(id) }
                return;
            }
            "track" => {
                let track = known::get(&uri).unwrap_or_else(|| TrackItem {
                    uri: uri.clone(),
                    track_id: sf_api::util::uri_id(&uri).unwrap_or_default().to_owned(),
                    available: true,
                    ..Default::default()
                });
                self.toggle_like(track);
                return;
            }
            "album" | "artist" | "show" | "episode" => {}
            _ => return,
        }
        let (target, events) = {
            let mut state = lock(&self.state);
            if state.account.is_none() {
                drop(state);
                self.notice(mf_i18n::t("Sign in to your account"));
                return;
            }
            let target = !state.is_saved(&uri);
            state.saved_intents.insert(uri.clone(), target);
            state.saved_revision += 1;
            let mut events = vec![LibraryEvent::SavedChanged];
            if !target {
                events.extend(remove_saved(&mut state, &uri));
                events.push(LibraryEvent::EntriesChanged);
            }
            (target, events)
        };
        self.send(events);
        let generation = self.generation();
        self.spawn(move |service| async move {
            let uris = [uri.clone()];
            let result = collection_session::write(&service.auth.api(), &service.player, &uris, target).await;
            if !service.current(generation) {
                return;
            }
            {
                let mut state = lock(&service.state);
                if result.is_err() {
                    state.saved_intents.remove(&uri);
                }
                state.saved_revision += 1;
            }
            service.send([LibraryEvent::SavedChanged]);
            let section = match kind.as_str() {
                "album" => Section::Albums,
                "artist" => Section::Artists,
                "show" => Section::Shows,
                _ => Section::Episodes,
            };
            match result {
                Ok(()) => {
                    if target {
                        service.load_section(section);
                    }
                    let text = match (kind.as_str(), target) {
                        ("artist", true) => mf_i18n::t("You followed the artist"),
                        ("artist", false) => mf_i18n::t("You unfollowed the artist"),
                        (_, true) => mf_i18n::t("Added to your library"),
                        (_, false) => mf_i18n::t("Removed from your library"),
                    };
                    service.notice(text);
                }
                Err(error) => {
                    if !target {
                        service.load_section(section);
                    }
                    let text = service.error_text_of(&error);
                    service.notice(mf_i18n::tf("Your library didn’t change: {0}", &[&text]));
                }
            }
        });
    }

    // --- экран «Библиотека» ----------------------------------------------------------------

    /// Строки раздела: owner — "all" | "mine" | "followed" | "collab"; sort — как Settings.librarySort.
    /// Плейлисты: сначала «Любимые треки» и закреплённые, папки rootlist как kind=folder.
    pub fn entries(&self, kind: LibraryKind, owner: &str, sort: &str) -> Vec<LibraryEntry> {
        let times = self.history.context_times();
        let state = lock(&self.state);
        let mut entries = match kind {
            LibraryKind::Playlists => {
                let mut entries = Vec::new();
                if state.account.is_some() && matches!(owner, "all" | "mine" | "") {
                    entries.push(state.liked_entry(&times));
                }
                let folders_shown = matches!(owner, "all" | "") && matches!(sort, "spotify" | "");
                let tree = state.rootlist.as_ref().map(folder_tree);
                let mut placed: HashSet<&str> = HashSet::new();
                if let (true, Some((root, folders))) = (folders_shown, &tree) {
                    for key in root {
                        if let Some(uri) = key.strip_prefix("playlist:") {
                            if let Some(item) = state.playlists.iter().find(|item| item.uri == uri) {
                                placed.insert(item.uri.as_str());
                                entries.push(state.playlist_entry(item, "", &times));
                            }
                        } else if let Some(id) = key.strip_prefix("folder:")
                            && let Some(folder) = folders.get(id)
                        {
                            entries.push(state.folder_entry(folder, folders));
                            collect_placed(folder, folders, &mut placed);
                        }
                    }
                }
                let folder_of: HashMap<String, String> = tree
                    .as_ref()
                    .map(|(_, folders)| {
                        folders
                            .values()
                            .flat_map(|folder| {
                                folder.children.iter().filter_map(|child| {
                                    child.strip_prefix("playlist:").map(|uri| (uri.to_owned(), folder.id.clone()))
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for item in &state.playlists {
                    if placed.contains(item.uri.as_str()) || !state.owner_matches(item, owner) {
                        continue;
                    }
                    let folder = if folders_shown { "" } else { folder_of.get(&item.uri).map_or("", String::as_str) };
                    entries.push(state.playlist_entry(item, folder, &times));
                }
                entries
            }
            LibraryKind::Albums => state
                .albums
                .iter()
                .map(|(added, album)| LibraryEntry {
                    kind: "album".to_owned(),
                    id: album.album_id.clone(),
                    uri: album.uri.clone(),
                    title: album.title.clone(),
                    subtitle: if album.artists.is_empty() {
                        album.kind.clone()
                    } else {
                        format!("{} · {}", album.kind, album.artists)
                    },
                    cover: album.cover.clone(),
                    covers: vec![album.cover.clone()],
                    owner: album.artists.clone(),
                    read_only: true,
                    pinned: state.pins.contains(&album.uri),
                    track_count: album.track_count,
                    added_at: added.clone(),
                    played_at: times.get(&album.uri).cloned().unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
            LibraryKind::Artists => state
                .artists
                .iter()
                .map(|artist| LibraryEntry {
                    kind: "artist".to_owned(),
                    id: artist.artist_id.clone(),
                    uri: artist.uri.clone(),
                    title: artist.name.clone(),
                    subtitle: mf_i18n::t("Artist"),
                    cover: artist.cover.clone(),
                    covers: vec![artist.cover.clone()],
                    owner: artist.name.clone(),
                    read_only: true,
                    pinned: state.pins.contains(&artist.uri),
                    played_at: times.get(&artist.uri).cloned().unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
            LibraryKind::Shows => state
                .shows
                .iter()
                .map(|(added, show)| LibraryEntry {
                    kind: "show".to_owned(),
                    id: show.show_id.clone(),
                    uri: show.uri.clone(),
                    title: show.name.clone(),
                    subtitle: if show.publisher.is_empty() {
                        mf_i18n::t("Podcast")
                    } else {
                        mf_i18n::tf("Podcast · {0}", &[&show.publisher])
                    },
                    cover: show.cover.clone(),
                    covers: vec![show.cover.clone()],
                    owner: show.publisher.clone(),
                    read_only: true,
                    pinned: state.pins.contains(&show.uri),
                    track_count: show.total_episodes,
                    added_at: added.clone(),
                    played_at: times.get(&show.uri).cloned().unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
        };
        for entry in &mut entries {
            entry.covers.retain(|cover| !cover.is_empty());
        }
        sort_entries(&mut entries, sort, &state.pins);
        entries
    }

    /// Последние добавленные или обновлённые строки всех разделов — для боковой панели.
    /// Папки остаются в дереве плейлистов: в «последних» они только мешают.
    pub fn recent_entries(&self, limit: usize) -> Vec<LibraryEntry> {
        let mut all = Vec::new();
        for kind in [LibraryKind::Playlists, LibraryKind::Albums, LibraryKind::Artists, LibraryKind::Shows] {
            all.extend(self.entries(kind, "all", "added"));
        }
        merge_recent(all, limit)
    }

    pub fn folder_children(&self, folder_id: &str) -> Vec<LibraryEntry> {
        let times = self.history.context_times();
        let state = lock(&self.state);
        let Some((_, folders)) = state.rootlist.as_ref().map(folder_tree) else { return Vec::new() };
        let Some(folder) = folders.get(folder_id) else { return Vec::new() };
        folder
            .children
            .iter()
            .filter_map(|child| {
                if let Some(uri) = child.strip_prefix("playlist:") {
                    let item = state.playlists.iter().find(|item| item.uri == uri)?;
                    Some(state.playlist_entry(item, folder_id, &times))
                } else {
                    let id = child.strip_prefix("folder:")?;
                    folders.get(id).map(|sub| state.folder_entry(sub, &folders))
                }
            })
            .collect()
    }

    /// Явная сортировка догружает страницы: (загружено, всего).
    pub fn loading_pages(&self) -> (u32, u32) {
        let state = lock(&self.state);
        let (done, total) = state
            .progress
            .values()
            .fold((0, 0), |(done, total), (d, t)| (done + d, total + (*t).max(*d)));
        if total == 0 || done >= total { (0, 0) } else { (done, total) }
    }

    // --- закрепления (локально, SettingKey::Pins) -------------------------------------------

    pub fn is_pinned(&self, uri: &str) -> bool {
        lock(&self.state).pins.iter().any(|pin| pin == uri)
    }
    pub fn toggle_pin(&self, uri: String) {
        if uri.is_empty() {
            return;
        }
        let (pins, events) = {
            let mut state = lock(&self.state);
            match state.pins.iter().position(|pin| *pin == uri) {
                Some(index) => {
                    state.pins.remove(index);
                }
                None => state.pins.insert(0, uri),
            }
            state.pins_revision += 1;
            let mut events: Vec<LibraryEvent> = state.rebuild_playlists().into_iter().collect();
            events.push(LibraryEvent::PinsChanged);
            events.push(LibraryEvent::EntriesChanged);
            (state.pins.clone(), events)
        };
        write_list(&self.settings, SettingKey::Pins, &pins);
        self.send(events);
    }
    pub fn pins(&self) -> Vec<String> {
        lock(&self.state).pins.clone()
    }
    pub fn pins_revision(&self) -> u64 {
        lock(&self.state).pins_revision
    }

    pub fn error_text(&self) -> String {
        lock(&self.state).error_text.clone()
    }
}

/// Коллаж, который Spotify сам нарисовал плейлисту без своей обложки (Web API отдаёт его как обложку).
fn spotify_mosaic(images: &[models::Image]) -> bool {
    !images.is_empty() && images.iter().all(|image| image.url.starts_with("https://mosaic.scdn.co/"))
}

fn collect_placed<'a>(folder: &'a Folder, folders: &'a HashMap<String, Folder>, placed: &mut HashSet<&'a str>) {
    collect_placed_from(folder, folders, placed, &mut HashSet::new());
}

fn collect_placed_from<'a>(
    folder: &'a Folder,
    folders: &'a HashMap<String, Folder>,
    placed: &mut HashSet<&'a str>,
    seen: &mut HashSet<&'a str>,
) {
    // цикл папок (A внутри B внутри A) не уводит в бесконечную рекурсию
    if !seen.insert(folder.id.as_str()) {
        return;
    }
    for child in &folder.children {
        if let Some(uri) = child.strip_prefix("playlist:") {
            placed.insert(uri);
        } else if let Some(id) = child.strip_prefix("folder:")
            && let Some(sub) = folders.get(id)
        {
            collect_placed_from(sub, folders, placed, seen);
        }
    }
}

/// Убрать снятое с сохранения из своего раздела.
fn remove_saved(state: &mut State, uri: &str) -> Option<LibraryEvent> {
    state.saved.remove(uri);
    if let Some(at) = state.albums.iter().position(|(_, item)| item.uri == uri) {
        state.albums.remove(at);
        return Some(LibraryEvent::Albums(ListChange::Remove { at, count: 1 }));
    }
    if let Some(at) = state.artists.iter().position(|item| item.uri == uri) {
        state.artists.remove(at);
        return Some(LibraryEvent::Artists(ListChange::Remove { at, count: 1 }));
    }
    if let Some(at) = state.shows.iter().position(|(_, item)| item.uri == uri) {
        state.shows.remove(at);
        return Some(LibraryEvent::Shows(ListChange::Remove { at, count: 1 }));
    }
    if let Some(at) = state.episodes.iter().position(|item| item.uri == uri) {
        state.episodes.remove(at);
        return Some(LibraryEvent::Episodes(ListChange::Remove { at, count: 1 }));
    }
    None
}

/// Для тестов: страница сохранённых треков.
#[cfg(test)]
fn saved_page(offset: u32, count: u32, total: u32) -> models::Page<SavedTrack> {
    models::Page {
        offset,
        limit: count,
        total,
        next: (offset + count < total).then(|| "next".into()),
        items: (offset..offset + count)
            .map(|n| SavedTrack {
                track: models::Track { uri: format!("spotify:track:{n}"), ..Default::default() },
                ..Default::default()
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, title: &str, uri: &str) -> LibraryEntry {
        LibraryEntry { kind: kind.into(), title: title.into(), uri: uri.into(), ..Default::default() }
    }

    #[test]
    fn library_kinds_from_qml_names() {
        assert_eq!(LibraryKind::from_name("podcasts"), LibraryKind::Shows);
        assert_eq!(LibraryKind::from_name("x"), LibraryKind::Playlists);
    }

    #[test]
    fn liked_and_pins_lead_every_sort() {
        let mut entries = vec![
            entry("playlist", "Б", "spotify:playlist:b"),
            entry("playlist", "А", "spotify:playlist:a"),
            entry("playlist", "В", "spotify:playlist:c"),
            entry("liked", "Liked Songs", "spotify:user:me:collection"),
        ];
        sort_entries(&mut entries, "alpha", &["spotify:playlist:c".to_owned()]);
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Liked Songs", "В", "А", "Б"]);
    }

    #[test]
    fn recent_entries_merge_every_kind_newest_first() {
        let mut playlist = entry("playlist", "playlist", "spotify:playlist:p");
        playlist.added_at = "2026-05-01T00:00:00Z".into();
        let mut album = entry("album", "album", "spotify:album:a");
        album.added_at = "2026-09-01T00:00:00Z".into();
        let mut artist = entry("artist", "artist", "spotify:artist:x");
        artist.played_at = "2026-08-01T00:00:00Z".into();
        let folder = entry("folder", "folder", "folder:1");
        let merged = merge_recent(vec![playlist, folder, artist, album], 10);
        let titles: Vec<&str> = merged.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["album", "artist", "playlist"], "newest first, folders dropped");
        assert_eq!(merge_recent(vec![entry("album", "only", "u")], 0).len(), 0, "limit is honoured");
    }

    #[test]
    fn recent_sort_puts_never_played_last() {
        let mut entries = vec![entry("album", "old", "a"), entry("album", "never", "b"), entry("album", "new", "c")];
        entries[0].played_at = "2026-01-01T00:00:00Z".into();
        entries[2].played_at = "2026-09-01T00:00:00Z".into();
        sort_entries(&mut entries, "recent", &[]);
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["new", "old", "never"]);
    }

    #[test]
    fn folders_nest_from_the_rootlist() {
        let rootlist = Rootlist {
            entries: vec![
                RootlistEntry::Playlist("spotify:playlist:a".into()),
                RootlistEntry::FolderStart { id: "f1".into(), name: "Рок".into() },
                RootlistEntry::Playlist("spotify:playlist:b".into()),
                RootlistEntry::FolderStart { id: "f2".into(), name: "Старое".into() },
                RootlistEntry::Playlist("spotify:playlist:c".into()),
                RootlistEntry::FolderEnd,
                RootlistEntry::FolderEnd,
            ],
            editable: Default::default(),
        };
        let (root, folders) = folder_tree(&rootlist);
        assert_eq!(root, ["playlist:spotify:playlist:a", "folder:f1"]);
        assert_eq!(folders["f1"].children, ["playlist:spotify:playlist:b", "folder:f2"]);
        assert_eq!(folders["f2"].parent, "f1");
        let mut placed = HashSet::new();
        collect_placed(&folders["f1"], &folders, &mut placed);
        assert!(placed.contains("spotify:playlist:c"));
    }

    #[test]
    fn a_folder_inside_itself_does_not_loop() {
        let rootlist = Rootlist {
            entries: vec![
                RootlistEntry::FolderStart { id: "f1".into(), name: "A".into() },
                RootlistEntry::Playlist("spotify:playlist:a".into()),
                RootlistEntry::FolderStart { id: "f1".into(), name: "A again".into() },
                RootlistEntry::Playlist("spotify:playlist:b".into()),
                RootlistEntry::FolderEnd,
                RootlistEntry::FolderEnd,
                RootlistEntry::FolderStart { id: "f1".into(), name: "A elsewhere".into() },
                RootlistEntry::Playlist("spotify:playlist:c".into()),
                RootlistEntry::FolderEnd,
            ],
            editable: Default::default(),
        };
        let (root, folders) = folder_tree(&rootlist);
        assert_eq!(root, ["folder:f1"]);
        let mut placed = HashSet::new();
        collect_placed(&folders["f1"], &folders, &mut placed);
        for uri in ["spotify:playlist:a", "spotify:playlist:b", "spotify:playlist:c"] {
            assert!(placed.contains(uri), "{uri}");
        }
    }

    #[test]
    fn a_stale_read_does_not_undo_a_like() {
        let mut state = State { account: Some("me".into()), ..Default::default() };
        state.liked.cache_checked = true;
        state.liked.start_refresh(1);
        state.liked.absorb(0, saved_page(0, 3, 3), now_seconds());
        state.sync_liked();
        assert!(state.is_liked("spotify:track:1"));
        let revision = state.likes_revision;
        state.liked.change("spotify:track:1".into(), false, models::Track::default());
        let events = state.sync_liked();
        assert!(!state.is_liked("spotify:track:1"), "unliking updates rows before the network answers");
        assert!(state.likes_revision > revision);
        assert!(events.iter().any(|e| matches!(e, LibraryEvent::Liked(ListChange::Remove { at: 1, count: 1 }))));
        state.liked.start_refresh(2);
        state.liked.absorb(0, saved_page(0, 3, 3), now_seconds());
        state.sync_liked();
        assert!(!state.is_liked("spotify:track:1"), "a lagging read keeps the unlike");
    }

    #[test]
    fn saved_state_follows_intents_first() {
        let mut state = State::default();
        state.saved.insert("spotify:album:x".into());
        assert!(state.is_saved("spotify:album:x"));
        state.saved_intents.insert("spotify:album:x".into(), false);
        assert!(!state.is_saved("spotify:album:x"));
        assert!(remove_saved(&mut state, "spotify:album:missing").is_none());
    }

    fn page_with(id: &str, complete: bool, loading: bool, reload_after_write: bool) -> (PlaylistId, State) {
        let id = PlaylistId::from_uri_or_id(id);
        let mut state = State { account: Some("me".into()), ..Default::default() };
        let page = state.pages.entry(id.clone()).or_default();
        page.complete = complete;
        page.loading = loading;
        page.reload_after_write = reload_after_write;
        (id, state)
    }

    #[test]
    fn reopening_a_ready_playlist_does_not_refetch_or_reset() {
        // Без подтверждённой шапки (snapshot не сверялся) готовая страница перечитывается:
        // дешёвая проверка snapshot ловит изменения с других устройств.
        let (id, state) = page_with("spotify:playlist:a", true, false, false);
        assert_eq!(state.pages[&id].open_action(false), OpenAction::Fetch);
        assert_eq!(state.pages[&id].open_action(true), OpenAction::Fetch);

        // Шапка сверена со snapshot и треки на месте — повторное открытие ничего не делает.
        let (id, mut state) = page_with("spotify:playlist:a", true, false, false);
        let page = state.pages.get_mut(&id).unwrap();
        page.snapshot = "snap".into();
        page.header = Some(serde_json::from_str(
            r#"{"id":"a","name":"A","uri":"spotify:playlist:a","snapshot_id":"snap","items":{"total":3},
                "images":[],"owner":{"id":"me","display_name":"Me"},"public":true}"#,
        )
        .unwrap());
        assert_eq!(state.pages[&id].open_action(false), OpenAction::Idle);
        assert_eq!(state.pages[&id].open_action(true), OpenAction::Fetch);
    }

    #[test]
    fn in_flight_read_is_not_duplicated_but_refresh_restarts_it() {
        let (id, state) = page_with("spotify:playlist:a", false, true, false);
        assert_eq!(state.pages[&id].open_action(false), OpenAction::Loading);
        assert_eq!(state.pages[&id].open_action(true), OpenAction::Fetch);
        // чтение, помеченное перечитыванием после записи, тоже продолжается
        let (id, state) = page_with("spotify:playlist:a", false, true, true);
        assert_eq!(state.pages[&id].open_action(true), OpenAction::Fetch);
    }

    #[test]
    fn incomplete_or_reload_marked_pages_refetch() {
        let (id, state) = page_with("spotify:playlist:a", false, false, false);
        assert_eq!(state.pages[&id].open_action(false), OpenAction::Fetch);
        let (id, state) = page_with("spotify:playlist:a", true, false, true);
        assert_eq!(state.pages[&id].open_action(false), OpenAction::Fetch);
    }

    #[test]
    fn stale_generation_or_a_started_write_drops_the_answer() {
        let (id, mut state) = page_with("spotify:playlist:a", false, true, false);
        state.generation = 2;
        assert!(!read_is_current(&state, &id, 1, 1), "смена аккаунта сбрасывает чтение");
        let (id, mut state) = page_with("spotify:playlist:a", false, true, false);
        state.pages.get_mut(&id).unwrap().generation = 7;
        assert!(!read_is_current(&state, &id, 1, 1), "новое чтение страницы сбрасывает старое");
        let (id, mut state) = page_with("spotify:playlist:a", false, true, false);
        state.pages.get_mut(&id).unwrap().pending_writes = 1;
        assert!(!read_is_current(&state, &id, 1, 1), "начавшаяся запись прерывает чтение");
        let (id, state) = page_with("spotify:playlist:a", false, true, false);
        assert!(read_is_current(&state, &id, 0, 0));
        assert!(same_read(&state, &id, 0, 0));
    }

    #[test]
    fn cached_rows_arrive_in_order_and_with_repeats() {
        let id = PlaylistId::from_uri_or_id("spotify:playlist:a");
        let track = |i: usize| TrackItem { uri: format!("spotify:track:{i}"), ..Default::default() };
        let mut rows: Vec<TrackItem> = (0..CHUNK + 3).map(track).collect();
        rows.push(track(0)); // повтор в конце должен дойти до модели
        rows.insert(1, track(2)); // повтор в начале
        let events = cached_events(&id, &rows);
        assert_eq!(events.len(), 2, "по куску CHUNK");
        let mut delivered: Vec<String> = Vec::new();
        for event in &events {
            match event {
                LibraryEvent::PlaylistTracks { change: ListChange::Reset(chunk), .. } => {
                    assert!(delivered.is_empty(), "Reset только первым куском");
                    delivered.extend(chunk.iter().map(|r| r.uri.clone()));
                }
                LibraryEvent::PlaylistTracks { change: ListChange::Append(chunk), .. } => {
                    delivered.extend(chunk.iter().map(|r| r.uri.clone()));
                }
                _ => panic!("только Reset/Append"),
            }
        }
        let source: Vec<&str> = rows.iter().map(|r| r.uri.as_str()).collect();
        assert_eq!(delivered, source, "порядок и повторы сохраняются");
        assert!(matches!(&events[0], LibraryEvent::PlaylistTracks { change: ListChange::Reset(_), .. }));
    }
}
