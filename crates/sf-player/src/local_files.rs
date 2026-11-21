//! Локальные файлы (mp3, flac, ogg/vorbis): своя маленькая очередь и свой поток декодирования, отдельно от
//! очереди Spotify и Connect. Звук идёт через тот же вывод, что у librespot (RodioSink за Tapped: эквалайзер,
//! лимитер, громкость на выходе, устройство из настроек), но со своим AudioControl.
//!
//! Поток один: write() блокируется, пока очередь вывода полна, и это задаёт темп декодирования. Команды
//! проверяются между кусками (~23 мс), на паузе поток спит в recv().
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use librespot_playback::audio_backend::Sink;
use librespot_playback::config::VolumeCtrl;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::softmixer::SoftMixer;
use librespot_playback::mixer::{Mixer, MixerConfig};
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use mf_local::LocalTrack;
use rodio::Source;
use tokio::sync::watch;

use crate::engine::eq;
use crate::engine::resample::Resampler;
use crate::engine::sink::{AudioControl, ErrorHook, RodioSink};
use crate::engine::vis::{AudioTap, Tapped};

/// Кадров исходника за одну запись: ~23 мс при 44,1 кГц
const CHUNK_FRAMES: usize = 1024;
/// Как часто позиция уходит в снимок во время игры (как position_update_interval движка)
const POSITION_EVERY: Duration = Duration::from_millis(200);

/// Состояние для интерфейса и медиапанели.
#[derive(Clone, Debug, Default)]
pub struct FileSnapshot {
    pub queue: Arc<Vec<LocalTrack>>,
    /// трек очереди на экране; None: локально ничего не выбрано (тогда плеер показывает Spotify)
    pub index: Option<usize>,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// 0..1
    pub volume: f64,
    /// "MP3" | "FLAC" | "OGG"
    pub codec: String,
    /// последняя ошибка (файл не открылся, нет вывода); "" значит нет
    pub error: String,
}

impl FileSnapshot {
    pub fn track(&self) -> Option<&LocalTrack> {
        self.queue.get(self.index?)
    }
}

pub struct FileConfig {
    /// громкость 0..1 из настроек
    pub volume: f64,
    /// Settings.audioDevice; None: устройство по умолчанию
    pub audio_device: Option<String>,
    pub buffer_ms: u32,
}

enum Command {
    PlayQueue(Vec<LocalTrack>, usize),
    Play,
    Pause,
    TogglePlay,
    Next,
    Previous,
    Seek(i64),
    SeekBy(i64),
    Volume(f64),
    Device(Option<String>),
    Stop,
    Quit(mpsc::Sender<()>),
}

/// Ручка плеера файлов. Clone дешёвый; команды не блокируют.
#[derive(Clone)]
pub struct FilePlayer {
    tx: mpsc::Sender<Command>,
    state: watch::Receiver<FileSnapshot>,
}

impl FilePlayer {
    pub fn start(config: FileConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let (state_tx, state) = watch::channel(FileSnapshot { volume: config.volume, ..FileSnapshot::default() });
        let spawned = std::thread::Builder::new()
            .name("local-files".into())
            .spawn(move || Worker::new(rx, state_tx, config).run());
        if let Err(error) = spawned {
            tracing::warn!("local file playback unavailable: {error}");
        }
        Self { tx, state }
    }

    pub fn snapshot(&self) -> FileSnapshot {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<FileSnapshot> {
        self.state.clone()
    }

    fn send(&self, command: Command) {
        // поток не поднялся или уже вышел: команды просто некому выполнять
        let _ = self.tx.send(command);
    }

    /// Заменить очередь и играть с `index`.
    pub fn play_queue(&self, tracks: Vec<LocalTrack>, index: usize) {
        self.send(Command::PlayQueue(tracks, index));
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn toggle_play(&self) {
        self.send(Command::TogglePlay);
    }

    pub fn next(&self) {
        self.send(Command::Next);
    }

    /// После 3 с в начало трека, иначе предыдущий.
    pub fn previous(&self) {
        self.send(Command::Previous);
    }

    pub fn seek_ms(&self, ms: i64) {
        self.send(Command::Seek(ms));
    }

    pub fn seek_relative(&self, delta_ms: i64) {
        self.send(Command::SeekBy(delta_ms));
    }

    pub fn set_volume(&self, volume: f64) {
        self.send(Command::Volume(volume));
    }

    pub fn set_output_device(&self, device: Option<String>) {
        self.send(Command::Device(device));
    }

    /// Остановить и забыть очередь: плеер снова показывает Spotify.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// При выходе: увести звук и закрыть устройство, ждать поток не дольше `timeout`.
    pub fn shutdown(&self, timeout: Duration) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Command::Quit(done)).is_ok() {
            let _ = wait.recv_timeout(timeout);
        }
    }
}

struct Worker {
    rx: mpsc::Receiver<Command>,
    state: watch::Sender<FileSnapshot>,
    output: Output,
    decoding: Option<Decoding>,
    published: Instant,
}

impl Worker {
    fn new(rx: mpsc::Receiver<Command>, state: watch::Sender<FileSnapshot>, config: FileConfig) -> Self {
        let output = Output::new(config.audio_device, config.buffer_ms);
        output.mixer.set_volume(crate::volume_to_u16(config.volume));
        Self { rx, state, output, decoding: None, published: Instant::now() }
    }

    fn playing(&self) -> bool {
        self.state.borrow().playing
    }

    fn run(mut self) {
        loop {
            let command = if self.decoding.is_some() && self.playing() {
                match self.rx.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            } else {
                match self.rx.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                }
            };
            match command {
                Some(Command::Quit(done)) => {
                    self.output.cut();
                    drop(self);
                    let _ = done.send(());
                    return;
                }
                Some(command) => self.apply(command),
                None => self.feed(),
            }
        }
        self.output.cut();
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::PlayQueue(tracks, index) => {
                self.state.send_modify(|s| s.queue = Arc::new(tracks));
                self.output.cut();
                self.open(index);
            }
            Command::Play => self.resume(),
            Command::Pause => self.pause(),
            Command::TogglePlay if self.playing() => self.pause(),
            Command::TogglePlay => self.resume(),
            Command::Next => self.skip(1),
            Command::Previous if self.position_ms() > crate::RESTART_BEFORE_PREVIOUS_MS => self.seek(0),
            Command::Previous => self.skip(-1),
            Command::Seek(ms) => self.seek(ms),
            Command::SeekBy(delta) => self.seek(self.position_ms() + delta),
            Command::Volume(volume) => {
                let volume = volume.clamp(0.0, 1.0);
                self.output.mixer.set_volume(crate::volume_to_u16(volume));
                self.state.send_if_modified(|s| std::mem::replace(&mut s.volume, volume) != volume);
            }
            Command::Device(device) => self.output.switch_device(device, self.playing()),
            Command::Stop => {
                self.output.cut();
                self.output.stop();
                self.decoding = None;
                let volume = self.state.borrow().volume;
                self.state.send_replace(FileSnapshot { volume, ..FileSnapshot::default() });
            }
            Command::Quit(_) => {}
        }
    }

    /// Открыть трек `index` и играть. Файл, который не открылся (удалён после обхода, битый), пропускается.
    fn open(&mut self, index: usize) {
        let queue = Arc::clone(&self.state.borrow().queue);
        let mut error = String::new();
        for (index, track) in queue.iter().enumerate().skip(index) {
            match Decoding::open(Path::new(&track.path)) {
                Ok(decoding) => {
                    let duration_ms = decoding.duration_ms.unwrap_or(i64::from(track.duration_ms));
                    let codec = codec_label(&track.path);
                    self.decoding = Some(decoding);
                    self.state.send_modify(|s| {
                        s.index = Some(index);
                        s.playing = true;
                        s.position_ms = 0;
                        s.duration_ms = duration_ms;
                        s.codec = codec;
                        s.error = std::mem::take(&mut error);
                    });
                    self.output.start();
                    return;
                }
                Err(reason) => {
                    tracing::warn!("local file not played {}: {reason}", track.path);
                    error = mf_i18n::t("Couldn't play this file");
                }
            }
        }
        // открыть нечего: локально ничего не выбрано, плеер вернётся к Spotify, ошибку покажет страница
        self.decoding = None;
        self.output.stop();
        self.state.send_modify(|s| {
            s.index = None;
            s.playing = false;
            s.position_ms = 0;
            if !error.is_empty() {
                s.error = error;
            }
        });
    }

    fn skip(&mut self, step: isize) {
        let (index, len) = {
            let s = self.state.borrow();
            (s.index, s.queue.len())
        };
        match index.and_then(|index| neighbour(index, len, step)) {
            Some(target) => {
                self.output.cut();
                self.open(target);
            }
            // «назад» с первого трека: в его начало
            None if step < 0 => self.seek(0),
            None => {}
        }
    }

    fn resume(&mut self) {
        if self.playing() {
            return;
        }
        if self.decoding.is_none() {
            // очередь доиграна или трек не открылся: играть выбранный заново
            let index = self.state.borrow().index;
            if let Some(index) = index {
                self.open(index);
            }
            return;
        }
        self.output.start();
        self.state.send_modify(|s| s.playing = true);
    }

    fn pause(&mut self) {
        if !self.playing() {
            return;
        }
        // Sink::stop доигрывает очередь вывода (~200 мс) с затуханием: кнопка меняется сразу, позиция после него
        self.state.send_modify(|s| s.playing = false);
        self.output.stop();
        let position_ms = self.position_ms();
        self.state.send_modify(|s| s.position_ms = position_ms);
    }

    fn seek(&mut self, ms: i64) {
        let duration_ms = self.state.borrow().duration_ms;
        let Some(decoding) = self.decoding.as_mut() else { return };
        let target = if duration_ms > 0 { ms.clamp(0, duration_ms) } else { ms.max(0) };
        self.output.cut();
        if !decoding.seek(target) {
            tracing::debug!("local file seek to {target} ms failed");
        }
        let position_ms = decoding.position_ms();
        self.state.send_modify(|s| s.position_ms = position_ms);
    }

    /// Позиция того, что слышно: прочитанное минус то, что ещё ждёт в выводе. После перемотки очередь пуста,
    /// и раньше цели слышно быть не может, иначе задержка устройства тянула бы позицию назад.
    fn position_ms(&self) -> i64 {
        let Some(decoding) = &self.decoding else {
            return self.state.borrow().position_ms;
        };
        (decoding.position_ms() - i64::from(self.output.audio.backlog_ms())).max(decoding.start_ms)
    }

    fn feed(&mut self) {
        let Some(decoding) = self.decoding.as_mut() else { return };
        let Some(samples) = decoding.next_chunk() else {
            self.track_ended();
            return;
        };
        if !samples.is_empty()
            && let Err(error) = self.output.write(samples)
        {
            self.decoding = None;
            self.output.stop();
            self.state.send_modify(|s| {
                s.playing = false;
                s.error = error;
            });
            return;
        }
        if self.published.elapsed() >= POSITION_EVERY {
            self.published = Instant::now();
            let position_ms = self.position_ms();
            self.state.send_modify(|s| s.position_ms = position_ms);
        }
    }

    /// Трек дочитан: следующий без паузы (хвост прошлого ещё играет из очереди вывода) или конец очереди.
    fn track_ended(&mut self) {
        let (index, len) = {
            let s = self.state.borrow();
            (s.index, s.queue.len())
        };
        match index.and_then(|index| neighbour(index, len, 1)) {
            Some(next) => self.open(next),
            None => {
                self.output.stop();
                self.decoding = None;
                self.state.send_modify(|s| {
                    s.playing = false;
                    s.position_ms = 0;
                });
            }
        }
    }
}

/// Соседний трек очереди или None за её краем.
fn neighbour(index: usize, len: usize, step: isize) -> Option<usize> {
    index.checked_add_signed(step).filter(|&next| next < len)
}

fn codec_label(path: &str) -> String {
    let ext = Path::new(path).extension().and_then(|ext| ext.to_str()).unwrap_or_default().to_ascii_uppercase();
    if ext == "OGA" { "OGG".into() } else { ext }
}

/// Вывод: тот же RodioSink, что у librespot, за тем же Tapped.
struct Output {
    sink: Box<dyn Sink>,
    audio: Arc<AudioControl>,
    converter: Converter,
    mixer: Arc<SoftMixer>,
    device: Option<String>,
    buffer_ms: u32,
    /// текст сбоя вывода от RodioSink (английский; переводится как у движка)
    failure: Arc<Mutex<Option<String>>>,
    started: bool,
    /// в очередь вывода что-то писали с последнего cut/stop
    fed: bool,
}

impl Output {
    fn new(device: Option<String>, buffer_ms: u32) -> Self {
        // та же кривая громкости, что у движка (player.rs): одна громкость приложения звучит одинаково
        let volume_ctrl = VolumeCtrl::Cubic(VolumeCtrl::DEFAULT_DB_RANGE);
        let config = MixerConfig { volume_ctrl, ..MixerConfig::default() };
        let mixer = Arc::new(SoftMixer::open(config).expect("softvol opens without a device"));
        let audio = AudioControl::new(buffer_ms);
        let failure = Arc::default();
        let sink = make_sink(device.clone(), buffer_ms, &mixer, &audio, &failure);
        let converter = Converter::new(None);
        Self { sink, audio, converter, mixer, device, buffer_ms, failure, started: false, fed: false }
    }

    fn start(&mut self) {
        if !self.started {
            // RodioSink::start не падает: нет устройства, и ошибка придёт с первым write
            let _ = self.sink.start();
            self.started = true;
        }
    }

    /// Доиграть очередь вывода с затуханием и отпустить устройство.
    fn stop(&mut self) {
        if self.started {
            let _ = self.sink.stop();
            self.started = false;
        }
        self.fed = false;
    }

    /// Быстро увести и выбросить то, что уже в очереди (смена трека, перемотка).
    fn cut(&mut self) {
        if self.started && self.fed {
            self.audio.interrupt();
            // писатель один (этот поток), старого декодера, от которого защищает ворота, нет
            self.audio.track_changed();
        }
        self.fed = false;
    }

    fn write(&mut self, samples: Vec<f64>) -> Result<(), String> {
        self.fed = true;
        self.sink.write(AudioPacket::Samples(samples), &mut self.converter).map_err(|error| {
            let raw = self.failure.lock().unwrap_or_else(PoisonError::into_inner).take();
            raw.map_or_else(|| error.to_string(), |raw| crate::state::core::engine_error_text(&raw))
        })
    }

    fn switch_device(&mut self, device: Option<String>, playing: bool) {
        if device == self.device {
            return;
        }
        self.cut();
        self.stop();
        self.audio = AudioControl::new(self.buffer_ms);
        self.sink = make_sink(device.clone(), self.buffer_ms, &self.mixer, &self.audio, &self.failure);
        self.device = device;
        if playing {
            self.start();
        }
    }
}

fn make_sink(
    device: Option<String>,
    buffer_ms: u32,
    mixer: &Arc<SoftMixer>,
    audio: &Arc<AudioControl>,
    failure: &Arc<Mutex<Option<String>>>,
) -> Box<dyn Sink> {
    let failure = Arc::clone(failure);
    let report: ErrorHook = Arc::new(move |message: String| {
        *failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(message);
    });
    let rodio = RodioSink::new(device, report, mixer.get_soft_volume(), buffer_ms, Arc::clone(audio));
    let unity = Arc::new(AtomicU64::new(1.0f64.to_bits()));
    // уровень для «дыхания» обложки не считается: tap никто не включает
    let ceiling = mixer.get_soft_volume();
    Box::new(Tapped::new(Box::new(rodio), Arc::clone(audio), AudioTap::new(), ceiling, false, eq::shared(), unity))
}

/// Декодер одного файла, приведённый к формату вывода: стерео 44,1 кГц.
struct Decoding {
    decoder: rodio::Decoder<BufReader<File>>,
    channels: usize,
    rate: u32,
    resampler: Option<Resampler>,
    /// кадров исходника прочитано от начала трека
    frame: u64,
    /// откуда играет: 0 или цель последней перемотки
    start_ms: i64,
    duration_ms: Option<i64>,
}

impl Decoding {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let decoder = rodio::Decoder::try_from(file).map_err(|error| error.to_string())?;
        let channels = usize::from(decoder.channels());
        let rate = decoder.sample_rate();
        if channels == 0 || rate == 0 {
            return Err("no audio stream".into());
        }
        let duration_ms = decoder.total_duration().and_then(|d| i64::try_from(d.as_millis()).ok());
        Ok(Self {
            decoder,
            channels,
            rate,
            resampler: Resampler::new(rate, SAMPLE_RATE, NUM_CHANNELS as usize),
            frame: 0,
            start_ms: 0,
            duration_ms,
        })
    }

    fn position_ms(&self) -> i64 {
        i64::try_from(self.frame * 1000 / u64::from(self.rate)).unwrap_or(i64::MAX)
    }

    /// Следующий кусок в f64 для Tapped; None: трек кончился. Пустой кусок: ресемплер копит вход.
    fn next_chunk(&mut self) -> Option<Vec<f64>> {
        let stereo = read_stereo(&mut self.decoder, self.channels, CHUNK_FRAMES);
        if stereo.is_empty() {
            return None;
        }
        self.frame += (stereo.len() / 2) as u64;
        let stereo = match &mut self.resampler {
            Some(resampler) => resampler.process(&stereo),
            None => stereo,
        };
        Some(stereo.into_iter().map(f64::from).collect())
    }

    fn seek(&mut self, ms: i64) -> bool {
        let target = Duration::from_millis(u64::try_from(ms).unwrap_or(0));
        if self.decoder.try_seek(target).is_err() {
            return false;
        }
        self.frame = target.as_millis() as u64 * u64::from(self.rate) / 1000;
        self.start_ms = ms.max(0);
        // хвост фильтра со старой позиции не должен попасть в новую
        self.resampler = Resampler::new(self.rate, SAMPLE_RATE, NUM_CHANNELS as usize);
        true
    }
}

/// До `frames` кадров стерео из чередующихся `channels` каналов: моно удваивается, из многоканального
/// берутся передние левый и правый.
// shortcut: 5.1 без центра и тыла теряет часть звука; сводить честно, если будут жалобы на многоканальные файлы
// shortcut: формат берётся при открытии; цепочка ogg с другой частотой посередине зазвучит не с той скоростью
fn read_stereo(samples: &mut impl Iterator<Item = f32>, channels: usize, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        let Some(left) = samples.next() else { break };
        let mut right = left;
        for channel in 1..channels {
            let sample = samples.next().unwrap_or(0.0);
            if channel == 1 {
                right = sample;
            }
        }
        out.push(left);
        out.push(right);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_doubled_and_extra_channels_are_dropped() {
        let mono = read_stereo(&mut [0.1, 0.2, 0.3].into_iter(), 1, 8);
        assert_eq!(mono, [0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
        let surround = read_stereo(&mut [1.0, 2.0, 3.0, 4.0, 5.0, 6.0].into_iter(), 3, 8);
        assert_eq!(surround, [1.0, 2.0, 4.0, 5.0]);
        let limited = read_stereo(&mut [1.0f32; 100].into_iter(), 2, 4);
        assert_eq!(limited.len(), 8);
        assert!(read_stereo(&mut std::iter::empty(), 2, 4).is_empty());
    }

    #[test]
    fn next_and_previous_stop_at_the_ends_of_the_queue() {
        assert_eq!(neighbour(0, 3, 1), Some(1));
        assert_eq!(neighbour(2, 3, 1), None);
        assert_eq!(neighbour(0, 3, -1), None);
        assert_eq!(neighbour(2, 3, -1), Some(1));
        assert_eq!(neighbour(0, 0, 1), None);
    }

    #[test]
    fn codec_follows_the_extension() {
        assert_eq!(codec_label("/m/a.flac"), "FLAC");
        assert_eq!(codec_label("/m/a.Mp3"), "MP3");
        assert_eq!(codec_label("/m/a.oga"), "OGG");
    }
}
