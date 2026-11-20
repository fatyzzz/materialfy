//! Главная и личное через сессию librespot — без квоты Web API. Проверено вживую (2026-10, Premium):
//! - GraphQL-шлюз (pathfinder v2, сохранённые запросы веб-плеера, токен login5 сессии; Accept-Language: ru —
//!   заголовки разделов по-русски):
//!   `home` — разделы главной веб-плеера («Рекомендуемые станции», «Любимые исполнители», «Лучшие треки»…);
//!   `browsePage` страницы «Для тебя» (spotify:page:0JQ5DAt0tbjZptfcdMSKl3) — daylist, «На повторе», миксы дня,
//!   миксы по жанрам/исполнителям/десятилетиям; у личных плейлистов атрибут madeFor.username, миксы дня
//!   приходят NotFound — только URI (подписи дочитывает session_reads::playlist);
//!   `userTopContent` — топ исполнителей и треков за 4 недели (у нового аккаунта пусто);
//!   `fetchEntitiesForRecentlyPlayed` — подписи и обложки по URI (трек, альбом, исполнитель, плейлист, шоу).
//! - spclient `/homeview/v1/home?platform=android&locale=ru` — главная мобильного клиента (JSON hubs, плоский
//!   body: заголовок раздела glue:sectionHeader, за ним карточки glue2:card). Запасной путь, если хэш `home`
//!   отозван.
//! - spclient `/recently-played/v3/user/{username}/recently-played` (как у веб-плеера) — недавние контексты;
//!   у проверенного аккаунта 404 и на spclient.wg — истории там нет, это читается как пустой список.
//!
//! Spotify отзывает хэши: свой запрос с Accept-Language пробует семя, а отозванное уходит в
//! [`super::persisted`] (свежий хэш из бандла веб-плеера; заголовки тогда по-английски).
use std::time::Duration;

use anyhow::{Context, anyhow};
use futures::StreamExt as _;
use serde_json::{Value, json};
use sf_api::Http;
use sf_api::models::{Album, Artist, ArtistRef, Image, Track};
use sf_player::Session;

use super::persisted;

const URL: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
/// Язык подписей Spotify — язык интерфейса приложения ("en" | "ru" | "pt").
pub fn locale() -> &'static str {
    mf_i18n::lang().code()
}
const TIMEOUT: Duration = Duration::from_secs(12);
/// Хэши сохранённых запросов веб-плеера (web-player.dbd3aa01.js и чанки, 2026-10).
const HOME_HASH: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";
const BROWSE_PAGE_HASH: &str = "f5c4e6d668f5716464a231c1cc8b22c1cbf6ad68b09929fd7de813a30581298b";
const TOP_HASH: &str = "49ee15704de4a7fdeac65a02db20604aa11e46f02e809c55d9a89f6db9754356";
const LOOKUP_HASH: &str = "cf5d2e94ffd82788470788ae1f6090cc3e9e774fb8fd383580634c6e6f50f7be";
/// Страница browse «Для тебя» (Made For You).
pub const MADE_FOR_YOU_PAGE: &str = "spotify:page:0JQ5DAt0tbjZptfcdMSKl3";
/// Карточек в разделе главной (веб-плеер просит 10).
const SECTION_ITEMS: u32 = 16;
/// URI в одном запросе подписей.
pub const LOOKUP_BATCH: usize = 50;
/// Сколько неразрешённых плейлистов (миксы дня) дочитывать через сессию одновременно.
const RESOLVE_CONCURRENCY: usize = 4;
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(15);
/// format плейлиста, дочитанного через сессию (миксы дня приходят из browse без него).
pub const RESOLVED: &str = "session";
/// Подложка «плитка»: картинка около 300 px.
const TILE_PX: u64 = 300;

/// Карточка Spotify: плейлист, альбом, исполнитель, выпуск, шоу или трек.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedCard {
    /// spotify:playlist|album|artist|episode|show|track:ID
    pub uri: String,
    /// "" — Spotify прислал только URI (NotFound), подписи надо дочитать
    pub title: String,
    /// плейлист — описание, альбом и трек — исполнители, выпуск — шоу, шоу — издатель
    pub subtitle: String,
    pub cover: String,
    /// владелец плейлиста ("spotify" у плейлистов Spotify)
    pub owner_id: String,
    pub owner: String,
    /// атрибут madeFor.username (Spotify ставит его и редакционным — сам по себе не признак личного)
    pub made_for: bool,
    /// format плейлиста: daylist, on-repeat, topic-mix, inspiredby-mix…
    pub format: String,
}

impl FeedCard {
    /// playlist | album | artist | episode | show | track | …
    pub fn kind(&self) -> &str {
        sf_api::util::uri_kind(&self.uri).unwrap_or("")
    }

    pub fn id(&self) -> &str {
        sf_api::util::uri_id(&self.uri).unwrap_or("")
    }
}

/// Раздел главной или страницы browse: заголовок Spotify и карточки по порядку.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedSection {
    /// spotify:section:…
    pub uri: String,
    pub title: String,
    pub cards: Vec<FeedCard>,
}

/// Недавний контекст (recently-played): что слушали и когда.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecentContext {
    pub uri: String,
    /// мс с эпохи
    pub played_at_ms: i64,
    pub last_track_uri: String,
}

// --- разбор --------------------------------------------------------------------------------------------

fn text(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

/// Картинка ближе всего к TILE_PX (размер неизвестен — подходит).
fn pick_image(sources: Option<&Value>) -> String {
    let Some(list) = sources.and_then(|s| s.get("sources")).and_then(Value::as_array) else { return String::new() };
    let distance = |s: &Value| s.get("width").and_then(Value::as_u64).map_or(0, |w| w.abs_diff(TILE_PX));
    list.iter()
        .filter(|s| s.get("url").and_then(Value::as_str).is_some_and(|u| u.starts_with("http")))
        .min_by_key(|s| distance(s))
        .map(|s| text(s, "/url"))
        .unwrap_or_default()
}

fn names(data: &Value, pointer: &str) -> String {
    data.pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| text(a, "/profile/name"))
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Описание плейлиста без HTML-ссылок Spotify (`<a href=…>Имя</a>`) и сущностей.
pub fn plain(description: &str) -> String {
    let mut out = String::with_capacity(description.len());
    let mut in_tag = false;
    for c in description.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&#x27;", "'").replace("&quot;", "\"").replace("&#39;", "'").trim().to_owned()
}

/// Сущность GraphQL (content.data / lookup.data) → карточка; `uri` — запасной (у NotFound в data его нет).
fn entity_card(data: &Value, uri: &str) -> Option<FeedCard> {
    let typename = data.get("__typename").and_then(Value::as_str).unwrap_or("");
    let own_uri = text(data, "/uri");
    let uri = if own_uri.is_empty() { uri.to_owned() } else { own_uri };
    if !uri.starts_with("spotify:") {
        return None;
    }
    let mut card = FeedCard { uri, ..Default::default() };
    match typename {
        "Playlist" => {
            card.title = text(data, "/name");
            card.subtitle = plain(&text(data, "/description"));
            card.cover = pick_image(data.pointer("/images/items/0"));
            let owner = data.pointer("/ownerV2/data").unwrap_or(&Value::Null);
            card.owner = text(owner, "/name");
            card.owner_id = Some(text(owner, "/username"))
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| text(owner, "/uri").rsplit(':').next().unwrap_or_default().to_owned());
            card.format = text(data, "/format");
            card.made_for = data
                .get("attributes")
                .and_then(Value::as_array)
                .is_some_and(|attrs| attrs.iter().any(|a| text(a, "/key") == "madeFor.username" && !text(a, "/value").is_empty()));
        }
        "Album" => {
            card.title = text(data, "/name");
            card.subtitle = names(data, "/artists/items");
            card.cover = pick_image(data.get("coverArt"));
        }
        "Artist" => {
            card.title = text(data, "/profile/name");
            card.cover = pick_image(data.pointer("/visuals/avatarImage"));
        }
        "Episode" => {
            card.title = text(data, "/name");
            card.subtitle = text(data, "/podcastV2/data/name");
            card.cover = pick_image(data.get("coverArt"));
        }
        "Podcast" => {
            card.title = text(data, "/name");
            card.subtitle = text(data, "/publisher/name");
            card.cover = pick_image(data.get("coverArt"));
        }
        "Track" => {
            card.title = text(data, "/name");
            card.subtitle = names(data, "/artists/items");
            card.cover = pick_image(data.pointer("/albumOfTrack/coverArt"));
        }
        // NotFound и прочее: только URI плейлиста (миксы дня) — подписи дочитаются
        _ if card.uri.starts_with("spotify:playlist:") => {}
        _ => return None,
    }
    Some(card)
}

/// Разделы GraphQL (home и browsePage одной формы): {uri, data.title.transformedLabel, sectionItems.items}.
fn graphql_sections(items: Option<&Value>) -> Vec<FeedSection> {
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|section| {
            let title = text(section, "/data/title/transformedLabel");
            let cards: Vec<FeedCard> = section
                .pointer("/sectionItems/items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| entity_card(item.pointer("/content/data").unwrap_or(&Value::Null), &text(item, "/uri")))
                .collect();
            (!cards.is_empty()).then(|| FeedSection { uri: text(section, "/uri"), title, cards })
        })
        .collect()
}

fn graphql_error(body: &Value, root: &str) -> anyhow::Result<()> {
    if body.pointer(root).is_some_and(|v| !v.is_null()) {
        return Ok(());
    }
    match body.get("errors") {
        Some(errors) => Err(anyhow!("GraphQL errors: {errors}")),
        None => Err(anyhow!("no {root}")),
    }
}

/// Ответ `home` → разделы главной.
pub fn parse_home(body: &Value) -> anyhow::Result<Vec<FeedSection>> {
    graphql_error(body, "/data/home")?;
    Ok(graphql_sections(body.pointer("/data/home/sectionContainer/sections/items")))
}

/// Ответ `browsePage` → разделы страницы.
pub fn parse_browse(body: &Value) -> anyhow::Result<Vec<FeedSection>> {
    graphql_error(body, "/data/browse")?;
    Ok(graphql_sections(body.pointer("/data/browse/sections/items")))
}

/// Ответ spclient homeview: плоский body — заголовок раздела, за ним его карточки.
pub fn parse_homeview(body: &Value) -> anyhow::Result<Vec<FeedSection>> {
    let nodes = body.get("body").and_then(Value::as_array).ok_or_else(|| anyhow!("homeview: no body"))?;
    let mut sections: Vec<FeedSection> = Vec::new();
    for node in nodes {
        let section = text(node, "/metadata/sectionId");
        let component = text(node, "/component/id");
        if component.ends_with("sectionHeader") {
            let uri = if section.is_empty() { text(node, "/id").trim_end_matches("-header").to_owned() } else { section };
            sections.push(FeedSection { uri, title: text(node, "/text/title"), cards: Vec::new() });
            continue;
        }
        let uri = text(node, "/target/uri");
        if !uri.starts_with("spotify:") {
            continue;
        }
        let card = FeedCard {
            title: text(node, "/text/title"),
            subtitle: plain(&text(node, "/text/subtitle")),
            cover: Some(text(node, "/images/main/uri")).filter(|u| u.starts_with("http")).unwrap_or_default(),
            // в homeview владельца нет: плейлисты «37i9dQZ…» — Spotify
            owner_id: if uri.starts_with("spotify:playlist:37i9dQZ") { "spotify".into() } else { String::new() },
            owner: if uri.starts_with("spotify:playlist:37i9dQZ") { "Spotify".into() } else { String::new() },
            uri,
            ..Default::default()
        };
        match sections.last_mut() {
            Some(last) if section.is_empty() || last.uri == section => last.cards.push(card),
            _ => sections.push(FeedSection { uri: section, title: String::new(), cards: vec![card] }),
        }
    }
    sections.retain(|s| !s.cards.is_empty());
    Ok(sections)
}

/// Ответ `fetchEntitiesForRecentlyPlayed` → карточки (неизвестные Spotify URI пропускаются).
pub fn parse_lookup(body: &Value) -> anyhow::Result<Vec<FeedCard>> {
    graphql_error(body, "/data/lookup")?;
    Ok(body
        .pointer("/data/lookup")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| entity_card(item.get("data").unwrap_or(&Value::Null), &text(item, "/_uri")))
        .filter(|card| !card.title.is_empty())
        .collect())
}

fn sf_images(sources: Option<&Value>) -> Vec<Image> {
    let size = |s: &Value, key: &str| s.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    sources
        .and_then(|s| s.get("sources"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| Some(Image { url: s.get("url")?.as_str()?.to_owned(), width: size(s, "width"), height: size(s, "height") }))
        .collect()
}

fn id_of(uri: &str) -> String {
    uri.rsplit(':').next().unwrap_or_default().to_owned()
}

fn top_track(data: &Value) -> Option<Track> {
    let uri = text(data, "/uri");
    if data.get("__typename").and_then(Value::as_str) != Some("Track") || !uri.starts_with("spotify:track:") {
        return None;
    }
    let album = data.get("albumOfTrack").filter(|a| !a.is_null()).map(|album| {
        let uri = text(album, "/uri");
        Album { id: id_of(&uri), uri, name: text(album, "/name"), images: sf_images(album.get("coverArt")), ..Default::default() }
    });
    let artists = data
        .pointer("/artists/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| {
            let uri = text(a, "/uri");
            ArtistRef { id: Some(id_of(&uri)).filter(|id| !id.is_empty()), name: text(a, "/profile/name"), uri: Some(uri) }
        })
        .collect();
    Some(Track {
        id: Some(id_of(&uri)),
        name: text(data, "/name"),
        duration_ms: data.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).map_or(0, |ms| ms as u32),
        explicit: text(data, "/contentRating/label") == "EXPLICIT",
        artists,
        album,
        is_playable: data.pointer("/playability/playable").and_then(Value::as_bool),
        uri,
        ..Default::default()
    })
}

fn top_artist(data: &Value) -> Option<Artist> {
    let uri = text(data, "/uri");
    if data.get("__typename").and_then(Value::as_str) != Some("Artist") || !uri.starts_with("spotify:artist:") {
        return None;
    }
    Some(Artist {
        id: id_of(&uri),
        name: text(data, "/profile/name"),
        images: sf_images(data.pointer("/visuals/avatarImage")),
        uri,
        ..Default::default()
    })
}

/// Ответ `userTopContent` → (исполнители, треки) в форме Web API.
pub fn parse_top(body: &Value) -> anyhow::Result<(Vec<Artist>, Vec<Track>)> {
    graphql_error(body, "/data/me/profile")?;
    let items = |pointer: &str| -> Vec<Value> {
        body.pointer(pointer).and_then(Value::as_array).cloned().unwrap_or_default()
    };
    let artists = items("/data/me/profile/topArtists/items").iter().filter_map(|i| top_artist(i.get("data").unwrap_or(i))).collect();
    let tracks = items("/data/me/profile/topTracks/items").iter().filter_map(|i| top_track(i.get("data").unwrap_or(i))).collect();
    Ok((artists, tracks))
}

/// Ответ recently-played: {"playContexts": [{uri, lastPlayedTime, lastPlayedTrackUri}]}, новые первыми.
pub fn parse_recently_played(body: &Value) -> Vec<RecentContext> {
    let mut list: Vec<RecentContext> = body
        .get("playContexts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let uri = text(c, "/uri");
            let at = c.get("lastPlayedTime").and_then(|t| t.as_i64().or_else(|| t.as_str()?.parse().ok()))?;
            uri.starts_with("spotify:").then(|| RecentContext { uri, played_at_ms: at, last_track_uri: text(c, "/lastPlayedTrackUri") })
        })
        .collect();
    list.sort_by_key(|c| std::cmp::Reverse(c.played_at_ms));
    list
}

// --- сеть ----------------------------------------------------------------------------------------------

/// Запрос к шлюзу с Accept-Language; хэш отозван — через persisted (свежий хэш из бандла).
async fn query(http: &Http, session: &Session, operation: &str, hash: &str, variables: Value) -> anyhow::Result<Value> {
    let token = session.login5().auth_token().await.map_err(|error| anyhow!("login5: {error}"))?;
    let client = http.client().map_err(|error| anyhow!(error))?;
    let body = json!({
        "operationName": operation,
        "variables": variables,
        "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } },
    });
    let response = client
        .post(URL)
        .bearer_auth(&token.access_token)
        .header("app-platform", "WebPlayer")
        .header(reqwest::header::USER_AGENT, persisted::USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_LANGUAGE, locale())
        .json(&body)
        .timeout(TIMEOUT)
        .send()
        .await
        .with_context(|| format!("pathfinder {operation}"))?;
    let status = response.status();
    let answer: Value = response.json().await.with_context(|| format!("pathfinder {operation} {status}"))?;
    let retired = answer
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| errors.iter().any(|e| text(e, "/message") == "PersistedQueryNotFound"));
    if retired {
        tracing::debug!("pathfinder {operation}: seed hash retired, looking for a fresh one");
        return persisted::query(http, session, operation, &[], &variables).await;
    }
    anyhow::ensure!(status.is_success(), "pathfinder {operation} {status}: {answer}");
    Ok(answer)
}

fn time_zone() -> String {
    jiff::tz::TimeZone::system().iana_name().unwrap_or("UTC").to_owned()
}

/// Главная Spotify аккаунта сессии: GraphQL `home`, при отказе — spclient homeview.
pub async fn home(http: &Http, session: &Session) -> anyhow::Result<Vec<FeedSection>> {
    let variables = json!({
        "timeZone": time_zone(),
        "sp_t": "",
        "facet": "",
        "sectionItemsLimit": SECTION_ITEMS,
        "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
        "includeHomeChapterVideoCards": false,
        "includeEpisodeContentRatingsV2": false,
    });
    let graphql = async { parse_home(&query(http, session, "home", HOME_HASH, variables).await?) };
    match graphql.await {
        Ok(sections) if !sections.is_empty() => Ok(sections),
        Ok(_) => homeview(session).await,
        Err(error) => {
            tracing::debug!("home via GraphQL: {error:#}; falling back to homeview");
            homeview(session).await.map_err(|e| e.context(format!("GraphQL home: {error:#}")))
        }
    }
}

/// Главная мобильного клиента (spclient homeview).
pub async fn homeview(session: &Session) -> anyhow::Result<Vec<FeedSection>> {
    let path = format!("/homeview/v1/home?platform=android&locale={}", locale());
    let read = session.spclient().request_as_json(&reqwest::Method::GET, &path, None, None);
    let bytes = tokio::time::timeout(TIMEOUT, read).await.map_err(|_| anyhow!("homeview: timeout"))??;
    parse_homeview(&serde_json::from_slice(&bytes)?)
}

/// Spotify's DJ: a playlist-like context made for each listener, offered in some markets only.
pub const DJ_URI: &str = "spotify:playlist:37i9dQZF1EYkqdzj48dyYq";

/// The DJ's cover when this account has the DJ, else `None`. Spotify's own
/// home feed for the web player leaves the DJ out, so this asks for a DJ
/// session: it only has tracks where the DJ is offered.
pub async fn dj_cover(session: &Session) -> Option<String> {
    let url = sf_player::SpClient::context_session_url(DJ_URI);
    let read = session.spclient().get_context_from_url(&url);
    let context = match tokio::time::timeout(TIMEOUT, read).await {
        Ok(Ok(context)) => context,
        Ok(Err(error)) => {
            tracing::debug!("no DJ for this account: {error}");
            return None;
        }
        Err(_) => return None,
    };
    let mut tracks = context.pages.iter().flat_map(|page| page.tracks.iter()).peekable();
    tracks.peek()?;
    let cover = tracks
        .find_map(|track| track.metadata.get("narration.intro.image").filter(|image| image.starts_with("https://")))
        .cloned();
    Some(cover.unwrap_or_else(|| DJ_COVER.to_owned()))
}

const DJ_COVER: &str = "https://lexicon-assets.spotifycdn.com/Your-DJ-Cover-Art-640.png";

/// Страница «Для тебя»: личные плейлисты Spotify по разделам.
pub async fn made_for_you(http: &Http, session: &Session) -> anyhow::Result<Vec<FeedSection>> {
    let variables = json!({
        "pagePagination": { "offset": 0, "limit": 20 },
        "sectionPagination": { "offset": 0, "limit": SECTION_ITEMS },
        "uri": MADE_FOR_YOU_PAGE,
        "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
        "includeEpisodeContentRatingsV2": false,
    });
    parse_browse(&query(http, session, "browsePage", BROWSE_PAGE_HASH, variables).await?)
}

/// Топ исполнителей и треков за 4 недели (как «Топ» профиля веб-плеера).
pub async fn top(http: &Http, session: &Session, limit: u32) -> anyhow::Result<(Vec<Artist>, Vec<Track>)> {
    let input = json!({ "offset": 0, "limit": limit, "sortBy": "AFFINITY", "timeRange": "SHORT_TERM" });
    let variables = json!({
        "includeTopArtists": true,
        "topArtistsInput": input,
        "includeTopTracks": true,
        "topTracksInput": input,
    });
    parse_top(&query(http, session, "userTopContent", TOP_HASH, variables).await?)
}

/// Подписи и обложки по URI (≤ LOOKUP_BATCH за запрос).
pub async fn lookup(http: &Http, session: &Session, uris: &[String]) -> anyhow::Result<Vec<FeedCard>> {
    let mut cards = Vec::new();
    for chunk in uris.chunks(LOOKUP_BATCH) {
        let body = query(http, session, "fetchEntitiesForRecentlyPlayed", LOOKUP_HASH, json!({ "uris": chunk })).await?;
        cards.extend(parse_lookup(&body)?);
    }
    Ok(cards)
}

/// Недавние контексты аккаунта сессии; 404 (истории нет) — пустой список.
pub async fn recently_played(session: &Session, limit: u32) -> anyhow::Result<Vec<RecentContext>> {
    let user = session.username();
    let path = format!(
        "/recently-played/v3/user/{user}/recently-played?format=json&offset=0&limit={limit}&filter=default,collection-new-episodes"
    );
    let read = session.spclient().request_as_json(&reqwest::Method::GET, &path, None, None);
    match tokio::time::timeout(TIMEOUT, read).await.map_err(|_| anyhow!("recently-played: timeout"))? {
        Ok(bytes) => Ok(parse_recently_played(&serde_json::from_slice(&bytes)?)),
        // librespot_core не в зависимостях sf-services: вид ошибки — по имени
        Err(error) if format!("{:?}", error.kind) == "NotFound" => Ok(Vec::new()),
        Err(error) => Err(anyhow!("recently-played: {error}")),
    }
}

/// Дочитать через сессию плейлисты без подписей (миксы дня приходят из browse только URI).
pub async fn resolve_playlists(session: &Session, cards: Vec<FeedCard>) -> Vec<FeedCard> {
    futures::stream::iter(cards)
        .map(|mut card| async move {
            if !card.title.is_empty() || card.kind() != "playlist" {
                return card;
            }
            let read = sf_player::engine::session_reads::playlist(session, card.id());
            if let Ok(Ok(playlist)) = tokio::time::timeout(RESOLVE_TIMEOUT, read).await {
                card.title = playlist.name.clone();
                card.subtitle = plain(playlist.description.as_deref().unwrap_or(""));
                card.cover = playlist.images.first().map(|i| i.url.clone()).unwrap_or_default();
                card.owner_id = playlist.owner.id.clone().unwrap_or_default();
                card.owner = playlist.owner.display_name.clone().unwrap_or_default();
                card.format = RESOLVED.to_owned();
            }
            card
        })
        .buffered(RESOLVE_CONCURRENCY)
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(url: &str, w: Option<u64>) -> Value {
        json!({ "url": url, "width": w, "height": w })
    }

    #[test]
    fn graphql_home_reads_sections_and_cards() {
        let body = json!({ "data": { "home": { "sectionContainer": { "sections": { "items": [
            { "uri": "spotify:section:st", "data": { "title": { "transformedLabel": "Рекомендуемые станции" } },
              "sectionItems": { "items": [
                { "uri": "spotify:playlist:37i9dQZF1E4x", "content": { "__typename": "PlaylistResponseWrapper", "data": {
                    "__typename": "Playlist", "uri": "spotify:playlist:37i9dQZF1E4x", "name": "IMANU Radio",
                    "description": "With <a href=\"spotify:artist:1\">Noisia</a> &amp; more", "format": "inspiredby-mix",
                    "attributes": [{ "key": "madeFor.username", "value": "fatyzzz" }],
                    "images": { "items": [{ "sources": [src("https://big", Some(640)), src("https://mid", Some(300))] }] },
                    "ownerV2": { "data": { "__typename": "User", "name": "Spotify", "uri": "spotify:user:spotify" } } } } },
                { "uri": "spotify:artist:a1", "content": { "__typename": "ArtistResponseWrapper", "data": {
                    "__typename": "Artist", "uri": "spotify:artist:a1", "profile": { "name": "IMANU" },
                    "visuals": { "avatarImage": { "sources": [src("https://face", Some(320))] } } } } },
                { "uri": "spotify:concert:x", "content": { "__typename": "UnknownTypeWrapper" } }
              ] } },
            { "uri": "spotify:section:empty", "data": { "title": { "transformedLabel": "Пусто" } }, "sectionItems": { "items": [] } }
        ] } } } } });
        let sections = parse_home(&body).unwrap();
        assert_eq!(sections.len(), 1, "empty sections are skipped");
        let s = &sections[0];
        assert_eq!((s.uri.as_str(), s.title.as_str()), ("spotify:section:st", "Рекомендуемые станции"));
        assert_eq!(s.cards.len(), 2, "unknown entities are skipped");
        let radio = &s.cards[0];
        assert_eq!((radio.kind(), radio.id()), ("playlist", "37i9dQZF1E4x"));
        assert_eq!(radio.subtitle, "With Noisia & more");
        assert_eq!(radio.cover, "https://mid", "closest to 300 px");
        assert!(radio.made_for);
        assert_eq!((radio.owner.as_str(), radio.owner_id.as_str(), radio.format.as_str()), ("Spotify", "spotify", "inspiredby-mix"));
        assert_eq!((s.cards[1].kind(), s.cards[1].title.as_str(), s.cards[1].cover.as_str()), ("artist", "IMANU", "https://face"));
        assert!(parse_home(&json!({ "errors": [{ "message": "boom" }] })).is_err());
    }

    #[test]
    fn browse_keeps_unresolved_mixes_as_uris() {
        let body = json!({ "data": { "browse": { "sections": { "items": [
            { "uri": "spotify:section:mixes", "data": { "title": { "transformedLabel": "Твои Миксы дня" } },
              "sectionItems": { "items": [
                { "uri": "spotify:playlist:37i9dQZF1E35", "content": { "__typename": "PlaylistResponseWrapper", "data": { "__typename": "NotFound" } } },
                { "uri": "spotify:xlink:feature", "content": { "__typename": "BrowseClientFeature" } }
              ] } }
        ] } } } });
        let sections = parse_browse(&body).unwrap();
        assert_eq!(sections[0].cards.len(), 1);
        assert_eq!((sections[0].cards[0].uri.as_str(), sections[0].cards[0].title.as_str()), ("spotify:playlist:37i9dQZF1E35", ""));
    }

    #[test]
    fn homeview_groups_cards_under_headers() {
        let body = json!({ "body": [
            { "id": "spotify:section:mfy-header", "component": { "id": "glue:sectionHeader" }, "text": { "title": "Только для тебя, fatyzzz" },
              "metadata": { "sectionId": "spotify:section:mfy" } },
            { "id": "spotify:section:mfy_card0", "component": { "id": "glue2:card" },
              "text": { "title": "Открытия недели", "subtitle": "Каждый понедельник" },
              "images": { "main": { "uri": "https://dw" } }, "target": { "uri": "spotify:playlist:37i9dQZEVXcPdivXIgWGoc" },
              "metadata": { "sectionId": "spotify:section:mfy" } },
            { "id": "spotify:section:art-header", "component": { "id": "glue:sectionHeader" }, "text": { "title": "Популярные исполнители" },
              "metadata": { "sectionId": "spotify:section:art" } },
            { "id": "spotify:section:art_card0", "component": { "id": "glue2:card" }, "text": { "title": "Arijit Singh" },
              "images": { "main": { "uri": "https://face" } }, "target": { "uri": "spotify:artist:4Y" },
              "metadata": { "sectionId": "spotify:section:art" } }
        ] });
        let sections = parse_homeview(&body).unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!((sections[0].uri.as_str(), sections[0].title.as_str()), ("spotify:section:mfy", "Только для тебя, fatyzzz"));
        let dw = &sections[0].cards[0];
        assert_eq!((dw.title.as_str(), dw.owner_id.as_str(), dw.cover.as_str()), ("Открытия недели", "spotify", "https://dw"));
        assert_eq!(sections[1].cards[0].kind(), "artist");
    }

    #[test]
    fn top_content_reads_as_web_api() {
        let body = json!({ "data": { "me": { "profile": {
            "topArtists": { "items": [{ "data": { "__typename": "Artist", "uri": "spotify:artist:a1", "profile": { "name": "Muse" },
                "visuals": { "avatarImage": { "sources": [src("https://face", Some(640))] } } } }] },
            "topTracks": { "items": [{ "data": { "__typename": "Track", "uri": "spotify:track:t1", "name": "Uprising",
                "duration": { "totalMilliseconds": 305000 }, "contentRating": { "label": "EXPLICIT" },
                "artists": { "items": [{ "uri": "spotify:artist:a1", "profile": { "name": "Muse" } }] },
                "albumOfTrack": { "uri": "spotify:album:al1", "name": "The Resistance", "coverArt": { "sources": [src("https://cov", Some(300))] } } } }] }
        } } } });
        let (artists, tracks) = parse_top(&body).unwrap();
        assert_eq!((artists[0].id.as_str(), artists[0].name.as_str()), ("a1", "Muse"));
        let t = sf_api::map::track_item(&tracks[0]);
        assert_eq!((t.track_id.as_str(), t.album_id.as_str(), t.artists.as_str()), ("t1", "al1", "Muse"));
        assert_eq!(t.duration_ms, 305000);
        assert!(t.explicit);
        // новый аккаунт: пустые страницы
        let empty = json!({ "data": { "me": { "profile": { "topArtists": { "items": [] }, "topTracks": { "items": [] } } } } });
        assert_eq!(parse_top(&empty).unwrap(), (Vec::new(), Vec::new()));
    }

    #[test]
    fn lookup_and_recently_played() {
        let body = json!({ "data": { "lookup": [
            { "_uri": "spotify:track:t1", "data": { "__typename": "Track", "uri": "spotify:track:t1", "name": "Blinding Lights",
                "artists": { "items": [{ "profile": { "name": "The Weeknd" } }] },
                "albumOfTrack": { "coverArt": { "sources": [src("https://ah", Some(300))] } } } },
            { "_uri": "spotify:show:s1", "data": { "__typename": "Podcast", "uri": "spotify:show:s1", "name": "Without Fail",
                "publisher": { "name": "Gimlet" }, "coverArt": { "sources": [src("https://wf", None)] } } },
            { "_uri": "spotify:user:me:collection", "__typename": "UnknownTypeWrapper" }
        ] } });
        let cards = parse_lookup(&body).unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[0].title.as_str(), cards[0].subtitle.as_str(), cards[0].cover.as_str()), ("Blinding Lights", "The Weeknd", "https://ah"));
        assert_eq!((cards[1].kind(), cards[1].subtitle.as_str()), ("show", "Gimlet"));

        let recent = parse_recently_played(&json!({ "playContexts": [
            { "uri": "spotify:album:a", "lastPlayedTime": 1000, "lastPlayedTrackUri": "spotify:track:1" },
            { "uri": "spotify:playlist:p", "lastPlayedTime": "2000" },
            { "uri": "", "lastPlayedTime": 3000 }
        ] }));
        assert_eq!(recent.iter().map(|c| c.uri.as_str()).collect::<Vec<_>>(), ["spotify:playlist:p", "spotify:album:a"]);
        assert_eq!(recent[1].last_track_uri, "spotify:track:1");
    }
}
