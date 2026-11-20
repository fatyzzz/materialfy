//! Коллекция аккаунта через сессию librespot (sf_player::engine::session_collection): «Любимые треки»,
//! сохранённые альбомы, подписки на исполнителей, сохранение/снятие и «сохранено ли». Web API — только
//! запасной путь с коротким сроком: общий ключ приложения упирается в 429 и суточную квоту.
use std::collections::HashMap;
use std::time::Duration;

use sf_api::models::{Artist, Page, SavedAlbum, SavedTrack, Track};
use sf_api::{ApiError, ApiGateway, Operation};
use sf_player::PlayerHandle;
use sf_player::engine::session_collection::{self as collection, CollectionItem, SET_ARTIST, SET_COLLECTION};
use sf_player::engine::session_reads::Failure;

use super::route;

/// Строк «Любимых треков» за страницу через сессию (один пакет метаданных).
pub const LIKED_PAGE: u32 = 50;
/// Сколько ждать чтение набора целиком (с подробностями строк). Срок каждой страницы и пакета
/// (с одним повтором) держит session_collection; этот — лишь потолок: у большой библиотеки
/// десятки страниц и сотни альбомов.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
/// Сколько ждать страницу подробностей «Любимых треков» (пакет и его повтор, иногда два пакета).
const PAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Сколько ждать запись или проверку через сессию, прежде чем спросить Web API.
const SESSION_TIMEOUT: Duration = Duration::from_secs(8);
/// Дольше Web API не ждём: под 429 он отвечает минутами.
const WEB_DEADLINE: Duration = Duration::from_secs(12);

/// Web API не дольше `WEB_DEADLINE`.
async fn web<T>(call: impl Future<Output = Result<T, ApiError>>) -> Result<T, ApiError> {
    tokio::time::timeout(WEB_DEADLINE, call)
        .await
        .unwrap_or_else(|_| Err(ApiError::Network("Spotify did not answer in time".to_owned())))
}

// --- «Любимые треки» ---------------------------------------------------------------------------------

/// «Любимые треки» через сессию: набор URI читается один раз (новые сверху), подробности — страницами.
/// Страницы — в смещениях выданных строк, как у Web API: трек, которого Spotify больше не отдаёт,
/// пропускается, а страница добирается следующими.
pub struct LikedFeed {
    session: Option<sf_player::Session>,
    rows: Vec<CollectionItem>,
    /// смещение страницы → место в `rows`, с которого она начинается
    starts: HashMap<u32, usize>,
    skipped: usize,
}

impl LikedFeed {
    pub fn new(rows: Vec<CollectionItem>) -> Self {
        let rows = rows.into_iter().filter(|row| row.uri.starts_with("spotify:track:")).collect();
        Self { session: None, rows, starts: HashMap::from([(0, 0)]), skipped: 0 }
    }

    /// Набор сессии того же аккаунта; None — сессии нет или она не ответила (спросить Web API).
    pub async fn open(api: &ApiGateway, player: &PlayerHandle) -> Option<Self> {
        let session = route::account_session(api, player)?;
        match tokio::time::timeout(READ_TIMEOUT, collection::collection_all(&session, SET_COLLECTION)).await {
            Ok(Ok(rows)) => Some(Self { session: Some(session), ..Self::new(rows) }),
            Ok(Err(error)) => {
                tracing::debug!("Liked Songs via session: {error:#}; asking Web API");
                None
            }
            Err(_) => {
                tracing::debug!("Liked Songs via session timed out; asking Web API");
                None
            }
        }
    }

    /// Страница с `offset`; None — сессия не смогла (дальше — Web API).
    pub async fn next_page(&mut self, offset: u32) -> Option<Page<SavedTrack>> {
        let session = self.session.clone()?;
        let page = self.page(offset, LIKED_PAGE, |uris| {
            let session = session.clone();
            async move { collection::tracks(&session, &uris).await }
        });
        match tokio::time::timeout(PAGE_TIMEOUT, page).await {
            Ok(Ok(page)) => Some(page),
            Ok(Err(Failure::Retry(error))) => {
                tracing::debug!("Liked Songs page via session: {error:#}; asking Web API");
                None
            }
            Ok(Err(Failure::Definitive(error))) => {
                tracing::debug!("Liked Songs page via session: {error}; asking Web API");
                None
            }
            Err(_) => {
                tracing::debug!("Liked Songs page via session timed out; asking Web API");
                None
            }
        }
    }

    /// Страница с `offset` не длиннее `limit`; `resolve` — подробности треков по URI (нет в ответе —
    /// Spotify трек больше не отдаёт).
    pub async fn page<F, Fut>(&mut self, offset: u32, limit: u32, mut resolve: F) -> Result<Page<SavedTrack>, Failure>
    where
        F: FnMut(Vec<String>) -> Fut,
        Fut: Future<Output = Result<HashMap<String, Track>, Failure>>,
    {
        let len = self.rows.len();
        let mut cursor = self.starts.get(&offset).copied().unwrap_or(offset as usize).min(len);
        let mut items = Vec::new();
        while items.len() < limit as usize && cursor < len {
            let end = (cursor + limit as usize - items.len()).min(len);
            let window = &self.rows[cursor..end];
            let found = resolve(window.iter().map(|row| row.uri.clone()).collect()).await?;
            for row in window {
                match found.get(&row.uri) {
                    Some(track) => items.push(SavedTrack { added_at: row.added_at_iso(), track: track.clone() }),
                    None => self.skipped += 1,
                }
            }
            cursor = end;
        }
        let more = cursor < len;
        if more {
            self.starts.insert(offset + limit, cursor);
        }
        let shown = offset as usize + items.len();
        let total = len.saturating_sub(self.skipped).max(shown + usize::from(more));
        Ok(Page {
            items,
            total: u32::try_from(total).unwrap_or(u32::MAX),
            limit,
            offset,
            next: more.then(|| "session".to_owned()),
        })
    }
}

// --- альбомы и исполнители ---------------------------------------------------------------------------

/// Сохранённые альбомы, новые сверху. Альбом, которого Spotify больше не отдаёт, пропускается.
pub async fn saved_albums(session: &sf_player::Session) -> anyhow::Result<Vec<SavedAlbum>> {
    let rows: Vec<CollectionItem> = collection::collection_all(session, SET_COLLECTION)
        .await?
        .into_iter()
        .filter(|row| row.uri.starts_with("spotify:album:"))
        .collect();
    let uris: Vec<String> = rows.iter().map(|row| row.uri.clone()).collect();
    let mut albums = collection::albums(session, &uris).await?;
    Ok(rows
        .iter()
        .filter_map(|row| Some(SavedAlbum { added_at: row.added_at_iso(), album: albums.remove(&row.uri)? }))
        .collect())
}

/// Исполнители, на которых подписан аккаунт, новые подписки сверху.
pub async fn followed_artists(session: &sf_player::Session) -> anyhow::Result<Vec<Artist>> {
    let rows = collection::collection_all(session, SET_ARTIST).await?;
    let uris: Vec<String> = rows.iter().map(|row| row.uri.clone()).collect();
    let mut artists = collection::artists(session, &uris).await?;
    Ok(uris.iter().filter_map(|uri| artists.remove(uri)).collect())
}

/// Чтение раздела через сессию с ожиданием; None — сессии нет или она не смогла (спросить Web API).
pub async fn read<T, Fut>(
    api: &ApiGateway,
    player: &PlayerHandle,
    what: &str,
    read: impl FnOnce(sf_player::Session) -> Fut,
) -> Option<T>
where
    Fut: Future<Output = anyhow::Result<T>>,
{
    let session = route::account_session(api, player)?;
    match tokio::time::timeout(READ_TIMEOUT, read(session)).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            tracing::debug!("{what} via session: {error:#}; asking Web API");
            None
        }
        Err(_) => {
            tracing::debug!("{what} via session timed out; asking Web API");
            None
        }
    }
}

// --- запись и проверка -------------------------------------------------------------------------------

/// Сохранить (`add`) или снять треки, альбомы, исполнителей, подкасты, выпуски: сначала сессия
/// (каждый вид — в своём наборе), потом Web API /me/library не дольше WEB_DEADLINE.
pub async fn write(api: &ApiGateway, player: &PlayerHandle, uris: &[String], add: bool) -> Result<(), ApiError> {
    if let Some(session) = route::account_session(api, player) {
        match tokio::time::timeout(SESSION_TIMEOUT, collection::write(&session, uris, add)).await {
            Ok(Ok(())) => {
                // запись мимо Web API: его кэш GET (20 с) отдал бы запасному чтению старую коллекцию
                api.forget_cached();
                return Ok(());
            }
            Ok(Err(error)) => tracing::debug!("collection write via session: {error:#}; writing via Web API"),
            Err(_) => tracing::debug!("collection write via session timed out; writing via Web API"),
        }
    }
    web(async {
        let client = api.client_for(Operation::UserData).await?;
        if add { client.save(uris).await } else { client.unsave(uris).await }
    })
    .await
}

/// Как /me/library/contains — для каждого URI в том же порядке: сначала сессия, потом Web API.
pub async fn contains(api: &ApiGateway, player: &PlayerHandle, uris: &[String]) -> Result<Vec<bool>, ApiError> {
    if let Some(session) = route::account_session(api, player) {
        match tokio::time::timeout(SESSION_TIMEOUT, collection::contains(&session, uris)).await {
            Ok(Ok(found)) => return Ok(found),
            Ok(Err(error)) => tracing::debug!("collection check via session: {error:#}; asking Web API"),
            Err(_) => tracing::debug!("collection check via session timed out; asking Web API"),
        }
    }
    web(async { api.client_for(Operation::UserData).await?.contains(uris).await }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(count: usize) -> Vec<CollectionItem> {
        let mut rows: Vec<CollectionItem> = (0..count)
            .map(|n| CollectionItem { uri: format!("spotify:track:{n}"), added_at: 1_700_000_000 - n as i64 })
            .collect();
        rows.push(CollectionItem { uri: "spotify:album:x".into(), added_at: 1_800_000_000 });
        rows
    }

    /// Подробности всех URI, кроме `gone`.
    fn resolver(gone: &'static [&'static str]) -> impl FnMut(Vec<String>) -> std::future::Ready<Result<HashMap<String, Track>, Failure>> {
        move |uris| {
            std::future::ready(Ok(uris
                .into_iter()
                .filter(|uri| !gone.contains(&uri.as_str()))
                .map(|uri| (uri.clone(), Track { uri, ..Default::default() }))
                .collect()))
        }
    }

    #[tokio::test]
    async fn pages_follow_the_collection_and_skip_albums() {
        let mut feed = LikedFeed::new(rows(5));
        let first = feed.page(0, 2, resolver(&[])).await.unwrap();
        assert_eq!(first.items.iter().map(|i| i.track.uri.as_str()).collect::<Vec<_>>(), ["spotify:track:0", "spotify:track:1"]);
        assert_eq!(first.total, 5);
        assert_eq!(first.next_offset(), Some(2));
        assert_eq!(first.items[0].added_at.as_deref(), Some("2023-11-14T22:13:20Z"));
        let second = feed.page(2, 2, resolver(&[])).await.unwrap();
        let last = feed.page(4, 2, resolver(&[])).await.unwrap();
        assert_eq!(second.items[0].track.uri, "spotify:track:2");
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.next_offset(), None);
    }

    #[tokio::test]
    async fn gone_tracks_are_skipped_and_the_page_is_topped_up() {
        let mut feed = LikedFeed::new(rows(6));
        let first = feed.page(0, 3, resolver(&["spotify:track:1"])).await.unwrap();
        assert_eq!(
            first.items.iter().map(|i| i.track.uri.as_str()).collect::<Vec<_>>(),
            ["spotify:track:0", "spotify:track:2", "spotify:track:3"]
        );
        assert_eq!(first.total, 5);
        let rest = feed.page(3, 3, resolver(&["spotify:track:1"])).await.unwrap();
        assert_eq!(rest.items.iter().map(|i| i.track.uri.as_str()).collect::<Vec<_>>(), ["spotify:track:4", "spotify:track:5"]);
        assert_eq!(rest.total, 5, "the last page's total matches the rows shown");
        assert_eq!(rest.next_offset(), None);
    }

    #[tokio::test]
    async fn a_long_collection_pages_in_batches_of_fifty_and_skips_gone_rows_across_pages() {
        // 130 треков, пропавшие — на границах пакетов (49, 50) и в конце (129)
        let gone: &'static [&'static str] = &["spotify:track:49", "spotify:track:50", "spotify:track:129"];
        let mut feed = LikedFeed::new(rows(130));
        let batches = std::cell::RefCell::new(Vec::new());
        let mut shown = Vec::new();
        let mut offset = 0;
        let mut pages = 0;
        let mut last_total = 130;
        loop {
            let mut resolve = resolver(gone);
            let page = feed
                .page(offset, LIKED_PAGE, |uris| {
                    batches.borrow_mut().push(uris.len());
                    resolve(uris)
                })
                .await
                .unwrap();
            pages += 1;
            assert!(page.items.len() <= LIKED_PAGE as usize);
            shown.extend(page.items.iter().map(|item| item.track.uri.clone()));
            // пропавшие строки вычитаются из общего числа, как только встретились
            assert!(page.total >= 127 && page.total <= last_total, "{}", page.total);
            last_total = page.total;
            match page.next_offset() {
                Some(next) => {
                    assert_eq!(next, offset + LIKED_PAGE);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(pages, 3);
        assert_eq!(last_total, 127, "the last page's total is exact");
        assert_eq!(shown.len(), 127);
        assert!(gone.iter().all(|uri| !shown.iter().any(|s| s == uri)));
        let unique: std::collections::HashSet<&String> = shown.iter().collect();
        assert_eq!(unique.len(), shown.len(), "no row twice");
        assert!(batches.borrow().iter().all(|&n| n <= LIKED_PAGE as usize));
        // страница не короче лимита, пока строки есть: первая добрала пакетами (49 + 0 + 1)
        assert_eq!(batches.borrow()[..3], [50, 1, 1]);
    }

    #[tokio::test]
    async fn a_batch_that_fails_fails_its_page_and_the_page_can_be_asked_again() {
        let mut feed = LikedFeed::new(rows(60));
        let first = feed.page(0, 50, resolver(&[])).await.unwrap();
        assert_eq!(first.next_offset(), Some(50));
        let failed = feed
            .page(50, 50, |_| std::future::ready(Err(Failure::Retry(anyhow::anyhow!("network")))))
            .await;
        assert!(matches!(failed, Err(Failure::Retry(_))));
        let again = feed.page(50, 50, resolver(&[])).await.unwrap();
        assert_eq!(again.items.len(), 10);
        assert_eq!(again.items[0].track.uri, "spotify:track:50");
        assert_eq!(again.next_offset(), None);
    }

    #[tokio::test]
    async fn an_empty_collection_is_one_empty_page() {
        let mut feed = LikedFeed::new(Vec::new());
        let page = feed.page(0, 50, resolver(&[])).await.unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        assert_eq!(page.next_offset(), None);
    }
}
