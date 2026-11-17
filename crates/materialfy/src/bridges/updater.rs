// Updater — mirror of sf_services::UpdateService (auto-update from GitHub Releases).
// state: "idle" | "checking" | "upToDate" | "available" | "downloading" | "verifying" | "readyToInstall" |
// "installing" | "error". Download numbers have their own signal (progressChanged, ~8 times a second) so the
// rest of the UI (release notes) doesn't re-evaluate on every tick. autoCheck/channel are the settings
// autoUpdateCheck/updateChannel.
//
// Testing: MATERIALFY_UPDATE_FAKE_VERSION=0.0.1 (this build looks older), MATERIALFY_UPDATE_DEBUG=check,sheet,download
// (a manual check right away / open the sheet after the first check / start the download as soon as an update is
// found; any of them also enables checks in --screenshot mode).
use std::pin::Pin;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use mf_platform::{SettingKey, SettingValue};
use sf_services::update::{self, UpdateSnapshot, UpdateState};

use super::{Delivery, forward_events, forward_watch};
use crate::context::ctx;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(QString, state, READ, NOTIFY = state_changed)]
        #[qproperty(QString, current_version, cxx_name = "currentVersion", READ, NOTIFY = state_changed)]
        /// version of this build (Cargo.toml), unlike currentVersion never faked
        #[qproperty(QString, app_version, cxx_name = "appVersion", READ, CONSTANT)]
        /// short commit hash of a CI build ("" for local builds)
        #[qproperty(QString, build_commit, cxx_name = "buildCommit", READ, CONSTANT)]
        /// "installer" | "portable" | "appimage" | "mac" | "manual" | "dev"
        #[qproperty(QString, install_kind, cxx_name = "installKind", READ, CONSTANT)]
        #[qproperty(QString, version, READ, NOTIFY = state_changed)]
        /// release notes, Markdown
        #[qproperty(QString, notes, READ, NOTIFY = state_changed)]
        /// ISO 8601
        #[qproperty(QString, published_at, cxx_name = "publishedAt", READ, NOTIFY = state_changed)]
        #[qproperty(QString, release_url, cxx_name = "releaseUrl", READ, NOTIFY = state_changed)]
        #[qproperty(bool, prerelease, READ, NOTIFY = state_changed)]
        /// size of the file to download, bytes (0 — nothing to download: release page only)
        #[qproperty(f64, size, READ, NOTIFY = state_changed)]
        /// the app can download and apply this update itself
        #[qproperty(bool, can_install, cxx_name = "canInstall", READ, NOTIFY = state_changed)]
        /// the offered version is the one the user skipped (no banner)
        #[qproperty(bool, skipped, READ, NOTIFY = state_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = state_changed)]
        #[qproperty(bool, retryable, READ, NOTIFY = state_changed)]
        /// ms since the epoch of the last finished check, 0 — never
        #[qproperty(f64, last_checked, cxx_name = "lastChecked", READ, NOTIFY = state_changed)]
        /// hint after an install that doesn't restart the app (macOS)
        #[qproperty(QString, note, READ, NOTIFY = state_changed)]
        #[qproperty(f64, downloaded, READ, NOTIFY = progress_changed)]
        #[qproperty(f64, total, READ, NOTIFY = progress_changed)]
        /// 0..1
        #[qproperty(f64, progress, READ, NOTIFY = progress_changed)]
        /// bytes/s, 0 — unknown
        #[qproperty(f64, speed, READ, NOTIFY = progress_changed)]
        /// seconds left, -1 — unknown
        #[qproperty(i32, eta, READ, NOTIFY = progress_changed)]
        #[qproperty(bool, auto_check, cxx_name = "autoCheck", READ, WRITE = set_auto_check, NOTIFY = settings_changed)]
        /// "stable" | "beta"
        #[qproperty(QString, channel, READ, WRITE = set_channel, NOTIFY = settings_changed)]
        type Updater = super::UpdaterRust;

        fn set_auto_check(self: Pin<&mut Updater>, value: bool);
        fn set_channel(self: Pin<&mut Updater>, value: QString);

        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Updater>);
        #[qsignal]
        #[cxx_name = "progressChanged"]
        fn progress_changed(self: Pin<&mut Updater>);
        #[qsignal]
        #[cxx_name = "settingsChanged"]
        fn settings_changed(self: Pin<&mut Updater>);
        /// someone asked to show the update sheet (Settings, the sidebar pill)
        #[qsignal]
        #[cxx_name = "sheetRequested"]
        fn sheet_requested(self: Pin<&mut Updater>);

        /// Check GitHub now (works with automatic checks off).
        #[qinvokable]
        fn check(self: &Updater);
        /// Download the update (resumes a cancelled download) and verify it.
        #[qinvokable]
        fn download(self: &Updater);
        /// Stop the download or the check.
        #[qinvokable]
        fn cancel(self: &Updater);
        /// After an error: check / download / install again, whichever failed.
        #[qinvokable]
        fn retry(self: &Updater);
        /// Apply the verified update (Windows/Linux: the app quits and the new version starts).
        #[qinvokable]
        fn install(self: &Updater);
        /// Don't offer this version again.
        #[qinvokable]
        fn skip(self: &Updater);
        #[qinvokable]
        #[cxx_name = "openReleasePage"]
        fn open_release_page(self: &Updater);
        #[qinvokable]
        #[cxx_name = "showSheet"]
        fn show_sheet(self: Pin<&mut Updater>);
    }

    impl cxx_qt::Threading for Updater {}
    impl cxx_qt::Initialize for Updater {}
}

#[derive(Default)]
pub struct UpdaterRust {
    snapshot: UpdateSnapshot,
    state: QString,
    current_version: QString,
    app_version: QString,
    build_commit: QString,
    install_kind: QString,
    version: QString,
    notes: QString,
    published_at: QString,
    release_url: QString,
    prerelease: bool,
    size: f64,
    can_install: bool,
    skipped: bool,
    error_text: QString,
    retryable: bool,
    last_checked: f64,
    note: QString,
    downloaded: f64,
    total: f64,
    progress: f64,
    speed: f64,
    eta: i32,
    auto_check: bool,
    channel: QString,
    /// MATERIALFY_UPDATE_DEBUG
    debug_sheet: bool,
    debug_download: bool,
}

fn qs(text: &str) -> QString {
    QString::from(text)
}

impl qobject::Updater {
    fn check(&self) {
        ctx().services.update.check(true);
    }

    fn download(&self) {
        ctx().services.update.download();
    }

    fn cancel(&self) {
        ctx().services.update.cancel();
    }

    fn retry(&self) {
        ctx().services.update.retry();
    }

    fn install(&self) {
        ctx().services.update.install();
    }

    fn skip(&self) {
        ctx().services.update.skip();
    }

    fn open_release_page(&self) {
        ctx().services.update.open_release_page();
    }

    fn show_sheet(self: Pin<&mut Self>) {
        self.sheet_requested();
    }

    fn set_auto_check(mut self: Pin<&mut Self>, value: bool) {
        if self.auto_check == value {
            return;
        }
        self.as_mut().rust_mut().auto_check = value;
        ctx().services.settings.set(SettingKey::AutoUpdateCheck, SettingValue::Bool(value));
        self.settings_changed();
    }

    fn set_channel(mut self: Pin<&mut Self>, value: QString) {
        if self.channel == value {
            return;
        }
        ctx().services.settings.set(SettingKey::UpdateChannel, SettingValue::Str(value.to_string()));
        // the store normalizes ("stable" | "beta")
        let stored = qs(&ctx().services.settings.get_str(SettingKey::UpdateChannel));
        self.as_mut().rust_mut().channel = stored;
        self.settings_changed();
    }

    fn load_settings(mut self: Pin<&mut Self>) -> bool {
        let settings = &ctx().services.settings;
        let auto_check = settings.get_bool(SettingKey::AutoUpdateCheck);
        let channel = qs(&settings.get_str(SettingKey::UpdateChannel));
        if self.auto_check == auto_check && self.channel == channel {
            return false;
        }
        let mut this = self.as_mut().rust_mut();
        this.auto_check = auto_check;
        this.channel = channel;
        true
    }

    fn on_setting(mut self: Pin<&mut Self>, delivery: Delivery<SettingKey>) {
        let relevant = match delivery {
            Delivery::Event(key) => matches!(key, SettingKey::AutoUpdateCheck | SettingKey::UpdateChannel),
            Delivery::Lagged => true,
        };
        if relevant && self.as_mut().load_settings() {
            self.settings_changed();
        }
    }

    /// Snapshot → properties; signals only for what changed.
    fn apply(mut self: Pin<&mut Self>, next: UpdateSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        let progress = prev.downloaded != next.downloaded
            || prev.total != next.total
            || prev.speed != next.speed
            || prev.eta != next.eta;
        let rest = UpdateSnapshot { downloaded: 0, total: 0, speed: 0.0, eta: 0, ..prev.clone() }
            != UpdateSnapshot { downloaded: 0, total: 0, speed: 0.0, eta: 0, ..next.clone() };
        {
            let offer = next.offer.clone().unwrap_or_default();
            let mut this = self.as_mut().rust_mut();
            this.state = qs(next.state.code());
            this.current_version = qs(&next.current_version);
            this.version = qs(&offer.version);
            this.notes = qs(&offer.notes);
            this.published_at = qs(&offer.published_at);
            this.release_url = qs(&if offer.release_url.is_empty() { update::releases_page() } else { offer.release_url.clone() });
            this.prerelease = offer.prerelease;
            this.size = offer.size as f64;
            this.can_install = !offer.asset_url.is_empty();
            this.skipped = next.skipped;
            this.error_text = qs(&next.error_text);
            this.retryable = next.retryable;
            this.last_checked = next.last_checked as f64 * 1000.0;
            this.note = qs(&next.note);
            this.downloaded = next.downloaded as f64;
            this.total = if next.total > 0 { next.total as f64 } else { offer.size as f64 };
            this.progress = update::progress::fraction(next.downloaded, if next.total > 0 { next.total } else { offer.size });
            this.speed = next.speed;
            this.eta = i32::try_from(next.eta).unwrap_or(i32::MAX);
        }
        if rest {
            self.as_mut().state_changed();
        }
        if progress || rest {
            self.as_mut().progress_changed();
        }
        // MATERIALFY_UPDATE_DEBUG: show the sheet after the first check, start the download once an update is found
        if prev.state == UpdateState::Checking && next.state != UpdateState::Checking && self.debug_sheet {
            self.as_mut().rust_mut().debug_sheet = false;
            self.as_mut().sheet_requested();
        }
        if prev.state != UpdateState::Available && next.state == UpdateState::Available {
            if self.debug_download {
                self.as_mut().rust_mut().debug_download = false;
                ctx().services.update.download();
            }
        }
    }
}

impl cxx_qt::Initialize for qobject::Updater {
    fn initialize(mut self: Pin<&mut Self>) {
        let service = ctx().services.update.clone();
        let debug = std::env::var("MATERIALFY_UPDATE_DEBUG").unwrap_or_default();
        {
            let mut this = self.as_mut().rust_mut();
            this.install_kind = qs(service.install_kind().code());
            this.app_version = qs(env!("CARGO_PKG_VERSION"));
            this.build_commit = qs(option_env!("MATERIALFY_COMMIT").unwrap_or_default());
            this.debug_sheet = debug.contains("sheet");
            this.debug_download = debug.contains("download");
            this.eta = -1;
        }
        self.as_mut().load_settings();
        let mut watch = service.watch();
        let snapshot = watch.borrow_and_update().clone();
        // the first snapshot is published as a whole
        self.as_mut().rust_mut().snapshot = UpdateSnapshot { current_version: "-".into(), ..UpdateSnapshot::default() };
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::apply);
        forward_events(self.qt_thread(), ctx().services.settings.subscribe(), Self::on_setting);

        // no automatic checks in screenshot mode (unless an update is being simulated)
        let testing = update::fake_version_set() || !debug.is_empty();
        if debug.contains("check") {
            let service = service.clone();
            ctx().rt.spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                service.check(true);
            });
        } else if !ctx().flags.screenshot || testing {
            let delay = if testing { Duration::from_secs(1) } else { update::START_DELAY };
            service.start_schedule(delay);
        }
    }
}
