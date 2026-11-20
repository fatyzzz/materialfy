//! Нижний слой sf-player (исполнитель «engine»): librespot Engine, свой sink на fastframe-audio, EQ/лимитер,
//! tap уровня, чтение через сессию, zeroconf, обложка в файл. Наружу — через PlayerHandle (верхний слой)
//! и прямые вызовы session_reads/zeroconf/sink::output_devices из sf-services.
pub mod cover;
pub mod eq;
pub mod fx;
pub mod limiter;
/// «Новинки» через сессию: «Радар новинок», подписки, релизы исполнителей
pub mod new_releases;
pub mod player;
/// отчёт о прослушивании в Spotify (gabo RawCoreStream), по настройке reportPlays
pub mod play_report;
pub mod resample;
/// коллекция аккаунта через сессию: любимые треки, альбомы, подписки (collection v2)
pub mod session_collection;
/// плейлисты через сессию: список из rootlist, правки (playlist4 changes), создание, подписка, обложка
pub mod session_playlists;
/// подкасты через сессию: шоу и выпуски (metadata), сохранённые шоу и «Ваши выпуски»
pub mod session_podcasts;
pub mod session_reads;
pub mod sink;
pub mod stretch;
pub mod vis;
pub mod zeroconf;

pub use player::{
    Engine, EngineConfig, EngineEvent, Interrupted, LoadSpec, LocalState, LocalTrack, Notify, Playback,
    PlaybackResume, PlayerCommand, RepeatMode, Rootlist, RootlistEntry,
};
