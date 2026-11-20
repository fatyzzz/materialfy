//! Полки главной аккаунта на диске: пока сессия поднимается (на медленной сети это десятки секунд), главная
//! показывает прошлый выпуск вместо пустого экрана. Свежие ответы перезаписывают кэш.
use std::path::Path;

use md3_model::{ArtistItem, EpisodeItem, HomeShelf, LibraryEntry, TrackItem};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedHome {
    pub version: u32,
    /// «Сделано для вас» и личные разделы главной Spotify с их заголовками
    pub made_for_you: Vec<LibraryEntry>,
    pub feed: Vec<HomeShelf>,
    pub favorite_artists: Vec<ArtistItem>,
    pub podcasts: Vec<EpisodeItem>,
    pub top_artists: Vec<ArtistItem>,
    pub top_tracks: Vec<TrackItem>,
}

impl CachedHome {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        made_for_you: Vec<LibraryEntry>,
        feed: Vec<HomeShelf>,
        favorite_artists: Vec<ArtistItem>,
        podcasts: Vec<EpisodeItem>,
        top_artists: Vec<ArtistItem>,
        top_tracks: Vec<TrackItem>,
    ) -> Self {
        Self { version: VERSION, made_for_you, feed, favorite_artists, podcasts, top_artists, top_tracks }
    }

    /// Пустой выпуск кэш не заменяет: пустая главная на экране хуже прошлой.
    fn valid(&self) -> bool {
        self.version == VERSION && !self.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.made_for_you.is_empty()
            && self.feed.is_empty()
            && self.favorite_artists.is_empty()
            && self.podcasts.is_empty()
            && self.top_artists.is_empty()
            && self.top_tracks.is_empty()
    }
}

pub fn read(path: &Path) -> Option<CachedHome> {
    let bytes = std::fs::read(path).ok()?;
    let cache: CachedHome = serde_json::from_slice(&bytes).ok()?;
    cache.valid().then_some(cache)
}

pub fn write(path: &Path, cache: &CachedHome) {
    if !cache.valid() {
        return;
    }
    match serde_json::to_vec(cache) {
        Ok(bytes) => {
            if let Err(error) = super::fsio::write_atomic(path, &bytes) {
                tracing::debug!("home cache not written: {error:#}");
            }
        }
        Err(error) => tracing::debug!("home cache not written: {error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!("sf-home-{:016x}", rand::random::<u64>()))
    }

    fn file(dir: &Path) -> PathBuf {
        dir.join("account.json")
    }

    #[test]
    fn a_cache_reads_back_whole() {
        let dir = dir();
        let file = file(&dir);
        let cache = CachedHome::new(
            vec![LibraryEntry { id: "p".into(), title: "Mix".into(), ..Default::default() }],
            Vec::new(),
            vec![ArtistItem { name: "A".into(), ..Default::default() }],
            Vec::new(),
            vec![ArtistItem { name: "B".into(), ..Default::default() }],
            vec![TrackItem { uri: "spotify:track:1".into(), ..Default::default() }],
        );
        write(&file, &cache);
        assert_eq!(read(&file), Some(cache));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_shelf_set_is_not_written_over_a_good_cache() {
        let dir = dir();
        let file = file(&dir);
        let cache = CachedHome::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), vec![
            TrackItem { uri: "spotify:track:1".into(), ..Default::default() },
        ]);
        write(&file, &cache);
        write(&file, &CachedHome::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()));
        assert_eq!(read(&file), Some(cache));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
