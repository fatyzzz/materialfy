//! DictListModel — список словарей с фиксированным набором ключей; ключи = роли QML.
//! Замена TrackListModel/AlbumListModel/ArtistListModel/PlaylistListModel из models.py:
//! QML обращается к ним только через count, loading, totalDurationMs, get(), find() и роли.
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::CxxQtType;
use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant};
use serde_json::{Map, Value};

use crate::qt::{QVariantMap, json_to_qvariant, json_to_qvariantmap, qvariant_to_json};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }

    extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        #[qproperty(bool, loading, READ, NOTIFY = loading_changed)]
        #[qproperty(i64, total_duration_ms, cxx_name = "totalDurationMs", READ = total_duration_ms, NOTIFY = count_changed)]
        type DictListModel = super::DictListModelRust;

        fn count(self: &DictListModel) -> i32;
        #[cxx_name = "totalDurationMsValue"]
        fn total_duration_ms(self: &DictListModel) -> i64;

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut DictListModel>);
        #[qsignal]
        #[cxx_name = "loadingChanged"]
        fn loading_changed(self: Pin<&mut DictListModel>);
        /// Любое изменение строк (сброс, вставка, удаление, перестановка, замена) — для TrackFilter.
        #[qsignal]
        #[cxx_name = "rowsChanged"]
        fn rows_changed(self: Pin<&mut DictListModel>);

        #[qinvokable]
        fn get(self: &DictListModel, row: i32) -> QMap_QString_QVariant;
        #[qinvokable]
        fn find(self: &DictListModel, key: &QString, value: &QVariant) -> i32;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &DictListModel, parent: &QModelIndex) -> i32;
        #[cxx_override]
        fn data(self: &DictListModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &DictListModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut DictListModel>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut DictListModel>);
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(self: Pin<&mut DictListModel>, parent: &QModelIndex, first: i32, last: i32);
        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut DictListModel>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        fn begin_remove_rows(self: Pin<&mut DictListModel>, parent: &QModelIndex, first: i32, last: i32);
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        fn end_remove_rows(self: Pin<&mut DictListModel>);
        #[inherit]
        #[cxx_name = "beginMoveRows"]
        fn begin_move_rows(
            self: Pin<&mut DictListModel>,
            source_parent: &QModelIndex,
            source_first: i32,
            source_last: i32,
            destination_parent: &QModelIndex,
            destination_child: i32,
        ) -> bool;
        #[inherit]
        #[cxx_name = "endMoveRows"]
        fn end_move_rows(self: Pin<&mut DictListModel>);
        #[inherit]
        fn index(self: &DictListModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
        #[inherit]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut DictListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/common.h");
        #[doc(hidden)]
        #[namespace = "rust::cxxqtlib1"]
        #[cxx_name = "make_unique"]
        fn dict_list_model_make_unique() -> UniquePtr<DictListModel>;
    }
}

cxx_qt::impl_transitive_cast!(qobject::DictListModel, qobject::QAbstractListModel, cxx_qt::QObject);

/// Строка модели: словарь с ключами модели. Arc — дешёвые копии и сравнение по указателю (TrackFilter).
pub type Row = Arc<Map<String, Value>>;

/// Набор ключей (= ролей QML); = serde-ключи md3-model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelKind {
    #[default]
    Track,
    Album,
    Artist,
    Playlist,
    Show,
    Episode,
    Device,
    Queue,
    Library,
}

pub const TRACK_KEYS: &[&str] = &[
    "trackId", "albumId", "title", "version", "artists", "artistRefs", "album", "cover", "coverLarge", "durationMs",
    "explicit", "available", "uri", "isEpisode", "showId", "addedAt", "addedBy", "playCount",
];
pub const PLAYLIST_KEYS: &[&str] = &[
    "playlistId", "uri", "title", "description", "trackCount", "cover", "owner", "ownerId", "canEdit", "collaborative",
    "blend", "public", "snapshotId", "pinned",
];
pub const ALBUM_KEYS: &[&str] = &[
    "albumId", "uri", "title", "version", "artists", "artistRefs", "year", "kind", "trackCount", "cover", "coverLarge",
    "explicit", "saved",
];
pub const ARTIST_KEYS: &[&str] = &["artistId", "uri", "name", "cover", "coverLarge", "genres", "followers"];
pub const SHOW_KEYS: &[&str] =
    &["showId", "uri", "name", "publisher", "description", "cover", "coverLarge", "totalEpisodes", "saved", "explicit"];
pub const EPISODE_KEYS: &[&str] = &[
    "episodeId", "uri", "title", "description", "releaseDate", "durationMs", "explicit", "resumeMs", "fullyPlayed",
    "saved", "cover", "coverLarge", "showId", "showName", "number",
];
pub const DEVICE_KEYS: &[&str] =
    &["id", "name", "type", "isActive", "isThisComputer", "isRestricted", "supportsVolume", "volume", "source"];
pub const QUEUE_KEYS: &[&str] = &[
    "section", "uid", "trackId", "albumId", "title", "version", "artists", "artistRefs", "album", "cover", "coverLarge",
    "durationMs", "explicit", "available", "uri", "isEpisode", "showId", "addedAt", "addedBy", "playCount",
];
pub const LIBRARY_KEYS: &[&str] = &[
    "kind", "id", "uri", "title", "subtitle", "cover", "covers", "owner", "ownerId", "canEdit", "readOnly", "pinned",
    "trackCount", "addedAt", "playedAt", "folderId",
];

impl ModelKind {
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            ModelKind::Track => TRACK_KEYS,
            ModelKind::Album => ALBUM_KEYS,
            ModelKind::Artist => ARTIST_KEYS,
            ModelKind::Playlist => PLAYLIST_KEYS,
            ModelKind::Show => SHOW_KEYS,
            ModelKind::Episode => EPISODE_KEYS,
            ModelKind::Device => DEVICE_KEYS,
            ModelKind::Queue => QUEUE_KEYS,
            ModelKind::Library => LIBRARY_KEYS,
        }
    }
}

pub(crate) const USER_ROLE: i32 = 0x0100;

#[derive(Default)]
pub struct DictListModelRust {
    kind: ModelKind,
    rows: Vec<Row>,
    loading: bool,
    /// откуда треки (для плеера и учёта прослушиваний); у моделей не-треков пустой
    context: md3_model::PlayContext,
}

impl qobject::DictListModel {
    /// Новая модель во владении Rust (CppOwnership уже выставлен — QML её не удалит).
    pub fn create(kind: ModelKind) -> cxx::UniquePtr<Self> {
        let mut model = qobject::dict_list_model_make_unique();
        if let Some(pinned) = model.as_mut() {
            pinned.rust_mut().kind = kind;
            let ptr = model.as_mut_ptr();
            unsafe { crate::qt::set_cpp_ownership(<Self as Upcast<cxx_qt::QObject>>::upcast_ptr(ptr).cast_mut()) };
        }
        model
    }

    pub fn rows(&self) -> &[Row] {
        &self.rust().rows
    }

    pub fn context(&self) -> &md3_model::PlayContext {
        &self.rust().context
    }

    pub fn set_context(self: Pin<&mut Self>, context: md3_model::PlayContext) {
        self.rust_mut().context = context;
    }

    pub fn reset_rows(mut self: Pin<&mut Self>, rows: Vec<Row>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        self.as_mut().count_changed();
        self.rows_changed();
    }

    pub fn append_rows(self: Pin<&mut Self>, rows: Vec<Row>) {
        let at = self.rust().rows.len();
        self.insert_rows(at, rows);
    }

    pub fn insert_rows(mut self: Pin<&mut Self>, at: usize, rows: Vec<Row>) {
        if rows.is_empty() {
            return;
        }
        self.as_mut().splice_in(at, rows);
        self.as_mut().count_changed();
        self.rows_changed();
    }

    pub fn remove_rows(mut self: Pin<&mut Self>, at: usize, count: usize) {
        if self.as_mut().splice_out(at, count) {
            self.as_mut().count_changed();
            self.rows_changed();
        }
    }

    /// Перестановка одной строки (from → to — индексы до перемещения, как ListChange::Move).
    pub fn move_row(mut self: Pin<&mut Self>, from: usize, to: usize) {
        if self.as_mut().shift(from, to) {
            self.rows_changed();
        }
    }

    /// Привести строки к `rows` минимальными изменениями по ключу `key` (uid очереди, id устройства):
    /// удалить пропавшие, переставить, вставить новые, заменить изменившиеся. Пустой или повторный ключ — сброс.
    pub fn sync_rows(mut self: Pin<&mut Self>, rows: Vec<Row>, key: &str) {
        let Some(ops) = sync_ops(&self.rust().rows, &rows, key) else {
            self.reset_rows(rows);
            return;
        };
        if ops.is_empty() {
            return;
        }
        let count_before = self.rust().rows.len();
        for op in ops {
            match op {
                SyncOp::Remove { at, count } => {
                    self.as_mut().splice_out(at, count);
                }
                SyncOp::Move { from, to } => {
                    self.as_mut().shift(from, to);
                }
                SyncOp::Insert { at, count } => self.as_mut().splice_in(at, rows[at..at + count].to_vec()),
                SyncOp::Replace { at, count } => {
                    self.as_mut().rust_mut().rows.splice(at..at + count, rows[at..at + count].iter().cloned());
                    self.as_mut().notify_data(at, at + count - 1);
                }
            }
        }
        if self.rust().rows.len() != count_before {
            self.as_mut().count_changed();
        }
        self.rows_changed();
    }

    fn splice_in(mut self: Pin<&mut Self>, at: usize, rows: Vec<Row>) {
        if rows.is_empty() {
            return;
        }
        let at = at.min(self.rust().rows.len());
        let last = at + rows.len() - 1;
        self.as_mut().begin_insert_rows(&QModelIndex::default(), at as i32, last as i32);
        self.as_mut().rust_mut().rows.splice(at..at, rows);
        self.as_mut().end_insert_rows();
    }

    fn splice_out(mut self: Pin<&mut Self>, at: usize, count: usize) -> bool {
        let len = self.rust().rows.len();
        if at >= len || count == 0 {
            return false;
        }
        let count = count.min(len - at);
        self.as_mut().begin_remove_rows(&QModelIndex::default(), at as i32, (at + count - 1) as i32);
        self.as_mut().rust_mut().rows.drain(at..at + count);
        self.as_mut().end_remove_rows();
        true
    }

    fn shift(mut self: Pin<&mut Self>, from: usize, to: usize) -> bool {
        let len = self.rust().rows.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        // destinationChild у Qt — позиция до перемещения: при сдвиге вниз — на одну дальше
        let child = if to > from { to + 1 } else { to };
        let parent = QModelIndex::default();
        if !self.as_mut().begin_move_rows(&parent, from as i32, from as i32, &parent, child as i32) {
            return false;
        }
        let mut this = self.as_mut().rust_mut();
        let row = this.rows.remove(from);
        this.rows.insert(to, row);
        self.as_mut().end_move_rows();
        true
    }

    fn notify_data(mut self: Pin<&mut Self>, first: usize, last: usize) {
        let parent = QModelIndex::default();
        let top = self.index(first as i32, 0, &parent);
        let bottom = self.index(last as i32, 0, &parent);
        self.as_mut().data_changed(&top, &bottom, &QList::default());
    }

    pub fn is_loading(&self) -> bool {
        self.rust().loading
    }

    pub fn set_loading(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().loading != value {
            self.as_mut().rust_mut().loading = value;
            self.loading_changed();
        }
    }

    pub fn find_row(&self, key: &str, value: &Value) -> Option<usize> {
        find_in(&self.rust().rows, key, value)
    }

    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    fn total_duration_ms(&self) -> i64 {
        total_duration(&self.rust().rows)
    }

    fn get(&self, row: i32) -> QVariantMap {
        row_map(&self.rust().rows, row)
    }

    fn find(&self, key: &QString, value: &QVariant) -> i32 {
        self.find_row(&key.to_string(), &qvariant_to_json(value)).map_or(-1, |i| i as i32)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() { 0 } else { self.count() }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        row_data(&self.rust().rows, self.rust().kind.keys(), index, role)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        role_names_for(self.rust().kind.keys())
    }
}

/// Шаг sync_rows; индексы — в строках модели на момент шага. Insert/Replace берут строки `rows[at..at + count]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncOp {
    Remove { at: usize, count: usize },
    Move { from: usize, to: usize },
    Insert { at: usize, count: usize },
    Replace { at: usize, count: usize },
}

/// Ключ строки текстом ("" — нет).
fn row_key(row: &Row, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn same_row(a: &Row, b: &Row) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

/// Шаги от `old` к `new`: удаления (с конца, кусками), перестановки оставшихся, вставки (кусками), замены.
/// None — ключ пустой или повторяется (тогда сброс). Пусто — строки уже совпадают.
fn sync_ops(old: &[Row], new: &[Row], key: &str) -> Option<Vec<SyncOp>> {
    if old.len() == new.len() && old.iter().zip(new).all(|(a, b)| same_row(a, b)) {
        return Some(Vec::new());
    }
    let new_keys: Vec<String> = new.iter().map(|r| row_key(r, key)).collect();
    let mut new_set = HashSet::with_capacity(new.len());
    if new_keys.iter().any(|k| k.is_empty() || !new_set.insert(k.as_str())) {
        return None;
    }
    let mut cur: Vec<String> = old.iter().map(|r| row_key(r, key)).collect();
    let mut old_set = HashSet::with_capacity(old.len());
    if cur.iter().any(|k| k.is_empty() || !old_set.insert(k.clone())) {
        return None;
    }
    let mut ops = Vec::new();

    // удаление — с конца, подряд идущие одним куском
    let mut end = cur.len();
    while end > 0 {
        if new_set.contains(cur[end - 1].as_str()) {
            end -= 1;
            continue;
        }
        let mut start = end - 1;
        while start > 0 && !new_set.contains(cur[start - 1].as_str()) {
            start -= 1;
        }
        ops.push(SyncOp::Remove { at: start, count: end - start });
        cur.drain(start..end);
        end = start;
    }

    // перестановки оставшихся: их порядок → порядок `new` без новых строк
    let common: Vec<&String> = new_keys.iter().filter(|k| old_set.contains(*k)).collect();
    for target in 0..common.len() {
        if cur[target] == *common[target] {
            continue;
        }
        // строка ушла вниз (следующая уже на её месте) — двигаем её, иначе подтягиваем нужную снизу
        let (from, to) = if cur.get(target + 1) == Some(common[target]) {
            (target, common.iter().position(|k| **k == cur[target]).unwrap_or(target))
        } else {
            let from = cur.iter().skip(target + 1).position(|k| k == common[target]).map_or(target, |i| target + 1 + i);
            (from, target)
        };
        if from != to {
            let row = cur.remove(from);
            cur.insert(to, row);
            ops.push(SyncOp::Move { from, to });
        }
    }

    // вставка новых, подряд идущие одним куском
    let mut at = 0;
    while at < new.len() {
        if old_set.contains(&new_keys[at]) {
            at += 1;
            continue;
        }
        let mut end = at + 1;
        while end < new.len() && !old_set.contains(&new_keys[end]) {
            end += 1;
        }
        ops.push(SyncOp::Insert { at, count: end - at });
        at = end;
    }

    // замена изменившихся: ключи уже совпадают по позициям, старые строки ищутся по ключу
    let old_by_key: std::collections::HashMap<String, &Row> = old.iter().map(|r| (row_key(r, key), r)).collect();
    let mut at = 0;
    while at < new.len() {
        let changed = |i: usize| old_by_key.get(new_keys[i].as_str()).is_some_and(|o| !same_row(o, &new[i]));
        if !changed(at) {
            at += 1;
            continue;
        }
        let mut end = at + 1;
        while end < new.len() && changed(end) {
            end += 1;
        }
        ops.push(SyncOp::Replace { at, count: end - at });
        at = end;
    }
    Some(ops)
}

// Общие части DictListModel и TrackFilter

pub(crate) fn find_in(rows: &[Row], key: &str, value: &Value) -> Option<usize> {
    rows.iter().position(|row| row.get(key).is_some_and(|v| json_eq(v, value)))
}

/// sum(int(durationMs or 0)) из models.py
pub(crate) fn total_duration(rows: &[Row]) -> i64 {
    rows.iter()
        .filter_map(|r| r.get("durationMs"))
        .map(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).unwrap_or(0))
        .sum()
}

pub(crate) fn row_map(rows: &[Row], row: i32) -> QVariantMap {
    usize::try_from(row).ok().and_then(|r| rows.get(r)).map(|r| json_to_qvariantmap(r)).unwrap_or_default()
}

pub(crate) fn row_data(rows: &[Row], keys: &[&str], index: &QModelIndex, role: i32) -> QVariant {
    if !index.is_valid() {
        return QVariant::default();
    }
    let row = usize::try_from(index.row()).ok().and_then(|r| rows.get(r));
    let key = usize::try_from(role - USER_ROLE).ok().and_then(|k| keys.get(k));
    match (row, key) {
        (Some(row), Some(key)) => row.get(*key).map(json_to_qvariant).unwrap_or_default(),
        _ => QVariant::default(),
    }
}

pub(crate) fn role_names_for(keys: &[&str]) -> QHash<QHashPair_i32_QByteArray> {
    let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
    for (i, key) in keys.iter().enumerate() {
        roles.insert(USER_ROLE + i as i32, QByteArray::from(*key));
    }
    roles
}

/// Равенство как в Python (`item.get(key) == value`): числа сравниваются по значению (5 == 5.0).
pub fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(uid: u64, title: &str) -> Row {
        match json!({"uid": uid, "title": title}) {
            Value::Object(map) => Arc::new(map),
            _ => unreachable!(),
        }
    }

    fn rows(spec: &[(u64, &str)]) -> Vec<Row> {
        spec.iter().map(|(u, t)| row(*u, t)).collect()
    }

    /// Применить шаги к списку так же, как sync_rows к модели.
    fn apply(old: &[Row], new: &[Row]) -> (Vec<Row>, Vec<SyncOp>) {
        let ops = sync_ops(old, new, "uid").expect("keys are unique");
        let mut cur = old.to_vec();
        for op in &ops {
            match *op {
                SyncOp::Remove { at, count } => {
                    cur.drain(at..at + count);
                }
                SyncOp::Move { from, to } => {
                    let r = cur.remove(from);
                    cur.insert(to, r);
                }
                SyncOp::Insert { at, count } => {
                    cur.splice(at..at, new[at..at + count].iter().cloned());
                }
                SyncOp::Replace { at, count } => {
                    cur.splice(at..at + count, new[at..at + count].iter().cloned());
                }
            }
        }
        (cur, ops)
    }

    fn check(old: &[(u64, &str)], new: &[(u64, &str)]) -> Vec<SyncOp> {
        let (old, new) = (rows(old), rows(new));
        let (result, ops) = apply(&old, &new);
        assert!(same_rows(&result, &new), "{old:?} → {new:?}: {ops:?}");
        ops
    }

    fn same_rows(a: &[Row], b: &[Row]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_row(x, y))
    }

    #[test]
    fn same_rows_need_nothing() {
        assert!(check(&[(1, "a"), (2, "b")], &[(1, "a"), (2, "b")]).is_empty());
    }

    #[test]
    fn single_move_is_one_step() {
        let old = [(1, "a"), (2, "b"), (3, "c"), (4, "d"), (5, "e")];
        assert_eq!(check(&old, &[(1, "a"), (3, "c"), (4, "d"), (5, "e"), (2, "b")]), [SyncOp::Move { from: 1, to: 4 }]);
        assert_eq!(check(&old, &[(4, "d"), (1, "a"), (2, "b"), (3, "c"), (5, "e")]), [SyncOp::Move { from: 3, to: 0 }]);
    }

    #[test]
    fn next_track_removes_head_and_appends_tail() {
        let ops = check(&[(1, "a"), (2, "b"), (3, "c")], &[(2, "b"), (3, "c"), (4, "d"), (5, "e")]);
        assert_eq!(ops, [SyncOp::Remove { at: 0, count: 1 }, SyncOp::Insert { at: 2, count: 2 }]);
    }

    #[test]
    fn changed_row_is_replaced_in_place() {
        let ops = check(&[(1, "a"), (2, "b")], &[(1, "a"), (2, "B")]);
        assert_eq!(ops, [SyncOp::Replace { at: 1, count: 1 }]);
    }

    #[test]
    fn mixed_changes_converge() {
        check(
            &[(1, "a"), (2, "b"), (3, "c"), (4, "d"), (5, "e"), (6, "f")],
            &[(7, "g"), (5, "e"), (2, "B"), (8, "h"), (1, "a"), (6, "f"), (9, "i")],
        );
        check(&[], &[(1, "a"), (2, "b")]);
        check(&[(1, "a"), (2, "b")], &[]);
        check(&[(1, "a"), (2, "b"), (3, "c")], &[(3, "c"), (2, "b"), (1, "a")]);
    }

    #[test]
    fn duplicate_or_missing_key_means_reset() {
        assert!(sync_ops(&rows(&[(1, "a")]), &rows(&[(1, "a"), (1, "b")]), "uid").is_none());
        assert!(sync_ops(&rows(&[(1, "a")]), &rows(&[(2, "b")]), "id").is_none());
    }
}
