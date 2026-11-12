//! «Недавнее» по дням, как в телефоне: подряд сыгранные треки одного контекста — одна строка с их числом,
//! трек без контекста — своя строка. Источник — общая история (локальная + Spotify), новые первыми.
use jiff::tz::TimeZone;

use crate::history::PlayRecord;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentDay {
    /// YYYY-MM-DD по местному времени; "" — время записи неизвестно
    pub date: String,
    pub rows: Vec<RecentRow>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRow {
    /// URI контекста или трека — что открыть по клику
    pub uri: String,
    /// "playlist" | "album" | "artist" | "show" | … | "track"
    pub kind: String,
    pub title: String,
    /// у трека — исполнители; у контекста пусто (подпись собирает страница)
    pub subtitle: String,
    pub cover: String,
    /// сколько треков подряд из этого контекста
    pub tracks: u32,
    /// последний трек строки (у трека — он сам)
    pub track_uri: String,
}

fn local_date(at: &str, zone: &TimeZone) -> String {
    at.parse::<jiff::Timestamp>().map(|t| t.to_zoned(zone.clone()).date().to_string()).unwrap_or_default()
}

pub fn by_day(plays: &[PlayRecord], zone: &TimeZone) -> Vec<RecentDay> {
    let mut days: Vec<RecentDay> = Vec::new();
    for play in plays {
        let date = local_date(&play.played_at, zone);
        if days.last().is_none_or(|day| day.date != date) {
            days.push(RecentDay { date, rows: Vec::new() });
        }
        let Some(day) = days.last_mut() else { continue };
        let context = play.context.context_uri.as_deref().filter(|uri| !uri.is_empty());
        if let Some(uri) = context
            && let Some(row) = day.rows.last_mut().filter(|row| row.uri == uri)
        {
            row.tracks += 1;
            continue;
        }
        let track = &play.track;
        day.rows.push(match context {
            Some(uri) => RecentRow {
                uri: uri.to_owned(),
                kind: uri.split(':').nth(1).unwrap_or_default().to_owned(),
                title: if play.context.title.is_empty() { track.album.clone() } else { play.context.title.clone() },
                subtitle: String::new(),
                cover: if play.context.cover.is_empty() { track.cover.clone() } else { play.context.cover.clone() },
                tracks: 1,
                track_uri: track.play_uri(),
            },
            None => RecentRow {
                uri: track.play_uri(),
                kind: "track".to_owned(),
                title: track.title.clone(),
                subtitle: track.artists.clone(),
                cover: track.cover.clone(),
                tracks: 1,
                track_uri: track.play_uri(),
            },
        });
    }
    days
}

#[cfg(test)]
mod tests {
    use md3_model::{PlayContext, TrackItem};

    use super::*;

    fn play(track: &str, context: Option<&str>, at: &str) -> PlayRecord {
        let mut ctx = PlayContext::new("playlist");
        ctx.context_uri = context.map(str::to_owned);
        ctx.title = context.map(|_| "Relaxar".to_owned()).unwrap_or_default();
        PlayRecord {
            track: TrackItem { uri: track.to_owned(), title: track.to_owned(), ..Default::default() },
            context: ctx,
            played_at: at.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn songs_in_a_row_from_one_playlist_are_one_row_per_day() {
        let zone = TimeZone::get("America/Sao_Paulo").unwrap();
        let plays = [
            play("spotify:track:b", Some("spotify:playlist:r"), "2026-11-11T22:00:00Z"),
            play("spotify:track:a", Some("spotify:playlist:r"), "2026-11-11T21:55:00Z"),
            play("spotify:track:c", None, "2026-11-11T21:50:00Z"),
            play("spotify:track:d", Some("spotify:playlist:r"), "2026-11-11T02:00:00Z"),
        ];
        let days = by_day(&plays, &zone);
        assert_eq!(days.iter().map(|d| d.date.as_str()).collect::<Vec<_>>(), ["2026-11-11", "2026-11-10"]);
        let today = &days[0].rows;
        assert_eq!(today.len(), 2);
        assert_eq!((today[0].kind.as_str(), today[0].title.as_str(), today[0].tracks), ("playlist", "Relaxar", 2));
        assert_eq!(today[0].track_uri, "spotify:track:b");
        assert_eq!((today[1].kind.as_str(), today[1].uri.as_str()), ("track", "spotify:track:c"));
        // 02:00 UTC — ещё вчера в Сан-Паулу
        assert_eq!(days[1].rows[0].tracks, 1);
    }

    #[test]
    fn the_same_playlist_again_later_is_a_new_row() {
        let zone = TimeZone::UTC;
        let plays = [
            play("spotify:track:a", Some("spotify:playlist:r"), "2026-11-11T22:00:00Z"),
            play("spotify:track:c", None, "2026-11-11T21:00:00Z"),
            play("spotify:track:b", Some("spotify:playlist:r"), "2026-11-11T20:00:00Z"),
        ];
        assert_eq!(by_day(&plays, &zone)[0].rows.len(), 3);
    }
}
