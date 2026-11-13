//! Spoken lines around tracks, as Spotify's DJ plays them.
//!
//! A DJ context gives some of its tracks up to three SSML scripts in their
//! metadata: `narration.intro.*` to say before the track when playback
//! reaches it, `narration.jump.*` to say before it instead when the
//! listener skips to it, and `narration.outro.*` to say after it ends. The
//! playing device voices them itself through Spotify's text-to-speech
//! service, which hands back a short MP3 clip.

use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures_util::FutureExt;
use librespot_protocol::{
    client_tts::TtsRequest,
    tts_resolve::resolve_request::{AudioFormat, TtsProvider, TtsVoice},
};
use protobuf::Enum;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};
use tokio::task::JoinHandle;

use crate::{
    NUM_CHANNELS, SAMPLE_RATE,
    core::{Error, Session},
};

/// How long a clip may take to arrive before the track plays without it.
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(10);

/// Language for a script that doesn't name one.
const DEFAULT_LANGUAGE: &str = "en-US";

/// One spoken line, as the track metadata describes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NarrationScript {
    /// What to say, as SSML.
    pub ssml: String,
    /// The voice to say it in, such as `VOICE1`.
    pub voice: String,
    /// The speech engine, such as `SONANTIC_FAST`.
    pub provider: String,
    /// The language, such as `en-US`, read from the SSML.
    pub language: String,
    /// The sample rate the clip is rendered at.
    pub sample_rate: u32,
}

impl NarrationScript {
    /// Reads the script stored under `narration.<part>.*`, where `part` is
    /// `intro`, `jump` or `outro`. `None` without SSML to say.
    pub fn from_metadata(metadata: &HashMap<String, String>, part: &str) -> Option<Self> {
        let field = |name: &str| metadata.get(&format!("narration.{part}.{name}"));
        let ssml = field("ssml")
            .filter(|ssml| !ssml.trim().is_empty())?
            .clone();
        let language = ssml_language(&ssml).unwrap_or(DEFAULT_LANGUAGE).to_owned();
        Some(Self {
            voice: field("voice").cloned().unwrap_or_default(),
            provider: field("tts_provider").cloned().unwrap_or_default(),
            sample_rate: field("sample_rate")
                .and_then(|rate| rate.parse().ok())
                .filter(|rate| *rate > 0)
                .unwrap_or(SAMPLE_RATE),
            language,
            ssml,
        })
    }

    /// The request for Spotify's text-to-speech service.
    pub fn request(&self) -> TtsRequest {
        let mut request = TtsRequest::new();
        request.set_ssml(self.ssml.clone());
        request.audio_format = AudioFormat::MP3.into();
        request.language = self.language.clone();
        request.tts_voice = TtsVoice::from_str(&self.voice)
            .unwrap_or(TtsVoice::UNSET_TTS_VOICE)
            .into();
        request.tts_provider = TtsProvider::from_str(&self.provider)
            .unwrap_or(TtsProvider::UNSET_TTS_PROVIDER)
            .into();
        request.sample_rate_hz = self.sample_rate as i32;
        request
    }
}

/// The `xml:lang` of an SSML document's root.
fn ssml_language(ssml: &str) -> Option<&str> {
    let start = ssml.find("xml:lang=")? + "xml:lang=".len();
    let rest = &ssml[start..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    let language = &rest[..rest.find(quote)?];
    (!language.is_empty()).then_some(language)
}

/// What to say around one play of a track.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Narration {
    /// Said before the track starts.
    pub before: Option<NarrationScript>,
    /// Said after the track plays to its end.
    pub after: Option<NarrationScript>,
}

impl Narration {
    /// The narration for a track from its metadata. Nothing is said before
    /// a track that starts part way through; `jumped` picks the line for a
    /// track the listener skipped to, when the track has one.
    pub fn for_track(metadata: &HashMap<String, String>, position_ms: u32, jumped: bool) -> Self {
        let before = if position_ms > 0 {
            None
        } else if jumped {
            NarrationScript::from_metadata(metadata, "jump")
                .or_else(|| NarrationScript::from_metadata(metadata, "intro"))
        } else {
            NarrationScript::from_metadata(metadata, "intro")
        };
        Self {
            before,
            after: NarrationScript::from_metadata(metadata, "outro"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.before.is_none() && self.after.is_none()
    }

    fn scripts(&self) -> impl Iterator<Item = &NarrationScript> {
        self.before.iter().chain(self.after.iter())
    }
}

/// A voiced line, ready for the sink: interleaved stereo at the player's
/// sample rate.
#[derive(Debug)]
pub(crate) struct Clip {
    pub samples: Vec<f64>,
}

/// Voices `script` through Spotify's text-to-speech service.
pub(crate) async fn synthesize(session: Session, script: NarrationScript) -> Result<Clip, Error> {
    let url = session.spclient().get_tts_url(&script.request()).await?;
    let request = http::Request::builder()
        .method(http::Method::GET)
        .uri(url)
        .body(Bytes::new())?;
    let audio = session.http_client().request_body(request).await?;
    decode(audio)
}

/// Decodes a clip to interleaved stereo at the player's sample rate. The
/// service sends mono MP3, which the player can't take as it is.
pub(crate) fn decode(audio: Bytes) -> Result<Clip, Error> {
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(audio)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    hint.mime_type("audio/mpeg");
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| Error::failed_precondition(format!("unreadable narration clip: {e}")))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| Error::failed_precondition("narration clip has no audio"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::failed_precondition(format!("undecodable narration clip: {e}")))?;

    let mut samples = Vec::new();
    let mut channels = 0;
    let mut rate = 0;
    let mut buffer: Option<SampleBuffer<f64>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                return Err(Error::failed_precondition(format!(
                    "broken narration clip: {e}"
                )));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count();
                rate = spec.rate;
                let buffer = match buffer.as_mut() {
                    Some(buffer) if buffer.capacity() >= decoded.capacity() * channels => buffer,
                    _ => buffer.insert(SampleBuffer::new(decoded.capacity() as u64, spec)),
                };
                buffer.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buffer.samples());
            }
            Err(SymphoniaError::DecodeError(e)) => warn!("skipping a narration frame: {e}"),
            Err(e) => {
                return Err(Error::failed_precondition(format!(
                    "undecodable narration clip: {e}"
                )));
            }
        }
    }

    if samples.is_empty() || channels == 0 || rate == 0 {
        return Err(Error::failed_precondition("narration clip is silent"));
    }
    let stereo = to_stereo(&samples, channels);
    Ok(Clip {
        samples: resample(&stereo, rate, SAMPLE_RATE),
    })
}

/// Interleaved stereo from interleaved audio with `channels` channels: mono
/// goes to both sides, anything wider keeps its first two.
pub(crate) fn to_stereo(samples: &[f64], channels: usize) -> Vec<f64> {
    match channels {
        0 => Vec::new(),
        1 => samples.iter().flat_map(|s| [*s, *s]).collect(),
        2 => samples.to_vec(),
        n => samples
            .chunks_exact(n)
            .flat_map(|frame| [frame[0], frame[1]])
            .collect(),
    }
}

/// Linear resampling of interleaved stereo. Speech survives it well, and a
/// clip asked for at the player's rate never needs it.
pub(crate) fn resample(samples: &[f64], from: u32, to: u32) -> Vec<f64> {
    let channels = NUM_CHANNELS as usize;
    if from == to || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }
    let out_frames = (frames as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_frames * channels);
    for frame in 0..out_frames {
        let position = frame as f64 * step;
        let index = (position as usize).min(frames - 1);
        let next = (index + 1).min(frames - 1);
        let weight = position - index as f64;
        for channel in 0..channels {
            let a = samples[index * channels + channel];
            let b = samples[next * channels + channel];
            out.push(a + (b - a) * weight);
        }
    }
    out
}

enum Slot {
    Loading(JoinHandle<Option<Arc<Clip>>>),
    Ready(Option<Arc<Clip>>),
}

/// Clips being voiced or ready, by script. A failed clip is kept as `None`
/// so the track plays on without asking again.
#[derive(Default)]
pub(crate) struct Clips {
    slots: HashMap<NarrationScript, Slot>,
}

impl Clips {
    /// Starts voicing every line of `narration` not yet voiced.
    pub fn request(&mut self, session: &Session, narration: &Narration) {
        for script in narration.scripts() {
            if self.slots.contains_key(script) {
                continue;
            }
            let session = session.clone();
            let task_script = script.clone();
            let task = tokio::spawn(async move {
                let preview: String = task_script.ssml.chars().take(80).collect();
                match tokio::time::timeout(SYNTHESIS_TIMEOUT, synthesize(session, task_script))
                    .await
                {
                    Ok(Ok(clip)) => {
                        debug!(
                            "narration ready ({} ms): {preview}",
                            clip.samples.len() as u64 * 1000
                                / u64::from(SAMPLE_RATE * NUM_CHANNELS as u32)
                        );
                        Some(Arc::new(clip))
                    }
                    Ok(Err(e)) => {
                        warn!("couldn't voice the DJ's line, playing on without it: {e}");
                        None
                    }
                    Err(_) => {
                        warn!("the DJ's line took too long to voice, playing on without it");
                        None
                    }
                }
            });
            self.slots.insert(script.clone(), Slot::Loading(task));
        }
    }

    /// The clip for `script` once it is voiced; `None` when it couldn't be,
    /// or was never asked for.
    pub fn poll(
        &mut self,
        script: &NarrationScript,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Arc<Clip>>> {
        let Some(slot) = self.slots.get_mut(script) else {
            return Poll::Ready(None);
        };
        match slot {
            Slot::Ready(clip) => Poll::Ready(clip.clone()),
            Slot::Loading(task) => match task.poll_unpin(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => {
                    let clip = result.ok().flatten();
                    *slot = Slot::Ready(clip.clone());
                    Poll::Ready(clip)
                }
            },
        }
    }

    /// Forgets every clip not in `keep`, stopping any still being voiced.
    pub fn retain(&mut self, keep: &[&Narration]) {
        self.slots.retain(|script, slot| {
            let kept = keep.iter().any(|n| n.scripts().any(|s| s == script));
            if let (false, Slot::Loading(task)) = (kept, slot) {
                task.abort();
            }
            kept
        });
    }
}

/// Which line is being said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Before,
    After,
}

/// Where one play of a track is in its narration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The line before the track, still to come or being said.
    Before,
    /// The track itself.
    Track,
    /// The line after the track, still to come or being said.
    After,
    /// Everything has been said.
    Done,
}

/// The narration of the play the player is on.
pub(crate) struct TrackNarration {
    pub play_request_id: u64,
    pub narration: Narration,
    pub stage: Stage,
    /// The clip being said and how far into it the sink is.
    pub speaking: Option<(Arc<Clip>, usize)>,
}

impl TrackNarration {
    pub fn new(play_request_id: u64, narration: Narration) -> Self {
        let stage = if narration.before.is_some() {
            Stage::Before
        } else {
            Stage::Track
        };
        Self {
            play_request_id,
            narration,
            stage,
            speaking: None,
        }
    }

    /// The script for the current stage, if it says anything.
    pub fn script(&self) -> Option<&NarrationScript> {
        match self.stage {
            Stage::Before => self.narration.before.as_ref(),
            Stage::After => self.narration.after.as_ref(),
            Stage::Track | Stage::Done => None,
        }
    }

    pub fn part(&self) -> Option<Part> {
        match self.stage {
            Stage::Before => Some(Part::Before),
            Stage::After => Some(Part::After),
            Stage::Track | Stage::Done => None,
        }
    }

    /// Whether a line follows the track, so the next track doesn't start
    /// straight after it.
    pub fn speaks_after(&self) -> bool {
        self.narration.after.is_some() && matches!(self.stage, Stage::Before | Stage::Track)
    }

    /// Whether the narration holds the track back right now.
    pub fn active(&self) -> bool {
        matches!(self.stage, Stage::Before | Stage::After)
    }

    /// Moves past the current line.
    pub fn advance(&mut self) {
        self.speaking = None;
        self.stage = match self.stage {
            Stage::Before => Stage::Track,
            Stage::Track => {
                if self.narration.after.is_some() {
                    Stage::After
                } else {
                    Stage::Done
                }
            }
            Stage::After | Stage::Done => Stage::Done,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn dj_track() -> HashMap<String, String> {
        metadata(&[
            (
                "narration.intro.ssml",
                "<speak xml:lang=\"en-US\">Up next, something new.</speak>",
            ),
            ("narration.intro.voice", "VOICE1"),
            ("narration.intro.tts_provider", "SONANTIC_FAST"),
            ("narration.intro.sample_rate", "44100"),
            (
                "narration.jump.ssml",
                "<speak xml:lang='es-MX'>Vamos a cambiarlo.</speak>",
            ),
            ("narration.jump.voice", "VOICE3"),
            (
                "narration.outro.ssml",
                "<speak xml:lang=\"en-US\">That was it.</speak>",
            ),
        ])
    }

    #[test]
    fn reads_scripts_from_track_metadata() {
        let intro = NarrationScript::from_metadata(&dj_track(), "intro").unwrap();
        assert_eq!(intro.voice, "VOICE1");
        assert_eq!(intro.provider, "SONANTIC_FAST");
        assert_eq!(intro.language, "en-US");
        assert_eq!(intro.sample_rate, 44100);

        let jump = NarrationScript::from_metadata(&dj_track(), "jump").unwrap();
        assert_eq!(jump.language, "es-MX");
        assert_eq!(jump.provider, "");
        assert_eq!(jump.sample_rate, SAMPLE_RATE);

        assert!(NarrationScript::from_metadata(&HashMap::new(), "intro").is_none());
        let blank = metadata(&[("narration.intro.ssml", "  ")]);
        assert!(NarrationScript::from_metadata(&blank, "intro").is_none());
    }

    #[test]
    fn picks_the_line_for_how_the_track_was_reached() {
        let track = dj_track();
        let reached = Narration::for_track(&track, 0, false);
        assert!(reached.before.unwrap().ssml.contains("Up next"));
        assert!(reached.after.unwrap().ssml.contains("That was it"));

        let jumped = Narration::for_track(&track, 0, true);
        assert!(jumped.before.unwrap().ssml.contains("cambiarlo"));

        let resumed = Narration::for_track(&track, 30_000, false);
        assert!(resumed.before.is_none());
        assert!(resumed.after.is_some());

        let only_intro = metadata(&[("narration.intro.ssml", "<speak>Hi</speak>")]);
        let jumped = Narration::for_track(&only_intro, 0, true);
        assert_eq!(jumped.before.unwrap().language, DEFAULT_LANGUAGE);

        assert!(Narration::for_track(&HashMap::new(), 0, false).is_empty());
    }

    #[test]
    fn builds_the_speech_request() {
        let script = NarrationScript::from_metadata(&dj_track(), "intro").unwrap();
        let request = script.request();
        assert_eq!(request.ssml(), script.ssml);
        assert_eq!(request.audio_format.enum_value(), Ok(AudioFormat::MP3));
        assert_eq!(request.tts_voice.enum_value(), Ok(TtsVoice::VOICE1));
        assert_eq!(
            request.tts_provider.enum_value(),
            Ok(TtsProvider::SONANTIC_FAST)
        );
        assert_eq!(request.sample_rate_hz, 44100);
        assert_eq!(request.language, "en-US");

        let mut odd = script.clone();
        odd.voice = "VOICE999".into();
        odd.provider = "SOMETHING_NEW".into();
        let request = odd.request();
        assert_eq!(
            request.tts_voice.enum_value(),
            Ok(TtsVoice::UNSET_TTS_VOICE)
        );
        assert_eq!(
            request.tts_provider.enum_value(),
            Ok(TtsProvider::UNSET_TTS_PROVIDER)
        );
    }

    #[test]
    fn widens_mono_to_stereo() {
        assert_eq!(to_stereo(&[0.1, 0.2], 1), vec![0.1, 0.1, 0.2, 0.2]);
        assert_eq!(to_stereo(&[0.1, 0.2], 2), vec![0.1, 0.2]);
        assert_eq!(
            to_stereo(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 3),
            vec![0.1, 0.2, 0.4, 0.5]
        );
    }

    #[test]
    fn resamples_to_the_player_rate() {
        let input: Vec<f64> = (0..100).flat_map(|i| [i as f64, -(i as f64)]).collect();
        assert_eq!(resample(&input, 44100, 44100), input);

        let doubled = resample(&input, 22050, 44100);
        assert_eq!(doubled.len(), 400);
        assert_eq!(&doubled[..6], &[0.0, 0.0, 0.5, -0.5, 1.0, -1.0]);

        let halved = resample(&input, 48000, 24000);
        assert_eq!(halved.len(), 100);
        assert_eq!(&halved[..4], &[0.0, 0.0, 2.0, -2.0]);
    }

    #[test]
    fn walks_through_the_stages() {
        let both = Narration::for_track(&dj_track(), 0, false);
        let mut play = TrackNarration::new(7, both);
        assert_eq!(play.stage, Stage::Before);
        assert!(play.active() && play.speaks_after());
        assert!(play.script().unwrap().ssml.contains("Up next"));
        play.advance();
        assert_eq!(play.stage, Stage::Track);
        assert!(!play.active() && play.speaks_after());
        assert!(play.script().is_none());
        play.advance();
        assert_eq!(play.stage, Stage::After);
        assert_eq!(play.part(), Some(Part::After));
        assert!(!play.speaks_after());
        play.advance();
        assert_eq!(play.stage, Stage::Done);

        let mut silent = TrackNarration::new(8, Narration::default());
        assert_eq!(silent.stage, Stage::Track);
        silent.advance();
        assert_eq!(silent.stage, Stage::Done);
    }

    #[test]
    fn reads_the_language_of_ssml() {
        assert_eq!(
            ssml_language("<speak xml:lang=\"pt-BR\">Oi</speak>"),
            Some("pt-BR")
        );
        assert_eq!(
            ssml_language("<speak xml:lang='de'>Hallo</speak>"),
            Some("de")
        );
        assert_eq!(ssml_language("<speak>Hi</speak>"), None);
        assert_eq!(ssml_language("<speak xml:lang=\"\">Hi</speak>"), None);
    }
}
