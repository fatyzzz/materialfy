//! Системная панель «Сейчас играет» и медиаклавиши через fastframe-now-playing: SMTC на Windows, Now Playing на macOS,
//! MPRIS на Linux (org.mpris.MediaPlayer2.materialfy). Свой поток: туда приходят снимки плеера (из задачи tokio)
//! и пробуждения «есть команды»; команды уходят в PlayerHandle, Raise/Quit — в AppRequest.
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fastframe_now_playing::{App, Command, Controls, NowPlaying, Playback, Repeat as NpRepeat, State, Track};
use sf_player::{PlayerHandle, PlayerSnapshot, Repeat};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use crate::AppRequest;

pub const BUS_NAME: &str = "materialfy";
pub const IDENTITY: &str = "Materialfy";
pub const DESKTOP_ENTRY: &str = "materialfy";
/// расхождение позиции, которое считается перемоткой, а не игрой
const SEEK_JUMP_MS: i64 = 2000;

pub enum Msg {
    Snapshot(Box<PlayerSnapshot>),
    Commands,
    Stop,
}

/// Ручка потока медиаклавиш.
pub struct Media {
    tx: mpsc::Sender<Msg>,
    done: mpsc::Receiver<()>,
}

impl Media {
    pub fn stop(self, timeout: Duration) {
        let _ = self.tx.send(Msg::Stop);
        let _ = self.done.recv_timeout(timeout);
    }
}

fn repeat_to(repeat: Repeat) -> NpRepeat {
    match repeat {
        Repeat::Off => NpRepeat::Off,
        Repeat::All => NpRepeat::Playlist,
        Repeat::One => NpRepeat::Track,
    }
}

fn repeat_from(repeat: NpRepeat) -> Repeat {
    match repeat {
        NpRepeat::Off => Repeat::Off,
        NpRepeat::Playlist => Repeat::All,
        NpRepeat::Track => Repeat::One,
    }
}

/// Что показать системе по снимку плеера.
pub fn state_from(s: &PlayerSnapshot) -> State {
    let track = s.track.as_ref().map(|t| {
        let artists = if t.artist_refs.is_empty() {
            t.artists.split(", ").filter(|a| !a.is_empty()).map(str::to_owned).collect()
        } else {
            t.artist_refs.iter().map(|a| a.name.clone()).collect()
        };
        let art_url = if t.cover_large.is_empty() { &t.cover } else { &t.cover_large };
        Track {
            id: t.play_uri(),
            title: t.title.clone(),
            artists,
            album: t.album.clone(),
            duration: (s.duration_ms > 0).then(|| Duration::from_millis(s.duration_ms as u64)),
            art_file: (!s.cover_file.is_empty()).then(|| PathBuf::from(&s.cover_file)),
            art_url: (!art_url.is_empty()).then(|| art_url.clone()),
            url: Some(t.play_uri()),
            ..Track::default()
        }
    });
    let has_track = track.is_some();
    State {
        playback: match (has_track, s.playing) {
            (false, _) => Playback::Stopped,
            (true, true) => Playback::Playing,
            (true, false) => Playback::Paused,
        },
        track,
        position: Duration::from_millis(s.position_ms.max(0) as u64),
        volume: s.can_set_volume.then_some(s.volume.clamp(0.0, 1.0)),
        shuffle: Some(s.shuffle),
        repeat: Some(repeat_to(s.repeat)),
        controls: Controls {
            play: has_track,
            pause: has_track,
            next: has_track,
            previous: has_track,
            seek: has_track && s.duration_ms > 0,
        },
    }
}

fn track_id(s: &PlayerSnapshot) -> Option<String> {
    s.track.as_ref().map(|t| t.play_uri())
}

/// Позиция прыгнула не из-за игры (перемотка) — системе нужно сказать об этом отдельно.
pub fn jumped(prev: &PlayerSnapshot, elapsed: Duration, now: &PlayerSnapshot) -> bool {
    if track_id(prev) != track_id(now) || now.track.is_none() {
        return false;
    }
    let expected = prev.position_ms + if prev.playing { elapsed.as_millis() as i64 } else { 0 };
    (now.position_ms - expected).abs() > SEEK_JUMP_MS
}

/// Команда системы → плеер / оболочка.
pub fn apply(command: Command, current: Option<&PlayerSnapshot>, player: &PlayerHandle, requests: &broadcast::Sender<AppRequest>) {
    match command {
        Command::Play => player.play(),
        Command::Pause | Command::Stop => player.pause(),
        Command::PlayPause => player.toggle_play(),
        Command::Next => player.next(),
        Command::Previous => player.previous(),
        Command::SeekBy(ms) => player.seek_relative(ms),
        Command::SetPosition { track_id: id, position } => {
            if current.and_then(track_id).as_deref() == Some(id.as_str()) {
                player.seek_ms(position.as_millis() as i64);
            }
        }
        Command::SetVolume(volume) => player.set_volume(volume.clamp(0.0, 1.0)),
        Command::SetShuffle(on) => player.set_shuffle(on),
        Command::SetRepeat(repeat) => player.set_repeat(repeat_from(repeat)),
        Command::OpenUri(_) => {}
        Command::Raise => drop(requests.send(AppRequest::RaiseWindow)),
        Command::Quit => drop(requests.send(AppRequest::Quit)),
    }
}

/// Запустить поток медиаклавиш. None — поток не создался.
pub fn start(rt: &Handle, player: PlayerHandle, requests: broadcast::Sender<AppRequest>) -> Option<Media> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let (done_tx, done_rx) = mpsc::channel();
    let wake = tx.clone();
    let commands_player = player.clone();
    let spawned = std::thread::Builder::new().name("now-playing".into()).spawn(move || {
        let app = App {
            desktop_entry: fastframe_now_playing::desktop_entry(DESKTOP_ENTRY),
            ..App::new(BUS_NAME, IDENTITY)
        };
        let mut controls = NowPlaying::start(app, move || {
            let _ = wake.send(Msg::Commands);
        });
        let mut last: Option<(PlayerSnapshot, Instant)> = None;
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Snapshot(snapshot) => {
                    if let Some((prev, at)) = &last
                        && jumped(prev, at.elapsed(), &snapshot)
                    {
                        controls.seeked(Duration::from_millis(snapshot.position_ms.max(0) as u64));
                    }
                    controls.update(state_from(&snapshot));
                    last = Some((*snapshot, Instant::now()));
                }
                Msg::Commands => {
                    for command in controls.commands() {
                        apply(command, last.as_ref().map(|(s, _)| s), &commands_player, &requests);
                    }
                }
                Msg::Stop => break,
            }
        }
        drop(controls);
        let _ = done_tx.send(());
    });
    if let Err(e) = spawned {
        tracing::warn!("media keys unavailable: {e}");
        return None;
    }
    let mut snapshots = player.subscribe().snapshot;
    let forward = tx.clone();
    rt.spawn(async move {
        let first = snapshots.borrow_and_update().clone();
        if forward.send(Msg::Snapshot(Box::new(first))).is_err() {
            return;
        }
        while snapshots.changed().await.is_ok() {
            let snapshot = snapshots.borrow_and_update().clone();
            if forward.send(Msg::Snapshot(Box::new(snapshot))).is_err() {
                return;
            }
        }
    });
    Some(Media { tx, done: done_rx })
}

#[cfg(test)]
mod tests {
    use md3_model::{ArtistRef, TrackItem};

    use super::*;

    fn snapshot() -> PlayerSnapshot {
        PlayerSnapshot {
            track: Some(TrackItem {
                track_id: "1".into(),
                uri: "spotify:track:1".into(),
                title: "Песня".into(),
                artists: "А, Б".into(),
                artist_refs: vec![ArtistRef { id: "a".into(), name: "А".into() }, ArtistRef { id: "b".into(), name: "Б".into() }],
                album: "Альбом".into(),
                cover: "https://i/300".into(),
                cover_large: "https://i/640".into(),
                ..Default::default()
            }),
            playing: true,
            duration_ms: 200_000,
            position_ms: 10_000,
            volume: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn state_follows_the_snapshot() {
        let mut s = snapshot();
        let state = state_from(&s);
        assert_eq!(state.playback, Playback::Playing);
        let track = state.track.as_ref().unwrap();
        assert_eq!(track.id, "spotify:track:1");
        assert_eq!(track.artists, ["А", "Б"]);
        assert_eq!(track.art_url.as_deref(), Some("https://i/640"));
        assert_eq!(track.art_file, None);
        assert_eq!(track.duration, Some(Duration::from_secs(200)));
        assert_eq!(state.position, Duration::from_secs(10));
        // громкость удалённого устройства без поддержки — не предлагать
        assert_eq!(state.volume, None);
        s.can_set_volume = true;
        s.cover_file = "C:/cache/c.jpg".into();
        s.playing = false;
        s.repeat = Repeat::One;
        let state = state_from(&s);
        assert_eq!(state.volume, Some(0.5));
        assert_eq!(state.playback, Playback::Paused);
        assert_eq!(state.repeat, Some(NpRepeat::Track));
        assert_eq!(state.track.unwrap().art_file, Some(PathBuf::from("C:/cache/c.jpg")));
        let empty = state_from(&PlayerSnapshot::default());
        assert_eq!(empty.playback, Playback::Stopped);
        assert!(!empty.controls.play && !empty.controls.seek);
    }

    #[test]
    fn seeks_are_told_apart_from_playing() {
        let prev = snapshot();
        let mut now = snapshot();
        now.position_ms = 11_000;
        assert!(!jumped(&prev, Duration::from_secs(1), &now));
        now.position_ms = 60_000;
        assert!(jumped(&prev, Duration::from_secs(1), &now));
        now.track.as_mut().unwrap().uri = "spotify:track:2".into();
        assert!(!jumped(&prev, Duration::from_secs(1), &now));
    }

    #[test]
    fn repeat_round_trips() {
        for r in [Repeat::Off, Repeat::All, Repeat::One] {
            assert_eq!(repeat_from(repeat_to(r)), r);
        }
    }
}
