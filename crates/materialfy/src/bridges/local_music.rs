// LocalMusic: страница «Локальные файлы»: папка из настроек (localMusicFolder), обход mf-local в spawn_blocking
// (кэш тегов в cache_subdir("local"), повторный обход не перечитывает неизменённые файлы), список треков и запуск
// их в плеере файлов (sf_player::FilePlayer), отдельно от очереди Spotify.
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use md3_model::TrackItem;
use mf_local::LocalTrack;
use mf_platform::{SettingKey, SettingValue};
use sf_player::FileSnapshot;

use super::{forward_watch, local_path, model_ptr, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_rows};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        /// выбранная папка; пустая, пока не выбрана
        #[qproperty(QString, folder, READ, NOTIFY = folder_changed)]
        #[qproperty(*mut QObject, tracks, READ = tracks_ptr, CONSTANT)]
        #[qproperty(bool, scanning, READ, NOTIFY = state_changed)]
        /// папки больше нет (удалили, отключили диск)
        #[qproperty(bool, missing, READ, NOTIFY = state_changed)]
        /// последняя ошибка плеера файлов; пустая, если её нет
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        type LocalMusic = super::LocalMusicRust;

        fn tracks_ptr(self: &LocalMusic) -> *mut QObject;

        #[qsignal]
        #[cxx_name = "folderChanged"]
        fn folder_changed(self: Pin<&mut LocalMusic>);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut LocalMusic>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut LocalMusic>);

        /// Папка из FolderDialog (file:// URL или путь); сразу обходится.
        #[qinvokable]
        #[cxx_name = "setFolder"]
        fn set_folder(self: Pin<&mut LocalMusic>, url: &QString);
        #[qinvokable]
        fn rescan(self: Pin<&mut LocalMusic>);
        /// Играть весь список с row (своя очередь плеера файлов).
        #[qinvokable]
        fn play(self: &LocalMusic, row: i32);
    }

    impl cxx_qt::Threading for LocalMusic {}
    impl cxx_qt::Initialize for LocalMusic {}
}

pub struct LocalMusicRust {
    folder: QString,
    tracks: UniquePtr<DictListModel>,
    scanning: bool,
    missing: bool,
    error_text: QString,
    found: Arc<Vec<LocalTrack>>,
    /// номер обхода: результат старого (папку сменили посреди обхода) отбрасывается
    generation: u64,
}

impl Default for LocalMusicRust {
    fn default() -> Self {
        Self {
            folder: QString::default(),
            tracks: DictListModel::create(ModelKind::Track),
            scanning: false,
            missing: false,
            error_text: QString::default(),
            found: Arc::default(),
            generation: 0,
        }
    }
}

/// Строка списка в ключах TrackItem: те же роли, что у списков Spotify.
fn track_item(track: &LocalTrack) -> TrackItem {
    TrackItem {
        title: track.title.clone(),
        artists: track.artist.clone(),
        album: track.album.clone(),
        duration_ms: i64::from(track.duration_ms),
        available: true,
        uri: sf_player::local_files::file_uri(&track.path),
        ..TrackItem::default()
    }
}

impl qobject::LocalMusic {
    fn tracks_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.tracks)
    }

    fn set_folder(mut self: Pin<&mut Self>, url: &QString) {
        let path = local_path(url);
        if path.is_empty() {
            return;
        }
        ctx().services.settings.set(SettingKey::LocalMusicFolder, SettingValue::Str(path.clone()));
        self.as_mut().rust_mut().folder = QString::from(path.as_str());
        self.as_mut().folder_changed();
        self.rescan();
    }

    fn rescan(mut self: Pin<&mut Self>) {
        let folder = self.folder.to_string();
        let mut this = self.as_mut().rust_mut();
        this.generation += 1;
        let generation = this.generation;
        if folder.is_empty() {
            return;
        }
        this.scanning = true;
        self.as_mut().state_changed();
        let thread = self.qt_thread();
        ctx().rt.spawn(async move {
            // обход и чтение тегов блокируют: тысячи файлов на медленном диске идут секунды
            let found = tokio::task::spawn_blocking(move || {
                let root = PathBuf::from(folder);
                root.is_dir().then(|| {
                    let cache = mf_platform::dirs::cache_subdir("local").join("scan.json");
                    mf_local::scan(&root, Some(&cache))
                })
            })
            .await
            .ok()
            .flatten();
            let _ = thread.queue(move |me| me.scanned(generation, found));
        });
    }

    fn scanned(mut self: Pin<&mut Self>, generation: u64, found: Option<Vec<LocalTrack>>) {
        if generation != self.generation {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.scanning = false;
        this.missing = found.is_none();
        let found = found.unwrap_or_default();
        let rows: Vec<TrackItem> = found.iter().map(track_item).collect();
        this.found = Arc::new(found);
        with_model(&mut this.tracks, |m| m.reset_rows(to_rows(&rows)));
        self.as_mut().state_changed();
    }

    fn play(&self, row: i32) {
        let Ok(row) = usize::try_from(row) else { return };
        if row < self.found.len() {
            ctx().services.files.play_queue(self.found.to_vec(), row);
        }
    }

    fn files_changed(mut self: Pin<&mut Self>, snapshot: FileSnapshot) {
        let error = QString::from(snapshot.error.as_str());
        if self.error_text != error {
            self.as_mut().rust_mut().error_text = error;
            self.as_mut().error_changed();
        }
    }
}

impl cxx_qt::Initialize for qobject::LocalMusic {
    fn initialize(mut self: Pin<&mut Self>) {
        let folder = ctx().services.settings.get_str(SettingKey::LocalMusicFolder);
        self.as_mut().rust_mut().folder = QString::from(folder.as_str());
        let files = ctx().services.files.subscribe();
        forward_watch(self.qt_thread(), files, Self::files_changed);
        self.rescan();
    }
}
