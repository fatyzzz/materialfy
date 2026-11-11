//! Отчёт о прослушивании в Spotify, как у официального клиента: событие RawCoreStream в конверте gabo
//! (POST {spclient}/gabo-receiver-service/v3/events/, тело PublishEventsRequest) и запись в историю
//! прослушиваний аккаунта (herodotus, spotify:list:play-history:v1). librespot ни того ни другого не делает
//! (librespot-org/librespot#913). Схемы — из дескрипторов клиента 1.3.8 (gabito.proto,
//! include/RawCoreStream.proto, context_*.proto, resumption_platform.proto). Проверено на живом аккаунте:
//! оба запроса принимаются, трек появляется в play-history (без отчёта — нет); учтётся ли RawCoreStream
//! в статистике, отсюда не видно. Выключено по умолчанию (настройка reportPlays).
//! Один запрос на трек и без повторов: ответ с ошибкой только пишется в лог.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use librespot_core::Session;
use protobuf::CodedOutputStream;
use reqwest::header::CONTENT_TYPE;

const ENDPOINT: &str = "/gabo-receiver-service/v3/events/";
const HISTORY_ENDPOINT: &str = "/herodotus/spotify.resumption.v1.ResumePointRevisionService/CreateResumePointRevision";
const PLAY_HISTORY: &str = "spotify:list:play-history:v1";
const TIMEOUT: Duration = Duration::from_secs(20);

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Один запуск: sequence_id (20 байт, как у настольного клиента), id сессии приложения, id монотонных
/// часов и их начало. sequence_number считается отдельно для каждого имени события, с 1.
struct Run {
    sequence_id: [u8; 20],
    app_session: [u8; 16],
    clock_id: i64,
    started: Instant,
}

static RUN: LazyLock<Run> = LazyLock::new(|| Run {
    sequence_id: rand::random(),
    app_session: rand::random(),
    clock_id: i64::from(rand::random::<u32>()),
    started: Instant::now(),
});
static SEQUENCES: Mutex<Vec<(&'static str, i64)>> = Mutex::new(Vec::new());
/// Версия SDK отправки событий, как её пишет настольный клиент (1.2.95 и 1.3.8 одинаково); сервис сверяет
/// с ней размеры пачек, а у нас в запросе всегда одна.
const SDK_VERSION: &str = "0.9.4-rl-essopt-loginsend-onlinesend-bcdsend-heartbeat300.0s/30.0s-modern-payload125kB-batch100";

fn next_sequence(name: &'static str) -> i64 {
    let mut sequences = SEQUENCES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, number)) = sequences.iter_mut().find(|(held, _)| *held == name) {
        *number += 1;
        return *number;
    }
    sequences.push((name, 1));
    1
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    if on {
        // монотонные часы событий идут от включения, а не от первого отчёта
        LazyLock::force(&RUN);
    }
    ENABLED.store(on, Ordering::Relaxed);
}

/// Одно прослушивание на этом ПК.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    /// spotify:track:… | spotify:episode:…
    pub uri: String,
    /// "" — играл без контекста
    pub context_uri: String,
    pub ms_played: u32,
    /// unix-время начала, мс
    pub started_ms: i64,
    /// unix-время конца, мс
    pub ended_ms: i64,
    /// trackdone | clickrow | …
    pub reason_start: &'static str,
    pub reason_end: &'static str,
    /// длительность трека, мс (0 — неизвестна)
    pub duration_ms: i64,
    /// битрейт движка, кбит/с: по нему librespot выбрал файл (0 — неизвестен)
    pub bitrate_kbps: u16,
    /// скорость воспроизведения (1.0 — как записано)
    pub speed: f64,
}

/// Файл, который играл librespot: id (20 байт) и подпись формата («Vorbis 320 kbps»).
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub file_id: Vec<u8>,
    pub format: String,
}

impl Play {
    /// Доигран без пауз и перемоток: один отрезок от начала до конца. Только тогда пишется
    /// RawCoreStreamSegment — иначе его границ мы не знаем.
    fn one_segment(&self) -> bool {
        self.reason_end == "trackdone"
            && self.duration_ms > 0
            && ((f64::from(self.ms_played) * self.speed) as i64 - self.duration_ms).abs() <= 3_000
    }

    fn ms_nominal(&self) -> i32 {
        (f64::from(self.ms_played) * self.speed) as i32
    }
}

/// Тот же выбор, что у librespot (playback/src/player.rs, порядок форматов по битрейту).
async fn played_file(session: &Session, uri: &str, bitrate_kbps: u16) -> Option<Audio> {
    use librespot_metadata::audio::{AudioFileFormat as F, AudioItem};
    let order = match bitrate_kbps {
        96 => [F::OGG_VORBIS_96, F::MP3_96, F::OGG_VORBIS_160, F::MP3_160, F::MP3_256, F::OGG_VORBIS_320, F::MP3_320],
        160 => [F::OGG_VORBIS_160, F::MP3_160, F::OGG_VORBIS_96, F::MP3_96, F::MP3_256, F::OGG_VORBIS_320, F::MP3_320],
        320 => [F::OGG_VORBIS_320, F::MP3_320, F::MP3_256, F::OGG_VORBIS_160, F::MP3_160, F::OGG_VORBIS_96, F::MP3_96],
        _ => return None,
    };
    let uri = librespot_core::SpotifyUri::from_uri(uri).ok()?;
    let item = AudioItem::get_file(session, uri).await.ok()?;
    let (format, file) = order.iter().find_map(|format| item.files.get(format).map(|file| (*format, file)))?;
    let label = match format {
        F::OGG_VORBIS_96 => "Vorbis 96 kbps",
        F::OGG_VORBIS_160 => "Vorbis 160 kbps",
        F::OGG_VORBIS_320 => "Vorbis 320 kbps",
        F::MP3_96 => "MP3 96 kbps",
        F::MP3_160 => "MP3 160 kbps",
        F::MP3_256 => "MP3 256 kbps",
        F::MP3_320 => "MP3 320 kbps",
        _ => return None,
    };
    Some(Audio { file_id: file.0.to_vec(), format: label.to_owned() })
}

/// Причина конца: доигран сам — trackdone, иначе — endplay (пропуск, выбор другого, стоп).
pub fn reason_end(natural: bool) -> &'static str {
    if natural { "trackdone" } else { "endplay" }
}

/// Причина начала: после доигранного — trackdone, иначе — выбор пользователя.
pub fn reason_start(previous_natural: bool) -> &'static str {
    if previous_natural { "trackdone" } else { "clickrow" }
}

/// «Источник» — тип контекста: playlist, album, artist, collection…; без контекста — тип самого трека.
fn source(uri: &str) -> &str {
    let mut parts = uri.split(':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("spotify"), Some("user"), Some(_)) if uri.ends_with(":collection") => "collection",
        (Some("spotify"), Some(kind), Some(_)) => kind,
        _ => "unknown",
    }
}

fn encode(write: impl FnOnce(&mut CodedOutputStream) -> protobuf::Result<()>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut stream = CodedOutputStream::vec(&mut out);
    // запись в Vec не отказывает
    let _ = write(&mut stream).and_then(|()| stream.flush());
    drop(stream);
    out
}

fn context_of(play: &Play) -> &str {
    if play.context_uri.is_empty() { &play.uri } else { &play.context_uri }
}

/// RawCoreStream (номера полей — из дескриптора 1.3.8; поля, которых этот плеер не знает, не пишутся).
/// media_id — id файла (20 байт), как у настольного клиента в перехвате, а не gid трека.
pub fn raw_core_stream(play: &Play, audio: Option<&Audio>, playback_id: &[u8; 16], stream_id: &[u8; 16]) -> Vec<u8> {
    let context = context_of(play);
    encode(|out| {
        out.write_bytes(1, playback_id)?;
        out.write_bytes(2, &[0; 16])?;
        if let Some(audio) = audio {
            out.write_bytes(4, &audio.file_id)?;
        }
        out.write_string(5, "audio")?;
        out.write_string(9, source(context))?;
        out.write_string(10, play.reason_start)?;
        out.write_string(11, source(context))?;
        out.write_string(12, play.reason_end)?;
        out.write_int64(13, play.started_ms)?;
        out.write_int32(14, play.ms_played as i32)?;
        out.write_int32(15, play.ms_nominal())?;
        if let Some(audio) = audio {
            out.write_string(22, &audio.format)?;
        }
        out.write_string(23, context)?;
        out.write_string(24, &play.uri)?;
        out.write_string(28, "context")?;
        out.write_string(32, "none")?;
        out.write_bool(39, true)?;
        out.write_bytes(53, stream_id)?;
        out.write_int64(56, play.ended_ms)
    })
}

/// Единственный отрезок прослушивания (RawCoreStreamSegment): только для play.one_segment().
pub fn raw_core_stream_segment(play: &Play, audio: Option<&Audio>, playback_id: &[u8; 16], stream_id: &[u8; 16]) -> Vec<u8> {
    let context = context_of(play);
    encode(|out| {
        out.write_bytes(1, playback_id)?;
        out.write_int64(2, 0)?;
        out.write_int64(3, i64::from(play.ms_nominal()))?;
        out.write_int32(4, play.ms_played as i32)?;
        out.write_string(5, play.reason_start)?;
        out.write_string(6, play.reason_end)?;
        out.write_double(7, play.speed)?;
        out.write_int64(8, play.started_ms)?;
        out.write_int64(9, play.ended_ms)?;
        out.write_string(13, "audio")?;
        out.write_string(15, &play.uri)?;
        out.write_bool(18, true)?;
        out.write_string(19, "context")?;
        out.write_bytes(22, stream_id)?;
        out.write_string(25, context)?;
        out.write_int32(33, play.ms_played as i32)?;
        out.write_int32(34, play.ms_nominal())?;
        if let Some(audio) = audio {
            out.write_string(56, &audio.format)?;
        }
        out.write_string(58, "none")?;
        out.write_string(62, source(context))?;
        out.write_string(63, source(context))?;
        out.write_int64(73, play.started_ms)
    })
}

fn fragment(name: &str, data: &[u8]) -> Vec<u8> {
    encode(|out| {
        out.write_string(1, name)?;
        out.write_bytes(2, data)
    })
}

fn platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

/// EventEnvelope: имя, сообщение и контексты клиента (как у настольного клиента).
fn envelope(session: &Session, name: &'static str, message: &[u8], now_ms: i64) -> Vec<u8> {
    let run = &*RUN;
    let sequence = next_sequence(name);
    let client_id = hex::decode(session.client_id()).unwrap_or_default();
    // установка = устройство Connect этого ПК (device_id стабилен между запусками)
    let installation: Vec<u8> = hex::decode(session.device_id()).unwrap_or_default().into_iter().take(16).collect();
    let fragments = [
        fragment("message", message),
        fragment("context_client_id", &encode(|out| out.write_bytes(1, &client_id))),
        fragment("context_installation_id", &encode(|out| out.write_bytes(1, &installation))),
        fragment(
            "context_application_desktop",
            &encode(|out| {
                out.write_string(1, librespot_core::version::SPOTIFY_SEMANTIC_VERSION)?;
                out.write_int64(2, librespot_core::version::SPOTIFY_VERSION as i64)?;
                out.write_bytes(3, &run.app_session)
            }),
        ),
        fragment(
            "context_device_desktop",
            &encode(|out| {
                out.write_string(1, platform())?;
                out.write_string(4, session.device_id())
            }),
        ),
        fragment("context_time", &encode(|out| out.write_int64(1, now_ms))),
        fragment(
            "context_monotonic_clock",
            &encode(|out| {
                out.write_int64(1, run.clock_id)?;
                out.write_int64(2, run.started.elapsed().as_millis() as i64)
            }),
        ),
        fragment(
            "context_sdk",
            &encode(|out| {
                out.write_string(1, SDK_VERSION)?;
                out.write_string(2, "cpp")
            }),
        ),
        fragment("context_client_context_id", &[]),
    ];
    encode(|out| {
        out.write_string(2, name)?;
        for fragment in &fragments {
            out.write_bytes(3, fragment)?;
        }
        out.write_bytes(4, &run.sequence_id)?;
        out.write_int64(5, sequence)
    })
}

/// Ошибки по событиям из PublishEventsResponse: (index, transient, reason).
pub fn response_errors(body: &[u8]) -> Vec<(i32, bool, i32)> {
    let mut errors = Vec::new();
    let mut input = protobuf::CodedInputStream::from_bytes(body);
    while let Ok(Some(tag)) = input.read_raw_tag_or_eof() {
        if tag >> 3 != 1 {
            if input.skip_field(protobuf::rt::WireType::new(tag & 7).unwrap_or(protobuf::rt::WireType::LengthDelimited)).is_err() {
                break;
            }
            continue;
        }
        let Ok(bytes) = input.read_bytes() else { break };
        let mut error = (0, false, 0);
        let mut item = protobuf::CodedInputStream::from_bytes(&bytes);
        while let Ok(Some(tag)) = item.read_raw_tag_or_eof() {
            match tag {
                8 => error.0 = item.read_int32().unwrap_or_default(),
                16 => error.1 = item.read_bool().unwrap_or_default(),
                24 => error.2 = item.read_int32().unwrap_or_default(),
                _ => break,
            }
        }
        errors.push(error);
    }
    errors
}

/// Запись в историю прослушиваний аккаунта (herodotus, список spotify:list:play-history:v1): трек и
/// время начала. Её читают «Недавнее» на других устройствах. Схема — resumption_platform.proto 1.3.8:
/// CreateResumePointRevisionRequest { uri = 2, resume_point_revision = 4 { snapshot = 2 { uri = 6 },
/// create_time = 3 } }; user_id сервис берёт из токена.
pub fn play_history_revision(uri: &str, started_ms: i64) -> Vec<u8> {
    let snapshot = encode(|out| out.write_string(6, uri));
    let time = encode(|out| {
        out.write_int64(1, started_ms.div_euclid(1000))?;
        out.write_int32(2, (started_ms.rem_euclid(1000) * 1_000_000) as i32)
    });
    let revision = encode(|out| {
        out.write_bytes(2, &snapshot)?;
        out.write_bytes(3, &time)
    });
    encode(|out| {
        out.write_string(2, PLAY_HISTORY)?;
        out.write_bytes(4, &revision)
    })
}

/// Один POST protobuf на spclient токеном сессии; тело ответа — при 2xx.
async fn post(session: &Session, http: &reqwest::Client, path: &str, body: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let token = session.login5().auth_token().await?;
    let base = session.spclient().base_url().await?;
    let mut request = http
        .post(format!("{base}{path}"))
        .bearer_auth(&token.access_token)
        .header(CONTENT_TYPE, "application/x-protobuf")
        .body(body)
        .timeout(TIMEOUT);
    if let Ok(client_token) = session.spclient().client_token().await {
        request = request.header("client-token", client_token);
    }
    let response = request.send().await.map_err(reqwest::Error::without_url)?;
    let status = response.status();
    let reply = response.bytes().await.map_err(reqwest::Error::without_url)?;
    if !status.is_success() {
        anyhow::bail!("{path}: HTTP {status}: {}", String::from_utf8_lossy(&reply[..reply.len().min(200)]));
    }
    Ok(reply.to_vec())
}

/// Отправить одно прослушивание: RawCoreStream и запись в историю. По одной попытке: при ошибке HTTP
/// событие теряется (не повторяем). `http` — общий клиент приложения (с прокси).
pub async fn report(session: &Session, http: &reqwest::Client, play: &Play) -> anyhow::Result<()> {
    let playback_id: [u8; 16] = rand::random();
    let stream_id: [u8; 16] = rand::random();
    let audio = played_file(session, &play.uri, play.bitrate_kbps).await;
    let mut events = Vec::new();
    if play.one_segment() {
        let segment = raw_core_stream_segment(play, audio.as_ref(), &playback_id, &stream_id);
        events.push(envelope(session, "RawCoreStreamSegment", &segment, play.ended_ms));
    }
    let message = raw_core_stream(play, audio.as_ref(), &playback_id, &stream_id);
    events.push(envelope(session, "RawCoreStream", &message, play.ended_ms));
    let body = encode(|out| events.iter().try_for_each(|event| out.write_bytes(1, event)));
    let reply = post(session, http, ENDPOINT, body).await?;
    let errors = response_errors(&reply);
    if !errors.is_empty() {
        anyhow::bail!("rejected (index, transient, reason): {errors:?}");
    }
    post(session, http, HISTORY_ENDPOINT, play_history_revision(&play.uri, play.started_ms)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play() -> Play {
        Play {
            uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
            context_uri: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".into(),
            ms_played: 212_000,
            started_ms: 1_790_000_000_000,
            ended_ms: 1_790_000_212_000,
            reason_start: reason_start(false),
            reason_end: reason_end(true),
            duration_ms: 212_400,
            bitrate_kbps: 320,
            speed: 1.0,
        }
    }

    fn audio() -> Audio {
        Audio { file_id: vec![7; 20], format: "Vorbis 320 kbps".into() }
    }

    /// Поле `number` сообщения как сырые байты/varint (для проверки без сгенерированных типов).
    fn field(message: &[u8], number: u32) -> Option<Vec<u8>> {
        let mut input = protobuf::CodedInputStream::from_bytes(message);
        while let Ok(Some(tag)) = input.read_raw_tag_or_eof() {
            let value = match tag & 7 {
                0 => input.read_raw_varint64().ok()?.to_le_bytes().to_vec(),
                1 => input.read_raw_little_endian64().ok()?.to_le_bytes().to_vec(),
                2 => input.read_bytes().ok()?,
                _ => return None,
            };
            if tag >> 3 == number {
                return Some(value);
            }
        }
        None
    }

    #[test]
    fn a_finished_play_carries_its_uris_reasons_and_time() {
        let message = raw_core_stream(&play(), Some(&audio()), &[1; 16], &[2; 16]);
        assert_eq!(field(&message, 24).unwrap(), b"spotify:track:4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(field(&message, 23).unwrap(), b"spotify:playlist:37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(field(&message, 9).unwrap(), b"playlist");
        assert_eq!(field(&message, 10).unwrap(), b"clickrow");
        assert_eq!(field(&message, 12).unwrap(), b"trackdone");
        assert_eq!(field(&message, 14).unwrap()[..4], 212_000u32.to_le_bytes());
        // media_id — 20 байт id файла, как у настольного клиента
        assert_eq!(field(&message, 4).unwrap(), [7; 20]);
        assert_eq!(field(&message, 22).unwrap(), b"Vorbis 320 kbps");
    }

    #[test]
    fn without_a_context_the_track_is_its_own_context() {
        let message = raw_core_stream(&Play { context_uri: String::new(), ..play() }, None, &[1; 16], &[2; 16]);
        assert_eq!(field(&message, 23).unwrap(), b"spotify:track:4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(field(&message, 9).unwrap(), b"track");
        assert_eq!(source("spotify:user:someone:collection"), "collection");
    }

    #[test]
    fn only_an_uninterrupted_play_gets_a_segment() {
        assert!(play().one_segment());
        let segment = raw_core_stream_segment(&play(), Some(&audio()), &[1; 16], &[2; 16]);
        assert_eq!(field(&segment, 4).unwrap()[..4], 212_000u32.to_le_bytes());
        assert_eq!(field(&segment, 18).unwrap()[0], 1);
        // пропуск, пауза в середине или неизвестная длина — отрезков не знаем
        assert!(!Play { reason_end: reason_end(false), ..play() }.one_segment());
        assert!(!Play { ms_played: 150_000, ..play() }.one_segment());
        assert!(!Play { duration_ms: 0, ..play() }.one_segment());
    }

    #[test]
    fn each_event_name_counts_its_own_sequence() {
        let a = next_sequence("TestEventA");
        assert_eq!(next_sequence("TestEventA"), a + 1);
        assert_eq!(next_sequence("TestEventB"), 1);
    }

    #[test]
    fn the_history_entry_names_the_song_and_when_it_started() {
        let request = play_history_revision("spotify:track:a", 1_790_000_000_250);
        assert_eq!(field(&request, 2).unwrap(), b"spotify:list:play-history:v1");
        let revision = field(&request, 4).unwrap();
        assert_eq!(field(&field(&revision, 2).unwrap(), 6).unwrap(), b"spotify:track:a");
        let time = field(&revision, 3).unwrap();
        assert_eq!(field(&time, 1).unwrap()[..8], 1_790_000_000i64.to_le_bytes());
        assert_eq!(field(&time, 2).unwrap()[..4], 250_000_000u32.to_le_bytes());
    }

    #[test]
    fn rejected_events_are_read_from_the_response() {
        // error { index: 0, transient: true, reason: 7 }
        let body = [0x0a, 0x06, 0x08, 0x00, 0x10, 0x01, 0x18, 0x07];
        assert_eq!(response_errors(&body), [(0, true, 7)]);
        assert!(response_errors(&[]).is_empty());
    }
}


