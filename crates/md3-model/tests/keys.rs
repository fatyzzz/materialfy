//! Ключи сериализации = роли моделей и ключи словарей QML. Менять только вместе с QML.
use md3_model::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TRACK_KEYS: &[&str] = &[
    "trackId", "albumId", "title", "version", "artists", "artistRefs", "album", "cover", "coverLarge", "durationMs",
    "explicit", "available", "uri", "isEpisode", "showId", "addedAt", "addedBy", "playCount",
];
const ALBUM_KEYS: &[&str] = &[
    "albumId", "uri", "title", "version", "artists", "artistRefs", "year", "kind", "trackCount", "cover", "coverLarge",
    "explicit", "saved",
];
const ARTIST_KEYS: &[&str] = &["artistId", "uri", "name", "cover", "coverLarge", "genres", "followers"];
const PLAYLIST_KEYS: &[&str] = &[
    "playlistId", "uri", "title", "description", "trackCount", "cover", "owner", "ownerId", "canEdit", "collaborative",
    "blend", "public", "snapshotId", "pinned",
];
const SHOW_KEYS: &[&str] =
    &["showId", "uri", "name", "publisher", "description", "cover", "coverLarge", "totalEpisodes", "saved", "explicit"];
const EPISODE_KEYS: &[&str] = &[
    "episodeId", "uri", "title", "description", "releaseDate", "durationMs", "explicit", "resumeMs", "fullyPlayed",
    "saved", "cover", "coverLarge", "showId", "showName", "number",
];
const DEVICE_KEYS: &[&str] =
    &["id", "name", "type", "isActive", "isThisComputer", "isRestricted", "supportsVolume", "volume", "source"];
const LIBRARY_KEYS: &[&str] = &[
    "kind", "id", "uri", "title", "subtitle", "cover", "covers", "owner", "ownerId", "canEdit", "readOnly", "pinned",
    "trackCount", "addedAt", "playedAt", "folderId",
];

fn keys<T: Serialize>(value: &T) -> Vec<String> {
    let mut keys: Vec<String> = match serde_json::to_value(value).unwrap() {
        Value::Object(map) => map.keys().cloned().collect(),
        other => panic!("not an object: {other}"),
    };
    keys.sort();
    keys
}

fn expected(sets: &[&[&str]]) -> Vec<String> {
    let mut keys: Vec<String> = sets.iter().flat_map(|s| s.iter().map(|k| (*k).to_owned())).collect();
    keys.sort();
    let before = keys.len();
    keys.dedup();
    assert_eq!(before, keys.len(), "keys repeat");
    keys
}

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) {
    let text = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, value, "{text}");
}

fn track() -> TrackItem {
    TrackItem {
        track_id: "4uLU6hMCjMI75M1A2tKUQC".into(),
        album_id: "6N9PS4QXF1D0OWPk0Sxtb4".into(),
        title: "Never Gonna Give You Up".into(),
        artists: "Rick Astley".into(),
        artist_refs: vec![ArtistRef { id: "0gxyHStUsqpMadRV0Di1Qt".into(), name: "Rick Astley".into() }],
        album: "Whenever You Need Somebody".into(),
        cover: "https://i.scdn.co/image/300".into(),
        cover_large: "https://i.scdn.co/image/640".into(),
        duration_ms: 213_573,
        available: true,
        uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
        added_at: "2026-10-01T10:00:00Z".into(),
        added_by: "Друг".into(),
        ..Default::default()
    }
}

#[test]
fn track_keys() {
    assert_eq!(keys(&TrackItem::default()), expected(&[TRACK_KEYS]));
    let value = serde_json::to_value(track()).unwrap();
    assert_eq!(value["artistRefs"], json!([{"id": "0gxyHStUsqpMadRV0Di1Qt", "name": "Rick Astley"}]));
    assert_eq!(value["durationMs"], json!(213_573));
    assert_eq!(value["version"], json!(""));
    roundtrip(&track());
}

#[test]
fn track_defaults_when_keys_missing() {
    // QML может прислать неполный словарь (track_from_map) — недостающее берётся по умолчанию
    let t: TrackItem = serde_json::from_value(json!({"trackId": "x", "title": "Т"})).unwrap();
    assert_eq!(t.track_id, "x");
    assert_eq!(t.play_uri(), "spotify:track:x");
    let e: TrackItem = serde_json::from_value(json!({"trackId": "e", "isEpisode": true})).unwrap();
    assert_eq!(e.play_uri(), "spotify:episode:e");
    assert_eq!(track().play_uri(), "spotify:track:4uLU6hMCjMI75M1A2tKUQC");
}

#[test]
fn album_artist_playlist_keys() {
    assert_eq!(keys(&AlbumItem::default()), expected(&[ALBUM_KEYS]));
    assert_eq!(keys(&ArtistItem::default()), expected(&[ARTIST_KEYS]));
    assert_eq!(keys(&PlaylistItem::default()), expected(&[PLAYLIST_KEYS]));
    roundtrip(&AlbumItem { album_id: "a".into(), year: 1987, kind: album_kind_label("album", 10), ..Default::default() });
    roundtrip(&PlaylistItem { playlist_id: "p".into(), can_edit: true, public: true, ..Default::default() });
}

#[test]
fn info_types_are_flat() {
    assert_eq!(
        keys(&AlbumInfo::default()),
        expected(&[ALBUM_KEYS, &["label", "genre", "durationMs", "releaseDate", "copyright", "copyrights", "courtesyLine"]])
    );
    assert_eq!(keys(&ArtistInfo::default()), expected(&[ARTIST_KEYS, &["listeners", "followed", "biography", "headerImage"]]));
    let info = AlbumInfo {
        item: AlbumItem { album_id: "a1".into(), title: "Альбом".into(), track_count: 12, ..Default::default() },
        label: "RCA".into(),
        duration_ms: 2_400_000,
        release_date: "1987-11-12".into(),
        ..Default::default()
    };
    let value = serde_json::to_value(&info).unwrap();
    assert_eq!(value["albumId"], json!("a1"));
    assert_eq!(value["trackCount"], json!(12));
    assert_eq!(value["label"], json!("RCA"));
    roundtrip(&info);
    roundtrip(&ArtistInfo { item: ArtistItem { artist_id: "r".into(), ..Default::default() }, followed: true, listeners: 0, ..Default::default() });
}

#[test]
fn show_episode_keys() {
    assert_eq!(keys(&ShowItem::default()), expected(&[SHOW_KEYS]));
    assert_eq!(keys(&EpisodeItem::default()), expected(&[EPISODE_KEYS]));
    let ep = EpisodeItem {
        episode_id: "ep1".into(),
        uri: "spotify:episode:ep1".into(),
        title: "Выпуск 12".into(),
        duration_ms: 3_600_000,
        resume_ms: 120_000,
        show_id: "sh1".into(),
        show_name: "Подкаст".into(),
        cover: "c".into(),
        ..Default::default()
    };
    roundtrip(&ep);
    let t = ep.to_track();
    assert!(t.is_episode);
    assert_eq!(t.track_id, "ep1");
    assert_eq!(t.album, "Подкаст");
    assert_eq!(t.artists, "Подкаст");
    assert_eq!(t.show_id, "sh1");
    assert_eq!(t.play_uri(), "spotify:episode:ep1");
}

#[test]
fn device_keys() {
    assert_eq!(keys(&DeviceItem::default()), expected(&[DEVICE_KEYS]));
    let dev = DeviceItem {
        id: "d1".into(),
        name: "Кухня".into(),
        kind: "speaker".into(),
        is_active: true,
        supports_volume: true,
        volume: 40,
        source: "api".into(),
        ..Default::default()
    };
    let value = serde_json::to_value(&dev).unwrap();
    assert_eq!(value["type"], json!("speaker"));
    assert_eq!(value["volume"], json!(40));
    roundtrip(&dev);
}

#[test]
fn queue_row_is_flat_track() {
    assert_eq!(keys(&QueueRow::default()), expected(&[&["section", "uid"], TRACK_KEYS]));
    let row = QueueRow { section: QueueSection::Manual, uid: 7, track: track() };
    let value = serde_json::to_value(&row).unwrap();
    assert_eq!(value["section"], json!("manual"));
    assert_eq!(value["uid"], json!(7));
    assert_eq!(value["trackId"], json!("4uLU6hMCjMI75M1A2tKUQC"));
    roundtrip(&row);
    for (section, name) in
        [(QueueSection::Current, "current"), (QueueSection::Manual, "manual"), (QueueSection::Context, "context")]
    {
        assert_eq!(serde_json::to_value(section).unwrap(), json!(name));
    }
}

#[test]
fn library_account_keys() {
    assert_eq!(keys(&LibraryEntry::default()), expected(&[LIBRARY_KEYS]));
    roundtrip(&LibraryEntry {
        kind: "folder".into(),
        id: "f".into(),
        covers: vec!["a".into(), "b".into()],
        read_only: true,
        ..Default::default()
    });
    assert_eq!(
        keys(&Account::default()),
        expected(&[&["id", "uri", "displayName", "email", "country", "product", "premium", "avatar"]])
    );
}

#[test]
fn radio_home_search_keys() {
    assert_eq!(keys(&RadioSeed::default()), expected(&[&["uri", "label", "cover", "kind"]]));
    assert_eq!(keys(&RadioSeedGroup::default()), expected(&[&["key", "title", "items"]]));
    assert_eq!(keys(&HomeShelf::default()), expected(&[&["key", "title", "kind", "items"]]));
    assert_eq!(
        keys(&SearchTotals::default()),
        expected(&[&["tracks", "albums", "artists", "playlists", "shows", "episodes"]])
    );
    assert_eq!(keys(&SearchBest::default()), expected(&[&["type", "item"]]));
    let shelf = HomeShelf {
        key: "topTracks".into(),
        title: "Ваши любимые".into(),
        kind: "tracks".into(),
        items: vec![serde_json::to_value(track()).unwrap()],
    };
    roundtrip(&shelf);
    let mut item = serde_json::Map::new();
    item.insert("artistId".into(), json!("r"));
    let best = SearchBest { kind: "artist".into(), item };
    assert_eq!(serde_json::to_value(&best).unwrap(), json!({"type": "artist", "item": {"artistId": "r"}}));
    roundtrip(&best);
}

#[test]
fn play_context() {
    // пустые uri/title/cover не пишутся
    assert_eq!(serde_json::to_value(PlayContext::new(contexts::LIKED)).unwrap(), json!({"type": "liked"}));
    let ctx = PlayContext::with_uri(contexts::PLAYLIST, "spotify:playlist:p1").titled("Мой плейлист");
    assert_eq!(
        serde_json::to_value(&ctx).unwrap(),
        json!({"type": "playlist", "uri": "spotify:playlist:p1", "title": "Мой плейлист"})
    );
    let covered = ctx.clone().with_cover("https://i.scdn.co/image/300");
    assert_eq!(serde_json::to_value(&covered).unwrap()["cover"], json!("https://i.scdn.co/image/300"));
    roundtrip(&ctx);
    roundtrip(&covered);
    // неполный словарь из QML
    let parsed: PlayContext = serde_json::from_value(json!({"type": "album"})).unwrap();
    assert_eq!(parsed, PlayContext::new(contexts::ALBUM));
    let parsed: PlayContext = serde_json::from_value(json!({})).unwrap();
    assert_eq!(parsed.kind, contexts::UNKNOWN);
    assert_eq!(contexts::liked_uri("user1"), "spotify:user:user1:collection");
}

#[test]
fn ids_are_plain_strings() {
    let id = PlaylistId::new("37i9dQZF1DXcBWIGoYBM5M");
    assert_eq!(serde_json::to_value(&id).unwrap(), json!("37i9dQZF1DXcBWIGoYBM5M"));
    assert_eq!(serde_json::from_value::<PlaylistId>(json!("x")).unwrap(), PlaylistId::new("x"));
    assert_eq!(id.uri(), "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M");
    assert_eq!(PlaylistId::from_uri_or_id("spotify:playlist:abc"), PlaylistId::new("abc"));
    assert_eq!(PlaylistId::from_uri_or_id("abc"), PlaylistId::new("abc"));
    assert_eq!(PlaylistId::from_uri_or_id("https://open.spotify.com/playlist/abc?si=1"), PlaylistId::new("abc"));
    assert_eq!(id.to_string(), id.as_str());
    let account = AccountId::new("user.name");
    assert_eq!(serde_json::to_value(&account).unwrap(), json!("user.name"));
    assert_eq!(account.as_str(), "user.name");
}

#[test]
fn list_change_shapes() {
    let change: ListChange<i32> = ListChange::Move { from: 1, to: 3 };
    assert_eq!(change, ListChange::Move { from: 1, to: 3 });
    assert_ne!(ListChange::<i32>::Loading(true), ListChange::Loading(false));
}

#[test]
fn album_kinds() {
    assert_eq!(album_kind_label("album", 10), "Album");
    assert_eq!(album_kind_label("single", 1), "Single");
    assert_eq!(album_kind_label("single", 4), "EP");
    assert_eq!(album_kind_label("compilation", 20), "Compilation");
    assert_eq!(album_kind_label("", 0), "Album");
}
