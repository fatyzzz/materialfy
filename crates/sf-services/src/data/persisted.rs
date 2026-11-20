//! Запрос к GraphQL-шлюзу клиентов Spotify (pathfinder) по хэшу сохранённого запроса, с токеном сессии
//! librespot (login5). Spotify со временем отзывает хэши (ответ PersistedQueryNotFound) — тогда хэши
//! заново читаются из бандла веб-плеера open.spotify.com: основной web-player.*.js и чанки маршрутов
//! объявляют каждую операцию как `new X("имя","query","sha256",null)`. Найденное хранится на диске
//! (mf_platform state dir); хэши в коде — семена, с ними всё работает и без чтения бандла.
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sf_api::Http;

use crate::data::fsio;
use crate::lock;

const URL: &str = "https://api-partner.spotify.com/pathfinder/v1/query";
const WEB_PLAYER: &str = "https://open.spotify.com/";
/// pathfinder отвечает 403 «Client/request not allowed» на клиент, не похожий на Spotify (User-Agent
/// «Materialfy/…», curl), — запросы идут от имени браузера, как у веб-плеера, чьи хэши и используются.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const TIMEOUT: Duration = Duration::from_secs(10);
const BUNDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Бандл — мегабайты: перечитывать не чаще, даже если отозванные хэши идут подряд.
const DISCOVERY_PAUSE: Duration = Duration::from_secs(30 * 60);
/// Сколько запрос ждёт чтения бандла (само чтение продолжается и без него).
const DISCOVERY_WAIT: Duration = Duration::from_secs(15);
/// Чанков веб-плеера за одно чтение бандла, не больше.
const MAX_CHUNKS: usize = 60;
const FILE: &str = "pathfinder-hashes.json";

/// Найденные в бандле хэши: операция → sha256.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Found {
    #[serde(default)]
    operations: BTreeMap<String, String>,
}

#[derive(Default)]
struct Book {
    /// None — файл ещё не читали
    found: Option<Found>,
    /// хэши, на которые шлюз ответил PersistedQueryNotFound в этом запуске
    retired: HashSet<String>,
    /// последнее чтение бандла
    discovered_at: Option<Instant>,
}

static BOOK: LazyLock<Mutex<Book>> = LazyLock::new(Mutex::default);
/// Одно чтение бандла за раз: остальные ждут его и берут найденное.
static DISCOVERY: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

fn file() -> PathBuf {
    mf_platform::dirs::state_dir().join(FILE)
}

fn load(path: &Path) -> Found {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(path: &Path, found: &Found) {
    let result = serde_json::to_vec_pretty(found).map_err(std::io::Error::other).and_then(|bytes| fsio::write_atomic(path, &bytes));
    if let Err(error) = result {
        tracing::debug!("pathfinder hashes not saved: {error}");
    }
}

/// Хэш с диска (если есть), затем семена; отозванные в этом запуске — мимо.
fn candidates(book: &mut Book, operation: &str, seeds: &[&str]) -> Vec<String> {
    let found = book.found.get_or_insert_with(|| load(&file()));
    let mut list: Vec<String> = Vec::new();
    for hash in found.operations.get(operation).map(String::as_str).into_iter().chain(seeds.iter().copied()) {
        if !book.retired.contains(hash) && !list.iter().any(|known| known == hash) {
            list.push(hash.to_owned());
        }
    }
    list
}

/// Ответ шлюза на операцию `operation` (тело целиком: data и, возможно, errors).
pub async fn query(
    http: &Http,
    session: &sf_player::Session,
    operation: &str,
    seeds: &[&str],
    variables: &Value,
) -> anyhow::Result<Value> {
    let token = session.login5().auth_token().await.map_err(|error| anyhow!("login5: {error}"))?;
    let client = http.client().map_err(|error| anyhow!(error))?;
    let variables = variables.to_string();
    let hashes = candidates(&mut lock(&BOOK), operation, seeds);
    for hash in &hashes {
        if let Some(body) = ask(&client, &token.access_token, operation, hash, &variables).await? {
            return Ok(body);
        }
        lock(&BOOK).retired.insert(hash.clone());
    }
    // все известные хэши отозваны — свежий из бандла веб-плеера
    let discovered = tokio::time::timeout(DISCOVERY_WAIT, discover(&client, operation)).await.ok().flatten();
    if let Some(hash) = discovered.filter(|hash| !hashes.contains(hash))
        && let Some(body) = ask(&client, &token.access_token, operation, &hash, &variables).await?
    {
        return Ok(body);
    }
    Err(anyhow!("{operation}: persisted query hashes retired (PersistedQueryNotFound)"))
}

/// Один запрос; None — хэш отозван.
async fn ask(
    client: &reqwest::Client,
    token: &str,
    operation: &str,
    hash: &str,
    variables: &str,
) -> anyhow::Result<Option<Value>> {
    let extensions = json!({ "persistedQuery": { "version": 1, "sha256Hash": hash } }).to_string();
    let response = client
        .get(URL)
        .query(&[("operationName", operation), ("variables", variables), ("extensions", &extensions)])
        .bearer_auth(token)
        .header("app-platform", "WebPlayer")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(TIMEOUT)
        .send()
        .await
        .with_context(|| format!("pathfinder {operation}"))?;
    let status = response.status();
    let body: Value = response.json().await.with_context(|| format!("pathfinder {operation} {status}"))?;
    if retired(&body) {
        tracing::debug!("pathfinder {operation} {hash:.8}: PersistedQueryNotFound");
        return Ok(None);
    }
    if !status.is_success() {
        return Err(anyhow!("pathfinder {operation} {status}: {body}"));
    }
    Ok(Some(body))
}

fn retired(body: &Value) -> bool {
    body.get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| errors.iter().any(|e| e.get("message").and_then(Value::as_str) == Some("PersistedQueryNotFound")))
}

/// Свежий хэш операции из бандла веб-плеера (не чаще DISCOVERY_PAUSE); найденное — на диск. Чтение идёт
/// отдельной задачей: запрос, который перестал ждать (страница не ждёт шлюз дольше пары секунд), не обрывает
/// его, и найденное достаётся следующим запросам.
async fn discover(client: &reqwest::Client, operation: &str) -> Option<String> {
    let (client, operation) = (client.clone(), operation.to_owned());
    tokio::spawn(async move { discover_now(&client, &operation).await }).await.ok().flatten()
}

async fn discover_now(client: &reqwest::Client, operation: &str) -> Option<String> {
    let _one = DISCOVERY.lock().await;
    {
        let mut book = lock(&BOOK);
        // пока ждали, бандл прочитал другой запрос
        let fresh = book.found.as_ref().and_then(|found| found.operations.get(operation)).cloned();
        if let Some(hash) = fresh.filter(|hash| !book.retired.contains(hash)) {
            return Some(hash);
        }
        if book.discovered_at.is_some_and(|at| at.elapsed() < DISCOVERY_PAUSE) {
            return None;
        }
        book.discovered_at = Some(Instant::now());
    }
    let read = match read_bundle(client, operation).await {
        Ok(read) => read,
        Err(error) => {
            tracing::debug!("web player bundle not read: {error:#}");
            return None;
        }
    };
    let hash = read.get(operation).cloned();
    tracing::debug!("web player bundle: {} operations, {operation}: {:?}", read.len(), hash.as_deref().map(|h| &h[..8]));
    let found = {
        let mut book = lock(&BOOK);
        let found = book.found.get_or_insert_with(Found::default);
        found.operations.extend(read);
        found.clone()
    };
    save(&file(), &found);
    hash
}

async fn fetch(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(BUNDLE_TIMEOUT)
        .send()
        .await
        .with_context(|| url.to_owned())?
        .error_for_status()?;
    Ok(response.text().await?)
}

/// Все операции основного бандла и чанков — пока не найдётся `wanted` (чанки, чьё имя похоже на
/// операцию, — первыми).
async fn read_bundle(client: &reqwest::Client, wanted: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let page = fetch(client, WEB_PLAYER).await?;
    let scripts = main_scripts(&page);
    anyhow::ensure!(!scripts.is_empty(), "no web-player.*.js on open.spotify.com");
    let mut operations = BTreeMap::new();
    let mut chunks = Vec::new();
    for script in &scripts {
        let js = fetch(client, script).await?;
        operations.extend(triples(&js));
        let base = &script[..script.rfind('/').map_or(0, |slash| slash + 1)];
        chunks.extend(chunk_files(&js).into_iter().map(|(name, file)| (name, format!("{base}{file}"))));
    }
    chunks.sort_by_key(|(name, _)| std::cmp::Reverse(likeness(name, wanted)));
    for (_, url) in chunks.iter().take(MAX_CHUNKS) {
        if operations.contains_key(wanted) {
            break;
        }
        match fetch(client, url).await {
            Ok(js) => operations.extend(triples(&js)),
            Err(error) => tracing::debug!("web player chunk: {error:#}"),
        }
    }
    Ok(operations)
}

/// Основные скрипты веб-плеера на странице open.spotify.com (…/web-player/web-player.<хэш>.js).
fn main_scripts(page: &str) -> Vec<String> {
    let mut scripts = Vec::new();
    for piece in page.split("src=\"").skip(1) {
        let Some(url) = piece.split('"').next() else { continue };
        let name = url.rsplit('/').next().unwrap_or_default();
        if url.starts_with("https://") && name.starts_with("web-player.") && name.ends_with(".js") {
            scripts.push(url.to_owned());
        }
    }
    scripts
}

/// Объявления операций: `"имя","query","sha256"` (и mutation).
fn triples(js: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for marker in ["\",\"query\",\"", "\",\"mutation\",\""] {
        let mut from = 0;
        while let Some(at) = js[from..].find(marker).map(|at| at + from) {
            from = at + marker.len();
            let hash = js.get(from..from + 64).filter(|hash| hash.bytes().all(|b| b.is_ascii_hexdigit()));
            let closed = js.as_bytes().get(from + 64) == Some(&b'"');
            let start = js[..at].rfind('"').map(|quote| quote + 1);
            let name = start.map(|start| &js[start..at]).filter(|name| {
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            });
            if let (Some(hash), true, Some(name)) = (hash, closed, name) {
                found.push((name.to_owned(), hash.to_ascii_lowercase()));
            }
        }
    }
    found
}

/// Файлы JS-чанков из загрузчика webpack: `(({id:"имя",…})[e]||e)+"."+({id:"хэш",…})[e]+".js"`.
/// Такой же загрузчик CSS (`+".css"`) — мимо.
fn chunk_files(js: &str) -> Vec<(String, String)> {
    const JOIN: &str = "[e]||e)+\".\"+({";
    let mut files = Vec::new();
    let mut from = 0;
    while let Some(at) = js[from..].find(JOIN).map(|at| at + from) {
        from = at + JOIN.len();
        let Some(end) = js[from..].find('}').map(|end| end + from) else { break };
        if !js[end..].starts_with("})[e]+\".js\"") {
            continue;
        }
        let hashes = pairs(&js[from..end]);
        let names = js[..at].rfind("({").map(|open| pairs(js[open + 2..at].trim_end_matches(['}', ')']))).unwrap_or_default();
        for (id, hash) in hashes {
            let name = names.iter().find(|(known, _)| *known == id).map_or(id.clone(), |(_, name)| name.clone());
            files.push((name.clone(), format!("{name}.{hash}.js")));
        }
    }
    files
}

/// `1328:"a",1401:"b"` → [(1328, a), (1401, b)].
fn pairs(map: &str) -> Vec<(String, String)> {
    map.split(',')
        .filter_map(|pair| {
            let (id, value) = pair.split_once(':')?;
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some((id.trim_matches('"').to_owned(), value.to_owned()))
        })
        .collect()
}

/// Насколько имя чанка похоже на операцию: «xpui-routes-search» → searchDesktop.
fn likeness(chunk: &str, operation: &str) -> usize {
    const GENERIC: [&str; 4] = ["xpui", "routes", "desktop", "page"];
    let operation = operation.to_ascii_lowercase();
    chunk
        .split('-')
        .filter(|word| word.len() >= 4 && !GENERIC.contains(word) && operation.contains(word))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = r#"let u=new l.l("queryArtistOverview","query","9F8134EF565E78621F1E1793555BD6633C5AC144AE0F89604ED3AE3F80B3C8E6",null);
        let a=new l.l("addToLibrary","mutation","7c5a69420e2bfae3da5cc4e14cbc8bb3f6090f80afc00ffc179177f19be3f33d",null);
        x("broken","query","abc",null);
        u.u=e=>""+(({1328:"xpui-pip-mini-player",4406:"xpui-routes-search",7161:"xpui-routes-artist"})[e]||e)+"."+({1015:"9eee9cf3",1328:"59320cd9",4406:"0e343fae",7161:"aa11bb22"})[e]+".js",
        u.miniCssF=e=>""+(({1328:"xpui-pip-mini-player"})[e]||e)+"."+({1328:"2d152445"})[e]+".css""#;

    #[test]
    fn operations_are_read_from_the_bundle() {
        let found = triples(MAIN);
        assert_eq!(found.len(), 2, "a malformed declaration is passed over");
        assert_eq!(found[0], ("queryArtistOverview".into(), "9f8134ef565e78621f1e1793555bd6633c5ac144ae0f89604ed3ae3f80b3c8e6".into()));
        assert_eq!(found[1].0, "addToLibrary");
    }

    #[test]
    fn chunk_files_come_from_the_js_loader_only() {
        let files = chunk_files(MAIN);
        assert_eq!(files.len(), 4);
        assert!(files.contains(&("xpui-routes-search".into(), "xpui-routes-search.0e343fae.js".into())));
        // чанк без имени зовётся своим номером
        assert!(files.contains(&("1015".into(), "1015.9eee9cf3.js".into())));
        assert!(files.iter().all(|(_, file)| !file.contains("2d152445")), "css is not a script");
    }

    #[test]
    fn chunks_named_like_the_operation_are_read_first() {
        assert_eq!(likeness("xpui-routes-search", "searchDesktop"), 1);
        assert_eq!(likeness("xpui-routes-artist", "queryArtistOverview"), 1);
        assert_eq!(likeness("xpui-routes-search", "queryArtistOverview"), 0);
        assert_eq!(likeness("xpui-desktop-modals", "searchDesktop"), 0, "generic words do not count");
    }

    #[test]
    fn the_main_script_is_found_on_the_page() {
        let page = r#"<script src="https://open.spotifycdn.com/cdn/js/gtm.96d60fd6.js"></script>
            <script src="https://open.spotifycdn.com/cdn/build/web-player/web-player.dbd3aa01.js"></script>
            <script src="https://open.spotifycdn.com/cdn/build/web-player/vendor~web-player.9d1c96f6.js"></script>"#;
        assert_eq!(main_scripts(page), vec!["https://open.spotifycdn.com/cdn/build/web-player/web-player.dbd3aa01.js"]);
    }

    #[test]
    fn found_hashes_go_before_seeds_and_retired_ones_are_skipped() {
        let mut book = Book {
            found: Some(Found { operations: BTreeMap::from([("searchDesktop".into(), "new".into())]) }),
            ..Default::default()
        };
        assert_eq!(candidates(&mut book, "searchDesktop", &["old", "new"]), vec!["new", "old"]);
        book.retired.insert("new".into());
        assert_eq!(candidates(&mut book, "searchDesktop", &["old"]), vec!["old"]);
        assert_eq!(candidates(&mut book, "getTrack", &["t"]), vec!["t"]);
    }

    #[test]
    fn a_retired_answer_is_told_apart() {
        assert!(retired(&json!({ "errors": [{ "message": "PersistedQueryNotFound" }] })));
        assert!(!retired(&json!({ "errors": [{ "message": "Unauthorized" }] })));
        assert!(!retired(&json!({ "data": {} })));
    }

    #[test]
    fn found_hashes_survive_on_disk() {
        let dir = std::env::temp_dir().join(format!("sf-persisted-{:016x}", rand::random::<u64>()));
        let path = dir.join(FILE);
        assert_eq!(load(&path), Found::default(), "no file yet");
        let found = Found { operations: BTreeMap::from([("getTrack".into(), "abc".into())]) };
        save(&path, &found);
        assert_eq!(load(&path), found);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
