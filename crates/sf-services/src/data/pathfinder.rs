//! Поиск через GraphQL-шлюз клиентов Spotify (pathfinder, сохранённый запрос searchDesktop) с токеном сессии
//! librespot (login5): без квоты Web API, все разделы одним запросом. Ответ приводится к SearchResults Web API.
//! Хэши сохранённого запроса и их обновление из бандла веб-плеера — в [`super::persisted`]; если не подошёл
//! ни один, поиск уходит в Web API.
use anyhow::{anyhow, bail};
use serde_json::{Value, json};
use sf_api::Http;
use sf_api::models::{Album, Artist, ArtistRef, Episode, Image, Owner, Page, Playlist, SearchResults, Show, Track};

use super::persisted;

/// Хэши searchDesktop, проверенные вживую (2026-10): ответы одной формы (tracksV2, albumsV2, podcasts…).
/// Семена: свежий хэш найдётся в бандле веб-плеера, когда все эти отзовут.
const SEARCH_HASHES: [&str; 4] = [
    "eef7cc54888d91bdd6802623477873caa3948ae173a0c34fd86827b267e94c03",
    "4801118d4a100f756e833d33984436a3899cff359c532f8fd3aaf174b60b3b49",
    "d9f785900f0710b31c07818d617f4f7600c1e21217e80f5b043d1e78d74e6026",
    "f1f1c151cd392433ef4d2683a10deb9adeefd660f29692d8539ce450d2dfdb96",
];
/// Результатов в разделе — как search_limit общего приложения.
pub const SEARCH_LIMIT: u32 = 20;

/// Все разделы поиска по `query`.
pub async fn search(http: &Http, session: &sf_player::Session, query: &str) -> anyhow::Result<SearchResults> {
    let variables = json!({
        "searchTerm": query,
        "offset": 0,
        "limit": SEARCH_LIMIT,
        "numberOfTopResults": 5,
        "includeAudiobooks": false,
        "includeArtistHasConcertsField": false,
        "includePreReleases": false,
        "includeLocalConcertsField": false,
    });
    let body = persisted::query(http, session, "searchDesktop", &SEARCH_HASHES, &variables).await?;
    match parse(&body) {
        Ok(results) => Ok(results),
        Err(Failure::Retired) => Err(anyhow!("searchDesktop: PersistedQueryNotFound")),
        Err(Failure::Other(error)) => Err(error.context("pathfinder searchDesktop")),
    }
}

enum Failure {
    Retired,
    Other(anyhow::Error),
}

fn parse(body: &Value) -> Result<SearchResults, Failure> {
    if let Some(errors) = body.get("errors").and_then(Value::as_array) {
        let retired = errors.iter().any(|e| e.get("message").and_then(Value::as_str) == Some("PersistedQueryNotFound"));
        if retired {
            return Err(Failure::Retired);
        }
        if body.pointer("/data/searchV2").is_none() {
            return Err(Failure::Other(anyhow!("GraphQL errors: {errors:?}")));
        }
    }
    let search = body.pointer("/data/searchV2").ok_or_else(|| Failure::Other(anyhow!("no data.searchV2")))?;
    results(search).map_err(Failure::Other)
}

/// data.searchV2 → SearchResults Web API.
fn results(search: &Value) -> anyhow::Result<SearchResults> {
    if !search.is_object() {
        bail!("searchV2 is not an object");
    }
    Ok(SearchResults {
        tracks: section(search, "tracksV2", |item| track(item.pointer("/item/data").unwrap_or(&item["data"]))),
        artists: section(search, "artists", |item| artist(&item["data"])),
        albums: section(search, "albumsV2", |item| album(&item["data"])),
        playlists: section(search, "playlists", |item| playlist(&item["data"])),
        shows: section(search, "podcasts", |item| show(&item["data"])),
        episodes: section(search, "episodes", |item| episode(&item["data"])),
    })
}

/// Раздел как страница Web API; раздела нет — None, как у Web API без этого типа.
fn section<T>(search: &Value, name: &str, map: impl Fn(&Value) -> Option<T>) -> Option<Page<T>> {
    let section = search.get(name)?;
    let items: Vec<T> = section.get("items")?.as_array()?.iter().filter_map(map).collect();
    let total = section
        .get("totalCount")
        .and_then(Value::as_u64)
        .map_or(items.len() as u32, |total| u32::try_from(total).unwrap_or(u32::MAX));
    Some(Page { limit: SEARCH_LIMIT, total, items, offset: 0, next: None })
}

pub(super) fn text(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// "spotify:album:ID" → ID.
pub(super) fn id_of(uri: &str) -> String {
    uri.rsplit(':').next().unwrap_or_default().to_owned()
}

/// Сущность нужного вида: {"__typename": kind, "uri": "spotify:…"}; иное (PreRelease, Audiobook…) — мимо.
fn uri_of(data: &Value, kind: &str) -> Option<String> {
    if data.get("__typename").and_then(Value::as_str).is_some_and(|name| name != kind) {
        return None;
    }
    data.get("uri").and_then(Value::as_str).filter(|uri| !uri.is_empty()).map(str::to_owned)
}

pub(super) fn images(value: Option<&Value>) -> Vec<Image> {
    let dimension = |source: &Value, key: &str| source.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    value
        .and_then(|sources| sources.get("sources"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| {
            let url = source.get("url")?.as_str()?.to_owned();
            Some(Image { url, width: dimension(source, "width"), height: dimension(source, "height") })
        })
        .collect()
}

fn artist_refs(data: &Value) -> Vec<ArtistRef> {
    data.pointer("/artists/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|artist| {
            let uri = text(artist, "/uri");
            ArtistRef { id: Some(id_of(&uri)).filter(|id| !id.is_empty()), name: text(artist, "/profile/name"), uri: Some(uri) }
        })
        .collect()
}

fn explicit(data: &Value) -> bool {
    data.pointer("/contentRating/label").and_then(Value::as_str) == Some("EXPLICIT")
}

fn track(data: &Value) -> Option<Track> {
    let uri = uri_of(data, "Track")?;
    let album = data.get("albumOfTrack").map(|album| {
        let uri = text(album, "/uri");
        Album { id: id_of(&uri), uri, name: text(album, "/name"), images: images(album.get("coverArt")), ..Default::default() }
    });
    Some(Track {
        id: Some(id_of(&uri)),
        name: text(data, "/name"),
        duration_ms: data.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).map_or(0, |ms| ms as u32),
        explicit: explicit(data),
        artists: artist_refs(data),
        album,
        is_playable: data.pointer("/playability/playable").and_then(Value::as_bool),
        uri,
        ..Default::default()
    })
}

fn artist(data: &Value) -> Option<Artist> {
    let uri = uri_of(data, "Artist")?;
    Some(Artist {
        id: id_of(&uri),
        name: text(data, "/profile/name"),
        images: images(data.pointer("/visuals/avatarImage")),
        uri,
        ..Default::default()
    })
}

fn album(data: &Value) -> Option<Album> {
    let uri = uri_of(data, "Album")?;
    Some(Album {
        id: id_of(&uri),
        name: text(data, "/name"),
        album_type: data.get("type").and_then(Value::as_str).map(str::to_lowercase),
        images: images(data.get("coverArt")),
        artists: artist_refs(data),
        release_date: data.pointer("/date/year").and_then(Value::as_i64).filter(|year| *year > 0).map(|y| y.to_string()),
        uri,
        ..Default::default()
    })
}

fn playlist(data: &Value) -> Option<Playlist> {
    let uri = uri_of(data, "Playlist")?;
    let owner = data.pointer("/ownerV2/data").unwrap_or(&Value::Null);
    let owner_uri = text(owner, "/uri");
    Some(Playlist {
        id: id_of(&uri),
        name: text(data, "/name"),
        description: Some(text(data, "/description")).filter(|text| !text.is_empty()),
        images: images(data.pointer("/images/items/0")),
        owner: Owner {
            id: owner.get("username").and_then(Value::as_str).map(str::to_owned).or_else(|| {
                Some(id_of(&owner_uri)).filter(|id| !id.is_empty())
            }),
            display_name: owner.get("name").and_then(Value::as_str).map(str::to_owned),
            uri: Some(owner_uri).filter(|uri| !uri.is_empty()),
        },
        uri,
        ..Default::default()
    })
}

fn show(data: &Value) -> Option<Show> {
    let uri = uri_of(data, "Podcast")?;
    Some(Show {
        id: id_of(&uri),
        name: text(data, "/name"),
        publisher: text(data, "/publisher/name"),
        images: images(data.get("coverArt")),
        uri,
        ..Default::default()
    })
}

fn episode(data: &Value) -> Option<Episode> {
    let uri = uri_of(data, "Episode")?;
    let show = data.pointer("/podcastV2/data").and_then(show);
    Some(Episode {
        id: id_of(&uri),
        name: text(data, "/name"),
        description: text(data, "/description"),
        duration_ms: data.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).map_or(0, |ms| ms as u32),
        images: images(data.get("coverArt")),
        release_date: Some(text(data, "/releaseDate/isoString")).filter(|date| !date.is_empty()),
        explicit: explicit(data),
        show,
        uri,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(url: &str) -> Value {
        json!({ "sources": [{ "url": url, "width": 640, "height": 640 }, { "url": "small", "width": null, "height": null }] })
    }

    fn body() -> Value {
        json!({ "data": { "searchV2": {
            "tracksV2": { "totalCount": 318, "items": [{ "item": { "__typename": "TrackResponseWrapper", "data": {
                "__typename": "Track", "uri": "spotify:track:t1", "name": "Supermassive Black Hole",
                "duration": { "totalMilliseconds": 212_000 }, "contentRating": { "label": "EXPLICIT" },
                "playability": { "playable": true },
                "artists": { "items": [{ "uri": "spotify:artist:a1", "profile": { "name": "Muse" } }] },
                "albumOfTrack": { "uri": "spotify:album:al1", "name": "Black Holes and Revelations", "coverArt": cover("big") }
            } }, "matchedFields": [] }] },
            "artists": { "totalCount": 114, "items": [{ "data": { "__typename": "Artist", "uri": "spotify:artist:a1",
                "profile": { "name": "Muse" }, "visuals": { "avatarImage": cover("face") } } }] },
            "albumsV2": { "totalCount": 133, "items": [
                { "data": { "__typename": "Album", "uri": "spotify:album:al2", "name": "Absolution", "type": "ALBUM",
                    "date": { "year": 2003 }, "coverArt": cover("abs"),
                    "artists": { "items": [{ "uri": "spotify:artist:a1", "profile": { "name": "Muse" } }] } } },
                { "data": { "__typename": "PreRelease", "uri": "spotify:prerelease:x" } }
            ] },
            "playlists": { "totalCount": 112, "items": [{ "data": { "__typename": "Playlist",
                "uri": "spotify:playlist:p1", "name": "This Is Muse", "description": "",
                "images": { "items": [{ "sources": [{ "url": "mix", "width": null, "height": null }] }] },
                "ownerV2": { "data": { "__typename": "User", "uri": "spotify:user:spotify", "username": "spotify", "name": "Spotify" } } } }] },
            "podcasts": { "totalCount": 1000, "items": [{ "data": { "__typename": "Podcast", "uri": "spotify:show:s1",
                "name": "M.U.S.E.", "publisher": { "name": "Realm" }, "coverArt": cover("pod") } }] },
            "episodes": { "totalCount": 1000, "items": [{ "data": { "__typename": "Episode", "uri": "spotify:episode:e1",
                "name": "Ep", "description": "About", "duration": { "totalMilliseconds": 60_000 },
                "releaseDate": { "isoString": "2026-09-30T11:13:00Z" }, "contentRating": { "label": "NONE" },
                "podcastV2": { "data": { "__typename": "Podcast", "uri": "spotify:show:s2", "name": "My First Million" } } } }] },
            "users": { "totalCount": 1000, "items": [] }
        } } })
    }

    #[test]
    fn a_search_answer_reads_as_the_web_api_gives_it() {
        let Ok(results) = parse(&body()) else { panic!("parsed") };
        let tracks = results.tracks.unwrap();
        assert_eq!(tracks.total, 318);
        let track = &tracks.items[0];
        assert_eq!((track.id.as_deref(), track.uri.as_str()), (Some("t1"), "spotify:track:t1"));
        assert_eq!(track.duration_ms, 212_000);
        assert!(track.explicit);
        assert_eq!(track.artists[0].name, "Muse");
        assert_eq!(track.artists[0].id.as_deref(), Some("a1"));
        let album = track.album.as_ref().unwrap();
        assert_eq!((album.id.as_str(), album.images[0].url.as_str()), ("al1", "big"));
        assert_eq!(album.images[1].width, None, "unknown sizes stay unknown");

        let artist = &results.artists.unwrap().items[0];
        assert_eq!((artist.id.as_str(), artist.name.as_str(), artist.images[0].url.as_str()), ("a1", "Muse", "face"));

        let albums = results.albums.unwrap();
        assert_eq!(albums.items.len(), 1, "pre-releases are passed over");
        assert_eq!(albums.items[0].album_type.as_deref(), Some("album"));
        assert_eq!(albums.items[0].release_date.as_deref(), Some("2003"));

        let playlist = &results.playlists.unwrap().items[0];
        assert_eq!(playlist.id, "p1");
        assert_eq!(playlist.description, None);
        assert_eq!(playlist.owner.id.as_deref(), Some("spotify"));
        assert_eq!(playlist.owner.display_name.as_deref(), Some("Spotify"));
        assert_eq!(playlist.images[0].url, "mix");

        let show = &results.shows.unwrap().items[0];
        assert_eq!((show.id.as_str(), show.publisher.as_str()), ("s1", "Realm"));

        let episode = &results.episodes.unwrap().items[0];
        assert_eq!(episode.duration_ms, 60_000);
        assert_eq!(episode.release_date.as_deref(), Some("2026-09-30T11:13:00Z"));
        assert_eq!(episode.show.as_ref().map(|show| show.id.as_str()), Some("s2"));
    }

    #[test]
    fn a_retired_query_moves_on_and_other_errors_give_up() {
        let retired = json!({ "errors": [{ "message": "PersistedQueryNotFound" }] });
        assert!(matches!(parse(&retired), Err(Failure::Retired)));
        let other = json!({ "errors": [{ "message": "Unauthorized" }] });
        assert!(matches!(parse(&other), Err(Failure::Other(_))));
        assert!(matches!(parse(&json!({ "data": {} })), Err(Failure::Other(_))));
        // раздела нет — None, как у Web API
        let Ok(partial) = parse(&json!({ "data": { "searchV2": { "artists": { "items": [] } } } })) else { panic!() };
        assert!(partial.tracks.is_none());
        assert_eq!(partial.artists.map(|page| page.total), Some(0));
    }
}
