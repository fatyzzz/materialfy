//! Update state machine: the snapshot the UI shows and the pure transitions between states.
//!
//! idle → checking → upToDate | available | error
//! available → downloading → verifying → readyToInstall → installing
//! downloading → available (cancel; the partial file is kept for resume) | error
//! verifying → error (checksum mismatch: the file is deleted)
//! error → checking | downloading (retry)
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verifying,
    ReadyToInstall,
    Installing,
    Error,
}

impl UpdateState {
    /// Name for QML ("upToDate", "readyToInstall", …).
    pub fn code(self) -> &'static str {
        match self {
            UpdateState::Idle => "idle",
            UpdateState::Checking => "checking",
            UpdateState::UpToDate => "upToDate",
            UpdateState::Available => "available",
            UpdateState::Downloading => "downloading",
            UpdateState::Verifying => "verifying",
            UpdateState::ReadyToInstall => "readyToInstall",
            UpdateState::Installing => "installing",
            UpdateState::Error => "error",
        }
    }

    /// A download, verification or install is running — a new check must not interrupt it.
    pub fn busy(self) -> bool {
        matches!(self, UpdateState::Downloading | UpdateState::Verifying | UpdateState::Installing)
    }
}

/// Where an error happened: decides what Retry does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Check,
    Download,
    Install,
}

/// The release on offer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offer {
    pub version: String,
    pub notes: String,
    /// ISO 8601
    pub published_at: String,
    pub release_url: String,
    pub prerelease: bool,
    /// empty — this install can't be updated automatically (only the release page)
    pub asset_name: String,
    pub asset_url: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSnapshot {
    pub state: UpdateState,
    pub current_version: String,
    /// "installer" | "portable" | "appimage" | "mac" | "manual" | "dev"
    pub install_kind: String,
    pub offer: Option<Offer>,
    /// the offered version is the one the user skipped
    pub skipped: bool,
    /// the last check was started by the user (Settings shows its result)
    pub manual: bool,
    pub downloaded: u64,
    pub total: u64,
    /// bytes/s, 0 — unknown
    pub speed: f64,
    /// seconds, -1 — unknown
    pub eta: i64,
    pub error_text: String,
    pub retryable: bool,
    pub error_stage: Stage,
    /// unix seconds of the last finished check, 0 — never
    pub last_checked: i64,
    /// a hint after a non-restarting install (macOS: the disk image is open)
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    CheckStarted { manual: bool },
    /// None — up to date
    CheckFinished { offer: Option<Offer>, skipped: bool, at: i64 },
    DownloadStarted { resumed_from: u64, total: u64 },
    Progress { downloaded: u64, total: u64, speed: Option<f64>, eta: Option<u64> },
    Downloaded,
    Verified,
    Cancelled,
    InstallStarted,
    /// installer opened, the app keeps running (macOS dmg)
    InstallHandedOff { note: String },
    Failed { stage: Stage, text: String, retryable: bool },
    /// the user skipped the offered version
    Skipped,
}

/// Applies an event; false — the event doesn't apply in the current state (the snapshot is unchanged).
pub fn reduce(s: &mut UpdateSnapshot, event: Event) -> bool {
    use UpdateState::*;
    match event {
        Event::CheckStarted { manual } => {
            if s.state.busy() || s.state == ReadyToInstall {
                return false;
            }
            s.state = Checking;
            s.manual = manual;
            s.error_text.clear();
            s.retryable = false;
        }
        Event::CheckFinished { offer, skipped, at } => {
            if s.state != Checking {
                return false;
            }
            s.last_checked = at;
            // a different version than before: the old progress means nothing
            if s.offer.as_ref().map(|o| &o.version) != offer.as_ref().map(|o| &o.version) {
                s.downloaded = 0;
                s.total = 0;
            }
            s.state = if offer.is_some() { Available } else { UpToDate };
            s.skipped = offer.is_some() && skipped;
            s.offer = offer;
        }
        Event::DownloadStarted { resumed_from, total } => {
            if !matches!(s.state, Available | Error) || s.offer.as_ref().is_none_or(|o| o.asset_url.is_empty()) {
                return false;
            }
            s.state = Downloading;
            s.downloaded = resumed_from;
            s.total = total;
            s.speed = 0.0;
            s.eta = -1;
            s.error_text.clear();
            s.retryable = false;
        }
        Event::Progress { downloaded, total, speed, eta } => {
            if s.state != Downloading {
                return false;
            }
            s.downloaded = downloaded;
            s.total = total;
            s.speed = speed.unwrap_or(0.0);
            s.eta = eta.map_or(-1, |e| e.min(i64::MAX as u64) as i64);
        }
        Event::Downloaded => {
            if s.state != Downloading {
                return false;
            }
            s.state = Verifying;
            s.downloaded = s.total.max(s.downloaded);
            s.speed = 0.0;
            s.eta = -1;
        }
        Event::Verified => {
            if s.state != Verifying {
                return false;
            }
            s.state = ReadyToInstall;
        }
        Event::Cancelled => {
            if !matches!(s.state, Downloading | Verifying) {
                return false;
            }
            s.state = Available;
            s.speed = 0.0;
            s.eta = -1;
        }
        Event::InstallStarted => {
            if s.state != ReadyToInstall {
                return false;
            }
            s.state = Installing;
            s.note.clear();
        }
        Event::InstallHandedOff { note } => {
            if s.state != Installing {
                return false;
            }
            s.state = ReadyToInstall;
            s.note = note;
        }
        Event::Failed { stage, text, retryable } => {
            s.state = Error;
            s.error_stage = stage;
            s.error_text = text;
            s.retryable = retryable;
            s.speed = 0.0;
            s.eta = -1;
            if stage == Stage::Check {
                s.offer = None;
                s.skipped = false;
            }
        }
        Event::Skipped => {
            if s.state != Available || s.offer.is_none() {
                return false;
            }
            s.skipped = true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(version: &str) -> Offer {
        Offer {
            version: version.into(),
            asset_name: "Materialfy-0.2.0-Setup.exe".into(),
            asset_url: "https://example.invalid/x".into(),
            size: 100,
            ..Offer::default()
        }
    }

    fn checked(version: Option<&str>) -> UpdateSnapshot {
        let mut s = UpdateSnapshot::default();
        assert!(reduce(&mut s, Event::CheckStarted { manual: false }));
        assert!(reduce(&mut s, Event::CheckFinished { offer: version.map(offer), skipped: false, at: 5 }));
        s
    }

    #[test]
    fn check_results() {
        let s = checked(None);
        assert_eq!(s.state, UpdateState::UpToDate);
        assert_eq!(s.last_checked, 5);
        let s = checked(Some("0.2.0"));
        assert_eq!(s.state, UpdateState::Available);
        assert_eq!(s.offer.as_ref().unwrap().version, "0.2.0");
        // a finished check outside Checking is ignored
        let mut s = checked(None);
        assert!(!reduce(&mut s, Event::CheckFinished { offer: Some(offer("9.0.0")), skipped: false, at: 6 }));
        assert_eq!(s.state, UpdateState::UpToDate);
    }

    #[test]
    fn full_download_path() {
        let mut s = checked(Some("0.2.0"));
        assert!(reduce(&mut s, Event::DownloadStarted { resumed_from: 0, total: 100 }));
        assert_eq!(s.state, UpdateState::Downloading);
        // a check can't interrupt a download
        assert!(!reduce(&mut s, Event::CheckStarted { manual: true }));
        assert!(reduce(&mut s, Event::Progress { downloaded: 40, total: 100, speed: Some(10.0), eta: Some(6) }));
        assert_eq!((s.downloaded, s.speed, s.eta), (40, 10.0, 6));
        assert!(reduce(&mut s, Event::Progress { downloaded: 50, total: 100, speed: None, eta: None }));
        assert_eq!((s.speed, s.eta), (0.0, -1));
        assert!(reduce(&mut s, Event::Downloaded));
        assert_eq!(s.state, UpdateState::Verifying);
        assert_eq!(s.downloaded, 100);
        assert!(!reduce(&mut s, Event::InstallStarted));
        assert!(reduce(&mut s, Event::Verified));
        assert_eq!(s.state, UpdateState::ReadyToInstall);
        // ready: a check doesn't throw the verified file away
        assert!(!reduce(&mut s, Event::CheckStarted { manual: false }));
        assert!(reduce(&mut s, Event::InstallStarted));
        assert_eq!(s.state, UpdateState::Installing);
        assert!(reduce(&mut s, Event::InstallHandedOff { note: "open".into() }));
        assert_eq!((s.state, s.note.as_str()), (UpdateState::ReadyToInstall, "open"));
    }

    #[test]
    fn cancel_and_resume() {
        let mut s = checked(Some("0.2.0"));
        assert!(!reduce(&mut s, Event::Cancelled));
        assert!(reduce(&mut s, Event::DownloadStarted { resumed_from: 0, total: 100 }));
        assert!(reduce(&mut s, Event::Progress { downloaded: 30, total: 100, speed: Some(1.0), eta: Some(70) }));
        assert!(reduce(&mut s, Event::Cancelled));
        assert_eq!(s.state, UpdateState::Available);
        assert_eq!(s.downloaded, 30, "the partial size is kept to show resume");
        assert!(!reduce(&mut s, Event::Progress { downloaded: 31, total: 100, speed: None, eta: None }));
        assert!(reduce(&mut s, Event::DownloadStarted { resumed_from: 30, total: 100 }));
        assert_eq!(s.downloaded, 30);
        // the same version found again keeps the progress, a new one resets it
        let mut s2 = s.clone();
        s2.state = UpdateState::Available;
        assert!(reduce(&mut s2, Event::CheckStarted { manual: true }));
        assert!(reduce(&mut s2, Event::CheckFinished { offer: Some(offer("0.2.0")), skipped: false, at: 9 }));
        assert_eq!(s2.downloaded, 30);
        assert!(reduce(&mut s2, Event::CheckStarted { manual: true }));
        assert!(reduce(&mut s2, Event::CheckFinished { offer: Some(offer("0.3.0")), skipped: false, at: 10 }));
        assert_eq!(s2.downloaded, 0);
    }

    #[test]
    fn errors_and_retry() {
        let mut s = checked(Some("0.2.0"));
        assert!(reduce(&mut s, Event::DownloadStarted { resumed_from: 0, total: 100 }));
        assert!(reduce(&mut s, Event::Failed { stage: Stage::Download, text: "net".into(), retryable: true }));
        assert_eq!((s.state, s.error_stage, s.retryable), (UpdateState::Error, Stage::Download, true));
        assert!(s.offer.is_some(), "a download error keeps the offer for Retry");
        assert!(reduce(&mut s, Event::DownloadStarted { resumed_from: 10, total: 100 }));
        assert!(s.error_text.is_empty());
        assert!(reduce(&mut s, Event::Failed { stage: Stage::Check, text: "rate".into(), retryable: true }));
        assert!(s.offer.is_none());
        // without an offer there is nothing to download
        assert!(!reduce(&mut s, Event::DownloadStarted { resumed_from: 0, total: 1 }));
        assert!(reduce(&mut s, Event::CheckStarted { manual: true }));
        assert!(s.manual && s.error_text.is_empty());
    }

    #[test]
    fn manual_offer_cant_download_and_skip() {
        let mut s = UpdateSnapshot::default();
        reduce(&mut s, Event::CheckStarted { manual: false });
        let manual = Offer { version: "0.2.0".into(), ..Offer::default() };
        reduce(&mut s, Event::CheckFinished { offer: Some(manual), skipped: true, at: 1 });
        assert!(s.skipped);
        assert!(!reduce(&mut s, Event::DownloadStarted { resumed_from: 0, total: 1 }));
        let mut s = checked(Some("0.2.0"));
        assert!(!s.skipped);
        assert!(reduce(&mut s, Event::Skipped));
        assert!(s.skipped);
        assert_eq!(UpdateState::ReadyToInstall.code(), "readyToInstall");
        assert_eq!(UpdateState::UpToDate.code(), "upToDate");
    }
}
