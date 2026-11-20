//! Известные треки на весь процесс: строки для оптимистичных вставок (добавить в плейлист по URI) и «записи»
//! (ISRC): один трек на разных рынках имеет разные URI, лайк любого — лайк всех.
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

use md3_model::TrackItem;
use sf_api::models::{Album, ArtistRef, Image, Track};

const KEPT: usize = 5000;

#[derive(Default)]
struct Known {
    tracks: HashMap<String, TrackItem>,
    order: VecDeque<String>,
    recordings: HashMap<String, String>,
}

static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Mutex::default);

fn known() -> std::sync::MutexGuard<'static, Known> {
    crate::lock(&KNOWN)
}

/// Запомнить строки (обновляет уже известные).
pub fn remember<'a>(items: impl IntoIterator<Item = &'a TrackItem>) {
    let mut known = known();
    for item in items {
        let uri = item.play_uri();
        if uri.is_empty() {
            continue;
        }
        if known.tracks.insert(uri.clone(), item.clone()).is_none() {
            known.order.push_back(uri);
            if known.order.len() > KEPT
                && let Some(old) = known.order.pop_front()
            {
                known.tracks.remove(&old);
            }
        }
    }
}

/// Запомнить «запись» трека Web API (ISRC или linked_from).
pub fn remember_recordings<'a>(tracks: impl IntoIterator<Item = &'a Track>) {
    let mut known = known();
    for track in tracks {
        if let Some(key) = track.recording_key()
            && !track.uri.is_empty()
        {
            if let Some(linked) = &track.linked_from
                && !linked.uri.is_empty()
            {
                known.recordings.insert(linked.uri.clone(), key.clone());
            }
            known.recordings.insert(track.uri.clone(), key);
        }
    }
}

pub fn get(uri: &str) -> Option<TrackItem> {
    known().tracks.get(uri).cloned()
}

pub fn recording(uri: &str) -> Option<String> {
    known().recordings.get(uri).cloned()
}

/// URI трека по id или URI.
pub fn track_uri(id_or_uri: &str) -> String {
    if id_or_uri.starts_with("spotify:") { id_or_uri.to_owned() } else { format!("spotify:track:{id_or_uri}") }
}

/// Строка QML → модель Web API (для кэша «Любимых треков»).
pub fn track_from_item(item: &TrackItem) -> Track {
    let images = [(&item.cover_large, 640), (&item.cover, 300)]
        .into_iter()
        .filter(|(url, _)| !url.is_empty())
        .map(|(url, px)| Image { url: url.clone(), width: Some(px), height: Some(px) })
        .collect();
    Track {
        id: (!item.track_id.is_empty()).then(|| item.track_id.clone()),
        name: item.title.clone(),
        uri: item.play_uri(),
        duration_ms: item.duration_ms.max(0) as u32,
        explicit: item.explicit,
        artists: item
            .artist_refs
            .iter()
            .map(|a| ArtistRef {
                id: (!a.id.is_empty()).then(|| a.id.clone()),
                name: a.name.clone(),
                uri: (!a.id.is_empty()).then(|| format!("spotify:artist:{}", a.id)),
            })
            .collect(),
        album: Some(Album {
            id: item.album_id.clone(),
            name: item.album.clone(),
            uri: if item.album_id.is_empty() { String::new() } else { format!("spotify:album:{}", item.album_id) },
            images,
            ..Default::default()
        }),
        is_playable: Some(item.available),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_specific_uris_share_one_recording() {
        let mut first = Track { uri: "spotify:track:known-a".into(), ..Default::default() };
        first.external_ids.isrc = Some("GB0001".into());
        let mut second = Track { uri: "spotify:track:known-b".into(), ..Default::default() };
        second.external_ids.isrc = Some("GB0001".into());
        remember_recordings([&first, &second]);
        assert_eq!(recording("spotify:track:known-a"), recording("spotify:track:known-b"));
        assert!(recording("spotify:track:known-c").is_none());
    }

    #[test]
    fn a_row_round_trips_through_the_web_api_model() {
        let item = TrackItem {
            track_id: "t1".into(),
            title: "Песня".into(),
            album_id: "a1".into(),
            cover: "c300".into(),
            duration_ms: 1000,
            available: true,
            ..Default::default()
        };
        let track = track_from_item(&item);
        assert_eq!(track.uri, "spotify:track:t1");
        assert_eq!(track.album.unwrap().uri, "spotify:album:a1");
        remember([&item]);
        assert_eq!(get("spotify:track:t1").unwrap().title, "Песня");
        assert_eq!(track_uri("t1"), "spotify:track:t1");
    }
}
