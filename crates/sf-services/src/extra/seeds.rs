//! Группы сидов радио на орбите главной (вместо групп «Моей волны» Яндекса): недавние треки, исполнители,
//! альбомы и плейлисты, «любой трек» (выбранные через поиск). Закрепления (SettingKey::Pins) — первыми в группе.
use std::collections::HashSet;

use md3_model::{AlbumItem, ArtistItem, PlayContext, PlaylistItem, RadioSeed, RadioSeedGroup, TrackItem};
use sf_api::util::{station_seed, uri_kind};

pub const RECENT_MAX: usize = 6;
pub const ARTISTS_MAX: usize = 5;
pub const COLLECTIONS_MAX: usize = 6;
pub const ANY_MAX: usize = 6;

pub const RECENT: &str = "recent";
pub const ARTISTS: &str = "artists";
pub const COLLECTIONS: &str = "collections";
pub const ANY: &str = "any";

/// Откуда берутся сиды.
#[derive(Default)]
pub struct SeedSources<'a> {
    /// история, новые первыми (повторы допустимы)
    pub recent_tracks: &'a [TrackItem],
    /// Любимые — запас, пока история короткая
    pub liked: &'a [TrackItem],
    pub recent_contexts: &'a [PlayContext],
    pub playlists: &'a [PlaylistItem],
    pub albums: &'a [AlbumItem],
    /// подписки на исполнителей
    pub artists: &'a [ArtistItem],
    /// закреплённые URI (сиды или станции)
    pub pins: &'a [String],
    /// выбранные через поиск, новые первыми
    pub any: &'a [RadioSeed],
}

/// Сид по URI, если о нём что-то известно (подпись и обложка).
pub fn known_seed(sources: &SeedSources<'_>, uri: &str) -> Option<RadioSeed> {
    let seed = |label: &str, cover: &str, kind: &str| RadioSeed {
        uri: uri.to_owned(),
        label: label.to_owned(),
        cover: cover.to_owned(),
        kind: kind.to_owned(),
    };
    match uri_kind(uri)? {
        "track" => sources
            .recent_tracks
            .iter()
            .chain(sources.liked)
            .find(|t| t.play_uri() == uri)
            .map(|t| seed(&t.title, &t.cover, "track")),
        "artist" => {
            let id = sf_api::util::uri_id(uri)?;
            sources
                .artists
                .iter()
                .find(|a| a.artist_id == id || a.uri == uri)
                .map(|a| seed(&a.name, &a.cover, "artist"))
                .or_else(|| {
                    let track = sources
                        .recent_tracks
                        .iter()
                        .chain(sources.liked)
                        .find(|t| t.artist_refs.iter().any(|r| r.id == id))?;
                    let name = &track.artist_refs.iter().find(|r| r.id == id)?.name;
                    Some(seed(name, &track.cover, "artist"))
                })
        }
        "album" => {
            let id = sf_api::util::uri_id(uri)?;
            sources
                .albums
                .iter()
                .find(|a| a.album_id == id || a.uri == uri)
                .map(|a| seed(&a.title, &a.cover, "album"))
                .or_else(|| context_seed(sources.recent_contexts, uri, "album"))
                .or_else(|| {
                    let track = sources.recent_tracks.iter().chain(sources.liked).find(|t| t.album_id == id)?;
                    Some(seed(&track.album, &track.cover, "album"))
                })
        }
        "playlist" => {
            let id = sf_api::util::uri_id(uri)?;
            sources
                .playlists
                .iter()
                .find(|p| p.playlist_id == id || p.uri == uri)
                .map(|p| seed(&p.title, &p.cover, "playlist"))
                .or_else(|| context_seed(sources.recent_contexts, uri, "playlist"))
        }
        _ => None,
    }
}

fn context_seed(contexts: &[PlayContext], uri: &str, kind: &str) -> Option<RadioSeed> {
    contexts
        .iter()
        .find(|c| c.context_uri.as_deref() == Some(uri) && !c.title.is_empty())
        .map(|c| RadioSeed { uri: uri.to_owned(), label: c.title.clone(), cover: c.cover.clone(), kind: kind.to_owned() })
}

/// Закреплённые сиды нужного вида (станция → её сид).
fn pinned(sources: &SeedSources<'_>, kinds: &[&str]) -> Vec<RadioSeed> {
    sources
        .pins
        .iter()
        .filter_map(|pin| station_seed(pin).or_else(|| Some(pin.clone())))
        .filter(|uri| uri_kind(uri).is_some_and(|k| kinds.contains(&k)))
        .filter_map(|uri| known_seed(sources, &uri))
        .collect()
}

struct Group {
    items: Vec<RadioSeed>,
    seen: HashSet<String>,
    max: usize,
}

impl Group {
    fn new(max: usize) -> Self {
        Self { items: Vec::new(), seen: HashSet::new(), max }
    }

    fn push(&mut self, seed: RadioSeed) {
        if self.items.len() < self.max && !seed.uri.is_empty() && self.seen.insert(seed.uri.clone()) {
            self.items.push(seed);
        }
    }

    fn full(&self) -> bool {
        self.items.len() >= self.max
    }

    fn into_group(self, key: &str, title: &str) -> RadioSeedGroup {
        RadioSeedGroup { key: key.to_owned(), title: title.to_owned(), items: self.items }
    }
}

fn track_seed(track: &TrackItem) -> RadioSeed {
    RadioSeed { uri: track.play_uri(), label: track.title.clone(), cover: track.cover.clone(), kind: "track".into() }
}

/// Четыре группы орбиты (всегда все четыре, пустые тоже: у «любого трека» есть кнопка «+»).
/// `hidden` — локальный стоп-лист: такие треки не предлагаются.
pub fn build(sources: &SeedSources<'_>, hidden: &dyn Fn(&str) -> bool) -> Vec<RadioSeedGroup> {
    let playable = |t: &&TrackItem| !t.is_episode && t.available && !t.track_id.is_empty() && !hidden(&t.play_uri());

    let mut recent = Group::new(RECENT_MAX);
    for seed in pinned(sources, &["track"]) {
        recent.push(seed);
    }
    for track in sources.recent_tracks.iter().filter(playable) {
        recent.push(track_seed(track));
    }
    if !recent.full() {
        for track in sources.liked.iter().filter(playable) {
            recent.push(track_seed(track));
        }
    }

    let mut artists = Group::new(ARTISTS_MAX);
    for seed in pinned(sources, &["artist"]) {
        artists.push(seed);
    }
    for track in sources.recent_tracks.iter().filter(|t| !t.is_episode) {
        let Some(artist) = track.artist_refs.first().filter(|a| !a.id.is_empty()) else {
            continue;
        };
        let uri = format!("spotify:artist:{}", artist.id);
        let followed = sources.artists.iter().find(|a| a.artist_id == artist.id);
        let cover = followed.map(|a| a.cover.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| track.cover.clone());
        artists.push(RadioSeed { uri, label: artist.name.clone(), cover, kind: "artist".into() });
    }
    for artist in sources.artists {
        let uri = if artist.uri.is_empty() { format!("spotify:artist:{}", artist.artist_id) } else { artist.uri.clone() };
        artists.push(RadioSeed { uri, label: artist.name.clone(), cover: artist.cover.clone(), kind: "artist".into() });
    }

    // The DJ looks like a playlist but Spotify builds no station from it: it plays only as itself (Home's DJ tile)
    let not_dj = |uri: &str| uri != crate::data::home_feed::DJ_URI;
    let mut collections = Group::new(COLLECTIONS_MAX);
    for seed in pinned(sources, &["album", "playlist"]).into_iter().filter(|s| not_dj(&s.uri)) {
        collections.push(seed);
    }
    for context in sources.recent_contexts {
        let Some(uri) = context.context_uri.as_deref().filter(|u| not_dj(u)) else { continue };
        let Some(kind @ ("album" | "playlist")) = uri_kind(uri) else { continue };
        if let Some(seed) = known_seed(sources, uri).or_else(|| {
            (!context.title.is_empty()).then(|| RadioSeed {
                uri: uri.to_owned(),
                label: context.title.clone(),
                cover: context.cover.clone(),
                kind: kind.to_owned(),
            })
        }) {
            collections.push(seed);
        }
    }
    for playlist in sources.playlists {
        let uri = if playlist.uri.is_empty() { format!("spotify:playlist:{}", playlist.playlist_id) } else { playlist.uri.clone() };
        if !not_dj(&uri) {
            continue;
        }
        collections.push(RadioSeed { uri, label: playlist.title.clone(), cover: playlist.cover.clone(), kind: "playlist".into() });
    }

    let mut any = Group::new(ANY_MAX);
    for seed in sources.any {
        any.push(seed.clone());
    }

    vec![
        recent.into_group(RECENT, &mf_i18n::t("Recent tracks")),
        artists.into_group(ARTISTS, &mf_i18n::t("Your artists")),
        collections.into_group(COLLECTIONS, &mf_i18n::t("Albums and playlists")),
        any.into_group(ANY, &mf_i18n::t("Any track")),
    ]
}

/// Сид по умолчанию: первый недавний трек (иначе первый сид любой группы).
pub fn default_seed(groups: &[RadioSeedGroup]) -> Option<String> {
    groups
        .iter()
        .find(|g| g.key == RECENT)
        .and_then(|g| g.items.first())
        .or_else(|| groups.iter().find_map(|g| g.items.first()))
        .map(|s| s.uri.clone())
}

/// Сид из групп по URI.
pub fn find<'a>(groups: &'a [RadioSeedGroup], uri: &str) -> Option<&'a RadioSeed> {
    groups.iter().flat_map(|g| g.items.iter()).find(|s| s.uri == uri)
}

/// «Радио «…»» (Radio “…”).
pub fn station_title(label: &str) -> String {
    if label.is_empty() { mf_i18n::t("Radio") } else { mf_i18n::tf("Radio “{0}”", &[&label]) }
}

#[cfg(test)]
mod tests {
    use md3_model::ArtistRef;

    use super::*;

    fn track(id: &str, artist: &str) -> TrackItem {
        TrackItem {
            track_id: id.into(),
            uri: format!("spotify:track:{id}"),
            title: format!("T{id}"),
            cover: format!("c{id}"),
            album_id: format!("al{id}"),
            album: format!("Al{id}"),
            available: true,
            artist_refs: vec![ArtistRef { id: artist.into(), name: format!("A{artist}") }],
            ..Default::default()
        }
    }

    fn uris(group: &RadioSeedGroup) -> Vec<&str> {
        group.items.iter().map(|s| s.uri.as_str()).collect()
    }

    #[test]
    fn groups_are_always_four_in_order() {
        let groups = build(&SeedSources::default(), &|_| false);
        let keys: Vec<_> = groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, [RECENT, ARTISTS, COLLECTIONS, ANY]);
        assert!(groups.iter().all(|g| g.items.is_empty()));
        assert_eq!(default_seed(&groups), None);
    }

    #[test]
    fn recent_tracks_are_unique_newest_first_and_skip_hidden() {
        let history = [track("1", "a"), track("2", "b"), track("1", "a"), track("3", "a")];
        let sources = SeedSources { recent_tracks: &history, ..Default::default() };
        let groups = build(&sources, &|uri| uri == "spotify:track:2");
        assert_eq!(uris(&groups[0]), ["spotify:track:1", "spotify:track:3"]);
        assert_eq!(default_seed(&groups).as_deref(), Some("spotify:track:1"));
        // исполнители — из недавних треков, без повторов
        assert_eq!(uris(&groups[1]), ["spotify:artist:a", "spotify:artist:b"]);
        assert_eq!(groups[1].items[0].label, "Aa");
    }

    #[test]
    fn liked_fills_a_short_history() {
        let history = [track("1", "a")];
        let liked = [track("1", "a"), track("9", "z")];
        let sources = SeedSources { recent_tracks: &history, liked: &liked, ..Default::default() };
        let groups = build(&sources, &|_| false);
        assert_eq!(uris(&groups[0]), ["spotify:track:1", "spotify:track:9"]);
    }

    #[test]
    fn episodes_and_unavailable_tracks_are_not_seeds() {
        let mut episode = track("e", "a");
        episode.is_episode = true;
        let mut gone = track("g", "a");
        gone.available = false;
        let history = [episode, gone];
        let groups = build(&SeedSources { recent_tracks: &history, ..Default::default() }, &|_| false);
        assert!(groups[0].items.is_empty());
    }

    #[test]
    fn pins_come_first_and_stations_pin_their_seed() {
        let history = [track("1", "a"), track("2", "b")];
        let playlists = [PlaylistItem {
            playlist_id: "p".into(),
            uri: "spotify:playlist:p".into(),
            title: "Мой".into(),
            ..Default::default()
        }];
        let contexts = [PlayContext::with_uri("album", "spotify:album:x").titled("Альбом X")];
        let pins = ["spotify:station:track:2".to_owned(), "spotify:playlist:p".to_owned(), "spotify:artist:zz".to_owned()];
        let sources = SeedSources {
            recent_tracks: &history,
            recent_contexts: &contexts,
            playlists: &playlists,
            pins: &pins,
            ..Default::default()
        };
        let groups = build(&sources, &|_| false);
        assert_eq!(uris(&groups[0]), ["spotify:track:2", "spotify:track:1"]);
        assert_eq!(uris(&groups[2]), ["spotify:playlist:p", "spotify:album:x"]);
        assert_eq!(groups[2].items[1].label, "Альбом X");
        // неизвестный закреплённый исполнитель без подписи не показывается
        assert_eq!(uris(&groups[1]), ["spotify:artist:a", "spotify:artist:b"]);
    }

    #[test]
    fn the_dj_is_never_a_station_seed() {
        let dj = crate::data::home_feed::DJ_URI;
        let contexts = [PlayContext::with_uri("playlist", dj).titled("DJ"), PlayContext::with_uri("album", "spotify:album:x").titled("X")];
        let playlists = [PlaylistItem { playlist_id: "37i9dQZF1EYkqdzj48dyYq".into(), title: "DJ".into(), ..Default::default() }];
        let pins = [dj.to_owned()];
        let sources = SeedSources { recent_contexts: &contexts, playlists: &playlists, pins: &pins, ..Default::default() };
        let groups = build(&sources, &|_| false);
        assert_eq!(uris(&groups[2]), ["spotify:album:x"]);
    }

    #[test]
    fn followed_artist_cover_wins_and_groups_are_capped() {
        let history: Vec<_> = (0..10).map(|i| track(&i.to_string(), &format!("r{i}"))).collect();
        let followed = [ArtistItem { artist_id: "r0".into(), name: "R0".into(), cover: "face".into(), ..Default::default() }];
        let sources = SeedSources { recent_tracks: &history, artists: &followed, ..Default::default() };
        let groups = build(&sources, &|_| false);
        assert_eq!(groups[0].items.len(), RECENT_MAX);
        assert_eq!(groups[1].items.len(), ARTISTS_MAX);
        assert_eq!(groups[1].items[0].cover, "face");
    }

    #[test]
    fn any_keeps_the_users_choices() {
        let any = [RadioSeed { uri: "spotify:track:s".into(), label: "S".into(), cover: String::new(), kind: "track".into() }];
        let groups = build(&SeedSources { any: &any, ..Default::default() }, &|_| false);
        assert_eq!(uris(&groups[3]), ["spotify:track:s"]);
        assert_eq!(find(&groups, "spotify:track:s").map(|s| s.label.as_str()), Some("S"));
        assert_eq!(default_seed(&groups).as_deref(), Some("spotify:track:s"));
    }

    #[test]
    fn titles() {
        assert_eq!(station_title("Song"), "Radio “Song”");
        assert_eq!(station_title(""), "Radio");
    }
}
