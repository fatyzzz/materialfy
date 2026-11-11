//! Синхронные тексты — порт lyrics_sources.py из yamusic-md3 (+ кэш из lyrics.py).
//!
//! Цепочка источников (fetch): Spotify (внешний источник ExtraLrc — первым, как в spotifast; SYLLABLE_SYNCED —
//! по словам из слогов ответа, LINE_SYNCED — построчно) → NetEase YRC
//! (по словам) → Musixmatch richsync (по словам) → LRCLIB (строки) → Musixmatch subtitles → NetEase LRC.
//! У построчных текстов время слов интерполируется. Формат результата и кэша (JSON) — по ключам как в Python.
pub mod lrc;
pub mod sources;
pub mod text;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::future::BoxFuture;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use sha1::{Digest, Sha1};

use crate::lrc::{add_gaps, build_from_timed_lines, group_syllables, lines_from_lrc, parse_yrc, round3};
use crate::text::{is_meta_line, is_placeholder};

/// 4 — внешний LRC в конце цепочки; 5 — заглушки NetEase «текста нет» пропускаются;
/// 6 — LRCLIB: нечёткое название и разметка, с которой согласно большинство записей;
/// 7 — Spotify первым, id трека — base62 Spotify;
/// 8 — Spotify SYLLABLE_SYNCED: время слов из слогов ответа вместо интерполяции
pub const CACHE_VERSION: u32 = 9;
/// «Не найдено» перепроверяется через 6 часов
pub const NOT_FOUND_TTL_SECS: u64 = 6 * 3600;
/// Таймаут HTTP источников, с (2 повтора на 5xx/сеть с паузой 0,7·(n+1) с)
pub const TIMEOUT_SECS: u64 = 8;
/// Тишина ≥ 4,5 с — вставка «• • •» (gap)
pub const GAP_MIN: f64 = 4.5;
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// Что ищем. title уже с версией: "Title (Version)", если версия есть.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricsQuery {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// длительность трека, секунды
    pub duration: f64,
}

/// Слово: {"x": текст (с хвостовым пробелом), "s": начало, "e": конец, "p": [[символов, s, e], …]?}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub x: String,
    pub s: f64,
    pub e: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub p: Option<Vec<(u32, f64, f64)>>,
}

/// Строка текста {"t","e","w","i"} или пауза {"gap": true, "t", "e"} — порядок вариантов важен для untagged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    Gap { gap: bool, t: f64, e: f64 },
    Words { t: f64, e: f64, w: Vec<Word>, i: bool },
}

impl Line {
    pub fn words(t: f64, e: f64, w: Vec<Word>, i: bool) -> Self {
        Self::Words { t, e, w, i }
    }

    /// "t" — начало
    pub fn start(&self) -> f64 {
        match self {
            Self::Gap { t, .. } | Self::Words { t, .. } => *t,
        }
    }

    /// "e" — конец
    pub fn end(&self) -> f64 {
        match self {
            Self::Gap { e, .. } | Self::Words { e, .. } => *e,
        }
    }

    /// Текст строки (слова подряд); у паузы — пусто
    pub fn text(&self) -> String {
        match self {
            Self::Gap { .. } => String::new(),
            Self::Words { w, .. } => w.iter().map(|w| w.x.as_str()).collect(),
        }
    }
}

/// Результат fetch и содержимое файла кэша:
/// {"ok": true, "source": "Spotify"|"NetEase"|"Musixmatch"|"LRCLIB", "kind": "word"|"line", "lines": [...]}
/// или {"ok": false, "error": "not found", "details": ["источник: ошибка", …]}.
/// Сериализация — вручную, ровно этими двумя формами (как dict в Python).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LyricsResult {
    pub ok: bool,
    pub source: String,
    pub kind: String,
    pub lines: Vec<Line>,
    pub error: Option<String>,
    pub details: Vec<String>,
}

impl LyricsResult {
    pub fn found(source: &str, kind: &str, lines: Vec<Line>) -> Self {
        Self { ok: true, source: source.into(), kind: kind.into(), lines, ..Default::default() }
    }

    pub fn not_found(details: Vec<String>) -> Self {
        Self { ok: false, error: Some("not found".into()), details, ..Default::default() }
    }

    /// JSON как json.dumps(result, ensure_ascii=False) в Python (разделители ", " и ": ")
    pub fn to_python_json(&self) -> String {
        let mut out = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter);
        // запись в Vec не падает
        let _ = self.serialize(&mut ser);
        String::from_utf8(out).unwrap_or_default()
    }
}

impl Serialize for LyricsResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("ok", &self.ok)?;
        if self.ok {
            map.serialize_entry("source", &self.source)?;
            map.serialize_entry("kind", &self.kind)?;
            map.serialize_entry("lines", &self.lines)?;
        } else {
            map.serialize_entry("error", self.error.as_deref().unwrap_or("not found"))?;
            map.serialize_entry("details", &self.details)?;
        }
        map.end()
    }
}

/// Разделители json.dumps по умолчанию
struct PyFormatter;

impl serde_json::ser::Formatter for PyFormatter {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }
}

/// Внешний источник — JSON ответа Spotify `color-lyrics` (замыкание из sf-services поверх
/// PlayerHandle::lyrics_json). Ok(None) — текста нет; Err — источник не ответил (такое «не найдено» не кэшируется).
pub type ExtraLrc = Arc<dyn Fn() -> BoxFuture<'static, Result<Option<serde_json::Value>, String>> + Send + Sync>;

/// Подпись источника ExtraLrc в результате (`source`).
pub const EXTRA_SOURCE: &str = "Spotify";

/// Миллисекунды Spotify: строка или число
fn ms(value: &serde_json::Value) -> Option<u64> {
    value.as_str().and_then(|s| s.trim().parse().ok()).or_else(|| value.as_u64())
}

/// Слог Spotify {"startTimeMs", "endTimeMs", "words"} → (текст, начало, конец) в секундах
fn syllable(value: &serde_json::Value) -> Option<(String, f64, f64)> {
    let start = value.get("startTimeMs").and_then(ms)?;
    let end = value.get("endTimeMs").and_then(ms).unwrap_or(start);
    let words = value.get("words")?.as_str()?;
    Some((words.to_owned(), round3(start as f64 / 1000.0), round3(end as f64 / 1000.0)))
}

/// Ответ Spotify `color-lyrics` с SYLLABLE_SYNCED → строки с настоящим временем слов из `syllables` (i = false).
/// None — syncType другой или разметки слогов нет хотя бы у одной поющейся строки: тогда работает построчный
/// `lrc_from_spotify` с интерполяцией, как раньше. Строки «♪» и пустые метки пропускаются.
pub fn lines_from_spotify(json: &serde_json::Value) -> Option<Vec<Line>> {
    let lyrics = json.get("lyrics")?;
    if lyrics.get("syncType").and_then(|v| v.as_str()) != Some("SYLLABLE_SYNCED") {
        return None;
    }
    let mut out = Vec::new();
    for line in lyrics.get("lines")?.as_array()? {
        let Some(at_ms) = line.get("startTimeMs").and_then(ms) else { continue };
        let words = line.get("words").and_then(|v| v.as_str()).unwrap_or("");
        let text = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || text == "\u{266a}" || is_meta_line(&text) {
            continue;
        }
        let syllables: Vec<(String, f64, f64)> = line
            .get("syllables")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(syllable).collect())
            .unwrap_or_default();
        // неполная разметка — построчный текст лучше, чем потерянная строка
        if syllables.is_empty() {
            return None;
        }
        let words = group_syllables(&syllables);
        let Some(last) = words.last() else { continue };
        let t = round3(at_ms as f64 / 1000.0);
        out.push(Line::words(t, round3(t.max(last.e)), words, false));
    }
    (!out.is_empty()).then_some(out)
}

/// Ответ Spotify `color-lyrics` (JSON {"lyrics": {"syncType", "lines": [{"startTimeMs", "words"}]}}) → LRC
/// построчно, `[mm:ss.mmm]текст`. Перенос lyrics::from_spotify из spotifast. Строки «♪» и пустые остаются
/// пустыми метками — по ним считается конец предыдущей строки. Текст без времени (UNSYNCED) — None:
/// синхронный текст тогда ищут остальные источники.
pub fn lrc_from_spotify(json: &serde_json::Value) -> Option<String> {
    let lyrics = json.get("lyrics")?;
    let sync = lyrics.get("syncType").and_then(|v| v.as_str()).unwrap_or("");
    if sync != "LINE_SYNCED" && sync != "SYLLABLE_SYNCED" {
        return None;
    }
    let mut timed: Vec<(u64, String)> = Vec::new();
    for line in lyrics.get("lines")?.as_array()? {
        let Some(at_ms) = line.get("startTimeMs").and_then(ms) else {
            continue;
        };
        let words = line.get("words").and_then(|v| v.as_str()).unwrap_or("");
        // переводы строк внутри не должны рвать LRC
        let text = words.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = if text == "\u{266a}" { String::new() } else { text };
        timed.push((at_ms, text));
    }
    let sung: Vec<&(u64, String)> = timed.iter().filter(|(_, text)| !text.is_empty()).collect();
    // все метки нулевые — синхронизации на деле нет
    if sung.is_empty() || (sung.len() > 1 && sung.iter().all(|(at, _)| *at == 0)) {
        return None;
    }
    let mut out = String::new();
    for (at_ms, text) in timed {
        out.push_str(&format!("[{:02}:{:02}.{:03}]{text}\n", at_ms / 60_000, at_ms / 1000 % 60, at_ms % 1000));
    }
    Some(out)
}

/// Имя файла кэша: sha1(f"{CACHE_VERSION}|{track_id}") hex + ".json"
pub fn cache_file_name(track_id: &str) -> String {
    cache_file_name_v(CACHE_VERSION, track_id)
}

/// То же для явной версии (эталоны Python посчитаны для версии 5)
pub fn cache_file_name_v(version: u32, track_id: &str) -> String {
    let key = Sha1::digest(format!("{version}|{track_id}").as_bytes());
    format!("{}.json", hex::encode(key))
}

/// Ошибка источника → в details (как attempt в Python)
fn attempt<T>(errors: &mut Vec<String>, name: &str, r: Result<T, String>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!("lyrics: {name} failed: {e}");
            errors.push(format!("{name}: {e}"));
            None
        }
    }
}

/// Откуда брать HTTP-клиент; Err — сеть закрыта (например, неверные настройки прокси).
type ClientSource = std::sync::Arc<dyn Fn() -> Result<reqwest::Client, String> + Send + Sync>;

/// Источник текстов с дисковым кэшем.
#[derive(Clone)]
pub struct LyricsFetcher {
    cache_dir: PathBuf,
    http: ClientSource,
}

impl LyricsFetcher {
    /// `cache_dir` — dirs::cache_subdir("lyrics"); в нём же mxm_token.json.
    pub fn new(cache_dir: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self::with_client(cache_dir, http)
    }

    /// Со своим HTTP-клиентом (заголовки и таймаут источники всё равно ставят на каждый запрос).
    pub fn with_client(cache_dir: PathBuf, http: reqwest::Client) -> Self {
        Self::with_source(cache_dir, move || Ok(http.clone()))
    }

    /// Клиент берётся заново для каждого поиска: смена прокси в приложении применяется сразу.
    pub fn with_source(
        cache_dir: PathBuf,
        source: impl Fn() -> Result<reqwest::Client, String> + Send + Sync + 'static,
    ) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("lyrics: cache directory {} not created: {e}", cache_dir.display());
        }
        Self { cache_dir, http: std::sync::Arc::new(source) }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// <cache_dir>/<sha1>.json
    pub fn cache_path(&self, track_id: &str) -> PathBuf {
        self.cache_dir.join(cache_file_name(track_id))
    }

    /// Свежая запись кэша: ok — навсегда, not found — пока младше NOT_FOUND_TTL_SECS.
    pub async fn read_cache(&self, track_id: &str) -> Option<LyricsResult> {
        let path = self.cache_path(track_id);
        let raw = tokio::fs::read_to_string(&path).await.ok()?;
        let cached: LyricsResult = serde_json::from_str(&raw).ok()?;
        if cached.ok {
            return Some(cached);
        }
        let mtime = tokio::fs::metadata(&path).await.ok()?.modified().ok()?;
        // mtime из будущего — тоже свежий, как в Python
        let age = SystemTime::now().duration_since(mtime).unwrap_or_default();
        (age < Duration::from_secs(NOT_FOUND_TTL_SECS)).then_some(cached)
    }

    /// Запись через .part + rename
    pub async fn write_cache(&self, track_id: &str, result: &LyricsResult) -> io::Result<()> {
        let path = self.cache_path(track_id);
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, result.to_python_json()).await?;
        tokio::fs::rename(&tmp, &path).await
    }

    /// Word-level result from the sources that have it, without lines-only fallbacks.
    /// Called after Spotify returned line lyrics: the caller waits a little and, if this is
    /// slow, keeps the line result and lets a background task fill the cache.
    async fn word_result(&self, query: &LyricsQuery) -> Option<LyricsResult> {
        let http = (self.http)().ok()?;
        if let Ok(Some(song)) = sources::ne_search(&http, query).await {
            if let Some(id) = song.get("id") {
                if let Ok(data) = sources::ne_lyric_by_id(&http, &py_id(id)).await {
                    // нет YRC — не выход из функции: дальше пробуем Musixmatch
                    let yrc = data.get("yrc").and_then(|y| y.get("lyric")).and_then(|v| v.as_str()).unwrap_or("");
                    if !yrc.is_empty() {
                        let lines = parse_yrc(yrc);
                        let texts: Vec<String> = lines.iter().map(Line::text).collect();
                        if !lines.is_empty() && !is_placeholder(texts.iter().map(String::as_str)) {
                            return Some(LyricsResult::found("NetEase", "word", lines));
                        }
                    }
                }
            }
        }
        let token = sources::mxm_token(&http, &self.cache_dir, "").await.ok().flatten().filter(|t| !t.is_empty())?;
        match sources::mxm_fetch(&http, query, &token).await {
            Ok(Some(("word", lines))) => Some(LyricsResult::found("Musixmatch", "word", lines)),
            _ => None,
        }
    }

    /// Как _fetch_cached в lyrics.py: файл <cache_dir>/<sha1(f"{CACHE_VERSION}|{track_id}")>.json;
    /// ok — навсегда, not found — NOT_FOUND_TTL_SECS по mtime. Найденное проходит add_gaps(lines, duration).
    /// Результат с непустым details (источник ответил ошибкой) не кэшируется. Запись — через .part + rename.
    pub async fn fetch_cached(&self, query: &LyricsQuery, extra: Option<ExtraLrc>) -> LyricsResult {
        if let Some(cached) = self.read_cache(&query.track_id).await {
            return cached;
        }
        let mut result = self.fetch(query, "", extra).await;
        if result.ok && result.source == EXTRA_SOURCE && result.kind == "line" {
            // Spotify knew the lines immediately; a word-level source makes the fill non-linear.
            // Wait a little, then keep the line and finish the upgrade in the background.
            // один запрос: по таймауту он не перезапускается, а дорабатывает в фоне
            let me = self.clone();
            let q = query.clone();
            let mut task = tokio::spawn(async move { me.word_result(&q).await });
            match tokio::time::timeout(Duration::from_millis(1500), &mut task).await {
                Ok(Ok(Some(word))) => result = word,
                Ok(_) => {}
                Err(_) => {
                    let me = self.clone();
                    let query = query.clone();
                    tokio::spawn(async move {
                        let Ok(Some(mut word)) = task.await else { return };
                        word.lines = add_gaps(std::mem::take(&mut word.lines), query.duration);
                        if let Err(e) = me.write_cache(&query.track_id, &word).await {
                            tracing::warn!("lyrics: word upgrade {} not cached: {e}", query.track_id);
                        }
                    });
                }
            }
        }
        if result.ok {
            result.lines = add_gaps(std::mem::take(&mut result.lines), query.duration);
        }
        // Источник с ошибкой (503, таймаут) мог знать текст — такое «не найдено» не кэшируем
        if (result.ok || result.details.is_empty())
            && let Err(e) = self.write_cache(&query.track_id, &result).await
        {
            tracing::warn!("lyrics: cache {} not written: {e}", query.track_id);
        }
        result
    }

    /// Цепочка источников без кэша (fetch в lyrics_sources.py); mxm_user_token — пусто = анонимный токен.
    pub async fn fetch(&self, query: &LyricsQuery, mxm_user_token: &str, extra: Option<ExtraLrc>) -> LyricsResult {
        let duration = query.duration;
        let mut errors = Vec::new();

        // 0. Spotify — первым: это текст именно этой записи. SYLLABLE_SYNCED даёт время слов;
        // LINE_SYNCED отдаётся сразу, чтобы текст не ждал медленных источников. Если рядом есть
        // время слов (NetEase/Musixmatch), fetch_cached подтянет его за 1.5 с и закэширует;
        // медленный источник догружается в фоне.
        if let Some(extra) = extra {
            let json = attempt(&mut errors, "spotify", extra().await).flatten();
            if let Some(json) = json {
                if let Some(lines) = lines_from_spotify(&json) {
                    return LyricsResult::found(EXTRA_SOURCE, "word", lines);
                }
                if let Some(lrc) = lrc_from_spotify(&json).filter(|l| !l.is_empty()) {
                    let lines = build_from_timed_lines(&lines_from_lrc(&lrc), duration);
                    if !lines.is_empty() {
                        return LyricsResult::found(EXTRA_SOURCE, "line", lines);
                    }
                }
            }
        }

        let http = match (self.http)() {
            Ok(http) => http,
            Err(e) => {
                errors.push(format!("network: {e}"));
                return LyricsResult::not_found(errors);
            }
        };
        let http = &http;

        // 1. NetEase YRC (по словам)
        let mut ne_data = None;
        if let Some(song) = attempt(&mut errors, "netease-search", sources::ne_search(http, query).await).flatten() {
            tracing::debug!("lyrics: netease match {} {}", song["id"], song["name"]);
            let lyric = match song.get("id") {
                Some(id) => sources::ne_lyric_by_id(http, &py_id(id)).await,
                None => Err("'id'".to_owned()),
            };
            ne_data = attempt(&mut errors, "netease-lyric", lyric);
            let yrc = ne_data.as_ref().and_then(|d| d.get("yrc")?.get("lyric")?.as_str()).unwrap_or("");
            if !yrc.is_empty() {
                let lines = parse_yrc(yrc);
                let texts: Vec<String> = lines.iter().map(Line::text).collect();
                if !lines.is_empty() && is_placeholder(texts.iter().map(String::as_str)) {
                    tracing::debug!("lyrics: netease yrc: placeholder, skip");
                } else if !lines.is_empty() {
                    return LyricsResult::found("NetEase", "word", lines);
                }
            }
        }

        // 2. Musixmatch richsync (по словам) — заодно даёт построчный запасной вариант
        let mut mxm_line = Vec::new();
        let token = sources::mxm_token(http, &self.cache_dir, mxm_user_token).await;
        let token = attempt(&mut errors, "musixmatch-token", token).flatten().filter(|t| !t.is_empty());
        if let Some(token) = token {
            match attempt(&mut errors, "musixmatch", sources::mxm_fetch(http, query, &token).await).flatten() {
                Some(("word", lines)) => return LyricsResult::found("Musixmatch", "word", lines),
                Some((_, lines)) => mxm_line = lines,
                None => {}
            }
        }

        // 3. LRCLIB (по строкам, интерполяция)
        let lrc = attempt(&mut errors, "lrclib", sources::lrclib_fetch(http, query).await).flatten();
        if let Some(lrc) = lrc.filter(|l| !l.is_empty()) {
            let lines = build_from_timed_lines(&lines_from_lrc(&lrc), duration);
            if !lines.is_empty() {
                return LyricsResult::found("LRCLIB", "line", lines);
            }
        }

        // 4. Musixmatch subtitles
        if !mxm_line.is_empty() {
            return LyricsResult::found("Musixmatch", "line", mxm_line);
        }

        // 5. NetEase LRC
        let ne_lrc = ne_data.as_ref().and_then(|d| d.get("lrc")?.get("lyric")?.as_str()).unwrap_or("");
        if !ne_lrc.is_empty() {
            let pairs = lines_from_lrc(ne_lrc);
            if !is_placeholder(pairs.iter().map(|(_, t)| t.as_str())) {
                let lines = build_from_timed_lines(&pairs, duration);
                if !lines.is_empty() {
                    return LyricsResult::found("NetEase", "line", lines);
                }
            }
        }

        LyricsResult::not_found(errors)
    }
}

/// str(song["id"]) Python
fn py_id(id: &serde_json::Value) -> String {
    match id {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LyricsResult {
        LyricsResult::found(
            "Spotify",
            "line",
            vec![
                Line::Gap { gap: true, t: 0.0, e: 4.85 },
                Line::words(
                    5.0,
                    6.5,
                    vec![Word { x: "раз ".into(), s: 5.0, e: 5.5, p: Some(vec![(1, 5.0, 5.25), (2, 5.25, 5.5)]) }],
                    true,
                ),
            ],
        )
    }

    #[test]
    fn result_json_is_python_shaped() {
        assert_eq!(
            sample().to_python_json(),
            r#"{"ok": true, "source": "Spotify", "kind": "line", "lines": [{"gap": true, "t": 0.0, "e": 4.85}, {"t": 5.0, "e": 6.5, "w": [{"x": "раз ", "s": 5.0, "e": 5.5, "p": [[1, 5.0, 5.25], [2, 5.25, 5.5]]}], "i": true}]}"#
        );
        assert_eq!(
            LyricsResult::not_found(vec![]).to_python_json(),
            r#"{"ok": false, "error": "not found", "details": []}"#
        );
    }

    #[test]
    fn result_json_roundtrips() {
        let r = sample();
        assert_eq!(serde_json::from_str::<LyricsResult>(&r.to_python_json()).unwrap(), r);
        let nf = LyricsResult::not_found(vec!["lrclib: HTTP Error 503: Service Unavailable".into()]);
        assert_eq!(serde_json::from_str::<LyricsResult>(&serde_json::to_string(&nf).unwrap()).unwrap(), nf);
    }

    #[test]
    fn cache_key_matches_python() {
        // hashlib.sha1(b"5|12345").hexdigest()
        assert_eq!(cache_file_name_v(5, "12345"), "e13466f72878a42f8fe435c7ee2b441ee93e2d57.json");
        assert_ne!(cache_file_name("12345"), cache_file_name_v(5, "12345"));
    }

    #[tokio::test]
    async fn cache_is_read_back_and_not_found_expires() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        fetcher.write_cache("1", &sample()).await.unwrap();
        assert_eq!(fetcher.read_cache("1").await, Some(sample()));
        assert!(!dir.join(cache_file_name("1")).with_extension("part").exists());

        let nf = LyricsResult::not_found(vec![]);
        fetcher.write_cache("2", &nf).await.unwrap();
        assert_eq!(fetcher.read_cache("2").await, Some(nf));
        // устаревшее «не найдено» перепроверяется
        let old = SystemTime::now() - Duration::from_secs(NOT_FOUND_TTL_SECS + 60);
        std::fs::File::options().write(true).open(fetcher.cache_path("2")).unwrap().set_modified(old).unwrap();
        assert_eq!(fetcher.read_cache("2").await, None);
        assert_eq!(fetcher.read_cache("missing").await, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_closed_network_is_not_bypassed_and_not_cached() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-closed-{}", std::process::id()));
        let fetcher = LyricsFetcher::with_source(dir.clone(), || Err("прокси настроен с ошибкой".into()));
        let query = LyricsQuery { track_id: "closed".into(), title: "t".into(), artist: "a".into(), ..Default::default() };
        let result = fetcher.fetch_cached(&query, None).await;
        assert!(!result.ok);
        assert!(result.details.iter().any(|detail| detail.contains("прокси")));
        assert_eq!(fetcher.read_cache("closed").await, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cached_result_skips_sources() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-cached-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        fetcher.write_cache("42", &sample()).await.unwrap();
        let query = LyricsQuery { track_id: "42".into(), title: "x".into(), ..Default::default() };
        assert_eq!(fetcher.fetch_cached(&query, None).await, sample());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn color_lyrics(sync: &str) -> serde_json::Value {
        serde_json::json!({"lyrics": {"syncType": sync, "lines": [
            {"startTimeMs": "0", "words": "♪", "syllables": []},
            {"startTimeMs": "17120", "words": "Hello darkness, my old friend"},
            {"startTimeMs": 21860, "words": "  I've come\nto talk  "},
            {"startTimeMs": "27010", "words": ""},
            {"startTimeMs": "3723456", "words": "конец"},
            {"words": "без времени"}
        ]}})
    }

    #[test]
    fn spotify_lines_become_lrc() {
        let lrc = lrc_from_spotify(&color_lyrics("LINE_SYNCED")).unwrap();
        assert_eq!(
            lrc,
            "[00:00.000]\n[00:17.120]Hello darkness, my old friend\n[00:21.860]I've come to talk\n[00:27.010]\n\
             [62:03.456]конец\n"
        );
        let pairs = lrc::lines_from_lrc(&lrc);
        assert_eq!(pairs[1], (17.12, "Hello darkness, my old friend".to_owned()));
        assert_eq!(pairs[4], (3723.456, "конец".to_owned()));
        // пустые метки ограничивают предыдущую строку
        let lines = lrc::build_from_timed_lines(&pairs, 4000.0);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].end() <= 26.96 + 1e-9);

        assert_eq!(lrc_from_spotify(&color_lyrics("UNSYNCED")), None);
        assert!(lrc_from_spotify(&color_lyrics("SYLLABLE_SYNCED")).is_some());
        assert_eq!(lrc_from_spotify(&serde_json::json!({})), None);
        assert_eq!(lrc_from_spotify(&serde_json::json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": []}})), None);
        let zeros = serde_json::json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": [
            {"startTimeMs": "0", "words": "раз"}, {"startTimeMs": "0", "words": "два"}]}});
        assert_eq!(lrc_from_spotify(&zeros), None);
    }

    #[tokio::test]
    async fn spotify_comes_first() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-spotify-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        let json = color_lyrics("LINE_SYNCED");
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "4uLU6hMCjMI75M1A2tKUQC".into(), title: "x".into(), duration: 300.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!(result.source, EXTRA_SOURCE);
        assert_eq!(result.kind, "line");
        assert_eq!(result.lines.iter().filter(|l| !l.text().is_empty()).count(), 3);
        // найденное записано в кэш текущей версии
        assert_eq!(fetcher.read_cache(&query.track_id).await, Some(result));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// SYLLABLE_SYNCED: у каждой поющейся строки есть слоги со своим временем.
    fn color_lyrics_syllables() -> serde_json::Value {
        serde_json::json!({"lyrics": {"syncType": "SYLLABLE_SYNCED", "lines": [
            {"startTimeMs": "0", "words": "♪", "syllables": []},
            {"startTimeMs": "17120", "words": "Hello darkness", "syllables": [
                {"startTimeMs": "17120", "endTimeMs": "17620", "words": "Hel"},
                {"startTimeMs": "17620", "endTimeMs": "18140", "words": "lo "},
                {"startTimeMs": "18140", "endTimeMs": "19100", "words": "dark"},
                {"startTimeMs": "19100", "endTimeMs": "19700", "words": "ness"}]},
            {"startTimeMs": "21860", "words": "my old friend", "syllables": [
                {"startTimeMs": "21860", "endTimeMs": "22100", "words": "my "},
                {"startTimeMs": "22100", "endTimeMs": "22600", "words": "old "},
                {"startTimeMs": "22600", "endTimeMs": "23260", "words": "friend"}]},
            {"startTimeMs": "27010", "words": "", "syllables": []}
        ]}})
    }

    #[test]
    fn syllable_synced_lines_keep_word_timing() {
        let lines = lines_from_spotify(&color_lyrics_syllables()).unwrap();
        assert_eq!(lines.len(), 2, "«♪» и пустая метка — не строки");
        let Line::Words { t, e, w, i, .. } = &lines[0] else { panic!("ожидалась строка со словами") };
        assert!(!i, "время слов настоящее, а не интерполированное");
        assert_eq!((*t, *e), (17.12, 19.7));
        assert_eq!(w.iter().map(|w| w.x.as_str()).collect::<Vec<_>>(), vec!["Hello ", "darkness"]);
        // слоги одного слова остаются внутри него: "Hel"+"lo " и "dark"+"ness"
        assert_eq!(w[0], Word { x: "Hello ".into(), s: 17.12, e: 18.14, p: Some(vec![(3, 17.12, 17.62), (2, 17.62, 18.14)]) });
        assert_eq!(w[1], Word { x: "darkness".into(), s: 18.14, e: 19.7, p: Some(vec![(4, 18.14, 19.1), (4, 19.1, 19.7)]) });
        assert_eq!(lines[1].end(), 23.26);
        assert_eq!(lines[1].text(), "my old friend");
    }

    #[tokio::test]
    async fn synthetic_spotify_lines_win_over_interpolation() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-syllables-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        let json = color_lyrics_syllables();
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "syllables".into(), title: "x".into(), duration: 300.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!((result.source.as_str(), result.kind.as_str()), (EXTRA_SOURCE, "word"));
        assert_eq!(result.lines.iter().filter(|l| !l.text().is_empty()).count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn incomplete_syllables_fall_back_to_the_line_lrc() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-half-syllables-{}", std::process::id()));
        // Сеть выключена: проверяем, что построчный Spotify не теряется, когда другие
        // источники недоступны (он теперь запасной, а не первый).
        let fetcher = LyricsFetcher::with_source(dir.clone(), || Err("offline".into()));
        // SYLLABLE_SYNCED, но слогов у поющейся строки нет — построчный путь и интерполяция
        let json = serde_json::json!({"lyrics": {"syncType": "SYLLABLE_SYNCED", "lines": [
            {"startTimeMs": "1000", "words": "раз два", "syllables": []},
            {"startTimeMs": "3000", "words": "три", "syllables": [
                {"startTimeMs": "3000", "endTimeMs": "3500", "words": "три"}]}
        ]}});
        assert_eq!(lines_from_spotify(&json), None);
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "half".into(), title: "x".into(), duration: 60.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!((result.source.as_str(), result.kind.as_str()), (EXTRA_SOURCE, "line"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Живая проверка источников: `MATERIALFY_LYRICS_LIVE=1 cargo test -p mf-lyrics live -- --nocapture`.
    #[tokio::test]
    async fn live_sources_find_a_known_song() {
        if std::env::var("MATERIALFY_LYRICS_LIVE").as_deref() != Ok("1") {
            return;
        }
        let dir = std::env::temp_dir().join("mf-lyrics-live");
        let fetcher = LyricsFetcher::new(dir);
        for (title, artist, duration) in [("Creep", "Radiohead", 238.0), ("Группа крови", "Кино", 286.0)] {
            let query = LyricsQuery { title: title.into(), artist: artist.into(), duration, ..Default::default() };
            let http = (fetcher.http)().unwrap();
            if let Ok(Some(song)) = sources::ne_search(&http, &query).await {
                let data = sources::ne_lyric_by_id(&http, &py_id(&song["id"])).await;
                let has = |k: &str| data.as_ref().ok().and_then(|d| d[k]["lyric"].as_str()).is_some_and(|s| !s.is_empty());
                println!("netease {} {}: yrc {} lrc {}", song["id"], song["name"], has("yrc"), has("lrc"));
            }
            let r = fetcher.fetch(&query, "", None).await;
            println!("{artist} — {title}: {} {} строк {} ошибки {:?}", r.source, r.kind, r.lines.len(), r.details);
            let first: Vec<String> = r.lines.iter().take(3).map(Line::text).collect();
            println!("   {}", first.join(" | "));
            assert!(r.ok);
        }
    }
}
