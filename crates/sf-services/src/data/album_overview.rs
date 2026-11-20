//! Альбом и трек через GraphQL-шлюз клиентов Spotify (сохранённые запросы getAlbum и getTrack) с токеном
//! сессии: то, чего нет в метаданных librespot, — прослушивания каждого трека, а также лейбл, правообладатели,
//! дата выпуска с её точностью и «courtesy line». Без квоты Web API. Проверено вживую (2026-10):
//! data.albumUnion { label, copyright.items[{type C|P, text}], date { isoString, precision }, courtesyLine,
//! tracksV2 { totalCount, items[{ track { uri, playcount ("164947776"), discNumber, trackNumber } }] } };
//! data.trackUnion { playcount, trackNumber, albumOfTrack { copyright, date, courtesyLine } }.
use std::collections::HashMap;

use anyhow::{anyhow, bail};
use serde_json::{Value, json};
use sf_api::{Http, map};

use super::persisted;
use crate::catalog::AlbumPage;

/// Хэши getAlbum (тот же документ, что queryAlbumTracks; свежий — из бандла веб-плеера).
const ALBUM_HASHES: [&str; 1] = ["11c9cad9cb24c4f9e25ef9456f68621cfd6006a90f679e729c1d2f2fd70dfe16"];
/// Хэши getTrack.
const TRACK_HASHES: [&str; 1] = ["a8ef9e9f02b836feb0da3003c31dbb30decc6f4b473ef89ca88c882386d668de"];
/// Треков на страницу getAlbum (как у веб-плеера).
const PAGE: u64 = 50;
/// Сборники по тысяче треков — не дальше.
const MAX_PAGES: u64 = 40;

/// Трек альбома по шлюзу.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackPlays {
    pub uri: String,
    /// 0 — шлюз не знает
    pub play_count: i64,
    pub disc_number: i64,
    pub track_number: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumOverview {
    pub label: String,
    /// «© …», «℗ …»
    pub copyrights: Vec<String>,
    /// "2001-03-12" | "2001-03" | "2001" — по точности даты
    pub release_date: String,
    pub courtesy_line: String,
    pub total_tracks: u64,
    /// треки в порядке альбома
    pub tracks: Vec<TrackPlays>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackOverview {
    pub play_count: i64,
    pub track_number: i64,
    pub release_date: String,
    pub copyrights: Vec<String>,
    pub courtesy_line: String,
}

/// Альбом целиком: первая страница, затем остальные разом (у длинных альбомов).
pub async fn album(http: &Http, session: &sf_player::Session, album_id: &str) -> anyhow::Result<AlbumOverview> {
    let first = album_page(http, session, album_id, 0).await?;
    let mut overview = parse_album(&first)?;
    let total = overview.total_tracks.min(PAGE * MAX_PAGES);
    let offsets: Vec<u64> = (1..MAX_PAGES).map(|page| page * PAGE).take_while(|offset| *offset < total).collect();
    let rest = futures::future::join_all(offsets.iter().map(|offset| album_page(http, session, album_id, *offset))).await;
    for (offset, body) in offsets.iter().zip(rest) {
        // страница, что не пришла, обрывает список: дальше совпадения ищутся только по URI
        match body.and_then(|body| parse_album(&body)) {
            Ok(more) => overview.tracks.extend(more.tracks),
            Err(error) => {
                tracing::debug!("album {album_id} via gateway from {offset}: {error:#}");
                break;
            }
        }
    }
    Ok(overview)
}

async fn album_page(http: &Http, session: &sf_player::Session, album_id: &str, offset: u64) -> anyhow::Result<Value> {
    let variables =
        json!({ "uri": format!("spotify:album:{album_id}"), "locale": "", "offset": offset, "limit": PAGE });
    persisted::query(http, session, "getAlbum", &ALBUM_HASHES, &variables).await
}

/// Трек: прослушивания и сведения о его альбоме (без лейбла — его знает только getAlbum).
pub async fn track(http: &Http, session: &sf_player::Session, track_id: &str) -> anyhow::Result<TrackOverview> {
    let variables = json!({ "uri": format!("spotify:track:{track_id}") });
    let body = persisted::query(http, session, "getTrack", &TRACK_HASHES, &variables).await?;
    parse_track(&body)
}

/// "164947776" (шлюз отдаёт строкой) или число; иное — 0.
fn play_count(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        _ => 0,
    }
}

/// Дата с её точностью, как release_date Web API: DAY → "2001-03-12", MONTH → "2001-03", YEAR → "2001".
fn release_date(date: Option<&Value>) -> String {
    let Some(date) = date else { return String::new() };
    let iso = date.get("isoString").and_then(Value::as_str).unwrap_or_default();
    let year = date.get("year").and_then(Value::as_i64).filter(|year| *year > 0);
    let day = iso.get(..10).filter(|day| day.as_bytes().get(4) == Some(&b'-'));
    match (date.get("precision").and_then(Value::as_str).unwrap_or("DAY"), day) {
        ("YEAR", Some(day)) => day.get(..4).unwrap_or(day).to_owned(),
        ("MONTH", Some(day)) => day.get(..7).unwrap_or(day).to_owned(),
        (_, Some(day)) => day.to_owned(),
        (_, None) => year.map(|year| format!("{year:04}")).unwrap_or_default(),
    }
}

fn copyrights(copyright: Option<&Value>) -> Vec<String> {
    let items = copyright.and_then(|copyright| copyright.get("items")).and_then(Value::as_array);
    map::copyright_lines(items.into_iter().flatten().map(|item| {
        let field = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or_default();
        (field("type"), field("text"))
    }))
}

fn text(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

fn number(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn parse_album(body: &Value) -> anyhow::Result<AlbumOverview> {
    let Some(album) = body.pointer("/data/albumUnion").filter(|album| album.is_object()) else {
        bail!("no data.albumUnion: {}", body.get("errors").unwrap_or(&Value::Null));
    };
    let kind = album.get("__typename").and_then(Value::as_str).unwrap_or("Album");
    if kind != "Album" {
        return Err(anyhow!("albumUnion: {kind}"));
    }
    let tracks = album.get("tracksV2").or_else(|| album.get("tracks"));
    let items = tracks.and_then(|tracks| tracks.get("items")).and_then(Value::as_array);
    Ok(AlbumOverview {
        label: text(album, "/label"),
        copyrights: copyrights(album.get("copyright")),
        release_date: release_date(album.get("date")),
        courtesy_line: text(album, "/courtesyLine"),
        total_tracks: tracks.and_then(|tracks| tracks.get("totalCount")).and_then(Value::as_u64).unwrap_or(0),
        tracks: items
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("track"))
            .map(|track| TrackPlays {
                uri: text(track, "/uri"),
                play_count: play_count(track.get("playcount")),
                disc_number: number(track, "discNumber"),
                track_number: number(track, "trackNumber"),
            })
            .collect(),
    })
}

fn parse_track(body: &Value) -> anyhow::Result<TrackOverview> {
    let Some(track) = body.pointer("/data/trackUnion").filter(|track| track.is_object()) else {
        bail!("no data.trackUnion: {}", body.get("errors").unwrap_or(&Value::Null));
    };
    let kind = track.get("__typename").and_then(Value::as_str).unwrap_or("Track");
    if kind != "Track" {
        return Err(anyhow!("trackUnion: {kind}"));
    }
    let album = track.get("albumOfTrack").unwrap_or(&Value::Null);
    Ok(TrackOverview {
        play_count: play_count(track.get("playcount")),
        track_number: number(track, "trackNumber"),
        release_date: release_date(album.get("date")),
        copyrights: copyrights(album.get("copyright")),
        courtesy_line: text(album, "/courtesyLine"),
    })
}

/// Дополнить страницу альбома тем, что знает шлюз: прослушивания треков (по URI; если списки совпали
/// по длине — и по месту, для подменённых треков), лейбл и дата — если их нет или шлюз знает точнее,
/// правообладатели — как их показывает Spotify.
pub fn apply(page: &mut AlbumPage, overview: &AlbumOverview) {
    let by_uri: HashMap<&str, i64> =
        overview.tracks.iter().map(|track| (track.uri.as_str(), track.play_count)).collect();
    let in_order = overview.tracks.len() == page.tracks.len();
    for (index, track) in page.tracks.iter_mut().enumerate() {
        let uri = track.play_uri();
        let found = by_uri
            .get(uri.as_str())
            .copied()
            .or_else(|| in_order.then(|| overview.tracks[index].play_count));
        if let Some(count) = found.filter(|count| *count > 0) {
            track.play_count = count;
        }
    }
    let info = &mut page.info;
    if info.label.is_empty() {
        info.label = overview.label.clone();
    }
    if overview.release_date.len() > info.release_date.len() {
        info.release_date = overview.release_date.clone();
    }
    if !overview.copyrights.is_empty() {
        info.copyrights = overview.copyrights.clone();
    }
    if info.copyright.is_empty()
        && let Some(first) = overview.copyrights.first()
    {
        info.copyright = first.trim_start_matches(['©', '℗']).trim_start().to_owned();
    }
    if !overview.courtesy_line.is_empty() {
        info.courtesy_line = overview.courtesy_line.clone();
    }
}

/// Прослушивания трека на странице альбома (0 — неизвестно).
pub fn plays_on(page: &AlbumPage, track_id: &str) -> i64 {
    let uri = format!("spotify:track:{track_id}");
    page.tracks
        .iter()
        .find(|track| track.track_id == track_id || track.uri == uri)
        .map_or(0, |track| track.play_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use md3_model::{AlbumInfo, TrackItem};

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/get_album.json")).unwrap()
    }

    fn row(id: &str) -> TrackItem {
        TrackItem { track_id: id.into(), uri: format!("spotify:track:{id}"), ..Default::default() }
    }

    #[test]
    fn get_album_reads_plays_label_copyrights_and_date() {
        let overview = parse_album(&fixture()).unwrap();
        assert_eq!(overview.label, "Daft Life Ltd./ADA France");
        assert_eq!(overview.copyrights, ["© 2001 Daft Life Limited", "℗ 2001 Daft Life Limited"]);
        assert_eq!(overview.release_date, "2001-03-12");
        assert_eq!(overview.courtesy_line, "");
        assert_eq!(overview.total_tracks, 14);
        assert_eq!(overview.tracks.len(), 3);
        let first = &overview.tracks[0];
        assert_eq!(first.uri, "spotify:track:0DiWol3AO6WpXZgp0goxAV");
        assert_eq!((first.disc_number, first.track_number), (1, 1));
        assert!(first.play_count > 900_000_000, "{}", first.play_count);
        assert!(overview.tracks.iter().all(|track| track.play_count > 0));
    }

    #[test]
    fn dates_keep_their_precision() {
        let date = |iso: &str, precision: &str| release_date(Some(&json!({ "isoString": iso, "precision": precision })));
        assert_eq!(date("2013-05-20T00:00:00Z", "DAY"), "2013-05-20");
        assert_eq!(date("1975-04-01T00:00:00Z", "MONTH"), "1975-04");
        assert_eq!(date("1969-01-01T00:00:00Z", "YEAR"), "1969");
        assert_eq!(release_date(Some(&json!({ "year": 1987 }))), "1987");
        assert_eq!(release_date(None), "");
    }

    #[test]
    fn play_counts_come_as_strings_or_numbers() {
        assert_eq!(play_count(Some(&json!("164947776"))), 164_947_776);
        assert_eq!(play_count(Some(&json!(42))), 42);
        assert_eq!(play_count(Some(&json!(null))), 0);
        assert_eq!(play_count(None), 0);
    }

    #[test]
    fn a_missing_album_is_an_error() {
        assert!(parse_album(&json!({ "data": { "albumUnion": { "__typename": "NotFound" } } })).is_err());
        assert!(parse_album(&json!({ "errors": [{ "message": "Unauthorized" }] })).is_err());
    }

    #[test]
    fn the_overview_fills_the_session_page() {
        let overview = parse_album(&fixture()).unwrap();
        let mut page = AlbumPage {
            info: AlbumInfo { release_date: "2001".into(), copyright: "own".into(), ..Default::default() },
            tracks: vec![row("2VEZx7NWsZ1D0eJ4uv5Fym"), row("nothing"), row("0DiWol3AO6WpXZgp0goxAV")],
        };
        apply(&mut page, &overview);
        assert!(page.tracks[0].play_count > 0, "matched by URI out of order");
        assert!(page.tracks[2].play_count > 900_000_000);
        // три строки против трёх треков шлюза — неизвестный по URI берётся по месту
        assert_eq!(page.tracks[1].play_count, overview.tracks[1].play_count);
        assert_eq!(page.info.label, "Daft Life Ltd./ADA France");
        assert_eq!(page.info.release_date, "2001-03-12", "the more precise date wins");
        assert_eq!(page.info.copyrights.len(), 2);
        assert_eq!(page.info.copyright, "own", "the session's copyright stays");
        assert_eq!(plays_on(&page, "0DiWol3AO6WpXZgp0goxAV"), page.tracks[2].play_count);

        // списки разной длины — только по URI
        let mut short = AlbumPage { tracks: vec![row("nothing"), row("0DiWol3AO6WpXZgp0goxAV")], ..Default::default() };
        apply(&mut short, &overview);
        assert_eq!(short.tracks[0].play_count, 0);
        assert!(short.tracks[1].play_count > 0);
    }

    #[test]
    fn get_track_reads_plays_and_its_album() {
        let body = json!({ "data": { "trackUnion": {
            "__typename": "Track", "id": "4uLU6hMCjMI75M1A2tKUQC", "uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            "name": "Never Gonna Give You Up", "trackNumber": 1, "playcount": "1189454447",
            "duration": { "totalMilliseconds": 213573 },
            "albumOfTrack": {
                "uri": "spotify:album:6N9PS4QXF1D0OWPk0Sxtb4", "name": "Whenever You Need Somebody", "type": "ALBUM",
                "copyright": { "items": [{ "text": "(P) 1987 Sony Music Entertainment UK Limited", "type": "P" }], "totalCount": 1 },
                "courtesyLine": "", "date": { "isoString": "1987-11-12T00:00:00Z", "precision": "DAY", "year": 1987 }
            }
        } } });
        let track = parse_track(&body).unwrap();
        assert_eq!(track.play_count, 1_189_454_447);
        assert_eq!(track.track_number, 1);
        assert_eq!(track.release_date, "1987-11-12");
        assert_eq!(track.copyrights, ["℗ 1987 Sony Music Entertainment UK Limited"]);
        assert!(parse_track(&json!({ "data": { "trackUnion": { "__typename": "NotFound" } } })).is_err());
    }
}
