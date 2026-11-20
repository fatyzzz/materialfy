//! Локальная история прослушиваний (перенос spotifast/src/history.rs): librespot-воспроизведение Spotify
//! не записывает, поэтому прослушивания этого ПК хранятся в файле и сливаются с /me/player/recently-played.
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::history::PlayRecord;

/// Прослушивание засчитывается после 30 с или половины короткого трека.
const COUNTS_AFTER: Duration = Duration::from_secs(30);

/// Сколько локальных прослушиваний хранить.
pub const KEPT: usize = 500;

/// Своя запись и запись Spotify об этом же треке в пределах минуты — одно прослушивание.
const SAME_PLAY: i64 = 60;

pub fn counts_after(duration_ms: i64) -> Duration {
    let half = Duration::from_millis(duration_ms.max(0) as u64 / 2);
    COUNTS_AFTER.min(half).max(Duration::from_secs(1))
}

/// Прослушивания этого ПК, новые первыми.
#[derive(Default)]
pub struct Journal {
    plays: Vec<PlayRecord>,
    /// в памяти не то, что на диске
    pub dirty: bool,
}

impl Journal {
    /// Нечитаемый файл — пустая история.
    pub fn load(path: &Path) -> Self {
        let plays = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<PlayRecord>>(&text).ok())
            .unwrap_or_default();
        Self { plays, dirty: false }
    }

    pub fn plays(&self) -> &[PlayRecord] {
        &self.plays
    }

    /// Записать, если менялось; неудачная запись остаётся на следующий раз.
    pub fn save(&mut self, path: &Path) {
        if !self.dirty {
            return;
        }
        match serde_json::to_vec(&self.plays) {
            Ok(bytes) => match super::fsio::write_atomic(path, &bytes) {
                Ok(()) => self.dirty = false,
                Err(error) => tracing::warn!("could not write history: {error:#}"),
            },
            Err(error) => tracing::warn!("could not write history: {error:#}"),
        }
    }

    pub fn record(&mut self, record: PlayRecord) {
        self.plays.insert(0, record);
        self.plays.truncate(KEPT);
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        if !self.plays.is_empty() {
            self.plays.clear();
            self.dirty = true;
        }
    }
}

fn seconds(play: &PlayRecord) -> Option<i64> {
    play.played_at.parse::<jiff::Timestamp>().ok().map(|at| at.as_second())
}

/// Слияние локальной истории и истории Spotify, новые первыми. Запись Spotify рядом (≤ 60 с) со своей —
/// то же прослушивание; записи одного источника между собой не сливаются. Без времени — в конец.
pub fn merged(local: &[PlayRecord], remote: &[PlayRecord]) -> Vec<PlayRecord> {
    let mut unmatched: HashMap<&str, Vec<i64>> = HashMap::new();
    let mut out: Vec<(Option<i64>, PlayRecord)> = Vec::new();
    for play in local {
        let at = seconds(play);
        if let Some(at) = at {
            unmatched.entry(play.track.uri.as_str()).or_default().push(at);
        }
        out.push((at, play.clone()));
    }
    for play in remote {
        let at = seconds(play);
        if let Some(at) = at
            && let Some(times) = unmatched.get_mut(play.track.uri.as_str())
            && let Some(nearest) = times
                .iter()
                .enumerate()
                .filter(|&(_, &held)| (held - at).abs() <= SAME_PLAY)
                .min_by_key(|&(_, &held)| (held - at).abs())
                .map(|(index, _)| index)
        {
            times.swap_remove(nearest);
            continue;
        }
        out.push((at, play.clone()));
    }
    out.sort_by(|a, b| match (a.0, b.0) {
        (Some(a), Some(b)) => b.cmp(&a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    out.into_iter().map(|(_, play)| play).collect()
}

/// Дополнить ранее загруженную историю Spotify страницей: повтор (uri, время) отбрасывается.
pub fn absorb_remote(known: &mut Vec<PlayRecord>, page: Vec<PlayRecord>) -> usize {
    let mut added = 0;
    for play in page {
        if !known.iter().any(|k| k.track.uri == play.track.uri && k.played_at == play.played_at) {
            known.push(play);
            added += 1;
        }
    }
    known.sort_by(|a, b| b.played_at.cmp(&a.played_at));
    added
}

#[cfg(test)]
mod tests {
    use md3_model::TrackItem;

    use super::*;

    fn play(uri: &str, at: &str) -> PlayRecord {
        PlayRecord {
            track: TrackItem { uri: uri.to_owned(), ..Default::default() },
            played_at: at.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn a_song_counts_after_half_a_minute_or_half_of_it() {
        assert_eq!(counts_after(240_000).as_secs(), 30);
        assert_eq!(counts_after(40_000).as_secs(), 20);
        assert!(counts_after(0) >= Duration::from_secs(1));
    }

    #[test]
    fn the_two_histories_interleave_by_time() {
        let local = vec![
            play("spotify:track:here-late", "2026-09-01T15:00:00Z"),
            play("spotify:track:here-early", "2026-09-01T09:00:00Z"),
        ];
        let remote = vec![play("spotify:track:phone", "2026-09-01T12:00:00Z")];
        let uris: Vec<String> = merged(&local, &remote).into_iter().map(|p| p.track.uri).collect();
        assert_eq!(uris, ["spotify:track:here-late", "spotify:track:phone", "spotify:track:here-early"]);
    }

    #[test]
    fn recents_keep_a_song_played_twice() {
        let local = vec![play("spotify:track:a", "2026-09-01T15:00:00Z"), play("spotify:track:a", "2026-09-01T09:00:00Z")];
        assert_eq!(merged(&local, &[]).len(), 2);
    }

    #[test]
    fn one_play_seen_twice_is_one_row() {
        let local = vec![play("spotify:track:a", "2026-09-01T15:00:00Z")];
        let remote = vec![play("spotify:track:a", "2026-09-01T15:00:20Z")];
        assert_eq!(merged(&local, &remote).len(), 1);
        let distant = vec![play("spotify:track:a", "2026-09-01T15:05:00Z")];
        assert_eq!(merged(&local, &distant).len(), 2);
    }

    #[test]
    fn each_repeat_of_a_short_song_is_its_own_row() {
        let remote = vec![play("spotify:track:s", "2026-09-01T15:00:40Z"), play("spotify:track:s", "2026-09-01T15:00:00Z")];
        assert_eq!(merged(&[], &remote).len(), 2);
        assert_eq!(merged(&remote, &[]).len(), 2);
        let reported =
            vec![play("spotify:track:s", "2026-09-01T15:00:45Z"), play("spotify:track:s", "2026-09-01T15:00:05Z")];
        assert_eq!(merged(&remote, &reported).len(), 2);
    }

    #[test]
    fn a_play_with_no_time_sinks_to_the_end() {
        let local = vec![play("spotify:track:timeless", ""), play("spotify:track:a", "2026-09-01T09:00:00Z")];
        let uris: Vec<String> = merged(&local, &[]).into_iter().map(|p| p.track.uri).collect();
        assert_eq!(uris, ["spotify:track:a", "spotify:track:timeless"]);
    }

    #[test]
    fn recents_drop_a_play_that_arrives_twice() {
        let mut known = vec![play("spotify:track:a", "2026-09-01T09:00:00Z")];
        let added = absorb_remote(
            &mut known,
            vec![play("spotify:track:a", "2026-09-01T09:00:00Z"), play("spotify:track:b", "2026-09-01T10:00:00Z")],
        );
        assert_eq!(added, 1);
        assert_eq!(known[0].track.uri, "spotify:track:b");
    }

    #[test]
    fn the_newest_play_is_first_and_the_list_is_capped() {
        let mut journal = Journal::default();
        for index in 0..KEPT + 10 {
            journal.record(play(&format!("spotify:track:{index}"), "2026-09-01T09:00:00Z"));
        }
        assert_eq!(journal.plays().len(), KEPT);
        assert_eq!(journal.plays()[0].track.uri, format!("spotify:track:{}", KEPT + 9));
    }

    #[test]
    fn a_saved_history_reads_back_and_a_failed_save_keeps_it_dirty() {
        let dir = std::env::temp_dir().join(format!("sf-history-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "").unwrap();
        let mut journal = Journal::default();
        journal.record(play("spotify:track:a", "2026-09-01T09:00:00Z"));
        journal.save(&blocked.join("history.json"));
        assert!(journal.dirty);
        journal.record(play("spotify:track:b", "2026-09-01T10:00:00Z"));
        let path = dir.join("history.json");
        journal.save(&path);
        assert!(!journal.dirty);
        let read = Journal::load(&path);
        let uris: Vec<&str> = read.plays().iter().map(|p| p.track.uri.as_str()).collect();
        assert_eq!(uris, ["spotify:track:b", "spotify:track:a"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
