//! Модели Web API → нейтральные элементы md3-model для QML.
//! Размеры обложек (how-it-connects.md): сетки/строки — ~300 px, шапки — 640 px.
use md3_model as m;

use crate::models;
use crate::util::{strip_html, uri_id, uri_kind};

pub const COVER_PX: u32 = 300;
pub const COVER_LARGE_PX: u32 = 640;

/// URL обложки ближайшего размера, "" — нет.
pub fn cover(images: &[models::Image], px: u32) -> String {
    models::pick_image(images, px).unwrap_or_default().to_owned()
}

/// id из поля или из URI нужного вида.
fn id_or_uri(id: Option<&str>, uri: &str, kind: &str) -> String {
    id.filter(|id| !id.is_empty())
        .or_else(|| (uri_kind(uri) == Some(kind)).then(|| uri_id(uri)).flatten())
        .unwrap_or_default()
        .to_owned()
}

fn uri_or(uri: &str, kind: &str, id: &str) -> String {
    if !uri.is_empty() {
        uri.to_owned()
    } else if id.is_empty() {
        String::new()
    } else {
        format!("spotify:{kind}:{id}")
    }
}

/// «2024-03-15» → 2024; нет даты или мусор → 0.
fn year(date: Option<&str>) -> i64 {
    date.and_then(|date| date.get(..4)).and_then(|year| year.parse().ok()).unwrap_or(0)
}

fn artist_refs(artists: &[models::ArtistRef]) -> Vec<m::ArtistRef> {
    artists
        .iter()
        .map(|artist| m::ArtistRef {
            id: id_or_uri(artist.id.as_deref(), artist.uri.as_deref().unwrap_or_default(), "artist"),
            name: artist.name.clone(),
        })
        .collect()
}

fn artist_names(artists: &[models::ArtistRef]) -> String {
    models::join_names(artists.iter().map(|artist| artist.name.as_str()))
}

pub fn track_item(track: &models::Track) -> m::TrackItem {
    let track_id = id_or_uri(track.id.as_deref(), &track.uri, "track");
    let album = track.album.as_ref();
    let images = album.map(|album| album.images.as_slice()).unwrap_or_default();
    m::TrackItem {
        uri: uri_or(&track.uri, "track", &track_id),
        track_id,
        album_id: album.map(|album| id_or_uri(Some(&album.id), &album.uri, "album")).unwrap_or_default(),
        title: track.name.clone(),
        version: String::new(),
        artists: track.artist_names(),
        artist_refs: artist_refs(&track.artists),
        album: album.map(|album| album.name.clone()).unwrap_or_default(),
        cover: cover(images, COVER_PX),
        cover_large: cover(images, COVER_LARGE_PX),
        duration_ms: i64::from(track.duration_ms),
        explicit: track.explicit,
        available: track.is_playable != Some(false),
        is_episode: false,
        show_id: String::new(),
        added_at: String::new(),
        added_by: String::new(),
        play_count: 0,
    }
}

/// Выпуск как строка трека (isEpisode = true).
pub fn episode_track(episode: &models::Episode) -> m::TrackItem {
    let show = episode.show.as_ref();
    let show_name = show.map(|show| show.name.clone()).unwrap_or_default();
    let episode_id = id_or_uri(Some(&episode.id), &episode.uri, "episode");
    let (cover_small, cover_large) = episode_covers(episode, show);
    m::TrackItem {
        uri: uri_or(&episode.uri, "episode", &episode_id),
        track_id: episode_id,
        album_id: String::new(),
        title: episode.name.clone(),
        version: String::new(),
        artists: show.map(|show| show.publisher.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| show_name.clone()),
        artist_refs: Vec::new(),
        album: show_name,
        cover: cover_small,
        cover_large,
        duration_ms: i64::from(episode.duration_ms),
        explicit: episode.explicit,
        available: true,
        is_episode: true,
        show_id: show.map(|show| id_or_uri(Some(&show.id), &show.uri, "show")).unwrap_or_default(),
        added_at: String::new(),
        added_by: String::new(),
        play_count: 0,
    }
}

/// Обложка выпуска, иначе — шоу.
fn episode_covers(episode: &models::Episode, show: Option<&models::Show>) -> (String, String) {
    let images = if episode.images.is_empty() {
        show.map(|show| show.images.as_slice()).unwrap_or_default()
    } else {
        episode.images.as_slice()
    };
    (cover(images, COVER_PX), cover(images, COVER_LARGE_PX))
}

/// Элемент плейлиста/очереди/истории: трек или выпуск; локальные файлы → None.
pub fn playable_item(item: &models::PlayableItem) -> Option<m::TrackItem> {
    match item {
        models::PlayableItem::Track(track) if track.is_local || uri_kind(&track.uri) == Some("local") => None,
        models::PlayableItem::Track(track) => Some(track_item(track)),
        models::PlayableItem::Episode(episode) => Some(episode_track(episode)),
    }
}

/// Строка плейлиста с addedAt/addedBy (у Web API в added_by только id пользователя).
pub fn playlist_entry(entry: &models::PlaylistItem) -> Option<m::TrackItem> {
    if entry.is_local {
        return None;
    }
    let mut item = playable_item(entry.playable()?)?;
    item.added_at = entry.added_at.clone().unwrap_or_default();
    item.added_by = entry.added_by.as_ref().and_then(|user| user.id.clone()).unwrap_or_default();
    Some(item)
}

/// «Любимые»: трек с временем добавления.
pub fn saved_track(saved: &models::SavedTrack) -> m::TrackItem {
    let mut item = track_item(&saved.track);
    item.added_at = saved.added_at.clone().unwrap_or_default();
    item
}

fn album_kind(album: &models::Album) -> String {
    let kind = album.album_type.as_deref().or(album.album_group.as_deref()).unwrap_or("album");
    m::album_kind_label(kind, i64::from(album.total_tracks.unwrap_or(0)))
}

pub fn album_item(album: &models::Album) -> m::AlbumItem {
    let album_id = id_or_uri(Some(&album.id), &album.uri, "album");
    let explicit = album.tracks.as_ref().is_some_and(|page| page.items.iter().any(|track| track.explicit));
    let track_count = album
        .total_tracks
        .map(i64::from)
        .or_else(|| album.tracks.as_ref().map(|page| i64::from(page.total.max(page.items.len() as u32))))
        .unwrap_or(0);
    m::AlbumItem {
        uri: uri_or(&album.uri, "album", &album_id),
        album_id,
        title: album.name.clone(),
        version: String::new(),
        artists: artist_names(&album.artists),
        artist_refs: artist_refs(&album.artists),
        year: year(album.release_date.as_deref()),
        kind: album_kind(album),
        track_count,
        cover: cover(&album.images, COVER_PX),
        cover_large: cover(&album.images, COVER_LARGE_PX),
        explicit,
        saved: false,
    }
}

/// AlbumData.info: duration — сумма треков (`tracks`, иначе вложенная страница альбома).
pub fn album_info(album: &models::Album, tracks: &[models::Track]) -> m::AlbumInfo {
    let nested = album.tracks.as_ref().map(|page| page.items.as_slice()).unwrap_or_default();
    let tracks = if tracks.is_empty() { nested } else { tracks };
    let mut item = album_item(album);
    item.explicit |= tracks.iter().any(|track| track.explicit);
    if item.track_count == 0 {
        item.track_count = tracks.len() as i64;
    }
    let copyright = album
        .copyrights
        .iter()
        .find(|copyright| copyright.kind == "C" && !copyright.text.is_empty())
        .or_else(|| album.copyrights.iter().find(|copyright| !copyright.text.is_empty()))
        .map(|copyright| copyright.text.clone())
        .unwrap_or_default();
    m::AlbumInfo {
        item,
        label: album.label.clone().unwrap_or_default(),
        genre: album.genres.join(", "),
        duration_ms: tracks.iter().map(|track| i64::from(track.duration_ms)).sum(),
        release_date: album.release_date.clone().unwrap_or_default(),
        copyright,
        copyrights: copyright_lines(album.copyrights.iter().map(|copyright| (copyright.kind.as_str(), copyright.text.as_str()))),
        courtesy_line: String::new(),
    }
}

/// Строка правообладателя с префиксом по типу: C → «© …», P → «℗ …». Свои префиксы текста («(P) 2013 …»,
/// «℗ 2013 …», «© …») снимаются, чтобы знак не повторялся; пустой текст — пустая строка.
pub fn copyright_line(kind: &str, text: &str) -> String {
    let mut rest = text.trim();
    loop {
        let stripped = ["©", "℗", "(C)", "(c)", "(P)", "(p)"].iter().find_map(|mark| rest.strip_prefix(mark));
        match stripped {
            Some(after) => rest = after.trim_start(),
            None => break,
        }
    }
    if rest.is_empty() {
        return String::new();
    }
    let mark = match kind {
        "C" | "c" => "©",
        "P" | "p" => "℗",
        _ => return rest.to_owned(),
    };
    format!("{mark} {rest}")
}

/// Строки правообладателей (тип, текст) — без пустых и повторов, © перед ℗.
pub fn copyright_lines<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut lines: Vec<(u8, String)> = Vec::new();
    for (kind, text) in items {
        let line = copyright_line(kind, text);
        if !line.is_empty() && !lines.iter().any(|(_, known)| *known == line) {
            let order = match kind {
                "C" | "c" => 0,
                "P" | "p" => 1,
                _ => 2,
            };
            lines.push((order, line));
        }
    }
    lines.sort_by_key(|(order, _)| *order);
    lines.into_iter().map(|(_, line)| line).collect()
}

pub fn artist_item(artist: &models::Artist) -> m::ArtistItem {
    let artist_id = id_or_uri(Some(&artist.id), &artist.uri, "artist");
    m::ArtistItem {
        uri: uri_or(&artist.uri, "artist", &artist_id),
        artist_id,
        name: artist.name.clone(),
        cover: cover(&artist.images, COVER_PX),
        cover_large: cover(&artist.images, COVER_LARGE_PX),
        genres: artist.genres.join(", "),
        followers: artist.followers.as_ref().map_or(0, |followers| followers.total as i64),
    }
}

pub fn artist_info(artist: &models::Artist, followed: bool) -> m::ArtistInfo {
    m::ArtistInfo { item: artist_item(artist), listeners: 0, followed, ..Default::default() }
}

/// `me` — id аккаунта (для ownerId/canEdit; права из rootlist уточняет LibraryService).
pub fn playlist_item(playlist: &models::Playlist, me: Option<&m::AccountId>) -> m::PlaylistItem {
    let playlist_id = id_or_uri(Some(&playlist.id), &playlist.uri, "playlist");
    let owned = me.is_some_and(|me| playlist.owned_by(me.as_str()));
    m::PlaylistItem {
        uri: uri_or(&playlist.uri, "playlist", &playlist_id),
        playlist_id,
        title: playlist.name.clone(),
        description: playlist.description.as_deref().map(strip_html).unwrap_or_default(),
        track_count: i64::from(playlist.track_total()),
        cover: cover(&playlist.images, COVER_PX),
        owner: playlist.owner_name().to_owned(),
        owner_id: playlist.owner.id.clone().unwrap_or_default(),
        can_edit: owned || playlist.collaborative,
        collaborative: playlist.collaborative,
        blend: playlist.blend,
        public: playlist.public.unwrap_or(false),
        snapshot_id: playlist.snapshot_id.clone().unwrap_or_default(),
        pinned: false,
    }
}

pub fn show_item(show: &models::Show) -> m::ShowItem {
    let show_id = id_or_uri(Some(&show.id), &show.uri, "show");
    let total = show
        .total_episodes
        .or_else(|| show.episodes.as_ref().map(|page| page.total))
        .unwrap_or(0);
    m::ShowItem {
        uri: uri_or(&show.uri, "show", &show_id),
        show_id,
        name: show.name.clone(),
        publisher: show.publisher.clone(),
        description: strip_html(&show.description),
        cover: cover(&show.images, COVER_PX),
        cover_large: cover(&show.images, COVER_LARGE_PX),
        total_episodes: i64::from(total),
        saved: false,
        explicit: show.explicit,
    }
}

/// `show` — когда выпуск пришёл без вложенного шоу (страница шоу).
pub fn episode_item(episode: &models::Episode, show: Option<&models::Show>) -> m::EpisodeItem {
    let show = episode.show.as_ref().or(show);
    let episode_id = id_or_uri(Some(&episode.id), &episode.uri, "episode");
    let (cover_small, cover_large) = episode_covers(episode, show);
    m::EpisodeItem {
        uri: uri_or(&episode.uri, "episode", &episode_id),
        episode_id,
        title: episode.name.clone(),
        description: strip_html(&episode.description),
        release_date: episode.release_date.clone().unwrap_or_default(),
        duration_ms: i64::from(episode.duration_ms),
        explicit: episode.explicit,
        resume_ms: episode.resume_ms().map_or(0, i64::from),
        fully_played: episode.resume_point.as_ref().is_some_and(|point| point.fully_played),
        saved: false,
        cover: cover_small,
        cover_large,
        show_id: show.map(|show| id_or_uri(Some(&show.id), &show.uri, "show")).unwrap_or_default(),
        show_name: show.map(|show| show.name.clone()).unwrap_or_default(),
        number: 0,
    }
}

/// `this_device_id` — id Connect-устройства этого ПК (isThisComputer).
pub fn device_item(device: &models::Device, this_device_id: Option<&str>) -> m::DeviceItem {
    let id = device.id.clone().unwrap_or_default();
    m::DeviceItem {
        is_this_computer: !id.is_empty() && this_device_id == Some(id.as_str()),
        id,
        name: device.name.clone(),
        kind: device.kind.to_lowercase(),
        is_active: device.is_active,
        is_restricted: device.is_restricted,
        supports_volume: device.supports_volume.unwrap_or(device.volume_percent.is_some()),
        volume: device.volume_percent.map_or(-1, |volume| i32::from(volume.min(100))),
        source: "api".into(),
    }
}

pub fn account(user: &models::User) -> m::Account {
    let product = user.product.clone().unwrap_or_default();
    m::Account {
        uri: user.uri.clone().unwrap_or_else(|| uri_or("", "user", &user.id)),
        id: user.id.clone(),
        display_name: user.name().to_owned(),
        email: String::new(),
        country: user.country.clone().unwrap_or_default(),
        premium: product == "premium",
        product,
        avatar: user.images.first().map(|image| image.url.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn copyright_lines_get_one_mark_each() {
        assert_eq!(super::copyright_line("C", "© 2001 Daft Life Limited"), "© 2001 Daft Life Limited");
        assert_eq!(super::copyright_line("P", "(P) 2013 Daft Life Limited"), "℗ 2013 Daft Life Limited");
        assert_eq!(super::copyright_line("P", "2013 Columbia"), "℗ 2013 Columbia");
        assert_eq!(super::copyright_line("C", "  "), "");
        let lines = super::copyright_lines([("P", "℗ 2001 Daft Life"), ("C", "2001 Daft Life"), ("P", "(P) 2001 Daft Life")]);
        assert_eq!(lines, ["© 2001 Daft Life", "℗ 2001 Daft Life"]);
    }

    use super::*;

    fn images() -> Vec<models::Image> {
        [(640, "large"), (300, "medium"), (64, "small")]
            .into_iter()
            .map(|(width, url)| models::Image { url: url.into(), width: Some(width), height: Some(width) })
            .collect()
    }

    #[test]
    fn track_maps_covers_artists_and_availability() {
        let json = r#"{"id":"t","name":"Song","uri":"spotify:track:t","duration_ms":1000,"explicit":true,
            "is_playable":false,"artists":[{"id":"a","name":"Tyler, the Creator"},{"uri":"spotify:artist:b","name":"B"}],
            "album":{"id":"al","name":"Album","uri":"spotify:album:al","images":[
                {"url":"large","width":640},{"url":"medium","width":300},{"url":"small","width":64}]}}"#;
        let track: models::Track = serde_json::from_str(json).unwrap();
        let item = track_item(&track);
        assert_eq!(item.track_id, "t");
        assert_eq!(item.album_id, "al");
        assert_eq!(item.artists, "Tyler, the Creator, B");
        assert_eq!(item.artist_refs[1], m::ArtistRef { id: "b".into(), name: "B".into() });
        assert_eq!((item.cover.as_str(), item.cover_large.as_str()), ("medium", "large"));
        assert_eq!(item.duration_ms, 1000);
        assert!(item.explicit && !item.available && !item.is_episode);
        assert_eq!(item.uri, "spotify:track:t");

        let unknown: models::Track = serde_json::from_str(r#"{"uri":"spotify:track:x"}"#).unwrap();
        let item = track_item(&unknown);
        assert!(item.available);
        assert_eq!(item.track_id, "x");
    }

    #[test]
    fn playlist_entries_skip_local_files_and_keep_who_added() {
        let json = r#"{"items":[
            {"added_at":"2024-01-01T00:00:00Z","added_by":{"id":"alice"},
             "item":{"type":"episode","id":"e","name":"Ep","uri":"spotify:episode:e","duration_ms":2000,
                     "show":{"id":"s","name":"Show","publisher":"Pub","images":[{"url":"show","width":300}]}}},
            {"is_local":true,"track":{"type":"track","uri":"spotify:local:a:b:c:1","name":"Local","is_local":true}}
        ],"total":2}"#;
        let page: models::Page<models::PlaylistItem> = serde_json::from_str(json).unwrap();
        let episode = playlist_entry(&page.items[0]).unwrap();
        assert!(episode.is_episode);
        assert_eq!(episode.show_id, "s");
        assert_eq!(episode.album, "Show");
        assert_eq!(episode.cover, "show");
        assert_eq!(episode.added_at, "2024-01-01T00:00:00Z");
        assert_eq!(episode.added_by, "alice");
        assert!(playlist_entry(&page.items[1]).is_none());
    }

    #[test]
    fn album_year_kind_and_duration() {
        let json = r#"{"id":"al","name":"EP","uri":"spotify:album:al","album_type":"single","total_tracks":4,
            "release_date":"2019-05","label":"Label","genres":["rock","pop"],
            "copyrights":[{"text":"(P) Label","type":"P"},{"text":"(C) Label","type":"C"}],
            "artists":[{"id":"a","name":"A"}],
            "tracks":{"items":[{"duration_ms":1000},null,{"duration_ms":2000,"explicit":true}],"total":3}}"#;
        let album: models::Album = serde_json::from_str(json).unwrap();
        let item = album_item(&album);
        assert_eq!(item.year, 2019);
        assert_eq!(item.kind, "EP");
        assert_eq!(item.track_count, 4);
        assert!(item.explicit);
        let info = album_info(&album, &[]);
        assert_eq!(info.duration_ms, 3000);
        assert_eq!(info.copyright, "(C) Label");
        assert_eq!(info.genre, "rock, pop");
        assert_eq!(info.release_date, "2019-05");
        assert_eq!(info.label, "Label");

        let single: models::Album =
            serde_json::from_str(r#"{"id":"s","album_type":"single","total_tracks":1,"release_date":"x"}"#).unwrap();
        let item = album_item(&single);
        assert_eq!((item.kind.as_str(), item.year), ("Single", 0));
        assert_eq!(item.uri, "spotify:album:s");
    }

    #[test]
    fn playlist_ownership_and_editing() {
        let json = r#"{"id":"p","name":"P","uri":"spotify:playlist:p","description":"Hi &amp; <b>you</b>",
            "owner":{"id":"me","display_name":"Me"},"public":true,"snapshot_id":"snap","items":{"total":7},
            "images":[{"url":"cover","width":300}]}"#;
        let mut playlist: models::Playlist = serde_json::from_str(json).unwrap();
        let me = m::AccountId::new("me");
        let item = playlist_item(&playlist, Some(&me));
        assert_eq!(item.playlist_id, "p");
        assert_eq!(item.description, "Hi & you");
        assert_eq!((item.owner.as_str(), item.owner_id.as_str()), ("Me", "me"));
        assert!(item.can_edit && item.public);
        assert_eq!(item.track_count, 7);
        assert_eq!(item.snapshot_id, "snap");
        assert_eq!(item.cover, "cover");
        assert!(!playlist_item(&playlist, Some(&m::AccountId::new("other"))).can_edit);
        assert!(!playlist_item(&playlist, None).can_edit);
        playlist.collaborative = true;
        assert!(playlist_item(&playlist, Some(&m::AccountId::new("other"))).can_edit);
    }

    #[test]
    fn episodes_and_shows() {
        let show = models::Show {
            id: "s".into(),
            name: "Show".into(),
            publisher: "Pub".into(),
            images: images(),
            total_episodes: Some(12),
            ..Default::default()
        };
        let episode = models::Episode {
            id: "e".into(),
            name: "Ep".into(),
            duration_ms: 60_000,
            release_date: Some("2026-10-03".into()),
            resume_point: Some(models::ResumePoint { fully_played: false, resume_position_ms: 30_000 }),
            ..Default::default()
        };
        let item = episode_item(&episode, Some(&show));
        assert_eq!(item.uri, "spotify:episode:e");
        assert_eq!(item.resume_ms, 30_000);
        assert!(!item.fully_played);
        assert_eq!((item.show_id.as_str(), item.show_name.as_str()), ("s", "Show"));
        assert_eq!((item.cover.as_str(), item.cover_large.as_str()), ("medium", "large"));
        assert_eq!(item.release_date, "2026-10-03");
        let shown = show_item(&show);
        assert_eq!(shown.total_episodes, 12);
        assert_eq!(shown.uri, "spotify:show:s");
        let track = episode_track(&models::Episode { show: Some(show), ..episode });
        assert!(track.is_episode);
        assert_eq!(track.artists, "Pub");
        assert_eq!(track.play_uri(), "spotify:episode:e");
    }

    #[test]
    fn devices_and_account() {
        let device: models::Device = serde_json::from_str(
            r#"{"id":"d1","name":"Kitchen","type":"Speaker","is_active":true,"supports_volume":true,"volume_percent":40}"#,
        )
        .unwrap();
        let item = device_item(&device, Some("d1"));
        assert_eq!(item.kind, "speaker");
        assert!(item.is_active && item.is_this_computer && item.supports_volume);
        assert_eq!(item.volume, 40);
        assert_eq!(item.source, "api");
        let silent: models::Device = serde_json::from_str(r#"{"id":"d2","name":"TV","type":"TV"}"#).unwrap();
        let item = device_item(&silent, Some("d1"));
        assert_eq!(item.volume, -1);
        assert!(!item.is_this_computer && !item.supports_volume);
        assert_eq!(item.kind, "tv");

        let user: models::User = serde_json::from_str(
            r#"{"id":"u","display_name":"User","product":"premium","country":"DE","images":[{"url":"avatar"}]}"#,
        )
        .unwrap();
        let account = account(&user);
        assert!(account.premium);
        assert_eq!(account.avatar, "avatar");
        assert_eq!(account.uri, "spotify:user:u");
        assert_eq!(account.display_name, "User");
        let free: models::User = serde_json::from_str(r#"{"id":"f","product":"free"}"#).unwrap();
        assert!(!super::account(&free).premium);
        assert_eq!(super::account(&free).display_name, "f");
    }

    #[test]
    fn artists() {
        let artist: models::Artist = serde_json::from_str(
            r#"{"id":"a","name":"A","genres":["indie","rock"],"followers":{"total":1234},"images":[{"url":"x","width":640}]}"#,
        )
        .unwrap();
        let info = artist_info(&artist, true);
        assert_eq!(info.item.genres, "indie, rock");
        assert_eq!(info.item.followers, 1234);
        assert_eq!(info.item.uri, "spotify:artist:a");
        assert_eq!(info.item.cover, "x");
        assert!(info.followed);
        assert_eq!(info.listeners, 0);
    }
}
