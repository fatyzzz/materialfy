//! Строки плейлистов на диске по аккаунту: <playlist_cache_dir(account)>/<id>.json, действительны только для
//! своего snapshot_id (иначе — промах, плейлист перечитывается).
use std::path::{Path, PathBuf};

use md3_model::TrackItem;
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedPlaylist {
    pub version: u32,
    pub snapshot: String,
    pub total: u32,
    pub items: Vec<TrackItem>,
}

impl CachedPlaylist {
    pub fn new(snapshot: String, items: Vec<TrackItem>) -> Self {
        Self { version: VERSION, snapshot, total: items.len() as u32, items }
    }

    fn valid(&self) -> bool {
        self.version == VERSION && !self.snapshot.is_empty() && self.items.len() == self.total as usize
    }
}

pub fn path(dir: &Path, id: &str) -> PathBuf {
    let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    dir.join(format!("{safe}.json"))
}

pub fn read(path: &Path) -> Option<CachedPlaylist> {
    let bytes = std::fs::read(path).ok()?;
    let cache: CachedPlaylist = serde_json::from_slice(&bytes).ok()?;
    cache.valid().then_some(cache)
}

pub fn write(path: &Path, cache: &CachedPlaylist) {
    match serde_json::to_vec(cache) {
        Ok(bytes) => {
            if let Err(error) = super::fsio::write_atomic(path, &bytes) {
                tracing::debug!("playlist cache not written: {error:#}");
            }
        }
        Err(error) => tracing::debug!("playlist cache not written: {error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_reads_back_only_when_whole() {
        let dir = std::env::temp_dir().join(format!("sf-playlists-{:016x}", rand::random::<u64>()));
        let file = path(&dir, "abc:../x");
        assert_eq!(file.file_name().unwrap(), "abcx.json");
        let rows = vec![TrackItem { uri: "spotify:track:1".into(), ..Default::default() }];
        write(&file, &CachedPlaylist::new("snap".into(), rows.clone()));
        assert_eq!(read(&file).unwrap().items, rows);
        let mut broken = CachedPlaylist::new("snap".into(), rows);
        broken.total = 5;
        write(&file, &broken);
        assert!(read(&file).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
