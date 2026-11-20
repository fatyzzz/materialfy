//! Исполнитель через GraphQL-шлюз клиентов Spotify (сохранённый запрос queryArtistOverview) с токеном
//! сессии: то, чего нет в метаданных librespot, — подписчики, ежемесячные слушатели, подписка аккаунта,
//! похожие исполнители, биография и широкая шапка. Без квоты Web API. Проверено вживую (2026-10):
//! data.artistUnion { stats { followers, monthlyListeners }, saved, relatedContent.relatedArtists,
//! profile.biography.text, headerImage.data.sources[{url, maxWidth}], visuals.avatarImage,
//! discography.topTracks.items[{ track { uri, playcount ("984913127") } }],
//! preRelease { uri: "spotify:prerelease:…", releaseDate.isoString, preReleaseContent { name, type, coverArt } } }
//! (preRelease приходит только с includePrerelease: true; у кого ничего не анонсировано — null).
use anyhow::{anyhow, bail};
use serde_json::{Value, json};
use sf_api::models::{Artist, Image};
use sf_api::{Http, map, util};

use super::pathfinder::{id_of, images, text};
use super::persisted;
use crate::catalog::ArtistPage;

/// Хэши queryArtistOverview (семена; свежий — из бандла веб-плеера).
const HASHES: [&str; 1] = ["9f8134ef565e78621f1e1793555bd6633c5ac144ae0f89604ed3ae3f80b3c8e6"];
/// Похожих — как /artists/{id}/related-artists.
const RELATED: usize = 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistOverview {
    pub name: String,
    pub avatar: Vec<Image>,
    pub followers: Option<u64>,
    pub monthly_listeners: Option<u64>,
    /// аккаунт подписан
    pub followed: Option<bool>,
    pub related: Vec<Artist>,
    pub biography: String,
    pub header_image: Option<String>,
    /// популярные треки: URI → прослушивания (> 0)
    pub top_plays: Vec<(String, i64)>,
    /// анонсированный релиз (обратный отсчёт в телефоне)
    pub pre_release: Option<PreRelease>,
}

/// Анонс: альбом или сингл, который ещё не вышел.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreRelease {
    /// spotify:prerelease:… (не альбом: до выхода его не открыть и не сыграть)
    pub uri: String,
    pub name: String,
    /// ALBUM | SINGLE | EP
    pub kind: String,
    pub cover: String,
    /// момент выхода, ISO 8601 UTC
    pub release_at: String,
    pub artist_id: String,
    pub artist: String,
}

pub async fn artist(http: &Http, session: &sf_player::Session, artist_id: &str) -> anyhow::Result<ArtistOverview> {
    let variables = json!({ "uri": format!("spotify:artist:{artist_id}"), "locale": "", "includePrerelease": true });
    let body = persisted::query(http, session, "queryArtistOverview", &HASHES, &variables).await?;
    parse(&body)
}

fn parse(body: &Value) -> anyhow::Result<ArtistOverview> {
    let Some(artist) = body.pointer("/data/artistUnion").filter(|artist| artist.is_object()) else {
        bail!("no data.artistUnion: {}", body.get("errors").unwrap_or(&Value::Null));
    };
    let kind = artist.get("__typename").and_then(Value::as_str).unwrap_or("Artist");
    if kind != "Artist" {
        return Err(anyhow!("artistUnion: {kind}"));
    }
    let count = |pointer: &str| artist.pointer(pointer).and_then(Value::as_u64);
    Ok(ArtistOverview {
        name: text(artist, "/profile/name"),
        avatar: images(artist.pointer("/visuals/avatarImage")),
        followers: count("/stats/followers"),
        monthly_listeners: count("/stats/monthlyListeners"),
        followed: artist.get("saved").and_then(Value::as_bool),
        related: artist
            .pointer("/relatedContent/relatedArtists/items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(related)
            .take(RELATED)
            .collect(),
        biography: util::strip_html(&text(artist, "/profile/biography/text")).trim().to_owned(),
        header_image: header(artist),
        top_plays: top_plays(artist),
        pre_release: pre_release(artist),
    })
}

fn pre_release(artist: &Value) -> Option<PreRelease> {
    let pre = artist.get("preRelease").filter(|pre| pre.is_object())?;
    let content = pre.get("preReleaseContent")?;
    let release_at = text(pre, "/releaseDate/isoString");
    let name = text(content, "/name");
    if name.is_empty() || release_at.is_empty() {
        return None;
    }
    Some(PreRelease {
        uri: text(pre, "/uri"),
        name,
        kind: text(content, "/type"),
        cover: map::cover(&images(content.get("coverArt")), 300),
        release_at,
        artist_id: id_of(&text(artist, "/uri")),
        artist: text(artist, "/profile/name"),
    })
}

fn top_plays(artist: &Value) -> Vec<(String, i64)> {
    let items = artist.pointer("/discography/topTracks/items").and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let track = item.get("track")?;
            let uri = track.get("uri").and_then(Value::as_str).filter(|uri| !uri.is_empty())?;
            let plays = match track.get("playcount")? {
                Value::String(text) => text.trim().parse().ok()?,
                Value::Number(number) => number.as_i64()?,
                _ => return None,
            };
            (plays > 0).then(|| (uri.to_owned(), plays))
        })
        .collect()
}

fn related(data: &Value) -> Option<Artist> {
    let uri = data.get("uri").and_then(Value::as_str).filter(|uri| uri.starts_with("spotify:artist:"))?;
    Some(Artist {
        id: id_of(uri),
        name: text(data, "/profile/name"),
        uri: uri.to_owned(),
        images: images(data.pointer("/visuals/avatarImage")),
        ..Default::default()
    })
}

/// Самая широкая шапка.
fn header(artist: &Value) -> Option<String> {
    let sources = artist.pointer("/headerImage/data/sources")?.as_array()?;
    let width = |source: &Value| source.get("maxWidth").and_then(Value::as_u64).unwrap_or(0);
    let mut best: Option<&Value> = None;
    for source in sources {
        if best.is_none_or(|best| width(source) > width(best)) {
            best = Some(source);
        }
    }
    best?.get("url")?.as_str().filter(|url| !url.is_empty()).map(str::to_owned)
}

/// Дополнить страницу исполнителя тем, что знает шлюз: подписчики, слушатели, подписка, похожие (если
/// метаданные сессии их не дали), биография, шапка; фото и имя — если их нет.
pub fn apply(page: &mut ArtistPage, overview: &ArtistOverview) {
    let info = &mut page.info;
    let mapped = map::artist_item(&Artist {
        name: overview.name.clone(),
        images: overview.avatar.clone(),
        ..Default::default()
    });
    if info.item.name.is_empty() {
        info.item.name = mapped.name;
    }
    if info.item.cover.is_empty() {
        info.item.cover = mapped.cover;
        info.item.cover_large = mapped.cover_large;
    }
    if let Some(followers) = overview.followers {
        info.item.followers = i64::try_from(followers).unwrap_or(i64::MAX);
    }
    if let Some(listeners) = overview.monthly_listeners {
        info.listeners = i64::try_from(listeners).unwrap_or(i64::MAX);
    }
    if let Some(followed) = overview.followed {
        info.followed = followed;
    }
    info.biography = overview.biography.clone();
    info.header_image = overview.header_image.clone().unwrap_or_default();
    if page.similar.is_empty() {
        page.similar = overview.related.iter().map(map::artist_item).collect();
    }
    for track in &mut page.popular {
        let uri = track.play_uri();
        if let Some((_, plays)) = overview.top_plays.iter().find(|(known, _)| *known == uri) {
            track.play_count = *plays;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md3_model::{ArtistInfo, ArtistItem, TrackItem};

    fn body() -> Value {
        json!({ "data": { "artistUnion": {
            "__typename": "Artist", "id": "a1", "uri": "spotify:artist:a1",
            "profile": { "name": "Muse", "biography": { "text": "The Wow! Signal. <a href=\"spotify:album:x\">Out</a> &amp; now.", "type": "AUTOBIOGRAPHY" } },
            "saved": true,
            "stats": { "followers": 9_094_030, "monthlyListeners": 21_950_166, "worldRank": 399 },
            "headerImage": { "data": { "__typename": "ImageV2", "sources": [
                { "maxHeight": 640, "maxWidth": 1494, "url": "small" },
                { "maxHeight": 1080, "maxWidth": 1920, "url": "wide" }
            ] } },
            "visuals": { "avatarImage": { "sources": [
                { "height": 640, "width": 640, "url": "face640" }, { "height": 160, "width": 160, "url": "face160" }
            ] } },
            "discography": { "topTracks": { "items": [
                { "track": { "id": "0DiWol3AO6WpXZgp0goxAV", "uri": "spotify:track:0DiWol3AO6WpXZgp0goxAV", "name": "One More Time",
                             "playcount": "984913127", "discNumber": 1, "duration": { "totalMilliseconds": 320357 } },
                  "uid": "5d99bb101e828b296cc3" },
                { "track": { "id": "2Foc5Q5nqNiosCNqttzHof", "uri": "spotify:track:2Foc5Q5nqNiosCNqttzHof",
                             "name": "Get Lucky (Radio Edit) [feat. Pharrell Williams and Nile Rodgers]",
                             "playcount": "1456452451", "discNumber": 1, "duration": { "totalMilliseconds": 247632 } },
                  "uid": "7e2bde934c60abb38dd2" },
                { "track": { "uri": "spotify:track:unknown", "playcount": null } }
            ] } },
            "relatedContent": { "relatedArtists": { "totalCount": 40, "items": [
                { "id": "r1", "uri": "spotify:artist:r1", "profile": { "name": "Franz Ferdinand" },
                  "visuals": { "avatarImage": { "sources": [{ "height": 640, "width": 640, "url": "ff" }] } } },
                { "id": "x", "uri": "", "profile": { "name": "Broken" } }
            ] } },
            "preRelease": {
                "preReleaseContent": { "name": "Castelos & Ruínas (Remix 10 Anos)", "type": "ALBUM", "coverArt": { "sources": [
                    { "height": 64, "width": 64, "url": "c64" }, { "height": 300, "width": 300, "url": "c300" }, { "height": 640, "width": 640, "url": "c640" }
                ] } },
                "releaseDate": { "isoString": "2026-11-17T03:00:00Z" },
                "uri": "spotify:prerelease:43AOJotQ2jR7y76VSqwiXH"
            }
        } } })
    }

    #[test]
    fn an_overview_reads_stats_related_and_profile() {
        let overview = parse(&body()).unwrap();
        assert_eq!(overview.name, "Muse");
        assert_eq!((overview.followers, overview.monthly_listeners), (Some(9_094_030), Some(21_950_166)));
        assert_eq!(overview.followed, Some(true));
        assert_eq!(overview.related.len(), 1, "an artist without a link is passed over");
        let related = &overview.related[0];
        assert_eq!((related.id.as_str(), related.name.as_str(), related.images[0].url.as_str()), ("r1", "Franz Ferdinand", "ff"));
        assert_eq!(overview.biography, "The Wow! Signal. Out & now.");
        assert_eq!(overview.header_image.as_deref(), Some("wide"));
        assert_eq!(overview.avatar.len(), 2);
        assert_eq!(
            overview.top_plays,
            [
                ("spotify:track:0DiWol3AO6WpXZgp0goxAV".to_owned(), 984_913_127),
                ("spotify:track:2Foc5Q5nqNiosCNqttzHof".to_owned(), 1_456_452_451)
            ],
            "a track without plays is passed over"
        );
        let pre = overview.pre_release.unwrap();
        assert_eq!((pre.name.as_str(), pre.kind.as_str(), pre.release_at.as_str()), ("Castelos & Ruínas (Remix 10 Anos)", "ALBUM", "2026-11-17T03:00:00Z"));
        assert_eq!((pre.artist_id.as_str(), pre.artist.as_str(), pre.uri.as_str()), ("a1", "Muse", "spotify:prerelease:43AOJotQ2jR7y76VSqwiXH"));
        assert!(!pre.cover.is_empty());
    }

    #[test]
    fn a_missing_artist_is_an_error() {
        assert!(parse(&json!({ "data": { "artistUnion": { "__typename": "NotFound" } } })).is_err());
        assert!(parse(&json!({ "errors": [{ "message": "Unauthorized" }] })).is_err());
        // пустой профиль — не ошибка: страница возьмёт то, что есть
        let bare = parse(&json!({ "data": { "artistUnion": { "__typename": "Artist" } } })).unwrap();
        assert_eq!((bare.followers, bare.followed, bare.header_image), (None, None, None));
        assert_eq!(bare.pre_release, None);
    }

    #[test]
    fn the_overview_fills_the_session_page_without_overriding_it() {
        let mut page = ArtistPage {
            info: ArtistInfo {
                item: ArtistItem { artist_id: "a1".into(), name: "Muse".into(), cover: "own".into(), ..Default::default() },
                ..Default::default()
            },
            popular: vec![
                TrackItem { track_id: "2Foc5Q5nqNiosCNqttzHof".into(), ..Default::default() },
                TrackItem { track_id: "other".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        apply(&mut page, &parse(&body()).unwrap());
        assert_eq!(page.info.item.cover, "own", "the session's photo stays");
        assert_eq!(page.info.item.followers, 9_094_030);
        assert_eq!(page.info.listeners, 21_950_166);
        assert!(page.info.followed);
        assert_eq!(page.info.header_image, "wide");
        assert_eq!(page.similar[0].artist_id, "r1");
        assert_eq!(page.popular[0].play_count, 1_456_452_451, "plays follow the URI, not the place");
        assert_eq!(page.popular[1].play_count, 0);

        let mut empty = ArtistPage::default();
        apply(&mut empty, &parse(&body()).unwrap());
        assert_eq!(empty.info.item.name, "Muse");
        assert!(!empty.info.item.cover.is_empty(), "the overview's photo fills in");
    }
}
