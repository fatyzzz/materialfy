//! Маршрутизация чтений: плейлисты через сессию librespot (без квоты Web API), остальное — ApiGateway.
//! Тексты ошибок Web API — для пользователя (mf_i18n).
use std::time::Duration;

use sf_api::models::{Page, Playlist, PlaylistItem};
use sf_api::{ApiError, ApiGateway, Operation};
use sf_player::PlayerHandle;
use sf_player::engine::session_reads::{self, Failure};

pub const PLAYLIST_PAGE: u32 = 50;

/// Сколько ждать сессию, прежде чем спросить Web API.
const SESSION_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Короткий текст ошибки для пользователя (перевод — в sf-api, ApiError::user_text).
pub fn api_error_text(error: &ApiError) -> String {
    error.user_text()
}

/// Ошибка страницы плейлиста (403/404 — Spotify не отдаёт его сторонним приложениям).
pub fn playlist_error_text(error: &ApiError) -> String {
    match error.status() {
        Some(403 | 404) => mf_i18n::t("Spotify doesn't share this playlist's tracks with third-party apps"),
        _ => api_error_text(error),
    }
}

/// Плейлисты — всегда сначала сессия (свои тоже: личное приложение Web API — лишь запасной путь).
fn session_for(api: &ApiGateway, player: &PlayerHandle, _operation: Operation) -> Option<sf_player::Session> {
    account_session(api, player)
}

/// Сколько фоновое чтение на старте ждёт подключения движка, прежде чем пойти в Web API.
pub const STARTUP_SESSION_WAIT: Duration = Duration::from_secs(20);

/// Сессия того же аккаунта; пока движок подключается (LocalPlayback::Connecting) — ждать её, не дольше `max`.
/// На старте Web API готов сразу (сохранённый профиль), а сессия — через секунды: без ожидания библиотека
/// и история ушли бы в Web API, а потом перечитались бы через сессию ещё раз.
pub async fn wait_account_session(api: &ApiGateway, player: &PlayerHandle, max: Duration) -> Option<sf_player::Session> {
    let started = std::time::Instant::now();
    loop {
        if let Some(session) = account_session(api, player) {
            return Some(session);
        }
        let coming = matches!(player.local_playback(), sf_player::LocalPlayback::Connecting);
        if !coming || started.elapsed() >= max {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Сессия librespot того же аккаунта, что и Web API (иначе её ответ — про чужую библиотеку).
pub fn account_session(api: &ApiGateway, player: &PlayerHandle) -> Option<sf_player::Session> {
    let session = player.session()?;
    let account = api.account()?;
    (session.username() == account.as_str()).then_some(session)
}

/// Чтение через сессию с ожиданием SESSION_READ_TIMEOUT; None — сессия не смогла, спросить Web API.
pub async fn session_read<T>(
    read: impl std::future::Future<Output = Result<T, Failure>>,
) -> Option<Result<T, ApiError>> {
    settle(tokio::time::timeout(SESSION_READ_TIMEOUT, read).await)
}

/// Одно место выбора «сессия или Web API» для чтения. Сначала сессия librespot того же аккаунта (у неё нет квоты
/// Web API), не дольше `deadline`; Web API (`operation` выбирает общее/личное приложение) — только если сессии нет,
/// она не успела или ответила Failure::Retry. Failure::Definitive отдаётся как есть: Web API ответил бы так же,
/// а квоту бы потратил. Фоновое чтение оборачивайте в `sf_api::limits::background(...)`: запасной путь тогда
/// уступит страницам и не пойдёт в сеть, пока Web API ограничен.
pub async fn prefer_session<T, SF, WF>(
    api: &ApiGateway,
    player: &PlayerHandle,
    operation: Operation,
    deadline: Duration,
    session_read: impl FnOnce(sf_player::Session) -> SF,
    web_read: impl FnOnce(std::sync::Arc<sf_api::ApiClient>) -> WF,
) -> Result<T, ApiError>
where
    SF: std::future::Future<Output = Result<T, Failure>>,
    WF: std::future::Future<Output = Result<T, ApiError>>,
{
    session_first(account_session(api, player), deadline, session_read, || async move {
        web_read(api.client_for(operation).await?).await
    })
    .await
}

async fn session_first<S, T, SF, WF>(
    session: Option<S>,
    deadline: Duration,
    session_read: impl FnOnce(S) -> SF,
    web_read: impl FnOnce() -> WF,
) -> Result<T, ApiError>
where
    SF: std::future::Future<Output = Result<T, Failure>>,
    WF: std::future::Future<Output = Result<T, ApiError>>,
{
    if let Some(session) = session
        && let Some(result) = settle(tokio::time::timeout(deadline, session_read(session)).await)
    {
        return result;
    }
    web_read().await
}

/// Ответ сессии как ответ Web API; None — сессия не смогла, спросить Web API.
fn settle<T>(result: Result<Result<T, Failure>, tokio::time::error::Elapsed>) -> Option<Result<T, ApiError>> {
    match result {
        Ok(Ok(value)) => Some(Ok(value)),
        Ok(Err(Failure::Definitive(error))) => Some(Err(error)),
        Ok(Err(Failure::Retry(error))) => {
            tracing::debug!("read via session failed: {error:#}; asking Web API");
            None
        }
        Err(_) => {
            tracing::debug!("read via session timed out; asking Web API");
            None
        }
    }
}

/// Шапка плейлиста.
pub async fn playlist(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Result<Playlist, ApiError> {
    let operation = Operation::PlaylistMetadata(api.playlist_access(id));
    if let Some(session) = session_for(api, player, operation)
        && let Some(result) =
            settle(tokio::time::timeout(SESSION_READ_TIMEOUT, session_reads::playlist(&session, id)).await)
    {
        if let Ok(playlist) = &result {
            api.observe_playlist(playlist);
        }
        return result;
    }
    let playlist = api.client_for(operation).await?.playlist(id).await?;
    api.observe_playlist(&playlist);
    Ok(playlist)
}

/// Страница строк плейлиста (по 50).
pub async fn playlist_items(
    api: &ApiGateway,
    player: &PlayerHandle,
    id: &str,
    offset: u32,
) -> Result<Page<PlaylistItem>, ApiError> {
    let operation = Operation::PlaylistItems(api.playlist_access(id));
    if let Some(session) = session_for(api, player, operation)
        && let Some(result) = settle(
            tokio::time::timeout(SESSION_READ_TIMEOUT, session_reads::items(&session, id, offset, PLAYLIST_PAGE)).await,
        )
    {
        return result;
    }
    api.client_for(operation).await?.playlist_items(id, offset, PLAYLIST_PAGE).await
}

/// Все страницы списка со смещением (`limit` за раз), с потолком `max_pages`.
pub async fn all_pages<T, F, Fut>(limit: u32, max_pages: u32, mut fetch: F) -> Result<Vec<T>, ApiError>
where
    F: FnMut(u32, u32) -> Fut,
    Fut: std::future::Future<Output = Result<Page<T>, ApiError>>,
{
    let mut items = Vec::new();
    let mut offset = 0;
    for _ in 0..max_pages {
        let page = fetch(offset, limit).await?;
        let next = page.next_offset();
        items.extend(page.items);
        match next {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_user_text() {
        assert_eq!(api_error_text(&ApiError::NotSignedIn), "Sign in to your account");
        assert!(api_error_text(&ApiError::Network("x".into())).contains("proxy"));
        let refused = ApiError::Status { status: 404, message: "Not found".into() };
        assert!(playlist_error_text(&refused).contains("third-party"));
    }

    #[tokio::test]
    async fn the_session_answers_first_and_the_web_api_only_backs_it_up() {
        let web_calls = std::sync::atomic::AtomicUsize::new(0);
        let web = || async {
            web_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, ApiError>("web")
        };
        let short = Duration::from_millis(50);
        // сессия ответила — Web API не спрашиваем
        let got = session_first(Some(()), short, |()| async { Ok("session") }, web).await;
        assert_eq!(got.unwrap(), "session");
        // окончательный ответ сессии (нет такого объекта) — тоже без Web API
        let missing = ApiError::Status { status: 404, message: "нет".into() };
        let got = session_first(Some(()), short, |()| async { Err(Failure::Definitive(missing)) }, web).await;
        assert_eq!(got.unwrap_err().status(), Some(404));
        assert_eq!(web_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        // сессии нет, она не смогла или не успела — Web API
        let none: Option<()> = None;
        assert_eq!(session_first(none, short, |()| async { Ok("session") }, web).await.unwrap(), "web");
        let failed = |()| async { Err(Failure::Retry(anyhow::anyhow!("сеть"))) };
        assert_eq!(session_first(Some(()), short, failed, web).await.unwrap(), "web");
        let slow = |()| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok("session")
        };
        assert_eq!(session_first(Some(()), short, slow, web).await.unwrap(), "web");
        assert_eq!(web_calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn all_pages_follows_next_offsets() {
        let pages = all_pages(2, 10, |offset, limit| async move {
            Ok(Page {
                items: (offset..(offset + limit).min(5)).collect::<Vec<u32>>(),
                total: 5,
                limit,
                offset,
                next: (offset + limit < 5).then(|| "n".to_owned()),
            })
        })
        .await
        .unwrap();
        assert_eq!(pages, vec![0, 1, 2, 3, 4]);
    }
}
