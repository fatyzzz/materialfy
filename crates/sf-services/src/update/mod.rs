//! Auto-update from GitHub Releases (fatyzzz/materialfy).
//!
//! Check: `releases/latest` (stable) or `releases?per_page=20` (beta, pre-releases too) with a User-Agent and
//! If-None-Match (a 304 doesn't count against GitHub's 60 requests/hour); a rate-limit answer pauses
//! automatic checks until X-RateLimit-Reset. Automatic checks start ~10 s after launch and repeat every 6 h,
//! only while `autoUpdateCheck` is on; "Check now" works regardless.
//!
//! The asset follows the install kind ([`release::detect_install_kind`]): Setup.exe for an Inno install, the
//! portable zip, the AppImage of the same arch, the macOS dmg; other installs only get the release page.
//! Download: to `<cache>/updates/<asset>.part` with resume (Range) and cancel, then SHA-256 against
//! SHA256SUMS.txt (or GitHub's asset digest), then rename. Install: [`install::install`].
//!
//! Testing without a newer release: MATERIALFY_UPDATE_FAKE_VERSION=0.0.1 makes this build report that
//! version; MATERIALFY_UPDATE_INSTALL_KIND=installer|portable|appimage|mac|manual overrides the detected kind
//! (a cargo build is "dev": never replaced automatically).
pub mod install;
pub mod progress;
pub mod release;
pub mod state;
pub mod version;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mf_platform::{SettingKey, SettingValue, SettingsStore};
use reqwest::StatusCode;
use reqwest::header::{CONTENT_LENGTH, ETAG, IF_NONE_MATCH, RANGE};
use tokio::io::AsyncWriteExt as _;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::{AppRequest, lock};
pub use release::{Channel, InstallKind};
pub use state::{Event, Offer, Stage, UpdateSnapshot, UpdateState};
use version::Version;

pub const REPO: &str = "fatyzzz/materialfy";
const API: &str = "https://api.github.com/repos/fatyzzz/materialfy";
pub const RELEASES_PAGE: &str = "https://github.com/fatyzzz/materialfy/releases";
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
pub const START_DELAY: Duration = Duration::from_secs(10);
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// whole download (the per-chunk stall timeout catches dead connections much earlier)
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(4 * 3600);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const PROGRESS_EVERY: Duration = Duration::from_millis(120);
/// the UI shows "Restarting…" for a moment before the app quits
const QUIT_DELAY: Duration = Duration::from_millis(700);
const SUMS_NAME: &str = "SHA256SUMS.txt";

pub const FAKE_VERSION_ENV: &str = "MATERIALFY_UPDATE_FAKE_VERSION";
pub const INSTALL_KIND_ENV: &str = "MATERIALFY_UPDATE_INSTALL_KIND";

/// Version of this build (or MATERIALFY_UPDATE_FAKE_VERSION, if it parses).
pub fn current_version() -> String {
    std::env::var(FAKE_VERSION_ENV)
        .ok()
        .and_then(|v| Version::parse(&v))
        .map(|v| v.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
}

pub fn fake_version_set() -> bool {
    std::env::var(FAKE_VERSION_ENV).is_ok_and(|v| Version::parse(&v).is_some())
}

/// How this copy was installed (MATERIALFY_UPDATE_INSTALL_KIND overrides).
pub fn install_kind() -> InstallKind {
    if let Some(kind) = std::env::var(INSTALL_KIND_ENV).ok().and_then(|k| InstallKind::from_code(&k)) {
        return kind;
    }
    let exe = std::env::current_exe().ok().map(|p| std::fs::canonicalize(&p).unwrap_or(p)).unwrap_or_default();
    let exe = PathBuf::from(exe.to_string_lossy().trim_start_matches(r"\\?\"));
    let appimage = std::env::var("APPIMAGE").ok();
    let has_uninstaller = exe.parent().is_some_and(|dir| dir.join("unins000.exe").is_file());
    let mut roots: Vec<PathBuf> =
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].into_iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    // per-user Inno installs: %LOCALAPPDATA%\Programs
    roots.extend(std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs")));
    let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    release::detect_install_kind(&release::Probe {
        os: std::env::consts::OS,
        exe: &exe,
        appimage: appimage.as_deref(),
        has_uninstaller,
        install_roots: &roots,
    })
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn updates_dir() -> PathBuf {
    mf_platform::dirs::cache_subdir("updates")
}

#[derive(Default)]
struct Inner {
    task: Option<JoinHandle<()>>,
    /// URL → (ETag, body) of the last 200 answer
    etags: HashMap<String, (String, String)>,
    /// GitHub rate limit: no automatic checks before this unix time
    blocked_until: i64,
    /// SHA256SUMS.txt of the offered release
    sums_url: Option<String>,
    /// "sha256:<hex>" GitHub reports for the offered asset (fallback checksum)
    asset_digest: Option<String>,
    scheduled: bool,
}

pub struct UpdateService {
    rt: Handle,
    settings: Arc<SettingsStore>,
    http: sf_api::Http,
    app_requests: broadcast::Sender<AppRequest>,
    kind: InstallKind,
    snapshot: watch::Sender<UpdateSnapshot>,
    inner: Mutex<Inner>,
}

struct CheckError {
    text: String,
    retryable: bool,
}

impl CheckError {
    fn net(error: reqwest::Error) -> CheckError {
        let text = if error.is_timeout() {
            mf_i18n::t("The connection timed out")
        } else if error.is_connect() {
            mf_i18n::t("Couldn't connect to GitHub. Check your internet connection")
        } else {
            error.without_url().to_string()
        };
        CheckError { text, retryable: true }
    }

    fn fatal(text: String) -> CheckError {
        CheckError { text, retryable: false }
    }
}

impl UpdateService {
    pub fn new(
        rt: Handle,
        settings: Arc<SettingsStore>,
        http: sf_api::Http,
        app_requests: broadcast::Sender<AppRequest>,
    ) -> Arc<UpdateService> {
        let kind = install_kind();
        let snapshot = UpdateSnapshot {
            current_version: current_version(),
            install_kind: kind.code().to_owned(),
            eta: -1,
            ..UpdateSnapshot::default()
        };
        Arc::new(UpdateService {
            rt,
            settings,
            http,
            app_requests,
            kind,
            snapshot: watch::Sender::new(snapshot),
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn watch(&self) -> watch::Receiver<UpdateSnapshot> {
        self.snapshot.subscribe()
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn install_kind(&self) -> InstallKind {
        self.kind
    }

    fn apply(&self, event: Event) -> bool {
        let mut applied = false;
        self.snapshot.send_if_modified(|s| {
            applied = state::reduce(s, event);
            applied
        });
        applied
    }

    fn auto_enabled(&self) -> bool {
        self.settings.get_bool(SettingKey::AutoUpdateCheck)
    }

    /// Automatic checks: the first after `first_delay`, then every 6 h, re-checking soon after the setting
    /// is turned on or the channel changes. Idempotent.
    pub fn start_schedule(self: &Arc<Self>, first_delay: Duration) {
        {
            let mut inner = lock(&self.inner);
            if inner.scheduled {
                return;
            }
            inner.scheduled = true;
        }
        let this = Arc::downgrade(self);
        let mut changes = self.settings.subscribe();
        self.rt.spawn(async move {
            let mut next = tokio::time::Instant::now() + first_delay;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(next) => {
                        let Some(service) = this.upgrade() else { break };
                        if service.auto_enabled() && lock(&service.inner).blocked_until <= now_unix() {
                            service.check(false);
                        }
                        next = tokio::time::Instant::now() + CHECK_INTERVAL;
                    }
                    key = changes.recv() => match key {
                        Ok(SettingKey::AutoUpdateCheck | SettingKey::UpdateChannel) => {
                            next = tokio::time::Instant::now() + Duration::from_secs(2);
                        }
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        });
    }

    /// Check GitHub now. `manual` — the user asked (errors are shown); an automatic check that fails
    /// keeps the previous state quietly.
    pub fn check(self: &Arc<Self>, manual: bool) {
        let before = self.snapshot();
        if !self.apply(Event::CheckStarted { manual }) {
            return;
        }
        let this = self.clone();
        let task = self.rt.spawn(async move {
            match this.fetch_offer().await {
                Ok((offer, skipped)) => {
                    this.apply(Event::CheckFinished { offer, skipped, at: now_unix() });
                }
                Err(error) if manual => {
                    this.apply(Event::Failed { stage: Stage::Check, text: error.text, retryable: error.retryable });
                }
                Err(error) => {
                    tracing::warn!("update check: {}", error.text);
                    this.snapshot.send_if_modified(|s| {
                        if s.state != UpdateState::Checking {
                            return false;
                        }
                        let last_checked = s.last_checked;
                        *s = UpdateSnapshot { last_checked, ..before };
                        true
                    });
                }
            }
        });
        lock(&self.inner).task = Some(task);
    }

    async fn get_json(&self, client: &reqwest::Client, url: &str) -> Result<Option<String>, CheckError> {
        let cached = lock(&self.inner).etags.get(url).cloned();
        let mut request = client
            .get(url)
            .timeout(CHECK_TIMEOUT)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some((etag, _)) = &cached {
            request = request.header(IF_NONE_MATCH, etag.as_str());
        }
        let response = request.send().await.map_err(CheckError::net)?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED
            && let Some((_, body)) = cached
        {
            return Ok(Some(body));
        }
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
            let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
            let limited = header("x-ratelimit-remaining").as_deref() == Some("0") || status == StatusCode::TOO_MANY_REQUESTS;
            if limited {
                let reset = header("x-ratelimit-reset").and_then(|v| v.parse::<i64>().ok()).unwrap_or(now_unix() + 3600);
                let retry_after = header("retry-after").and_then(|v| v.parse::<i64>().ok()).map(|s| now_unix() + s);
                let until = retry_after.unwrap_or(reset);
                lock(&self.inner).blocked_until = until;
                return Err(CheckError {
                    text: mf_i18n::tr!("GitHub is limiting update checks. Try again after {0}", clock_time(until)),
                    retryable: true,
                });
            }
        }
        if !status.is_success() {
            return Err(CheckError { text: mf_i18n::tr!("GitHub answered {0}", status.as_u16()), retryable: true });
        }
        let etag = response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let body = response.text().await.map_err(CheckError::net)?;
        if let Some(etag) = etag {
            lock(&self.inner).etags.insert(url.to_owned(), (etag, body.clone()));
        }
        Ok(Some(body))
    }

    async fn fetch_offer(&self) -> Result<(Option<Offer>, bool), CheckError> {
        let client = self.http.client().map_err(CheckError::fatal)?;
        let channel = Channel::from_setting(&self.settings.get_str(SettingKey::UpdateChannel));
        let current = Version::parse(&current_version()).unwrap_or(Version::parse("0.0.0").expect("version"));
        let (url, list) = match channel {
            Channel::Stable => (format!("{API}/releases/latest"), false),
            Channel::Beta => (format!("{API}/releases?per_page=20"), true),
        };
        let Some(body) = self.get_json(&client, &url).await? else {
            return Ok((None, false));
        };
        let releases: Vec<release::GhRelease> = if list {
            serde_json::from_str(&body)
        } else {
            serde_json::from_str(&body).map(|r| vec![r])
        }
        .map_err(|e| CheckError { text: format!("GitHub: {e}"), retryable: true })?;
        let Some((found, version)) = release::pick_release(&releases, channel, &current) else {
            return Ok((None, false));
        };
        let asset = if self.kind.automatic() {
            release::select_asset(&found.assets, self.kind, std::env::consts::ARCH)
                .filter(|a| release::safe_file_name(&a.name).is_some())
        } else {
            None
        };
        let sums = found.assets.iter().find(|a| a.name == SUMS_NAME).map(|a| a.browser_download_url.clone());
        {
            let mut inner = lock(&self.inner);
            inner.sums_url = sums;
            inner.asset_digest = asset.and_then(|a| a.digest.clone());
        }
        let version = version.to_string();
        let offer = Offer {
            version: version.clone(),
            notes: found.body.clone().unwrap_or_default().trim().to_owned(),
            published_at: found.published_at.clone().unwrap_or_default(),
            release_url: if found.html_url.is_empty() { RELEASES_PAGE.to_owned() } else { found.html_url.clone() },
            prerelease: found.prerelease,
            asset_name: asset.map(|a| a.name.clone()).unwrap_or_default(),
            asset_url: asset.map(|a| a.browser_download_url.clone()).unwrap_or_default(),
            size: asset.map_or(0, |a| a.size),
        };
        let skipped = self.settings.get_str(SettingKey::SkippedVersion) == version;
        Ok((Some(offer), skipped))
    }

    /// Download (or resume) the offered asset, then verify it.
    pub fn download(self: &Arc<Self>) {
        let snapshot = self.snapshot();
        let Some(offer) = snapshot.offer.clone().filter(|o| !o.asset_url.is_empty()) else { return };
        let dir = updates_dir();
        let part = dir.join(format!("{}.part", offer.asset_name));
        let resumed_from = std::fs::metadata(&part).map_or(0, |m| m.len());
        if !self.apply(Event::DownloadStarted { resumed_from, total: offer.size }) {
            return;
        }
        let this = self.clone();
        let task = self.rt.spawn(async move {
            if let Err(error) = this.download_and_verify(&offer, &dir).await {
                this.apply(Event::Failed { stage: Stage::Download, text: error.text, retryable: error.retryable });
            }
        });
        lock(&self.inner).task = Some(task);
    }

    async fn expected_hash(&self, client: &reqwest::Client, offer: &Offer) -> Result<String, CheckError> {
        let (sums_url, digest) = {
            let inner = lock(&self.inner);
            (inner.sums_url.clone(), inner.asset_digest.clone())
        };
        let mut sums = HashMap::new();
        if let Some(url) = sums_url {
            let response = client.get(&url).timeout(CHECK_TIMEOUT).send().await.map_err(CheckError::net)?;
            if response.status().is_success() {
                sums = release::parse_sha256sums(&response.text().await.map_err(CheckError::net)?);
            }
        }
        let asset = release::GhAsset { name: offer.asset_name.clone(), digest, ..release::GhAsset::default() };
        release::expected_sha256(&sums, &asset).ok_or_else(|| {
            CheckError::fatal(mf_i18n::t("This release has no checksums, so it can't be installed safely. Download it from the release page"))
        })
    }

    async fn download_and_verify(self: &Arc<Self>, offer: &Offer, dir: &Path) -> Result<(), CheckError> {
        let client = self.http.client().map_err(CheckError::fatal)?;
        tokio::fs::create_dir_all(dir).await.map_err(|e| CheckError::fatal(e.to_string()))?;
        remove_stale(dir, &offer.asset_name).await;
        let expected = self.expected_hash(&client, offer).await?;
        let file = dir.join(&offer.asset_name);
        let part = dir.join(format!("{}.part", offer.asset_name));

        // already downloaded and intact (an earlier session, or Retry after an install error)
        if tokio::fs::metadata(&file).await.is_ok() && sha256_file(file.clone()).await.ok().as_deref() == Some(expected.as_str()) {
            self.apply(Event::Progress { downloaded: offer.size, total: offer.size, speed: None, eta: None });
            self.apply(Event::Downloaded);
            self.apply(Event::Verified);
            return Ok(());
        }
        let _ = tokio::fs::remove_file(&file).await;

        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.fetch_to(&client, offer, &part).await {
                Ok(()) => break,
                // 416: the partial file doesn't fit the asset any more — start over once
                Err(FetchError::RangeRejected) if attempt == 1 => {
                    let _ = tokio::fs::remove_file(&part).await;
                }
                Err(FetchError::RangeRejected) => {
                    return Err(CheckError { text: mf_i18n::t("The download failed. Try again"), retryable: true });
                }
                Err(FetchError::Check(error)) => return Err(error),
            }
        }

        if !self.apply(Event::Downloaded) {
            return Ok(());
        }
        let actual = sha256_file(part.clone()).await.map_err(|e| CheckError::fatal(e.to_string()))?;
        if actual != expected {
            let _ = tokio::fs::remove_file(&part).await;
            tracing::warn!("update {}: sha256 {actual}, expected {expected}", offer.asset_name);
            return Err(CheckError {
                text: mf_i18n::t("The downloaded file is damaged (checksum mismatch). Try again"),
                retryable: true,
            });
        }
        tokio::fs::rename(&part, &file).await.map_err(|e| CheckError::fatal(e.to_string()))?;
        self.apply(Event::Verified);
        Ok(())
    }

    async fn fetch_to(&self, client: &reqwest::Client, offer: &Offer, part: &Path) -> Result<(), FetchError> {
        let mut have = tokio::fs::metadata(part).await.map_or(0, |m| m.len());
        if offer.size > 0 && have == offer.size {
            return Ok(());
        }
        if offer.size > 0 && have > offer.size {
            let _ = tokio::fs::remove_file(part).await;
            have = 0;
        }
        let mut request = client.get(&offer.asset_url).timeout(DOWNLOAD_TIMEOUT);
        if have > 0 {
            request = request.header(RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().await.map_err(|e| FetchError::Check(CheckError::net(e)))?;
        let status = response.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(FetchError::RangeRejected);
        }
        if !status.is_success() {
            return Err(FetchError::Check(CheckError {
                text: mf_i18n::tr!("The download failed: the server answered {0}", status.as_u16()),
                retryable: true,
            }));
        }
        let append = have > 0 && status == StatusCode::PARTIAL_CONTENT;
        if !append {
            have = 0;
        }
        let length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let total = length.map_or(offer.size, |len| len + have);
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(part)
            .await
            .map_err(|e| FetchError::Check(CheckError::fatal(e.to_string())))?;

        let started = Instant::now();
        let mut meter = progress::RateMeter::default();
        let mut done = have;
        let mut last_emit = Instant::now() - PROGRESS_EVERY;
        meter.sample(0.0, done);
        self.apply(Event::Progress { downloaded: done, total, speed: None, eta: None });
        loop {
            let chunk = match tokio::time::timeout(STALL_TIMEOUT, response.chunk()).await {
                Err(_) => {
                    return Err(FetchError::Check(CheckError {
                        text: mf_i18n::t("The download stalled. Check your connection and try again"),
                        retryable: true,
                    }));
                }
                Ok(Err(error)) => return Err(FetchError::Check(CheckError::net(error))),
                Ok(Ok(None)) => break,
                Ok(Ok(Some(chunk))) => chunk,
            };
            file.write_all(&chunk).await.map_err(|e| FetchError::Check(CheckError::fatal(e.to_string())))?;
            done += chunk.len() as u64;
            let rate = meter.sample(started.elapsed().as_secs_f64(), done);
            if last_emit.elapsed() >= PROGRESS_EVERY {
                last_emit = Instant::now();
                let eta = progress::eta_secs(done, total, rate);
                self.apply(Event::Progress { downloaded: done, total, speed: rate, eta });
            }
        }
        file.flush().await.map_err(|e| FetchError::Check(CheckError::fatal(e.to_string())))?;
        let _ = file.sync_all().await;
        drop(file);
        self.apply(Event::Progress { downloaded: done, total, speed: meter.rate(), eta: Some(0) });
        if total > 0 && done != total {
            return Err(FetchError::Check(CheckError {
                text: mf_i18n::t("The download was interrupted. Try again to continue"),
                retryable: true,
            }));
        }
        Ok(())
    }

    /// Stop the download (the partial file stays for resume) or a running check.
    pub fn cancel(&self) {
        let task = lock(&self.inner).task.take();
        if let Some(task) = task {
            task.abort();
        }
        let checking = self.snapshot.borrow().state == UpdateState::Checking;
        if checking {
            self.snapshot.send_if_modified(|s| {
                s.state = if s.offer.is_some() { UpdateState::Available } else { UpdateState::Idle };
                true
            });
        } else {
            self.apply(Event::Cancelled);
        }
    }

    /// Retry after an error: check again, download again (resumes), or re-verify and install.
    pub fn retry(self: &Arc<Self>) {
        let snapshot = self.snapshot();
        if snapshot.state != UpdateState::Error {
            return;
        }
        match snapshot.error_stage {
            Stage::Check => self.check(true),
            Stage::Download | Stage::Install => self.download(),
        }
    }

    /// Apply the verified update. Windows/AppImage: the app quits and the new version starts; macOS: the
    /// disk image opens and the app keeps running.
    pub fn install(self: &Arc<Self>) {
        let snapshot = self.snapshot();
        let Some(offer) = snapshot.offer.clone().filter(|o| !o.asset_name.is_empty()) else { return };
        if !self.apply(Event::InstallStarted) {
            return;
        }
        let this = self.clone();
        let kind = self.kind;
        self.rt.spawn(async move {
            let dir = updates_dir();
            let file = dir.join(&offer.asset_name);
            // the file could have changed on disk since it was verified
            let client = this.http.client().ok();
            let expected = match &client {
                Some(client) => this.expected_hash(client, &offer).await.ok(),
                None => None,
            };
            let actual = sha256_file(file.clone()).await.ok();
            if expected.is_none() || expected != actual {
                this.apply(Event::Failed {
                    stage: Stage::Install,
                    text: mf_i18n::t("The downloaded update is missing or damaged. Try again"),
                    retryable: true,
                });
                return;
            }
            let result = tokio::task::spawn_blocking(move || install::install(kind, &file, &dir))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(install::Outcome::Quit) => {
                    tokio::time::sleep(QUIT_DELAY).await;
                    let _ = this.app_requests.send(AppRequest::Quit);
                }
                Ok(install::Outcome::HandedOff) => {
                    this.apply(Event::InstallHandedOff {
                        note: mf_i18n::t("Drag Materialfy to Applications in the window that opened, then restart the app"),
                    });
                }
                Err(text) => {
                    this.apply(Event::Failed {
                        stage: Stage::Install,
                        text: mf_i18n::tr!("Couldn't install the update: {0}", text),
                        retryable: true,
                    });
                }
            }
        });
    }

    /// Don't offer this version again (a newer one is offered as usual).
    pub fn skip(&self) {
        let Some(version) = self.snapshot.borrow().offer.as_ref().map(|o| o.version.clone()) else { return };
        self.settings.set(SettingKey::SkippedVersion, SettingValue::Str(version));
        self.apply(Event::Skipped);
    }

    pub fn release_url(&self) -> String {
        self.snapshot.borrow().offer.as_ref().map_or_else(|| RELEASES_PAGE.to_owned(), |o| o.release_url.clone())
    }

    pub fn open_release_page(&self) {
        if let Err(error) = sf_auth::open_browser(&self.release_url()) {
            tracing::warn!("release page: {error}");
        }
    }
}

enum FetchError {
    RangeRejected,
    Check(CheckError),
}

/// Old downloads (other versions) and leftovers of earlier installs.
async fn remove_stale(dir: &Path, keep: &str) {
    let keep_part = format!("{keep}.part");
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else { return };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == keep || name == keep_part || name == "update.log" {
            continue;
        }
        let path = entry.path();
        let _ = if path.is_dir() { tokio::fs::remove_dir_all(&path).await } else { tokio::fs::remove_file(&path).await };
    }
}

/// Lowercase hex SHA-256 of a file (on the blocking pool).
pub async fn sha256_file(path: PathBuf) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        use sf_api::sha2::{Digest, Sha256};
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hex(&hasher.finalize()))
    })
    .await
    .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

/// "14:05" in local time.
fn clock_time(unix: i64) -> String {
    jiff::Timestamp::from_second(unix)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sha256_of_file() {
        let path = std::env::temp_dir().join(format!("mf-update-sha-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(path.clone()).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            sha256_file(path.clone()).await.unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = std::fs::remove_file(&path);
        assert!(sha256_file(path).await.is_err());
    }

    #[test]
    fn versions_and_kind() {
        assert!(Version::parse(&current_version()).is_some());
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert!(!clock_time(1_791_741_253).is_empty());
    }

    #[tokio::test]
    async fn stale_downloads_are_removed() {
        let dir = std::env::temp_dir().join(format!("mf-update-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("portable")).unwrap();
        for name in ["Materialfy-0.1.0-Setup.exe", "Materialfy-0.2.0-Setup.exe", "Materialfy-0.2.0-Setup.exe.part", "update.log"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        remove_stale(&dir, "Materialfy-0.2.0-Setup.exe").await;
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["Materialfy-0.2.0-Setup.exe", "Materialfy-0.2.0-Setup.exe.part", "update.log"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
