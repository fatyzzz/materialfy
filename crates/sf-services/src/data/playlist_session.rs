//! Плейлисты через сессию librespot того же аккаунта (sf_player::engine::session_playlists): список
//! библиотеки из rootlist, правки, создание, подписка, видимость, обложка — без квоты Web API.
//! None — сессии нет или она не смогла: повторяемую запись и чтение можно отдать Web API. Неповторяемая
//! запись (добавление, перенос), чей ответ потерялся, — ошибка, а не повтор через Web API (могла лечь).
use std::time::Duration;

use sf_api::models::{Image, Playlist};
use sf_api::{ApiError, ApiGateway};
use sf_player::PlayerHandle;
use sf_player::engine::session_playlists::{self as session, Change};
use sf_player::engine::session_reads::Failure;

pub use sf_player::engine::session_playlists::Library;

use super::route;

/// Сколько ждать запись через сессию.
const WRITE_TIMEOUT: Duration = Duration::from_secs(20);
/// Сколько ждать список библиотеки (rootlist и имена владельцев).
const LIBRARY_TIMEOUT: Duration = Duration::from_secs(30);
/// Сколько ждать готовности сессии при входе, прежде чем спросить список у Web API.
pub const SESSION_WAIT: Duration = Duration::from_secs(12);

/// Ответ сессии как ответ Web API; None — спросить Web API (для неповторяемой записи — только если
/// запрос не дошёл).
fn settle<T>(result: Result<Result<T, Failure>, tokio::time::error::Elapsed>, repeatable: bool) -> Option<Result<T, ApiError>> {
    match result {
        Ok(Ok(value)) => Some(Ok(value)),
        Ok(Err(Failure::Definitive(error))) => Some(Err(error)),
        Ok(Err(Failure::Retry(error))) => {
            tracing::debug!("playlist via session failed: {error:#}; asking Web API");
            None
        }
        Err(_) if repeatable => {
            tracing::debug!("playlist via session timed out; asking Web API");
            None
        }
        Err(_) => Some(Err(ApiError::Network("Spotify did not answer in time — refresh the playlist".into()))),
    }
}

async fn run<T>(
    api: &ApiGateway,
    player: &PlayerHandle,
    repeatable: bool,
    run: impl AsyncFnOnce(sf_player::Session) -> Result<T, Failure>,
) -> Option<Result<T, ApiError>> {
    let session = route::account_session(api, player)?;
    settle(tokio::time::timeout(WRITE_TIMEOUT, run(session)).await, repeatable)
}

/// Запись через сессию. Она идёт мимо Web API, поэтому после успеха его кэш GET (20 с) забываем:
/// иначе запасное чтение через Web API отдало бы плейлист до правки.
async fn write<T>(
    api: &ApiGateway,
    player: &PlayerHandle,
    repeatable: bool,
    edit: impl AsyncFnOnce(sf_player::Session) -> Result<T, Failure>,
) -> Option<Result<T, ApiError>> {
    let result = run(api, player, repeatable, edit).await;
    if matches!(result, Some(Ok(_))) {
        api.forget_cached();
    }
    result
}

/// Сессия того же аккаунта; ждёт её до `wait` (вход: движок ещё подключается).
pub async fn wait_session(api: &ApiGateway, player: &PlayerHandle, wait: Duration) -> Option<sf_player::Session> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if let Some(session) = route::account_session(api, player) {
            return Some(session);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Плейлисты библиотеки: дерево rootlist и шапки; None — сессия не готова или не ответила.
pub async fn library(api: &ApiGateway, player: &PlayerHandle, wait: Duration) -> Option<Library> {
    let session = wait_session(api, player, wait).await?;
    match tokio::time::timeout(LIBRARY_TIMEOUT, session::library(&session)).await {
        Ok(Ok(library)) => Some(library),
        Ok(Err(Failure::Definitive(error))) => {
            tracing::debug!("rootlist not loaded: {error}");
            None
        }
        Ok(Err(Failure::Retry(error))) => {
            tracing::debug!("rootlist not loaded: {error:#}");
            None
        }
        Err(_) => {
            tracing::debug!("rootlist load timed out");
            None
        }
    }
}

/// Коллаж обложек для плейлиста без своей; пусто — не вышло.
pub async fn mosaic(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Vec<Image> {
    let Some(session) = route::account_session(api, player) else { return Vec::new() };
    tokio::time::timeout(WRITE_TIMEOUT, session::mosaic(&session, id)).await.unwrap_or_default()
}

/// Правка плейлиста; Ok — snapshot_id после записи.
pub async fn change(api: &ApiGateway, player: &PlayerHandle, id: &str, change: Change) -> Option<Result<Option<String>, ApiError>> {
    let repeatable = !matches!(change, Change::Add { .. } | Change::Move { .. });
    write(api, player, repeatable, async move |session| session::change(&session, id, &change).await).await
}

/// Какие из `uris` уже есть в плейлисте.
pub async fn duplicates(api: &ApiGateway, player: &PlayerHandle, id: &str, uris: &[String]) -> Option<Result<Vec<String>, ApiError>> {
    run(api, player, true, async move |session| session::duplicates(&session, id, uris).await).await
}

pub async fn create(api: &ApiGateway, player: &PlayerHandle, name: &str, public: bool) -> Option<Result<Playlist, ApiError>> {
    write(api, player, true, async move |session| session::create(&session, name, "", public).await).await
}

pub async fn follow(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::follow(&session, &uri, None).await).await
}

pub async fn unfollow(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::unfollow(&session, &uri).await).await
}

pub async fn set_public(api: &ApiGateway, player: &PlayerHandle, id: &str, public: bool) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::set_public(&session, &uri, public).await).await
}

/// Обложка из JPEG; Ok — snapshot_id после записи.
pub async fn upload_cover(api: &ApiGateway, player: &PlayerHandle, id: &str, jpeg: Vec<u8>) -> Option<Result<Option<String>, ApiError>> {
    let http = api.http().client().ok()?;
    write(api, player, true, async move |session| session::upload_cover(&session, &http, id, jpeg).await).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn elapsed() -> tokio::time::error::Elapsed {
        tokio::time::timeout(Duration::ZERO, std::future::pending::<()>()).await.unwrap_err()
    }

    #[tokio::test]
    async fn a_lost_answer_is_retried_only_for_repeatable_writes() {
        assert!(settle::<()>(Err(elapsed().await), true).is_none());
        assert!(matches!(settle::<()>(Err(elapsed().await), false), Some(Err(ApiError::Network(_)))));
        assert!(settle::<()>(Ok(Err(Failure::Retry(anyhow::anyhow!("x")))), false).is_none());
        let refused = Failure::Definitive(ApiError::Status { status: 403, message: String::new() });
        assert!(matches!(settle::<()>(Ok(Err(refused)), true), Some(Err(ApiError::Status { status: 403, .. }))));
        assert!(matches!(settle(Ok(Ok(5)), false), Some(Ok(5))));
    }
}
