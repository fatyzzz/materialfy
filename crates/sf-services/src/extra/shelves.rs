//! Состав полок главной (spotifast ui/home.rs + App::load_home): чистые функции без сети.
use std::collections::HashSet;

use md3_model::{EpisodeItem, HomeShelf, LibraryEntry, PlayContext, PlaylistItem, ShowItem, TrackItem};
use serde::Serialize;
use sf_api::util::{station_seed, uri_id, uri_kind};

use crate::extra::seeds::station_title;
use crate::releases::NewRelease;

/// Поисковые термины «Сделано для вас» (reference model.rs DISCOVER_TERMS).
pub const DISCOVER_TERMS: &[&str] = &["Discover Weekly", "Release Radar", "Daily Mix", "daylist"];
pub const MADE_FOR_YOU_PER_TERM: usize = 6;
pub const QUICK_PLAYLISTS: usize = 5;
pub const RECENT_TILES: usize = 16;
pub const HOME_RECENTS: u32 = 50;
pub const HOME_PODCAST_SHOWS: usize = 8;
pub const EPISODES_PER_SHOW: u32 = 5;
pub const PODCAST_CARDS: usize = 16;
/// сколько дней невыслушанный выпуск считается новым
pub const NEW_EPISODE_DAYS: i64 = 30;
pub const TOP_LIMIT: u32 = 20;
/// плиток в полке «Новинки» (вместе с «Радаром»)
pub const NEW_RELEASE_TILES: usize = 16;

pub const QUICK: &str = "quick";
pub const RECENT: &str = "recent";
pub const MADE_FOR_YOU: &str = "madeForYou";
/// «Новинки»: «Радар новинок» + свежие релизы исполнителей (ReleasesService)
pub const NEW_RELEASES: &str = "newReleases";
pub const PODCASTS: &str = "podcasts";
pub const TOP_ARTISTS: &str = "topArtists";
pub const TOP_TRACKS: &str = "topTracks";

/// Порядок полок на главной.
pub const ORDER: &[&str] = &[QUICK, RECENT, MADE_FOR_YOU, NEW_RELEASES, PODCASTS, TOP_ARTISTS, TOP_TRACKS];

/// Английский исходник; в снимок полки — через mf_i18n::t (shelf).
pub fn title(key: &str) -> &'static str {
    match key {
        QUICK => "Quick access",
        RECENT => "Recently played",
        MADE_FOR_YOU => "Made for you",
        NEW_RELEASES => "New releases",
        PODCASTS => "Continue your podcasts",
        TOP_ARTISTS => "Top artists",
        TOP_TRACKS => "Top tracks",
        _ => "",
    }
}

/// kind полки: tiles | tracks | episodes | artists
pub fn kind(key: &str) -> &'static str {
    match key {
        PODCASTS => "episodes",
        TOP_ARTISTS => "artists",
        TOP_TRACKS => "tracks",
        _ => "tiles",
    }
}

/// Полка из элементов (сериализация serde = ключи QML).
pub fn shelf<T: Serialize>(key: &str, items: &[T]) -> HomeShelf {
    HomeShelf {
        key: key.to_owned(),
        title: mf_i18n::t(title(key)),
        kind: kind(key).to_owned(),
        items: items.iter().filter_map(|item| serde_json::to_value(item).ok()).collect(),
    }
}

/// Скрытые полки (SettingKey::HomeHidden: ключи через запятую).
pub fn hidden_keys(csv: &str) -> HashSet<String> {
    csv.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect()
}

/// Плейлист Spotify под этот термин: «Discover Weekly», «Daily Mix 3» и т. п.
pub fn is_made_for_you(name: &str, term: &str) -> bool {
    let name = name.trim().to_lowercase();
    let term = term.to_lowercase();
    if name == term {
        return true;
    }
    term == "daily mix"
        && name
            .strip_prefix("daily mix ")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Ответ поиска по термину → плейлисты Spotify (владелец spotify), без повторов имён, ≤ 6.
pub fn made_for_you(term: &str, playlists: Vec<PlaylistItem>) -> Vec<PlaylistItem> {
    let mut seen = HashSet::new();
    let mut matching: Vec<PlaylistItem> = playlists
        .into_iter()
        .filter(|p| {
            is_made_for_you(&p.title, term)
                && (p.owner_id == "spotify" || p.owner == "Spotify")
                && seen.insert(p.title.to_lowercase())
        })
        .collect();
    matching.truncate(MADE_FOR_YOU_PER_TERM);
    matching
}

pub fn playlist_entry(p: &PlaylistItem) -> LibraryEntry {
    // DJ — не плейлист: у него нет списка треков, он только запускается (как кнопка)
    let dj = p.uri == crate::data::home_feed::DJ_URI;
    LibraryEntry {
        kind: if dj { "dj".into() } else { "playlist".into() },
        id: p.playlist_id.clone(),
        uri: if p.uri.is_empty() { format!("spotify:playlist:{}", p.playlist_id) } else { p.uri.clone() },
        title: if dj { "DJ".into() } else { p.title.clone() },
        subtitle: if dj {
            mf_i18n::t("Music and commentary picked for you")
        } else if p.owner.is_empty() {
            mf_i18n::t("Playlist")
        } else {
            mf_i18n::tf("Playlist · {0}", &[&p.owner])
        },
        cover: p.cover.clone(),
        covers: if p.cover.is_empty() { Vec::new() } else { vec![p.cover.clone()] },
        owner: p.owner.clone(),
        owner_id: p.owner_id.clone(),
        can_edit: p.can_edit,
        read_only: !p.can_edit,
        pinned: p.pinned,
        track_count: p.track_count,
        ..Default::default()
    }
}

/// «Новинки»: «Радар новинок» первой плиткой, потом свежие релизы (плитки альбомов открывают их страницу).
pub fn new_release_tiles(radar: Option<&PlaylistItem>, releases: &[NewRelease]) -> Vec<LibraryEntry> {
    let radar = radar.map(|p| LibraryEntry { subtitle: mf_i18n::tf("Playlist · {0}", &[&"Spotify"]), ..playlist_entry(p) });
    radar
        .into_iter()
        .chain(releases.iter().map(|r| {
            let album = &r.album;
            let details: Vec<&str> =
                [album.artists.as_str(), r.date_label.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
            LibraryEntry {
                kind: "album".into(),
                id: album.album_id.clone(),
                uri: album.uri.clone(),
                title: album.title.clone(),
                subtitle: details.join(" · "),
                cover: album.cover.clone(),
                covers: if album.cover.is_empty() { Vec::new() } else { vec![album.cover.clone()] },
                read_only: true,
                track_count: album.track_count,
                ..Default::default()
            }
        }))
        .take(NEW_RELEASE_TILES)
        .collect()
}

pub fn liked_entry(liked_uri: &str, count: i64) -> LibraryEntry {
    LibraryEntry {
        kind: "liked".into(),
        id: "liked".into(),
        uri: liked_uri.to_owned(),
        title: mf_i18n::t("Liked Songs"),
        subtitle: crate::data::text::tracks(count),
        track_count: count,
        ..Default::default()
    }
}

/// «Быстрый доступ»: Любимые + первые 5 плейлистов библиотеки.
pub fn quick_tiles(liked_uri: &str, liked_count: i64, playlists: &[PlaylistItem]) -> Vec<LibraryEntry> {
    std::iter::once(liked_entry(liked_uri, liked_count))
        .chain(playlists.iter().take(QUICK_PLAYLISTS).map(playlist_entry))
        .collect()
}

/// Одно прослушивание (локальная история или /me/player/recently-played).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Play {
    pub track: TrackItem,
    pub context: Option<PlayContext>,
    pub played_at: String,
}

fn played_key(at: &str) -> (i64, String) {
    let secs = at.parse::<jiff::Timestamp>().map(|t| t.as_millisecond()).unwrap_or(i64::MIN);
    (secs, at.to_owned())
}

/// Плитка «Недавно прослушанного» по прослушиванию: контекст (плейлист/альбом/исполнитель/радио/Любимые/шоу),
/// без контекста — альбом трека (выпуск — его шоу).
pub fn recent_tile(play: &Play, playlists: &[PlaylistItem]) -> Option<LibraryEntry> {
    let track = &play.track;
    let context = play.context.as_ref();
    let context_uri = context.and_then(|c| c.context_uri.as_deref()).unwrap_or("");
    let context_title = context.map(|c| c.title.as_str()).unwrap_or("");
    let context_cover = context.map(|c| c.cover.as_str()).filter(|c| !c.is_empty()).unwrap_or(&track.cover);
    let base = |kind: &str, id: &str, uri: &str, title: &str, subtitle: String, cover: &str| LibraryEntry {
        kind: kind.to_owned(),
        id: id.to_owned(),
        uri: uri.to_owned(),
        title: title.to_owned(),
        subtitle,
        cover: cover.to_owned(),
        covers: if cover.is_empty() { Vec::new() } else { vec![cover.to_owned()] },
        read_only: true,
        played_at: play.played_at.clone(),
        ..Default::default()
    };
    if let Some(seed) = station_seed(context_uri) {
        let title = if context_title.is_empty() { station_title(&track.title) } else { context_title.to_owned() };
        return Some(base("radio", &seed, context_uri, &title, mf_i18n::t("Radio"), context_cover));
    }
    if context_uri.ends_with(":collection") {
        let mut entry = base("liked", "liked", context_uri, &mf_i18n::t("Liked Songs"), mf_i18n::t("Playlist"), "");
        entry.covers.clear();
        return Some(entry);
    }
    match uri_kind(context_uri).zip(uri_id(context_uri)) {
        Some(("playlist", id)) => {
            if let Some(p) = playlists.iter().find(|p| p.playlist_id == id) {
                let mut entry = playlist_entry(p);
                entry.played_at = play.played_at.clone();
                return Some(entry);
            }
            let title = if context_title.is_empty() { mf_i18n::t("Playlist") } else { context_title.to_owned() };
            return Some(base("playlist", id, context_uri, &title, mf_i18n::t("Playlist"), context_cover));
        }
        Some(("album", id)) => {
            let title = if context_title.is_empty() { track.album.as_str() } else { context_title };
            return Some(base("album", id, context_uri, title, album_subtitle(track), context_cover));
        }
        Some(("artist", id)) => {
            let name = track.artist_refs.iter().find(|a| a.id == id).map(|a| a.name.as_str()).unwrap_or("");
            let title = if context_title.is_empty() { name } else { context_title };
            if title.is_empty() {
                return None;
            }
            return Some(base("artist", id, context_uri, title, mf_i18n::t("Artist"), context_cover));
        }
        Some(("show", id)) => {
            let title = if context_title.is_empty() { track.album.as_str() } else { context_title };
            return Some(base("show", id, context_uri, title, mf_i18n::t("Podcast"), context_cover));
        }
        _ => {}
    }
    if track.is_episode {
        if track.show_id.is_empty() {
            return None;
        }
        let uri = format!("spotify:show:{}", track.show_id);
        return Some(base("show", &track.show_id, &uri, &track.album, mf_i18n::t("Podcast"), &track.cover));
    }
    if track.album_id.is_empty() {
        return None;
    }
    let uri = format!("spotify:album:{}", track.album_id);
    Some(base("album", &track.album_id, &uri, &track.album, album_subtitle(track), &track.cover))
}

fn album_subtitle(track: &TrackItem) -> String {
    if track.artists.is_empty() { mf_i18n::t("Album") } else { mf_i18n::tf("Album · {0}", &[&track.artists]) }
}

/// «Недавно прослушанное»: прослушивания новые первыми → плитки без повторов, ≤ 16.
pub fn recent_tiles(mut plays: Vec<Play>, playlists: &[PlaylistItem]) -> Vec<LibraryEntry> {
    plays.sort_by_key(|play| std::cmp::Reverse(played_key(&play.played_at)));
    let mut seen = HashSet::new();
    plays
        .iter()
        .filter_map(|play| recent_tile(play, playlists))
        .filter(|tile| !tile.uri.is_empty() && seen.insert(tile.uri.clone()))
        .take(RECENT_TILES)
        .collect()
}

/// «Продолжить подкасты»: сначала начатые (новые первыми), затем самый новый невыслушанный выпуск каждого шоу
/// за последние 30 дней; без повторов, ≤ 16. Выпуски шоу Spotify отдаёт новыми первыми.
pub fn podcast_episodes(podcasts: &[(ShowItem, Vec<EpisodeItem>)], today: jiff::civil::Date) -> Vec<EpisodeItem> {
    let oldest_new = today.checked_sub(jiff::Span::new().days(NEW_EPISODE_DAYS)).unwrap_or(today);
    let released = |e: &EpisodeItem| e.release_date.get(..10).and_then(|d| d.parse::<jiff::civil::Date>().ok());
    let with_show = |show: &ShowItem, e: &EpisodeItem| {
        let mut e = e.clone();
        if e.show_id.is_empty() {
            e.show_id = show.show_id.clone();
        }
        if e.show_name.is_empty() {
            e.show_name = show.name.clone();
        }
        if e.cover.is_empty() {
            e.cover = show.cover.clone();
            e.cover_large = show.cover_large.clone();
        }
        e
    };
    let mut continuing = Vec::new();
    let mut new = Vec::new();
    for (show, episodes) in podcasts {
        for e in episodes {
            if !e.fully_played && e.resume_ms > 0 {
                continuing.push(with_show(show, e));
            }
        }
        if let Some(newest) = episodes.first()
            && !newest.fully_played
            && newest.resume_ms == 0
            && released(newest).is_some_and(|d| d >= oldest_new)
        {
            new.push(with_show(show, newest));
        }
    }
    for group in [&mut continuing, &mut new] {
        group.sort_by_key(|e| std::cmp::Reverse(released(e)));
    }
    let mut seen = HashSet::new();
    continuing
        .into_iter()
        .chain(new)
        .filter(|e| !e.uri.is_empty() && seen.insert(e.uri.clone()))
        .take(PODCAST_CARDS)
        .collect()
}

#[cfg(test)]
mod tests {
    use md3_model::ArtistRef;

    use super::*;

    fn playlist(id: &str, title: &str, owner_id: &str) -> PlaylistItem {
        PlaylistItem {
            playlist_id: id.into(),
            uri: format!("spotify:playlist:{id}"),
            title: title.into(),
            owner_id: owner_id.into(),
            owner: if owner_id == "spotify" { "Spotify".into() } else { owner_id.into() },
            ..Default::default()
        }
    }

    fn track(id: &str) -> TrackItem {
        TrackItem {
            track_id: id.into(),
            uri: format!("spotify:track:{id}"),
            title: format!("T{id}"),
            album_id: format!("al{id}"),
            album: format!("Al{id}"),
            artists: "Группа".into(),
            artist_refs: vec![ArtistRef { id: "ar".into(), name: "Группа".into() }],
            cover: format!("c{id}"),
            ..Default::default()
        }
    }

    fn play(id: &str, context: Option<PlayContext>, at: &str) -> Play {
        Play { track: track(id), context, played_at: at.into() }
    }

    #[test]
    fn made_for_you_names() {
        assert!(is_made_for_you("Discover Weekly", "Discover Weekly"));
        assert!(is_made_for_you(" daily mix 3", "Daily Mix"));
        assert!(!is_made_for_you("Daily Mix Chill", "Daily Mix"));
        assert!(!is_made_for_you("Daily Mix 2b", "Daily Mix"));
        assert!(!is_made_for_you("Discover Weekly Archive", "Discover Weekly"));
    }

    #[test]
    fn made_for_you_takes_spotify_playlists_once() {
        let found = vec![
            playlist("1", "Daily Mix 1", "spotify"),
            playlist("2", "Daily Mix 1", "spotify"),
            playlist("3", "Daily Mix 2", "someone"),
            playlist("4", "Daily Mix 3", "spotify"),
            playlist("5", "Daily Mixes", "spotify"),
        ];
        let ids: Vec<_> = made_for_you("Daily Mix", found).into_iter().map(|p| p.playlist_id).collect();
        assert_eq!(ids, ["1", "4"]);
        let many: Vec<_> = (0..9).map(|i| playlist(&i.to_string(), &format!("Daily Mix {i}"), "spotify")).collect();
        assert_eq!(made_for_you("Daily Mix", many).len(), MADE_FOR_YOU_PER_TERM);
    }

    #[test]
    fn quick_access_is_liked_and_five_playlists() {
        let playlists: Vec<_> = (0..8).map(|i| playlist(&i.to_string(), "P", "me")).collect();
        let tiles = quick_tiles("spotify:user:me:collection", 21, &playlists);
        assert_eq!(tiles.len(), 1 + QUICK_PLAYLISTS);
        assert_eq!((tiles[0].kind.as_str(), tiles[0].subtitle.as_str()), ("liked", "21 tracks"));
        assert_eq!(tiles[1].uri, "spotify:playlist:0");
        assert!(tiles[1].read_only);
    }

    #[test]
    fn the_dj_row_is_a_button_not_a_playlist() {
        let dj = PlaylistItem {
            playlist_id: "37i9dQZF1EYkqdzj48dyYq".into(),
            uri: crate::data::home_feed::DJ_URI.into(),
            title: "DJ".into(),
            owner: "Spotify".into(),
            ..Default::default()
        };
        let entry = playlist_entry(&dj);
        assert_eq!((entry.kind.as_str(), entry.title.as_str()), ("dj", "DJ"), "у DJ нет списка треков, он только играет");
        assert_eq!(entry.subtitle, mf_i18n::t("Music and commentary picked for you"));
        assert_eq!(entry.uri, crate::data::home_feed::DJ_URI);
        assert_eq!(entry.owner, "Spotify", "остальные поля не тронуты");

        assert_eq!(playlist_entry(&playlist("p", "Мой плейлист", "me")).kind, "playlist");
    }

    #[test]
    fn recent_tiles_follow_contexts_newest_first_without_repeats() {
        let mine = [playlist("p", "Мой плейлист", "me")];
        let plays = vec![
            play("1", Some(PlayContext::with_uri("playlist", "spotify:playlist:p")), "2026-10-10T10:00:00Z"),
            play("2", None, "2026-10-11T09:00:00Z"),
            play("3", Some(PlayContext::with_uri("radio", "spotify:station:track:3").titled("Radio “T3”")), "2026-10-11T08:00:00Z"),
            play("4", Some(PlayContext::with_uri("playlist", "spotify:playlist:p")), "2026-10-09T10:00:00Z"),
            play("5", Some(PlayContext::with_uri("liked", "spotify:user:me:collection")), "2026-10-08T10:00:00Z"),
            play("6", Some(PlayContext::with_uri("artist", "spotify:artist:ar")), "2026-10-07T10:00:00Z"),
        ];
        let tiles = recent_tiles(plays, &mine);
        let kinds: Vec<_> = tiles.iter().map(|t| (t.kind.as_str(), t.uri.as_str())).collect();
        assert_eq!(
            kinds,
            [
                ("album", "spotify:album:al2"),
                ("radio", "spotify:station:track:3"),
                ("playlist", "spotify:playlist:p"),
                ("liked", "spotify:user:me:collection"),
                ("artist", "spotify:artist:ar"),
            ]
        );
        assert_eq!(tiles[1].id, "spotify:track:3");
        assert_eq!(tiles[1].title, "Radio “T3”");
        assert_eq!(tiles[2].title, "Мой плейлист");
        assert_eq!(tiles[2].played_at, "2026-10-10T10:00:00Z");
        assert_eq!(tiles[4].title, "Группа");
    }

    #[test]
    fn a_radio_without_title_is_named_after_its_song() {
        let tile = recent_tile(&play("9", Some(PlayContext::with_uri("radio", "spotify:station:track:9")), ""), &[]);
        assert_eq!(tile.map(|t| t.title), Some("Radio “T9”".into()));
    }

    fn episode(id: &str, released: &str, resume: i64, fully: bool) -> EpisodeItem {
        EpisodeItem {
            episode_id: id.into(),
            uri: format!("spotify:episode:{id}"),
            release_date: released.into(),
            resume_ms: resume,
            fully_played: fully,
            ..Default::default()
        }
    }

    #[test]
    fn podcasts_continue_first_then_new() {
        let today: jiff::civil::Date = "2026-10-11".parse().unwrap();
        let show = |id: &str| ShowItem { show_id: id.into(), name: format!("Шоу {id}"), cover: "sc".into(), ..Default::default() };
        let podcasts = vec![
            (show("a"), vec![episode("a3", "2026-10-10", 0, false), episode("a2", "2026-09-01", 5000, false)]),
            (show("b"), vec![episode("b2", "2026-08-01", 0, false)]),
            (show("c"), vec![episode("c2", "2026-10-05", 100, false), episode("c1", "2026-10-01", 0, true)]),
            (show("d"), vec![episode("d1", "2026-10-09", 0, false)]),
        ];
        let shelf = podcast_episodes(&podcasts, today);
        let ids: Vec<_> = shelf.iter().map(|e| e.episode_id.as_str()).collect();
        // начатые (новые первыми), затем новые за 30 дней; b2 старше месяца
        assert_eq!(ids, ["c2", "a2", "a3", "d1"]);
        assert_eq!(shelf[0].show_name, "Шоу c");
        assert_eq!(shelf[0].cover, "sc");
    }

    #[test]
    fn shelves_carry_qml_keys() {
        let s = shelf(TOP_TRACKS, &[track("1")]);
        assert_eq!((s.kind.as_str(), s.title.as_str()), ("tracks", "Top tracks"));
        assert_eq!(s.items[0]["trackId"], "1");
        assert_eq!(hidden_keys(" quick, podcasts ,"), HashSet::from(["quick".to_owned(), "podcasts".to_owned()]));
    }

    #[test]
    fn new_releases_shelf_starts_with_the_radar() {
        let radar = playlist("37i9dQZEVXbsGPw6v9FjFR", "Радар новинок", "spotify");
        let release = NewRelease {
            album: md3_model::AlbumItem {
                album_id: "a1".into(),
                uri: "spotify:album:a1".into(),
                title: "Fresh".into(),
                artists: "Artist".into(),
                cover: "c".into(),
                ..Default::default()
            },
            release_date: "2026-10-09".into(),
            date_label: "9 октября".into(),
            group: "single".into(),
        };
        let tiles = new_release_tiles(Some(&radar), std::slice::from_ref(&release));
        assert_eq!(tiles.len(), 2);
        assert_eq!((tiles[0].kind.as_str(), tiles[0].id.as_str()), ("playlist", "37i9dQZEVXbsGPw6v9FjFR"));
        assert_eq!(tiles[1].kind, "album");
        assert_eq!(tiles[1].uri, "spotify:album:a1");
        assert_eq!(tiles[1].subtitle, "Artist · 9 октября");
        let many = vec![release; 40];
        assert_eq!(new_release_tiles(None, &many).len(), NEW_RELEASE_TILES);
        let s = shelf(NEW_RELEASES, &new_release_tiles(None, &many[..1]));
        assert_eq!((s.kind.as_str(), s.title.as_str()), ("tiles", "New releases"));
        assert!(ORDER.contains(&NEW_RELEASES));
    }
}
