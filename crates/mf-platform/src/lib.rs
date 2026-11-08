//! Платформенный слой: каталоги, настройки, системные предпочтения,
//! MPRIS (Linux) и «единственный экземпляр» (Linux — D-Bus, Windows — именованный канал, macOS — unix-сокет).
//! Секреты Spotify здесь не хранятся — это sf-auth (keyring-core).
pub mod dirs;
pub mod kwin;
pub mod mpris;
pub mod settings;
pub mod single_instance;
pub mod system;

pub use dirs::CacheUsage;
pub use settings::{SettingKey, SettingType, SettingValue, SettingsStore};
pub use single_instance::InstanceListener;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("D-Bus: {0}")]
    DBus(String),
    #[error("not supported on this OS")]
    Unsupported,
}
