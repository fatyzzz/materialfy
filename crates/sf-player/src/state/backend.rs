//! Ввод-вывод ядра: Web API (через ApiGateway) и локальный движок. Трейты нужны, чтобы тесты подставляли
//! фейковый движок и фейковый Web API.
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use md3_model::TrackItem;
use sf_api::models::{Page, Track};
use sf_api::{ApiError, ApiGateway, Operation};
use sf_api::util::{uri_id, uri_kind};

use super::core::{ApiCall, ApiReply, FetchedQueue, RemoteAction, RemoteState};
use super::map;
use crate::engine::vis::AudioTap;
use crate::engine::{Engine, PlayerCommand};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Web API и загрузка обложек.
pub trait WebBackend: Send + Sync {
    fn call(&self, call: ApiCall) -> BoxFuture<ApiReply>;
    fn fetch_cover(&self, url: String) -> BoxFuture<Result<PathBuf, String>>;
}

/// Локальный движок librespot с точки зрения ядра.
pub trait LocalEngine: Send + Sync {
    fn command(&self, command: PlayerCommand) -> Result<(), String>;
    /// уровень звука 0..1 для «дыхания» обложки
    fn level(&self) -> f64;
    /// Гейт уровня: пока false, движок не наполняет tap (AudioTap::set_wanted).
    fn set_levels_wanted(&self, _wanted: bool) {}
}

/// Engine + его tap уровня.
pub struct EngineLink {
    pub engine: Engine,
    pub tap: Arc<AudioTap>,
}

impl LocalEngine for EngineLink {
    fn command(&self, command: PlayerCommand) -> Result<(), String> {
        self.engine.command(command).map_err(|error| format!("{error:#}"))
    }

    fn level(&self) -> f64 {
        self.tap.level()
    }

    fn set_levels_wanted(&self, wanted: bool) {
        self.tap.set_wanted(wanted);
    }
}

/// Настоящий Web API Spotify.
pub struct SpotifyBackend {
    api: Arc<ApiGateway>,
    http: sf_api::Http,
    cover_dir: PathBuf,
}

impl SpotifyBackend {
    pub fn new(api: Arc<ApiGateway>, http: sf_api::Http, cover_dir: PathBuf) -> Self {
        Self { api, http, cover_dir }
    }
}

impl WebBackend for SpotifyBackend {
    fn call(&self, call: ApiCall) -> BoxFuture<ApiReply> {
        let api = self.api.clone();
        Box::pin(async move { run(api, call).await })
    }

    fn fetch_cover(&self, url: String) -> BoxFuture<Result<PathBuf, String>> {
        let http = self.http.clone();
        let dir = self.cover_dir.clone();
        Box::pin(async move { crate::engine::cover::fetch_cover(&http, &dir, &url).await })
    }
}

async fn run(api: Arc<ApiGateway>, call: ApiCall) -> ApiReply {
    let operation = match call {
        ApiCall::AlbumTracks { .. } | ApiCall::TrackInfo { .. } => Operation::Catalog,
        _ => Operation::Playback,
    };
    let client = api.client_for(operation).await;
    match call {
        ApiCall::Playback { seq } => ApiReply::Playback {
            seq,
            result: match client {
                Ok(client) => client.playback_state().await.map(|state| state.map(RemoteState::from_api)),
                Err(error) => Err(error),
            },
        },
        ApiCall::Queue { seq } => ApiReply::Queue {
            seq,
            result: match client {
                Ok(client) => client.queue().await.map(FetchedQueue::from_api),
                Err(error) => Err(error),
            },
        },
        ApiCall::Remote { action, device } => {
            let result = match client {
                Ok(client) => {
                    let device = device.as_deref();
                    match &action {
                        RemoteAction::Play => client.play(device, None).await,
                        RemoteAction::Pause => client.pause(device).await,
                        RemoteAction::Next => client.next(device).await,
                        RemoteAction::Previous => client.previous(device).await,
                        RemoteAction::Seek(position) => client.seek(*position, device).await,
                        RemoteAction::Volume(percent) => client.set_volume(*percent, device).await,
                        RemoteAction::Shuffle(on) => client.set_shuffle(*on, device).await,
                        RemoteAction::Repeat(mode) => client.set_repeat(mode.api_name(), device).await,
                    }
                }
                Err(error) => Err(error),
            };
            ApiReply::Remote { action, result }
        }
        ApiCall::Play { device, request, shuffle } => {
            let result = match client {
                Ok(client) => {
                    let device = device.as_deref();
                    // перемешивание и запуск — одним упорядоченным обменом, иначе shuffle иногда проигрывает
                    let shuffled = if shuffle { client.set_shuffle(true, device).await } else { Ok(()) };
                    match shuffled {
                        Ok(()) => client.play(device, Some(&request)).await,
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            };
            ApiReply::Remote { action: RemoteAction::Play, result }
        }
        ApiCall::Transfer { device, play } => {
            let (name, result) = match client {
                Ok(client) => {
                    let name = client
                        .devices()
                        .await
                        .ok()
                        .and_then(|devices| devices.into_iter().find(|known| known.id.as_deref() == Some(&device)))
                        .map(|known| known.name);
                    (name, client.transfer(&device, play).await)
                }
                Err(error) => (None, Err(error)),
            };
            ApiReply::Transferred { device, name, result }
        }
        ApiCall::AddMany { request, uris, device } => {
            let (added, result) = match client {
                Ok(client) => client.add_many_to_queue(&uris, device.as_deref()).await,
                Err(error) => (0, Err(error)),
            };
            ApiReply::QueueBatchAdded { request, added, result }
        }
        ApiCall::AlbumTracks { request, id } => {
            let result = match client {
                Ok(client) => {
                    album_tracks(|offset| {
                        let client = client.clone();
                        let id = id.clone();
                        async move { client.album_tracks(&id, offset, 50).await }
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            ApiReply::AlbumTracks { request, result }
        }
        ApiCall::TrackInfo { request, uri } => {
            let result = match client {
                Ok(client) => {
                    let id = uri_id(&uri).unwrap_or_default().to_owned();
                    let fetched = if uri_kind(&uri) == Some("episode") {
                        client.episode(&id).await.map(|episode| sf_api::map::episode_track(&episode))
                    } else {
                        client.track(&id).await.map(|track| sf_api::map::track_item(&track))
                    };
                    fetched.map(|mut item| {
                        // ответ мог прийти с другим URI (рынок/точная версия) — оставляем тот, что играет
                        item.uri = uri.clone();
                        item.track_id = id.clone();
                        item
                    })
                }
                Err(error) => Err(error),
            };
            ApiReply::TrackInfo { request, uri, result }
        }
    }
}

fn out_of_order() -> ApiError {
    ApiError::Decode("album tracks arrived out of order, try again".into())
}

/// Все страницы альбома по порядку — целиком или ошибка (частичный альбом в очередь не попадает).
pub async fn album_tracks<F, Fut>(mut fetch: F) -> Result<Vec<TrackItem>, ApiError>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<Page<Track>, ApiError>>,
{
    let mut offset = 0;
    let mut tracks = Vec::new();
    loop {
        let page = fetch(offset).await?;
        if page.offset != offset {
            return Err(out_of_order());
        }
        let next = page.next_offset();
        if page.next.is_some() && next.is_none() {
            return Err(out_of_order());
        }
        tracks.extend(page.items.iter().map(map::track));
        match next {
            Some(next) if next <= offset => return Err(out_of_order()),
            Some(next) => offset = next,
            None => return Ok(tracks),
        }
    }
}
