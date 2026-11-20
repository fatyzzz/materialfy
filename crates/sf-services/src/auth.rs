//! Вход (исполнитель «services-core»). Основной вход — сессия librespot: PKCE в браузере (:8898, Premium)
//! → креды librespot → PlayerHandle::connect_local; аккаунт и профиль — из сессии, Web API не нужен.
//! Web API (общий или личный Client ID, :8989) — необязательное ускорение и вход для Free; прокси; выход.
//! Вошли = есть проверенный токен Web API ИЛИ вход воспроизведения (сохранённые креды librespot).
//! Последовательность — spotifast/src/backend.rs (sign_in_source, on_account_checked, connect_engine,
//! restore_spotify_grants).
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use md3_model::{Account, AccountId};
use crate::data::session_profile;
use sf_api::{ApiError, ApiGateway, ApiSource, Operation, SessionState, TokenProvider, WebTokens};
use sf_auth::credentials::{Error as StoreError, Grant as StoredGrant, ProxyPassword, Slot};
use sf_auth::oauth::{self, StoredToken};
use sf_auth::{ProxyConfig, ProxyMode};
use sf_player::{Credentials, EngineConfig, LocalPlayback, PlayerEvent, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use mf_platform::{SettingKey, SettingValue, SettingsStore};

use crate::lock;

/// Состояние входа (Web API). as_str → Auth.state в QML.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuthState {
    /// восстановление сохранённого входа
    #[default]
    Checking,
    SignedOut,
    /// браузер открыт, ждём редирект на 127.0.0.1:8989
    AwaitingBrowser,
    /// код получен, обмен/проверка аккаунта
    Connecting,
    SignedIn,
    Error,
}

impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::SignedOut => "signedOut",
            Self::AwaitingBrowser => "awaitingBrowser",
            Self::Connecting => "connecting",
            Self::SignedIn => "signedIn",
            Self::Error => "error",
        }
    }
}

/// Воспроизведение на этом ПК (второй грант + librespot). as_str → Auth.playbackState.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackAuthState {
    /// не включено (Premium есть, разрешения нет)
    #[default]
    None,
    /// браузер открыт, ждём редирект на 127.0.0.1:8898
    Authorizing,
    /// поднимается Engine
    Connecting,
    Ready,
    Error,
    /// аккаунт Free — движок не запускается (librespot 0.8 делает exit(1))
    PremiumRequired,
}

impl PlaybackAuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Authorizing => "authorizing",
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::PremiumRequired => "premiumRequired",
        }
    }
}

/// Снимок для моста Auth.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthSnapshot {
    pub state: AuthState,
    /// по-русски: «Вход отменён в браузере», «Порт 8989 занят другой программой», …
    pub error_text: String,
    /// URL accounts.spotify.com/authorize текущего ожидания (Web API или playback), "" — нет
    pub auth_url: String,
    /// порт loopback текущего ожидания (8989 | 8898), 0 — нет
    pub auth_port: u16,
    pub playback: PlaybackAuthState,
    pub playback_error: String,
    /// стабильный код error_text для подсказок в QML (текст переведён): "" | "free" | "cancelled" | "portBusy" |
    /// "noResponse" | "accountMismatch" | "otherAccount" | "audioKey"
    pub error_code: String,
    /// то же для playback_error
    pub playback_error_code: String,
    pub account: Option<Account>,
    /// "keyring" | ""
    pub token_storage: String,
    /// личный Client ID ("" — нет) и его состояние: "none" | "authorizing" | "ready" | "error"
    pub personal_client_id: String,
    pub personal_state: String,
    /// токен общего Web API (необязателен при входе через сессию): "none" | "authorizing" | "ready" | "error"
    pub web_state: String,
    /// применение прокси: "" | "applying" | "ok" | "error"
    pub proxy_state: String,
    pub proxy_error: String,
    /// параметры движка в настройках (glue::engine_config) отличаются от запущенного; сброс — в restart_engine
    pub engine_restart_pending: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthEvent {
    SignedIn(Account),
    SignedOut,
    /// локальное воспроизведение готово (device_id)
    PlaybackReady(String),
    /// вход есть, но аккаунт Free
    PremiumRequired,
}

/// Поля формы прокси (Settings → «Сеть»); пароль не сохраняется в настройках.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyInput {
    /// "off" | "system" | "http" | "socks"
    pub mode: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

const PREMIUM_NEEDED: &str = "Playing on this computer requires Spotify Premium";
const OTHER_ACCOUNT: &str = "A different Spotify account was chosen in the browser — allow playback with the same account";
const EXPIRED: &str = "Your Spotify sign-in has expired — sign in again";
/// Вход через сессию librespot на Free: librespot такой аккаунт не обслуживает.
const FREE_ACCOUNT: &str = "This is a Spotify Free account — Materialfy can only play music with Premium";
const PORT_BUSY: &str = "Port {0} is in use by another program";
const SIGN_IN_REFUSED: &str = "Sign-in was cancelled in the browser";
const NO_RESPONSE: &str = "Spotify isn't responding — check your proxy";
const ACCOUNT_MISMATCH: &str = "Permissions were granted to different Spotify accounts";
const NETWORK_MISCONFIGURED: &str = "Network settings are invalid: {0}";
/// librespot может зависнуть на рукопожатии; у движка свой срок (75 с) с понятным текстом — этот страхующий
const CONNECT_DEADLINE: Duration = Duration::from_secs(90);
/// Чей токен воспроизведения: публичный профиль отдаётся и со scope streaming.
const ME_URL: &str = "https://api.spotify.com/v1/me";
/// Движок упал сам (сон, сеть): повторы с сохранённым грантом, не больше этих за окно.
const ENGINE_RETRY_DELAYS: [Duration; 3] = [Duration::from_secs(5), Duration::from_secs(15), Duration::from_secs(60)];
const ENGINE_RETRY_WINDOW: Duration = Duration::from_secs(600);

/// Настройки, при смене которых движок надо перезапустить.
const ENGINE_KEYS: [SettingKey; 7] = [
    SettingKey::DeviceName,
    SettingKey::Bitrate,
    SettingKey::Normalisation,
    SettingKey::Gapless,
    SettingKey::AudioDevice,
    SettingKey::AudioCache,
    SettingKey::AudioCacheMb,
];

/// Отпечаток адреса прокси движка (с логином): заметить смену, не храня пароль в открытом виде.
fn proxy_fingerprint(proxy: &ProxyConfig) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    proxy.librespot_url().map(|url| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.as_str().hash(&mut hasher);
        hasher.finish()
    })
}

/// Чем запущенный движок отличается от настроек (без tap/eq/громкости/autoplay).
#[derive(Clone, Debug, PartialEq, Eq)]
struct EngineKey {
    device_name: String,
    bitrate_kbps: u16,
    normalisation: bool,
    gapless: bool,
    audio_device: Option<String>,
    audio_cache_dir: Option<PathBuf>,
    audio_cache_limit: Option<u64>,
    /// отпечаток адреса прокси движка: в адресе может быть пароль, а ключ попадает в отладочный вывод
    proxy: Option<u64>,
}

impl EngineKey {
    fn of(config: &EngineConfig) -> Self {
        Self {
            device_name: config.device_name.clone(),
            bitrate_kbps: config.bitrate_kbps,
            normalisation: config.normalisation,
            gapless: config.gapless,
            audio_device: config.audio_device.clone(),
            audio_cache_dir: config.audio_cache_dir.clone(),
            audio_cache_limit: config.audio_cache_limit,
            proxy: proxy_fingerprint(&config.proxy),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waiting {
    Shared,
    Personal,
    Playback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Restore,
    /// вход через сессию librespot (Premium)
    Login,
    /// вход через Web API (Free, «без Premium»)
    WebLogin,
}

struct Inner {
    started: bool,
    /// выход: поздние результаты старого входа отбрасываются
    generation: u64,
    /// текущее ожидание браузера
    attempt: u64,
    waiting: Option<(Waiting, watch::Sender<bool>)>,
    /// личный вход отложен до конца общего (порт 8989 один)
    personal_pending: bool,
    last_step: Step,
    web_tokens: [Option<Arc<WebTokens>>; 2],
    playback_grant: Option<Credentials>,
    engine_busy: bool,
    /// поколение подключения движка (disable_playback отменяет идущее)
    engine_epoch: u64,
    engine_running: Option<EngineKey>,
    /// автоповторы после падения движка и время последнего
    engine_retries: usize,
    engine_retry_at: Option<Instant>,
    premium: Option<bool>,
    /// карточка аккаунта уже из сессии librespot: профиль Web API её не заменяет, только дополняет
    session_profile: bool,
    proxy: ProxyConfig,
}

pub struct AuthService {
    rt: Handle,
    settings: Arc<SettingsStore>,
    api: Arc<ApiGateway>,
    http: sf_api::Http,
    player: PlayerHandle,
    store: sf_auth::Store,
    snapshot: watch::Sender<AuthSnapshot>,
    events: broadcast::Sender<AuthEvent>,
    /// действующий прокси — для оболочки (QNetworkAccessManager картинок QML)
    proxy: watch::Sender<ProxyConfig>,
    inner: Mutex<Inner>,
    me: Weak<AuthService>,
}

fn source_index(source: ApiSource) -> usize {
    match source {
        ApiSource::Shared => 0,
        ApiSource::Personal => 1,
    }
}

fn web_slot(source: ApiSource) -> Slot {
    match source {
        ApiSource::Shared => Slot::Shared,
        ApiSource::Personal => Slot::Personal,
    }
}

/// Ошибка входа через браузер по-русски; None — вход отменён.
fn oauth_error_text(error: &anyhow::Error, port: u16) -> Option<String> {
    let text = format!("{error:#}");
    let lower = text.to_lowercase();
    // oauth::CANCELLED и прочие ошибки sf-auth — непереведённый английский
    Some(if lower.contains("cancelled") {
        return None;
    } else if lower.contains("unable to listen") {
        mf_i18n::tf(PORT_BUSY, &[&port])
    } else if lower.contains("timed out") {
        mf_i18n::t("The browser didn't respond within 10 minutes — try again")
    } else if lower.contains("refused") || lower.contains("declined") {
        mf_i18n::t(SIGN_IN_REFUSED)
    } else if lower.contains("state mismatch") {
        mf_i18n::t("The browser's response didn't match the request — try again")
    } else if lower.contains("client id") || lower.contains("application settings") {
        mf_i18n::t("Spotify didn't accept the app's Client ID")
    } else if lower.contains("token request failed") || lower.contains("answered") || lower.contains("unreadable") {
        mf_i18n::t(NO_RESPONSE)
    } else {
        mf_i18n::tf("Couldn't sign in: {0}", &[&text])
    })
}

/// Стабильный код ошибки для QML: тексты уже переведены, сравнивать их там нельзя.
/// Сравнение — с переводом на текущем языке (текст переведён тем же языком при записи в снимок).
fn error_code(text: &str) -> &'static str {
    if text.is_empty() {
        return "";
    }
    let is = |key: &str| text == mf_i18n::t(key);
    if is(FREE_ACCOUNT) {
        "free"
    } else if is(SIGN_IN_REFUSED) {
        "cancelled"
    } else if is(NO_RESPONSE) {
        "noResponse"
    } else if is(ACCOUNT_MISMATCH) || is(sf_api::gateway::ACCOUNT_MISMATCH) {
        "accountMismatch"
    } else if is(OTHER_ACCOUNT) {
        "otherAccount"
    } else if [oauth::WEB_REDIRECT_PORT, oauth::PLAYBACK_REDIRECT_PORT].iter().any(|port| text == mf_i18n::tf(PORT_BUSY, &[port])) {
        "portBusy"
    } else if text.starts_with(&mf_i18n::t(sf_player::engine::player::AUDIO_KEY_REFUSED)) {
        "audioKey"
    } else {
        ""
    }
}

/// Ошибка подключения librespot по-русски.
fn connect_error_text(text: &str) -> String {
    let lower = text.to_lowercase();
    if lower.contains("badcredentials") || lower.contains("bad credentials") {
        mf_i18n::t("Spotify rejected the playback permission — allow it again")
    } else if lower.contains("premium") {
        mf_i18n::t(PREMIUM_NEEDED)
    } else if lower.contains("dns") || lower.contains("connect") || lower.contains("resolve") {
        mf_i18n::t("Can't reach Spotify — check your network and proxy")
    } else if text.is_empty() {
        mf_i18n::t("Playback on this computer is unavailable")
    } else {
        text.to_owned()
    }
}

fn premium_of(product: Option<&str>) -> Option<bool> {
    product.map(|product| product == "premium")
}

/// Прокси из применённых настроек (+ пароль из слота).
pub(crate) fn settings_proxy(settings: &SettingsStore, password: Option<&ProxyPassword>) -> ProxyConfig {
    let mode = ProxyMode::from_setting(&settings.get_str(SettingKey::ProxyMode));
    let host = settings.get_str(SettingKey::ProxyHost);
    let port = settings.get_str(SettingKey::ProxyPort);
    let username = settings.get_str(SettingKey::ProxyUsername);
    match ProxyConfig::from_parts(mode, &host, &port, &username, "") {
        Ok(mut config) => {
            if let Some(password) = password {
                config.restore_password(password);
            }
            config
        }
        Err(error) => ProxyConfig::Invalid(error),
    }
}

impl AuthService {
    pub fn new(
        rt: Handle,
        settings: Arc<SettingsStore>,
        api: Arc<ApiGateway>,
        http: sf_api::Http,
        player: PlayerHandle,
        store: sf_auth::Store,
    ) -> Arc<Self> {
        let personal = settings.get_str(SettingKey::WebClientId).trim().to_owned();
        // до start() входа нет: с --offline оболочка start() не зовёт, «Входим…» висеть не должно
        let snapshot = AuthSnapshot {
            state: AuthState::SignedOut,
            token_storage: store.backend_name().to_owned(),
            personal_state: "none".to_owned(),
            personal_client_id: personal,
            web_state: "none".to_owned(),
            ..Default::default()
        };
        // тот же прокси, что у HTTP-клиента из Services::start; пароль подставит restore()
        let proxy = settings_proxy(&settings, None);
        let service = Arc::new_cyclic(|me| Self {
            rt,
            settings,
            api,
            http,
            player,
            store,
            snapshot: watch::channel(snapshot).0,
            events: broadcast::channel(64).0,
            proxy: watch::channel(proxy.clone()).0,
            inner: Mutex::new(Inner {
                started: false,
                generation: 0,
                attempt: 0,
                waiting: None,
                personal_pending: false,
                last_step: Step::Restore,
                web_tokens: [None, None],
                playback_grant: None,
                engine_busy: false,
                engine_epoch: 0,
                engine_running: None,
                engine_retries: 0,
                engine_retry_at: None,
                premium: None,
                session_profile: false,
                proxy,
            }),
            me: me.clone(),
        });
        service.spawn_watchers();
        service
    }

    fn spawn_watchers(&self) {
        let mut keys = self.settings.subscribe();
        let me = self.me.clone();
        self.rt.spawn(async move {
            loop {
                match keys.recv().await {
                    Ok(key) if ENGINE_KEYS.contains(&key) => {
                        let Some(service) = me.upgrade() else { return };
                        service.recheck_engine_settings();
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some(service) = me.upgrade() else { return };
                        service.recheck_engine_settings();
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
                    Ok(PlayerEvent::LocalChanged) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some(service) = me.upgrade() else { return };
                        service.mirror_local();
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn update(&self, change: impl FnOnce(&mut AuthSnapshot)) {
        self.snapshot.send_if_modified(|snapshot| {
            let before = snapshot.clone();
            change(snapshot);
            if snapshot.error_text != before.error_text {
                snapshot.error_code = error_code(&snapshot.error_text).to_owned();
            }
            if snapshot.playback_error != before.playback_error {
                snapshot.playback_error_code = error_code(&snapshot.playback_error).to_owned();
            }
            *snapshot != before
        });
    }

    fn emit(&self, event: AuthEvent) {
        let _ = self.events.send(event);
    }

    fn generation(&self) -> u64 {
        lock(&self.inner).generation
    }

    fn current(&self, generation: u64) -> bool {
        lock(&self.inner).generation == generation
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

    fn open_browser(&self, url: String) {
        self.rt.spawn_blocking(move || {
            if let Err(error) = sf_auth::open_browser(&url) {
                tracing::warn!("could not open the browser: {error:#}");
            }
        });
    }

    /// Ошибка хранилища секретов: вход не запомнится (без Debug секретов).
    fn storage_notice(&self) -> Arc<dyn Fn(StoreError) + Send + Sync> {
        let me = self.me.clone();
        Arc::new(move |error| {
            if error == StoreError::Stale {
                return;
            }
            tracing::warn!("credential store: {error:#}");
            if let Some(service) = me.upgrade()
                && matches!(error, StoreError::Unavailable | StoreError::Locked | StoreError::Verification)
            {
                service.update(|s| s.token_storage.clear());
            }
        })
    }

    fn delete_slot(&self, slot: Slot) {
        let pending = self.store.lease(slot).delete();
        let notice = self.storage_notice();
        self.rt.spawn(async move {
            if let Err(error) = pending.await {
                notice(error);
            }
        });
    }

    fn cancel_waiting(&self) -> Option<Waiting> {
        let mut inner = lock(&self.inner);
        inner.attempt += 1;
        let (kind, cancel) = inner.waiting.take()?;
        let _ = cancel.send(true);
        Some(kind)
    }

    // --- вход ---------------------------------------------------------------------------------

    /// Восстановить сохранённые гранты (прокси → shared/personal → playback); нет — signedOut.
    pub fn start(&self) {
        {
            let mut inner = lock(&self.inner);
            if inner.started {
                return;
            }
            inner.started = true;
            inner.last_step = Step::Restore;
        }
        self.update(|s| {
            s.state = AuthState::Checking;
            s.error_text.clear();
        });
        if self.settings.get_bool(SettingKey::PlaybackAuthorized) {
            // «Играть» до подключения движка ждёт его, а не просит выбрать устройство
            self.player.expect_local(true);
        }
        self.spawn(|service| async move { service.restore().await });
    }

    async fn restore(self: Arc<Self>) {
        let generation = self.generation();
        let password = match self.store.lease(Slot::Proxy).load().await {
            Ok(loaded) => {
                if let Some(warning) = loaded.warning {
                    tracing::warn!("proxy password: {warning}");
                }
                match loaded.grant {
                    Some(StoredGrant::Proxy(password)) => Some(password),
                    _ => None,
                }
            }
            Err(error) => {
                tracing::warn!("proxy password not read: {error:#}");
                None
            }
        };
        self.install_proxy(settings_proxy(&self.settings, password.as_ref()));

        let personal_id = self.settings.get_str(SettingKey::WebClientId).trim().to_owned();
        self.api.set_state(ApiSource::Shared, SessionState::Authorizing);
        if !personal_id.is_empty() {
            self.api.set_state(ApiSource::Personal, SessionState::Authorizing);
        }
        let mut verifying = false;
        let mut playback_grant = false;
        // сначала вход воспроизведения: с ним приложение открывается сразу, не дожидаясь Web API
        for slot in [Slot::Playback, Slot::Shared, Slot::Personal] {
            let lease = self.store.lease(slot);
            let loaded = lease.load().await;
            if !self.current(generation) {
                return;
            }
            let grant = match loaded {
                Ok(loaded) => {
                    if let Some(warning) = loaded.warning {
                        tracing::warn!("saved sign-in: {warning}");
                        self.update(|s| s.error_text = warning.to_string());
                    }
                    loaded.grant
                }
                Err(error) => {
                    tracing::warn!("saved sign-in not read: {error:#}");
                    self.update(|s| s.error_text = error.to_string());
                    None
                }
            };
            match (slot, grant) {
                (Slot::Playback, Some(StoredGrant::Playback(credentials))) => {
                    playback_grant = true;
                    let username = credentials.username.clone().filter(|name| !name.is_empty());
                    lock(&self.inner).playback_grant = Some(credentials);
                    if let Some(username) = username {
                        // грант есть — воспроизведение разрешено (без него не будет и входа)
                        self.settings.set(SettingKey::PlaybackAuthorized, SettingValue::Bool(true));
                        self.player.expect_local(true);
                        self.api.set_session_account(Some(AccountId::new(username.clone())));
                        self.show_signed_in(sf_api::map::account(&remembered_user(&username)), false);
                    }
                    self.resume_engine();
                }
                (Slot::Shared, Some(StoredGrant::Web(token))) if token.has_scopes(oauth::WEB_SCOPES) => {
                    verifying = true;
                    self.update(|s| {
                        if s.state != AuthState::SignedIn {
                            s.state = AuthState::Checking;
                        }
                        s.web_state = "authorizing".to_owned();
                    });
                    self.spawn(move |service| service.verify(ApiSource::Shared, token, lease, generation));
                }
                (Slot::Personal, Some(StoredGrant::Web(token)))
                    if token.client_id == personal_id && token.has_scopes(oauth::WEB_SCOPES) =>
                {
                    verifying = true;
                    self.update(|s| s.personal_state = "authorizing".to_owned());
                    self.spawn(move |service| service.verify(ApiSource::Personal, token, lease, generation));
                }
                (Slot::Shared, Some(StoredGrant::Web(_))) => {
                    self.api.clear(ApiSource::Shared);
                    self.update(|s| {
                        s.error_text = mf_i18n::t("Spotify permissions changed — sign in again");
                        s.web_state = "error".to_owned();
                    });
                }
                (Slot::Personal, Some(StoredGrant::Web(_))) => {
                    self.api.clear(ApiSource::Personal);
                    if !personal_id.is_empty() {
                        self.update(|s| {
                            s.personal_state = "error".to_owned();
                        });
                    }
                }
                (Slot::Shared, _) => self.api.clear(ApiSource::Shared),
                (Slot::Personal, _) => self.api.clear(ApiSource::Personal),
                _ => {}
            }
        }
        if !playback_grant && self.current(generation) {
            self.player.expect_local(false);
        }
        if !verifying && self.current(generation) {
            self.update(|s| {
                if s.state == AuthState::Checking {
                    s.state = AuthState::SignedOut;
                }
            });
        }
    }

    /// Проверить грант через GET /me; только после этого он пишется в хранилище.
    /// Сохранённый грант при известном аккаунте (профиль прошлой проверки того же приложения или вход
    /// воспроизведения) ставится сразу, а /me лишь подтверждает его в фоне: запуск не ждёт Web API, а
    /// лимит частоты общего ключа (429) вход не ломает. Истёкший токен всплывёт на /me или первом запросе.
    async fn verify(self: Arc<Self>, source: ApiSource, token: StoredToken, lease: sf_auth::Lease, generation: u64) {
        let client_id = token.client_id.clone();
        let known = load_profile(source, &client_id)
            .or_else(|| self.playback_account().map(|id| sf_api::models::User { id, ..Default::default() }));
        let tokens = WebTokens::new(self.http.clone(), token, lease, source, self.storage_notice());
        lock(&self.inner).web_tokens[source_index(source)] = Some(tokens.clone());
        self.api.begin_verification(source, TokenProvider::Web(tokens.clone()));
        let client = self.api.verification_client(source);
        let installed = match known {
            Some(user) => {
                self.on_verified(source, tokens.clone(), user);
                if !matches!(self.api.state(source), SessionState::Ready { .. }) {
                    return;
                }
                true
            }
            None => false,
        };
        let mut wait = Duration::from_secs(2);
        let result = loop {
            if !self.current(generation) {
                return;
            }
            match client.me().await {
                Ok(user) => {
                    save_profile(source, &client_id, &user);
                    if installed {
                        self.on_confirmed(source, user);
                        return;
                    }
                    break Ok(user);
                }
                Err(error @ ApiError::SignInExpired { .. }) => break Err(error),
                Err(ApiError::RateLimited) if installed => {
                    tracing::info!("sign-in check: Spotify rate-limited, using the profile from the previous check");
                    return;
                }
                Err(error) if error.status().is_some_and(|status| status < 500) => break Err(error),
                Err(error) => {
                    tracing::warn!("sign-in check will be retried: {error:#}");
                    if source == ApiSource::Shared && !installed {
                        self.update(|s| s.error_text = mf_i18n::t(NO_RESPONSE));
                    }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(60));
                    let state = self.api.state(source);
                    let still = if installed {
                        matches!(state, SessionState::Ready { .. })
                    } else {
                        matches!(state, SessionState::Authorizing)
                    };
                    if !still {
                        return;
                    }
                }
            }
        };
        if !self.current(generation) {
            return;
        }
        match result {
            Ok(user) => self.on_verified(source, tokens, user),
            Err(error) => self.on_verification_failed(source, error),
        }
    }

    fn on_verified(&self, source: ApiSource, tokens: Arc<WebTokens>, user: sf_api::models::User) {
        if user.id.is_empty() {
            self.on_verification_failed(source, ApiError::NotSignedIn);
            return;
        }
        if !matches!(self.api.state(source), SessionState::Authorizing) {
            return;
        }
        if self.api.install(source, AccountId::new(user.id.clone())).is_err() {
            self.api.clear(source);
            lock(&self.inner).web_tokens[source_index(source)] = None;
            self.on_other_web_account(source);
            return;
        }
        let notice = self.storage_notice();
        self.rt.spawn(async move {
            if let Err(error) = tokens.remember().await {
                notice(error);
            }
        });
        let account = sf_api::map::account(&user);
        let premium = premium_of(user.product.as_deref());
        sf_player::engine::player::note_account_product(user.product.as_deref());
        match source {
            ApiSource::Personal => self.update(|s| s.personal_state = "ready".to_owned()),
            ApiSource::Shared => self.update(|s| s.web_state = "ready".to_owned()),
        }
        let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
        if source == ApiSource::Shared || !was_signed_in {
            self.show_signed_in(account, false);
            self.on_account_checked(premium);
        }
        let start_personal = source == ApiSource::Shared && std::mem::take(&mut lock(&self.inner).personal_pending);
        if start_personal {
            self.begin_web(ApiSource::Personal);
        }
    }

    fn on_verification_failed(&self, source: ApiSource, error: ApiError) {
        if matches!(error, ApiError::SignInExpired { .. }) {
            self.forget_web_grant(source);
        } else {
            self.api.clear(source);
            lock(&self.inner).web_tokens[source_index(source)] = None;
        }
        let text = match &error {
            ApiError::SignInExpired { .. } => mf_i18n::t(EXPIRED),
            other => crate::data::route::api_error_text(other),
        };
        let expired = matches!(error, ApiError::SignInExpired { .. });
        match source {
            // вход через сессию остаётся: без Web API работает почти всё
            ApiSource::Shared if self.session_signed_in() => self.update(|s| {
                s.web_state = if expired { "none" } else { "error" }.to_owned();
                s.error_text = text;
            }),
            ApiSource::Shared => {
                self.player.expect_local(false);
                self.update(|s| {
                    s.state = if expired { AuthState::SignedOut } else { AuthState::Error };
                    s.error_text = text;
                    s.account = None;
                    s.web_state = if expired { "none" } else { "error" }.to_owned();
                });
            }
            ApiSource::Personal => {
                self.update(|s| s.personal_state = "error".to_owned());
                if !matches!(self.api.state(ApiSource::Shared), SessionState::Ready { .. }) {
                    self.update(|s| {
                        if s.state != AuthState::SignedIn {
                            s.state = AuthState::SignedOut;
                        }
                    });
                }
            }
        }
    }

    fn forget_web_grant(&self, source: ApiSource) {
        let slot = web_slot(source);
        if let Err(error) = self.store.revoke(slot) {
            tracing::warn!("grant revocation: {error:#}");
        }
        self.delete_slot(slot);
        let _ = std::fs::remove_file(profile_file(source));
        lock(&self.inner).web_tokens[source_index(source)] = None;
        self.api.clear(source);
    }

    /// Ошибка запроса любого сервиса: истёкший вход сбрасывает его грант.
    pub fn note_api_error(&self, error: &ApiError) {
        let ApiError::SignInExpired { api_source } = error else { return };
        let source = *api_source;
        if lock(&self.inner).web_tokens[source_index(source)].is_none() {
            return;
        }
        self.forget_web_grant(source);
        match source {
            ApiSource::Personal => self.update(|s| s.personal_state = "error".to_owned()),
            ApiSource::Shared => {
                self.update(|s| s.web_state = "none".to_owned());
                if self.api.personal_ready() || self.session_signed_in() {
                    return;
                }
                let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
                self.update(|s| {
                    s.state = AuthState::SignedOut;
                    s.error_text = mf_i18n::t(EXPIRED);
                    s.account = None;
                });
                if was_signed_in {
                    self.emit(AuthEvent::SignedOut);
                }
            }
        }
    }

    /// PKCE Web API (общее или личное приложение), ожидание на :8989.
    fn begin_web(&self, source: ApiSource) {
        let http = match self.http.client() {
            Ok(http) => http,
            Err(error) => {
                match source {
                    ApiSource::Shared => self.update(|s| {
                        if s.state != AuthState::SignedIn {
                            s.state = AuthState::Error;
                        }
                        s.web_state = "error".to_owned();
                        s.error_text = mf_i18n::tf(NETWORK_MISCONFIGURED, &[&error]);
                    }),
                    ApiSource::Personal => self.update(|s| s.personal_state = "error".to_owned()),
                }
                return;
            }
        };
        let grant = match source {
            ApiSource::Shared => oauth::Grant::shared_web_api(),
            ApiSource::Personal => {
                let id = self.settings.get_str(SettingKey::WebClientId);
                match oauth::Grant::personal_web_api(&id) {
                    Ok(grant) => grant,
                    Err(_) => {
                        self.update(|s| s.personal_state = "error".to_owned());
                        return;
                    }
                }
            }
        };
        self.cancel_waiting();
        let slot = web_slot(source);
        self.store.invalidate(slot);
        let lease = self.store.lease(slot);
        let flow = oauth::begin(grant.clone());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (attempt, generation) = {
            let mut inner = lock(&self.inner);
            inner.attempt += 1;
            inner.waiting = Some((
                if source == ApiSource::Shared { Waiting::Shared } else { Waiting::Personal },
                cancel_tx,
            ));
            if source == ApiSource::Shared {
                inner.last_step = Step::WebLogin;
            }
            (inner.attempt, inner.generation)
        };
        self.api.set_state(source, SessionState::Authorizing);
        let url = flow.url.clone();
        self.update(|s| {
            s.auth_url = url.clone();
            s.auth_port = grant.redirect_port;
            match source {
                ApiSource::Shared => {
                    if s.state != AuthState::SignedIn {
                        s.state = AuthState::AwaitingBrowser;
                    }
                    s.web_state = "authorizing".to_owned();
                    s.error_text.clear();
                }
                ApiSource::Personal => s.personal_state = "authorizing".to_owned(),
            }
        });
        self.open_browser(url);
        self.spawn(move |service| async move {
            let port = grant.redirect_port;
            let code = oauth::wait_for_code(port, &flow.state, cancel_rx).await;
            if lock(&service.inner).attempt != attempt || !service.current(generation) {
                return;
            }
            if source == ApiSource::Shared && code.is_ok() {
                service.update(|s| {
                    if s.state != AuthState::SignedIn {
                        s.state = AuthState::Connecting;
                    }
                });
            }
            let result = async {
                let code = code?;
                let response = oauth::exchange_code(&http, &grant, &code, &flow.verifier).await?;
                StoredToken::from_response(&grant.client_id, response, None)
            }
            .await;
            {
                let mut inner = lock(&service.inner);
                if inner.attempt == attempt {
                    inner.waiting = None;
                }
            }
            service.update(|s| {
                s.auth_url.clear();
                s.auth_port = 0;
            });
            match result {
                Ok(token) => service.verify(source, token, lease, generation).await,
                Err(error) => {
                    let text = oauth_error_text(&error, port);
                    if !matches!(service.api.state(source), SessionState::Ready { .. }) {
                        service.api.clear(source);
                    }
                    match source {
                        ApiSource::Shared => service.update(|s| {
                            let signed_in = s.state == AuthState::SignedIn;
                            s.web_state = if text.is_some() { "error" } else { "none" }.to_owned();
                            match text {
                                Some(text) => {
                                    if !signed_in {
                                        s.state = AuthState::Error;
                                    }
                                    s.error_text = text;
                                }
                                None => {
                                    if !signed_in {
                                        s.state = AuthState::SignedOut;
                                    }
                                    s.error_text.clear();
                                }
                            }
                        }),
                        ApiSource::Personal => service.update(|s| {
                            s.personal_state = if text.is_some() { "error" } else { "none" }.to_owned();
                        }),
                    }
                    if source == ApiSource::Shared && std::mem::take(&mut lock(&service.inner).personal_pending) {
                        service.update(|s| s.personal_state = "none".to_owned());
                    }
                }
            }
        });
    }

    /// «Войти через Spotify» (Premium): один вход — сессия librespot (:8898), с ней и воспроизведение здесь.
    pub fn start_login(&self) {
        lock(&self.inner).started = true;
        self.authorize_playback();
    }

    /// «Войти без Premium» / «Ускорить медиатеку»: PKCE общего приложения Web API, ожидание на :8989.
    pub fn start_web_login(&self) {
        lock(&self.inner).started = true;
        self.begin_web(ApiSource::Shared);
    }

    /// «Открыть снова» — тот же auth_url в браузере.
    pub fn reopen_browser(&self) {
        let url = self.snapshot.borrow().auth_url.clone();
        if !url.is_empty() {
            self.open_browser(url);
        }
    }

    /// Отменить ожидание браузера (Web API или playback).
    pub fn cancel_login(&self) {
        let waiting = {
            let inner = lock(&self.inner);
            inner.waiting.as_ref().map(|(kind, cancel)| {
                let _ = cancel.send(true);
                *kind
            })
        };
        match waiting {
            Some(Waiting::Shared) | None => self.update(|s| {
                if matches!(s.state, AuthState::AwaitingBrowser | AuthState::Connecting) {
                    s.state = AuthState::SignedOut;
                }
                if s.web_state == "authorizing" {
                    s.web_state = "none".to_owned();
                }
                s.auth_url.clear();
                s.auth_port = 0;
            }),
            Some(Waiting::Personal) => self.update(|s| {
                s.personal_state = "none".to_owned();
                s.auth_url.clear();
                s.auth_port = 0;
            }),
            Some(Waiting::Playback) => self.update(|s| {
                if matches!(s.state, AuthState::AwaitingBrowser | AuthState::Connecting) {
                    s.state = AuthState::SignedOut;
                }
                s.playback = PlaybackAuthState::None;
                s.auth_url.clear();
                s.auth_port = 0;
            }),
        }
    }

    /// Выход: отзыв всех трёх слотов, останов движка, очистка шлюза; кэш не трогается.
    pub fn sign_out(&self) {
        self.cancel_waiting();
        {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            inner.engine_epoch += 1;
            inner.web_tokens = [None, None];
            inner.playback_grant = None;
            inner.engine_busy = false;
            inner.engine_running = None;
            inner.premium = None;
            inner.session_profile = false;
            inner.personal_pending = false;
            inner.started = true;
        }
        if let Err(error) = self.store.revoke_spotify() {
            tracing::warn!("sign-in revocation: {error:#}");
        }
        self.api.clear_all();
        sf_player::engine::player::note_account_product(None);
        self.player.disconnect_local();
        self.player.reset_account();
        for slot in Slot::SPOTIFY {
            self.delete_slot(slot);
        }
        self.settings.set(SettingKey::PlaybackAuthorized, SettingValue::Bool(false));
        let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
        self.update(|s| {
            s.state = AuthState::SignedOut;
            s.error_text.clear();
            s.auth_url.clear();
            s.auth_port = 0;
            s.account = None;
            s.playback = PlaybackAuthState::None;
            s.playback_error.clear();
            s.personal_state = "none".to_owned();
            s.web_state = "none".to_owned();
            s.engine_restart_pending = false;
        });
        if was_signed_in {
            self.emit(AuthEvent::SignedOut);
        }
    }

    /// Повтор после ошибки (= start или повтор последнего шага).
    pub fn retry(&self) {
        let snapshot = self.snapshot();
        if snapshot.state == AuthState::SignedIn {
            if snapshot.playback == PlaybackAuthState::Error {
                let has_grant = lock(&self.inner).playback_grant.is_some();
                if has_grant {
                    self.update(|s| s.playback = PlaybackAuthState::None);
                    self.resume_engine();
                } else {
                    self.authorize_playback();
                }
            }
            return;
        }
        let step = lock(&self.inner).last_step;
        match step {
            Step::Login => self.start_login(),
            Step::WebLogin => self.start_web_login(),
            Step::Restore => {
                lock(&self.inner).started = false;
                self.start();
            }
        }
    }

    /// Перечитать профиль (Premium мог появиться — «Проверить снова»): из сессии, если она есть, иначе
    /// /me — только при уже вставшем Web API (не ждать его).
    pub fn refresh_profile(&self) {
        if self.player.session().is_some() {
            self.refresh_session_profile();
            return;
        }
        if !matches!(self.api.state(ApiSource::Shared), SessionState::Ready { .. }) {
            return;
        }
        let generation = self.generation();
        self.spawn(move |service| async move {
            let result = async { service.api.client_for(Operation::CanonicalAccount).await?.me().await }.await;
            if !service.current(generation) {
                return;
            }
            match result {
                Ok(user) => service.on_confirmed(ApiSource::Shared, user),
                Err(error) => {
                    tracing::warn!("profile not refreshed: {error:#}");
                    service.note_api_error(&error);
                }
            }
        });
    }

    // --- воспроизведение на этом ПК ------------------------------------------------------------

    /// librespot 0.8 на Free делает exit(1): движок только для Premium (неизвестно — как раньше, пробуем).
    fn on_account_checked(&self, premium: Option<bool>) {
        lock(&self.inner).premium = premium;
        if premium == Some(false) {
            let was_running = {
                let mut inner = lock(&self.inner);
                inner.engine_epoch += 1;
                inner.engine_busy = false;
                inner.engine_running.take().is_some()
            };
            if was_running {
                self.player.disconnect_local();
            }
            self.player.expect_local(false);
            self.update(|s| {
                s.playback = PlaybackAuthState::PremiumRequired;
                s.playback_error = mf_i18n::t(PREMIUM_NEEDED);
                s.engine_restart_pending = false;
            });
            self.emit(AuthEvent::PremiumRequired);
            return;
        }
        self.update(|s| {
            if s.playback == PlaybackAuthState::PremiumRequired {
                s.playback = PlaybackAuthState::None;
                s.playback_error.clear();
            }
        });
        self.resume_engine();
    }

    /// Креды воспроизведения того же аккаунта, что и Web API; без Web API — любые (аккаунт задаёт сессия).
    fn account_matches(&self, credentials: &Credentials) -> bool {
        let Some(account) = self.api.web_account() else {
            return true;
        };
        credentials.username.as_deref().filter(|name| !name.is_empty()).is_some_and(|name| account.as_str() == name)
    }

    /// Поднять движок с сохранёнными кредами, если воспроизведение разрешено.
    /// Аккаунт сохранённого входа воспроизведения (Credentials.username), если он есть.
    fn playback_account(&self) -> Option<String> {
        lock(&self.inner)
            .playback_grant
            .as_ref()
            .and_then(|credentials| credentials.username.clone())
            .filter(|name| !name.is_empty())
    }

    fn resume_engine(&self) {
        if !self.settings.get_bool(SettingKey::PlaybackAuthorized) {
            return;
        }
        let grant = {
            let inner = lock(&self.inner);
            if inner.engine_busy || inner.engine_running.is_some() || inner.premium == Some(false) {
                return;
            }
            inner.playback_grant.clone()
        };
        let Some(credentials) = grant else { return };
        if !self.account_matches(&credentials) {
            self.player.expect_local(false);
            lock(&self.inner).playback_grant = None;
            self.update(|s| {
                s.playback = PlaybackAuthState::None;
                s.playback_error = mf_i18n::t(OTHER_ACCOUNT);
            });
            return;
        }
        self.connect_engine(credentials);
    }

    fn connect_engine(&self, credentials: Credentials) {
        if let Err(error) = self.http.client() {
            self.player.expect_local(false);
            self.update(|s| {
                s.playback = PlaybackAuthState::Error;
                s.playback_error = mf_i18n::tf(NETWORK_MISCONFIGURED, &[&error]);
            });
            return;
        }
        if self.refuse_unsupported_proxy() {
            // текст — копией: borrow() держит замок снимка, а session_login_failed в него пишет (взаимная блокировка)
            let text = self.snapshot.borrow().playback_error.clone();
            self.session_login_failed(&text);
            return;
        }
        let (epoch, generation, proxy) = {
            let mut inner = lock(&self.inner);
            if inner.engine_busy {
                return;
            }
            if inner.premium == Some(false) {
                drop(inner);
                self.update(|s| s.playback = PlaybackAuthState::PremiumRequired);
                return;
            }
            inner.engine_busy = true;
            inner.engine_epoch += 1;
            (inner.engine_epoch, inner.generation, inner.proxy.clone())
        };
        self.update(|s| {
            s.playback = PlaybackAuthState::Connecting;
            s.playback_error.clear();
        });
        let config = crate::glue::engine_config(&self.settings, proxy);
        let key = EngineKey::of(&config);
        self.spawn(move |service| async move {
            let result = tokio::time::timeout(CONNECT_DEADLINE, service.player.connect_local(credentials, config)).await;
            let stale = {
                let mut inner = lock(&service.inner);
                let stale = inner.engine_epoch != epoch || inner.generation != generation;
                if !stale {
                    inner.engine_busy = false;
                }
                stale
            };
            if stale {
                if matches!(result, Ok(Ok(_))) {
                    service.player.disconnect_local();
                }
                return;
            }
            match result {
                Ok(Ok(started)) => {
                    if let Some(reusable) = started.reusable {
                        if !service.account_matches(&reusable) {
                            lock(&service.inner).engine_running = None;
                            service.player.disconnect_local();
                            service.update(|s| {
                                s.playback = PlaybackAuthState::Error;
                                s.playback_error = mf_i18n::t(OTHER_ACCOUNT);
                            });
                            service.session_login_failed(&mf_i18n::t(OTHER_ACCOUNT));
                            return;
                        }
                        lock(&service.inner).playback_grant = Some(reusable.clone());
                        let pending = service.store.lease(Slot::Playback).save(StoredGrant::Playback(reusable));
                        let notice = service.storage_notice();
                        service.rt.spawn(async move {
                            if let Err(error) = pending.await {
                                notice(error);
                            }
                        });
                    }
                    lock(&service.inner).engine_running = Some(key);
                    service.settings.set(SettingKey::PlaybackAuthorized, SettingValue::Bool(true));
                    service.update(|s| {
                        s.playback = PlaybackAuthState::Ready;
                        s.playback_error.clear();
                    });
                    service.on_session_up();
                    service.recheck_engine_settings();
                    service.emit(AuthEvent::PlaybackReady(started.device_id));
                }
                Ok(Err(error)) => {
                    tracing::warn!("engine failed to connect: {error:#}");
                    // прежний движок (если был) host.connect уже остановил
                    lock(&service.inner).engine_running = None;
                    let lower = error.to_lowercase();
                    let rejected = lower.contains("badcredentials") || lower.contains("bad credentials");
                    if rejected {
                        lock(&service.inner).playback_grant = None;
                        let _ = service.store.revoke(Slot::Playback);
                        service.delete_slot(Slot::Playback);
                        service.on_session_lost();
                    }
                    let premium = lower.contains("premium");
                    service.update(|s| {
                        s.playback =
                            if premium { PlaybackAuthState::PremiumRequired } else { PlaybackAuthState::Error };
                        s.playback_error = connect_error_text(&error);
                    });
                    service.session_login_failed(&if premium { mf_i18n::t(FREE_ACCOUNT) } else { connect_error_text(&error) });
                    if !rejected && !premium {
                        service.continue_engine_retries();
                    }
                }
                Err(_) => {
                    lock(&service.inner).engine_running = None;
                    service.player.expect_local(false);
                    let text = &mf_i18n::t("Spotify didn't respond in time — try again");
                    service.update(|s| {
                        s.playback = PlaybackAuthState::Error;
                        s.playback_error = text.to_owned();
                    });
                    service.session_login_failed(text);
                }
            }
        });
    }

    /// Состояние локального движка от плеера (переподключение, отказ).
    fn mirror_local(&self) {
        let local = self.player.local_playback();
        let running = lock(&self.inner).engine_running.is_some();
        if !running {
            return;
        }
        match local {
            LocalPlayback::Failed(text) => {
                // перезапуск или переподключение не удались: движка больше нет, повтор — по сохранённому гранту
                if !self.forget_engine() {
                    return;
                }
                self.update(|s| {
                    s.playback = PlaybackAuthState::Error;
                    s.playback_error = connect_error_text(&text);
                });
                self.schedule_engine_retry();
            }
            LocalPlayback::Ready { .. } => self.update(|s| {
                if s.playback == PlaybackAuthState::Error {
                    s.playback = PlaybackAuthState::Ready;
                    s.playback_error.clear();
                }
            }),
            LocalPlayback::Unavailable => {
                self.forget_engine();
            }
            LocalPlayback::Connecting => {}
        }
    }

    /// Движок пропал не по нашей команде; false — идёт своё подключение, оно и разберётся.
    fn forget_engine(&self) -> bool {
        let mut inner = lock(&self.inner);
        if inner.engine_busy {
            return false;
        }
        inner.engine_running = None;
        true
    }

    /// Отложенный повтор подключения с сохранённым грантом (5/15/60 с, не больше трёх за 10 минут).
    fn schedule_engine_retry(&self) {
        let (delay, epoch, generation) = {
            let mut inner = lock(&self.inner);
            if inner.playback_grant.is_none() {
                return;
            }
            let now = Instant::now();
            if inner.engine_retry_at.is_none_or(|at| now.duration_since(at) > ENGINE_RETRY_WINDOW) {
                inner.engine_retries = 0;
            }
            let Some(delay) = ENGINE_RETRY_DELAYS.get(inner.engine_retries).copied() else {
                return;
            };
            inner.engine_retries += 1;
            inner.engine_retry_at = Some(now);
            (delay, inner.engine_epoch, inner.generation)
        };
        self.spawn(move |service| async move {
            tokio::time::sleep(delay).await;
            {
                let inner = lock(&service.inner);
                if inner.engine_epoch != epoch || inner.generation != generation || inner.engine_running.is_some() {
                    return;
                }
            }
            if service.snapshot.borrow().playback != PlaybackAuthState::Error {
                return;
            }
            service.resume_engine();
        });
    }

    /// Неудачный автоповтор — следующий, пока серия не исчерпана.
    fn continue_engine_retries(&self) {
        let chained = {
            let inner = lock(&self.inner);
            inner.engine_retries > 0 && inner.engine_retry_at.is_some_and(|at| at.elapsed() <= ENGINE_RETRY_WINDOW)
        };
        if chained {
            self.schedule_engine_retry();
        }
    }

    /// SOCKS5 или HTTP с паролем librespot не умеет: не запускать его мимо прокси.
    fn refuse_unsupported_proxy(&self) -> bool {
        let Some(text) = lock(&self.inner).proxy.engine_unsupported() else {
            return false;
        };
        self.player.expect_local(false);
        self.update(|s| {
            s.playback = PlaybackAuthState::Error;
            s.playback_error = text.to_owned();
        });
        true
    }

    /// Вход через сессию и «Разрешить воспроизведение»: PKCE с PLAYBACK_CLIENT_ID (:8898, всегда show_dialog)
    /// → проверка Premium по токену → connect_local. Без входа Web API это и есть вход в приложение.
    pub fn authorize_playback(&self) {
        let premium = lock(&self.inner).premium;
        if premium == Some(false) {
            self.update(|s| {
                s.playback = PlaybackAuthState::PremiumRequired;
                s.playback_error = mf_i18n::t(PREMIUM_NEEDED);
            });
            self.emit(AuthEvent::PremiumRequired);
            return;
        }
        if lock(&self.inner).engine_busy {
            return;
        }
        if self.refuse_unsupported_proxy() {
            // текст — копией: borrow() держит замок снимка, а session_login_failed в него пишет (взаимная блокировка)
            let text = self.snapshot.borrow().playback_error.clone();
            self.session_login_failed(&text);
            return;
        }
        let http = match self.http.client() {
            Ok(http) => http,
            Err(error) => {
                let text = mf_i18n::tf(NETWORK_MISCONFIGURED, &[&error]);
                self.update(|s| {
                    s.playback = PlaybackAuthState::Error;
                    s.playback_error = text.clone();
                });
                self.session_login_failed(&text);
                return;
            }
        };
        self.cancel_waiting();
        self.store.invalidate(Slot::Playback);
        let grant = oauth::Grant::playback();
        let flow = oauth::begin(grant.clone());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
        let (attempt, generation) = {
            let mut inner = lock(&self.inner);
            inner.attempt += 1;
            inner.waiting = Some((Waiting::Playback, cancel_tx));
            if !signed_in {
                inner.last_step = Step::Login;
            }
            (inner.attempt, inner.generation)
        };
        let url = flow.url.clone();
        self.update(|s| {
            s.playback = PlaybackAuthState::Authorizing;
            s.playback_error.clear();
            s.auth_url = url.clone();
            s.auth_port = grant.redirect_port;
            if s.state != AuthState::SignedIn {
                s.state = AuthState::AwaitingBrowser;
                s.error_text.clear();
            }
        });
        self.open_browser(url);
        self.spawn(move |service| async move {
            let port = grant.redirect_port;
            let result = async {
                let code = oauth::wait_for_code(port, &flow.state, cancel_rx).await?;
                service.update(|s| {
                    if s.state == AuthState::AwaitingBrowser {
                        s.state = AuthState::Connecting;
                    }
                });
                oauth::exchange_code(&http, &grant, &code, &flow.verifier).await
            }
            .await;
            let personal = {
                let mut inner = lock(&service.inner);
                if inner.attempt != attempt || inner.generation != generation {
                    return;
                }
                inner.waiting = None;
                std::mem::take(&mut inner.personal_pending)
            };
            service.update(|s| {
                s.auth_url.clear();
                s.auth_port = 0;
            });
            if personal {
                service.begin_web(ApiSource::Personal);
            }
            match result {
                Ok(token) => {
                    // librespot на Free завершает процесс: тип аккаунта — до его запуска (spclient, не Web API)
                    let product = session_profile::product_of_token(&http, &token.access_token).await;
                    if !service.current(generation) {
                        return;
                    }
                    let web = service.api.web_account();
                    if product.is_some() || web.is_none() {
                        sf_player::engine::player::note_account_product(product.as_deref());
                    }
                    if product.as_deref().is_some_and(|product| product != "premium") {
                        service.on_free_session_login();
                        return;
                    }
                    // вход Web API уже есть: токен другого аккаунта в движок не пускаем
                    if let Some(account) = &web
                        && let Some(owner) = token_owner(&http, &token.access_token).await
                        && owner != account.as_str()
                    {
                        service.player.expect_local(false);
                        service.update(|s| {
                            s.playback = PlaybackAuthState::Error;
                            s.playback_error = mf_i18n::t(OTHER_ACCOUNT);
                        });
                        return;
                    }
                    if !service.current(generation) {
                        return;
                    }
                    let mut credentials = Credentials::with_access_token(token.access_token);
                    credentials.username = web.map(|account| account.as_str().to_owned());
                    service.connect_engine(credentials);
                }
                Err(error) => match oauth_error_text(&error, port) {
                    Some(text) => {
                        service.update(|s| {
                            s.playback = PlaybackAuthState::Error;
                            s.playback_error = text.clone();
                        });
                        service.session_login_failed(&text);
                    }
                    None => service.update(|s| {
                        s.playback = PlaybackAuthState::None;
                        if matches!(s.state, AuthState::AwaitingBrowser | AuthState::Connecting) {
                            s.state = AuthState::SignedOut;
                        }
                    }),
                },
            }
        });
    }

    /// Вход через сессию на Free: librespot не запускать. Вошли через Web API — «Только пульт»;
    /// не вошли — ошибка входа с предложением войти без Premium (Web API).
    fn on_free_session_login(&self) {
        if self.snapshot.borrow().state == AuthState::SignedIn {
            self.on_account_checked(Some(false));
            return;
        }
        self.player.expect_local(false);
        self.update(|s| {
            s.state = AuthState::Error;
            s.error_text = mf_i18n::t(FREE_ACCOUNT);
            s.playback = PlaybackAuthState::PremiumRequired;
            s.playback_error = mf_i18n::t(PREMIUM_NEEDED);
        });
    }

    /// Вход через сессию не удался, а другого входа нет: ошибка на экране входа (выход — уже показан).
    fn session_login_failed(&self, text: &str) {
        self.update(|s| {
            if !matches!(s.state, AuthState::SignedIn | AuthState::SignedOut) {
                s.state = AuthState::Error;
                s.error_text = text.to_owned();
            }
        });
    }

    /// Вошли через сессию librespot или Web API.
    fn session_signed_in(&self) -> bool {
        self.api.session_account().is_some()
    }

    /// Сессия librespot поднялась: её аккаунт — аккаунт приложения (вход без Web API), профиль — из неё.
    fn on_session_up(&self) {
        let username = self
            .playback_account()
            .or_else(|| self.player.session().map(|session| session.username()))
            .filter(|name| !name.is_empty());
        let Some(username) = username else { return };
        self.api.set_session_account(Some(AccountId::new(username.clone())));
        if self.snapshot.borrow().state != AuthState::SignedIn {
            self.show_signed_in(sf_api::map::account(&remembered_user(&username)), false);
        }
        self.refresh_session_profile();
    }

    /// Spotify отозвал вход воспроизведения: без Web API это выход.
    fn on_session_lost(&self) {
        self.api.set_session_account(None);
        lock(&self.inner).session_profile = false;
        if self.api.web_account().is_some() {
            return;
        }
        let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
        self.player.expect_local(false);
        self.update(|s| {
            s.state = AuthState::SignedOut;
            s.error_text = mf_i18n::t(EXPIRED);
            s.account = None;
        });
        if was_signed_in {
            self.emit(AuthEvent::SignedOut);
        }
    }

    /// Профиль из сессии librespot (имя, аватар, тип аккаунта, страна) — без Web API.
    fn refresh_session_profile(&self) {
        let Some(session) = self.player.session() else { return };
        let generation = self.generation();
        self.spawn(move |service| async move {
            let mut user = session_profile::fetch(&session).await;
            if !service.current(generation) || user.id.is_empty() {
                return;
            }
            if service.api.session_account().is_some_and(|account| account.as_str() != user.id) {
                return;
            }
            // сессия librespot работает только с Premium
            user.product.get_or_insert_with(|| "premium".to_owned());
            save_session_profile(&user);
            sf_player::engine::player::note_account_product(user.product.as_deref());
            lock(&service.inner).premium = premium_of(user.product.as_deref());
            service.show_signed_in(sf_api::map::account(&user), true);
        });
    }

    /// Вошли (Web API или сессия): состояние, карточка аккаунта, SignedIn при первом входе.
    /// Карточку из сессии профиль Web API не заменяет, а лишь дополняет пустые поля.
    fn show_signed_in(&self, account: Account, from_session: bool) {
        let (keep_session, waiting) = {
            let mut inner = lock(&self.inner);
            let keep = !from_session && inner.session_profile;
            if from_session {
                inner.session_profile = true;
            }
            (keep, inner.waiting.is_some())
        };
        let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
        let mut shown = account.clone();
        self.update(|s| {
            shown = match s.account.take() {
                Some(current) if keep_session => merge_account(current, Some(&account)),
                current => merge_account(account, current.as_ref()),
            };
            s.account = Some(shown.clone());
            s.state = AuthState::SignedIn;
            if !was_signed_in || !from_session {
                s.error_text.clear();
            }
            if !waiting {
                s.auth_url.clear();
                s.auth_port = 0;
            }
        });
        if !was_signed_in {
            self.emit(AuthEvent::SignedIn(shown));
        }
    }

    /// Web API вошёл в другой аккаунт, чем уже открытый (сессия или другое приложение).
    fn on_other_web_account(&self, source: ApiSource) {
        let text = mf_i18n::t(ACCOUNT_MISMATCH);
        match source {
            ApiSource::Shared => self.update(|s| {
                if s.state != AuthState::SignedIn {
                    s.state = AuthState::Error;
                }
                s.web_state = "error".to_owned();
                s.error_text = text;
            }),
            ApiSource::Personal => self.update(|s| s.personal_state = "error".to_owned()),
        }
    }

    /// /me ответил для уже вставшего Web API: другой аккаунт — снять его, иначе обновить профиль и Premium.
    fn on_confirmed(&self, source: ApiSource, user: sf_api::models::User) {
        let installed = self.api.state(source).account().cloned();
        if user.id.is_empty() || installed.as_ref().is_some_and(|account| account.as_str() != user.id) {
            tracing::warn!("Web API answered for a different Spotify account");
            self.api.clear(source);
            lock(&self.inner).web_tokens[source_index(source)] = None;
            if self.api.account().is_none() {
                let was_signed_in = self.snapshot.borrow().state == AuthState::SignedIn;
                self.update(|s| {
                    s.state = AuthState::Error;
                    s.account = None;
                });
                if was_signed_in {
                    self.emit(AuthEvent::SignedOut);
                }
            }
            self.on_other_web_account(source);
            return;
        }
        if source == ApiSource::Shared || self.api.web_account().is_none() {
            self.show_signed_in(sf_api::map::account(&user), false);
        }
        sf_player::engine::player::note_account_product(user.product.as_deref());
        self.on_account_checked(premium_of(user.product.as_deref()));
    }

    /// «Отключить»: остановить движок, удалить слот Playback, PlaybackAuthorized = false.
    pub fn disable_playback(&self) {
        // без Web API вход воспроизведения — единственный вход: «Отключить» = выйти
        if self.api.web_account().is_none() {
            self.sign_out();
            return;
        }
        let waiting = lock(&self.inner).waiting.as_ref().map(|(kind, _)| *kind);
        if waiting == Some(Waiting::Playback) {
            self.cancel_waiting();
        }
        let premium = {
            let mut inner = lock(&self.inner);
            inner.engine_epoch += 1;
            inner.engine_busy = false;
            inner.engine_running = None;
            inner.playback_grant = None;
            inner.session_profile = false;
            inner.premium
        };
        self.api.set_session_account(None);
        self.player.disconnect_local();
        if let Err(error) = self.store.revoke(Slot::Playback) {
            tracing::warn!("playback permission revocation: {error:#}");
        }
        self.delete_slot(Slot::Playback);
        self.settings.set(SettingKey::PlaybackAuthorized, SettingValue::Bool(false));
        self.update(|s| {
            s.playback =
                if premium == Some(false) { PlaybackAuthState::PremiumRequired } else { PlaybackAuthState::None };
            s.playback_error.clear();
            if waiting == Some(Waiting::Playback) {
                s.auth_url.clear();
                s.auth_port = 0;
            }
            s.engine_restart_pending = false;
        });
    }

    /// Перезапустить движок с текущими настройками (glue::engine_config) — «Применить» в ApplyBar.
    pub fn restart_engine(&self) {
        let (running, proxy) = {
            let inner = lock(&self.inner);
            (inner.engine_running.is_some(), inner.proxy.clone())
        };
        if running && proxy.engine_unsupported().is_some() {
            {
                let mut inner = lock(&self.inner);
                inner.engine_epoch += 1;
                inner.engine_running = None;
            }
            self.player.disconnect_local();
            self.refuse_unsupported_proxy();
            self.update(|s| s.engine_restart_pending = false);
            return;
        }
        if running {
            let config = crate::glue::engine_config(&self.settings, proxy);
            lock(&self.inner).engine_running = Some(EngineKey::of(&config));
            self.player.restart_engine(config);
        } else if self.snapshot.borrow().playback == PlaybackAuthState::Error {
            self.resume_engine();
        }
        self.update(|s| s.engine_restart_pending = false);
    }

    fn recheck_engine_settings(&self) {
        let (running, proxy) = {
            let inner = lock(&self.inner);
            (inner.engine_running.clone(), inner.proxy.clone())
        };
        let pending = match running {
            Some(running) => EngineKey::of(&crate::glue::engine_config(&self.settings, proxy)) != running,
            None => false,
        };
        self.update(|s| s.engine_restart_pending = pending);
    }

    // --- личный Client ID и прокси --------------------------------------------------------------

    /// Личный Client ID: Some — PKCE личного приложения; None — удалить.
    pub fn set_personal_client_id(&self, client_id: Option<String>) {
        let client_id = client_id.map(|id| id.trim().to_owned()).filter(|id| !id.is_empty());
        let waiting = lock(&self.inner).waiting.as_ref().map(|(kind, _)| *kind);
        if waiting == Some(Waiting::Personal) {
            self.cancel_waiting();
        }
        if let Err(error) = self.store.revoke(Slot::Personal) {
            tracing::warn!("personal app revocation: {error:#}");
        }
        self.delete_slot(Slot::Personal);
        lock(&self.inner).web_tokens[1] = None;
        self.api.clear(ApiSource::Personal);
        self.settings.set(SettingKey::WebClientId, SettingValue::Str(client_id.clone().unwrap_or_default()));
        let id = client_id.clone().unwrap_or_default();
        self.update(|s| {
            s.personal_client_id = id;
            s.personal_state = if client_id.is_some() { "authorizing" } else { "none" }.to_owned();
            if waiting == Some(Waiting::Personal) {
                s.auth_url.clear();
                s.auth_port = 0;
            }
        });
        if client_id.is_none() {
            lock(&self.inner).personal_pending = false;
            return;
        }
        if matches!(waiting, Some(Waiting::Shared | Waiting::Playback)) {
            lock(&self.inner).personal_pending = true;
        } else {
            self.begin_web(ApiSource::Personal);
        }
    }

    fn install_proxy(&self, config: ProxyConfig) {
        match sf_api::http::build_client(&config) {
            Ok(client) => {
                self.http.replace(client);
                lock(&self.inner).proxy = config.clone();
                self.publish_proxy(config);
            }
            Err(error) => {
                tracing::warn!("proxy settings are invalid: {error:#}");
                self.http.block(error.clone());
                lock(&self.inner).proxy = ProxyConfig::Invalid(error.clone());
                self.publish_proxy(ProxyConfig::Invalid(error.clone()));
                self.update(|s| {
                    s.proxy_state = "error".to_owned();
                    s.proxy_error = error;
                });
            }
        }
    }

    /// Применить прокси: проверить поля; только после успешной проверки записать ProxyMode/Host/Port/Username
    /// в настройки (мост их только читает) и пароль в слот Proxy, заменить HTTP-клиент;
    /// движок перезапускается, если HTTP-прокси для librespot изменился.
    pub fn apply_proxy(&self, input: ProxyInput) {
        self.update(|s| {
            s.proxy_state = "applying".to_owned();
            s.proxy_error.clear();
        });
        self.spawn(move |service| async move {
            let fail = |text: String| {
                service.update(|s| {
                    s.proxy_state = "error".to_owned();
                    s.proxy_error = text;
                })
            };
            let mode_name = input.mode.trim().to_lowercase();
            if !ProxyMode::ALL.iter().any(|mode| mode.as_str() == mode_name) {
                return fail(mf_i18n::t("Unknown proxy mode"));
            }
            let mode = ProxyMode::from_setting(&mode_name);
            let (host, port, username) =
                (input.host.trim().to_owned(), input.port.trim().to_owned(), input.username.trim().to_owned());
            let mut password = input.password.clone();
            if mode.is_manual() && password.is_empty() && !username.is_empty()
                && let Ok(loaded) = service.store.lease(Slot::Proxy).load().await
                && let Some(StoredGrant::Proxy(saved)) = loaded.grant
                && saved.host == host
                && saved.port.to_string() == port
                && saved.username == username
            {
                password = saved.password;
            }
            let config = match ProxyConfig::from_parts(mode, &host, &port, &username, &password) {
                Ok(config) => config,
                Err(text) => return fail(text),
            };
            let client = match sf_api::http::build_client(&config) {
                Ok(client) => client,
                Err(text) => return fail(mf_i18n::tf("Proxy rejected: {0}", &[&text])),
            };
            let settings = &service.settings;
            settings.set(SettingKey::ProxyMode, SettingValue::Str(mode.as_str().to_owned()));
            settings.set(SettingKey::ProxyHost, SettingValue::Str(if mode.is_manual() { host } else { String::new() }));
            settings.set(SettingKey::ProxyPort, SettingValue::Str(if mode.is_manual() { port } else { String::new() }));
            settings.set(
                SettingKey::ProxyUsername,
                SettingValue::Str(if mode.is_manual() { username } else { String::new() }),
            );
            if mode.is_manual() {
                if let Some(record) = config.password_record() {
                    service.store.invalidate(Slot::Proxy);
                    if let Err(error) = service.store.lease(Slot::Proxy).save(StoredGrant::Proxy(record)).await {
                        tracing::warn!("proxy password not saved: {error:#}");
                    }
                } else {
                    let _ = service.store.revoke(Slot::Proxy);
                    service.delete_slot(Slot::Proxy);
                }
            }
            service.http.replace(client);
            let new_url = proxy_fingerprint(&config);
            let restart = {
                let mut inner = lock(&service.inner);
                inner.proxy = config.clone();
                inner.engine_running.as_ref().is_some_and(|running| {
                    running.proxy != new_url || inner.proxy.engine_unsupported().is_some()
                })
            };
            service.publish_proxy(config);
            if restart {
                service.restart_engine();
            }
            service.update(|s| {
                s.proxy_state = "ok".to_owned();
                s.proxy_error.clear();
            });
        });
    }

    pub fn is_signed_in(&self) -> bool {
        self.snapshot.borrow().state == AuthState::SignedIn
    }

    pub fn is_premium(&self) -> bool {
        self.snapshot.borrow().account.as_ref().is_some_and(|a| a.premium)
    }

    pub fn account(&self) -> Option<Account> {
        self.snapshot.borrow().account.clone()
    }

    pub fn account_id(&self) -> Option<md3_model::AccountId> {
        self.account().map(|a| md3_model::AccountId::new(a.id))
    }

    /// Шлюз Web API (общий для всех сервисов).
    pub fn api(&self) -> Arc<ApiGateway> {
        self.api.clone()
    }

    pub fn snapshot(&self) -> AuthSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<AuthSnapshot> {
        self.snapshot.subscribe()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AuthEvent> {
        self.events.subscribe()
    }

    /// Действующий прокси (с паролем) — тот же, через который идут запросы к Web API.
    pub fn proxy(&self) -> ProxyConfig {
        self.proxy.borrow().clone()
    }

    /// Изменения действующего прокси: после restore() при входе и после каждого успешного apply_proxy.
    pub fn watch_proxy(&self) -> watch::Receiver<ProxyConfig> {
        self.proxy.subscribe()
    }

    fn publish_proxy(&self, config: ProxyConfig) {
        self.proxy.send_if_modified(|current| {
            let changed = *current != config;
            *current = config;
            changed
        });
    }
}

/// id аккаунта, которому принадлежит токен (GET /v1/me); None — проверить не удалось.
async fn token_owner(http: &reqwest::Client, token: &str) -> Option<String> {
    let response = match http.get(ME_URL).bearer_auth(token).timeout(Duration::from_secs(15)).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("playback token owner not verified: {}", error.without_url());
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!("playback token owner not verified: {}", response.status());
        return None;
    }
    owner_id(&response.text().await.ok()?)
}

fn owner_id(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("id")?.as_str().filter(|id| !id.is_empty()).map(str::to_owned)
}


/// Профиль последней удачной проверки входа (state/profile-<источник>.json), привязан к Client ID.
#[derive(serde::Deserialize, serde::Serialize)]
struct RememberedProfile {
    client_id: String,
    user: sf_api::models::User,
}

fn profile_file(source: ApiSource) -> PathBuf {
    let name = match source {
        ApiSource::Shared => "profile-shared.json",
        ApiSource::Personal => "profile-personal.json",
    };
    mf_platform::dirs::state_dir().join(name)
}

fn load_profile(source: ApiSource, client_id: &str) -> Option<sf_api::models::User> {
    let remembered = read_profile(&profile_file(source))?;
    (remembered.client_id == client_id && !remembered.user.id.is_empty()).then_some(remembered.user)
}

fn read_profile(path: &std::path::Path) -> Option<RememberedProfile> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Профиль последнего входа через сессию librespot (state/profile-session.json).
fn session_profile_file() -> PathBuf {
    mf_platform::dirs::state_dir().join("profile-session.json")
}

/// Аккаунт сессии для карточки до ответа самой сессии: профиль прошлого входа этого аккаунта
/// (сессия, затем Web API); нет — только id.
fn remembered_user(username: &str) -> sf_api::models::User {
    let paths = [session_profile_file(), profile_file(ApiSource::Shared), profile_file(ApiSource::Personal)];
    remembered_user_in(&paths, username)
}

fn remembered_user_in(paths: &[PathBuf], username: &str) -> sf_api::models::User {
    let mut user = paths
        .iter()
        .filter_map(|path| read_profile(path))
        .map(|remembered| remembered.user)
        .find(|user| user.id == username)
        .unwrap_or_else(|| sf_api::models::User { id: username.to_owned(), ..Default::default() });
    // вход воспроизведения librespot выдаёт только Premium
    user.product.get_or_insert_with(|| "premium".to_owned());
    user
}

/// Свежая карточка аккаунта, пустые поля — из прежней карточки того же аккаунта. Имени нет (профиль
/// не ответил) — имя и аватар прежние.
fn merge_account(mut fresh: Account, current: Option<&Account>) -> Account {
    let Some(current) = current.filter(|current| current.id == fresh.id) else {
        return fresh;
    };
    if fresh.display_name.is_empty() || fresh.display_name == fresh.id {
        fresh.display_name = current.display_name.clone();
        if fresh.avatar.is_empty() {
            fresh.avatar = current.avatar.clone();
        }
    }
    for (field, old) in [(&mut fresh.email, &current.email), (&mut fresh.country, &current.country), (&mut fresh.uri, &current.uri)] {
        if field.is_empty() {
            *field = old.clone();
        }
    }
    if fresh.product.is_empty() {
        fresh.product = current.product.clone();
        fresh.premium = current.premium;
    }
    fresh
}

fn save_session_profile(user: &sf_api::models::User) {
    write_profile(&session_profile_file(), "librespot", user);
}

fn save_profile(source: ApiSource, client_id: &str, user: &sf_api::models::User) {
    write_profile(&profile_file(source), client_id, user);
}

fn write_profile(path: &std::path::Path, client_id: &str, user: &sf_api::models::User) {
    let remembered = RememberedProfile { client_id: client_id.to_owned(), user: user.clone() };
    let written = serde_json::to_string(&remembered)
        .map_err(std::io::Error::other)
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, text)
        });
    if let Err(error) = written {
        tracing::warn!("sign-in profile not saved: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_owner_is_the_profile_id() {
        assert_eq!(owner_id(r#"{"id":"alice","display_name":"A"}"#).as_deref(), Some("alice"));
        assert_eq!(owner_id(r#"{"id":""}"#), None);
        assert_eq!(owner_id("not json"), None);
    }

    #[test]
    fn error_codes_survive_translation() {
        let busy = oauth_error_text(&anyhow::anyhow!("unable to listen on 127.0.0.1:8898"), 8898).unwrap();
        assert_eq!(error_code(&busy), "portBusy");
        assert_eq!(error_code(&mf_i18n::t(FREE_ACCOUNT)), "free");
        assert_eq!(error_code(&mf_i18n::t(OTHER_ACCOUNT)), "otherAccount");
        assert_eq!(error_code("something else"), "");
        assert_eq!(error_code(""), "");
    }

    #[test]
    fn browser_errors_become_user_text() {
        let busy = anyhow::anyhow!("unable to listen on 127.0.0.1:8989 for the Spotify redirect");
        assert_eq!(oauth_error_text(&busy, 8989).unwrap(), "Port 8989 is in use by another program");
        assert!(oauth_error_text(&anyhow::anyhow!("sign-in cancelled"), 8989).is_none());
        let refused = anyhow::anyhow!("Spotify refused the sign-in. Try again.");
        assert_eq!(oauth_error_text(&refused, 8989).unwrap(), "Sign-in was cancelled in the browser");
        assert_eq!(connect_error_text("needs Premium"), PREMIUM_NEEDED);
    }

    #[test]
    fn premium_is_known_only_from_the_product() {
        assert_eq!(premium_of(Some("premium")), Some(true));
        assert_eq!(premium_of(Some("free")), Some(false));
        assert_eq!(premium_of(None), None);
    }

    fn account(id: &str, name: &str, avatar: &str) -> Account {
        Account { id: id.into(), display_name: name.into(), avatar: avatar.into(), ..Default::default() }
    }

    #[test]
    fn a_fresh_card_keeps_what_the_old_one_knew() {
        let old = Account { country: "IN".into(), product: "premium".into(), premium: true, ..account("me", "Fatyzzz", "face") };
        // профиль сессии не ответил: имя = id — имя и аватар прежние, тип аккаунта тоже
        let bare = merge_account(account("me", "me", ""), Some(&old));
        assert_eq!((bare.display_name.as_str(), bare.avatar.as_str(), bare.country.as_str()), ("Fatyzzz", "face", "IN"));
        assert!(bare.premium);
        // ответил: аватар сняли — так и показываем
        let fresh = merge_account(account("me", "New", ""), Some(&old));
        assert_eq!((fresh.display_name.as_str(), fresh.avatar.as_str()), ("New", ""));
        // другой аккаунт — ничего не переносится
        let other = merge_account(account("you", "you", ""), Some(&old));
        assert_eq!((other.display_name.as_str(), other.country.as_str()), ("you", ""));
    }

    #[test]
    fn a_session_sign_in_remembers_the_profile_of_that_account() {
        let dir = std::env::temp_dir().join(format!("mf-auth-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let session = dir.join("profile-session.json");
        let shared = dir.join("profile-shared.json");
        let user = |id: &str, name: &str, product: Option<&str>| sf_api::models::User {
            id: id.into(),
            display_name: Some(name.into()),
            product: product.map(str::to_owned),
            ..Default::default()
        };
        write_profile(&shared, "web-client", &user("me", "From web", Some("premium")));
        let paths = [session.clone(), shared.clone()];
        assert_eq!(remembered_user_in(&paths, "me").display_name.as_deref(), Some("From web"));
        write_profile(&session, "librespot", &user("me", "From session", None));
        let remembered = remembered_user_in(&paths, "me");
        assert_eq!(remembered.display_name.as_deref(), Some("From session"));
        assert_eq!(remembered.product.as_deref(), Some("premium"), "a playback sign-in is Premium");
        // чужой профиль не берётся: только id
        let stranger = remembered_user_in(&paths, "other");
        assert_eq!((stranger.id.as_str(), stranger.display_name.as_deref()), ("other", None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn states_name_like_qml() {
        assert_eq!(AuthState::AwaitingBrowser.as_str(), "awaitingBrowser");
        assert_eq!(PlaybackAuthState::PremiumRequired.as_str(), "premiumRequired");
    }
}
