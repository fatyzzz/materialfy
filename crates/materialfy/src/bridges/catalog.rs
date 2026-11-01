// Catalog: переходы (сигнал openRequested слушает главное окно), страницы альбома/исполнителя/шоу/выпуска
// (кэш AlbumData/ArtistData, повтор при ошибке), поиск с поколениями запросов, меню трека.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Map, Value};
use md3_model::{PlayContext, contexts};
use sf_services::{
    AlbumPage, ArtistPage, AuthEvent, CatalogService, EpisodePage, SearchKind, SearchResults, ServiceError, ShowPage,
};

use super::data::qobject::{AlbumData, ArtistData, EpisodeData, ShowData};
use super::data::{PageData, error_text};
use super::{Delivery, forward_events, json_str, map_to_json, model_ptr, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_rows};
use crate::qt::{QVariantList, QVariantMap, json_to_qvariantlist, json_to_qvariantmap, qvariant_to_json};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(QString, search_state, cxx_name = "searchState", READ, NOTIFY = search_changed)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY = search_changed)]
        #[qproperty(QMap_QString_QVariant, best, READ, NOTIFY = search_changed)]
        #[qproperty(QMap_QString_QVariant, totals, READ, NOTIFY = search_changed)]
        #[qproperty(*mut QObject, search_tracks, cxx_name = "searchTracks", READ = search_tracks_ptr, CONSTANT)]
        #[qproperty(*mut QObject, search_albums, cxx_name = "searchAlbums", READ = search_albums_ptr, CONSTANT)]
        #[qproperty(*mut QObject, search_artists, cxx_name = "searchArtists", READ = search_artists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, search_playlists, cxx_name = "searchPlaylists", READ = search_playlists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, search_shows, cxx_name = "searchShows", READ = search_shows_ptr, CONSTANT)]
        #[qproperty(*mut QObject, search_episodes, cxx_name = "searchEpisodes", READ = search_episodes_ptr, CONSTANT)]
        type Catalog = super::CatalogRust;

        fn search_tracks_ptr(self: &Catalog) -> *mut QObject;
        fn search_albums_ptr(self: &Catalog) -> *mut QObject;
        fn search_artists_ptr(self: &Catalog) -> *mut QObject;
        fn search_playlists_ptr(self: &Catalog) -> *mut QObject;
        fn search_shows_ptr(self: &Catalog) -> *mut QObject;
        fn search_episodes_ptr(self: &Catalog) -> *mut QObject;

        /// страница ("artist" | "album" | "playlist" | "show" | "episode" | "radio"), параметры
        #[qsignal]
        #[cxx_name = "openRequested"]
        fn open_requested(self: Pin<&mut Catalog>, page: QString, props: QMap_QString_QVariant);
        /// у трека несколько исполнителей — пусть выберет
        #[qsignal]
        #[cxx_name = "chooseArtist"]
        fn choose_artist(self: Pin<&mut Catalog>, refs: QList_QVariant);
        /// карточка исполнителя: id, x, y (низ ссылки), ширина
        #[qsignal]
        #[cxx_name = "previewRequested"]
        fn preview_requested(self: Pin<&mut Catalog>, artist_id: QString, x: f64, y: f64, width: f64);
        #[qsignal]
        #[cxx_name = "previewCancelled"]
        fn preview_cancelled(self: Pin<&mut Catalog>);
        #[qsignal]
        #[cxx_name = "searchChanged"]
        fn search_changed(self: Pin<&mut Catalog>);
        /// меню трека у точки (x, y) — его показывает главное окно; w, h > 0 — размер кнопки «⋯»
        /// с левым верхним углом в (x, y): меню встаёт у неё
        #[qsignal]
        #[cxx_name = "trackMenuRequested"]
        fn track_menu_requested(self: Pin<&mut Catalog>, track: QMap_QString_QVariant, x: f64, y: f64, w: f64, h: f64);
        /// ответ на requestTrackDetails: id трека и словарь TrackDetails (пустой — не загрузилось)
        #[qsignal]
        #[cxx_name = "trackDetailsReady"]
        fn track_details_ready(self: Pin<&mut Catalog>, track_id: QString, details: QMap_QString_QVariant);

        #[qinvokable]
        #[cxx_name = "openArtist"]
        fn open_artist(self: Pin<&mut Catalog>, artist_id: &QString);
        #[qinvokable]
        #[cxx_name = "openAlbum"]
        fn open_album(self: Pin<&mut Catalog>, album_id: &QString);
        /// Страница альбома прокручивает к треку и подсвечивает его.
        #[qinvokable]
        #[cxx_name = "openAlbum"]
        fn open_album_at(self: Pin<&mut Catalog>, album_id: &QString, track_id: &QString);
        #[qinvokable]
        #[cxx_name = "openArtists"]
        fn open_artists(self: Pin<&mut Catalog>, refs: &QList_QVariant);
        #[qinvokable]
        #[cxx_name = "previewArtist"]
        fn preview_artist(self: Pin<&mut Catalog>, artist_id: &QString, x: f64, y: f64, width: f64);
        #[qinvokable]
        #[cxx_name = "cancelPreview"]
        fn cancel_preview(self: Pin<&mut Catalog>);
        /// {playlistId | uri, title, cover} → openRequested("playlist", {playlistId, playlistTitle, cover})
        #[qinvokable]
        #[cxx_name = "openPlaylist"]
        fn open_playlist(self: Pin<&mut Catalog>, playlist: &QMap_QString_QVariant);
        #[qinvokable]
        #[cxx_name = "openShow"]
        fn open_show(self: Pin<&mut Catalog>, show_id: &QString);
        #[qinvokable]
        #[cxx_name = "openEpisode"]
        fn open_episode(self: Pin<&mut Catalog>, episode_id: &QString);
        /// Любая ссылка open.spotify.com / spotify:… → нужная страница; false — не распознана.
        #[qinvokable]
        #[cxx_name = "openUri"]
        fn open_uri(self: Pin<&mut Catalog>, text: &QString) -> bool;

        #[qinvokable]
        fn album(self: Pin<&mut Catalog>, album_id: &QString) -> *mut QObject;
        #[qinvokable]
        fn artist(self: Pin<&mut Catalog>, artist_id: &QString) -> *mut QObject;
        /// ShowData
        #[qinvokable]
        fn show(self: Pin<&mut Catalog>, show_id: &QString) -> *mut QObject;
        /// EpisodeData
        #[qinvokable]
        fn episode(self: Pin<&mut Catalog>, episode_id: &QString) -> *mut QObject;

        #[qinvokable]
        fn search(self: Pin<&mut Catalog>, query: &QString, kind: &QString);
        #[qinvokable]
        #[cxx_name = "search"]
        fn search_all(self: Pin<&mut Catalog>, query: &QString);

        #[qinvokable]
        #[cxx_name = "copyLink"]
        fn copy_link(self: &Catalog, url: &QString);
        /// https://open.spotify.com/track/<id> (выпуск — /episode/<id>)
        #[qinvokable]
        #[cxx_name = "trackUrl"]
        fn track_url(self: &Catalog, track: &QMap_QString_QVariant) -> QString;
        /// https://open.spotify.com/artist/<id>
        #[qinvokable]
        #[cxx_name = "artistUrl"]
        fn artist_url(self: &Catalog, artist_id: &QString) -> QString;
        /// https://open.spotify.com/<kind>/<id> для любого spotify:-URI
        #[qinvokable]
        #[cxx_name = "uriUrl"]
        fn uri_url(self: &Catalog, uri: &QString) -> QString;
        #[qinvokable]
        #[cxx_name = "requestTrackMenu"]
        fn request_track_menu(self: Pin<&mut Catalog>, track: &QMap_QString_QVariant, x: f64, y: f64);
        /// меню трека у кнопки: (x, y) — её левый верхний угол в координатах окна, w × h — размер
        #[qinvokable]
        #[cxx_name = "requestTrackMenuAt"]
        fn request_track_menu_at(self: Pin<&mut Catalog>, track: &QMap_QString_QVariant, x: f64, y: f64, w: f64, h: f64);
        /// Окно «О треке»: всё, что известно о треке {trackId, albumId} → trackDetailsReady.
        #[qinvokable]
        #[cxx_name = "requestTrackDetails"]
        fn request_track_details(self: Pin<&mut Catalog>, track: &QMap_QString_QVariant);
    }

    impl cxx_qt::Threading for Catalog {}
    impl cxx_qt::Initialize for Catalog {}
}

const NOT_SIGNED_IN: &str = "Sign in to your account";

/// Объект страницы и номер его текущей загрузки (ответ старой загрузки отбрасывается).
struct Entry<T: cxx::memory::UniquePtrTarget> {
    /// 0 — ещё не загружался
    load: u64,
    /// загружен для прошлого аккаунта — перечитать при следующем обращении
    stale: bool,
    data: UniquePtr<T>,
}

type Fetch<P> = Pin<Box<dyn Future<Output = Result<P, ServiceError>> + Send>>;

/// Страница каталога: где лежит её кэш и чем она грузится.
trait CatalogPage: PageData {
    fn entries(catalog: &mut CatalogRust) -> &mut HashMap<String, Entry<Self>>;
    fn fetch(catalog: Arc<CatalogService>, id: String) -> Fetch<Self::Page>;
    /// перед загрузкой (шоу запоминает свой id для loadMore)
    fn prepare(_data: Pin<&mut Self>, _id: &str) {}
}

impl CatalogPage for AlbumData {
    fn entries(catalog: &mut CatalogRust) -> &mut HashMap<String, Entry<Self>> {
        &mut catalog.albums
    }
    fn fetch(catalog: Arc<CatalogService>, id: String) -> Fetch<AlbumPage> {
        Box::pin(async move { catalog.album(&id).await })
    }
}

impl CatalogPage for ArtistData {
    fn entries(catalog: &mut CatalogRust) -> &mut HashMap<String, Entry<Self>> {
        &mut catalog.artists
    }
    fn fetch(catalog: Arc<CatalogService>, id: String) -> Fetch<ArtistPage> {
        Box::pin(async move { catalog.artist(&id).await })
    }
}

impl CatalogPage for ShowData {
    fn entries(catalog: &mut CatalogRust) -> &mut HashMap<String, Entry<Self>> {
        &mut catalog.shows
    }
    fn fetch(catalog: Arc<CatalogService>, id: String) -> Fetch<ShowPage> {
        Box::pin(async move { catalog.show(&id).await })
    }
    fn prepare(data: Pin<&mut Self>, id: &str) {
        data.set_show_id(id);
    }
}

impl CatalogPage for EpisodeData {
    fn entries(catalog: &mut CatalogRust) -> &mut HashMap<String, Entry<Self>> {
        &mut catalog.episodes
    }
    fn fetch(catalog: Arc<CatalogService>, id: String) -> Fetch<EpisodePage> {
        Box::pin(async move { catalog.episode(&id).await })
    }
}

pub struct CatalogRust {
    albums: HashMap<String, Entry<AlbumData>>,
    artists: HashMap<String, Entry<ArtistData>>,
    shows: HashMap<String, Entry<ShowData>>,
    episodes: HashMap<String, Entry<EpisodeData>>,
    loads: u64,
    /// номер перехода: поздний ответ «трек → альбом» не уводит со страницы, открытой позже
    nav: u64,
    // поиск
    search_state: QString,
    search_query: QString,
    best: QVariantMap,
    totals: QVariantMap,
    query: String,
    kind: SearchKind,
    state: &'static str,
    generation: u64,
    search_tracks: UniquePtr<DictListModel>,
    search_albums: UniquePtr<DictListModel>,
    search_artists: UniquePtr<DictListModel>,
    search_playlists: UniquePtr<DictListModel>,
    search_shows: UniquePtr<DictListModel>,
    search_episodes: UniquePtr<DictListModel>,
}

impl Default for CatalogRust {
    fn default() -> Self {
        let mut search_tracks = DictListModel::create(ModelKind::Track);
        with_model(&mut search_tracks, |m| m.set_context(PlayContext::new(contexts::SEARCH)));
        Self {
            albums: HashMap::new(),
            artists: HashMap::new(),
            shows: HashMap::new(),
            episodes: HashMap::new(),
            loads: 0,
            nav: 0,
            search_state: QString::from("idle"),
            search_query: QString::default(),
            best: QVariantMap::default(),
            totals: QVariantMap::default(),
            query: String::new(),
            kind: SearchKind::All,
            state: "idle",
            generation: 0,
            search_tracks,
            search_albums: DictListModel::create(ModelKind::Album),
            search_artists: DictListModel::create(ModelKind::Artist),
            search_playlists: DictListModel::create(ModelKind::Playlist),
            search_shows: DictListModel::create(ModelKind::Show),
            search_episodes: DictListModel::create(ModelKind::Episode),
        }
    }
}

fn props(pairs: &[(&str, Value)]) -> QVariantMap {
    let map: Map<String, Value> = pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
    json_to_qvariantmap(&map)
}

fn to_map<T: serde::Serialize>(value: &T) -> QVariantMap {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => json_to_qvariantmap(&map),
        _ => QVariantMap::default(),
    }
}

/// «Любимые треки»: spotify:user:<id>:collection (и spotify:collection).
fn is_collection_uri(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("spotify:") else { return false };
    let rest = rest.strip_suffix(":tracks").unwrap_or(rest);
    rest == "collection" || rest.strip_prefix("user:").is_some_and(|r| r.ends_with(":collection") && r.len() > 11)
}

fn signed_in() -> bool {
    ctx().services.auth.is_signed_in()
}

impl qobject::Catalog {
    fn search_tracks_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_tracks)
    }

    fn search_albums_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_albums)
    }

    fn search_artists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_artists)
    }

    fn search_playlists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_playlists)
    }

    fn search_shows_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_shows)
    }

    fn search_episodes_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.search_episodes)
    }

    // --- переходы ------------------------------------------------------------

    fn open_artist(self: Pin<&mut Self>, artist_id: &QString) {
        if !artist_id.is_empty() {
            self.navigate("artist", props(&[("artistId", Value::from(artist_id.to_string()))]));
        }
    }

    fn open_album(self: Pin<&mut Self>, album_id: &QString) {
        self.open_album_at(album_id, &QString::default());
    }

    fn open_album_at(self: Pin<&mut Self>, album_id: &QString, track_id: &QString) {
        if album_id.is_empty() {
            return;
        }
        let mut pairs = vec![("albumId", Value::from(album_id.to_string()))];
        if !track_id.is_empty() {
            pairs.push(("trackId", Value::from(track_id.to_string())));
        }
        self.navigate("album", props(&pairs));
    }

    fn open_artists(self: Pin<&mut Self>, refs: &QVariantList) {
        let refs: Vec<Value> = refs
            .iter()
            .map(qvariant_to_json)
            .filter(|r| r.as_object().is_some_and(|o| !json_str(o, "id").is_empty()))
            .collect();
        match refs.as_slice() {
            [] => {}
            [single] => {
                let id = single.as_object().map(|o| json_str(o, "id")).unwrap_or_default();
                self.open_artist(&QString::from(id.as_str()));
            }
            _ => self.choose_artist(json_to_qvariantlist(&refs)),
        }
    }

    fn preview_artist(self: Pin<&mut Self>, artist_id: &QString, x: f64, y: f64, width: f64) {
        if !artist_id.is_empty() {
            self.preview_requested(artist_id.clone(), x, y, width);
        }
    }

    fn cancel_preview(self: Pin<&mut Self>) {
        self.preview_cancelled();
    }

    fn open_playlist(self: Pin<&mut Self>, playlist: &QVariantMap) {
        let playlist = map_to_json(playlist);
        let mut id = json_str(&playlist, "playlistId");
        let uri = json_str(&playlist, "uri");
        if id.is_empty() && (uri.contains(":playlist:") || uri.contains("/playlist/")) {
            id = md3_model::PlaylistId::from_uri_or_id(&uri).0;
        }
        // строка экрана «Библиотека» (LIBRARY_KEYS): id без префикса
        if id.is_empty() && uri.is_empty() {
            id = json_str(&playlist, "id");
        }
        if id.is_empty() {
            return;
        }
        let mut title = json_str(&playlist, "title");
        if title.is_empty() {
            title = json_str(&playlist, "playlistTitle");
        }
        let pairs = [
            ("playlistId", Value::from(id)),
            ("playlistTitle", Value::from(title)),
            ("cover", Value::from(json_str(&playlist, "cover"))),
        ];
        self.navigate("playlist", props(&pairs));
    }

    fn open_show(self: Pin<&mut Self>, show_id: &QString) {
        if !show_id.is_empty() {
            self.navigate("show", props(&[("showId", Value::from(show_id.to_string()))]));
        }
    }

    fn open_episode(self: Pin<&mut Self>, episode_id: &QString) {
        if !episode_id.is_empty() {
            self.navigate("episode", props(&[("episodeId", Value::from(episode_id.to_string()))]));
        }
    }

    fn navigate(mut self: Pin<&mut Self>, page: &str, props: QVariantMap) {
        self.as_mut().rust_mut().nav += 1;
        self.open_requested(QString::from(page), props);
    }

    fn open_uri(mut self: Pin<&mut Self>, text: &QString) -> bool {
        let text = text.to_string();
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        if is_collection_uri(text) {
            self.navigate("liked", QVariantMap::default());
            return true;
        }
        if text.starts_with("spotify:station:") {
            let Some(seed) = sf_api::util::station_seed(text) else { return false };
            self.navigate("radio", props(&[("seed", Value::from(seed))]));
            return true;
        }
        let Some(uri) = sf_api::link::parse(text) else { return false };
        if let Some(query) = sf_api::link::search_query(&uri) {
            self.as_mut().search_all(&QString::from(query.as_str()));
            self.navigate("search", QVariantMap::default());
            return true;
        }
        let (Some(kind), Some(id)) = (sf_api::util::uri_kind(&uri), sf_api::util::uri_id(&uri)) else { return false };
        let id = QString::from(id);
        match kind {
            "artist" => self.open_artist(&id),
            "album" => self.open_album(&id),
            // DJ выглядит плейлистом, но списка треков у него нет: ссылка на него только запускает его
            "playlist" if uri == sf_services::data::home_feed::DJ_URI => {
                let context = PlayContext::with_uri("playlist", uri.clone());
                ctx().services.player.play_context(context, None, None);
            }
            "playlist" => {
                let pairs = [
                    ("playlistId", Value::from(id.to_string())),
                    ("playlistTitle", Value::from("")),
                    ("cover", Value::from("")),
                ];
                self.navigate("playlist", props(&pairs));
            }
            "show" => self.open_show(&id),
            "episode" => self.open_episode(&id),
            "track" => self.open_track(id.to_string()),
            _ => return false,
        }
        true
    }

    /// Ссылка на трек → его альбом с подсветкой трека (альбом узнаётся запросом).
    fn open_track(mut self: Pin<&mut Self>, track_id: String) {
        let thread = self.qt_thread();
        let mut this = self.as_mut().rust_mut();
        this.nav += 1;
        let nav = this.nav;
        if !signed_in() {
            return;
        }
        let catalog = ctx().services.catalog.clone();
        ctx().rt.spawn(async move {
            let result = catalog.track(&track_id).await;
            let _ = thread.queue(move |catalog| catalog.track_resolved(nav, track_id, result));
        });
    }

    fn track_resolved(self: Pin<&mut Self>, nav: u64, track_id: String, result: Result<md3_model::TrackItem, ServiceError>) {
        if nav != self.nav {
            return;
        }
        match result {
            Ok(track) if !track.album_id.is_empty() => {
                let track_id = if track.track_id.is_empty() { track_id } else { track.track_id };
                self.open_album_at(&QString::from(track.album_id.as_str()), &QString::from(track_id.as_str()));
            }
            Ok(_) => tracing::warn!("track {track_id}: album unknown"),
            Err(error) => tracing::warn!("track {track_id}: {error}"),
        }
    }

    // --- страницы альбома, исполнителя, шоу, выпуска ---------------------------

    fn album(self: Pin<&mut Self>, album_id: &QString) -> *mut cxx_qt::QObject {
        self.page::<AlbumData>(album_id.to_string())
    }

    fn artist(self: Pin<&mut Self>, artist_id: &QString) -> *mut cxx_qt::QObject {
        self.page::<ArtistData>(artist_id.to_string())
    }

    fn show(self: Pin<&mut Self>, show_id: &QString) -> *mut cxx_qt::QObject {
        self.page::<ShowData>(show_id.to_string())
    }

    fn episode(self: Pin<&mut Self>, episode_id: &QString) -> *mut cxx_qt::QObject {
        self.page::<EpisodeData>(episode_id.to_string())
    }

    /// Объект страницы из кэша; загрузка — при первом обращении, после ошибки и после смены аккаунта.
    fn page<T: CatalogPage>(mut self: Pin<&mut Self>, id: String) -> *mut cxx_qt::QObject {
        let thread = self.qt_thread();
        let this = self.as_mut().rust_mut().get_mut();
        this.loads += 1;
        let load = this.loads;
        let entry = T::entries(this)
            .entry(id.clone())
            .or_insert_with(|| Entry { load: 0, stale: false, data: T::new_object() });
        let Some(mut data) = entry.data.as_mut() else { return std::ptr::null_mut() };
        let ptr = data.as_mut().object_ptr();
        // уже загружен или грузится; пустой id (привязка QML ещё не готова) — без запроса
        if id.is_empty() || (entry.load != 0 && !entry.stale && !data.failed()) {
            return ptr;
        }
        let again = entry.load != 0;
        entry.load = load;
        entry.stale = false;
        if !signed_in() {
            data.fail(&mf_i18n::t(NOT_SIGNED_IN));
            return ptr;
        }
        if again {
            data.as_mut().reload();
        }
        T::prepare(data, &id);
        let catalog = ctx().services.catalog.clone();
        ctx().rt.spawn(async move {
            let result = T::fetch(catalog, id.clone()).await;
            let _ = thread.queue(move |catalog| catalog.page_loaded::<T>(id, load, result));
        });
        ptr
    }

    fn page_loaded<T: CatalogPage>(mut self: Pin<&mut Self>, id: String, load: u64, result: Result<T::Page, ServiceError>) {
        let Some(entry) = T::entries(self.as_mut().rust_mut().get_mut()).get_mut(&id) else { return };
        // ответ устаревшей загрузки (повтор, смена аккаунта)
        if entry.load != load {
            return;
        }
        let Some(data) = entry.data.as_mut() else { return };
        match result {
            Ok(page) => data.fill(page),
            Err(error) => {
                tracing::warn!("page {id}: {error}");
                data.fail(&error_text(&error));
            }
        }
    }

    /// Страницы с ошибкой — загрузить снова (вход завершился после открытия страницы).
    fn retry_failed<T: CatalogPage>(mut self: Pin<&mut Self>) {
        let ids: Vec<String> = T::entries(self.as_mut().rust_mut().get_mut())
            .iter()
            .filter(|(_, entry)| entry.load != 0 && entry.data.as_ref().is_some_and(|d| d.failed()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.as_mut().page::<T>(id);
        }
    }

    /// Загруженное прошлым аккаунтом перечитывается при следующем обращении.
    fn mark_stale(self: Pin<&mut Self>) {
        fn stale<T: cxx::memory::UniquePtrTarget>(entries: &mut HashMap<String, Entry<T>>) {
            for entry in entries.values_mut().filter(|e| e.load != 0) {
                entry.stale = true;
                // ответ, который ещё в пути, уже не подойдёт
                entry.load = u64::MAX;
            }
        }
        let this = self.rust_mut().get_mut();
        stale(&mut this.albums);
        stale(&mut this.artists);
        stale(&mut this.shows);
        stale(&mut this.episodes);
    }

    // --- поиск ---------------------------------------------------------------

    fn set_state(mut self: Pin<&mut Self>, state: &'static str) {
        let mut this = self.as_mut().rust_mut();
        this.state = state;
        this.search_state = QString::from(state);
    }

    fn search(mut self: Pin<&mut Self>, query: &QString, kind: &QString) {
        let query = query.to_string().trim().to_owned();
        let kind = SearchKind::from_name(&kind.to_string());
        if query == self.query && kind == self.kind && matches!(self.state, "loading" | "ok" | "empty") {
            return;
        }
        let thread = self.qt_thread();
        let mut this = self.as_mut().rust_mut();
        this.query = query.clone();
        this.search_query = QString::from(query.as_str());
        this.kind = kind;
        this.generation += 1;
        let generation = this.generation;
        if query.is_empty() {
            return self.clear();
        }
        if !signed_in() {
            self.as_mut().set_state("error");
            return self.search_changed();
        }
        self.as_mut().set_state("loading");
        self.as_mut().search_changed();
        let catalog = ctx().services.catalog.clone();
        ctx().rt.spawn(async move {
            let result = catalog.search(&query, kind).await;
            let _ = thread.queue(move |catalog| catalog.search_done(generation, result));
        });
    }

    fn search_all(self: Pin<&mut Self>, query: &QString) {
        self.search(query, &QString::from("all"));
    }

    fn search_done(mut self: Pin<&mut Self>, generation: u64, result: Result<SearchResults, ServiceError>) {
        if generation != self.generation {
            return;
        }
        let results = match result {
            Ok(results) => results,
            Err(error) => {
                tracing::warn!("search: {error}");
                self.as_mut().set_state("error");
                return self.search_changed();
            }
        };
        {
            let this = self.as_mut().rust_mut().get_mut();
            with_model(&mut this.search_tracks, |m| m.reset_rows(to_rows(&results.tracks)));
            with_model(&mut this.search_albums, |m| m.reset_rows(to_rows(&results.albums)));
            with_model(&mut this.search_artists, |m| m.reset_rows(to_rows(&results.artists)));
            with_model(&mut this.search_playlists, |m| m.reset_rows(to_rows(&results.playlists)));
            with_model(&mut this.search_shows, |m| m.reset_rows(to_rows(&results.shows)));
            with_model(&mut this.search_episodes, |m| m.reset_rows(to_rows(&results.episodes)));
            this.totals = to_map(&results.totals);
            this.best = results.best.as_ref().map(to_map).unwrap_or_default();
        }
        self.as_mut().set_state(if results.is_empty() { "empty" } else { "ok" });
        self.search_changed();
    }

    fn clear(mut self: Pin<&mut Self>) {
        {
            let this = self.as_mut().rust_mut().get_mut();
            this.best = QVariantMap::default();
            this.totals = QVariantMap::default();
            for model in [
                &mut this.search_tracks,
                &mut this.search_albums,
                &mut this.search_artists,
                &mut this.search_playlists,
                &mut this.search_shows,
                &mut this.search_episodes,
            ] {
                with_model(model, |m| m.reset_rows(Vec::new()));
            }
        }
        self.as_mut().set_state("idle");
        self.search_changed();
    }

    fn on_auth(mut self: Pin<&mut Self>, delivery: Delivery<AuthEvent>) {
        let (signed_out, signed_in_now) = match delivery {
            Delivery::Event(AuthEvent::SignedOut) => (true, false),
            Delivery::Event(AuthEvent::SignedIn(_)) => (false, true),
            Delivery::Event(_) => (false, false),
            Delivery::Lagged => (!signed_in(), signed_in()),
        };
        if signed_out {
            // ответы запросов прошлого аккаунта отбрасываются
            self.as_mut().rust_mut().generation += 1;
            self.as_mut().mark_stale();
            self.clear();
        } else if signed_in_now {
            self.as_mut().retry_failed::<AlbumData>();
            self.as_mut().retry_failed::<ArtistData>();
            self.as_mut().retry_failed::<ShowData>();
            self.as_mut().retry_failed::<EpisodeData>();
            if self.state == "error" && !self.query.is_empty() {
                let (query, kind) = (QString::from(self.query.as_str()), QString::from(self.kind.name()));
                self.search(&query, &kind);
            }
        }
    }

    // --- меню трека ----------------------------------------------------------

    fn copy_link(&self, url: &QString) {
        crate::qt::copy_to_clipboard(&url.to_string());
    }

    fn track_url(&self, track: &QVariantMap) -> QString {
        let track = map_to_json(track);
        let uri = json_str(&track, "uri");
        let track_id = json_str(&track, "trackId");
        let episode = track.get("isEpisode").and_then(Value::as_bool).unwrap_or(false);
        let url = if !uri.is_empty() {
            sf_api::util::open_spotify_url(&uri).unwrap_or_default()
        } else if !track_id.is_empty() {
            format!("https://open.spotify.com/{}/{track_id}", if episode { "episode" } else { "track" })
        } else {
            String::new()
        };
        QString::from(url.as_str())
    }

    fn artist_url(&self, artist_id: &QString) -> QString {
        if artist_id.is_empty() {
            return QString::default();
        }
        QString::from(format!("https://open.spotify.com/artist/{artist_id}").as_str())
    }

    fn uri_url(&self, uri: &QString) -> QString {
        QString::from(sf_api::util::open_spotify_url(&uri.to_string()).unwrap_or_default().as_str())
    }

    fn request_track_details(self: Pin<&mut Self>, track: &QVariantMap) {
        let track = map_to_json(track);
        let track_id = json_str(&track, "trackId");
        let album_id = json_str(&track, "albumId");
        if track_id.is_empty() || track.get("isEpisode").and_then(Value::as_bool).unwrap_or(false) {
            return;
        }
        let thread = self.qt_thread();
        if !signed_in() {
            let _ = thread.queue(move |catalog| catalog.track_details_ready(QString::from(track_id.as_str()), QVariantMap::default()));
            return;
        }
        let catalog = ctx().services.catalog.clone();
        ctx().rt.spawn(async move {
            let details = catalog.track_details(&track_id, &album_id).await;
            let _ = thread.queue(move |catalog| {
                let details = match details {
                    Ok(details) => to_map(&details),
                    Err(error) => {
                        tracing::debug!("track details {track_id}: {error}");
                        QVariantMap::default()
                    }
                };
                catalog.track_details_ready(QString::from(track_id.as_str()), details);
            });
        });
    }

    fn request_track_menu(self: Pin<&mut Self>, track: &QVariantMap, x: f64, y: f64) {
        self.track_menu_requested(track.clone(), x, y, 0.0, 0.0);
    }

    fn request_track_menu_at(self: Pin<&mut Self>, track: &QVariantMap, x: f64, y: f64, w: f64, h: f64) {
        self.track_menu_requested(track.clone(), x, y, w, h);
    }
}

impl cxx_qt::Initialize for qobject::Catalog {
    fn initialize(self: Pin<&mut Self>) {
        forward_events(self.qt_thread(), ctx().services.auth.subscribe(), Self::on_auth);
    }
}

#[cfg(test)]
mod tests {
    use super::is_collection_uri;

    #[test]
    fn collection_uris_open_liked() {
        assert!(is_collection_uri("spotify:user:abc:collection"));
        assert!(is_collection_uri("spotify:collection"));
        assert!(is_collection_uri("spotify:collection:tracks"));
        assert!(!is_collection_uri("spotify:user::collection"));
        assert!(!is_collection_uri("spotify:playlist:abc"));
        assert!(!is_collection_uri("https://open.spotify.com/collection"));
    }
}
