//! Статистика прослушиваний этого ПК из локальной истории (journal): время за сегодня, неделю, месяц и всё
//! записанное, по дням и топ треков и исполнителей. Записи без played_ms (старая история) считаются по
//! длительности трека.
use std::collections::HashMap;

use jiff::Zoned;
use jiff::civil::Date;
use md3_model::TrackItem;

use crate::history::PlayRecord;

/// Сколько позиций в топах.
const TOP: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningStats {
    /// секунды за сегодня, эту неделю (с понедельника), этот месяц и всю историю
    pub today: i64,
    pub week: i64,
    pub month: i64,
    pub total: i64,
    pub plays: usize,
    /// дата первой записи (YYYY-MM-DD); "" — истории нет
    pub since: String,
    /// последние дни, старые первыми, сегодня последним
    pub days: Vec<DayTotal>,
    pub top_tracks: Vec<TopEntry>,
    pub top_artists: Vec<TopEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotal {
    /// YYYY-MM-DD
    pub date: String,
    pub seconds: i64,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopEntry {
    /// трек: spotify:track:…; исполнитель: id ("" — неизвестен)
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover: String,
    pub plays: usize,
    pub seconds: i64,
}

fn played_seconds(play: &PlayRecord) -> i64 {
    let ms = if play.played_ms > 0 { play.played_ms } else { play.track.duration_ms.max(0) };
    ms / 1000
}

/// Исполнители трека: (ключ, id, имя). Без artist_refs — имена из строки «a, b».
fn artists_of(track: &TrackItem) -> Vec<(String, String, String)> {
    if !track.artist_refs.is_empty() {
        return track
            .artist_refs
            .iter()
            .filter(|artist| !artist.name.is_empty() || !artist.id.is_empty())
            .map(|artist| {
                let key = if artist.id.is_empty() { artist.name.clone() } else { artist.id.clone() };
                (key, artist.id.clone(), artist.name.clone())
            })
            .collect();
    }
    track
        .artists
        .split(", ")
        .filter(|name| !name.is_empty())
        .map(|name| (name.to_owned(), String::new(), name.to_owned()))
        .collect()
}

fn add(top: &mut HashMap<String, TopEntry>, key: String, fresh: impl FnOnce() -> TopEntry, seconds: i64) {
    let entry = top.entry(key).or_insert_with(fresh);
    entry.plays += 1;
    entry.seconds += seconds;
}

fn ranked(top: HashMap<String, TopEntry>) -> Vec<TopEntry> {
    let mut entries: Vec<TopEntry> = top.into_values().collect();
    entries.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.seconds.cmp(&a.seconds)).then(a.title.cmp(&b.title)));
    entries.truncate(TOP);
    entries
}

/// Статистика на момент `now` (его часовой пояс задаёт границы дней); `days` — сколько последних дней.
pub fn listening(plays: &[PlayRecord], now: &Zoned, days: usize) -> ListeningStats {
    let today = now.date();
    let week_start = today
        .checked_sub(jiff::Span::new().days(i64::from(today.weekday().to_monday_zero_offset())))
        .unwrap_or(today);
    let month_start = today.first_of_month();
    let first_day = today.checked_sub(jiff::Span::new().days(days.saturating_sub(1) as i64)).unwrap_or(today);
    let mut per_day: HashMap<Date, i64> = HashMap::new();
    let mut tracks: HashMap<String, TopEntry> = HashMap::new();
    let mut artists: HashMap<String, TopEntry> = HashMap::new();
    let mut stats = ListeningStats::default();
    let mut earliest: Option<Date> = None;
    for play in plays {
        let seconds = played_seconds(play);
        stats.total += seconds;
        stats.plays += 1;
        if let Ok(at) = play.played_at.parse::<jiff::Timestamp>() {
            let date = at.to_zoned(now.time_zone().clone()).date();
            earliest = Some(earliest.map_or(date, |held| held.min(date)));
            if date == today {
                stats.today += seconds;
            }
            if date >= week_start && date <= today {
                stats.week += seconds;
            }
            if date >= month_start && date <= today {
                stats.month += seconds;
            }
            if date >= first_day && date <= today {
                *per_day.entry(date).or_default() += seconds;
            }
        }
        let track = &play.track;
        add(
            &mut tracks,
            track.play_uri(),
            || TopEntry {
                id: track.play_uri(),
                title: track.title.clone(),
                subtitle: track.artists.clone(),
                cover: track.cover.clone(),
                ..Default::default()
            },
            seconds,
        );
        for (key, id, name) in artists_of(track) {
            add(&mut artists, key, || TopEntry { id, title: name, ..Default::default() }, seconds);
        }
    }
    stats.since = earliest.map(|date| date.to_string()).unwrap_or_default();
    stats.days = (0..days)
        .filter_map(|offset| first_day.checked_add(jiff::Span::new().days(offset as i64)).ok())
        .map(|date| DayTotal { date: date.to_string(), seconds: per_day.get(&date).copied().unwrap_or(0) })
        .collect();
    stats.top_tracks = ranked(tracks);
    stats.top_artists = ranked(artists);
    stats
}

#[cfg(test)]
mod tests {
    use md3_model::ArtistRef;

    use super::*;

    fn play(uri: &str, artist: &str, at: &str, played_ms: i64) -> PlayRecord {
        PlayRecord {
            track: TrackItem {
                uri: uri.to_owned(),
                title: uri.to_owned(),
                artists: artist.to_owned(),
                artist_refs: vec![ArtistRef { id: artist.to_owned(), name: artist.to_owned() }],
                duration_ms: 200_000,
                ..Default::default()
            },
            played_at: at.to_owned(),
            played_ms,
            ..Default::default()
        }
    }

    // среда, 10 сентября 2026, по UTC
    fn now() -> Zoned {
        "2026-09-10T20:00:00[UTC]".parse().unwrap()
    }

    #[test]
    fn totals_split_by_today_week_month_and_all() {
        let plays = vec![
            play("spotify:track:a", "x", "2026-09-10T10:00:00Z", 60_000),
            play("spotify:track:a", "x", "2026-09-08T10:00:00Z", 120_000),
            play("spotify:track:b", "y", "2026-09-02T10:00:00Z", 30_000),
            play("spotify:track:b", "y", "2026-08-31T10:00:00Z", 45_000),
        ];
        let stats = listening(&plays, &now(), 14);
        assert_eq!(stats.today, 60);
        assert_eq!(stats.week, 180, "the week starts on Monday the 7th");
        assert_eq!(stats.month, 210);
        assert_eq!(stats.total, 255);
        assert_eq!(stats.plays, 4);
        assert_eq!(stats.since, "2026-08-31");
    }

    #[test]
    fn old_records_without_played_time_count_the_track_length() {
        let stats = listening(&[play("spotify:track:a", "x", "2026-09-10T10:00:00Z", 0)], &now(), 7);
        assert_eq!(stats.today, 200);
    }

    #[test]
    fn days_run_oldest_first_and_end_today_in_the_local_zone() {
        let plays = vec![
            play("spotify:track:a", "x", "2026-09-10T01:00:00Z", 60_000),
            play("spotify:track:a", "x", "2026-09-09T10:00:00Z", 30_000),
            play("spotify:track:a", "x", "2026-08-01T10:00:00Z", 30_000),
        ];
        let stats = listening(&plays, &now(), 3);
        let days: Vec<(&str, i64)> = stats.days.iter().map(|day| (day.date.as_str(), day.seconds)).collect();
        assert_eq!(days, [("2026-09-08", 0), ("2026-09-09", 30), ("2026-09-10", 60)]);
        // в Сан-Паулу (UTC−3) 01:00Z десятого — ещё девятое
        let sao_paulo: Zoned = "2026-09-10T17:00:00[America/Sao_Paulo]".parse().unwrap();
        let stats = listening(&plays, &sao_paulo, 3);
        let days: Vec<i64> = stats.days.iter().map(|day| day.seconds).collect();
        assert_eq!(days, [0, 90, 0]);
    }

    #[test]
    fn tops_rank_by_plays_then_time_and_credit_every_artist() {
        let mut duet = play("spotify:track:duet", "x", "2026-09-10T12:00:00Z", 100_000);
        duet.track.artist_refs.push(ArtistRef { id: "z".into(), name: "z".into() });
        let plays = vec![
            play("spotify:track:a", "x", "2026-09-10T10:00:00Z", 10_000),
            play("spotify:track:a", "x", "2026-09-10T11:00:00Z", 10_000),
            play("spotify:track:b", "y", "2026-09-10T10:30:00Z", 150_000),
            duet,
        ];
        let stats = listening(&plays, &now(), 7);
        let tracks: Vec<(&str, usize)> = stats.top_tracks.iter().map(|t| (t.id.as_str(), t.plays)).collect();
        assert_eq!(tracks, [("spotify:track:a", 2), ("spotify:track:b", 1), ("spotify:track:duet", 1)]);
        let artists: Vec<(&str, usize, i64)> =
            stats.top_artists.iter().map(|a| (a.title.as_str(), a.plays, a.seconds)).collect();
        assert_eq!(artists, [("x", 3, 120), ("y", 1, 150), ("z", 1, 100)]);
    }

    #[test]
    fn no_history_gives_empty_days_and_no_start() {
        let stats = listening(&[], &now(), 7);
        assert_eq!(stats.days.len(), 7);
        assert!(stats.days.iter().all(|day| day.seconds == 0));
        assert_eq!(stats.since, "");
        assert!(stats.top_tracks.is_empty());
    }
}
