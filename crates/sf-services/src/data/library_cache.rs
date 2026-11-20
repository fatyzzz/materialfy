//! Дерево плейлистов аккаунта на диске: пока сессия поднимается (на медленной сети это десятки секунд),
//! библиотека показывает прошлый список вместо пустого экрана. Свежий rootlist из сессии перезаписывает кэш.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sf_api::models::Playlist;
use sf_player::RootlistEntry;

const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedLibrary {
    pub version: u32,
    /// строки rootlist в порядке Spotify (маркеры папок включительно)
    pub rootlist: Vec<RootlistEntry>,
    /// шапки плейлистов: имя, владелец, обложка, число треков
    pub playlists: Vec<Playlist>,
}

impl CachedLibrary {
    pub fn new(rootlist: Vec<RootlistEntry>, playlists: Vec<Playlist>) -> Self {
        Self { version: VERSION, rootlist, playlists }
    }

    /// Пустое дерево кэш не заменяет: пустая библиотека на экране хуже старой.
    fn valid(&self) -> bool {
        self.version == VERSION && !self.rootlist.is_empty()
    }
}

/// <playlist_cache_dir(account)>/library.json
pub fn path(dir: &Path) -> PathBuf {
    dir.join("library.json")
}

pub fn read(path: &Path) -> Option<CachedLibrary> {
    let bytes = std::fs::read(path).ok()?;
    let cache: CachedLibrary = serde_json::from_slice(&bytes).ok()?;
    cache.valid().then_some(cache)
}

pub fn write(path: &Path, cache: &CachedLibrary) {
    if !cache.valid() {
        return;
    }
    match serde_json::to_vec(cache) {
        Ok(bytes) => {
            if let Err(error) = super::fsio::write_atomic(path, &bytes) {
                tracing::debug!("library cache not written: {error:#}");
            }
        }
        Err(error) => tracing::debug!("library cache not written: {error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!("sf-library-{:016x}", rand::random::<u64>()))
    }

    #[test]
    fn a_cache_reads_back_whole() {
        let dir = dir();
        let file = path(&dir);
        let entries = vec![
            RootlistEntry::Playlist("spotify:playlist:a".into()),
            RootlistEntry::FolderStart { id: "f1".into(), name: "Jazz".into() },
            RootlistEntry::FolderEnd,
        ];
        let playlists = vec![Playlist { id: "a".into(), name: "Mix".into(), ..Default::default() }];
        write(&file, &CachedLibrary::new(entries.clone(), playlists.clone()));
        let read_back = read(&file).expect("the cache is there");
        assert_eq!(read_back.rootlist, entries);
        assert_eq!(read_back.playlists, playlists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_tree_is_not_written_over_a_good_cache() {
        let dir = dir();
        let file = path(&dir);
        let entries = vec![RootlistEntry::Playlist("spotify:playlist:a".into())];
        write(&file, &CachedLibrary::new(entries.clone(), Vec::new()));
        write(&file, &CachedLibrary::new(Vec::new(), Vec::new()));
        assert_eq!(read(&file).unwrap().rootlist, entries);
        // a cache from another version is ignored (written by hand: `write` refuses invalid ones)
        let mut stale = CachedLibrary::new(entries, Vec::new());
        stale.version = 0;
        std::fs::write(&file, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(read(&file).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
