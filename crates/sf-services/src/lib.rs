//! Сервисы приложения (без Qt) — фасад той же формы, что сервисы yamusic-md3 + devices, history, home.
//! Каждый сервис — Arc; состояние — watch (снимок) и/или broadcast (события). Мосты QML подписываются в
//! cxx_qt::Initialize и переносят изменения в GUI-поток через CxxQtThread::queue.
//!
//! Один tokio-рантайм на всё (создаёт оболочка, engine.rs): сервисы, PlayerHandle-актор, librespot
//! (Session/Spirc спавнят задачи на текущем рантайме) и задачи мостов. Блокирующее (keyring, zeroconf,
//! размеры кэша) — spawn_blocking/свои потоки. Вход НЕ начинается в start — оболочка зовёт auth.start()
//! после загрузки QML (если не --offline).

pub mod auth;
pub mod catalog;
pub mod cover_fallback;
/// внутреннее «services-core»
pub mod data;
pub mod devices;
/// внутреннее «services-extra»
pub mod extra;
pub mod glue;
pub mod history;
pub mod home;
pub mod library;
pub mod lyrics;
/// «Новинки»: «Радар новинок» и свежие релизы исполнителей
pub mod releases;
/// Friends (friend activity) and Jam through the session
pub mod social;
/// auto-update from GitHub Releases
pub mod update;
pub mod wave;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sf_api::ApiGateway;
use sf_player::{PlayerConfig, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use mf_platform::{SettingKey, SettingsStore};

pub use auth::{AuthEvent, AuthService, AuthSnapshot, AuthState, PlaybackAuthState, ProxyInput};
pub use sf_auth::{Grant, ManualProxy, ProxyConfig, ProxyMode, set_browser_opener};
pub use catalog::{AlbumPage, ArtistPage, CatalogService, EpisodePage, SearchKind, SearchResults, ShowPage};
pub use devices::{DevicesEvent, DevicesService, DevicesSnapshot};
pub use history::{HistoryEvent, HistoryService, PlayRecord};
pub use home::{HomeService, HomeSnapshot};
pub use library::{FeedbackKind, LibraryEvent, LibraryKind, LibraryService, PlaylistInfo};
pub use lyrics::{LyricsService, LyricsSnapshot, LyricsStatus};
pub use md3_model::{ListChange, PlaylistId};
pub use releases::{NewRelease, ReleasesService, ReleasesSnapshot};
pub use social::{FriendActivity, Jam, JamMember, SocialService, SocialSnapshot};
pub use update::{UpdateService, UpdateSnapshot, UpdateState};
pub use wave::{WaveService, WaveSnapshot};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{}", mf_i18n::t("Sign in to your account"))]
    NotSignedIn,
    #[error("{}", data::route::api_error_text(.0))]
    Api(#[from] sf_api::ApiError),
    #[error("{0}")]
    Other(String),
}

/// Просьбы к оболочке Qt (медиаклавиши/MPRIS Raise/Quit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRequest {
    RaiseWindow,
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct ServicesOptions {
    /// сохранять/восстанавливать ручную очередь и точку возобновления (false в режиме снимков)
    pub persist_session: bool,
    /// медиаклавиши/SMTC/MPRIS (false в режиме снимков)
    pub enable_media: bool,
}

/// Все сервисы. Clone — дешёвый (Arc).
#[derive(Clone)]
pub struct Services {
    pub settings: Arc<SettingsStore>,
    pub api: Arc<ApiGateway>,
    pub auth: Arc<AuthService>,
    pub library: Arc<LibraryService>,
    pub catalog: Arc<CatalogService>,
    /// Радио Spotify (бывшая «Моя волна»)
    pub wave: Arc<WaveService>,
    pub home: Arc<HomeService>,
    /// «Новинки»
    pub releases: Arc<ReleasesService>,
    pub lyrics: Arc<LyricsService>,
    pub history: Arc<HistoryService>,
    pub devices: Arc<DevicesService>,
    /// friend activity and Jam
    pub social: Arc<SocialService>,
    /// auto-update (checks start from the Updater bridge, not here)
    pub update: Arc<update::UpdateService>,
    pub player: PlayerHandle,
    app_requests: broadcast::Sender<AppRequest>,
    media: Arc<Mutex<Option<glue::MediaStop>>>,
}

/// Настройка «Обложки мимо прокси» → флаг клиента обложек (меняется сразу, без перезапуска).
fn follow_covers_bypass(rt: &Handle, settings: &Arc<SettingsStore>, direct: Arc<std::sync::atomic::AtomicBool>) {
    let settings = settings.clone();
    let mut changes = settings.subscribe();
    rt.spawn(async move {
        loop {
            match changes.recv().await {
                Ok(SettingKey::CoversBypassProxy) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    direct.store(settings.get_bool(SettingKey::CoversBypassProxy), std::sync::atomic::Ordering::Relaxed);
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

impl Services {
    /// Порядок: HTTP (прокси из настроек) → ApiGateway → PlayerHandle → Auth → History → Library → Catalog →
    /// Wave → Releases → Home → Lyrics → Devices → glue (настройки → плеер) → медиаклавиши.
    pub fn start(rt: &Handle, settings: Arc<SettingsStore>, options: ServicesOptions) -> anyhow::Result<Services> {
        // применённый прокси из настроек; пароль из хранилища подставит auth.start()
        let proxy = auth::settings_proxy(&settings, None);
        let http = sf_api::Http::from_proxy(&proxy).unwrap_or_else(|error| {
            tracing::warn!("invalid proxy settings: {error:#}");
            sf_api::Http::unavailable(error)
        });
        let api = Arc::new(ApiGateway::new(http.clone(), Arc::new(sf_api::NetActivity::default())));
        let covers_direct = Arc::new(std::sync::atomic::AtomicBool::new(settings.get_bool(SettingKey::CoversBypassProxy)));
        follow_covers_bypass(rt, &settings, covers_direct.clone());
        let config = PlayerConfig {
            cover_dir: mf_platform::dirs::cache_subdir("covers"),
            session_file: options.persist_session.then(mf_platform::dirs::session_file),
            volume: settings.get_f64(SettingKey::Volume).clamp(0.0, 1.0),
            // клиент только для обложек: по настройке «Обложки мимо прокси» — напрямую
            http: http.bypassing(covers_direct),
        };
        let player = PlayerHandle::start(rt, config, api.clone()).map_err(|e| anyhow::anyhow!("player: {e}"))?;
        let store = sf_auth::Store::with_runtime(mf_platform::dirs::state_dir(), rt.clone());
        let auth = AuthService::new(rt.clone(), settings.clone(), api.clone(), http.clone(), player.clone(), store);
        let history = HistoryService::new(rt.clone(), auth.clone(), player.clone());
        let library =
            LibraryService::new(rt.clone(), auth.clone(), player.clone(), history.clone(), settings.clone());
        let catalog = CatalogService::new(auth.clone(), player.clone());
        let wave = WaveService::new(
            rt.clone(),
            auth.clone(),
            player.clone(),
            library.clone(),
            history.clone(),
            settings.clone(),
        );
        let releases =
            ReleasesService::new(rt.clone(), auth.clone(), library.clone(), history.clone(), player.clone(), http.clone());
        let home = HomeService::new(
            rt.clone(),
            auth.clone(),
            player.clone(),
            library.clone(),
            history.clone(),
            releases.clone(),
            settings.clone(),
        );
        // тексты — через общий клиент: тот же прокси, что у Web API, смена применяется сразу
        let fetcher = {
            let http = http.clone();
            mf_lyrics::LyricsFetcher::with_source(mf_platform::dirs::cache_subdir("lyrics"), move || http.client())
        };
        let lyrics = LyricsService::new(rt.clone(), player.clone(), fetcher);
        let devices = DevicesService::new(rt.clone(), auth.clone(), player.clone(), settings.clone());
        let social = SocialService::new(rt.clone(), auth.clone(), player.clone());
        glue::spawn_settings_glue(rt, settings.clone(), player.clone());

        let (app_requests, _) = broadcast::channel(16);
        let update = update::UpdateService::new(rt.clone(), settings.clone(), http.clone(), app_requests.clone());
        let media = if options.enable_media {
            glue::start_media_controls(rt, settings.clone(), player.clone(), app_requests.clone())
        } else {
            None
        };

        Ok(Services {
            settings,
            api,
            auth,
            library,
            catalog,
            wave,
            home,
            releases,
            lyrics,
            history,
            devices,
            social,
            update,
            player,
            app_requests,
            media: Arc::new(Mutex::new(media)),
        })
    }

    pub fn app_requests(&self) -> broadcast::Receiver<AppRequest> {
        self.app_requests.subscribe()
    }

    /// При выходе (из GUI-потока после exec): сохранить сессию и историю, остановить движок и медиаклавиши.
    /// Блокирует ≤ 2 с.
    pub fn shutdown(&self) {
        self.player.shutdown();
        self.history.flush();
        if let Some(stop) = lock(&self.media).take() {
            stop.stop(Duration::from_millis(500));
        }
    }
}

/// Mutex без паники на «отравлении».
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
