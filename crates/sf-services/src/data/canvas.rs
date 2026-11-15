//! Canvas трека, как в телефоне: короткое зацикленное видео вместо обложки. Сохранённый запрос веб-плеера
//! `canvas` ({ trackUri }) через GraphQL-шлюз с токеном сессии → data.trackUnion.canvas { url, type }
//! (type: VIDEO, VIDEO_LOOPING, VIDEO_LOOPING_RANDOM, IMAGE, GIF).
use serde_json::{Value, json};
use sf_api::Http;

use super::persisted;

/// Хэш запроса `canvas` (web-player.c078a389.js, 2026-11; свежий — из бандла веб-плеера).
const HASHES: [&str; 1] = ["575138ab27cd5c1b3e54da54d0a7cc8d85485402de26340c2145f0f6bb5e7a9f"];

/// Ссылка на видео Canvas; None — у трека его нет (или это картинка).
pub fn parse(body: &Value) -> Option<String> {
    let canvas = body.pointer("/data/trackUnion/canvas")?;
    let kind = canvas.get("type").and_then(Value::as_str).unwrap_or_default();
    let url = canvas.get("url").and_then(Value::as_str).unwrap_or_default();
    (kind.starts_with("VIDEO") && url.starts_with("https://")).then(|| url.to_owned())
}

pub async fn canvas_url(http: &Http, session: &sf_player::Session, track_id: &str) -> anyhow::Result<Option<String>> {
    let variables = json!({ "trackUri": format!("spotify:track:{track_id}") });
    Ok(parse(&persisted::query(http, session, "canvas", &HASHES, &variables).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_video_canvas_counts() {
        let body = |kind: &str| json!({ "data": { "trackUnion": { "__typename": "Track", "canvas": { "type": kind, "url": "https://canvaz.scdn.co/x.mp4" } } } });
        assert_eq!(parse(&body("VIDEO_LOOPING")).as_deref(), Some("https://canvaz.scdn.co/x.mp4"));
        assert_eq!(parse(&body("IMAGE")), None);
        assert_eq!(parse(&json!({ "data": { "trackUnion": { "__typename": "Track", "canvas": null } } })), None);
    }
}
