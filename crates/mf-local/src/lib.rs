//! Локальная музыка: обход папки и теги. Блокирующий код: звать из spawn_blocking.
//!
//! Кэш — один JSON: путь → (mtime, размер, трек). Файл, у которого они не изменились,
//! не перечитывается, так что большая папка при повторном запуске не читается заново.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};

/// Только то, что умеет декодировать плеер (mp3, flac, ogg/vorbis)
const EXTENSIONS: [&str; 4] = ["mp3", "flac", "ogg", "oga"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalTrack {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: u32,
    pub duration_ms: u32,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    mtime: u64,
    size: u64,
    track: LocalTrack,
}

type Cache = HashMap<String, CacheEntry>;

/// Читает теги одного файла; `None` — файл не открылся, в список он не попадает.
type ReadTags = fn(&Path) -> Option<LocalTrack>;

/// Обходит `root` и возвращает треки по исполнителю, альбому, номеру и названию.
/// `cache_file` — куда писать кэш обхода (без него каждый вызов читает все теги).
pub fn scan(root: &Path, cache_file: Option<&Path>) -> Vec<LocalTrack> {
    scan_with(root, cache_file, read_tags)
}

fn scan_with(root: &Path, cache_file: Option<&Path>, read: ReadTags) -> Vec<LocalTrack> {
    let mut cache: Cache = cache_file
        .and_then(|file| fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut files = Vec::new();
    walk(root, &mut files);

    let mut fresh = Cache::new();
    for file in files {
        let Ok(meta) = fs::metadata(&file) else { continue };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let key = file.to_string_lossy().into_owned();
        let entry = match cache.remove(&key) {
            Some(old) if old.mtime == mtime && old.size == meta.len() => old,
            _ => match read(&file) {
                Some(track) => CacheEntry { mtime, size: meta.len(), track },
                None => continue,
            },
        };
        fresh.insert(key, entry);
    }

    if let Some(file) = cache_file {
        match serde_json::to_vec(&fresh) {
            Ok(bytes) => {
                if let Err(err) = fs::write(file, bytes) {
                    tracing::warn!("local scan cache not saved to {}: {err}", file.display());
                }
            }
            Err(err) => tracing::warn!("local scan cache not serialized: {err}"),
        }
    }

    let mut tracks: Vec<LocalTrack> = fresh.into_values().map(|entry| entry.track).collect();
    tracks.sort_by_cached_key(|t| (t.artist.to_lowercase(), t.album.to_lowercase(), t.track_number, t.title.to_lowercase()));
    tracks
}

/// Симлинки не обходим: цикл в папке не должен зависнуть обход
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, out);
        } else if kind.is_file() && is_supported(&path) {
            out.push(path);
        }
    }
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

fn read_tags(path: &Path) -> Option<LocalTrack> {
    let file = lofty::read_from_path(path)
        .inspect_err(|err| tracing::debug!("local file skipped {}: {err}", path.display()))
        .ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    let text = |value: Option<std::borrow::Cow<'_, str>>| value.map(|v| v.trim().to_owned()).unwrap_or_default();
    let mut title = text(tag.and_then(|t| t.title()));
    if title.is_empty() {
        title = path.file_stem()?.to_string_lossy().into_owned();
    }
    Some(LocalTrack {
        path: path.to_string_lossy().into_owned(),
        title,
        artist: text(tag.and_then(|t| t.artist())),
        album: text(tag.and_then(|t| t.album())),
        track_number: tag.and_then(|t| t.track()).unwrap_or(0),
        duration_ms: u32::try_from(file.properties().duration().as_millis()).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // por thread: os testes rodam em paralelo e um contador global misturaria as leituras
    thread_local!(static READS: Cell<usize> = const { Cell::new(0) });

    fn fake(path: &Path) -> Option<LocalTrack> {
        READS.with(|reads| reads.set(reads.get() + 1));
        let name = path.file_stem()?.to_string_lossy().into_owned();
        // "artista-album-num" no nome do arquivo, só para montar a ordenação no teste
        let mut parts = name.split('-');
        Some(LocalTrack {
            path: path.to_string_lossy().into_owned(),
            title: name.clone(),
            artist: parts.next()?.into(),
            album: parts.next()?.into(),
            track_number: parts.next()?.parse().ok()?,
            duration_ms: 1000,
        })
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mf-local-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_supported_extensions_in_subfolders_sorted_by_artist_album_number() {
        let dir = temp_dir("sorted");
        fs::create_dir_all(dir.join("sub")).unwrap();
        for name in ["b-x-2.mp3", "b-x-1.FLAC", "a-z-1.ogg", "sub/a-y-9.mp3", "a-z-2.txt", "a-z-3.m4a"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let titles: Vec<String> = scan_with(&dir, None, fake).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["a-y-9", "a-z-1", "b-x-1", "b-x-2"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchanged_files_come_from_the_cache_and_changed_ones_are_read_again() {
        let dir = temp_dir("cache");
        let cache = dir.join("scan.json");
        let first = dir.join("a-x-1.mp3");
        fs::write(&first, b"x").unwrap();
        fs::write(dir.join("a-x-2.mp3"), b"x").unwrap();

        assert_eq!(scan_with(&dir, Some(&cache), fake).len(), 2);
        assert_eq!(READS.with(Cell::get), 2);

        assert_eq!(scan_with(&dir, Some(&cache), fake).len(), 2);
        assert_eq!(READS.with(Cell::get), 2, "second scan reads no tags");

        fs::write(&first, b"changed size").unwrap();
        assert_eq!(scan_with(&dir, Some(&cache), fake).len(), 2);
        assert_eq!(READS.with(Cell::get), 3, "only the changed file is read again");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_the_reader_rejects_is_left_out() {
        let dir = temp_dir("bad");
        fs::write(dir.join("broken.mp3"), b"x").unwrap();
        assert!(scan_with(&dir, None, |_| None).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
