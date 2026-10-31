//! Плоские элементы для QML. serde camelCase; имена полей = роли моделей/ключи словарей QML.
//! Обложки: `cover` ~300 px (сетки, строки), `cover_large` 640 px (шапки, NowPlaying); пусто — нет.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

/// Трек или выпуск подкаста. Ключи QML (TRACK_KEYS): trackId, albumId, title, version, artists, artistRefs, album,
/// cover, coverLarge, durationMs, explicit, available, uri, isEpisode, showId, addedAt, addedBy, playCount.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackItem {
    /// base62 id трека (у выпуска — id выпуска)
    pub track_id: String,
    pub album_id: String,
    pub title: String,
    /// у Spotify нет отдельного поля версии — всегда ""
    pub version: String,
    /// имена через ", "
    pub artists: String,
    pub artist_refs: Vec<ArtistRef>,
    /// название альбома (у выпуска — название шоу)
    pub album: String,
    pub cover: String,
    pub cover_large: String,
    pub duration_ms: i64,
    pub explicit: bool,
    /// is_playable != false; неизвестно → true
    pub available: bool,
    /// spotify:track:ID | spotify:episode:ID
    pub uri: String,
    pub is_episode: bool,
    /// у выпуска — id шоу, иначе ""
    pub show_id: String,
    /// строка плейлиста: ISO-время добавления ("" — неизвестно)
    pub added_at: String,
    /// строка совместного плейлиста: имя добавившего ("" — неизвестно/не нужно)
    pub added_by: String,
    /// прослушивания на Spotify (GraphQL-шлюз: страницы альбома и исполнителя); 0 — неизвестно
    pub play_count: i64,
}

impl TrackItem {
    /// URI для очереди/воспроизведения: `uri` или spotify:track:<trackId>.
    pub fn play_uri(&self) -> String {
        if !self.uri.is_empty() {
            self.uri.clone()
        } else if self.is_episode {
            format!("spotify:episode:{}", self.track_id)
        } else {
            format!("spotify:track:{}", self.track_id)
        }
    }
}

/// Подпись типа альбома по-русски: album_type Spotify (+ число треков для EP).
pub fn album_kind_label(album_type: &str, total_tracks: i64) -> String {
    match album_type {
        "single" if total_tracks > 1 => "EP".to_owned(),
        "single" => mf_i18n::t("Single"),
        "compilation" => mf_i18n::t("Compilation"),
        "ep" => "EP".to_owned(),
        "audiobook" => mf_i18n::t("Audiobook"),
        "podcast" | "show" => mf_i18n::t("Podcast"),
        _ => mf_i18n::t("Album"),
    }
}

/// ALBUM_KEYS: albumId, uri, title, version, artists, artistRefs, year, kind, trackCount, cover, coverLarge, explicit, saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumItem {
    pub album_id: String,
    pub uri: String,
    pub title: String,
    pub version: String,
    pub artists: String,
    pub artist_refs: Vec<ArtistRef>,
    /// первые 4 символа release_date
    pub year: i64,
    /// album_kind_label
    pub kind: String,
    pub track_count: i64,
    pub cover: String,
    pub cover_large: String,
    pub explicit: bool,
    /// в библиотеке пользователя (если известно)
    pub saved: bool,
}

/// ARTIST_KEYS: artistId, uri, name, cover, coverLarge, genres, followers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistItem {
    pub artist_id: String,
    pub uri: String,
    pub name: String,
    pub cover: String,
    pub cover_large: String,
    /// жанры через ", "
    pub genres: String,
    pub followers: i64,
}

/// PLAYLIST_KEYS: playlistId, uri, title, description, trackCount, cover, owner, ownerId, canEdit, collaborative,
/// public, snapshotId, pinned.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistItem {
    pub playlist_id: String,
    pub uri: String,
    pub title: String,
    pub description: String,
    pub track_count: i64,
    pub cover: String,
    /// имя владельца
    pub owner: String,
    pub owner_id: String,
    /// свой, совместный или по приглашению (права из rootlist librespot)
    pub can_edit: bool,
    pub collaborative: bool,
    pub public: bool,
    pub snapshot_id: String,
    /// закреплён локально
    pub pinned: bool,
}

/// AlbumData.info: поля AlbumItem + label, genre, durationMs, releaseDate, copyright, copyrights, courtesyLine.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumInfo {
    #[serde(flatten)]
    pub item: AlbumItem,
    pub label: String,
    /// часто пусто у Spotify
    pub genre: String,
    pub duration_ms: i64,
    /// "2001-03-12" | "2001-03" | "2001" — с той точностью, что знает Spotify
    pub release_date: String,
    pub copyright: String,
    /// все строки правообладателей с префиксом: "© 2001 …", "℗ 2001 …"
    pub copyrights: Vec<String>,
    /// строка «courtesy of …» (GraphQL-шлюз), часто пусто
    pub courtesy_line: String,
}

/// Всё, что известно о треке (окно «О треке»): поля TrackItem + номер, диск, ISRC, популярность и сведения
/// об альбоме (дата выпуска, лейбл, правообладатели).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackDetails {
    #[serde(flatten)]
    pub track: TrackItem,
    /// 0 — неизвестно
    pub track_number: i64,
    pub disc_number: i64,
    /// дисков в альбоме (0 — неизвестно)
    pub disc_count: i64,
    /// треков в альбоме (0 — неизвестно)
    pub album_track_count: i64,
    pub isrc: String,
    /// 0..100, -1 — неизвестно
    pub popularity: i64,
    /// album_kind_label альбома
    pub album_kind: String,
    pub release_date: String,
    pub label: String,
    pub copyrights: Vec<String>,
    pub courtesy_line: String,
}

/// ArtistData.info: поля ArtistItem + listeners, followed, biography, headerImage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistInfo {
    #[serde(flatten)]
    pub item: ArtistItem,
    /// ежемесячные слушатели (GraphQL-шлюз; Web API не отдаёт — 0)
    pub listeners: i64,
    /// пользователь подписан
    pub followed: bool,
    /// биография без HTML (GraphQL-шлюз; иначе пусто)
    pub biography: String,
    /// широкая шапка исполнителя (GraphQL-шлюз; иначе пусто)
    pub header_image: String,
}

/// SHOW_KEYS: showId, uri, name, publisher, description, cover, coverLarge, totalEpisodes, saved, explicit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowItem {
    pub show_id: String,
    pub uri: String,
    pub name: String,
    pub publisher: String,
    pub description: String,
    pub cover: String,
    pub cover_large: String,
    pub total_episodes: i64,
    pub saved: bool,
    pub explicit: bool,
}

/// EPISODE_KEYS: episodeId, uri, title, description, releaseDate, durationMs, explicit, resumeMs, fullyPlayed, saved,
/// cover, coverLarge, showId, showName, number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EpisodeItem {
    pub episode_id: String,
    pub uri: String,
    pub title: String,
    pub description: String,
    /// ISO-дата "2026-10-03"
    pub release_date: String,
    pub duration_ms: i64,
    pub explicit: bool,
    /// место, с которого продолжать (resume_point), 0 — сначала
    pub resume_ms: i64,
    pub fully_played: bool,
    pub saved: bool,
    pub cover: String,
    pub cover_large: String,
    pub show_id: String,
    pub show_name: String,
    /// номер выпуска, если известен (иначе 0)
    pub number: i64,
}

impl EpisodeItem {
    /// Как строка трека (очередь, плеер).
    pub fn to_track(&self) -> TrackItem {
        TrackItem {
            track_id: self.episode_id.clone(),
            album_id: String::new(),
            title: self.title.clone(),
            version: String::new(),
            artists: self.show_name.clone(),
            artist_refs: Vec::new(),
            album: self.show_name.clone(),
            cover: self.cover.clone(),
            cover_large: self.cover_large.clone(),
            duration_ms: self.duration_ms,
            explicit: self.explicit,
            available: true,
            uri: self.uri.clone(),
            is_episode: true,
            show_id: self.show_id.clone(),
            ..Default::default()
        }
    }
}

/// DEVICE_KEYS: id, name, type, isActive, isThisComputer, isRestricted, supportsVolume, volume, source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceItem {
    pub id: String,
    pub name: String,
    /// computer | smartphone | tablet | tv | speaker | avr | stb | automobile | gameconsole | castvideo | castaudio | …
    /// (Device.type Spotify в нижнем регистре)
    #[serde(rename = "type")]
    pub kind: String,
    pub is_active: bool,
    pub is_this_computer: bool,
    pub is_restricted: bool,
    pub supports_volume: bool,
    /// 0..100, -1 — неизвестно
    pub volume: i32,
    /// "api" — /me/player/devices, "zeroconf" — приёмник из локальной сети, ещё не в аккаунте
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueSection {
    /// сейчас играет
    #[default]
    Current,
    /// «Далее в очереди» — поставленные пользователем (Playing next)
    Manual,
    /// «Далее из контекста» (Next up)
    Context,
}

/// Строка Player.queue (QUEUE_KEYS): section, uid + все ключи трека.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueRow {
    pub section: QueueSection,
    /// уникален у каждого вхождения (дубликаты разрешены); стабилен, пока строка в очереди
    pub uid: u64,
    #[serde(flatten)]
    pub track: TrackItem,
}

/// Элемент библиотеки (LIBRARY_KEYS): kind, id, uri, title, subtitle, cover, covers, owner, ownerId, canEdit,
/// readOnly, pinned, trackCount, addedAt, playedAt, folderId.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryEntry {
    /// liked | playlist | dj | folder | album | artist | show (| radio — только в полках главной)
    pub kind: String,
    pub id: String,
    pub uri: String,
    pub title: String,
    /// «Плейлист · Вы · 34 трека» — собирает сервис
    pub subtitle: String,
    pub cover: String,
    /// до 4 обложек (мозаика папки)
    pub covers: Vec<String>,
    pub owner: String,
    pub owner_id: String,
    pub can_edit: bool,
    /// папка или чужой плейлист
    pub read_only: bool,
    pub pinned: bool,
    pub track_count: i64,
    /// ISO-время добавления в библиотеку, "" — неизвестно
    pub added_at: String,
    /// ISO-время последнего прослушивания (локальная история), ""
    pub played_at: String,
    /// id родительской папки rootlist, "" — корень
    pub folder_id: String,
}

/// Аккаунт для Auth.account: id, uri, displayName, email, country, product, premium, avatar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    pub id: String,
    pub uri: String,
    pub display_name: String,
    pub email: String,
    pub country: String,
    /// "premium" | "free" | "open" | …
    pub product: String,
    /// product == "premium"
    pub premium: bool,
    pub avatar: String,
}

/// Сид радио на орбите главной: uri (spotify:track|artist|album|playlist:ID), label, cover, kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioSeed {
    pub uri: String,
    pub label: String,
    pub cover: String,
    /// track | artist | album | playlist
    pub kind: String,
}

/// Группа сидов: key (recent | artists | collections | any), title, items.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioSeedGroup {
    pub key: String,
    pub title: String,
    pub items: Vec<RadioSeed>,
}

/// Полка главной: key (quick | recent | madeForYou | podcasts | topArtists | topTracks), title,
/// kind (tiles | tracks | episodes | artists), items — строки соответствующих *Item / LibraryEntry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeShelf {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub items: Vec<serde_json::Value>,
}

/// Счётчики поиска: tracks, albums, artists, playlists, shows, episodes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchTotals {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
    pub playlists: i64,
    pub shows: i64,
    pub episodes: i64,
}

/// Лучший результат: {type: track|album|artist|playlist|show, item: {…}} (эвристика, у Spotify нет).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchBest {
    #[serde(rename = "type")]
    pub kind: String,
    pub item: serde_json::Map<String, serde_json::Value>,
}
