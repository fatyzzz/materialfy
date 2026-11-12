//! Полки главной из главной Spotify и страницы «Для тебя» (data::home_feed), запасной топ из локальной истории
//! и недавнее Spotify как записи истории: чистые функции без сети.
use std::collections::{HashMap, HashSet};

use md3_model::{ArtistItem, EpisodeItem, HomeShelf, LibraryEntry, PlayContext, TrackItem, contexts};

use crate::data::home_feed::{FeedCard, FeedSection, RecentContext};
use crate::history::PlayRecord;

/// Ключи полок из разделов Spotify: "feed:<id раздела>" (id стабильны — скрытие полки переживает обновления).
pub const FEED_PREFIX: &str = "feed:";
/// Разделы главной Spotify, проверенные вживую (2026-10); id одинаковы у всех аккаунтов.
/// «Только для тебя, …» (homeview): Discover Weekly и др. — в «Сделано для вас».
pub const MADE_FOR_SECTION: &str = "spotify:section:0JQ5DAUnp4wcj0bCb3wh3S";
/// «Любимые исполнители» — запасной «Топ-исполнители».
pub const FAVORITE_ARTISTS_SECTION: &str = "spotify:section:0JQ5DAnM3wGh0gz1MXnu3z";
/// «Слушать музыку» — одиночные треки, плитками не играются.
const START_LISTENING_SECTION: &str = "spotify:section:0JQ5DBa4E2rlwCe7ty23US";
const SKIPPED_SECTIONS: &[&str] = &[MADE_FOR_SECTION, FAVORITE_ARTISTS_SECTION, START_LISTENING_SECTION];
/// Раздел из одной карточки — хвост главной («Для фанатов …» по одному плейлисту), полкой не показываем.
const MIN_FEED_TILES: usize = 2;
pub const FEED_TILES: usize = 16;
pub const MADE_FOR_YOU_TILES: usize = 24;
/// Из одного раздела «Для тебя» (жанровых миксов там до сотни).
pub const MADE_FOR_YOU_PER_SECTION: usize = 6;
/// format плейлистов «Для тебя», которым не место среди миксов (станции — своя полка).
const NOT_MIXES: &[&str] = &["concerts-near-you", "inspiredby-mix"];
/// Самые личные — первыми в «Сделано для вас» (миксы дня приходят без format и дочитываются сессией).
const FIRST_FORMATS: &[&str] =
    &["discover-weekly", "release-radar", "daily-mix", "daylist", "on-repeat", "repeat-rewind", crate::data::home_feed::RESOLVED];
const SUBTITLE_CHARS: usize = 90;

fn short(text: &str) -> String {
    if text.chars().count() <= SUBTITLE_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().take(SUBTITLE_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Плитка главной по карточке Spotify; выпуски и треки плитками не бывают.
pub fn card_entry(card: &FeedCard) -> Option<LibraryEntry> {
    if card.title.is_empty() {
        return None;
    }
    let (kind, subtitle) = match card.kind() {
        "playlist" => {
            let owner = if card.owner.is_empty() { "Spotify" } else { card.owner.as_str() };
            let subtitle = if card.subtitle.is_empty() { mf_i18n::tf("Playlist · {0}", &[&owner]) } else { short(&card.subtitle) };
            ("playlist", subtitle)
        }
        "album" if card.subtitle.is_empty() => ("album", mf_i18n::t("Album")),
        "album" => ("album", mf_i18n::tf("Album · {0}", &[&card.subtitle])),
        "artist" => ("artist", mf_i18n::t("Artist")),
        "show" if card.subtitle.is_empty() => ("show", mf_i18n::t("Podcast")),
        "show" => ("show", mf_i18n::tf("Podcast · {0}", &[&card.subtitle])),
        _ => return None,
    };
    let owner_id = if kind == "playlist" && card.owner_id.is_empty() { "spotify".to_owned() } else { card.owner_id.clone() };
    Some(LibraryEntry {
        kind: kind.to_owned(),
        id: card.id().to_owned(),
        uri: card.uri.clone(),
        title: card.title.clone(),
        subtitle,
        cover: card.cover.clone(),
        covers: if card.cover.is_empty() { Vec::new() } else { vec![card.cover.clone()] },
        owner: card.owner.clone(),
        owner_id: if kind == "playlist" { owner_id } else { String::new() },
        read_only: true,
        ..Default::default()
    })
}

pub fn card_artist(card: &FeedCard) -> ArtistItem {
    ArtistItem {
        artist_id: card.id().to_owned(),
        uri: card.uri.clone(),
        name: card.title.clone(),
        cover: card.cover.clone(),
        cover_large: card.cover.clone(),
        ..Default::default()
    }
}

pub fn card_episode(card: &FeedCard) -> EpisodeItem {
    EpisodeItem {
        episode_id: card.id().to_owned(),
        uri: card.uri.clone(),
        title: card.title.clone(),
        show_name: card.subtitle.clone(),
        cover: card.cover.clone(),
        cover_large: card.cover.clone(),
        ..Default::default()
    }
}

/// Плейлист Spotify с подписью, не из NOT_MIXES.
fn is_playlist(card: &FeedCard) -> bool {
    card.kind() == "playlist" && !card.title.is_empty() && !NOT_MIXES.contains(&card.format.as_str())
}

/// «Сделано для вас»: раздел «Только для тебя» главной (homeview: Discover Weekly…) и страница «Для тебя» —
/// по разделам, ≤ 6 из каждого; Discover Weekly, «Радар», миксы дня, daylist, «На повторе» — первыми.
/// Из страницы — только миксы с format (у «Videos For You» его нет).
pub fn made_for_you_tiles(home: &[FeedSection], browse: &[FeedSection]) -> Vec<LibraryEntry> {
    let from_home = home.iter().filter(|s| s.uri == MADE_FOR_SECTION).flat_map(|s| s.cards.iter()).filter(|c| is_playlist(c));
    let from_browse = browse.iter().flat_map(|s| {
        s.cards.iter().filter(|c| is_playlist(c) && !c.format.is_empty()).take(MADE_FOR_YOU_PER_SECTION)
    });
    let mut cards: Vec<&FeedCard> = from_home.chain(from_browse).collect();
    // устойчивая сортировка: порядок Spotify внутри групп сохраняется
    cards.sort_by_key(|c| !(c.format.is_empty() || FIRST_FORMATS.contains(&c.format.as_str())));
    let mut seen = HashSet::new();
    cards
        .into_iter()
        .filter(|c| seen.insert(c.uri.clone()))
        .filter_map(card_entry)
        .take(MADE_FOR_YOU_TILES)
        .collect()
}

/// The DJ's tile, first in «Made for you»: it plays the DJ rather than
/// opening a page, since the DJ has no track list to show.
pub fn dj_entry(cover: &str) -> LibraryEntry {
    LibraryEntry {
        kind: "dj".to_owned(),
        id: crate::data::home_feed::DJ_URI.rsplit(':').next().unwrap_or_default().to_owned(),
        uri: crate::data::home_feed::DJ_URI.to_owned(),
        title: "DJ".to_owned(),
        subtitle: mf_i18n::t("Music and commentary picked for you"),
        cover: cover.to_owned(),
        covers: if cover.is_empty() { Vec::new() } else { vec![cover.to_owned()] },
        owner: "Spotify".to_owned(),
        owner_id: "spotify".to_owned(),
        read_only: true,
        ..Default::default()
    }
}

/// «Made for you» with the DJ first when the account has it, and only once.
pub fn with_dj(tiles: Vec<LibraryEntry>, dj_cover: Option<&str>) -> Vec<LibraryEntry> {
    let Some(cover) = dj_cover else {
        return tiles;
    };
    std::iter::once(dj_entry(cover))
        .chain(tiles.into_iter().filter(|tile| tile.uri != crate::data::home_feed::DJ_URI))
        .collect()
}

fn section_id(uri: &str) -> &str {
    uri.rsplit(':').next().unwrap_or(uri)
}

/// Разделы главной Spotify — полками с заголовками Spotify, в его порядке, как в телефоне.
pub fn feed_shelves(home: &[FeedSection]) -> Vec<HomeShelf> {
    home.iter()
        .filter(|s| !s.title.is_empty() && !SKIPPED_SECTIONS.contains(&s.uri.as_str()))
        .filter_map(section_shelf)
        .filter(|shelf| shelf.items.len() >= MIN_FEED_TILES)
        .collect()
}

fn section_shelf(section: &FeedSection) -> Option<HomeShelf> {
    let cards: Vec<&FeedCard> = section.cards.iter().filter(|c| !c.title.is_empty()).take(FEED_TILES).collect();
    let all = |kind: &str| !cards.is_empty() && cards.iter().all(|c| c.kind() == kind);
    let (kind, items): (&str, Vec<serde_json::Value>) = if all("artist") {
        ("artists", cards.iter().map(|c| card_artist(c)).filter_map(|a| serde_json::to_value(a).ok()).collect())
    } else if all("episode") {
        ("episodes", cards.iter().map(|c| card_episode(c)).filter_map(|e| serde_json::to_value(e).ok()).collect())
    } else {
        ("tiles", cards.iter().filter_map(|c| card_entry(c)).filter_map(|e| serde_json::to_value(e).ok()).collect())
    };
    (!items.is_empty()).then(|| HomeShelf {
        key: format!("{FEED_PREFIX}{}", section_id(&section.uri)),
        title: section.title.clone(),
        kind: kind.to_owned(),
        items,
    })
}

/// «Любимые исполнители» главной Spotify (запасной «Топ-исполнители»).
pub fn favorite_artists(home: &[FeedSection]) -> Vec<ArtistItem> {
    home.iter()
        .filter(|s| s.uri == FAVORITE_ARTISTS_SECTION)
        .flat_map(|s| s.cards.iter())
        .filter(|c| c.kind() == "artist" && !c.title.is_empty())
        .map(card_artist)
        .collect()
}

/// Запасной «Топ-треки»: чаще слушаемые треки истории (новые первыми), при равенстве — недавние.
pub fn top_tracks_from_plays(plays: &[TrackItem], limit: usize) -> Vec<TrackItem> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (index, track) in plays.iter().enumerate() {
        if track.is_episode || track.uri.is_empty() {
            continue;
        }
        counts.entry(track.uri.as_str()).or_insert((0, index)).0 += 1;
    }
    let mut ranked: Vec<(&str, (usize, usize))> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));
    ranked.into_iter().take(limit).map(|(_, (_, index))| plays[index].clone()).collect()
}

/// Запасной «Топ-исполнители» по истории; обложки берутся из `known` (исполнители, у кого они есть).
pub fn top_artists_from_plays(plays: &[TrackItem], known: &[ArtistItem], limit: usize) -> Vec<ArtistItem> {
    let mut counts: HashMap<&str, (usize, usize, &str)> = HashMap::new();
    let mut order = 0;
    for track in plays.iter().filter(|t| !t.is_episode) {
        for artist in track.artist_refs.iter().filter(|a| !a.id.is_empty() && !a.name.is_empty()) {
            let entry = counts.entry(artist.id.as_str()).or_insert((0, order, artist.name.as_str()));
            entry.0 += 1;
            order += 1;
        }
    }
    let mut ranked: Vec<(&str, (usize, usize, &str))> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(id, (_, _, name))| {
            known.iter().find(|a| a.artist_id == id).cloned().unwrap_or_else(|| ArtistItem {
                artist_id: id.to_owned(),
                uri: format!("spotify:artist:{id}"),
                name: name.to_owned(),
                ..Default::default()
            })
        })
        .collect()
}

/// Вид контекста по URI недавнего Spotify.
fn context_kind(uri: &str) -> &'static str {
    if uri.ends_with(":collection") {
        return contexts::LIKED;
    }
    match sf_api::util::uri_kind(uri) {
        Some("playlist") => contexts::PLAYLIST,
        Some("album") => contexts::ALBUM,
        Some("artist") => contexts::ARTIST,
        Some("show") => contexts::SHOW,
        Some("station") => contexts::RADIO,
        _ => contexts::UNKNOWN,
    }
}

/// Недавние контексты Spotify → записи истории: подписи и обложки — из `cards` (lookup), трек — последний
/// сыгранный в контексте (если Spotify его назвал).
pub fn recent_records(recent: &[RecentContext], cards: &[FeedCard]) -> Vec<PlayRecord> {
    let card = |uri: &str| cards.iter().find(|c| c.uri == uri);
    recent
        .iter()
        .filter_map(|r| {
            let played_at = jiff::Timestamp::from_millisecond(r.played_at_ms).ok()?.to_string();
            let context = card(&r.uri);
            let track = card(&r.last_track_uri).filter(|c| c.kind() == "track" || c.kind() == "episode");
            let track = TrackItem {
                track_id: track.map(|c| c.id().to_owned()).unwrap_or_default(),
                title: track.map(|c| c.title.clone()).unwrap_or_default(),
                artists: track.map(|c| c.subtitle.clone()).unwrap_or_default(),
                cover: track.map(|c| c.cover.clone()).unwrap_or_default(),
                uri: r.last_track_uri.clone(),
                is_episode: r.last_track_uri.starts_with("spotify:episode:"),
                available: true,
                ..Default::default()
            };
            let mut ctx = PlayContext::with_uri(context_kind(&r.uri), r.uri.clone());
            if let Some(c) = context {
                ctx = ctx.titled(c.title.clone()).with_cover(c.cover.clone());
            }
            Some(PlayRecord { track, context: ctx, played_at })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use md3_model::ArtistRef;

    use super::*;

    fn card(uri: &str, title: &str) -> FeedCard {
        FeedCard { uri: uri.into(), title: title.into(), cover: format!("https://{title}"), ..Default::default() }
    }

    fn mix(uri: &str, title: &str, format: &str) -> FeedCard {
        FeedCard { made_for: true, format: format.into(), owner: "Spotify".into(), owner_id: "spotify".into(), ..card(uri, title) }
    }

    fn section(uri: &str, title: &str, cards: Vec<FeedCard>) -> FeedSection {
        FeedSection { uri: uri.into(), title: title.into(), cards }
    }

    #[test]
    fn the_dj_leads_made_for_you_when_the_account_has_it() {
        let dj = crate::data::home_feed::DJ_URI;
        let tiles = vec![
            LibraryEntry { uri: "spotify:playlist:dw".into(), ..Default::default() },
            LibraryEntry { uri: dj.into(), kind: "playlist".into(), ..Default::default() },
        ];
        assert_eq!(with_dj(tiles.clone(), None), tiles);

        let with = with_dj(tiles, Some("https://cover"));
        let uris: Vec<&str> = with.iter().map(|t| t.uri.as_str()).collect();
        assert_eq!(uris, [dj, "spotify:playlist:dw"]);
        assert_eq!(with[0].kind, "dj");
        assert_eq!(with[0].id, "37i9dQZF1EYkqdzj48dyYq");
        assert_eq!(with[0].cover, "https://cover");
        assert_eq!(with[0].title, "DJ");
    }

    #[test]
    fn made_for_you_puts_the_most_personal_first() {
        let home = vec![
            // madeFor бывает и у редакционных: из прочих разделов главной ничего не берётся
            section("spotify:section:other", "Хиты", vec![mix("spotify:playlist:hits", "Hot Hits", "editorial")]),
            section(MADE_FOR_SECTION, "Только для тебя", vec![card("spotify:playlist:dw", "Открытия недели")]),
        ];
        let niche: Vec<FeedCard> =
            (0..10).map(|i| mix(&format!("spotify:playlist:n{i}"), &format!("Mix {i}"), "descripto")).collect();
        let browse = vec![
            section("spotify:section:n", "Your Niche Mixes", niche),
            section("spotify:section:p", "Персональные подборки", vec![
                mix("spotify:playlist:day", "daylist", "daylist"),
                FeedCard { uri: "spotify:playlist:unresolved".into(), ..Default::default() },
                mix("spotify:playlist:cn", "Концерты поблизости", "concerts-near-you"),
                mix("spotify:playlist:video", "Videos For You", ""),
            ]),
            section("spotify:section:new", "Новая музыка", vec![mix("spotify:playlist:dw", "Открытия недели", "discover-weekly")]),
        ];
        let tiles = made_for_you_tiles(&home, &browse);
        let ids: Vec<_> = tiles.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids[..3], ["dw", "day", "n0"], "concerts, videos and unresolved are skipped, repeats appear once");
        assert_eq!(tiles.len(), 2 + MADE_FOR_YOU_PER_SECTION);
        assert_eq!((tiles[0].kind.as_str(), tiles[0].subtitle.as_str(), tiles[0].owner_id.as_str()), ("playlist", "Playlist · Spotify", "spotify"));
        assert!(tiles[0].read_only);
    }

    #[test]
    fn feed_shelves_keep_spotify_sections_in_order() {
        let home = vec![
            section("spotify:section:stations", "Рекомендуемые станции", vec![
                mix("spotify:playlist:r1", "IMANU Radio", "inspiredby-mix"),
                mix("spotify:playlist:r2", "KK Radio", "inspiredby-mix"),
            ]),
            section(FAVORITE_ARTISTS_SECTION, "Любимые исполнители", vec![card("spotify:artist:a1", "IMANU")]),
            section("spotify:section:like", "Похоже на Muse", vec![card("spotify:album:al", "Drones"), mix("spotify:playlist:p", "Muse Radio", "")]),
            section("spotify:section:tail", "Для фанатов Muse", vec![mix("spotify:playlist:t", "This Is Muse", "")]),
            section("spotify:section:mixes", "Миксы", vec![mix("spotify:playlist:m", "Mix", "topic-mix"), card("spotify:episode:e", "Ep")]),
            section("spotify:section:episodes", "Выпуски для тебя", vec![card("spotify:episode:e1", "Ep 1"), card("spotify:episode:e2", "Ep 2")]),
            section("spotify:section:artists", "Лучшие треки", vec![card("spotify:artist:a2", "Muse"), card("spotify:artist:a3", "Queen")]),
        ];
        let shelves = feed_shelves(&home);
        let keys: Vec<_> = shelves.iter().map(|s| (s.key.as_str(), s.kind.as_str(), s.title.as_str())).collect();
        assert_eq!(keys, [
            ("feed:stations", "tiles", "Рекомендуемые станции"),
            ("feed:like", "tiles", "Похоже на Muse"),
            ("feed:episodes", "episodes", "Выпуски для тебя"),
            ("feed:artists", "artists", "Лучшие треки"),
        ], "one-card sections and a mix shelf left with one tile are dropped");
        assert_eq!(shelves[0].items[0]["subtitle"], "Playlist · Spotify");
        assert_eq!(shelves[1].items[0]["kind"], "album");
        assert_eq!(shelves[3].items[0]["artistId"], "a2");
        assert_eq!(favorite_artists(&home)[0].name, "IMANU");
    }

    fn track(id: &str, artist: &str) -> TrackItem {
        TrackItem {
            track_id: id.into(),
            uri: format!("spotify:track:{id}"),
            artist_refs: vec![ArtistRef { id: artist.into(), name: artist.to_uppercase() }],
            ..Default::default()
        }
    }

    #[test]
    fn top_from_history_counts_plays_then_recency() {
        let plays = vec![track("1", "a"), track("2", "b"), track("2", "b"), track("3", "a"), track("1", "a")];
        let tracks: Vec<_> = top_tracks_from_plays(&plays, 10).into_iter().map(|t| t.track_id).collect();
        assert_eq!(tracks, ["1", "2", "3"]);
        let known = [ArtistItem { artist_id: "b".into(), name: "B".into(), cover: "https://b".into(), ..Default::default() }];
        let artists = top_artists_from_plays(&plays, &known, 10);
        assert_eq!(artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(artists[0].uri, "spotify:artist:a");
        assert_eq!(artists[1].cover, "https://b");
    }

    #[test]
    fn recent_contexts_become_history_records() {
        let recent = vec![
            RecentContext { uri: "spotify:playlist:p".into(), played_at_ms: 1_760_000_000_000, last_track_uri: "spotify:track:t".into() },
            RecentContext { uri: "spotify:user:me:collection".into(), played_at_ms: 1_759_000_000_000, last_track_uri: String::new() },
        ];
        let cards = vec![
            card("spotify:playlist:p", "Мой плейлист"),
            FeedCard { subtitle: "Muse".into(), ..card("spotify:track:t", "Uprising") },
        ];
        let records = recent_records(&recent, &cards);
        assert_eq!(records.len(), 2);
        assert_eq!((records[0].context.kind.as_str(), records[0].context.title.as_str()), ("playlist", "Мой плейлист"));
        assert_eq!((records[0].track.title.as_str(), records[0].track.artists.as_str()), ("Uprising", "Muse"));
        assert_eq!(records[0].played_at, "2025-10-09T08:53:20Z");
        assert_eq!(records[1].context.kind, contexts::LIKED);
    }
}
