// Library: «Любимые треки», плейлисты (строковые id, редактирование своего), альбомы/исполнители/подкасты,
// экран «Библиотека» (items/folderChildren), локальные закрепления. Модели повторяют LibraryEvent;
// Lagged — перечитать всё у сервиса.
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList};
use md3_model::{ListChange, PlayContext, PlaylistId, contexts};
use sf_services::{AuthEvent, HistoryEvent, LibraryEvent, LibraryKind};
use tokio::sync::{Semaphore, broadcast};

use super::{Delivery, forward_events, json_str, model_ptr, string_list, to_qmap, track_from_map, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, Row, apply_list_change, apply_row_change, to_row_change, to_rows};
use crate::qt::{QVariantList, QVariantMap, qvariant_to_json};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(*mut QObject, playlists, READ = playlists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, liked, READ = liked_ptr, CONSTANT)]
        #[qproperty(*mut QObject, albums, READ = albums_ptr, CONSTANT)]
        #[qproperty(*mut QObject, artists, READ = artists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, shows, READ = shows_ptr, CONSTANT)]
        #[qproperty(*mut QObject, episodes, READ = episodes_ptr, CONSTANT)]
        #[qproperty(*mut QObject, recent, READ = recent_ptr, CONSTANT)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        #[qproperty(i32, likes_revision, cxx_name = "likesRevision", READ, NOTIFY = likes_changed)]
        #[qproperty(i32, saved_revision, cxx_name = "savedRevision", READ, NOTIFY = saved_changed)]
        #[qproperty(i32, pins_revision, cxx_name = "pinsRevision", READ, NOTIFY = pins_changed)]
        #[qproperty(i32, pages_loaded, cxx_name = "pagesLoaded", READ, NOTIFY = pages_changed)]
        #[qproperty(i32, pages_total, cxx_name = "pagesTotal", READ, NOTIFY = pages_changed)]
        #[qproperty(i32, history_revision, cxx_name = "historyRevision", READ, NOTIFY = history_changed)]
        type Library = super::LibraryRust;

        fn playlists_ptr(self: &Library) -> *mut QObject;
        fn liked_ptr(self: &Library) -> *mut QObject;
        fn albums_ptr(self: &Library) -> *mut QObject;
        fn artists_ptr(self: &Library) -> *mut QObject;
        fn shows_ptr(self: &Library) -> *mut QObject;
        fn episodes_ptr(self: &Library) -> *mut QObject;
        fn recent_ptr(self: &Library) -> *mut QObject;

        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "likesChanged"]
        fn likes_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "savedChanged"]
        fn saved_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "pinsChanged"]
        fn pins_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "pagesChanged"]
        fn pages_changed(self: Pin<&mut Library>);
        /// локальная история прослушиваний изменилась — listeningStats() перечитать
        #[qsignal]
        #[cxx_name = "historyChanged"]
        fn history_changed(self: Pin<&mut Library>);
        /// дерево/сортировка изменились — модели items()/folderChildren() уже обновлены
        #[qsignal]
        #[cxx_name = "entriesChanged"]
        fn entries_changed(self: Pin<&mut Library>);
        /// шапка плейлиста изменилась — перечитать playlistInfo(id)
        #[qsignal]
        #[cxx_name = "playlistInfoChanged"]
        fn playlist_info_changed(self: Pin<&mut Library>, playlist_id: QString);
        /// Короткое сообщение (итог действия, ошибка с откатом).
        #[qsignal]
        fn notice(self: Pin<&mut Library>, text: QString);

        #[qinvokable]
        fn refresh(self: &Library);

        // --- плейлисты ---
        /// Модель треков плейлиста (кэш по id). Открытие экрана: счётчик владельцев растёт, модель
        /// не пересоздаётся; шапка сверяется со snapshot — изменённый плейлист перечитается.
        #[qinvokable]
        #[cxx_name = "playlistTracks"]
        fn playlist_tracks(self: Pin<&mut Library>, playlist_id: &QString) -> *mut QObject;
        /// Экран закрылся или скрылся: один владелец уходит. Когда владельцев не осталось — идущее
        /// чтение треков отменяется (записи и воспроизведение не трогаются).
        #[qinvokable]
        #[cxx_name = "releasePlaylist"]
        fn release_playlist(self: Pin<&mut Library>, playlist_id: &QString);
        /// Шапка: ключи PlaylistItem + totalDurationMs, followed; {} — неизвестно.
        #[qinvokable]
        #[cxx_name = "playlistInfo"]
        fn playlist_info(self: &Library, playlist_id: &QString) -> QMap_QString_QVariant;
        /// статистика прослушиваний этого ПК за `days` последних дней (ListeningStats)
        #[qinvokable]
        #[cxx_name = "listeningStats"]
        fn listening_stats(self: &Library, days: i32) -> QMap_QString_QVariant;
        #[qinvokable]
        #[cxx_name = "canEdit"]
        fn can_edit(self: &Library, playlist_id: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "addToPlaylist"]
        fn add_to_playlist(self: &Library, track: &QMap_QString_QVariant, playlist_id: &QString);
        #[qinvokable]
        #[cxx_name = "addTracks"]
        fn add_tracks(self: &Library, playlist_id: &QString, uris: &QStringList);
        /// Вставить на позицию («Вернуть» после удаления строк).
        #[qinvokable]
        #[cxx_name = "insertTracks"]
        fn insert_tracks(self: &Library, playlist_id: &QString, uris: &QStringList, position: i32);
        /// rows: [{uri, pos}]
        #[qinvokable]
        #[cxx_name = "removeTracks"]
        fn remove_tracks(self: &Library, playlist_id: &QString, rows: &QList_QVariant);
        #[qinvokable]
        #[cxx_name = "moveTracks"]
        fn move_tracks(self: &Library, playlist_id: &QString, from: i32, to: i32);
        #[qinvokable]
        #[cxx_name = "createPlaylist"]
        fn create_playlist(self: &Library, title: &QString);
        #[qinvokable]
        #[cxx_name = "createPlaylist"]
        fn create_playlist_with(self: &Library, title: &QString, track: &QMap_QString_QVariant);
        #[qinvokable]
        #[cxx_name = "createPlaylistFrom"]
        fn create_playlist_from(self: &Library, title: &QString, uris: &QStringList);
        #[qinvokable]
        #[cxx_name = "renamePlaylist"]
        fn rename_playlist(self: &Library, playlist_id: &QString, title: &QString);
        #[qinvokable]
        #[cxx_name = "setDescription"]
        fn set_description(self: &Library, playlist_id: &QString, text: &QString);
        #[qinvokable]
        #[cxx_name = "setPublic"]
        fn set_public(self: &Library, playlist_id: &QString, value: bool);
        /// Путь или file:// URL JPEG/PNG (FileDialog).
        #[qinvokable]
        #[cxx_name = "uploadCover"]
        fn upload_cover(self: &Library, playlist_id: &QString, source: &QString);
        #[qinvokable]
        #[cxx_name = "followPlaylist"]
        fn follow_playlist(self: &Library, playlist_id: &QString);
        #[qinvokable]
        #[cxx_name = "unfollowPlaylist"]
        fn unfollow_playlist(self: &Library, playlist_id: &QString);

        // --- лайки, сохранённое ---
        #[qinvokable]
        #[cxx_name = "isLiked"]
        fn is_liked(self: &Library, track_id: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "toggleLike"]
        fn toggle_like(self: &Library, track: &QMap_QString_QVariant);
        /// Локальный «не рекомендовать» + пропуск, если играет.
        #[qinvokable]
        fn dislike(self: &Library, track: &QMap_QString_QVariant);
        /// Альбом/шоу/выпуск сохранён, исполнитель — подписка.
        #[qinvokable]
        #[cxx_name = "isSaved"]
        fn is_saved(self: &Library, uri: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "toggleSaved"]
        fn toggle_saved(self: &Library, uri: &QString);

        // --- экран «Библиотека» ---
        /// kind: playlists|albums|artists|shows; owner: all|mine|followed|collab; sort — как Settings.librarySort.
        #[qinvokable]
        fn items(self: Pin<&mut Library>, kind: &QString, owner: &QString, sort: &QString) -> *mut QObject;
        /// Последние добавленные/обновлённые строки всех разделов (для боковой панели), limit — сколько.
        #[qinvokable]
        #[cxx_name = "recentEntries"]
        fn recent_entries(self: Pin<&mut Library>, limit: i32) -> *mut QObject;
        #[qinvokable]
        #[cxx_name = "folderChildren"]
        fn folder_children(self: Pin<&mut Library>, folder_id: &QString) -> *mut QObject;
        #[qinvokable]
        #[cxx_name = "isPinned"]
        fn is_pinned(self: &Library, uri: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "togglePin"]
        fn toggle_pin(self: &Library, uri: &QString);
    }

    impl cxx_qt::Threading for Library {}
    impl cxx_qt::Initialize for Library {}
}

pub struct LibraryRust {
    playlists: UniquePtr<DictListModel>,
    liked: UniquePtr<DictListModel>,
    albums: UniquePtr<DictListModel>,
    artists: UniquePtr<DictListModel>,
    shows: UniquePtr<DictListModel>,
    episodes: UniquePtr<DictListModel>,
    /// недавно прослушанные треки (HistoryService::recent) — источник «Недавние» в AddTracksPanel
    recent: UniquePtr<DictListModel>,
    playlist_models: HashMap<PlaylistId, UniquePtr<DictListModel>>,
    /// сколько экранов открыли плейлист: последний release отменяет идущее чтение (сервис решает сам)
    playlist_refs: HashMap<PlaylistId, u32>,
    /// items(kind, owner, sort) → модель; ключ "kind|owner|sort"
    entry_models: HashMap<String, UniquePtr<DictListModel>>,
    folder_models: HashMap<String, UniquePtr<DictListModel>>,
    error_text: QString,
    likes_revision: i32,
    saved_revision: i32,
    pins_revision: i32,
    pages_loaded: i32,
    pages_total: i32,
    history_revision: i32,
}

impl Default for LibraryRust {
    fn default() -> Self {
        let mut liked = DictListModel::create(ModelKind::Track);
        with_model(&mut liked, |m| m.set_context(PlayContext::new(contexts::LIKED)));
        let mut episodes = DictListModel::create(ModelKind::Episode);
        with_model(&mut episodes, |m| m.set_context(PlayContext::new(contexts::EPISODE)));
        let mut recent = DictListModel::create(ModelKind::Track);
        with_model(&mut recent, |m| m.set_context(PlayContext::new(contexts::TRACKS)));
        Self {
            playlists: DictListModel::create(ModelKind::Playlist),
            liked,
            albums: DictListModel::create(ModelKind::Album),
            artists: DictListModel::create(ModelKind::Artist),
            shows: DictListModel::create(ModelKind::Show),
            episodes,
            recent,
            playlist_models: HashMap::new(),
            playlist_refs: HashMap::new(),
            entry_models: HashMap::new(),
            folder_models: HashMap::new(),
            error_text: QString::default(),
            likes_revision: 0,
            saved_revision: 0,
            pins_revision: 0,
            pages_loaded: 0,
            pages_total: 0,
            history_revision: 0,
        }
    }
}

/// Контекст модели плейлиста; название и обложка — для карточки «Играет из …», если шапка уже известна.
fn playlist_context(id: &PlaylistId) -> PlayContext {
    let context = PlayContext::with_uri(contexts::PLAYLIST, id.uri());
    match ctx().services.library.playlist_info(id) {
        Some(info) => context.titled(info.item.title).with_cover(info.item.cover),
        None => context,
    }
}

/// «Любимые треки» вошедшего аккаунта — контекст spotify:user:<id>:collection.
fn liked_context() -> PlayContext {
    match ctx().services.auth.account_id() {
        Some(id) => PlayContext::with_uri(contexts::LIKED, contexts::liked_uri(id.as_str())).titled(mf_i18n::t("Liked Songs")),
        None => PlayContext::new(contexts::LIKED),
    }
}

/// Строки экрана «Библиотека» сверяются по uri (у папок — spotify:folder:<id>): прокрутка и делегаты сохраняются.
const ENTRY_KEY: &str = "uri";

fn pid(id: &QString) -> PlaylistId {
    PlaylistId::from_uri_or_id(&id.to_string())
}

/// Событие библиотеки, уже подготовленное для модели: строки плейлистов сериализованы вне GUI-потока.
/// permit — квитанция обратного давления: вернётся семафору после применения в GUI.
enum LibraryDelivery {
    /// пропущены события — мост перечитывает состояние сервиса целиком
    Lagged,
    Ready(LibraryEvent),
    Tracks { id: PlaylistId, change: ListChange<Row>, permit: tokio::sync::OwnedSemaphorePermit },
}

/// Сколько подготовленных задач с треками плейлиста может ждать применения в GUI. Квитанции (permits)
/// возвращаются после применения: GUI не догоняет — поставщик приостанавливается вместо безграничной очереди.
const TRACK_QUEUE: usize = 16;

/// События библиотеки → GUI. Строки плейлистов (serde) готовятся в tokio-задаче; скалярные события
/// проходят без ожидания, чтобы остальное не тормозилось. Порядок событий сохраняется.
fn forward_library(thread: CxxQtThread<qobject::Library>, mut events: broadcast::Receiver<LibraryEvent>) {
    let gui = Arc::new(Semaphore::new(TRACK_QUEUE));
    ctx().rt.spawn(async move {
        loop {
            let delivery = match events.recv().await {
                Ok(event) => match event {
                    LibraryEvent::PlaylistTracks { id, change } => {
                        // сериализация строк — до очереди GUI; Semaphore::close не зовётся, так что
                        // permit есть всегда
                        let permit = Arc::clone(&gui).acquire_owned().await.expect("semaphore is open");
                        LibraryDelivery::Tracks { id, change: to_row_change(&change), permit }
                    }
                    other => LibraryDelivery::Ready(other),
                },
                Err(broadcast::error::RecvError::Lagged(_)) => LibraryDelivery::Lagged,
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if thread
                .queue(move |object| {
                    object.on_event(delivery);
                    // permit освободится после применения — это и есть квитанция GUI
                })
                .is_err()
            {
                break;
            }
        }
    });
}

impl qobject::Library {
    fn playlists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.playlists)
    }

    fn liked_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.liked)
    }

    fn recent_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.recent)
    }

    fn albums_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.albums)
    }

    fn artists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.artists)
    }

    fn shows_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.shows)
    }

    fn episodes_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.episodes)
    }

    fn refresh(&self) {
        ctx().services.library.refresh();
    }

    // --- плейлисты ---

    fn playlist_tracks(mut self: Pin<&mut Self>, playlist_id: &QString) -> *mut cxx_qt::QObject {
        let id = pid(playlist_id);
        let library = &ctx().services.library;
        let this = self.as_mut().rust_mut().get_mut();
        *this.playlist_refs.entry(id.clone()).or_default() += 1;
        // стабильный экземпляр модели: не пересоздаём и не чистим — изменения приходят событиями
        let model = this.playlist_models.entry(id.clone()).or_insert_with(|| {
            let mut model = DictListModel::create(ModelKind::Track);
            with_model(&mut model, |m| m.set_context(playlist_context(&id)));
            model
        });
        let ptr = model_ptr(model);
        // модель пуста, а у сервиса есть готовые строки (мост создан позже чтения или события пропущены):
        // перезалить кусками вместо ожидания всей перечитки
        let hydrated = model.as_ref().is_some_and(|m| !m.rows().is_empty());
        if !hydrated {
            library.replay_playlist(&id);
        }
        library.load_playlist(id);
        ptr
    }

    fn release_playlist(mut self: Pin<&mut Self>, playlist_id: &QString) {
        let id = pid(playlist_id);
        let this = self.as_mut().rust_mut().get_mut();
        let Some(count) = this.playlist_refs.get_mut(&id) else { return };
        *count = count.saturating_sub(1);
        if *count == 0 {
            this.playlist_refs.remove(&id);
            ctx().services.library.cancel_playlist(&id);
        }
    }

    fn listening_stats(&self, days: i32) -> QVariantMap {
        to_qmap(&ctx().services.history.listening_stats(days.clamp(1, 366) as usize))
    }

    fn playlist_info(&self, playlist_id: &QString) -> QVariantMap {
        let Some(info) = ctx().services.library.playlist_info(&pid(playlist_id)) else { return QVariantMap::default() };
        let mut map = match serde_json::to_value(&info.item) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        map.insert("totalDurationMs".into(), info.total_duration_ms.into());
        map.insert("followed".into(), info.followed.into());
        crate::qt::json_to_qvariantmap(&map)
    }

    fn can_edit(&self, playlist_id: &QString) -> bool {
        ctx().services.library.can_edit(&pid(playlist_id))
    }

    fn add_to_playlist(&self, track: &QVariantMap, playlist_id: &QString) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.add_to_playlist(track, pid(playlist_id));
        }
    }

    fn add_tracks(&self, playlist_id: &QString, uris: &QStringList) {
        ctx().services.library.add_tracks(pid(playlist_id), string_list(uris));
    }

    fn insert_tracks(&self, playlist_id: &QString, uris: &QStringList, position: i32) {
        let position = usize::try_from(position).unwrap_or(0);
        ctx().services.library.insert_tracks(pid(playlist_id), string_list(uris), position);
    }

    fn remove_tracks(&self, playlist_id: &QString, rows: &QVariantList) {
        let rows = rows
            .iter()
            .filter_map(|row| {
                let row = qvariant_to_json(row);
                let row = row.as_object()?;
                let pos = row.get("pos").and_then(|p| p.as_f64())? as usize;
                Some((json_str(row, "uri"), pos))
            })
            .collect();
        ctx().services.library.remove_tracks(pid(playlist_id), rows);
    }

    fn move_tracks(&self, playlist_id: &QString, from: i32, to: i32) {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) {
            ctx().services.library.move_tracks(pid(playlist_id), from, to);
        }
    }

    fn create_playlist(&self, title: &QString) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist(title, None);
        }
    }

    fn create_playlist_with(&self, title: &QString, track: &QVariantMap) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist(title, track_from_map(track));
        }
    }

    fn create_playlist_from(&self, title: &QString, uris: &QStringList) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist_from(title, string_list(uris));
        }
    }

    fn rename_playlist(&self, playlist_id: &QString, title: &QString) {
        ctx().services.library.rename_playlist(pid(playlist_id), title.to_string());
    }

    fn set_description(&self, playlist_id: &QString, text: &QString) {
        ctx().services.library.set_description(pid(playlist_id), text.to_string());
    }

    fn set_public(&self, playlist_id: &QString, value: bool) {
        ctx().services.library.set_public(pid(playlist_id), value);
    }

    fn upload_cover(&self, playlist_id: &QString, source: &QString) {
        let path = super::local_path(source);
        ctx().services.library.upload_cover(pid(playlist_id), path.into());
    }

    fn follow_playlist(&self, playlist_id: &QString) {
        ctx().services.library.follow_playlist(pid(playlist_id));
    }

    fn unfollow_playlist(&self, playlist_id: &QString) {
        ctx().services.library.unfollow_playlist(pid(playlist_id));
    }

    // --- лайки, сохранённое ---

    fn is_liked(&self, track_id: &QString) -> bool {
        ctx().services.library.is_liked(&track_id.to_string())
    }

    fn toggle_like(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.toggle_like(track);
        }
    }

    fn dislike(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.dislike(track);
        }
    }

    fn is_saved(&self, uri: &QString) -> bool {
        ctx().services.library.is_saved(&uri.to_string())
    }

    fn toggle_saved(&self, uri: &QString) {
        ctx().services.library.toggle_saved(uri.to_string());
    }

    // --- экран «Библиотека» ---

    fn items(mut self: Pin<&mut Self>, kind: &QString, owner: &QString, sort: &QString) -> *mut cxx_qt::QObject {
        let (kind, owner, sort) = (kind.to_string(), owner.to_string(), sort.to_string());
        let key = format!("{kind}|{owner}|{sort}");
        let rows = to_rows(&ctx().services.library.entries(LibraryKind::from_name(&kind), &owner, &sort));
        let this = self.as_mut().rust_mut().get_mut();
        let model = this.entry_models.entry(key).or_insert_with(|| DictListModel::create(ModelKind::Library));
        with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        model_ptr(model)
    }

    fn recent_entries(mut self: Pin<&mut Self>, limit: i32) -> *mut cxx_qt::QObject {
        let limit = usize::try_from(limit.max(1)).unwrap_or(20).min(200);
        let key = format!("recent|{limit}");
        let library = &ctx().services.library;
        let rows = to_rows(&library.recent_entries(limit));
        let loading = library.playlists_loading();
        let this = self.as_mut().rust_mut().get_mut();
        let model = this.entry_models.entry(key).or_insert_with(|| DictListModel::create(ModelKind::Library));
        with_model(model, |mut m| {
            m.as_mut().sync_rows(rows, ENTRY_KEY);
            m.set_loading(loading);
        });
        model_ptr(model)
    }

    fn folder_children(mut self: Pin<&mut Self>, folder_id: &QString) -> *mut cxx_qt::QObject {
        let id = folder_id.to_string();
        let rows = to_rows(&ctx().services.library.folder_children(&id));
        let this = self.as_mut().rust_mut().get_mut();
        let model = this.folder_models.entry(id).or_insert_with(|| DictListModel::create(ModelKind::Library));
        with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        model_ptr(model)
    }

    fn is_pinned(&self, uri: &QString) -> bool {
        ctx().services.library.is_pinned(&uri.to_string())
    }

    fn toggle_pin(&self, uri: &QString) {
        ctx().services.library.toggle_pin(uri.to_string());
    }

    // --- синхронизация с сервисом ---

    fn sync_error(mut self: Pin<&mut Self>) {
        let text = QString::from(ctx().services.library.error_text().as_str());
        if self.error_text != text {
            self.as_mut().rust_mut().error_text = text;
            self.error_changed();
        }
    }

    fn sync_revisions(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        let likes = library.likes_revision() as i32;
        if self.likes_revision != likes {
            self.as_mut().rust_mut().likes_revision = likes;
            self.as_mut().likes_changed();
        }
        let saved = library.saved_revision() as i32;
        if self.saved_revision != saved {
            self.as_mut().rust_mut().saved_revision = saved;
            self.as_mut().saved_changed();
        }
        let pins = library.pins_revision() as i32;
        if self.pins_revision != pins {
            self.as_mut().rust_mut().pins_revision = pins;
            self.as_mut().pins_changed();
        }
        let (loaded, total) = library.loading_pages();
        if (self.pages_loaded, self.pages_total) != (loaded as i32, total as i32) {
            let mut this = self.as_mut().rust_mut();
            this.pages_loaded = loaded as i32;
            this.pages_total = total as i32;
            self.as_mut().pages_changed();
        }
    }

    /// Перезаполнить модели items()/folderChildren() по их ключам.
    fn sync_entries(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        let this = self.as_mut().rust_mut().get_mut();
        for (key, model) in this.entry_models.iter_mut() {
            // "recent|<limit>" — сводка по всем разделам для боковой панели
            if let Some(limit) = key.strip_prefix("recent|") {
                let limit = limit.parse::<usize>().unwrap_or(20);
                let rows = to_rows(&library.recent_entries(limit));
                let loading = library.playlists_loading();
                with_model(model, |mut m| {
                    m.as_mut().sync_rows(rows, ENTRY_KEY);
                    m.set_loading(loading);
                });
                continue;
            }
            let mut parts = key.splitn(3, '|');
            let (kind, owner, sort) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let rows = to_rows(&library.entries(LibraryKind::from_name(kind), owner, sort));
            with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        }
        for (id, model) in this.folder_models.iter_mut() {
            let rows = to_rows(&library.folder_children(id));
            with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        }
        self.entries_changed();
    }

    /// Всё содержимое заново (старт и пропуск событий).
    fn resync(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        {
            let this = self.as_mut().rust_mut().get_mut();
            with_model(&mut this.playlists, |mut m| {
                m.as_mut().reset_rows(to_rows(&library.playlists()));
                m.set_loading(library.playlists_loading());
            });
            with_model(&mut this.liked, |mut m| {
                m.as_mut().reset_rows(to_rows(&library.liked()));
                m.set_loading(library.liked_loading());
            });
            with_model(&mut this.albums, |m| m.reset_rows(to_rows(&library.albums())));
            with_model(&mut this.artists, |m| m.reset_rows(to_rows(&library.artists())));
            with_model(&mut this.shows, |m| m.reset_rows(to_rows(&library.shows())));
            with_model(&mut this.episodes, |m| m.reset_rows(to_rows(&library.episodes())));
        }
        let ids: Vec<PlaylistId> = self.playlist_models.keys().cloned().collect();
        // Перезаливка кэша всем моделям, не только пустым: пропуск событий (Lagged) мог оставить
        // в непустой модели лишнюю или пропавшую строку, а load_playlist для complete-страницы
        // вернёт Idle и её не перечитает. Первое событие перезаливки — Reset: модель заменяется,
        // а не дополняется, так что повтор поверх актуальных строк безопасен.
        for id in &ids {
            library.replay_playlist(id);
        }
        for id in ids {
            library.load_playlist(id);
        }
        self.as_mut().sync_entries();
        self.as_mut().sync_error();
        self.sync_revisions();
    }

    /// Недавние треки — из истории (дубли по uri убраны, 50 последних).
    fn sync_recent(mut self: Pin<&mut Self>) {
        let mut seen = std::collections::HashSet::new();
        let tracks: Vec<_> = ctx()
            .services
            .history
            .recent(200)
            .into_iter()
            .map(|record| record.track)
            .filter(|track| seen.insert(track.play_uri()))
            .take(50)
            .collect();
        let this = self.as_mut().rust_mut().get_mut();
        with_model(&mut this.recent, |m| m.reset_rows(to_rows(&tracks)));
    }

    fn on_auth(mut self: Pin<&mut Self>, _delivery: Delivery<AuthEvent>) {
        let context = liked_context();
        with_model(&mut self.as_mut().rust_mut().get_mut().liked, |m| m.set_context(context));
    }

    fn on_history(mut self: Pin<&mut Self>, _delivery: Delivery<HistoryEvent>) {
        self.as_mut().sync_recent();
        let revision = self.history_revision.wrapping_add(1);
        self.as_mut().rust_mut().get_mut().history_revision = revision;
        self.history_changed();
    }

    fn on_event(mut self: Pin<&mut Self>, delivery: LibraryDelivery) {
        let event = match delivery {
            LibraryDelivery::Lagged => return self.resync(),
            LibraryDelivery::Ready(event) => event,
            // строки уже сериализованы вне GUI — только применить
            LibraryDelivery::Tracks { id, change, permit: _permit } => {
                let this = self.as_mut().rust_mut().get_mut();
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| apply_row_change(m, &change));
                }
                return;
            }
        };
        let this = self.as_mut().rust_mut().get_mut();
        match event {
            LibraryEvent::Playlists(change) => with_model(&mut this.playlists, |m| apply_list_change(m, &change)),
            LibraryEvent::Liked(change) => with_model(&mut this.liked, |m| apply_list_change(m, &change)),
            LibraryEvent::Albums(change) => with_model(&mut this.albums, |m| apply_list_change(m, &change)),
            LibraryEvent::Artists(change) => with_model(&mut this.artists, |m| apply_list_change(m, &change)),
            LibraryEvent::Shows(change) => with_model(&mut this.shows, |m| apply_list_change(m, &change)),
            LibraryEvent::Episodes(change) => with_model(&mut this.episodes, |m| apply_list_change(m, &change)),
            LibraryEvent::PlaylistTracks { id, change } => {
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| apply_list_change(m, &change));
                }
            }
            LibraryEvent::PlaylistInfo(id) => {
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| m.set_context(playlist_context(&id)));
                }
                self.playlist_info_changed(QString::from(id.as_str()))
            }
            LibraryEvent::EntriesChanged => {
                // догрузка страниц раздела («3 из 5») меняет и счётчик страниц
                self.as_mut().sync_entries();
                self.sync_revisions()
            }
            LibraryEvent::LikesChanged | LibraryEvent::SavedChanged | LibraryEvent::PinsChanged => {
                self.sync_revisions()
            }
            LibraryEvent::ErrorChanged => self.sync_error(),
            LibraryEvent::Notice(text) => self.notice(QString::from(text.as_str())),
            LibraryEvent::Feedback { .. } => {}
        }
    }
}

impl cxx_qt::Initialize for qobject::Library {
    fn initialize(mut self: Pin<&mut Self>) {
        let events = ctx().services.library.subscribe();
        let history = ctx().services.history.subscribe();
        let auth = ctx().services.auth.subscribe();
        self.as_mut().resync();
        self.as_mut().sync_recent();
        self.as_mut().on_auth(Delivery::Lagged);
        forward_library(self.qt_thread(), events);
        forward_events(self.qt_thread(), history, Self::on_history);
        forward_events(self.qt_thread(), auth, Self::on_auth);
    }
}
