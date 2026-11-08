//! Тесты очереди и оптимистичного снимка: перенос тестов `mod tests` из spotifast/src/app.rs
//! (правила docs/_reference/queue.md) на ядро этого слоя, с фейковым движком и фейковым Web API.
use std::time::{Duration, Instant};

use md3_model::{PlayContext, TrackItem};
use sf_api::models::Device;
use sf_api::{ApiError, PlayRequest};

use super::core::*;
use crate::engine::{LocalState, LocalTrack, Playback, PlayerCommand, RepeatMode};
use crate::{LocalPlayback, QUEUE_STALE_RETRIES, Target};

fn song(uri: &str) -> TrackItem {
    TrackItem {
        track_id: uri.rsplit(':').next().unwrap_or_default().into(),
        uri: uri.into(),
        available: true,
        ..Default::default()
    }
}

fn named(uri: &str, title: &str) -> TrackItem {
    TrackItem { title: title.into(), ..song(uri) }
}

fn local_track(uri: &str) -> Option<LocalTrack> {
    Some(LocalTrack { uri: uri.into(), ..Default::default() })
}

/// Ядро с готовым локальным движком (как headless_app в spotifast).
fn headless() -> Core {
    let mut core = Core::new(0.5, false);
    core.local_ready = true;
    core
}

fn playing(core: &mut Core, uri: &str) {
    core.local.track = local_track(uri);
    core.local.playback = Playback::Playing;
}

fn loaded_queue(core: &mut Core, current: &str, next: &[&str]) {
    let fetched = FetchedQueue { current: Some(song(current)), next: next.iter().map(|uri| song(uri)).collect(), context: None };
    let current = fetched.current.clone().map(|track| QItem { uid: 1000, track });
    let next = fetched.next.iter().enumerate().map(|(i, track)| QItem { uid: 1001 + i as u64, track: track.clone() });
    core.queue = Some(QueueData { current, next: next.collect() });
}

fn fetched(current: &str, next: &[&str]) -> FetchedQueue {
    FetchedQueue { current: Some(song(current)), next: next.iter().map(|uri| song(uri)).collect(), context: None }
}

fn queue_uris(core: &Core) -> (Option<String>, Vec<String>) {
    let queue = core.queue.as_ref().expect("the queue stays loaded");
    (
        queue.current.as_ref().map(|item| item.uri().to_owned()),
        queue.next.iter().map(|item| item.uri().to_owned()).collect(),
    )
}

fn answer_queue(core: &mut Core, seq: u64, result: Result<FetchedQueue, ApiError>) {
    core.handle_api(ApiReply::Queue { seq, result });
}

fn answer_latest(core: &mut Core, result: Result<FetchedQueue, ApiError>) {
    let seq = core.queue_seq;
    answer_queue(core, seq, result);
}

fn engine_commands(core: &mut Core) -> Vec<PlayerCommand> {
    let effects = std::mem::take(&mut core.effects);
    let mut commands = Vec::new();
    for effect in effects {
        match effect {
            Effect::Engine(command) => commands.push(command),
            other => core.effects.push(other),
        }
    }
    commands
}

/// Только запросы метаданных трека.
fn info_calls(core: &mut Core) -> Vec<ApiCall> {
    api_calls(core).into_iter().filter(|call| matches!(call, ApiCall::TrackInfo { .. })).collect()
}

fn api_calls(core: &mut Core) -> Vec<ApiCall> {
    let effects = std::mem::take(&mut core.effects);
    let mut calls = Vec::new();
    for effect in effects {
        match effect {
            Effect::Api(call) => calls.push(call),
            other => core.effects.push(other),
        }
    }
    calls
}

fn queue_calls(core: &mut Core) -> Vec<ApiCall> {
    api_calls(core)
        .into_iter()
        .filter(|call| matches!(call, ApiCall::AddMany { .. } | ApiCall::AlbumTracks { .. }))
        .collect()
}

fn notices(core: &Core) -> Vec<String> {
    core.effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notice(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn remote_snapshot(device: &str, uri: &str, is_playing: bool) -> RemoteSnapshot {
    RemoteSnapshot {
        state: RemoteState {
            device: Some(Device {
                id: Some(device.into()),
                name: device.into(),
                is_active: true,
                ..Default::default()
            }),
            item: Some(song(uri)),
            is_playing,
            ..Default::default()
        },
        received_at: Instant::now(),
    }
}

// ---- Next и намерения ----------------------------------------------------------------------------

/// Next сразу делает голову очереди текущей.
#[test]
fn next_pops_the_queue_head_into_now_playing() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.next();
    let (current, next) = queue_uris(&core);
    assert_eq!(current.as_deref(), Some("spotify:track:b"));
    assert_eq!(next, vec!["spotify:track:c"]);
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:b"));
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Next]);
}

/// Следующая песня очереди скачивается заранее, пока играет текущая.
#[test]
fn the_next_queue_track_is_preloaded_while_the_current_one_plays() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.local.position_ms = 4_000;
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.tick();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Preload("spotify:track:b".into())]);
    // повторный tick не заказывает ту же песню снова
    core.tick();
    assert!(engine_commands(&mut core).is_empty(), "одного заказа на пару треков достаточно");
}

/// Загрузка вперёд ждёт, пока текущий трек заиграет: сначала качается он сам.
#[test]
fn nothing_is_preloaded_before_the_current_track_plays() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.local.position_ms = 500;
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    core.tick();
    assert!(engine_commands(&mut core).is_empty(), "первые секунды трека — его собственные");

    core.local.playback = Playback::Paused;
    core.local.position_ms = 30_000;
    core.tick();
    assert!(engine_commands(&mut core).is_empty(), "на паузе вперёд не качаем");

    core.local.playback = Playback::Playing;
    core.tick();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Preload("spotify:track:b".into())]);
}

/// Пока движок ещё тянет текущий трек, вперёд не заказываем: полоса нужна ему.
#[test]
fn nothing_is_preloaded_while_the_current_track_is_still_loading() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.local.position_ms = 30_000;
    core.local.loading = true;
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    core.tick();
    assert!(engine_commands(&mut core).is_empty(), "текущий трек ещё качается");

    core.local.loading = false;
    core.tick();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Preload("spotify:track:b".into())]);
}

/// На новом треке заказ вперёд вооружается заново: следующей становится песня после него.
#[test]
fn a_new_track_preloads_its_own_successor() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.local.position_ms = 4_000;
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.tick();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Preload("spotify:track:b".into())]);

    playing(&mut core, "spotify:track:b");
    core.local.position_ms = 4_000;
    loaded_queue(&mut core, "spotify:track:b", &["spotify:track:c"]);
    core.tick();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Preload("spotify:track:c".into())]);
}

/// Плеер-бар называет следующую песню сразу, из строки очереди.
#[test]
fn next_shows_the_next_songs_title_before_the_engine_loads_it() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.queue = Some(QueueData {
        current: Some(QItem { uid: 1, track: song("spotify:track:a") }),
        next: vec![QItem { uid: 2, track: named("spotify:track:b", "Second song") }],
    });
    assert!(core.cached("spotify:track:b").is_none());
    core.next();
    let now = core.now_playing().expect("the next song is on screen");
    assert_eq!(now.track.uri, "spotify:track:b");
    assert_eq!(now.track.title, "Second song");
}

#[test]
fn a_pending_next_keeps_paused_playback_paused() {
    let mut core = headless();
    core.local.track = local_track("spotify:track:a");
    core.local.playback = Playback::Paused;
    core.remember(&song("spotify:track:b"));
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    core.next();
    let now = core.now_playing().unwrap();
    assert_eq!(now.track.uri, "spotify:track:b");
    assert!(!now.playing, "Next while paused does not show “playing”");
}

/// Восстановленная очередь видна, но не служит догадкой, куда попадёт Next.
#[test]
fn next_does_not_guess_from_an_unanchored_queue() {
    let mut core = headless();
    playing(&mut core, "spotify:track:honey");
    core.queue = Some(QueueData { current: None, next: vec![QItem { uid: 1, track: song("spotify:track:stale") }] });
    core.next();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:honey"));
    assert!(core.intent_track.is_none());
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:stale"]);
}

#[test]
fn an_engine_track_report_overrules_the_optimistic_queue_marker() {
    let mut core = headless();
    playing(&mut core, "spotify:track:honey");
    loaded_queue(&mut core, "spotify:track:honey", &["spotify:track:guessed"]);
    core.next();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:guessed"));
    let mut reported = core.local.clone();
    reported.track = local_track("spotify:track:black-dove");
    core.handle_local(reported);
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:black-dove"));
    assert!(core.intent_track.is_none());
}

/// Обновление прежнего трека (догрузились обложка, длительность), пока выбранный ещё грузится, не снимает
/// намерение: иначе на экране мелькал предыдущий трек и возвращался выбранный.
#[test]
fn an_update_of_the_previous_track_keeps_the_chosen_one_on_screen() {
    let mut core = headless();
    playing(&mut core, "spotify:track:honey");
    loaded_queue(&mut core, "spotify:track:honey", &["spotify:track:next"]);
    core.next();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:next"));
    let mut refreshed = core.local.clone();
    refreshed.track = local_track("spotify:track:honey");
    if let Some(track) = refreshed.track.as_mut() {
        track.duration_ms += 1;
    }
    refreshed.track_sequence += 1;
    core.handle_local(refreshed);
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:next"));
    assert!(core.intent_track.is_some());
}

/// Клик по строке переключает экран сразу: выбранный трек показан и грузится, движок ещё молчит.
/// «Играет» при этом не показываем: звука ещё нет, и полоса не должна идти вперёд.
#[test]
fn a_chosen_track_is_shown_before_the_engine_reports_it() {
    let mut core = headless();
    playing(&mut core, "spotify:track:old");
    let items = vec![named("spotify:track:old", "Old"), named("spotify:track:new", "New")];
    core.play_items(items, 1, PlayContext::new("search"));
    let now = core.now_playing().expect("выбранный трек показан сразу");
    assert_eq!(now.track.uri, "spotify:track:new");
    assert!(now.loading, "до ответа движка — спиннер");
    assert!(!now.playing, "трек ещё грузится — «играет» было бы враньём");
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:new"));
    // движок отдал этот трек — теперь он и правда играет
    let mut started = core.local.clone();
    started.track = local_track("spotify:track:new");
    started.playback = Playback::Playing;
    started.track_sequence += 1;
    core.handle_local(started);
    let now = core.now_playing().expect("трек на экране");
    assert!(now.playing && !now.loading, "после старта — играет, спиннер снят");
}

/// Пауза и продолжение того же трека остаются мгновенными: оптимистичный ответ нужен именно им.
#[test]
fn pausing_the_playing_track_stays_instant() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.toggle_play();
    assert!(!core.believed_playing(), "пауза видна сразу, не дожидаясь движка");
}

/// Пока движок грузит выбранный трек, экран держит его, а не предыдущий: холодный старт бывает дольше 6 с.
#[test]
fn a_slow_load_keeps_the_chosen_track_on_screen() {
    let mut core = headless();
    core.local.connected = true;
    playing(&mut core, "spotify:track:old");
    let items = vec![named("spotify:track:old", "Old"), named("spotify:track:new", "New")];
    core.play_items(items, 1, PlayContext::new("search"));
    let mut loading = core.local.clone();
    loading.loading = true;
    core.handle_local(loading);
    core.age_intent(PLAYBACK_HOLD + Duration::from_secs(1));
    core.tick();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:new"));
    // движок отдал выбранный трек — намерение снято, дальше источник истины он сам
    let mut started = core.local.clone();
    started.loading = false;
    started.track = local_track("spotify:track:new");
    started.track_sequence += 1;
    core.handle_local(started);
    assert!(core.intent_track.is_none());
}

/// Загрузка, которая не кончается, отдаёт экран реальному состоянию: спиннер не висит вечно.
#[test]
fn a_load_that_never_finishes_gives_up_at_the_ceiling() {
    let mut core = headless();
    core.local.connected = true;
    playing(&mut core, "spotify:track:old");
    let items = vec![named("spotify:track:old", "Old"), named("spotify:track:new", "New")];
    core.play_items(items, 1, PlayContext::new("search"));
    let mut loading = core.local.clone();
    loading.loading = true;
    core.handle_local(loading);
    core.age_intent(INTENT_CEILING + Duration::from_secs(1));
    core.tick();
    assert!(core.intent_track.is_none());
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:old"));
}

/// Второй клик отменяет первый: на экране последний выбранный, в движок последним ушёл он же.
#[test]
fn a_second_click_replaces_the_pending_track() {
    let mut core = headless();
    core.local.connected = true;
    playing(&mut core, "spotify:track:old");
    let items = vec![named("spotify:track:a", "A"), named("spotify:track:b", "B")];
    core.play_items(items.clone(), 0, PlayContext::new("search"));
    core.play_items(items, 1, PlayContext::new("search"));
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:b"));
    let loads: Vec<PlayerCommand> = engine_commands(&mut core)
        .into_iter()
        .filter(|command| matches!(command, PlayerCommand::Load(_)))
        .collect();
    assert!(
        matches!(loads.as_slice(), [_, PlayerCommand::Load(load)] if load.offset_index == Some(1)),
        "последним в движок ушёл выбранный трек"
    );
}

/// Списки Web API не кладут строки в кэш ядра: клик по такому треку догружает его метаданные, и экран
/// переключается на него ещё до звука, а не на безымянную заглушку.
#[test]
fn a_click_fetches_the_track_it_does_not_know() {
    let mut core = headless();
    playing(&mut core, "spotify:track:old");
    // список из Web API: uri есть, в кэше ядра строки нет
    core.play_uris(vec!["spotify:track:home".into(), "spotify:track:other".into()], 0, PlayContext::new("tracks"));
    let calls = info_calls(&mut core);
    assert!(
        matches!(calls.as_slice(), [ApiCall::TrackInfo { uri, .. }] if uri == "spotify:track:home"),
        "asked for the clicked track: {calls:?}"
    );
    let ApiCall::TrackInfo { request, uri } = calls[0].clone() else { unreachable!() };
    core.handle_api(ApiReply::TrackInfo {
        request,
        uri: uri.clone(),
        result: Ok(TrackItem { title: "Loaded".into(), artists: "Band".into(), ..named(&uri, "Loaded") }),
    });
    let now = core.now_playing().expect("строка показана");
    assert_eq!(now.track.title, "Loaded", "метаданные доехали, пока грузился звук");
    assert!(now.loading);
    // повторно за тот же трек не спрашиваем
    core.play_uris(vec!["spotify:track:home".into()], 0, PlayContext::new("tracks"));
    assert!(info_calls(&mut core).is_empty(), "the row is cached now");
}

/// Строка очереди бывает безымянной заглушкой: такая запись не считается известной, метаданные всё равно
/// спрашиваются — иначе экран так и остался бы с пустым названием.
#[test]
fn a_nameless_cached_row_still_asks_for_metadata() {
    let mut core = headless();
    playing(&mut core, "spotify:track:old");
    core.remember(&song("spotify:track:stub"));
    core.play_uris(vec!["spotify:track:stub".into()], 0, PlayContext::new("tracks"));
    let calls = info_calls(&mut core);
    assert!(
        matches!(calls.as_slice(), [ApiCall::TrackInfo { uri, .. }] if uri == "spotify:track:stub"),
        "asked for the nameless row: {calls:?}"
    );
    let ApiCall::TrackInfo { request, uri } = calls[0].clone() else { unreachable!() };
    core.handle_api(ApiReply::TrackInfo { request, uri: uri.clone(), result: Ok(named(&uri, "Loaded")) });
    assert_eq!(core.cached(&uri).map(|track| track.title.as_str()), Some("Loaded"), "the row is upgraded");
}

/// Безымянная строка очереди, пришедшая, пока ждём ответ, не отменяет запрос: иначе ответ выбросили бы.
#[test]
fn a_nameless_row_does_not_cancel_the_pending_metadata_request() {
    let mut core = headless();
    playing(&mut core, "spotify:track:old");
    core.play_uris(vec!["spotify:track:y".into()], 0, PlayContext::new("tracks"));
    let ApiCall::TrackInfo { request, uri } = info_calls(&mut core)[0].clone() else { unreachable!() };
    core.remember(&song(&uri));
    core.handle_api(ApiReply::TrackInfo { request, uri: uri.clone(), result: Ok(named(&uri, "Loaded")) });
    assert_eq!(core.cached(&uri).map(|track| track.title.as_str()), Some("Loaded"));
}

/// Устаревший ответ не перебивает строку, которую уже принесли с другой стороны.
#[test]
fn a_stale_metadata_reply_is_dropped_once_the_row_is_known() {
    let mut core = headless();
    playing(&mut core, "spotify:track:old");
    core.play_uris(vec!["spotify:track:x".into()], 0, PlayContext::new("tracks"));
    let ApiCall::TrackInfo { request, uri } = info_calls(&mut core)[0].clone() else { unreachable!() };
    core.remember(&named(&uri, "Fresh"));
    core.handle_api(ApiReply::TrackInfo {
        request,
        uri: uri.clone(),
        result: Ok(named(&uri, "Stale")),
    });
    assert_eq!(core.cached(&uri).map(|track| track.title.as_str()), Some("Fresh"));
}

/// Список знает только URI: запомненная заранее строка всё равно показывается сразу.
#[test]
fn a_remembered_track_shows_up_for_a_uri_list_play() {
    let mut core = headless();
    core.local.connected = true;
    playing(&mut core, "spotify:track:old");
    core.remember(&named("spotify:track:new", "New"));
    core.play_uris(vec!["spotify:track:new".into()], 0, PlayContext::new("tracks"));
    let now = core.now_playing().expect("строка нашлась в кэше");
    assert_eq!(now.track.title, "New");
    assert!(now.loading);
}

/// Недоступный трек librespot пропускает сам: сообщение называет трек, иначе смена песни выглядит
/// самовольной.
#[test]
fn an_unavailable_track_is_named_in_the_notice() {
    let mut core = headless();
    core.remember(&named("spotify:track:gone", "Gone"));
    playing(&mut core, "spotify:track:gone");
    let mut failed = core.local.clone();
    failed.error = Some(format!("{}: spotify:track:gone", crate::engine::player::UNAVAILABLE));
    core.handle_local(failed);
    assert_eq!(notices(&core), [mf_i18n::tf("“{0}” isn't available — moving on", &[&"Gone".to_owned()])]);
}

/// Отказ в ключе — испорченная сессия: переподключиться сразу, но не чаще раза в 30 с.
#[test]
fn a_refused_audio_key_reconnects_the_session() {
    let mut core = headless();
    playing(&mut core, "spotify:track:honey");
    core.take_effects();
    let mut refused = core.local.clone();
    refused.error = Some(crate::engine::player::AUDIO_KEY_REFUSED.into());
    core.handle_local(refused.clone());
    assert!(core.take_effects().iter().any(|effect| matches!(effect, Effect::Reconnect)));
    let mut again = refused;
    again.error = None;
    core.handle_local(again.clone());
    again.error = Some(crate::engine::player::AUDIO_KEY_REFUSED.into());
    core.handle_local(again);
    assert!(!core.take_effects().iter().any(|effect| matches!(effect, Effect::Reconnect)), "not twice in 30 s");
}

/// Три «недоступно» за 20 с — тоже переподключение (по тексту, который движок пишет на самом деле).
#[test]
fn repeated_unavailable_tracks_reconnect_the_session() {
    let mut core = headless();
    playing(&mut core, "spotify:track:honey");
    core.take_effects();
    for (i, uri) in ["spotify:track:a", "spotify:track:b", "spotify:track:c"].iter().enumerate() {
        let mut failed = core.local.clone();
        failed.error = Some(format!("{}: {uri}", crate::engine::player::UNAVAILABLE));
        core.handle_local(failed);
        let reconnect = core.take_effects().iter().any(|effect| matches!(effect, Effect::Reconnect));
        assert_eq!(reconnect, i == 2, "after failure {i}");
    }
}

/// Удалённое намерение снимается только опросом, начатым после команды; одно несовпадение терпим.
#[test]
fn remote_track_intent_waits_for_a_fresh_confirming_poll() {
    let mut core = headless();
    core.local_ready = false;
    core.selected_device = Some("phone".into());
    core.remote_poll_seq = 7;
    core.expect_track("spotify:track:wanted".into(), 0);
    core.reconcile_remote_track_intent(7, Some("spotify:track:old"));
    assert!(core.intent_track.is_some(), "a poll in flight is older than the command");
    core.reconcile_remote_track_intent(8, Some("spotify:track:old"));
    assert!(core.intent_track.is_some(), "one stale reply is asked again");
    assert!(core.remote_recheck_at.is_some());
    core.reconcile_remote_track_intent(9, Some("spotify:track:actual"));
    assert!(core.intent_track.is_none(), "the second fresh reply decides");
}

#[test]
fn previous_discards_an_unconfirmed_next_marker() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    core.next();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:b"));
    core.previous();
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:a"));
    assert_eq!(core.direction, -1);
}

#[test]
fn a_song_starting_consumes_its_queue_row() {
    let mut core = headless();
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    playing(&mut core, "spotify:track:b");
    core.on_now_playing_changed();
    let (current, next) = queue_uris(&core);
    assert_eq!(current.as_deref(), Some("spotify:track:b"));
    assert_eq!(next, vec!["spotify:track:c"]);
}

#[test]
fn a_stale_queue_answer_does_not_undo_a_skip() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.next();
    let stale = fetched("spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    answer_latest(&mut core, Ok(stale.clone()));
    let (current, next) = queue_uris(&core);
    assert_eq!(current.as_deref(), Some("spotify:track:b"), "the skip holds");
    assert_eq!(next, vec!["spotify:track:c"]);
    assert!(core.queue_recheck_at.is_some(), "a stale reply is asked again");
    for _ in 0..QUEUE_STALE_RETRIES {
        answer_latest(&mut core, Ok(stale.clone()));
    }
    assert_eq!(queue_uris(&core).0.as_deref(), Some("spotify:track:a"), "after the limit Spotify’s reply is accepted");
}

#[test]
fn a_played_pending_add_is_not_put_back() {
    let mut core = headless();
    core.pending_queue_adds = vec![PendingQueueAdd {
        item: song("spotify:track:b"),
        at: Instant::now(),
        manual_index: 0,
        write: None,
    }];
    playing(&mut core, "spotify:track:b");
    loaded_queue(&mut core, "spotify:track:b", &["spotify:track:c"]);
    core.reconcile_pending_queue();
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:c"]);
    assert!(core.pending_queue_adds.is_empty());
}

#[test]
fn a_chosen_queue_row_plays_at_once_and_takes_the_rows_above() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.manual_queue = vec!["spotify:track:b".into(), "spotify:track:c".into()];
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c", "spotify:track:d"]);
    core.play_queue_item(1, "spotify:track:c".into());
    let (current, next) = queue_uris(&core);
    assert_eq!(current.as_deref(), Some("spotify:track:c"));
    assert_eq!(next, vec!["spotify:track:d"], "rows after the chosen one stay");
    assert_eq!(core.current_track_uri().as_deref(), Some("spotify:track:c"));
    assert!(core.manual_queue.is_empty());
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Next, PlayerCommand::Next], "N×Next");
}

#[test]
fn a_clicked_queue_row_is_found_by_its_song_when_rows_shifted() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.play_queue_item(0, "spotify:track:c".into());
    let (current, next) = queue_uris(&core);
    assert_eq!(current.as_deref(), Some("spotify:track:c"));
    assert!(next.is_empty());
}

#[test]
fn a_remote_queue_row_is_reached_with_one_next_per_row() {
    let mut core = headless();
    core.local_ready = false;
    core.remote = Some(remote_snapshot("phone", "spotify:track:a", true));
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:c"]);
    core.play_queue_item(1, "spotify:track:c".into());
    let nexts = api_calls(&mut core)
        .into_iter()
        .filter(|call| matches!(call, ApiCall::Remote { action: RemoteAction::Next, device: Some(device) } if device == "phone"))
        .count();
    assert_eq!(nexts, 2);
}

// ---- Очистка и ручная секция -----------------------------------------------------------------------

#[test]
fn clear_queue_takes_the_hand_queued_rows_and_keeps_the_context() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.manual_queue = vec!["spotify:track:b".into(), "spotify:track:c".into()];
    loaded_queue(
        &mut core,
        "spotify:track:a",
        &["spotify:track:b", "spotify:track:c", "spotify:track:c", "spotify:track:d"],
    );
    assert!(core.can_clear_queue());
    core.clear_queue();
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:c", "spotify:track:d"], "our own c from the context stays");
    assert!(core.manual_queue.is_empty());
    assert!(core.queue_recheck_at.is_some());
    assert!(notices(&core).contains(&"Queue cleared".to_owned()));
    assert_eq!(engine_commands(&mut core), [PlayerCommand::ClearQueue]);
}

#[test]
fn clearing_a_just_queued_song_leaves_the_contexts_copy_of_it() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1", "spotify:track:b", "spotify:track:ctx2"]);
    core.add_to_queue("spotify:track:b".into(), "b".into());
    assert_eq!(
        queue_uris(&core).1,
        vec!["spotify:track:b", "spotify:track:ctx1", "spotify:track:b", "spotify:track:ctx2"]
    );
    assert!(core.can_clear_queue());
    core.clear_queue();
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:ctx1", "spotify:track:b", "spotify:track:ctx2"]);
}

#[test]
fn clearing_is_only_offered_for_this_computers_queue() {
    let mut core = headless();
    core.local_ready = false;
    core.remote = Some(remote_snapshot("phone", "spotify:track:a", true));
    core.manual_queue = vec!["spotify:track:b".into()];
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    assert!(!core.can_clear_queue());
    core.clear_queue();
    assert_eq!(core.manual_queue.len(), 1, "a remote queue can’t be cleared");
}

#[test]
fn play_next_queues_after_the_songs_already_queued() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1", "spotify:track:ctx2"]);
    core.add_to_queue("spotify:track:b".into(), "b".into());
    core.add_to_queue("spotify:track:c".into(), "c".into());
    assert_eq!(
        queue_uris(&core).1,
        vec!["spotify:track:b", "spotify:track:c", "spotify:track:ctx1", "spotify:track:ctx2"]
    );
    assert_eq!(notices(&core).last().map(String::as_str), Some("“c” added to queue"));
    assert_eq!(
        engine_commands(&mut core),
        [PlayerCommand::AddToQueue("spotify:track:b".into()), PlayerCommand::AddToQueue("spotify:track:c".into())]
    );
}

#[test]
fn starting_a_queued_album_preserves_its_manual_copies_and_clear_button() {
    for advance in ["clear", "next", "natural"] {
        let mut core = headless();
        core.local.connected = true;
        playing(&mut core, "spotify:track:old");
        loaded_queue(&mut core, "spotify:track:old", &["spotify:track:old-next"]);
        core.on_now_playing_changed();
        core.queue_album_tracks(
            ["intro", "second", "third"].iter().map(|id| named(&format!("spotify:track:{id}"), id)).collect(),
            "Album".into(),
        );
        core.add_to_queue("spotify:track:extra".into(), "Extra".into());
        let manual = core.manual_queue.clone();
        assert_eq!(manual.len(), 4);
        core.play_context(PlayContext::with_uri("album", "spotify:album:album"), None, None);
        let mut started = core.local.clone();
        started.track = local_track("spotify:track:intro");
        started.track_sequence += 1;
        core.handle_local(started);
        assert_eq!(core.manual_queue, manual, "starting an album is not playing from the queue");

        let next = [
            "spotify:track:intro",
            "spotify:track:second",
            "spotify:track:third",
            "spotify:track:extra",
            "spotify:track:second",
            "spotify:track:third",
        ];
        answer_latest(&mut core, Ok(fetched("spotify:track:intro", &next)));
        assert_eq!(queue_uris(&core).1, next);
        assert_eq!(core.queued_rows_len(), 4);
        assert!(core.can_clear_queue());

        // перезапуск того же альбома тоже не съедает отдельную копию
        core.play_context(PlayContext::with_uri("album", "spotify:album:album"), None, None);
        let mut restarted = core.local.clone();
        restarted.track_sequence += 1;
        core.handle_local(restarted);
        assert_eq!(core.manual_queue, manual);
        assert_eq!(queue_uris(&core).1, next);

        if advance != "clear" {
            if advance == "next" {
                core.next();
                assert_eq!(core.queued_rows_len(), 3);
            }
            let mut queued_intro = core.local.clone();
            queued_intro.track_sequence += 1;
            core.handle_local(queued_intro);
            assert_eq!(core.manual_queue, manual[1..]);
            assert_eq!(queue_uris(&core).1, next[1..]);
            assert_eq!(core.queued_rows_len(), 3);
            assert!(core.can_clear_queue());
        }
        core.clear_queue();
        assert_eq!(queue_uris(&core).1, next[4..]);
        assert!(!core.can_clear_queue());
    }
}

#[test]
fn replaying_a_song_only_consumes_a_queue_copy_when_the_queue_advances() {
    for repeat in [RepeatMode::Off, RepeatMode::Track] {
        let mut core = headless();
        core.local = LocalState {
            connected: true,
            playback: Playback::Playing,
            repeat,
            track: local_track("spotify:track:a"),
            ..Default::default()
        };
        core.on_now_playing_changed();
        core.manual_queue = vec!["spotify:track:b".into(), "spotify:track:b".into()];
        loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:b", "spotify:track:context"]);
        core.next();
        let mut started = core.local.clone();
        started.track = local_track("spotify:track:b");
        started.track_sequence += 1;
        core.handle_local(started);
        assert_eq!(core.manual_queue, ["spotify:track:b"]);
        assert_eq!(core.queued_rows_len(), 1);

        let mut again = core.local.clone();
        again.track_sequence += 1;
        core.handle_local(again);
        if repeat == RepeatMode::Track {
            assert_eq!(core.manual_queue, ["spotify:track:b"]);
            assert_eq!(core.queued_rows_len(), 1, "repeating a track does not move the queue");
            core.next();
            let mut skipped = core.local.clone();
            skipped.track_sequence += 1;
            core.handle_local(skipped);
        }
        assert!(core.manual_queue.is_empty());
        assert_eq!(queue_uris(&core).1, ["spotify:track:context"]);
    }
}

// ---- Альбомы и удалённая запись ----------------------------------------------------------------------

#[test]
fn queue_album_uses_every_song_in_order_and_preserves_duplicates() {
    for local in [false, true] {
        let mut core = headless();
        core.local_ready = local;
        core.local.connected = local;
        playing(&mut core, "spotify:track:current");
        loaded_queue(&mut core, "spotify:track:current", &["spotify:track:manual", "spotify:track:context"]);
        core.manual_queue.push("spotify:track:manual".into());
        let mut tracks: Vec<TrackItem> = (0..120).map(|n| song(&format!("spotify:track:song{n}"))).collect();
        tracks.push(song("spotify:track:song0"));
        let expected: Vec<String> = tracks.iter().map(|track| track.uri.clone()).collect();
        tracks.push(TrackItem { available: false, ..song("spotify:track:unavailable") });
        tracks.push(TrackItem::default());
        core.queue_album_tracks(tracks, "Album".into());
        let queued = queue_uris(&core).1;
        assert_eq!(queued[0], "spotify:track:manual");
        assert_eq!(queued[1..queued.len() - 1], expected);
        assert_eq!(queued.last().unwrap(), "spotify:track:context");
        if local {
            let added: Vec<String> = engine_commands(&mut core)
                .into_iter()
                .filter_map(|command| match command {
                    PlayerCommand::AddToQueue(uri) => Some(uri),
                    _ => None,
                })
                .collect();
            assert_eq!(added, expected);
            assert!(queue_calls(&mut core).is_empty());
        } else {
            let calls = queue_calls(&mut core);
            assert_eq!(calls.len(), 1);
            assert!(matches!(&calls[0], ApiCall::AddMany { uris, .. } if *uris == expected));
        }
        assert!(core.pending_album_queues.is_empty());
        core.queue_one("spotify:track:after".into(), "After".into(), false);
        let queued = queue_uris(&core).1;
        assert_eq!(queued[queued.len() - 2], "spotify:track:after", "a late song goes after an album longer than 100");
    }
}

#[test]
fn an_album_is_fetched_whole_before_it_is_queued() {
    let mut core = headless();
    loaded_queue(&mut core, "spotify:track:current", &["spotify:track:context"]);
    core.add_to_queue("spotify:album:album".into(), "Album".into());
    let calls = queue_calls(&mut core);
    let [ApiCall::AlbumTracks { request, id }] = calls.as_slice() else {
        panic!("expected an album tracks request: {calls:?}");
    };
    assert_eq!(id, "album");
    assert_eq!(queue_uris(&core).1, ["spotify:track:context"], "nothing before the reply");
    // повторный клик в пределах 1,5 с — тот же
    core.add_to_queue("spotify:album:album".into(), "Album".into());
    assert!(queue_calls(&mut core).is_empty());
    core.handle_api(ApiReply::AlbumTracks {
        request: *request,
        result: Ok(vec![song("spotify:track:one"), song("spotify:track:two"), song("spotify:track:one")]),
    });
    assert_eq!(
        queue_uris(&core).1,
        ["spotify:track:one", "spotify:track:two", "spotify:track:one", "spotify:track:context"]
    );
    assert!(core.pending_album_queues.is_empty());
}

#[tokio::test]
async fn album_pages_are_read_in_order_or_not_at_all() {
    use sf_api::models::{Page, Track};
    let page = |ids: &[&str], offset: u32, next: bool| Page {
        items: ids.iter().map(|id| Track { uri: format!("spotify:track:{id}"), ..Default::default() }).collect(),
        offset,
        limit: 2,
        total: 3,
        next: next.then(|| "next".to_owned()),
    };
    let pages = [page(&["one", "two"], 0, true), page(&["one"], 2, false)];
    let tracks = super::backend::album_tracks(|offset| {
        let found = pages.iter().find(|page| page.offset == offset).cloned();
        async move { Ok(found.expect("a page at the offset")) }
    })
    .await
    .unwrap();
    let uris: Vec<_> = tracks.iter().map(|track| track.uri.as_str()).collect();
    assert_eq!(uris, ["spotify:track:one", "spotify:track:two", "spotify:track:one"]);

    // страница не с того места — альбом не добавляется частично
    let wrong = super::backend::album_tracks(|_| async { Ok(page(&["x"], 7, false)) }).await;
    assert!(wrong.is_err());
    let failed = super::backend::album_tracks(|offset| async move {
        if offset == 0 { Ok(page(&["one", "two"], 0, true)) } else { Err(ApiError::RateLimited) }
    })
    .await;
    assert!(failed.is_err());
}

#[test]
fn remote_album_queue_keeps_its_rows_while_rate_limited_writes_are_pending() {
    let mut core = headless();
    core.local_ready = false;
    loaded_queue(&mut core, "spotify:track:current", &["spotify:track:context"]);
    let old = fetched("spotify:track:current", &["spotify:track:context"]);
    core.queue_album_tracks(
        vec![song("spotify:track:a"), song("spotify:track:b"), song("spotify:track:a")],
        "Album".into(),
    );
    let request = core.serial;
    for addition in &mut core.pending_queue_adds {
        addition.at = Instant::now() - Duration::from_secs(60);
    }
    core.expire_pending_queue_adds();
    assert_eq!(core.pending_queue_adds.len(), 3, "pending adds do not expire while a write is in flight");
    let expected = ["spotify:track:a", "spotify:track:b", "spotify:track:a", "spotify:track:context"];
    for _ in 0..u16::from(u8::MAX) + 2 {
        answer_latest(&mut core, Ok(old.clone()));
        assert_eq!(queue_uris(&core).1, expected);
    }
    answer_latest(&mut core, Err(ApiError::RateLimited));
    assert_eq!(queue_uris(&core).1, expected);
    core.handle_api(ApiReply::QueueBatchAdded { request, added: 3, result: Ok(()) });
    assert!(core.pending_queue_batches.is_empty());
    assert!(core.pending_queue_adds.iter().all(|addition| addition.at.elapsed() < Duration::from_secs(1)));
}

#[test]
fn rejected_queue_copies_leave_accepted_later_and_context_copies_intact() {
    let mut core = headless();
    core.local_ready = false;
    loaded_queue(&mut core, "spotify:track:current", &["spotify:track:a"]);
    core.queue_album_tracks(
        vec![song("spotify:track:a"), song("spotify:track:a"), song("spotify:track:b")],
        "Album".into(),
    );
    let album = core.serial;
    // отдельная просьба — мимо защиты от двойного клика
    core.queue_one("spotify:track:a".into(), "Later A".into(), false);
    let later = core.serial;
    core.handle_api(ApiReply::QueueBatchAdded {
        request: album,
        added: 1,
        result: Err(ApiError::Status { status: 403, message: "Rejected".into() }),
    });
    assert_eq!(core.manual_queue, ["spotify:track:a", "spotify:track:a"]);
    assert_eq!(queue_uris(&core).1, ["spotify:track:a", "spotify:track:a", "spotify:track:a"]);
    assert_eq!(core.pending_queue_adds.len(), 2);
    assert_eq!(core.pending_queue_adds[1].manual_index, 1);
    assert_eq!(core.pending_queue_adds[1].write, Some((later, 0)));
    core.handle_api(ApiReply::QueueBatchAdded { request: later, added: 0, result: Err(ApiError::QuotaExhausted) });
    assert_eq!(core.manual_queue, ["spotify:track:a"]);
    assert_eq!(queue_uris(&core).1, ["spotify:track:a", "spotify:track:a"]);
}

#[test]
fn pending_single_keeps_its_name_and_duration_during_and_after_a_cooldown() {
    let mut core = headless();
    core.local_ready = false;
    loaded_queue(&mut core, "spotify:track:current", &["spotify:track:context"]);
    let old = fetched("spotify:track:current", &["spotify:track:context"]);
    let track = TrackItem { duration_ms: 215_000, ..named("spotify:track:single", "Single") };
    core.remember(&track);
    core.add_to_queue(track.uri.clone(), track.title.clone());
    let request = core.serial;
    core.pending_queue_adds[0].at = Instant::now() - Duration::from_secs(90);
    answer_latest(&mut core, Ok(old.clone()));
    let row = &core.queue.as_ref().unwrap().next[0];
    assert_eq!(row.track.title, track.title);
    assert_eq!(row.track.duration_ms, track.duration_ms);
    core.handle_api(ApiReply::QueueBatchAdded { request, added: 1, result: Ok(()) });
    answer_latest(&mut core, Ok(old));
    let row = &core.queue.as_ref().unwrap().next[0];
    assert_eq!(row.track.uri, track.uri);
    assert_eq!(row.track.title, track.title);
    assert_eq!(row.track.duration_ms, track.duration_ms);
}

#[test]
fn pending_album_queue_cannot_survive_clear_sign_out_or_device_change() {
    for cancel in ["clear", "sign-out", "device", "failure"] {
        let mut core = headless();
        loaded_queue(&mut core, "spotify:track:current", &["spotify:track:context"]);
        core.add_to_queue("spotify:album:album".into(), "Album".into());
        let request = core.serial;
        let _ = queue_calls(&mut core);
        match cancel {
            "clear" => core.clear_queue(),
            "sign-out" => core.reset_account(),
            "device" => core.selected_device = Some("phone".into()),
            "failure" => core.handle_api(ApiReply::AlbumTracks { request, result: Err(ApiError::RateLimited) }),
            _ => unreachable!(),
        }
        core.handle_api(ApiReply::AlbumTracks { request, result: Ok(vec![song("spotify:track:late")]) });
        assert!(core.manual_queue.is_empty(), "{cancel}");
        assert!(core.pending_album_queues.is_empty(), "{cancel}");
        assert!(queue_calls(&mut core).is_empty(), "{cancel}");
        assert!(
            !engine_commands(&mut core).iter().any(|command| matches!(command, PlayerCommand::AddToQueue(_))),
            "{cancel}"
        );
    }
}

/// Отдельные просьбы ставят дубли; двойной клик — нет.
#[test]
fn asking_play_next_twice_queues_two_rows() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1"]);
    core.add_to_queue("spotify:track:b".into(), "b".into());
    core.add_to_queue("spotify:track:b".into(), "b".into());
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:b", "spotify:track:ctx1"]);
    for addition in &mut core.pending_queue_adds {
        addition.at = Instant::now() - QUEUE_ADD_DEBOUNCE;
    }
    core.add_to_queue("spotify:track:b".into(), "b".into());
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:b", "spotify:track:b", "spotify:track:ctx1"]);
}

#[test]
fn a_song_picked_twice_is_queued_twice() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1"]);
    let picked = vec![named("spotify:track:b", "b"), named("spotify:track:c", "c"), named("spotify:track:b", "b")];
    core.queue_many(picked.clone());
    let told = notices(&core).len();
    core.queue_many(picked.clone());
    assert_eq!(notices(&core).len(), told, "the second click added nothing");
    assert_eq!(
        queue_uris(&core).1,
        vec!["spotify:track:b", "spotify:track:c", "spotify:track:b", "spotify:track:ctx1"]
    );
    assert_eq!(core.manual_queue, vec!["spotify:track:b", "spotify:track:c", "spotify:track:b"]);
    assert_eq!(notices(&core).last().map(String::as_str), Some("Added to queue: 3 tracks"));
    for addition in &mut core.pending_queue_adds {
        addition.at = Instant::now() - QUEUE_ADD_DEBOUNCE;
    }
    core.queue_many(picked);
    assert_eq!(
        queue_uris(&core).1,
        [
            "spotify:track:b",
            "spotify:track:c",
            "spotify:track:b",
            "spotify:track:b",
            "spotify:track:c",
            "spotify:track:b",
            "spotify:track:ctx1"
        ]
    );
}

#[test]
fn a_queue_batch_reports_only_rows_it_added() {
    let mut core = headless();
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1"]);
    core.add_to_queue("spotify:track:b".into(), "b".into());
    core.queue_many(vec![named("spotify:track:b", "b"), named("spotify:track:c", "c"), named("spotify:track:c", "c")]);
    assert_eq!(
        queue_uris(&core).1,
        ["spotify:track:b", "spotify:track:c", "spotify:track:c", "spotify:track:ctx1"]
    );
    assert_eq!(notices(&core).last().unwrap(), "Added to queue: 2 tracks");
    let told = notices(&core).len();
    core.queue_many(Vec::new());
    assert_eq!(notices(&core).len(), told, "an empty batch adds nothing");
}

#[test]
fn an_overtaken_queue_answer_is_dropped_unread() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    core.queue_seq = 2;
    answer_queue(&mut core, 1, Ok(fetched("spotify:track:a", &[])));
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:b"]);
    answer_queue(&mut core, 2, Ok(fetched("spotify:track:a", &["spotify:track:b", "spotify:track:c"])));
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:b", "spotify:track:c"]);
}

#[test]
fn a_missing_queued_row_returns_to_its_place() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.manual_queue = vec!["spotify:track:b".into(), "spotify:track:c".into()];
    core.pending_queue_adds =
        vec![PendingQueueAdd { item: song("spotify:track:c"), at: Instant::now(), manual_index: 1, write: None }];
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:ctx1"]);
    core.reconcile_pending_queue();
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:b", "spotify:track:c", "spotify:track:ctx1"]);
}

#[test]
fn the_queued_section_covers_only_the_users_own_rows() {
    let mut core = headless();
    core.manual_queue = vec!["spotify:track:b".into(), "spotify:track:c".into()];
    loaded_queue(
        &mut core,
        "spotify:track:a",
        &["spotify:track:b", "spotify:track:c", "spotify:track:ctx1", "spotify:track:c"],
    );
    assert_eq!(core.queued_rows_len(), 2, "the copy of c in the context is not manual");
    core.manual_queue.clear();
    assert_eq!(core.queued_rows_len(), 0);
}

// ---- Shuffle и перестановка ----------------------------------------------------------------------------

#[test]
fn changing_shuffle_schedules_a_queue_recheck() {
    let mut core = headless();
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b"]);
    assert!(core.queue_recheck_at.is_none());
    core.set_shuffle(true);
    assert!(core.queue_recheck_at.is_some());
}

#[test]
fn shuffling_local_playback_updates_queue_and_keeps_user_songs() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:manual1".into()];
    let before = ["spotify:track:manual1", "spotify:track:ctx1", "spotify:track:ctx2", "spotify:track:ctx3"];
    loaded_queue(&mut core, "spotify:track:playing", &before);
    assert_eq!(core.queued_rows_len(), 1);
    core.set_shuffle(true);
    assert!(core.queue_recheck_at.is_some());
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Shuffle(true)]);
    core.refresh_queue(true);
    let seq = core.queue_seq;
    answer_queue(&mut core, seq - 1, Ok(fetched("spotify:track:playing", &before)));
    assert_eq!(queue_uris(&core).1, before, "a reply from before the switch is dropped unread");
    let shuffled = ["spotify:track:manual1", "spotify:track:ctx3", "spotify:track:ctx1", "spotify:track:ctx2"];
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &shuffled)));
    assert_eq!(queue_uris(&core).0.as_deref(), Some("spotify:track:playing"));
    assert_eq!(queue_uris(&core).1, shuffled);
    assert_eq!(core.queued_rows_len(), 1);
}

#[test]
fn shuffling_remote_playback_updates_queue_and_keeps_user_songs() {
    let mut core = headless();
    core.local_ready = false;
    core.selected_device = Some("speaker".into());
    core.remote = Some(remote_snapshot("speaker", "spotify:track:playing", true));
    core.manual_queue = vec!["spotify:track:manual1".into()];
    let before = ["spotify:track:manual1", "spotify:track:ctx1", "spotify:track:ctx2"];
    loaded_queue(&mut core, "spotify:track:playing", &before);
    assert_eq!(core.queued_rows_len(), 1);
    core.set_shuffle(true);
    assert!(core.queue_recheck_at.is_some());
    assert!(api_calls(&mut core).iter().any(|call| matches!(
        call,
        ApiCall::Remote { action: RemoteAction::Shuffle(true), device: Some(device) } if device == "speaker"
    )));
    core.handle_api(ApiReply::Remote { action: RemoteAction::Shuffle(true), result: Ok(()) });
    assert!(core.queue_recheck_at.is_some());
    core.refresh_queue(true);
    let seq = core.queue_seq;
    answer_queue(&mut core, seq - 1, Ok(fetched("spotify:track:playing", &before)));
    assert_eq!(queue_uris(&core).1, before);
    let shuffled = ["spotify:track:manual1", "spotify:track:ctx2", "spotify:track:ctx1"];
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &shuffled)));
    assert_eq!(queue_uris(&core).1, shuffled);
    assert_eq!(core.queued_rows_len(), 1);
}

fn with_local_manual_queue() -> Core {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:a".into(), "spotify:track:b".into(), "spotify:track:c".into()];
    loaded_queue(
        &mut core,
        "spotify:track:playing",
        &["spotify:track:a", "spotify:track:b", "spotify:track:c", "spotify:track:ctx"],
    );
    core
}

/// spotifast MoveInQueue{to} — место вставки до перемещения; здесь `to` — итоговый индекс.
fn slot_to_index(from: usize, to: usize) -> usize {
    if to > from { to - 1 } else { to }
}

#[test]
fn stale_queue_response_after_local_reorder_does_not_undo_it() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:manual1".into(), "spotify:track:manual2".into()];
    let pre_drag = ["spotify:track:manual1", "spotify:track:manual2", "spotify:track:ctx1"];
    loaded_queue(&mut core, "spotify:track:playing", &pre_drag);
    core.refresh_queue(true);
    let outstanding = core.queue_seq;
    core.move_manual(0, slot_to_index(0, 2));
    let reordered = vec!["spotify:track:manual2", "spotify:track:manual1", "spotify:track:ctx1"];
    assert_eq!(queue_uris(&core).1, reordered, "the reorder shows at once");
    let commands = engine_commands(&mut core);
    assert_eq!(
        commands,
        [
            PlayerCommand::ClearQueue,
            PlayerCommand::AddToQueue("spotify:track:manual2".into()),
            PlayerCommand::AddToQueue("spotify:track:manual1".into()),
        ]
    );
    answer_queue(&mut core, outstanding, Ok(fetched("spotify:track:playing", &pre_drag)));
    assert_eq!(queue_uris(&core).1, reordered, "a late reply does not undo the reorder");
    core.queue_recheck_at = None;
    core.refresh_queue(true);
    let seq = core.queue_seq;
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &pre_drag)));
    assert_eq!(queue_uris(&core).1, reordered, "nor does a lagging fresh reply");
    assert_eq!(core.queue_stale_retries, 1);
    assert!(core.queue_recheck_at.is_some());
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &reordered)));
    assert_eq!(core.queue_stale_retries, 0);
    assert!(core.queue_reorder_pending.is_none());
    assert_eq!(queue_uris(&core).1, reordered);
}

#[test]
fn moving_a_queued_row_keeps_pending_additions_on_their_own_song() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:m0".into(), "spotify:track:m1".into(), "spotify:track:m2".into()];
    loaded_queue(&mut core, "spotify:track:playing", &["spotify:track:m0", "spotify:track:m1", "spotify:track:m2"]);
    core.pending_queue_adds = (0..3)
        .map(|i| PendingQueueAdd {
            item: song(&format!("spotify:track:m{i}")),
            at: Instant::now(),
            manual_index: i,
            write: None,
        })
        .collect();
    core.move_manual(0, slot_to_index(0, 3));
    assert_eq!(core.manual_queue, ["spotify:track:m1", "spotify:track:m2", "spotify:track:m0"]);
    assert_eq!(core.pending_queue_adds.len(), 3);
    for addition in &core.pending_queue_adds {
        assert_eq!(core.manual_queue[addition.manual_index], addition.item.uri);
    }
}

#[test]
fn every_local_move_restarts_the_stale_queue_allowance() {
    let mut core = with_local_manual_queue();
    let mid_resync = fetched("spotify:track:playing", &["spotify:track:ctx"]);
    let answer_stale = |core: &mut Core| {
        core.refresh_queue(true);
        let seq = core.queue_seq;
        answer_queue(core, seq, Ok(mid_resync.clone()));
    };
    core.move_manual(0, slot_to_index(0, 2));
    for _ in 0..QUEUE_STALE_RETRIES - 1 {
        answer_stale(&mut core);
    }
    core.move_manual(2, slot_to_index(2, 0));
    let moved = vec!["spotify:track:c", "spotify:track:b", "spotify:track:a", "spotify:track:ctx"];
    for _ in 0..QUEUE_STALE_RETRIES - 1 {
        answer_stale(&mut core);
        assert_eq!(queue_uris(&core).1, moved, "a lagging reply does not undo the last reorder");
    }
    assert_eq!(core.queued_rows_len(), 3);
}

#[test]
fn moving_a_row_shows_the_order_that_will_play() {
    let mut core = with_local_manual_queue();
    loaded_queue(
        &mut core,
        "spotify:track:playing",
        &["spotify:track:b", "spotify:track:a", "spotify:track:c", "spotify:track:ctx"],
    );
    core.move_manual(1, slot_to_index(1, 3));
    assert_eq!(core.manual_queue, ["spotify:track:a", "spotify:track:c", "spotify:track:b"]);
    assert_eq!(
        queue_uris(&core).1,
        ["spotify:track:a", "spotify:track:c", "spotify:track:b", "spotify:track:ctx"],
        "rows are in the order given to the engine"
    );
}

#[test]
fn inserting_in_queue_keeps_pending_additions_on_their_own_song() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:m0".into(), "spotify:track:m1".into()];
    loaded_queue(&mut core, "spotify:track:playing", &["spotify:track:m0", "spotify:track:m1"]);
    core.pending_queue_adds = (0..2)
        .map(|i| PendingQueueAdd {
            item: song(&format!("spotify:track:m{i}")),
            at: Instant::now(),
            manual_index: i,
            write: None,
        })
        .collect();
    core.insert_manual(vec![song("spotify:track:new")], 1);
    assert_eq!(core.manual_queue, ["spotify:track:m0", "spotify:track:new", "spotify:track:m1"]);
    assert_eq!(queue_uris(&core).1, ["spotify:track:m0", "spotify:track:new", "spotify:track:m1"]);
    assert_eq!(core.pending_queue_adds.len(), 3);
    for addition in &core.pending_queue_adds {
        assert_eq!(core.manual_queue[addition.manual_index], addition.item.uri);
    }
}

#[test]
fn a_drop_on_a_remote_queue_just_adds_to_the_end() {
    let mut core = headless();
    core.local_ready = false;
    core.remote = Some(remote_snapshot("phone", "spotify:track:a", true));
    core.manual_queue = vec!["spotify:track:m0".into()];
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:m0", "spotify:track:ctx"]);
    assert!(!core.queue_locally_reorderable());
    core.insert_manual(vec![song("spotify:track:new")], 0);
    assert_eq!(core.manual_queue, ["spotify:track:m0", "spotify:track:new"]);
    core.move_manual(1, 0);
    assert_eq!(core.manual_queue, ["spotify:track:m0", "spotify:track:new"], "not reordered remotely");
}

#[test]
fn stale_queue_after_shuffle_is_retried_and_not_accepted() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    loaded_queue(&mut core, "spotify:track:playing", &["spotify:track:ctx1", "spotify:track:ctx2"]);
    core.set_shuffle(true);
    core.refresh_queue(true);
    let seq = core.queue_seq;
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:old_song", &["spotify:track:ctx2", "spotify:track:ctx1"])));
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:ctx1", "spotify:track:ctx2"]);
    assert_eq!(core.queue_stale_retries, 1);
    assert!(core.queue_recheck_at.is_some());
}

#[test]
fn lagging_queue_after_shuffle_is_retried_until_new_order_arrives() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    core.manual_queue = vec!["spotify:track:manual1".into()];
    let before = ["spotify:track:manual1", "spotify:track:ctx1", "spotify:track:ctx2", "spotify:track:ctx3"];
    loaded_queue(&mut core, "spotify:track:playing", &before);
    core.set_shuffle(true);
    core.refresh_queue(true);
    let seq = core.queue_seq;
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &before)));
    assert_eq!(core.queue_stale_retries, 1);
    assert!(core.queue_recheck_at.is_some());
    assert_eq!(queue_uris(&core).1, before);
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &before)));
    assert_eq!(core.queue_stale_retries, 2);
    let after = ["spotify:track:manual1", "spotify:track:ctx3", "spotify:track:ctx1", "spotify:track:ctx2"];
    answer_queue(&mut core, seq, Ok(fetched("spotify:track:playing", &after)));
    assert_eq!(core.queue_stale_retries, 0);
    assert!(core.queue_shuffle_pending.is_none());
    assert_eq!(queue_uris(&core).1, after);
}

#[test]
fn unchanged_shuffle_result_has_bounded_fallback() {
    let mut core = headless();
    playing(&mut core, "spotify:track:playing");
    loaded_queue(&mut core, "spotify:track:playing", &["spotify:track:ctx1", "spotify:track:ctx2"]);
    core.set_shuffle(true);
    core.refresh_queue(true);
    let seq = core.queue_seq;
    let unchanged = fetched("spotify:track:playing", &["spotify:track:ctx1", "spotify:track:ctx2"]);
    for expected in 1..=QUEUE_STALE_RETRIES {
        answer_queue(&mut core, seq, Ok(unchanged.clone()));
        assert_eq!(core.queue_stale_retries, expected);
        assert!(core.queue_recheck_at.is_some());
    }
    answer_queue(&mut core, seq, Ok(unchanged));
    assert_eq!(core.queue_stale_retries, 0);
    assert!(core.queue_shuffle_pending.is_none());
    assert_eq!(queue_uris(&core).1, vec!["spotify:track:ctx1", "spotify:track:ctx2"]);
}

// ---- Сохранение, восстановление, старт ---------------------------------------------------------------

#[test]
fn the_queue_comes_back_after_a_restart() {
    let mut core = Core::new(0.5, true);
    core.local_ready = true;
    core.remember(&named("spotify:track:a", "A"));
    core.resume_track = Some("spotify:track:a".into());
    core.manual_queue = vec!["spotify:track:b".into()];
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:b", "spotify:track:ctx1"]);
    core.save_session();
    let saved = core
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Save(data) => Some(*data),
            _ => None,
        })
        .expect("the session is saved");
    let mut restarted = Core::new(0.5, true);
    restarted.restore_session(saved);
    assert_eq!(queue_uris(&restarted).1, vec!["spotify:track:b", "spotify:track:ctx1"]);
    assert_eq!(restarted.queued_rows_len(), 1, "our own manual song is in its own section");
    let now = restarted.now_playing().expect("the last session’s track is shown");
    assert!(now.resuming && !now.playing);
    assert_eq!(now.track.title, "A");
}

#[test]
fn the_remembered_song_is_shown_paused_at_its_position() {
    let mut core = headless();
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_position_ms = 19_566;
    assert!(core.now_playing().is_none(), "nothing to show until the song data arrives");
    core.remember(&TrackItem {
        title: "Karma Police".into(),
        artists: "Radiohead".into(),
        album: "OK Computer".into(),
        duration_ms: 264_000,
        ..song("spotify:track:abc")
    });
    let now = core.now_playing().expect("the last session’s track is shown");
    assert!(now.resuming);
    assert!(!now.playing);
    assert_eq!(now.track.title, "Karma Police");
    assert_eq!(now.track.artists, "Radiohead");
    assert_eq!(now.duration_ms, 264_000);
    assert_eq!(now.position_ms, 19_566, "not from zero");
}

#[test]
fn showing_the_remembered_song_keeps_its_position() {
    let mut core = headless();
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_position_ms = 19_566;
    core.remember(&TrackItem { duration_ms: 264_000, ..song("spotify:track:abc") });
    core.on_now_playing_changed();
    assert_eq!(core.resume_position_ms, 19_566);
    let data = core.session_data();
    assert_eq!(data.position_ms, 19_566);
    assert_eq!(core.resume_position_ms, 19_566);
}

#[test]
fn seeking_the_remembered_song_moves_the_resume_point() {
    let mut core = headless();
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_position_ms = 19_566;
    core.seek(90_000);
    assert_eq!(core.resume_position_ms, 90_000);
}

#[test]
fn pressing_play_on_a_cold_start_does_not_restart_the_song() {
    let mut core = headless();
    core.resume_context = Some("spotify:playlist:pl1".into());
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_position_ms = 19_566;
    core.toggle_play();
    let request = core.queued_play.as_ref().expect("the start waits for the engine");
    assert_eq!(request.context_uri.as_deref(), Some("spotify:playlist:pl1"));
    assert_eq!(request.offset_uri.as_deref(), Some("spotify:track:abc"));
    assert_eq!(request.position_ms, 19_566);
    // готовый движок получает его с той же позицией
    core.local.connected = true;
    core.handle_playback(LocalPlayback::Ready { device_id: "pc".into() });
    let commands = engine_commands(&mut core);
    assert!(matches!(
        commands.as_slice(),
        [PlayerCommand::Load(load)] if load.position_ms == 19_566 && load.offset_uri.as_deref() == Some("spotify:track:abc")
    ));
}

#[test]
fn playing_an_in_progress_episode_continues_from_its_resume_point() {
    let mut core = headless();
    let in_progress = "spotify:episode:mid";
    let play = |core: &mut Core, uri: &str, resume: i64| {
        core.queued_play = None;
        core.play_episode(uri.into(), resume);
        core.queued_play.as_ref().expect("waits for the engine").position_ms
    };
    assert_eq!(play(&mut core, in_progress, 600_000), 600_000);
    assert_eq!(play(&mut core, "spotify:episode:new", 0), 0);
    core.local.track = Some(LocalTrack {
        uri: in_progress.into(),
        duration_ms: 2_400_000,
        is_episode: true,
        ..Default::default()
    });
    core.local.playback = Playback::Playing;
    core.local.position_ms = 1_000_000;
    core.intent_track = None;
    core.queued_play = None;
    assert_eq!(play(&mut core, in_progress, 600_000), 0, "a playing episode is not sent back to the old point");
}

#[test]
fn the_saved_queue_follows_the_resumed_song_only() {
    let mut core = headless();
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_queue = vec!["spotify:track:q1".into(), "spotify:track:q2".into()];
    playing(&mut core, "spotify:track:other");
    core.on_now_playing_changed();
    assert!(core.resume_queue.is_empty(), "another start lets go of the saved queue");
    assert!(core.manual_queue.is_empty());
    assert!(core.session_dirty);
}

#[test]
fn the_saved_queue_comes_back_with_its_song() {
    let mut core = headless();
    core.resume_track = Some("spotify:track:abc".into());
    core.resume_queue = vec!["spotify:track:q1".into(), "spotify:track:q2".into()];
    playing(&mut core, "spotify:track:abc");
    core.on_now_playing_changed();
    assert_eq!(core.manual_queue, ["spotify:track:q1", "spotify:track:q2"]);
    assert_eq!(
        engine_commands(&mut core),
        [PlayerCommand::AddToQueue("spotify:track:q1".into()), PlayerCommand::AddToQueue("spotify:track:q2".into())]
    );
}

#[test]
fn the_transport_steps_a_restored_song_without_playing_it() {
    let mut core = headless();
    for uri in ["spotify:track:one", "spotify:track:two", "spotify:track:three"] {
        core.remember(&named(uri, uri));
    }
    core.resume_context = Some("spotify:playlist:pl1".into());
    core.resume_track = Some("spotify:track:one".into());
    core.resume_position_ms = 19_566;
    loaded_queue(&mut core, "spotify:track:one", &["spotify:track:two", "spotify:track:three"]);
    assert!(core.resume_only());
    core.next();
    assert_eq!(core.resume_track.as_deref(), Some("spotify:track:two"));
    assert_eq!(core.resume_position_ms, 0);
    assert!(core.queued_play.is_none() && core.local_list.is_none(), "a skip does not start a track");
    assert!(engine_commands(&mut core).is_empty());
    let now = core.now_playing().unwrap();
    assert!(now.resuming && !now.playing);
    assert_eq!(now.track.uri, "spotify:track:two");
    // после 3 с Previous перематывает в начало
    core.resume_position_ms = 19_566;
    core.previous();
    assert_eq!(core.resume_track.as_deref(), Some("spotify:track:two"));
    assert_eq!(core.resume_position_ms, 0);
    core.previous();
    assert_eq!(core.resume_track.as_deref(), Some("spotify:track:one"));
    assert_eq!(queue_uris(&core).1[0], "spotify:track:two");
    core.toggle_play();
    let request = core.queued_play.as_ref().expect("Play starts the chosen one");
    assert_eq!(request.context_uri.as_deref(), Some("spotify:playlist:pl1"));
    assert_eq!(request.offset_uri.as_deref(), Some("spotify:track:one"));
}

#[test]
fn a_chosen_row_in_a_list_never_loads_shuffled() {
    let request = PlayRequest::tracks(vec!["spotify:track:a".into(), "spotify:track:b".into()]).starting_at_index(1);
    let load = local_load(&request, true);
    assert_eq!(load.shuffle, None);
    assert_eq!(load.offset_index, Some(1));
    let request = PlayRequest::tracks(vec!["spotify:track:a".into(), "spotify:track:b".into()]);
    assert_eq!(local_load(&request, true).shuffle, Some(true));
    let request = PlayRequest::context("spotify:playlist:x").starting_at_uri("spotify:track:a");
    assert_eq!(local_load(&request, true).shuffle, Some(true));
}

#[test]
fn a_shuffled_list_play_switches_shuffle_on_after_loading_straight() {
    let mut core = headless();
    core.local.connected = true;
    let items: Vec<TrackItem> = (0..5).map(|n| song(&format!("spotify:track:{n}"))).collect();
    core.play_items(items, 2, PlayContext::new("search"));
    core.set_shuffle(true);
    let _ = engine_commands(&mut core);
    let items: Vec<TrackItem> = (0..5).map(|n| song(&format!("spotify:track:{n}"))).collect();
    core.play_items(items, 2, PlayContext::new("search"));
    let commands = engine_commands(&mut core);
    assert!(matches!(
        commands.as_slice(),
        [PlayerCommand::Load(load), PlayerCommand::Shuffle(true)] if load.shuffle.is_none() && load.offset_index == Some(2)
    ));
}

#[test]
fn one_song_is_loaded_as_a_context() {
    let one = local_load(&PlayRequest::tracks(vec!["spotify:track:a".into()]), false);
    assert_eq!(one.context_uri.as_deref(), Some("spotify:track:a"));
    assert!(one.uris.is_empty() && !one.autoplay);
    let two = local_load(
        &PlayRequest::tracks(vec!["spotify:track:a".into(), "spotify:track:b".into()]).starting_at_index(1),
        true,
    );
    assert_eq!(two.context_uri, None);
    assert_eq!(two.uris.len(), 2);
    assert_eq!(two.offset_index, Some(1));
    assert_eq!(two.shuffle, None);
    let episode = local_load(&PlayRequest::tracks(vec!["spotify:episode:e".into()]), false);
    assert_eq!(episode.context_uri, None);
    assert_eq!(episode.uris.len(), 1);
}

#[test]
fn a_list_that_ends_seeds_autoplay_with_its_last_song() {
    let track = |uri: &str| Some(LocalTrack { uri: uri.into(), duration_ms: 200_000, ..Default::default() });
    let playing = LocalState {
        playback: Playback::Playing,
        track: track("spotify:track:last"),
        position_ms: 198_500,
        connected: true,
        ..LocalState::default()
    };
    let stopped =
        LocalState { playback: Playback::Stopped, track: track("spotify:track:last"), connected: true, ..LocalState::default() };
    let list: Vec<String> = vec!["spotify:track:first".into(), "spotify:track:last".into()];
    assert_eq!(autoplay_seed(Some(&list), true, &playing, &stopped).as_deref(), Some("spotify:track:last"));
    assert_eq!(autoplay_seed(Some(&list), false, &playing, &stopped), None);
    assert_eq!(autoplay_seed(None, true, &playing, &stopped), None);
    let mid_song = LocalState { position_ms: 60_000, ..playing.clone() };
    assert_eq!(autoplay_seed(Some(&list), true, &mid_song, &stopped), None);
    let not_last = LocalState { track: track("spotify:track:first"), ..playing.clone() };
    assert_eq!(autoplay_seed(Some(&list), true, &not_last, &stopped), None);
    let dropped = LocalState { connected: false, ..stopped.clone() };
    assert_eq!(autoplay_seed(Some(&list), true, &playing, &dropped), None);
}

#[test]
fn an_ended_list_loads_autoplay_only_when_it_is_on() {
    for autoplay in [true, false] {
        let mut core = headless();
        core.autoplay = autoplay;
        core.local.connected = true;
        core.play_uris(vec!["spotify:track:a".into(), "spotify:track:b".into()], 0, PlayContext::new("tracks"));
        let _ = engine_commands(&mut core);
        core.local = LocalState {
            connected: true,
            playback: Playback::Playing,
            track: Some(LocalTrack { uri: "spotify:track:b".into(), duration_ms: 100_000, ..Default::default() }),
            position_ms: 99_000,
            ..Default::default()
        };
        let ended = LocalState { playback: Playback::Stopped, ..core.local.clone() };
        core.handle_local(ended);
        let loads: Vec<_> = engine_commands(&mut core)
            .into_iter()
            .filter(|command| matches!(command, PlayerCommand::Load(load) if load.autoplay))
            .collect();
        assert_eq!(loads.len(), usize::from(autoplay));
    }
}

#[test]
fn cap_uris_keeps_five_hundred_from_the_chosen_row() {
    let uris: Vec<String> = (0..800).map(|n| format!("spotify:track:{n}")).collect();
    let (capped, index) = cap_uris(&uris, 600);
    assert_eq!(capped.len(), 200);
    assert_eq!(capped[0], "spotify:track:600");
    assert_eq!(index, 0);
    let (capped, index) = cap_uris(&uris[..10], 3);
    assert_eq!((capped.len(), index), (10, 3));
}

// ---- Громкость, перемотка, play/pause ---------------------------------------------------------------

#[test]
fn a_stale_engine_snapshot_does_not_pull_the_volume_back() {
    let mut core = headless();
    core.set_volume(percent_to_volume(80));
    core.handle_local(LocalState { volume: percent_to_volume(20), ..LocalState::default() });
    assert_eq!(volume_to_percent(core.local.volume), 80);
    assert_eq!(volume_to_percent(core.volume), 80);
    core.handle_local(LocalState { volume: percent_to_volume(80), ..LocalState::default() });
    assert_eq!(volume_to_percent(core.local.volume), 80);
}

#[test]
fn a_volume_changed_outside_the_app_is_adopted() {
    let mut core = headless();
    core.handle_local(LocalState { volume: percent_to_volume(35), ..LocalState::default() });
    assert_eq!(volume_to_percent(core.local.volume), 35);
    assert_eq!(volume_to_percent(core.volume), 35);
}

#[test]
fn dragging_the_volume_previews_and_settles_once() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.set_volume(percent_to_volume(10));
    core.set_volume(percent_to_volume(20));
    assert_eq!(
        engine_commands(&mut core),
        [PlayerCommand::VolumePreview(percent_to_volume(10)), PlayerCommand::VolumePreview(percent_to_volume(20))]
    );
    std::thread::sleep(VOLUME_SETTLE + Duration::from_millis(20));
    core.tick();
    let settled: Vec<_> =
        engine_commands(&mut core).into_iter().filter(|command| matches!(command, PlayerCommand::Volume(_))).collect();
    assert_eq!(settled, [PlayerCommand::Volume(percent_to_volume(20))]);
}

#[test]
fn a_remote_device_without_volume_control_is_not_asked_to_change_it() {
    let mut core = headless();
    core.local_ready = false;
    let mut remote = remote_snapshot("phone", "spotify:track:a", true);
    if let Some(device) = remote.state.device.as_mut() {
        device.supports_volume = Some(false);
        device.volume_percent = Some(40);
    }
    core.remote = Some(remote);
    assert!(matches!(core.target(), Target::Remote(_)));
    assert!(!core.can_set_volume());
    core.set_volume(percent_to_volume(10));
    assert!(core.pending_remote_volume.is_none());
    assert_eq!(volume_to_percent(core.now_playing().unwrap().volume), 40);
    if let Some(device) = core.remote.as_mut().and_then(|remote| remote.state.device.as_mut()) {
        device.supports_volume = None;
    }
    assert!(core.can_set_volume());
    core.set_volume(percent_to_volume(10));
    assert!(matches!(core.pending_remote_volume, Some((10, _))));
}

#[test]
fn optimistic_local_seek_holds_position_until_player_confirms_seek() {
    let mut core = headless();
    core.local.track = Some(LocalTrack {
        uri: "spotify:track:test1234".into(),
        title: "Test Track".into(),
        duration_ms: 180_000,
        ..Default::default()
    });
    core.local.position_ms = 10_000;
    core.local.playback = Playback::Playing;
    core.local.seek_sequence = 1;
    core.seek(95_000);
    assert_eq!(core.pending_local_position.map(|(position, _)| position), Some(95_000));
    let now = core.now_playing().unwrap();
    assert!(now.position_ms >= 95_000 && now.position_ms < 96_000);
    let mut stale = core.local.clone();
    stale.position_ms = 11_000;
    core.handle_local(stale);
    assert_eq!(core.pending_local_position.map(|(position, _)| position), Some(95_000));
    assert!(core.now_playing().is_some_and(|now| now.position_ms >= 95_000));
    let mut confirmed = core.local.clone();
    confirmed.seek_sequence = 2;
    confirmed.position_ms = 95_200;
    core.handle_local(confirmed);
    assert_eq!(core.pending_local_position, None);
    assert_eq!(core.now_playing().map(|now| now.position_ms), Some(95_200));
}

#[test]
fn two_toggle_play_actions_return_to_playing() {
    let mut core = headless();
    core.local.track = Some(LocalTrack { uri: "spotify:track:a".into(), title: "A".into(), ..Default::default() });
    core.local.playback = Playback::Playing;
    assert!(core.now_playing().unwrap().playing);
    core.toggle_play();
    core.toggle_play();
    assert!(core.now_playing().unwrap().playing, "the second press sees the first pause");
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Toggle, PlayerCommand::Toggle]);
}

#[test]
fn play_and_pause_do_not_toggle_the_wrong_way() {
    let mut core = headless();
    core.local.track = Some(LocalTrack { uri: "spotify:track:a".into(), ..Default::default() });
    core.local.playback = Playback::Paused;
    core.pause();
    assert!(engine_commands(&mut core).is_empty(), "pause does nothing while paused");
    core.play();
    assert_eq!(engine_commands(&mut core), [PlayerCommand::Toggle]);
    core.play();
    assert!(engine_commands(&mut core).is_empty(), "already playing");
}

#[test]
fn a_load_reported_mid_song_shows_the_button_spinner() {
    let mut core = headless();
    core.local = LocalState {
        connected: true,
        track: Some(LocalTrack { uri: "spotify:track:a".into(), title: "A".into(), duration_ms: 100_000, ..Default::default() }),
        playback: Playback::Playing,
        position_ms: 30_000,
        ..Default::default()
    };
    let mut loading = core.local.clone();
    loading.loading = true;
    core.handle_local(loading);
    let now = core.now_playing().unwrap();
    assert!(now.playing && now.loading);
    let mut started = core.local.clone();
    started.loading = false;
    started.track = Some(LocalTrack { uri: "spotify:track:b".into(), title: "B".into(), duration_ms: 100_000, ..Default::default() });
    started.track_sequence += 1;
    core.handle_local(started);
    assert!(!core.now_playing().unwrap().loading);
}

// ---- Устройства и опрос ---------------------------------------------------------------------------

#[test]
fn shuffle_selected_without_a_device_applies_when_play_starts() {
    let mut core = Core::new(0.5, false);
    assert!(matches!(core.target(), Target::Remote(None)));
    core.set_shuffle(true);
    assert!(core.shuffle_wanted);
    assert!(api_calls(&mut core).iter().all(|call| !matches!(call, ApiCall::Remote { .. } | ApiCall::Play { .. })));
    assert!(notices(&core).is_empty(), "choosing a mode without a device is not an error");
    core.remote = Some(remote_snapshot("remote1", "spotify:track:x", true));
    core.play_context(PlayContext::with_uri("playlist", "spotify:playlist:pl0"), None, None);
    let calls = api_calls(&mut core);
    assert!(matches!(
        calls.as_slice(),
        [ApiCall::Play { device: Some(device), request, shuffle: true }]
            if device == "remote1" && request.context_uri.as_deref() == Some("spotify:playlist:pl0")
    ));
}

#[test]
fn shuffle_targets_active_remote_playback_without_a_device_id() {
    let mut core = Core::new(0.5, false);
    let mut remote = remote_snapshot("x", "spotify:track:x", true);
    if let Some(device) = remote.state.device.as_mut() {
        device.id = None;
    }
    core.remote = Some(remote);
    assert!(matches!(core.target(), Target::Remote(None)));
    core.set_shuffle(true);
    assert!(matches!(
        api_calls(&mut core).as_slice(),
        [ApiCall::Remote { action: RemoteAction::Shuffle(true), device: None }]
    ));
}

#[test]
fn playing_with_no_device_asks_for_one() {
    let mut core = Core::new(0.5, false);
    core.play_uris(vec!["spotify:track:a".into()], 0, PlayContext::new("tracks"));
    // метаданные строки — чтение, устройство им не нужно; играть «нигде» не просим
    let plays: Vec<ApiCall> = api_calls(&mut core)
        .into_iter()
        .filter(|call| matches!(call, ApiCall::Play { .. } | ApiCall::Remote { .. }))
        .collect();
    assert!(plays.is_empty(), "Spotify is never asked to play “nowhere”");
    assert_eq!(notices(&core), ["Choose a device or turn on playback on this computer"]);
    // движок подключается — запрос ждёт его
    let mut core = Core::new(0.5, false);
    core.handle_playback(LocalPlayback::Connecting);
    core.play_uris(vec!["spotify:track:a".into()], 0, PlayContext::new("tracks"));
    assert!(core.queued_play.is_some());
}

#[test]
fn a_start_before_the_promised_engine_waits_for_it() {
    // вход ещё читает ключи — движок обещан, запуск ждёт
    let mut core = Core::new(0.5, false);
    core.expect_local(true);
    core.play_uris(vec!["spotify:track:a".into()], 0, PlayContext::new("tracks"));
    assert!(core.queued_play.is_some());
    assert!(notices(&core).is_empty());
    core.local.connected = true;
    core.handle_playback(LocalPlayback::Ready { device_id: "here".into() });
    assert!(core.queued_play.is_none());
    assert!(engine_commands(&mut core).iter().any(|command| matches!(command, PlayerCommand::Load(_))));
    // вход передумал — просьба выбрать устройство
    let mut core = Core::new(0.5, false);
    core.expect_local(true);
    core.play_uris(vec!["spotify:track:a".into()], 0, PlayContext::new("tracks"));
    core.expect_local(false);
    assert!(core.queued_play.is_none());
    assert_eq!(notices(&core), ["Choose a device or turn on playback on this computer"]);
}

#[test]
fn a_playlist_in_order_starts_at_its_first_row() {
    let mut core = headless();
    core.local.connected = true;
    core.play_context(PlayContext::with_uri("playlist", "spotify:playlist:p"), None, None);
    assert!(matches!(
        engine_commands(&mut core).as_slice(),
        [PlayerCommand::Load(load)] if load.offset_index == Some(0) && load.context_uri.as_deref() == Some("spotify:playlist:p")
    ));
    core.play_context(PlayContext::with_uri("playlist", "spotify:playlist:p"), None, Some(true));
    assert!(matches!(
        engine_commands(&mut core).as_slice(),
        [PlayerCommand::Load(load)] if load.offset_index.is_none() && load.shuffle == Some(true)
    ));
}

#[test]
fn a_shuffled_play_does_not_start_at_track_one() {
    let mut core = headless();
    core.local.connected = true;
    let mut items: Vec<TrackItem> = (0..40).map(|n| song(&format!("spotify:track:{n}"))).collect();
    items[5].available = false;
    let mut starts = std::collections::HashSet::new();
    for _ in 0..40 {
        core.shuffle_from(items.clone(), PlayContext::with_uri("playlist", "spotify:playlist:p"));
        for command in engine_commands(&mut core) {
            if let PlayerCommand::Load(load) = command {
                let start = load.offset_uri.expect("the start is named");
                assert_ne!(start, "spotify:track:5", "an unavailable row is not chosen");
                starts.insert(start);
            }
        }
    }
    assert!(core.shuffle_wanted);
    assert!(starts.len() > 1, "the shuffle start varies");
}

#[test]
fn transferring_back_takes_the_connect_session_without_replaying_a_snapshot() {
    for playing_remote in [false, true] {
        let mut core = headless();
        core.local_device_id = Some("this-computer".into());
        core.local = LocalState {
            connected: true,
            playback: Playback::Stopped,
            track: local_track("spotify:track:bellaire"),
            track_sequence: 3,
            ..Default::default()
        };
        core.selected_device = Some("phone".into());
        let mut remote = remote_snapshot("phone", "spotify:track:metallica", playing_remote);
        remote.state.progress_ms = Some(65_000);
        core.remote = Some(remote);
        core.on_now_playing_changed();
        let upcoming = ["spotify:track:metallica", "spotify:track:extra", "spotify:track:context"];
        loaded_queue(&mut core, "spotify:track:metallica", &upcoming);
        core.manual_queue = upcoming[..2].iter().map(|uri| (*uri).to_owned()).collect();
        core.resume_track = Some("spotify:track:metallica".into());
        core.resume_queue = vec!["spotify:track:old-queue".into()];
        core.local_list = Some(vec!["spotify:track:bellaire".into()]);
        let _ = engine_commands(&mut core);
        core.transfer_to("this-computer".into(), true);
        assert_eq!(engine_commands(&mut core), [PlayerCommand::Transfer]);
        assert!(core.transferring());
        assert!(core.local_list.is_none());
        assert!(core.resume_queue.is_empty());
        assert_eq!(queue_uris(&core).1, upcoming);

        // Loading приходит раньше смены трека и ещё несёт старую локальную песню
        let loading = LocalState { playback: Playback::Loading, ..core.local.clone() };
        core.handle_local(loading);
        assert_eq!(core.now_playing().unwrap().track.uri, "spotify:track:metallica");
        assert_eq!(queue_uris(&core).1, upcoming);

        let transferred = LocalState {
            connected: true,
            playback: if playing_remote { Playback::Playing } else { Playback::Paused },
            track: Some(LocalTrack { uri: "spotify:track:metallica".into(), duration_ms: 300_000, ..Default::default() }),
            position_ms: 65_000,
            track_sequence: 4,
            ..Default::default()
        };
        core.handle_local(transferred.clone());
        assert!(!core.transferring(), "the transfer is confirmed by the engine");
        let now = core.now_playing().unwrap();
        assert_eq!(now.track.uri, "spotify:track:metallica");
        assert_eq!(now.position_ms, 65_000);
        assert_eq!(now.playing, playing_remote);
        assert_eq!(core.target(), Target::Local);
        assert_eq!(queue_uris(&core).1, upcoming);
        assert_eq!(core.manual_queue.len(), 2, "the transfer does not eat the manual copy");
        // повторный выбор этого ПК ничего не шлёт и не мешает следующему переходу
        core.transfer_to("this-computer".into(), true);
        assert!(engine_commands(&mut core).is_empty());
        core.handle_local(LocalState { track_sequence: 5, ..transferred });
        assert_eq!(queue_uris(&core).1, upcoming[1..]);
        assert_eq!(core.manual_queue, ["spotify:track:extra"]);
    }
}

#[test]
fn a_transfer_without_an_answer_says_the_device_did_not_respond() {
    let mut core = headless();
    core.transfer_to("speaker".into(), true);
    assert!(matches!(api_calls(&mut core).as_slice(), [ApiCall::Transfer { device, play: true }] if device == "speaker"));
    core.handle_api(ApiReply::Transferred { device: "speaker".into(), name: Some("Speaker".into()), result: Ok(()) });
    assert!(core.transferring());
    if let Some(transfer) = core.transfer.as_mut() {
        transfer.at = Instant::now() - TRANSFER_TIMEOUT - Duration::from_millis(1);
    }
    core.tick();
    assert!(!core.transferring());
    assert!(notices(&core).contains(&"Speaker didn't respond".to_owned()));
}

#[test]
fn a_transfer_is_confirmed_by_a_poll_of_the_new_device() {
    let mut core = headless();
    core.transfer_to("speaker".into(), true);
    let seq = {
        core.poll_remote();
        core.remote_poll_seq
    };
    let state = remote_snapshot("speaker", "spotify:track:a", true).state;
    core.handle_api(ApiReply::Playback { seq, result: Ok(Some(state)) });
    assert!(!core.transferring());
    assert_eq!(core.target(), Target::Remote(Some("speaker".into())));
}

#[test]
fn a_device_that_vanishes_twice_is_lost_and_playback_can_continue_here() {
    let mut core = headless();
    core.local.connected = true;
    let mut remote = remote_snapshot("phone", "spotify:track:a", true);
    remote.state.context_uri = Some("spotify:album:x".into());
    remote.state.progress_ms = Some(92_000);
    core.remote = Some(remote);
    core.poll_remote();
    core.handle_api(ApiReply::Playback { seq: core.remote_poll_seq, result: Ok(None) });
    assert!(!core.device_lost, "one empty poll is not a loss yet");
    assert!(core.now_playing().is_some());
    core.poll_remote();
    core.handle_api(ApiReply::Playback { seq: core.remote_poll_seq, result: Ok(None) });
    assert!(core.device_lost);
    let shown = core.now_playing().expect("the last track stays on screen");
    assert!(!shown.playing);
    assert!(shown.position_ms >= 92_000);
    let _ = engine_commands(&mut core);
    core.transfer_here(true);
    assert!(!core.device_lost);
    let commands = engine_commands(&mut core);
    assert!(
        matches!(
            commands.as_slice(),
            [PlayerCommand::Load(load)]
                if load.context_uri.as_deref() == Some("spotify:album:x")
                    && load.offset_uri.as_deref() == Some("spotify:track:a")
                    && load.position_ms >= 92_000
                    && load.play
        ),
        "{commands:?}"
    );
    assert!(!commands.contains(&PlayerCommand::Transfer), "nothing to take over — Load, not Transfer");
}

#[test]
fn a_command_answered_404_marks_the_device_lost() {
    let mut core = headless();
    core.local_ready = false;
    core.remote = Some(remote_snapshot("phone", "spotify:track:a", true));
    core.handle_api(ApiReply::Remote {
        action: RemoteAction::Pause,
        result: Err(ApiError::Status { status: 404, message: "Device not found".into() }),
    });
    assert!(core.device_lost);
    assert!(notices(&core).iter().any(|text| text.contains("Choose a device first")));
}

#[test]
fn an_old_poll_answer_is_dropped() {
    let mut core = headless();
    core.poll_remote();
    let old = core.remote_poll_seq;
    core.poll_remote();
    let state = remote_snapshot("phone", "spotify:track:old", true).state;
    core.handle_api(ApiReply::Playback { seq: old, result: Ok(Some(state)) });
    assert!(core.remote.is_none());
}

#[test]
fn local_idle_polls_every_twenty_seconds_not_four() {
    let mut core = headless();
    play_on_phone(&mut core);
    assert_eq!(core.poll_interval(), REMOTE_POLL_ACTIVE);
    core.remote = None;
    core.local.track = local_track("spotify:track:a");
    core.local.playback = Playback::Paused;
    assert_eq!(core.poll_interval(), REMOTE_POLL_IDLE);
}

#[test]
fn an_unseen_idle_window_polls_less_often() {
    let mut core = headless();
    core.app_active = false;
    assert_eq!(core.poll_interval(), REMOTE_POLL_UNSEEN);
    core.app_active = true;
    // нигде ничего не играет — спрашивать часто не о чем
    assert_eq!(core.poll_interval(), REMOTE_POLL_IDLE);
    play_on_phone(&mut core);
    assert_eq!(core.poll_interval(), REMOTE_POLL_ACTIVE);
}

fn play_on_phone(core: &mut Core) {
    core.poll_remote();
    let seq = core.remote_poll_seq;
    let state = remote_snapshot("phone", "spotify:track:a", true).state;
    core.handle_api(ApiReply::Playback { seq, result: Ok(Some(state)) });
}

#[test]
fn a_poll_asked_for_soon_is_not_put_off_while_unseen() {
    let mut core = headless();
    core.app_active = false;
    core.poll_remote_soon();
    let due = core.poll_interval().saturating_sub(core.remote_polled_at.elapsed());
    assert!(due <= Duration::from_millis(700), "in {due:?}");
}

#[test]
fn another_devices_shuffle_is_adopted_unless_ours_is_fresh() {
    let mut core = headless();
    core.local_ready = false;
    core.remote = Some(remote_snapshot("phone", "spotify:track:a", true));
    core.poll_remote();
    let mut state = remote_snapshot("phone", "spotify:track:a", true).state;
    state.shuffle = true;
    core.handle_api(ApiReply::Playback { seq: core.remote_poll_seq, result: Ok(Some(state)) });
    assert!(core.shuffle_wanted);
}

// ---- Контекст и история ----------------------------------------------------------------------------

#[test]
fn a_started_context_is_shown_with_its_callers_title_at_once() {
    let mut core = headless();
    core.local.connected = true;
    let context = PlayContext::with_uri("playlist", "spotify:playlist:p").titled("My playlist").with_cover("c.jpg");
    core.play_context(context.clone(), None, None);
    assert_eq!(core.shown_context(), context);
    // радио: играют показанные песни, а контекстом показана станция
    let station = PlayContext::with_uri("radio", "spotify:station:track:t").titled("Radio “T”");
    core.play_items(vec![song("spotify:track:a"), song("spotify:track:b")], 0, station.clone());
    assert!(matches!(
        engine_commands(&mut core).last(),
        Some(PlayerCommand::Load(load)) if load.context_uri.is_none() && load.uris.len() == 2
    ));
    assert_eq!(core.shown_context(), station);
}

#[test]
fn an_engine_restart_keeps_the_context_and_the_manual_queue() {
    let mut core = headless();
    core.local.connected = true;
    let playlist = PlayContext::with_uri("playlist", "spotify:playlist:p").titled("Playlist");
    core.play_context(playlist, Some("spotify:track:b".into()), None);
    playing(&mut core, "spotify:track:b");
    core.local.position_ms = 42_000;
    core.manual_queue = vec!["spotify:track:m1".into(), "spotify:track:m2".into()];
    engine_commands(&mut core);
    let spec = core.prepare_restart().expect("a restart point");
    assert_eq!(spec.context_uri.as_deref(), Some("spotify:playlist:p"));
    assert_eq!(spec.offset_uri.as_deref(), Some("spotify:track:b"));
    assert!(spec.play);
    // старый движок закрылся: пока новый не готов, очередь не возвращается
    let mut closed = core.local.clone();
    closed.connected = false;
    closed.playback = Playback::Stopped;
    core.handle_local(closed);
    core.handle_playback(LocalPlayback::Connecting);
    let mut early = core.local.clone();
    early.connected = true;
    early.playback = Playback::Playing;
    core.handle_local(early);
    assert!(engine_commands(&mut core).is_empty());
    core.handle_playback(LocalPlayback::Ready { device_id: "pc".into() });
    let mut fresh = LocalState { connected: true, track_sequence: 1, ..Default::default() };
    core.handle_local(fresh.clone());
    fresh.track = local_track("spotify:track:b");
    fresh.playback = Playback::Playing;
    core.handle_local(fresh);
    let added: Vec<_> = engine_commands(&mut core)
        .into_iter()
        .filter_map(|command| match command {
            PlayerCommand::AddToQueue(uri) => Some(uri),
            _ => None,
        })
        .collect();
    assert_eq!(added, vec!["spotify:track:m1".to_owned(), "spotify:track:m2".to_owned()]);
    assert_eq!(core.manual_queue, vec!["spotify:track:m1".to_owned(), "spotify:track:m2".to_owned()]);
    // список URI возвращается списком
    let mut core = headless();
    core.local.connected = true;
    core.play_items(vec![song("spotify:track:a"), song("spotify:track:b")], 0, PlayContext::new("search"));
    playing(&mut core, "spotify:track:b");
    let spec = core.prepare_restart().expect("a restart point");
    assert_eq!(spec.context_uri, None);
    assert_eq!(spec.uris, vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()]);
    assert_eq!(spec.offset_uri.as_deref(), Some("spotify:track:b"));
}

#[test]
fn a_paused_station_list_stays_shown_as_the_station() {
    let mut core = headless();
    core.local.connected = true;
    let station = PlayContext::with_uri("radio", "spotify:station:track:t").titled("Radio “T”");
    let songs = vec![song("spotify:track:a"), song("spotify:track:b"), song("spotify:track:c")];
    core.play_items(songs, 0, station.clone());
    core.local.playback = Playback::Paused;
    core.local.track = local_track("spotify:track:c");
    core.optimistic_playing = None;
    core.age_assumed_context(ASSUMED_CONTEXT_HOLD);
    assert_eq!(core.shown_context(), station, "a pause does not drop the station");
    // трек вне списка (автоплей) — станция больше не показана
    core.local.track = local_track("spotify:track:other");
    assert_ne!(core.shown_context(), station);
}

#[test]
fn an_album_started_without_a_title_is_named_after_the_playing_track() {
    let mut core = headless();
    core.local.connected = true;
    core.remember(&TrackItem { album_id: "al".into(), album: "Discovery".into(), cover: "c.jpg".into(), ..named("spotify:track:a", "One More Time") });
    // плитка «Слушать» на главной: только URI, без подписи
    core.play_context(PlayContext::with_uri("album", "spotify:album:al"), None, None);
    playing(&mut core, "spotify:track:a");
    let context = core.shown_context();
    assert_eq!(context.title, "Discovery");
    assert_eq!(context.cover, "c.jpg");
}

#[test]
fn a_context_named_with_the_queue_titles_the_playing_card() {
    let mut core = headless();
    core.local.connected = true;
    // плитка «Слушать»: только URI, а данных играющего трека ещё нет
    core.play_context(PlayContext::with_uri("album", "spotify:album:al"), None, None);
    playing(&mut core, "spotify:track:a");
    assert_eq!(core.shown_context().title, "");
    core.refresh_queue(true);
    let mut answer = fetched("spotify:track:a", &["spotify:track:b"]);
    answer.context = Some(PlayContext::with_uri("album", "spotify:album:al").titled("Discovery").with_cover("d.jpg"));
    answer_latest(&mut core, Ok(answer));
    let context = core.shown_context();
    assert_eq!((context.title.as_str(), context.cover.as_str()), ("Discovery", "d.jpg"));
    // подпись вызывающего не перекрывается
    let mine = PlayContext::with_uri("playlist", "spotify:playlist:p").titled("Mine").with_cover("m.jpg");
    core.play_context(mine.clone(), None, None);
    core.refresh_queue(true);
    let mut answer = fetched("spotify:track:a", &[]);
    answer.context = Some(PlayContext::with_uri("playlist", "spotify:playlist:p").titled("Theirs"));
    answer_latest(&mut core, Ok(answer));
    assert_eq!(core.shown_context(), mine);
}

#[test]
fn a_context_known_only_by_uri_is_named_by_its_kind() {
    let mut core = headless();
    core.local_ready = false;
    let mut remote = remote_snapshot("phone", "spotify:track:a", true);
    remote.state.context_uri = Some("spotify:user:me:collection".into());
    core.remote = Some(remote);
    let context = core.shown_context();
    assert_eq!(context.kind, "liked");
    assert_eq!(context.context_uri.as_deref(), Some("spotify:user:me:collection"));
}

#[test]
fn each_track_change_reports_the_end_of_the_last_and_the_start_of_the_next() {
    let mut core = headless();
    core.local = LocalState {
        connected: true,
        playback: Playback::Playing,
        track: Some(LocalTrack { uri: "spotify:track:a".into(), duration_ms: 10_000, ..Default::default() }),
        ..Default::default()
    };
    core.on_now_playing_changed();
    let mut next = core.local.clone();
    next.track = Some(LocalTrack { uri: "spotify:track:b".into(), duration_ms: 10_000, ..Default::default() });
    next.track_sequence += 1;
    core.handle_local(next);
    let mut started = Vec::new();
    let mut ended = Vec::new();
    for effect in core.take_effects() {
        match effect {
            Effect::Started(item) => started.push(item.track.uri.clone()),
            Effect::Ended(item) => {
                assert!(item.local);
                ended.push(item.track.uri.clone());
            }
            _ => {}
        }
    }
    assert_eq!(started, ["spotify:track:a", "spotify:track:b"]);
    assert_eq!(ended, ["spotify:track:a"]);
}

#[test]
fn queue_notices_count_tracks_in_english() {
    let mut core = headless();
    loaded_queue(&mut core, "spotify:track:a", &["spotify:track:ctx1"]);
    core.queue_many(vec![named("spotify:track:b", "b")]);
    assert_eq!(notices(&core).last().map(String::as_str), Some("Added to queue: 1 track"));
}

// ---- Актор: снимок и события поверх фейкового движка и Web API ------------------------------------

mod actor {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use md3_model::{PlayContext, QueueRow};
    use tokio::sync::{broadcast, mpsc, watch};

    use super::song;
    use crate::engine::{LocalState, LocalTrack, Playback, PlayerCommand};
    use crate::state::actor::{Actor, Command, LocalMsg, Msg, apply_changes, uris};
    use crate::state::backend::{BoxFuture, LocalEngine, WebBackend};
    use crate::state::core::{ApiCall, ApiReply, Core, FetchedQueue};
    use crate::{PlayerEvent, PlayerSnapshot};

    #[derive(Default)]
    struct FakeEngine {
        commands: Mutex<Vec<PlayerCommand>>,
    }

    impl LocalEngine for FakeEngine {
        fn command(&self, command: PlayerCommand) -> Result<(), String> {
            self.commands.lock().unwrap().push(command);
            Ok(())
        }

        fn level(&self) -> f64 {
            0.5
        }
    }

    #[derive(Default)]
    struct FakeWeb {
        calls: Mutex<Vec<ApiCall>>,
        queue: Mutex<Option<FetchedQueue>>,
    }

    impl WebBackend for FakeWeb {
        fn call(&self, call: ApiCall) -> BoxFuture<ApiReply> {
            self.calls.lock().unwrap().push(call.clone());
            let queue = self.queue.lock().unwrap().clone();
            Box::pin(async move {
                match call {
                    ApiCall::Queue { seq } => ApiReply::Queue { seq, result: Ok(queue.unwrap_or_default()) },
                    ApiCall::Playback { seq } => ApiReply::Playback { seq, result: Ok(None) },
                    ApiCall::Remote { action, .. } => ApiReply::Remote { action, result: Ok(()) },
                    ApiCall::Play { .. } => {
                        ApiReply::Remote { action: crate::state::core::RemoteAction::Play, result: Ok(()) }
                    }
                    ApiCall::Transfer { device, .. } => ApiReply::Transferred { device, name: None, result: Ok(()) },
                    ApiCall::AddMany { request, uris, .. } => {
                        ApiReply::QueueBatchAdded { request, added: uris.len(), result: Ok(()) }
                    }
                    ApiCall::AlbumTracks { request, .. } => ApiReply::AlbumTracks { request, result: Ok(Vec::new()) },
                    ApiCall::TrackInfo { request, uri, .. } => {
                        ApiReply::TrackInfo { request, uri, result: Err(sf_api::ApiError::Network("no metadata".into())) }
                    }
                }
            })
        }

        fn fetch_cover(&self, url: String) -> BoxFuture<Result<PathBuf, String>> {
            Box::pin(async move { Ok(PathBuf::from(format!("{}.jpg", url.len()))) })
        }
    }

    /// Web API, где запись очереди висит, пока её не отпустят.
    #[derive(Default)]
    struct GatedWeb {
        calls: Mutex<Vec<ApiCall>>,
        release: Arc<tokio::sync::Notify>,
    }

    impl WebBackend for GatedWeb {
        fn call(&self, call: ApiCall) -> BoxFuture<ApiReply> {
            self.calls.lock().unwrap().push(call.clone());
            let release = self.release.clone();
            Box::pin(async move {
                match call {
                    ApiCall::AddMany { request, uris, .. } => {
                        release.notified().await;
                        ApiReply::QueueBatchAdded { request, added: uris.len(), result: Ok(()) }
                    }
                    ApiCall::Remote { action, .. } => ApiReply::Remote { action, result: Ok(()) },
                    ApiCall::TrackInfo { request, uri, .. } => {
                        ApiReply::TrackInfo { request, uri, result: Err(sf_api::ApiError::Network("no metadata".into())) }
                    }
                    other => panic!("unexpected call {other:?}"),
                }
            })
        }

        fn fetch_cover(&self, _url: String) -> BoxFuture<Result<PathBuf, String>> {
            Box::pin(async { Err(String::new()) })
        }
    }

    #[tokio::test]
    async fn transport_does_not_wait_behind_queue_writes() {
        use crate::state::core::RemoteAction;
        let (tx, rx) = mpsc::unbounded_channel();
        let (snapshot, _receiver) = watch::channel(PlayerSnapshot::default());
        let (events, _events) = broadcast::channel(16);
        let web = Arc::new(GatedWeb::default());
        let mut actor = Actor::new(
            Core::new(0.5, false),
            rx,
            tx,
            Arc::new(snapshot),
            events,
            web.clone(),
            Arc::new(|_| {}),
            Arc::new(|_, _, _| {}),
            None,
        );
        let device = Some("phone".to_owned());
        let album: Vec<String> = (0..30).map(|i| format!("spotify:track:{i}")).collect();
        actor.api(ApiCall::AddMany { request: 1, uris: album, device: device.clone() });
        actor.api(ApiCall::AddMany { request: 2, uris: vec!["spotify:track:x".into()], device: device.clone() });
        actor.api(ApiCall::Remote { action: RemoteAction::Pause, device });
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        let calls = web.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|call| matches!(call, ApiCall::Remote { action: RemoteAction::Pause, .. })));
        // вторая запись очереди ждёт первую
        assert_eq!(calls.iter().filter(|call| matches!(call, ApiCall::AddMany { .. })).count(), 1);
        web.release.notify_one();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        let calls = web.calls.lock().unwrap().clone();
        assert!(matches!(calls.last(), Some(ApiCall::AddMany { request: 2, .. })));
    }

    struct Rig {
        actor: Actor,
        tx: mpsc::UnboundedSender<Msg>,
        snapshot: watch::Receiver<PlayerSnapshot>,
        events: broadcast::Receiver<PlayerEvent>,
        engine: Arc<FakeEngine>,
        model: Vec<QueueRow>,
    }

    impl Rig {
        fn new() -> Self {
            let (tx, rx) = mpsc::unbounded_channel();
            let (snapshot, receiver) = watch::channel(PlayerSnapshot::default());
            let snapshot = Arc::new(snapshot);
            let (events, receiver_events) = broadcast::channel(1024);
            let web = Arc::new(FakeWeb::default());
            *web.queue.lock().unwrap() =
                Some(FetchedQueue { current: Some(song("spotify:track:a")), next: vec![song("spotify:track:ctx")], context: None });
            let actor =
                Actor::new(
                    Core::new(0.5, false),
                    rx,
                    tx.clone(),
                    snapshot,
                    events,
                    web,
                    Arc::new(|_| {}),
                    Arc::new(|_, _, _| {}),
                    None,
                );
            Self { actor, tx, snapshot: receiver, events: receiver_events, engine: Arc::default(), model: Vec::new() }
        }

        /// Обработать сообщение и опубликовать снимок; события — в «модель моста».
        async fn send(&mut self, message: Msg) -> Vec<PlayerEvent> {
            self.actor.handle(message);
            self.actor.step();
            // ответы фейкового Web API
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
            self.actor.drain();
            self.actor.step();
            let mut seen = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                if let PlayerEvent::Queue(change) = &event {
                    apply_changes(&mut self.model, std::slice::from_ref(change));
                }
                seen.push(event);
            }
            seen
        }

        async fn ready(&mut self) {
            let engine = self.engine.clone() as Arc<dyn LocalEngine>;
            self.send(Msg::Local(LocalMsg::Ready {
                engine,
                device_id: "pc".into(),
                name: "This PC".into(),
                bitrate: 320,
                autoplay: true,
            }))
            .await;
        }

        async fn playing(&mut self, uri: &str, sequence: u64) -> Vec<PlayerEvent> {
            self.send(Msg::Engine(Box::new(crate::engine::EngineEvent::State(LocalState {
                connected: true,
                playback: Playback::Playing,
                track: Some(LocalTrack { uri: uri.into(), title: uri.into(), duration_ms: 100_000, ..Default::default() }),
                track_sequence: sequence,
                ..Default::default()
            }))))
            .await
        }
    }

    #[tokio::test]
    async fn the_snapshot_and_its_events_follow_playback() {
        let mut rig = Rig::new();
        rig.ready().await;
        let events = rig.playing("spotify:track:a", 1).await;
        let snapshot = rig.snapshot.borrow().clone();
        assert_eq!(snapshot.track.as_ref().map(|track| track.uri.as_str()), Some("spotify:track:a"));
        assert!(snapshot.playing && snapshot.is_local);
        assert_eq!(snapshot.codec, "OGG 320");
        assert_eq!(snapshot.device.as_ref().map(|device| device.name.as_str()), Some("This PC"));
        assert!(events.iter().any(|event| matches!(event, PlayerEvent::TrackChanged)));
        assert!(events.iter().any(|event| matches!(event, PlayerEvent::StateChanged)));
        assert!(events.iter().any(|event| matches!(event, PlayerEvent::TrackStarted(_))));
        // модель моста, собранная из изменений, совпадает со снимком
        assert_eq!(rig.model, *snapshot.queue);

        rig.send(Msg::Command(Command::AddToQueue(vec![super::named("spotify:track:q", "Q")]))).await;
        let snapshot = rig.snapshot.borrow().clone();
        assert_eq!(rig.model, *snapshot.queue);
        assert_eq!(snapshot.manual_count, 1);
        assert!(snapshot.can_clear_queue && snapshot.queue_reorderable);
        assert_eq!(
            rig.engine.commands.lock().unwrap().last(),
            Some(&PlayerCommand::AddToQueue("spotify:track:q".into()))
        );

        rig.send(Msg::Command(Command::Next)).await;
        let snapshot = rig.snapshot.borrow().clone();
        assert_eq!(snapshot.track.as_ref().map(|track| track.uri.as_str()), Some("spotify:track:q"));
        assert_eq!(rig.model, *snapshot.queue);
        assert_eq!(uris(&snapshot.queue)[0].1, "spotify:track:q");
        assert_eq!(snapshot.current_index, 0);
    }

    #[tokio::test]
    async fn a_notice_reaches_subscribers() {
        let mut rig = Rig::new();
        let events = rig.send(Msg::Command(Command::PlayContext {
            context: PlayContext::with_uri("playlist", "spotify:playlist:p"),
            offset: None,
            shuffle: None,
        }))
        .await;
        assert!(events.iter().any(|event| matches!(event, PlayerEvent::Notice(text) if text.contains("Choose a device"))));
        let _ = rig.tx.send(Msg::Restore(None));
    }
}

/// Пауза, пока грузится следующий трек, — пауза: строка показывает загрузку, а не «играет».
#[test]
fn pausing_during_a_switch_is_a_pause() {
    let mut core = headless();
    playing(&mut core, "spotify:track:a");
    core.play_items(vec![named("spotify:track:a", "A"), named("spotify:track:new", "New")], 1, PlayContext::new("search"));
    core.toggle_play();
    assert!(!core.believed_playing());
}
