//! Обложки не со Spotify: когда его картинку не дало ни одно зеркало, обложку ищем по названию и исполнителю —
//! iTunes, затем Deezer, затем Яндекс Музыка (все без ключа, все открываются в РФ). Берём только совпадение по
//! исполнителю (и по названию для альбома и трека): чужая обложка хуже пустой.
//!
//! У каждого источника свой бюджет запросов (замерено): iTunes — ~20 в минуту, дальше бан 403 на минуты (берём 15);
//! Deezer — 50 за 5 с (берём 40); Яндекс — пачка из 10+ разом даёт 429, 3–4 в секунду держит (берём 3). Источник
//! без бюджета пропускается — обложка идёт к следующему, а не ждёт; ответ 429/403 ставит источник на паузу.
//! Найденное и «не нашлось» помнятся до конца работы, одинаковые запросы в полёте не дублируются.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::watch;

/// Что на обложке.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoverKind {
    Album,
    Track,
    Artist,
}

impl CoverKind {
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "album" => Some(Self::Album),
            "track" => Some(Self::Track),
            "artist" => Some(Self::Artist),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Itunes,
    Deezer,
    Yandex,
}

const SOURCES: [Source; 3] = [Source::Itunes, Source::Deezer, Source::Yandex];

impl Source {
    /// (запросов, за сколько)
    fn budget(self) -> (usize, Duration) {
        match self {
            Self::Itunes => (15, Duration::from_secs(60)),
            Self::Deezer => (40, Duration::from_secs(5)),
            Self::Yandex => (3, Duration::from_secs(1)),
        }
    }

    /// Фото исполнителей в поиске iTunes нет
    fn serves(self, kind: CoverKind) -> bool {
        !(self == Self::Itunes && kind == CoverKind::Artist)
    }

    fn request(self, client: &reqwest::Client, kind: CoverKind, artist: &str, title: &str) -> reqwest::RequestBuilder {
        let query = if kind == CoverKind::Artist { artist.to_owned() } else { format!("{artist} {title}") };
        let request = match self {
            Self::Itunes => {
                let entity = if kind == CoverKind::Track { "song" } else { "album" };
                client
                    .get("https://itunes.apple.com/search")
                    .query(&[("term", query.as_str()), ("entity", entity), ("limit", "10")])
            }
            Self::Deezer => {
                let endpoint = match kind {
                    CoverKind::Album => "album",
                    CoverKind::Track => "track",
                    CoverKind::Artist => "artist",
                };
                client
                    .get(format!("https://api.deezer.com/search/{endpoint}"))
                    .query(&[("q", query.as_str()), ("limit", "10")])
            }
            Self::Yandex => {
                let kind = match kind {
                    CoverKind::Album => "album",
                    CoverKind::Track => "track",
                    CoverKind::Artist => "artist",
                };
                client
                    .get("https://api.music.yandex.net/search")
                    .query(&[("text", query.as_str()), ("type", kind), ("page", "0")])
            }
        };
        request.timeout(Duration::from_secs(10))
    }
}

/// Скользящее окно запросов и пауза после отказа источника.
#[derive(Default)]
struct Gate {
    sent: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

impl Gate {
    fn take(&mut self, source: Source, now: Instant) -> bool {
        if self.paused_until.is_some_and(|until| now < until) {
            return false;
        }
        let (count, window) = source.budget();
        while self.sent.front().is_some_and(|sent| now.duration_since(*sent) >= window) {
            self.sent.pop_front();
        }
        if self.sent.len() >= count {
            return false;
        }
        self.sent.push_back(now);
        true
    }
}

type Key = (CoverKind, String, String);

enum Outcome {
    Found(String),
    NotFound,
    /// сеть, квота, пропущен без бюджета — спросить снова в другой раз
    Unknown,
}

#[derive(Default)]
pub struct CoverFallback {
    known: Mutex<HashMap<Key, watch::Receiver<Option<Option<String>>>>>,
    gates: Mutex<[Gate; 3]>,
}

impl CoverFallback {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// URL обложки или None. `artist` — первый исполнитель; для Artist `title` пустой.
    pub async fn find(&self, http: &sf_api::Http, kind: CoverKind, artist: &str, title: &str) -> Option<String> {
        let key: Key = (kind, normalize(artist), normalize(title));
        if key.1.is_empty() || (kind != CoverKind::Artist && key.2.is_empty()) {
            return None;
        }
        let (mut rx, owner) = {
            let mut known = self.known.lock().unwrap_or_else(|lock| lock.into_inner());
            match known.get(&key) {
                Some(rx) => (rx.clone(), None),
                None => {
                    let (tx, rx) = watch::channel(None);
                    known.insert(key.clone(), rx.clone());
                    (rx, Some(tx))
                }
            }
        };
        let Some(tx) = owner else {
            return rx.wait_for(Option::is_some).await.ok()?.clone().flatten();
        };
        let mut settled = true;
        let mut found = None;
        for source in SOURCES.into_iter().filter(|source| source.serves(kind)) {
            match self.ask(http, source, kind, artist, title).await {
                Outcome::Found(url) => {
                    found = Some(url);
                    break;
                }
                Outcome::NotFound => {}
                Outcome::Unknown => settled = false,
            }
        }
        if found.is_none() && !settled {
            // кто-то не ответил — в следующий раз спросим снова
            self.known.lock().unwrap_or_else(|lock| lock.into_inner()).remove(&key);
        }
        let _ = tx.send(Some(found.clone()));
        found
    }

    async fn ask(&self, http: &sf_api::Http, source: Source, kind: CoverKind, artist: &str, title: &str) -> Outcome {
        let slot = SOURCES.iter().position(|s| *s == source).unwrap_or(0);
        if !self.gates.lock().unwrap_or_else(|lock| lock.into_inner())[slot].take(source, Instant::now()) {
            return Outcome::Unknown;
        }
        let Ok(client) = http.client() else {
            return Outcome::Unknown;
        };
        let reply = match source.request(&client, kind, artist, title).send().await {
            Ok(reply) => reply,
            Err(error) => {
                tracing::debug!("cover fallback {source:?}: {}", error.without_url());
                return Outcome::Unknown;
            }
        };
        let status = reply.status();
        if status.as_u16() == 429 || status.as_u16() == 403 {
            // лимит: источник отдыхает (iTunes банит на минуты)
            let pause = if source == Source::Itunes { Duration::from_secs(300) } else { Duration::from_secs(30) };
            self.gates.lock().unwrap_or_else(|lock| lock.into_inner())[slot].paused_until = Some(Instant::now() + pause);
            tracing::debug!("cover fallback {source:?}: {status}, paused");
            return Outcome::Unknown;
        }
        if !status.is_success() {
            return Outcome::Unknown;
        }
        let Ok(json) = reply.json::<Value>().await else {
            return Outcome::Unknown;
        };
        if source == Source::Deezer && json.get("error").is_some() {
            // квота Deezer приходит ответом 200 с ошибкой
            self.gates.lock().unwrap_or_else(|lock| lock.into_inner())[slot].paused_until =
                Some(Instant::now() + Duration::from_secs(6));
            return Outcome::Unknown;
        }
        match pick(source, &json, kind, artist, title) {
            Some(url) => Outcome::Found(url),
            None => Outcome::NotFound,
        }
    }
}

/// Лучшая обложка из ответа поиска: исполнитель совпадает; у альбома и трека — ещё и название (точно, а нет —
/// название начинается с искомого: «Album (Deluxe)»).
fn pick(source: Source, json: &Value, kind: CoverKind, artist: &str, title: &str) -> Option<String> {
    let (artist, title) = (normalize(artist), normalize(title));
    let items: Vec<Item> = match source {
        Source::Itunes => json.get("results")?.as_array()?.iter().map(|it| itunes_item(it, kind)).collect(),
        Source::Deezer => json.get("data")?.as_array()?.iter().map(|it| deezer_item(it, kind)).collect(),
        Source::Yandex => {
            let section = match kind {
                CoverKind::Album => "albums",
                CoverKind::Track => "tracks",
                CoverKind::Artist => "artists",
            };
            json.get("result")?.get(section)?.get("results")?.as_array()?.iter().map(|it| yandex_item(it, kind)).collect()
        }
    };
    let candidates: Vec<&Item> = items.iter().filter(|item| item.image.is_some() && item.artists.contains(&artist)).collect();
    let best = if kind == CoverKind::Artist {
        candidates.first().copied()
    } else {
        candidates
            .iter()
            .find(|item| item.title == title)
            .or_else(|| candidates.iter().find(|item| item.title.starts_with(&title)))
            .copied()
    };
    best.and_then(|item| item.image.clone())
}

/// Результат поиска в общем виде: нормализованные название и исполнители, картинка ~500 px.
struct Item {
    title: String,
    artists: Vec<String>,
    image: Option<String>,
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn https(url: &str) -> Option<String> {
    url.starts_with("https://").then(|| url.to_owned())
}

fn itunes_item(it: &Value, kind: CoverKind) -> Item {
    let title = if kind == CoverKind::Track { &it["trackName"] } else { &it["collectionName"] };
    // artworkUrl100 …/100x100bb.jpg — размер задаётся в адресе
    let image = text(&it["artworkUrl100"]).replace("100x100bb", "600x600bb");
    Item { title: normalize(text(title)), artists: vec![normalize(text(&it["artistName"]))], image: https(&image) }
}

fn deezer_item(it: &Value, kind: CoverKind) -> Item {
    let (title, artist, image) = match kind {
        CoverKind::Album => (&it["title"], &it["artist"]["name"], &it["cover_big"]),
        CoverKind::Track => (&it["title"], &it["artist"]["name"], &it["album"]["cover_big"]),
        CoverKind::Artist => (&it["name"], &it["name"], &it["picture_big"]),
    };
    // у исполнителя без фото Deezer отдаёт заглушку с пустым id: …/images/artist//…
    let image = https(text(image)).filter(|url| !url.contains("//500x500") && !url.contains("/artist//"));
    Item { title: normalize(text(title)), artists: vec![normalize(text(artist))], image }
}

fn yandex_item(it: &Value, kind: CoverKind) -> Item {
    let (title, artists, cover) = match kind {
        CoverKind::Artist => (&it["name"], vec![normalize(text(&it["name"]))], &it["cover"]["uri"]),
        _ => {
            let artists = it["artists"].as_array().map(|list| list.iter().map(|a| normalize(text(&a["name"]))).collect());
            let cover = if it["coverUri"].is_string() { &it["coverUri"] } else { &it["albums"][0]["coverUri"] };
            (&it["title"], artists.unwrap_or_default(), cover)
        }
    };
    // coverUri: "avatars.yandex.net/get-music-content/…/%%" — %% заменяется размером
    let cover = text(cover);
    let image = (!cover.is_empty()).then(|| format!("https://{}", cover.replace("%%", "600x600")));
    Item { title: normalize(text(title)), artists, image }
}

/// Для сравнения: строчные буквы и цифры, остальное — один пробел.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            out.push(ch);
            gap = false;
        } else {
            gap = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deezer_album(title: &str, artist: &str, cover: &str) -> Value {
        json!({ "title": title, "artist": { "name": artist }, "cover_big": cover })
    }

    #[test]
    fn album_needs_the_same_artist_and_title() {
        let reply = json!({ "data": [
            deezer_album("Random Access Memories (Drumless Edition)", "Daft Punk", "https://x/drumless.jpg"),
            deezer_album("Random Access Memories", "Daft Punk Tribute", "https://x/tribute.jpg"),
            deezer_album("Random Access Memories", "Daft Punk", "https://x/ram.jpg"),
        ]});
        let pick = |artist, title| pick(Source::Deezer, &reply, CoverKind::Album, artist, title);
        assert_eq!(pick("Daft Punk", "Random Access Memories").as_deref(), Some("https://x/ram.jpg"));
        assert_eq!(pick("Justice", "Random Access Memories"), None);
    }

    #[test]
    fn album_title_may_continue() {
        let reply = json!({ "data": [deezer_album("Discovery (Remastered)", "Daft Punk", "https://x/d.jpg")] });
        assert_eq!(pick(Source::Deezer, &reply, CoverKind::Album, "daft punk", "Discovery").as_deref(), Some("https://x/d.jpg"));
    }

    #[test]
    fn cyrillic_and_punctuation_compare_loosely() {
        let reply = json!({ "data": [deezer_album("Платина", "Платина", "https://x/p.jpg")] });
        assert_eq!(pick(Source::Deezer, &reply, CoverKind::Album, "ПЛАТИНА", "платина!").as_deref(), Some("https://x/p.jpg"));
        assert_eq!(normalize("  AC/DC — Back in Black "), "ac dc back in black");
    }

    #[test]
    fn itunes_album_and_song_get_a_600px_cover() {
        let album = json!({ "results": [{ "collectionName": "Discovery", "artistName": "Daft Punk",
            "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/a/b.jpg/100x100bb.jpg" }] });
        assert_eq!(
            pick(Source::Itunes, &album, CoverKind::Album, "Daft Punk", "Discovery").as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/a/b.jpg/600x600bb.jpg")
        );
        let song = json!({ "results": [{ "trackName": "One More Time", "collectionName": "Discovery", "artistName": "Daft Punk",
            "artworkUrl100": "https://x/100x100bb.jpg" }] });
        assert_eq!(pick(Source::Itunes, &song, CoverKind::Track, "Daft Punk", "One More Time").as_deref(), Some("https://x/600x600bb.jpg"));
        assert!(!Source::Itunes.serves(CoverKind::Artist));
    }

    #[test]
    fn yandex_album_track_and_artist() {
        let albums = json!({ "result": { "albums": { "results": [
            { "title": "Discovery", "artists": [{ "name": "Daft Punk" }], "coverUri": "avatars.yandex.net/get-music-content/1/a/%%" }
        ]}}});
        assert_eq!(
            pick(Source::Yandex, &albums, CoverKind::Album, "Daft Punk", "Discovery").as_deref(),
            Some("https://avatars.yandex.net/get-music-content/1/a/600x600")
        );
        let tracks = json!({ "result": { "tracks": { "results": [
            { "title": "One More Time", "artists": [{ "name": "Daft Punk" }], "albums": [{ "coverUri": "avatars.yandex.net/t/%%" }] }
        ]}}});
        assert_eq!(pick(Source::Yandex, &tracks, CoverKind::Track, "daft punk", "one more time").as_deref(), Some("https://avatars.yandex.net/t/600x600"));
        let artists = json!({ "result": { "artists": { "results": [
            { "name": "Daft Punk", "cover": { "uri": "avatars.yandex.net/p/%%" } }
        ]}}});
        assert_eq!(pick(Source::Yandex, &artists, CoverKind::Artist, "Daft Punk", "").as_deref(), Some("https://avatars.yandex.net/p/600x600"));
    }

    #[test]
    fn deezer_artist_without_picture_is_skipped() {
        let reply = json!({ "data": [{ "name": "Daft Punk", "picture_big": "https://e-cdns-images.dzcdn.net/images/artist//500x500-000000-80-0-0.jpg" }] });
        assert_eq!(pick(Source::Deezer, &reply, CoverKind::Artist, "Daft Punk", ""), None);
    }

    #[test]
    fn empty_or_broken_reply_finds_nothing() {
        for source in SOURCES {
            assert_eq!(pick(source, &json!({}), CoverKind::Album, "a", "b"), None);
        }
    }

    #[test]
    fn a_source_out_of_budget_is_skipped_until_the_window_moves() {
        let mut gate = Gate::default();
        let start = Instant::now();
        for _ in 0..3 {
            assert!(gate.take(Source::Yandex, start));
        }
        assert!(!gate.take(Source::Yandex, start));
        assert!(gate.take(Source::Yandex, start + Duration::from_millis(1100)));
        gate.paused_until = Some(start + Duration::from_secs(30));
        assert!(!gate.take(Source::Yandex, start + Duration::from_secs(5)));
    }
}
