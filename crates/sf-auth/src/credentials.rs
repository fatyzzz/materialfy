//! Защищённое хранилище грантов (перенос spotifast/src/credentials.rs): keyring-core + store ОС.
//!
//! Все вызовы хранилища ОС — в одном потоке «spotify-credentials» с таймаутом 20 с: заблокированное хранилище
//! не держит интерфейс, команды и завершение. Поколения (Lease) отбрасывают работу, начатую до выхода, в том числе
//! запись, вернувшуюся после выхода. Маркеры отзыва в state-каталоге (без секретов) не дают войти обратно, если
//! удалить запись не удалось. Секреты в обычных файлах не хранятся.
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use librespot_core::authentication::Credentials;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::oauth::{DEFAULT_WEB_CLIENT_ID, StoredToken};

/// Имя сервиса в хранилище ОС (Windows Credential Manager и т. п.).
pub const SERVICE: &str = "materialfy";
const TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    Shared,
    Personal,
    Playback,
    Proxy,
}

impl Slot {
    pub const SPOTIFY: [Self; 3] = [Self::Shared, Self::Personal, Self::Playback];
    pub const ALL: [Self; 4] = [Self::Shared, Self::Personal, Self::Playback, Self::Proxy];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::Shared => "shared-web",
            Self::Personal => "personal-web",
            Self::Playback => "playback",
            Self::Proxy => "proxy-password",
        }
    }
}

/// Секрет слота. Без Debug: содержит рабочие секреты.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Grant {
    Web(StoredToken),
    Playback(Credentials),
    Proxy(ProxyPassword),
}

impl Grant {
    fn valid_for(&self, slot: Slot) -> bool {
        match (slot, self) {
            (Slot::Shared, Self::Web(token)) => token.client_id == DEFAULT_WEB_CLIENT_ID && !token.refresh_token.is_empty(),
            (Slot::Personal, Self::Web(token)) => !token.client_id.is_empty() && !token.refresh_token.is_empty(),
            (Slot::Playback, Self::Playback(credentials)) => {
                credentials.username.as_deref().is_some_and(|name| !name.is_empty()) && !credentials.auth_data.is_empty()
            }
            (Slot::Proxy, Self::Proxy(password)) => password.valid(),
            _ => false,
        }
    }
}

/// Пароль прокси для одной пары host:port + username. Без Debug.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyPassword {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyPassword {
    /// Запись совпадает с той, что получается из её же адреса (как при сохранении).
    pub fn valid(&self) -> bool {
        crate::proxy::password_is_valid(self)
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    slot: Slot,
    grant: Grant,
}

/// Ошибки хранилища. Display — текст интерфейса через mf_i18n (показываются в Auth.errorText / Settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Locked,
    Timeout,
    Invalid,
    Filesystem,
    Verification,
    Stale,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Unavailable => {
                mf_i18n::t("The system credential store is unavailable — your sign-in won't be remembered.")
            }
            Self::Locked => mf_i18n::t("The credential store is locked or access was denied."),
            Self::Timeout => mf_i18n::t("The credential store didn't respond in time."),
            Self::Invalid => mf_i18n::t("The saved Spotify grant is damaged — sign in again."),
            Self::Filesystem => mf_i18n::t(
                "Couldn't update the credential store's service files (check permissions on the data folder).",
            ),
            Self::Verification => {
                mf_i18n::t("The credential store didn't save the grant — your sign-in won't be remembered.")
            }
            Self::Stale => mf_i18n::t("The sign-in changed while the grant was being saved."),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Текст той же ошибки для пароля прокси.
    pub fn proxy_message(self) -> String {
        match self {
            Self::Unavailable | Self::Locked => {
                mf_i18n::t("Unlock or turn on the credential store to remember the proxy password.")
            }
            Self::Timeout => {
                mf_i18n::t("The credential store didn't respond — the proxy password only lasts until you quit.")
            }
            Self::Invalid => {
                mf_i18n::t("The saved proxy password or its address is invalid — enter the proxy settings again.")
            }
            Self::Filesystem => mf_i18n::t(
                "Couldn't update the proxy password's service files (check permissions on the data folder).",
            ),
            Self::Verification => {
                mf_i18n::t("The credential store didn't save the proxy password — it only lasts until you quit.")
            }
            Self::Stale => mf_i18n::t("The proxy settings changed while the password was being saved."),
        }
    }
}

pub type StoreFuture = Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>;
type Request<T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send>>;

#[cfg(not(target_os = "android"))]
fn native_error(error: keyring_core::Error) -> Error {
    // текст ошибок провайдера может содержать секрет — наружу не передаём
    match error {
        keyring_core::Error::NoStorageAccess(_) => Error::Locked,
        _ => Error::Unavailable,
    }
}

trait ProtectedStore: Send {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error>;
    fn delete(&mut self, key: &str) -> Result<(), Error>;
}

#[cfg(not(target_os = "android"))]
#[derive(Default)]
struct NativeStore {
    store: Option<Arc<keyring_core::api::CredentialStore>>,
}

#[cfg(not(target_os = "android"))]
impl NativeStore {
    fn entry(&mut self, key: &str) -> Result<keyring_core::Entry, Error> {
        if self.store.is_none() {
            #[cfg(target_os = "linux")]
            let store = zbus_secret_service_keyring_store::Store::new();
            #[cfg(target_os = "macos")]
            let store = apple_native_keyring_store::keychain::Store::new();
            #[cfg(windows)]
            let store = windows_native_keyring_store::Store::new();
            #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
            let store: Result<Arc<keyring_core::api::CredentialStore>, keyring_core::Error> =
                Err(keyring_core::Error::NotSupportedByStore("platform".into()));
            self.store = Some(store.map_err(native_error)?);
        }
        self.store.as_ref().ok_or(Error::Unavailable)?.build(SERVICE, key, None).map_err(native_error)
    }
}

#[cfg(not(target_os = "android"))]
impl ProtectedStore for NativeStore {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.entry(key)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(native_error(error)),
        }
    }

    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error> {
        self.entry(key)?.set_secret(secret).map_err(native_error)
    }

    fn delete(&mut self, key: &str) -> Result<(), Error> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(native_error(error)),
        }
    }
}

/// Android: системного keyring для нативного кода нет. Секреты лежат файлами в приватном каталоге приложения
/// ($HOME = Context.getFilesDir(), mode 0600): другие приложения его не видят, резервное копирование
/// выключено в манифесте (allowBackup=false). Имя файла — SHA-256 ключа, без самого ключа.
#[cfg(target_os = "android")]
#[derive(Default)]
struct NativeStore;

#[cfg(target_os = "android")]
impl NativeStore {
    fn path(key: &str) -> Result<PathBuf, Error> {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or(Error::Unavailable)?;
        let dir = PathBuf::from(home).join(".secrets");
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir).map_err(|_| Error::Unavailable)?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let name = hex_digest(key.as_bytes());
        Ok(dir.join(name))
    }
}

#[cfg(target_os = "android")]
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(target_os = "android")]
impl ProtectedStore for NativeStore {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match std::fs::read(Self::path(key)?) {
            Ok(secret) => Ok(Some(secret)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(Error::Locked),
        }
    }

    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let path = Self::path(key)?;
        let tmp = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| Error::Unavailable)?;
        file.write_all(secret).and_then(|()| file.sync_all()).map_err(|_| Error::Unavailable)?;
        std::fs::rename(&tmp, &path).map_err(|_| Error::Unavailable)
    }

    fn delete(&mut self, key: &str) -> Result<(), Error> {
        match std::fs::remove_file(Self::path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(Error::Locked),
        }
    }
}

#[derive(Default)]
struct MemoryStore {
    values: std::collections::HashMap<String, Vec<u8>>,
}

impl ProtectedStore for MemoryStore {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.values.get(key).cloned())
    }

    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error> {
        self.values.insert(key.to_owned(), secret.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<(), Error> {
        self.values.remove(key);
        Ok(())
    }
}

type Job = Box<dyn FnOnce(&mut dyn ProtectedStore) + Send>;

struct Inner {
    state_dir: PathBuf,
    profile: String,
    generations: [AtomicU64; 4],
    // только вокруг крошечных файлов-маркеров, никогда вокруг вызова хранилища ОС
    markers: Mutex<()>,
    jobs: mpsc::SyncSender<Job>,
    available: Arc<AtomicBool>,
}

/// Хранилище. Clone — дешёвый (Arc внутри).
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

/// Право читать/писать один слот в одном поколении авторизации.
#[derive(Clone)]
pub struct Lease {
    store: Store,
    slot: Slot,
    generation: u64,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    version: u32,
    revoked: bool,
}

pub struct Loaded {
    pub grant: Option<Grant>,
    pub warning: Option<Error>,
}

impl Store {
    /// `state_dir` — mf_platform::dirs::state_dir() (маркеры отзыва, профиль).
    /// Поток хранилища входит в текущий tokio-рантайм, если он есть (нужно Secret Service на Linux).
    pub fn new(state_dir: PathBuf) -> Self {
        Self::with_backend(state_dir, tokio::runtime::Handle::try_current().ok(), Box::new(NativeStore::default()))
    }

    /// Как `new`, но с явным рантаймом для потока хранилища (вызов вне контекста рантайма).
    pub fn with_runtime(state_dir: PathBuf, runtime: tokio::runtime::Handle) -> Self {
        Self::with_backend(state_dir, Some(runtime), Box::new(NativeStore::default()))
    }

    /// Для тестов соседних крейтов: секреты в памяти, маркеры отзыва — в `state_dir`.
    #[doc(hidden)]
    pub fn in_memory(state_dir: PathBuf) -> Self {
        Self::with_backend(state_dir, None, Box::new(MemoryStore::default()))
    }

    fn with_backend(
        state_dir: PathBuf,
        runtime: Option<tokio::runtime::Handle>,
        mut backend: Box<dyn ProtectedStore>,
    ) -> Self {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(16);
        let spawned = std::thread::Builder::new().name("spotify-credentials".into()).spawn(move || {
            let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
            while let Ok(job) = receiver.recv() {
                job(backend.as_mut());
            }
        });
        if spawned.is_err() {
            tracing::warn!("failed to start the credential store thread");
        }
        let profile = std::fs::read_to_string(state_dir.join("credential-profile"))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| format!("{:x}", Sha256::digest(state_dir.to_string_lossy().as_bytes())));
        Self {
            inner: Arc::new(Inner {
                state_dir,
                profile,
                generations: std::array::from_fn(|_| AtomicU64::new(0)),
                markers: Mutex::new(()),
                jobs,
                available: Arc::new(AtomicBool::new(spawned.is_ok())),
            }),
        }
    }

    pub fn lease(&self, slot: Slot) -> Lease {
        Lease { store: self.clone(), slot, generation: self.inner.generations[slot.index()].load(Ordering::SeqCst) }
    }

    /// Сделать устаревшей текущую работу со слотом — до отмены задач и удаления грантов.
    pub fn invalidate(&self, slot: Slot) {
        self.inner.generations[slot.index()].fetch_add(1, Ordering::SeqCst);
    }

    /// Отзыв записывается до обращения к хранилищу ОС: даже заблокированное хранилище или прерванное удаление
    /// не вернут вход при следующем запуске. Саму запись удаляет `Lease::delete`.
    pub fn revoke(&self, slot: Slot) -> Result<(), Error> {
        self.invalidate(slot);
        let _guard = self.inner.markers.lock().unwrap_or_else(|p| p.into_inner());
        self.write_marker(slot, true)
    }

    /// Выход: отозвать все три слота Spotify (прокси остаётся).
    pub fn revoke_spotify(&self) -> Result<(), Error> {
        let mut error = None;
        for slot in Slot::SPOTIFY {
            if let Err(failure) = self.revoke(slot) {
                error = Some(failure);
            }
        }
        error.map_or(Ok(()), Err)
    }

    /// "keyring" — секреты в хранилище ОС, "" — недоступно (для Auth.tokenStorage).
    pub fn backend_name(&self) -> &'static str {
        if self.inner.available.load(Ordering::Relaxed) { "keyring" } else { "" }
    }

    fn marker_path(&self, slot: Slot) -> PathBuf {
        self.inner.state_dir.join("credential-storage").join(format!("{}.json", slot.name()))
    }

    fn marker(&self, slot: Slot) -> Result<Option<Marker>, Error> {
        match std::fs::read(self.marker_path(slot)) {
            Ok(bytes) => {
                let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
                if marker.version != 1 {
                    return Err(Error::Invalid);
                }
                Ok(Some(marker))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(Error::Filesystem),
        }
    }

    fn write_marker(&self, slot: Slot, revoked: bool) -> Result<(), Error> {
        let path = self.marker_path(slot);
        let parent = path.parent().ok_or(Error::Filesystem)?;
        std::fs::create_dir_all(parent).map_err(|_| Error::Filesystem)?;
        let bytes = serde_json::to_vec(&Marker { version: 1, revoked }).map_err(|_| Error::Invalid)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(|_| Error::Filesystem)?;
        std::fs::rename(&temporary, &path).map_err(|_| Error::Filesystem)
    }
}

impl Lease {
    pub fn slot(&self) -> Slot {
        self.slot
    }

    /// Поколение ещё актуально (не было выхода/повторной авторизации).
    pub fn current(&self) -> bool {
        self.store.inner.generations[self.slot.index()].load(Ordering::SeqCst) == self.generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn check(&self) -> Result<(), Error> {
        if self.current() { Ok(()) } else { Err(Error::Stale) }
    }

    fn key(&self) -> String {
        format!("{}:{}", self.store.inner.profile, self.slot.name())
    }

    fn request<T: Send + 'static>(
        &self,
        guard_generation: bool,
        operation: impl FnOnce(Self, &mut dyn ProtectedStore) -> Result<T, Error> + Send + 'static,
    ) -> Request<T> {
        let pending = (|| {
            if guard_generation {
                self.check()?;
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let lease = self.clone();
            let available = self.store.inner.available.clone();
            self.store
                .inner
                .jobs
                .try_send(Box::new(move |backend| {
                    let result = (if guard_generation { lease.check() } else { Ok(()) })
                        .and_then(|()| operation(lease.clone(), backend))
                        .and_then(|value| {
                            if guard_generation {
                                lease.check()?;
                            }
                            Ok(value)
                        });
                    match &result {
                        Ok(_) | Err(Error::Locked) => available.store(true, Ordering::Relaxed),
                        Err(Error::Unavailable) => available.store(false, Ordering::Relaxed),
                        Err(_) => {}
                    }
                    let _ = sender.send(result);
                }))
                .map_err(|_| Error::Unavailable)?;
            Ok::<_, Error>(receiver)
        })();
        Box::pin(async move {
            tokio::time::timeout(TIMEOUT, pending?).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Unavailable)?
        })
    }

    pub async fn load(&self) -> Result<Loaded, Error> {
        self.request(true, |lease, backend| {
            let marker = lease.store.marker(lease.slot)?;
            if marker.as_ref().is_some_and(|marker| marker.revoked) {
                // отозванный слот не требует даже доступа к хранилищу
                return Ok(Loaded { grant: None, warning: None });
            }
            let Some(bytes) = backend.read(&lease.key())? else {
                return Ok(Loaded { grant: None, warning: None });
            };
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
            if record.version != RECORD_VERSION || record.slot != lease.slot || !record.grant.valid_for(lease.slot) {
                return Err(Error::Invalid);
            }
            let _guard = lease.store.inner.markers.lock().unwrap_or_else(|p| p.into_inner());
            lease.check()?;
            let warning = if marker.is_none() { lease.store.write_marker(lease.slot, false).err() } else { None };
            Ok(Loaded { grant: Some(record.grant), warning })
        })
        .await
    }

    fn save_inner(&self, backend: &mut dyn ProtectedStore, grant: Grant) -> Result<(), Error> {
        self.check()?;
        if !grant.valid_for(self.slot) {
            return Err(Error::Invalid);
        }
        let bytes = serde_json::to_vec(&Record { version: RECORD_VERSION, slot: self.slot, grant })
            .map_err(|_| Error::Invalid)?;
        backend.write(&self.key(), &bytes)?;
        if !self.current() {
            // операции хранилища последовательны: более новая запись ещё в очереди за этой и не сотрётся
            let _ = backend.delete(&self.key());
            return Err(Error::Stale);
        }
        if backend.read(&self.key())?.as_deref() != Some(bytes.as_slice()) {
            return Err(Error::Verification);
        }
        let _guard = self.store.inner.markers.lock().unwrap_or_else(|p| p.into_inner());
        self.check()?;
        self.store.write_marker(self.slot, false)
    }

    /// Ставится в очередь сразу: обновления токена сохраняют порядок записи, даже если их ждут вне мьютекса.
    pub fn save(&self, grant: Grant) -> StoreFuture {
        self.request(true, move |lease, backend| lease.save_inner(backend, grant))
    }

    /// Вызывать после `Store::revoke`. Ошибка удаления видна, а маркер отзыва не даст восстановить грант.
    pub fn delete(&self) -> StoreFuture {
        // удаление стоит в очереди раньше новой записи: выполняется, даже если поколение уже сменилось
        self.request(false, |lease, backend| backend.delete(&lease.key()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    #[derive(Default)]
    struct Fake {
        values: HashMap<String, Vec<u8>>,
        read_error: Option<Error>,
        write_error: Option<Error>,
        delete_error: Option<Error>,
        discard_write: bool,
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        release: Option<mpsc::Receiver<()>>,
    }

    struct Backend(Arc<Mutex<Fake>>);

    impl ProtectedStore for Backend {
        fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
            let fake = self.0.lock().unwrap();
            if let Some(error) = fake.read_error {
                return Err(error);
            }
            Ok(fake.values.get(key).cloned())
        }

        fn write(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
            let (entered, release) = {
                let mut fake = self.0.lock().unwrap();
                if let Some(error) = fake.write_error {
                    return Err(error);
                }
                (fake.entered.take(), fake.release.take())
            };
            if let Some(entered) = entered {
                let _ = entered.send(());
            }
            if let Some(release) = release {
                release.recv().unwrap();
            }
            let mut fake = self.0.lock().unwrap();
            if !fake.discard_write {
                fake.values.insert(key.to_owned(), value.to_vec());
            }
            Ok(())
        }

        fn delete(&mut self, key: &str) -> Result<(), Error> {
            let mut fake = self.0.lock().unwrap();
            if let Some(error) = fake.delete_error {
                return Err(error);
            }
            fake.values.remove(key);
            Ok(())
        }
    }

    struct Fixture {
        root: PathBuf,
        state: PathBuf,
        fake: Arc<Mutex<Fake>>,
        store: Store,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "sf-auth-credential-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let state = root.join("state");
            std::fs::create_dir_all(&state).unwrap();
            let fake = Arc::new(Mutex::new(Fake::default()));
            let store = store_at(&state, &fake);
            Self { root, state, fake, store }
        }

        fn restart(&self) -> Store {
            store_at(&self.state, &self.fake)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn store_at(state: &Path, fake: &Arc<Mutex<Fake>>) -> Store {
        Store::with_backend(state.to_path_buf(), None, Box::new(Backend(fake.clone())))
    }

    fn web(client: &str) -> Grant {
        Grant::Web(StoredToken {
            client_id: client.into(),
            access_token: "dummy-access".into(),
            refresh_token: "dummy-refresh".into(),
            expires_at: u64::MAX,
            scope: "dummy-scope".into(),
        })
    }

    fn playback() -> Grant {
        Grant::Playback(Credentials {
            username: Some("dummy-account".into()),
            auth_data: b"dummy-reusable-grant".to_vec(),
            auth_type: librespot_protocol::authentication::AuthenticationType::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS,
        })
    }

    fn proxy_password() -> Grant {
        Grant::Proxy(ProxyPassword {
            host: "127.0.0.1".into(),
            port: 8080,
            username: "dummy-user".into(),
            password: " dummy-private-password\n".into(),
        })
    }

    #[tokio::test]
    async fn a_recorded_credential_profile_keeps_grants_and_revocation_markers() {
        let fixture = Fixture::new();
        let grant = web(DEFAULT_WEB_CLIENT_ID);
        fixture.store.lease(Slot::Shared).save(grant.clone()).await.unwrap();
        fixture.store.revoke(Slot::Playback).unwrap();
        // профиль, перенесённый в другой каталог, хранит идентичность исходного — гранты остаются доступны
        let renamed = fixture.root.join("new-state");
        std::fs::rename(&fixture.state, &renamed).unwrap();
        std::fs::write(renamed.join("credential-profile"), &fixture.store.inner.profile).unwrap();
        let store = store_at(&renamed, &fixture.fake);
        assert_eq!(store.inner.profile, fixture.store.inner.profile);
        assert!(store.lease(Slot::Shared).load().await.unwrap().grant == Some(grant));
        assert!(store.lease(Slot::Playback).load().await.unwrap().grant.is_none());
    }

    #[tokio::test]
    async fn all_grants_survive_restart_and_markers_hold_no_secrets() {
        let f = Fixture::new();
        let grants = [web(DEFAULT_WEB_CLIENT_ID), web("dummy-personal-client"), playback(), proxy_password()];
        for (slot, grant) in Slot::ALL.into_iter().zip(grants.iter()) {
            f.store.lease(slot).save(grant.clone()).await.unwrap();
            let marker = std::fs::read_to_string(f.store.marker_path(slot)).unwrap();
            assert!(!marker.contains("dummy"));
        }
        let restarted = f.restart();
        for (slot, grant) in Slot::ALL.into_iter().zip(grants) {
            let loaded = restarted.lease(slot).load().await.unwrap();
            assert!(loaded.grant == Some(grant));
            assert_eq!(loaded.warning, None);
        }
        assert_eq!(f.fake.lock().unwrap().values.len(), 4);
        for slot in Slot::ALL {
            assert_eq!(restarted.lease(slot).slot(), slot);
        }
    }

    #[tokio::test]
    async fn grants_must_match_their_slot() {
        let f = Fixture::new();
        assert_eq!(f.store.lease(Slot::Shared).save(web("dummy-personal-client")).await, Err(Error::Invalid));
        assert_eq!(f.store.lease(Slot::Playback).save(web(DEFAULT_WEB_CLIENT_ID)).await, Err(Error::Invalid));
        let broken = Grant::Proxy(ProxyPassword {
            host: "not a host".into(),
            port: 8080,
            username: String::new(),
            password: "dummy".into(),
        });
        assert_eq!(f.store.lease(Slot::Proxy).save(broken).await, Err(Error::Invalid));
        assert!(f.fake.lock().unwrap().values.is_empty());
    }

    #[tokio::test]
    async fn spotify_sign_out_retains_the_network_password() {
        let f = Fixture::new();
        let password = proxy_password();
        f.store.lease(Slot::Proxy).save(password.clone()).await.unwrap();
        f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await.unwrap();
        f.store.revoke_spotify().unwrap();
        let restarted = f.restart();
        assert!(restarted.lease(Slot::Proxy).load().await.unwrap().grant == Some(password));
        assert!(restarted.lease(Slot::Shared).load().await.unwrap().grant.is_none());
    }

    #[tokio::test]
    async fn a_failed_proxy_delete_cannot_restore_the_forgotten_password() {
        let f = Fixture::new();
        f.store.lease(Slot::Proxy).save(proxy_password()).await.unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke(Slot::Proxy).unwrap();
        assert_eq!(f.store.lease(Slot::Proxy).delete().await, Err(Error::Locked));
        assert!(f.restart().lease(Slot::Proxy).load().await.unwrap().grant.is_none());
    }

    #[tokio::test]
    #[ignore = "needs an unlocked OS credential store; dummy grants only"]
    async fn native_store_round_trip() {
        let f = Fixture::new();
        let key = f.store.lease(Slot::Shared).key();
        tokio::task::spawn_blocking(move || {
            let mut native = NativeStore::default();
            assert_eq!(native.read(&key).unwrap(), None);
            native.write(&key, b"dummy-probe").unwrap();
            assert_eq!(native.read(&key).unwrap().as_deref(), Some(b"dummy-probe".as_slice()));
            native.delete(&key).unwrap();
            assert_eq!(native.read(&key).unwrap(), None);
        })
        .await
        .unwrap();
        let store = Store::new(f.state.clone());
        assert_eq!(store.backend_name(), "keyring");
        let grants = [web(DEFAULT_WEB_CLIENT_ID), web("dummy-personal-client"), playback(), proxy_password()];
        for (slot, grant) in Slot::ALL.into_iter().zip(grants.iter()) {
            store.lease(slot).save(grant.clone()).await.unwrap();
        }
        let restarted = Store::new(f.state.clone());
        for (slot, grant) in Slot::ALL.into_iter().zip(grants) {
            assert!(restarted.lease(slot).load().await.unwrap().grant == Some(grant));
        }
        store.revoke_spotify().unwrap();
        store.revoke(Slot::Proxy).unwrap();
        for slot in Slot::ALL {
            let lease = store.lease(slot);
            lease.delete().await.unwrap();
            assert!(lease.request(false, |lease, backend| backend.read(&lease.key())).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn queued_deletion_cannot_erase_a_new_authorization() {
        let f = Fixture::new();
        let old = web("dummy-old-personal-client");
        let new = web("dummy-new-personal-client");
        f.store.lease(Slot::Personal).save(old).await.unwrap();
        f.store.revoke(Slot::Personal).unwrap();
        let deletion = f.store.lease(Slot::Personal).delete();
        f.store.invalidate(Slot::Personal);
        let saving = f.store.lease(Slot::Personal).save(new.clone());
        deletion.await.unwrap();
        saving.await.unwrap();
        assert!(f.restart().lease(Slot::Personal).load().await.unwrap().grant == Some(new));
    }

    #[test]
    fn a_failed_revocation_marker_is_reported() {
        let f = Fixture::new();
        std::fs::write(f.state.join("credential-storage"), b"not a directory").unwrap();
        assert_eq!(f.store.revoke(Slot::Shared), Err(Error::Filesystem));
        assert_eq!(f.store.revoke_spotify(), Err(Error::Filesystem));
        // поколение всё равно сменилось: работа до выхода отбрасывается
        assert_eq!(f.store.lease(Slot::Shared).generation(), 2);
    }

    #[tokio::test]
    async fn save_requires_read_back() {
        let f = Fixture::new();
        f.fake.lock().unwrap().discard_write = true;
        assert_eq!(f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await, Err(Error::Verification));
        assert!(!f.store.marker_path(Slot::Shared).exists());
    }

    #[tokio::test]
    async fn a_locked_store_reports_and_keeps_nothing() {
        let f = Fixture::new();
        f.fake.lock().unwrap().write_error = Some(Error::Locked);
        assert_eq!(f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await, Err(Error::Locked));
        f.fake.lock().unwrap().write_error = None;
        f.fake.lock().unwrap().read_error = Some(Error::Unavailable);
        assert!(matches!(f.store.lease(Slot::Shared).load().await, Err(Error::Unavailable)));
        assert_eq!(f.store.backend_name(), "");
        f.fake.lock().unwrap().read_error = None;
        assert!(f.store.lease(Slot::Shared).load().await.unwrap().grant.is_none());
        assert_eq!(f.store.backend_name(), "keyring");
    }

    #[tokio::test]
    async fn late_write_after_signout_cannot_recreate_grant() {
        let f = Fixture::new();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut fake = f.fake.lock().unwrap();
            fake.entered = Some(entered_tx);
            fake.release = Some(release_rx);
        }
        let lease = f.store.lease(Slot::Shared);
        let pending_lease = lease.clone();
        let pending = tokio::spawn(async move { pending_lease.save(web(DEFAULT_WEB_CLIENT_ID)).await });
        entered_rx.await.unwrap();
        f.store.revoke_spotify().unwrap();
        assert!(!lease.current());
        release_tx.send(()).unwrap();
        assert_eq!(pending.await.unwrap(), Err(Error::Stale));
        assert!(f.fake.lock().unwrap().values.is_empty());
        assert!(f.restart().lease(Slot::Shared).load().await.unwrap().grant.is_none());
    }

    #[tokio::test]
    async fn a_stale_lease_is_refused_before_touching_the_store() {
        let f = Fixture::new();
        let lease = f.store.lease(Slot::Shared);
        f.store.invalidate(Slot::Shared);
        assert!(!lease.current());
        assert_eq!(lease.save(web(DEFAULT_WEB_CLIENT_ID)).await, Err(Error::Stale));
        assert!(matches!(lease.load().await, Err(Error::Stale)));
        assert!(f.fake.lock().unwrap().values.is_empty());
    }

    #[tokio::test]
    async fn failed_native_deletion_remains_revoked_across_restart() {
        let f = Fixture::new();
        f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await.unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke_spotify().unwrap();
        assert_eq!(f.store.lease(Slot::Shared).delete().await, Err(Error::Locked));
        assert_eq!(f.fake.lock().unwrap().values.len(), 1);
        // отозванный слот не требует даже доступа к заблокированному хранилищу
        f.fake.lock().unwrap().read_error = Some(Error::Locked);
        assert!(f.restart().lease(Slot::Shared).load().await.unwrap().grant.is_none());
    }

    #[tokio::test]
    async fn a_new_authorization_after_signout_is_restored() {
        let f = Fixture::new();
        f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await.unwrap();
        f.store.revoke_spotify().unwrap();
        f.store.lease(Slot::Shared).delete().await.unwrap();
        let grant = web(DEFAULT_WEB_CLIENT_ID);
        f.store.lease(Slot::Shared).save(grant.clone()).await.unwrap();
        assert!(f.restart().lease(Slot::Shared).load().await.unwrap().grant == Some(grant));
    }

    #[tokio::test]
    async fn profiles_are_isolated_and_invalid_records_are_rejected() {
        let f = Fixture::new();
        let other = Fixture::new();
        let isolated = store_at(&other.state, &f.fake);
        f.store.lease(Slot::Shared).save(web(DEFAULT_WEB_CLIENT_ID)).await.unwrap();
        assert!(isolated.lease(Slot::Shared).load().await.unwrap().grant.is_none());
        for bytes in [
            b"not-json".to_vec(),
            serde_json::to_vec(&Record { version: 2, slot: Slot::Shared, grant: web(DEFAULT_WEB_CLIENT_ID) }).unwrap(),
            serde_json::to_vec(&Record { version: 1, slot: Slot::Playback, grant: playback() }).unwrap(),
        ] {
            f.fake.lock().unwrap().values.insert(f.store.lease(Slot::Shared).key(), bytes);
            assert!(matches!(f.store.lease(Slot::Shared).load().await, Err(Error::Invalid)));
        }
    }

    #[test]
    fn errors_are_readable_and_proxy_messages_differ() {
        for error in [
            Error::Unavailable,
            Error::Locked,
            Error::Timeout,
            Error::Invalid,
            Error::Filesystem,
            Error::Verification,
            Error::Stale,
        ] {
            assert!(!error.to_string().is_empty());
            assert!(error.proxy_message().contains("proxy"));
        }
    }
}
