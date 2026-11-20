//! Профиль аккаунта без Web API. После входа librespot: тип аккаунта и страна — из атрибутов сессии,
//! имя и аватар — из spclient `user-profile-view/v3/profile/{username}`. До входа (токен из браузера):
//! тип аккаунта — из spclient `melody/v1/product_state`. Проверено вживую (2026-10): атрибут `type`
//! приходит через ~0,1 с после входа; профиль отдаёт `name` и `image_url` (нет аватара — нет поля);
//! product_state — `{"product":"premium","catalogue":"premium","country":"IN",…}`.
use std::time::{Duration, Instant};

use serde_json::Value;
use sf_api::models::{Image, User};

const PRODUCT_STATE_URL: &str = "https://spclient.wg.spotify.com/melody/v1/product_state";
/// Сколько ждать атрибуты аккаунта после входа сессии.
const ATTRIBUTE_WAIT: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(10);

/// Имя и аватар из user-profile-view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub name: Option<String>,
    pub image: Option<String>,
}

/// Профиль аккаунта сессии в виде /v1/me. Профиль spotify не ответил — только id, тип и страна.
pub async fn fetch(session: &sf_player::Session) -> User {
    let username = session.username();
    let started = Instant::now();
    let mut product = session.get_user_attribute("type");
    while product.is_none() && started.elapsed() < ATTRIBUTE_WAIT {
        tokio::time::sleep(Duration::from_millis(100)).await;
        product = session.get_user_attribute("type");
    }
    let country = Some(session.country()).filter(|country| !country.is_empty());
    let path = format!(
        "/user-profile-view/v3/profile/{}?playlist_limit=0&artist_limit=0&episode_limit=0",
        encode(&username)
    );
    let request = session.spclient().request(&reqwest::Method::GET, &path, None, None);
    let profile = match tokio::time::timeout(TIMEOUT, request).await {
        Ok(Ok(body)) => parse_profile(&body),
        Ok(Err(error)) => {
            tracing::warn!("profile via session not received: {error}");
            Profile::default()
        }
        Err(_) => {
            tracing::warn!("profile via session not received: Spotify did not answer in time");
            Profile::default()
        }
    };
    user(&username, product, country, profile)
}

/// Тип аккаунта ("premium" | "free" | …) по токену из браузера — до запуска librespot, который на Free
/// завершает процесс. None — узнать не удалось.
pub async fn product_of_token(http: &reqwest::Client, token: &str) -> Option<String> {
    let response = match http.get(PRODUCT_STATE_URL).bearer_auth(token).timeout(TIMEOUT).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("account type not checked: {}", error.without_url());
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!("account type not checked: {}", response.status());
        return None;
    }
    parse_product(&response.text().await.ok()?)
}

fn parse_product(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    ["product", "type", "catalogue"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(|product| product.trim().to_ascii_lowercase())
        .filter(|product| !product.is_empty())
}

fn parse_profile(body: &[u8]) -> Profile {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Profile::default();
    };
    let field = |key: &str| {
        value.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
    };
    Profile { name: field("name"), image: field("image_url") }
}

fn user(username: &str, product: Option<String>, country: Option<String>, profile: Profile) -> User {
    User {
        id: username.to_owned(),
        display_name: profile.name,
        images: profile.image.map(|url| Image { url, width: None, height: None }).into_iter().collect(),
        product: product.map(|product| product.to_ascii_lowercase()),
        country,
        uri: Some(format!("spotify:user:{username}")),
    }
}

/// Имя пользователя в пути запроса.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => char::from(byte).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_view_gives_the_name_and_avatar() {
        let body = br#"{"uri":"spotify:user:spotify","name":"Spotify","image_url":"https://i.scdn.co/image/ab67","followers_count":1}"#;
        assert_eq!(
            parse_profile(body),
            Profile { name: Some("Spotify".into()), image: Some("https://i.scdn.co/image/ab67".into()) }
        );
        // без аватара поля нет
        let plain = br#"{"uri":"spotify:user:31qn","name":"fatyzzz","is_current_user":true}"#;
        assert_eq!(parse_profile(plain), Profile { name: Some("fatyzzz".into()), image: None });
        assert_eq!(parse_profile(b"not json"), Profile::default());
    }

    #[test]
    fn a_session_profile_reads_like_the_web_api() {
        let profile = Profile { name: Some("fatyzzz".into()), image: Some("face".into()) };
        let user = user("31qn", Some("premium".into()), Some("IN".into()), profile);
        let account = sf_api::map::account(&user);
        assert_eq!((account.id.as_str(), account.display_name.as_str()), ("31qn", "fatyzzz"));
        assert!(account.premium);
        assert_eq!((account.country.as_str(), account.avatar.as_str()), ("IN", "face"));
        assert_eq!(account.uri, "spotify:user:31qn");
        // имени нет — подпись по id, как у Web API
        let bare = sf_api::map::account(&super::user("31qn", None, None, Profile::default()));
        assert_eq!(bare.id, "31qn");
        assert!(!bare.premium);
    }

    #[test]
    fn the_product_state_names_the_account_type() {
        let body = r#"{"ads":"0","catalogue":"premium","country":"IN","product":"premium","on-demand":"1"}"#;
        assert_eq!(parse_product(body).as_deref(), Some("premium"));
        assert_eq!(parse_product(r#"{"catalogue":"Free"}"#).as_deref(), Some("free"));
        assert_eq!(parse_product(r#"{"product":""}"#), None);
        assert_eq!(parse_product("<html>"), None);
    }

    #[test]
    fn usernames_are_safe_in_the_path() {
        assert_eq!(encode("31qn6rfojzf5efui4bxblwz6llhe"), "31qn6rfojzf5efui4bxblwz6llhe");
        assert_eq!(encode("a b@c"), "a%20b%40c");
    }
}
