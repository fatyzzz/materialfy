//! CrashReport: прошлый запуск упал или зависал — в crash.log (main.rs, cpp/mf_shell.cpp) появились новые записи.
//! Окно при старте предлагает сообщить: папка с журналом и новый issue на GitHub с заготовкой текста.
//! «Просмотрено» — размер и время файла в crash.log.seen рядом с ним.
use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        /// в crash.log есть записи, которых пользователь ещё не видел
        #[qproperty(bool, pending, READ, NOTIFY = pending_changed)]
        /// file:/// URL папки с crash.log
        #[qproperty(QString, folder, READ, CONSTANT)]
        /// новый issue на GitHub: версия и ОС уже вписаны
        #[qproperty(QString, issue_url, cxx_name = "issueUrl", READ, CONSTANT)]
        type CrashReport = super::CrashReportRust;

        #[qsignal]
        #[cxx_name = "pendingChanged"]
        fn pending_changed(self: Pin<&mut CrashReport>);

        /// Пометить текущие записи просмотренными.
        #[qinvokable]
        fn dismiss(self: Pin<&mut CrashReport>);
    }
}

pub struct CrashReportRust {
    pending: bool,
    folder: QString,
    issue_url: QString,
}

fn log_path() -> PathBuf {
    mf_platform::dirs::app_data_dir().join("crash.log")
}

fn seen_path() -> PathBuf {
    mf_platform::dirs::app_data_dir().join("crash.log.seen")
}

/// Отпечаток журнала: размер и время изменения (журнал обрезается до 64 КиБ, так что одного размера мало).
fn stamp() -> Option<String> {
    let meta = std::fs::metadata(log_path()).ok().filter(|meta| meta.len() > 0)?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{} {modified}", meta.len()))
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn issue_url() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let title = format!("Crash report ({version}, {})", std::env::consts::OS);
    // на телефоне файл в форму не перетащить: хвост crash.log — прямо в текст (ссылка не длиннее ~8 КБ)
    let log = if cfg!(target_os = "android") {
        let text = std::fs::read_to_string(log_path()).unwrap_or_default();
        let tail = text.char_indices().rev().nth(2500).map_or(text.as_str(), |(at, _)| &text[at..]);
        format!("**crash.log** (end):\n```\n{}\n```", tail.trim())
    } else {
        "**crash.log**: drag the file from the folder Materialfy opened into this box.".to_owned()
    };
    let body = format!(
        "**What were you doing when it happened?**\n\n\n{log}\n\nMaterialfy {version}, {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    // отчёт — туда же, откуда приходят обновления этой сборки (на Android — форк с APK)
    format!(
        "https://github.com/{}/issues/new?title={}&body={}",
        sf_services::update::REPO,
        percent_encode(&title),
        percent_encode(&body)
    )
}

impl Default for CrashReportRust {
    fn default() -> Self {
        let seen = std::fs::read_to_string(seen_path()).unwrap_or_default();
        let pending = stamp().is_some_and(|stamp| stamp != seen.trim());
        let folder = mf_platform::dirs::app_data_dir();
        let folder = format!("file:///{}", folder.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
        Self { pending, folder: QString::from(folder.as_str()), issue_url: QString::from(issue_url().as_str()) }
    }
}

impl qobject::CrashReport {
    fn dismiss(mut self: Pin<&mut Self>) {
        if let Some(stamp) = stamp() {
            let _ = std::fs::write(seen_path(), stamp);
        }
        if self.pending {
            self.as_mut().rust_mut().pending = false;
            self.as_mut().pending_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_issue_link_is_encoded() {
        assert_eq!(percent_encode("a b/ё"), "a%20b%2F%D1%91");
        let url = issue_url();
        assert!(url.starts_with(&format!("https://github.com/{}/issues/new?title=Crash%20report%20%28", sf_services::update::REPO)));
        assert!(!url.contains(' ') && !url.contains('\n'));
    }
}
