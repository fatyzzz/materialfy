// Player — зеркало sf_player::PlayerHandle (локальный librespot или устройство Connect). Свойства сверяются со снимком
// на каждом событии; структурные события (трек, состояние, очередь, устройство) проходят полный sync(), а «дешёвые»
// (уровень ~30 Гц, позиция ~5 Гц, громкость, режимы, обложка, ошибка) обновляют только свою группу свойств.
// Сигнал — только по изменившейся группе; TrackChanged — всегда. Модель очереди приводится
// к очереди снимка по uid изменениями по месту (вставка/удаление/перестановка), поэтому порядок и пропуски событий
// QueueChange ей не страшны. Сеттеры (volume, shuffle, repeat, levelsWanted) меняют значение сразу; «эхо» из плеера,
// которое старше записи, какое-то время не принимается. Громкость в плеер передаёт только этот мост, он же пишет
// Settings.volume (с задержкой: при перетаскивании ползунка — одна запись в конце).
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cxx::UniquePtr;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList, QVariant};
use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};
use sf_player::{LocalPlayback, PlayerEvent, PlayerSnapshot, Repeat};
use mf_platform::{SettingKey, SettingValue};

use super::{Delivery, forward_events, held, model_ptr, string_list, to_qmap, track_from_map, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_row, to_rows};
use crate::qt::{QVariantMap, json_to_qvariantmap};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(bool, has_track, cxx_name = "hasTrack", READ, NOTIFY = track_changed)]
        #[qproperty(QString, track_id, cxx_name = "trackId", READ, NOTIFY = track_changed)]
        #[qproperty(QString, title, READ, NOTIFY = track_changed)]
        #[qproperty(QString, artist, READ, NOTIFY = track_changed)]
        #[qproperty(QString, album, READ, NOTIFY = track_changed)]
        #[qproperty(QString, cover, READ, NOTIFY = track_changed)]
        #[qproperty(QMap_QString_QVariant, track, READ, NOTIFY = track_changed)]
        #[qproperty(QString, codec, READ, NOTIFY = track_changed)]
        #[qproperty(i32, direction, READ, NOTIFY = track_changed)]
        #[qproperty(QString, cover_file, cxx_name = "coverFile", READ, NOTIFY = cover_file_changed)]
        #[qproperty(bool, playing, READ, NOTIFY = state_changed)]
        #[qproperty(bool, buffering, READ, NOTIFY = state_changed)]
        /// the DJ is saying a line between tracks (local playback)
        #[qproperty(bool, narrating, READ, NOTIFY = state_changed)]
        #[qproperty(i32, position_ms, cxx_name = "positionMs", READ, NOTIFY = position_changed)]
        #[qproperty(i32, duration_ms, cxx_name = "durationMs", READ, NOTIFY = position_changed)]
        #[qproperty(f64, position, READ, NOTIFY = position_changed)]
        #[qproperty(f64, volume, READ, WRITE = set_volume, NOTIFY = volume_changed)]
        #[qproperty(bool, shuffle, READ, WRITE = set_shuffle, NOTIFY = mode_changed)]
        /// Smart Shuffle: перемешивание с рекомендациями Spotify (shuffle при этом тоже true)
        #[qproperty(bool, smart_shuffle, cxx_name = "smartShuffle", READ, WRITE = set_smart_shuffle, NOTIFY = mode_changed)]
        #[qproperty(bool, smart_shuffle_available, cxx_name = "smartShuffleAvailable", READ, NOTIFY = mode_changed)]
        #[qproperty(QString, repeat, READ, WRITE = set_repeat, NOTIFY = mode_changed)]
        #[qproperty(*mut QObject, queue, READ = queue_ptr, CONSTANT)]
        #[qproperty(i32, current_index, cxx_name = "currentIndex", READ, NOTIFY = queue_changed)]
        #[qproperty(QString, source, READ, NOTIFY = queue_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        #[qproperty(f64, level, READ, NOTIFY = level_changed)]
        #[qproperty(bool, levels_wanted, cxx_name = "levelsWanted", READ, WRITE = set_levels_wanted, NOTIFY = level_changed)]
        #[qproperty(QString, cover_large, cxx_name = "coverLarge", READ, NOTIFY = track_changed)]
        #[qproperty(QString, uri, READ, NOTIFY = track_changed)]
        #[qproperty(bool, is_episode, cxx_name = "isEpisode", READ, NOTIFY = track_changed)]
        #[qproperty(bool, is_local, cxx_name = "isLocal", READ, NOTIFY = device_changed)]
        #[qproperty(bool, remote, READ, NOTIFY = device_changed)]
        #[qproperty(QMap_QString_QVariant, device, READ, NOTIFY = device_changed)]
        #[qproperty(QString, local_state, cxx_name = "localState", READ, NOTIFY = device_changed)]
        #[qproperty(bool, can_control, cxx_name = "canControl", READ, NOTIFY = device_changed)]
        #[qproperty(bool, can_set_volume, cxx_name = "canSetVolume", READ, NOTIFY = device_changed)]
        #[qproperty(bool, transferring, READ, NOTIFY = device_changed)]
        #[qproperty(bool, device_lost, cxx_name = "deviceLost", READ, NOTIFY = device_changed)]
        #[qproperty(QMap_QString_QVariant, queue_context, cxx_name = "queueContext", READ, NOTIFY = queue_changed)]
        #[qproperty(QString, context_title, cxx_name = "contextTitle", READ, NOTIFY = queue_changed)]
        #[qproperty(QString, context_uri, cxx_name = "contextUri", READ, NOTIFY = queue_changed)]
        #[qproperty(i32, context_remaining, cxx_name = "contextRemaining", READ, NOTIFY = queue_changed)]
        #[qproperty(i32, manual_count, cxx_name = "manualCount", READ, NOTIFY = queue_changed)]
        #[qproperty(bool, can_clear_queue, cxx_name = "canClearQueue", READ, NOTIFY = queue_changed)]
        #[qproperty(bool, queue_reorderable, cxx_name = "queueReorderable", READ, NOTIFY = queue_changed)]
        type Player = super::PlayerRust;

        fn queue_ptr(self: &Player) -> *mut QObject;
        fn set_volume(self: Pin<&mut Player>, value: f64);
        fn set_shuffle(self: Pin<&mut Player>, value: bool);
        fn set_smart_shuffle(self: Pin<&mut Player>, value: bool);
        fn set_repeat(self: Pin<&mut Player>, value: QString);
        fn set_levels_wanted(self: Pin<&mut Player>, value: bool);

        #[qsignal]
        #[cxx_name = "trackChanged"]
        fn track_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "positionChanged"]
        fn position_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "volumeChanged"]
        fn volume_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "modeChanged"]
        fn mode_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "queueChanged"]
        fn queue_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "coverFileChanged"]
        fn cover_file_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "levelChanged"]
        fn level_changed(self: Pin<&mut Player>);
        #[qsignal]
        #[cxx_name = "deviceChanged"]
        fn device_changed(self: Pin<&mut Player>);
        /// короткое сообщение («Очередь очищена», «Pixel 9 не отвечает»)
        #[qsignal]
        fn notice(self: Pin<&mut Player>, text: QString);

        /// Поставить в очередь всю модель (DictListModel или TrackFilter) и начать с row.
        #[qinvokable]
        #[cxx_name = "playFrom"]
        unsafe fn play_from(self: Pin<&mut Player>, model: *mut QObject, row: i32);
        /// «Перемешать»: включить shuffle и начать со случайного трека модели.
        #[qinvokable]
        #[cxx_name = "shuffleFrom"]
        unsafe fn shuffle_from(self: Pin<&mut Player>, model: *mut QObject);
        #[qinvokable]
        #[cxx_name = "playIndex"]
        fn play_index(self: &Player, index: i32);
        #[qinvokable]
        #[cxx_name = "togglePlay"]
        fn toggle_play(self: &Player);
        #[qinvokable]
        fn play(self: &Player);
        #[qinvokable]
        fn pause(self: &Player);
        #[qinvokable]
        fn stop(self: &Player);
        #[qinvokable]
        fn next(self: &Player);
        #[qinvokable]
        fn previous(self: &Player);
        #[qinvokable]
        fn seek(self: Pin<&mut Player>, fraction: f64);
        #[qinvokable]
        #[cxx_name = "seekMs"]
        fn seek_ms(self: Pin<&mut Player>, ms: i32);
        #[qinvokable]
        #[cxx_name = "cycleRepeat"]
        fn cycle_repeat(self: Pin<&mut Player>);
        /// Кнопка перемешивания: выкл → перемешивание → Smart Shuffle (где доступен) → выкл.
        #[qinvokable]
        #[cxx_name = "cycleShuffle"]
        fn cycle_shuffle(self: Pin<&mut Player>);
        /// ±мс от текущей позиции (выпуски: −10000 / +30000).
        #[qinvokable]
        #[cxx_name = "seekRelative"]
        fn seek_relative(self: Pin<&mut Player>, delta_ms: i32);

        /// Клик по строке очереди по её uid (строки выше снимаются, контекст продолжается).
        #[qinvokable]
        #[cxx_name = "playQueueRow"]
        fn play_queue_row(self: &Player, uid: f64);
        /// Контекст по URI (плейлист/альбом/исполнитель/шоу/станция) с трека offsetUri ("" — с начала).
        #[qinvokable]
        #[cxx_name = "playContext"]
        fn play_context(self: &Player, uri: &QString, offset_uri: &QString);
        /// Контекст по URI вперемешку («Перемешать» в меню плитки).
        #[qinvokable]
        #[cxx_name = "shuffleContext"]
        fn shuffle_context(self: &Player, uri: &QString);
        /// Список URI с индекса (радио, история).
        #[qinvokable]
        #[cxx_name = "playUris"]
        fn play_uris(self: &Player, uris: &QStringList, index: i32);
        /// Выпуск с позиции (resumeMs).
        #[qinvokable]
        #[cxx_name = "playEpisode"]
        fn play_episode(self: &Player, uri: &QString, position_ms: i32);
        /// «В очередь» (словарь трека/выпуска).
        #[qinvokable]
        #[cxx_name = "addToQueue"]
        fn add_to_queue(self: &Player, track: &QMap_QString_QVariant);
        /// «В очередь» по URI (альбом раскрывается целиком).
        #[qinvokable]
        #[cxx_name = "addUriToQueue"]
        fn add_uri_to_queue(self: &Player, uri: &QString);
        /// Запомнить строку до запуска списка по URI: экран переключается на неё сразу, без круга в Web API.
        #[qinvokable]
        #[cxx_name = "rememberTrack"]
        fn remember_track(self: &Player, track: &QMap_QString_QVariant);
        /// Вставка в «Далее в очереди» (только isLocal; иначе — в конец).
        #[qinvokable]
        #[cxx_name = "insertManual"]
        fn insert_manual(self: &Player, track: &QMap_QString_QVariant, at: i32);
        /// Перестановка внутри «Далее в очереди» (индексы в секции manual).
        #[qinvokable]
        #[cxx_name = "moveManual"]
        fn move_manual(self: &Player, from: i32, to: i32);
        #[qinvokable]
        #[cxx_name = "clearManualQueue"]
        fn clear_manual_queue(self: &Player);
        /// «Слушать здесь».
        #[qinvokable]
        #[cxx_name = "transferHere"]
        fn transfer_here(self: &Player);
        /// Окно приложения активно (частота опроса удалённого плеера).
        #[qinvokable]
        #[cxx_name = "setAppActive"]
        fn set_app_active(self: &Player, active: bool);
    }

    impl cxx_qt::Threading for Player {}
    impl cxx_qt::Initialize for Player {}
}

pub struct PlayerRust {
    has_track: bool,
    track_id: QString,
    title: QString,
    artist: QString,
    album: QString,
    cover: QString,
    track: QVariantMap,
    codec: QString,
    direction: i32,
    cover_file: QString,
    playing: bool,
    buffering: bool,
    narrating: bool,
    position_ms: i32,
    duration_ms: i32,
    position: f64,
    volume: f64,
    shuffle: bool,
    smart_shuffle: bool,
    smart_shuffle_available: bool,
    repeat: QString,
    queue: UniquePtr<DictListModel>,
    current_index: i32,
    source: QString,
    error_text: QString,
    level: f64,
    levels_wanted: bool,
    cover_large: QString,
    uri: QString,
    is_episode: bool,
    is_local: bool,
    remote: bool,
    device: QVariantMap,
    local_state: QString,
    can_control: bool,
    can_set_volume: bool,
    transferring: bool,
    device_lost: bool,
    queue_context: QVariantMap,
    context_title: QString,
    context_uri: QString,
    context_remaining: i32,
    manual_count: i32,
    can_clear_queue: bool,
    queue_reorderable: bool,

    /// трек снимка, по которому заполнены свойства группы trackChanged
    track_item: Option<TrackItem>,
    repeat_mode: Repeat,
    volume_set_at: Option<Instant>,
    mode_set_at: Option<Instant>,
    position_set_at: Option<Instant>,
    /// поколение отложенной записи Settings.volume
    volume_save: u64,
    /// очередь снимка, к которой приведена модель
    queue_rows: Arc<Vec<QueueRow>>,
    /// типизированное значение устройства из снимка: чтобы не перестраивать QMap на каждую сверку
    device_item: Option<DeviceItem>,
    /// эхо отклонено удержанием: по истечении ECHO_HOLD перепроверить громкость и режимы
    stale: Option<()>,
}

impl Default for PlayerRust {
    fn default() -> Self {
        Self {
            has_track: false,
            track_id: QString::default(),
            title: QString::default(),
            artist: QString::default(),
            album: QString::default(),
            cover: QString::default(),
            track: QVariantMap::default(),
            codec: QString::default(),
            direction: 1,
            cover_file: QString::default(),
            playing: false,
            buffering: false,
            narrating: false,
            position_ms: 0,
            duration_ms: 0,
            position: 0.0,
            volume: 0.0,
            shuffle: false,
            smart_shuffle: false,
            smart_shuffle_available: false,
            repeat: QString::from(Repeat::Off.as_str()),
            queue: DictListModel::create(ModelKind::Queue),
            current_index: -1,
            source: QString::default(),
            error_text: QString::default(),
            level: 0.0,
            levels_wanted: false,
            cover_large: QString::default(),
            uri: QString::default(),
            is_episode: false,
            is_local: false,
            remote: false,
            device: QVariantMap::default(),
            local_state: QString::from("unavailable"),
            can_control: true,
            can_set_volume: true,
            transferring: false,
            device_lost: false,
            queue_context: QVariantMap::default(),
            context_title: QString::default(),
            context_uri: QString::default(),
            context_remaining: 0,
            manual_count: 0,
            can_clear_queue: false,
            queue_reorderable: false,
            track_item: None,
            repeat_mode: Repeat::Off,
            volume_set_at: None,
            mode_set_at: None,
            position_set_at: None,
            volume_save: 0,
            stale: None,
            queue_rows: Arc::default(),
            device_item: None,
        }
    }
}

/// Пауза перед записью громкости в настройки.
const VOLUME_SAVE_DELAY: Duration = Duration::from_millis(400);

/// Списки присылают только URI, а выбранный трек нужен ядру, чтобы показать его сразу — до ответа движка.
/// Метаданные уже лежат в общем кэше каталога (его наполняют те же списки).
fn remember_track(uri: &str) {
    if let Some(track) = sf_services::data::known::get(uri) {
        ctx().services.player.remember(track);
    }
}

/// {type, uri, title, cover} — карточка «Играет из …»; {} — контекста нет.
fn context_map(context: &PlayContext) -> QVariantMap {
    let mut map = QVariantMap::default();
    if context.kind.is_empty() && context.context_uri.is_none() {
        return map;
    }
    let uri = context.context_uri.as_deref().unwrap_or_default();
    for (key, value) in [("type", context.kind.as_str()), ("uri", uri), ("title", &context.title), ("cover", &context.cover)] {
        map.insert(QString::from(key), QVariant::from(&QString::from(value)));
    }
    map
}

fn clamp_ms(ms: i64) -> i32 {
    ms.clamp(0, i32::MAX as i64) as i32
}

fn fraction(position_ms: i32, duration_ms: i32) -> f64 {
    if duration_ms > 0 { position_ms as f64 / duration_ms as f64 } else { 0.0 }
}

impl qobject::Player {
    fn queue_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.queue)
    }

    // --- сеттеры -----------------------------------------------------------

    fn set_volume(mut self: Pin<&mut Self>, value: f64) {
        let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        if value == self.volume {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.volume = value;
        // Новое решение пользователя отменяет ожидаемое эхо-перепроверку
        this.stale = None;
        this.volume_set_at = Some(Instant::now());
        this.volume_save += 1;
        let generation = this.volume_save;
        self.as_mut().volume_changed();
        ctx().services.player.set_volume(value);
        self.schedule_volume_save(generation);
    }

    /// Снимок был отклонён эхо-удержанием: перепроверить громкость и режимы, когда удержание истечёт.
    fn schedule_resync(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        ctx().rt.spawn(async move {
            tokio::time::sleep(super::ECHO_HOLD + std::time::Duration::from_millis(100)).await;
            let _ = thread.queue(move |player| player.resync());
        });
    }

    fn resync(mut self: Pin<&mut Self>) {
        // stale — только флаг «перепроверить»: за 600 мс трек или устройство могли смениться,
        // применение старого снимка целиком откатывало бы свежее состояние. Берём текущий.
        if self.stale.is_none() {
            return;
        }
        self.as_mut().rust_mut().stale = None;
        let s = ctx().services.player.snapshot();
        self.as_mut().sync(&s, false);
    }

    fn schedule_volume_save(self: Pin<&mut Self>, generation: u64) {
        let thread = self.qt_thread();
        ctx().rt.spawn(async move {
            tokio::time::sleep(VOLUME_SAVE_DELAY).await;
            let _ = thread.queue(move |player| player.save_volume(generation));
        });
    }

    fn save_volume(&self, generation: u64) {
        // громкость чужого устройства Connect — не громкость этого ПК
        if generation == self.volume_save && !self.remote {
            ctx().services.settings.set(SettingKey::Volume, SettingValue::Float(self.volume));
        }
    }

    fn set_shuffle(mut self: Pin<&mut Self>, value: bool) {
        if value == self.shuffle {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.shuffle = value;
        this.smart_shuffle = false;
        this.stale = None;
        this.mode_set_at = Some(Instant::now());
        self.mode_changed();
        ctx().services.player.set_shuffle(value);
    }

    /// Smart Shuffle включает и перемешивание; выключение оставляет обычное перемешивание.
    fn set_smart_shuffle(mut self: Pin<&mut Self>, value: bool) {
        if value == self.smart_shuffle || (value && !self.smart_shuffle_available) {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.smart_shuffle = value;
        this.shuffle = true;
        this.stale = None;
        this.mode_set_at = Some(Instant::now());
        self.mode_changed();
        ctx().services.player.set_smart_shuffle(value);
    }

    fn cycle_shuffle(self: Pin<&mut Self>) {
        if self.shuffle && !self.smart_shuffle && self.smart_shuffle_available {
            self.set_smart_shuffle(true);
        } else {
            let next = !self.shuffle;
            self.set_shuffle(next);
        }
    }

    fn set_repeat(self: Pin<&mut Self>, value: QString) {
        if let Some(repeat) = Repeat::from_str(&value.to_string()) {
            self.apply_repeat(repeat);
        }
    }

    fn apply_repeat(mut self: Pin<&mut Self>, repeat: Repeat) {
        if repeat == self.repeat_mode {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.stale = None;
        this.repeat_mode = repeat;
        this.repeat = QString::from(repeat.as_str());
        this.mode_set_at = Some(Instant::now());
        self.mode_changed();
        ctx().services.player.set_repeat(repeat);
    }

    fn set_levels_wanted(mut self: Pin<&mut Self>, value: bool) {
        if value == self.levels_wanted {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.levels_wanted = value;
        if !value {
            this.level = 0.0;
        }
        self.as_mut().level_changed();
        ctx().services.player.set_levels_wanted(value);
    }

    // --- управление --------------------------------------------------------

    unsafe fn play_from(self: Pin<&mut Self>, model: *mut cxx_qt::QObject, row: i32) {
        let Some((items, context)) = (unsafe { crate::models::tracks_from_model(model) }) else { return };
        let Ok(row) = usize::try_from(row) else { return };
        if row < items.len() {
            ctx().services.player.play_items(items, row, context);
        }
    }

    unsafe fn shuffle_from(mut self: Pin<&mut Self>, model: *mut cxx_qt::QObject) {
        let Some((items, context)) = (unsafe { crate::models::tracks_from_model(model) }) else { return };
        if items.is_empty() {
            return;
        }
        if !self.shuffle {
            let mut this = self.as_mut().rust_mut();
            this.shuffle = true;
            this.mode_set_at = Some(Instant::now());
            self.as_mut().mode_changed();
        }
        ctx().services.player.shuffle_from(items, context);
    }

    fn play_index(&self, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            ctx().services.player.play_index(index);
        }
    }

    fn toggle_play(&self) {
        ctx().services.player.toggle_play();
    }

    fn play(&self) {
        ctx().services.player.play();
    }

    fn pause(&self) {
        ctx().services.player.pause();
    }

    fn stop(&self) {
        ctx().services.player.stop();
    }

    fn next(&self) {
        ctx().services.player.next();
    }

    /// После 3 с — в начало трека, иначе предыдущий.
    fn previous(&self) {
        ctx().services.player.previous();
    }

    fn seek(self: Pin<&mut Self>, fraction: f64) {
        if self.duration_ms > 0 && fraction.is_finite() {
            let ms = (fraction * self.duration_ms as f64) as i64;
            self.seek_ms(clamp_ms(ms));
        }
    }

    fn seek_ms(mut self: Pin<&mut Self>, ms: i32) {
        if !self.has_track {
            return;
        }
        let ms = ms.max(0);
        let mut this = self.as_mut().rust_mut();
        this.position_ms = ms;
        this.position = fraction(ms, this.duration_ms);
        this.position_set_at = Some(Instant::now());
        self.position_changed();
        ctx().services.player.seek_ms(ms as i64);
    }

    /// off → all → one → off
    fn cycle_repeat(self: Pin<&mut Self>) {
        let next = self.repeat_mode.cycled();
        self.apply_repeat(next);
    }

    fn seek_relative(mut self: Pin<&mut Self>, delta_ms: i32) {
        if !self.has_track {
            return;
        }
        let mut target = i64::from(self.position_ms) + i64::from(delta_ms);
        if self.duration_ms > 0 {
            target = target.min(i64::from(self.duration_ms));
        }
        let ms = clamp_ms(target);
        let mut this = self.as_mut().rust_mut();
        this.position_ms = ms;
        this.position = fraction(ms, this.duration_ms);
        this.position_set_at = Some(Instant::now());
        self.as_mut().position_changed();
        ctx().services.player.seek_relative(i64::from(delta_ms));
    }

    fn play_queue_row(&self, uid: f64) {
        if uid.is_finite() && uid >= 0.0 {
            ctx().services.player.play_queue_row(uid as u64);
        }
    }

    fn play_context(&self, uri: &QString, offset_uri: &QString) {
        let uri = uri.to_string();
        if uri.is_empty() {
            return;
        }
        let kind = sf_api::util::uri_kind(&uri).unwrap_or_default().to_owned();
        let offset = offset_uri.to_string();
        remember_track(&offset);
        let context = PlayContext::with_uri(&kind, uri);
        ctx().services.player.play_context(context, (!offset.is_empty()).then_some(offset), None);
    }

    fn shuffle_context(&self, uri: &QString) {
        let uri = uri.to_string();
        if uri.is_empty() {
            return;
        }
        let kind = sf_api::util::uri_kind(&uri).unwrap_or_default().to_owned();
        ctx().services.player.play_context(PlayContext::with_uri(&kind, uri), None, Some(true));
    }

    fn play_uris(&self, uris: &QStringList, index: i32) {
        let uris = string_list(uris);
        if let Ok(index) = usize::try_from(index)
            && index < uris.len()
        {
            remember_track(&uris[index]);
            ctx().services.player.play_uris(uris, index, PlayContext::new(md3_model::contexts::TRACKS));
        }
    }

    fn play_episode(&self, uri: &QString, position_ms: i32) {
        remember_track(&uri.to_string());
        ctx().services.player.play_episode(uri.to_string(), i64::from(position_ms.max(0)));
    }

    fn add_to_queue(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.player.add_to_queue(vec![track]);
        }
    }

    fn remember_track(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.player.remember(track);
        }
    }

    fn add_uri_to_queue(&self, uri: &QString) {
        ctx().services.player.add_uri_to_queue(uri.to_string());
    }

    fn insert_manual(&self, track: &QVariantMap, at: i32) {
        if let (Some(track), Ok(at)) = (track_from_map(track), usize::try_from(at)) {
            ctx().services.player.insert_manual(track, at);
        }
    }

    fn move_manual(&self, from: i32, to: i32) {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) {
            ctx().services.player.move_manual(from, to);
        }
    }

    fn clear_manual_queue(&self) {
        ctx().services.player.clear_manual_queue();
    }

    fn transfer_here(&self) {
        ctx().services.player.transfer_here(true);
    }

    fn set_app_active(&self, active: bool) {
        ctx().services.player.set_app_active(active);
    }

    // --- снимок плеера → свойства -------------------------------------------

    fn on_event(mut self: Pin<&mut Self>, delivery: Delivery<PlayerEvent>) {
        // Смена устройства/переподключение: отклонённые эхо-снимки прошлого соединения неактуальны
        if matches!(delivery, Delivery::Event(PlayerEvent::DeviceChanged)) {
            self.as_mut().rust_mut().stale = None;
        }
        if let Delivery::Event(PlayerEvent::Notice(text)) = &delivery {
            tracing::debug!("Player.notice: {text}");
            self.as_mut().notice(QString::from(text.as_str()));
        }
        // Кадры «дыхания» приходят ~30 Гц, позиция ~5 Гц: для «дешёвых» событий сверяем
        // только свою группу свойств — snapshot() клонируется как раньше, зато не
        // перестраиваются QString'ы и QMap'ы. Структурные события (трек, состояние,
        // очередь, устройство) редки — им остаётся полный sync().
        let snapshot = ctx().services.player.snapshot();
        match delivery {
            Delivery::Event(PlayerEvent::LevelChanged) => self.as_mut().sync_level(&snapshot),
            Delivery::Event(PlayerEvent::PositionChanged) => self.as_mut().sync_position(&snapshot),
            Delivery::Event(PlayerEvent::VolumeChanged) => self.as_mut().sync_volume(&snapshot),
            Delivery::Event(PlayerEvent::ModeChanged) => self.as_mut().sync_mode(&snapshot),
            Delivery::Event(PlayerEvent::CoverFileChanged) => self.as_mut().sync_cover(&snapshot),
            Delivery::Event(PlayerEvent::ErrorChanged) => self.as_mut().sync_error(&snapshot),
            _ => {
                let force_track = matches!(delivery, Delivery::Event(PlayerEvent::TrackChanged));
                self.sync(&snapshot, force_track);
            }
        }
    }

    /// Полная сверка со снимком — для структурных (редких) событий и после пропуска событий.
    fn sync(mut self: Pin<&mut Self>, s: &PlayerSnapshot, force_track: bool) {
        self.as_mut().sync_queue(s);
        self.as_mut().sync_track(s, force_track);
        self.as_mut().sync_playback(s);
        self.as_mut().sync_position(s);
        self.as_mut().sync_volume(s);
        self.as_mut().sync_mode(s);
        self.as_mut().sync_queue_index(s);
        self.as_mut().sync_queue_sections(s);
        self.as_mut().sync_device(s);
        self.as_mut().sync_cover(s);
        self.as_mut().sync_error(s);
        self.as_mut().sync_level(s);
    }

    /// Модель очереди ← очередь снимка (по uid, изменения по месту).
    fn sync_queue(mut self: Pin<&mut Self>, snapshot: &PlayerSnapshot) {
        if Arc::ptr_eq(&self.queue_rows, &snapshot.queue) {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.queue_rows = snapshot.queue.clone();
        with_model(&mut this.queue, |m| m.sync_rows(to_rows(&snapshot.queue), "uid"));
    }

    fn sync_track(mut self: Pin<&mut Self>, s: &PlayerSnapshot, force_track: bool) {
        // трек
        let track_changed = self.track_item != s.track || self.codec != QString::from(s.codec.as_str())
            || self.direction != s.direction;
        if track_changed {
            let mut this = self.as_mut().rust_mut();
            let track = s.track.clone().unwrap_or_default();
            this.has_track = s.track.is_some();
            this.track_id = QString::from(track.track_id.as_str());
            this.title = QString::from(track.title.as_str());
            this.artist = QString::from(track.artists.as_str());
            this.album = QString::from(track.album.as_str());
            this.cover = QString::from(track.cover.as_str());
            this.track = if s.track.is_some() { json_to_qvariantmap(&to_row(&track)) } else { QVariantMap::default() };
            this.cover_large = QString::from(track.cover_large.as_str());
            this.uri = QString::from(track.uri.as_str());
            this.is_episode = track.is_episode;
            this.codec = QString::from(s.codec.as_str());
            this.direction = s.direction;
            if this.track_item.as_ref().map(|t| &t.track_id) != s.track.as_ref().map(|t| &t.track_id) {
                this.position_set_at = None;
            }
            this.track_item = s.track.clone();
        }
        if track_changed || force_track {
            self.as_mut().track_changed();
        }
    }

    fn sync_playback(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // воспроизведение
        if self.playing != s.playing || self.buffering != s.buffering || self.narrating != s.narrating {
            let mut this = self.as_mut().rust_mut();
            this.playing = s.playing;
            this.buffering = s.buffering;
            this.narrating = s.narrating;
            self.as_mut().state_changed();
        }
    }

    fn sync_position(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // позиция
        let duration_ms = clamp_ms(s.duration_ms);
        let position_ms = if held(self.position_set_at) { self.position_ms } else { clamp_ms(s.position_ms) };
        if self.position_ms != position_ms || self.duration_ms != duration_ms {
            let mut this = self.as_mut().rust_mut();
            this.position_ms = position_ms;
            this.duration_ms = duration_ms;
            this.position = fraction(position_ms, duration_ms);
            self.as_mut().position_changed();
        }
    }

    fn sync_volume(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // громкость; изменение из системной панели (SMTC/MPRIS) тоже сохраняется в Settings.volume
        // Отклонённое из-за удержания эхо запоминаем: иначе на паузе (позиция не тикает) оно
        // никогда не перепроверится, и UI застрянет на запрошенном значении.
        if held(self.volume_set_at) && self.volume != s.volume {
            let mut this = self.as_mut().rust_mut();
            this.stale = Some(());
            self.as_mut().schedule_resync();
        }
        if !held(self.volume_set_at) && self.volume != s.volume {
            let mut this = self.as_mut().rust_mut();
            this.volume = s.volume;
            this.volume_save += 1;
            let generation = this.volume_save;
            this.stale = None;
            self.as_mut().volume_changed();
            if s.is_local || s.device.is_none() {
                self.as_mut().schedule_volume_save(generation);
            }
        }
    }

    fn sync_mode(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // режимы
        let changed = self.shuffle != s.shuffle || self.smart_shuffle != s.smart_shuffle || self.repeat_mode != s.repeat;
        if held(self.mode_set_at) && changed {
            let mut this = self.as_mut().rust_mut();
            this.stale = Some(());
            self.as_mut().schedule_resync();
        }
        if !held(self.mode_set_at) && changed {
            let mut this = self.as_mut().rust_mut();
            this.shuffle = s.shuffle;
            this.smart_shuffle = s.smart_shuffle;
            this.repeat_mode = s.repeat;
            this.repeat = QString::from(s.repeat.as_str());
            self.as_mut().mode_changed();
        }
        if self.smart_shuffle_available != s.smart_shuffle_available {
            self.as_mut().rust_mut().smart_shuffle_available = s.smart_shuffle_available;
            self.as_mut().mode_changed();
        }
    }

    fn sync_queue_index(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // очередь
        let source = QString::from(s.source());
        let context_changed = self.queue.as_ref().is_some_and(|q| q.context() != &s.context);
        if self.current_index != s.current_index || self.source != source || context_changed {
            let mut this = self.as_mut().rust_mut();
            this.current_index = s.current_index;
            this.source = source;
            with_model(&mut this.queue, |m| m.set_context(s.context.clone()));
            self.as_mut().queue_changed();
        }
    }

    fn sync_queue_sections(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // контекст и секции очереди
        let queue_context = context_map(&s.context);
        if self.queue_context != queue_context
            || self.context_remaining != s.context_remaining
            || self.manual_count != s.manual_count
            || self.can_clear_queue != s.can_clear_queue
            || self.queue_reorderable != s.queue_reorderable
        {
            let mut this = self.as_mut().rust_mut();
            this.queue_context = queue_context;
            this.context_title = QString::from(s.context.title.as_str());
            this.context_uri = QString::from(s.context.context_uri.as_deref().unwrap_or_default());
            this.context_remaining = s.context_remaining;
            this.manual_count = s.manual_count;
            this.can_clear_queue = s.can_clear_queue;
            this.queue_reorderable = s.queue_reorderable;
            self.as_mut().queue_changed();
        }
    }

    fn sync_device(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // устройство Connect. QMap из снимка строится только когда типизированное значение
        // устройства изменилось: сериализация дороже сравнения, а DeviceChanged приходит и от
        // полей, до QMap не доходящих (canControl и т.п.).
        let device_changed = self.device_item != s.device;
        let local_state = QString::from(match &s.local {
            LocalPlayback::Unavailable => "unavailable",
            LocalPlayback::Connecting => "connecting",
            LocalPlayback::Ready { .. } => "ready",
            LocalPlayback::Failed(_) => "failed",
        });
        let remote = !s.is_local && s.device.is_some();
        if device_changed
            || self.is_local != s.is_local
            || self.remote != remote
            || self.local_state != local_state
            || self.can_control != s.can_control
            || self.can_set_volume != s.can_set_volume
            || self.transferring != s.transferring
            || self.device_lost != s.device_lost
        {
            let mut this = self.as_mut().rust_mut();
            if device_changed {
                this.device = s.device.as_ref().map(to_qmap).unwrap_or_default();
                this.device_item = s.device.clone();
            }
            this.is_local = s.is_local;
            this.remote = remote;
            this.local_state = local_state;
            this.can_control = s.can_control;
            this.can_set_volume = s.can_set_volume;
            this.transferring = s.transferring;
            this.device_lost = s.device_lost;
            self.as_mut().device_changed();
        }
    }

    fn sync_cover(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // обложка
        let cover_file = QString::from(s.cover_file.as_str());
        if self.cover_file != cover_file {
            self.as_mut().rust_mut().cover_file = cover_file;
            self.as_mut().cover_file_changed();
        }
    }

    fn sync_error(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // ошибка
        let error_text = QString::from(s.error_text.as_str());
        if self.error_text != error_text {
            self.as_mut().rust_mut().error_text = error_text;
            self.as_mut().error_changed();
        }
    }

    fn sync_level(mut self: Pin<&mut Self>, s: &PlayerSnapshot) {
        // уровень — только пока он нужен QML
        let level = if self.levels_wanted { s.level } else { 0.0 };
        if self.level != level {
            self.as_mut().rust_mut().level = level;
            self.level_changed();
        }
    }
}

impl cxx_qt::Initialize for qobject::Player {
    fn initialize(mut self: Pin<&mut Self>) {
        let player = &ctx().services.player;
        let subscription = player.subscribe();
        let snapshot = player.snapshot();
        self.as_mut().sync(&snapshot, false);
        forward_events(self.qt_thread(), subscription.events, Self::on_event);
    }
}
