//! Контекст воспроизведения списка: тип (для Player.source) + context URI Spotify.
use serde::{Deserialize, Serialize};

pub const DJ_URI: &str = "spotify:playlist:37i9dQZF1EYkqdzj48dyYq";
pub const DJ_COVER: &str = "https://lexicon-assets.spotifycdn.com/Your-DJ-Cover-Art-640.png";

/// {"type": liked|playlist|album|artist|search|radio|show|queue|…, "uri": "spotify:…"?, "title": "…", "cover": "…"}.
/// `uri` — то, что уходит в librespot/Web API как context_uri (None — играть списком URI).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayContext {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "uri", skip_serializing_if = "Option::is_none")]
    pub context_uri: Option<String>,
    /// подпись «Играет из …» (имя плейлиста/альбома/станции); пусто — неизвестно
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// обложка контекста ~300 px (карточка «Играет из …»); пусто — нет
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cover: String,
}

impl PlayContext {
    pub fn new(kind: &str) -> Self {
        Self { kind: kind.to_owned(), context_uri: None, title: String::new(), cover: String::new() }
    }

    pub fn with_uri(kind: &str, uri: impl Into<String>) -> Self {
        let uri = uri.into();
        if uri == DJ_URI {
            return Self { kind: contexts::DJ.to_owned(), context_uri: Some(uri), title: "DJ".to_owned(), cover: DJ_COVER.to_owned() };
        }
        Self { kind: kind.to_owned(), context_uri: Some(uri), title: String::new(), cover: String::new() }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_cover(mut self, cover: impl Into<String>) -> Self {
        self.cover = cover.into();
        self
    }
}

/// Значения `PlayContext.kind` (= Player.source в QML).
pub mod contexts {
    pub const DJ: &str = "dj";
    pub const LIKED: &str = "liked";
    pub const PLAYLIST: &str = "playlist";
    pub const ALBUM: &str = "album";
    pub const ARTIST: &str = "artist";
    pub const SEARCH: &str = "search";
    /// радио Spotify (станция spotify:station:…); бывшее "wave"
    pub const RADIO: &str = "radio";
    pub const SHOW: &str = "show";
    pub const EPISODE: &str = "episode";
    /// отдельные треки (очередь, история, «любой трек»)
    pub const TRACKS: &str = "tracks";
    /// контекст неизвестен (удалённое устройство не сообщило)
    pub const UNKNOWN: &str = "";

    /// `spotify:user:<id>:collection` — «Любимые треки» как контекст.
    pub fn liked_uri(account_id: &str) -> String {
        format!("spotify:user:{account_id}:collection")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dj_is_a_session_even_though_its_uri_is_a_playlist() {
        let dj = PlayContext::with_uri(contexts::PLAYLIST, DJ_URI);
        assert_eq!(dj.kind, contexts::DJ);
        assert_eq!(dj.title, "DJ");
        assert_eq!(dj.context_uri.as_deref(), Some(DJ_URI));
        assert_eq!(dj.cover, DJ_COVER);
        let ordinary = PlayContext::with_uri(contexts::PLAYLIST, "spotify:playlist:ordinary");
        assert_eq!(ordinary.kind, contexts::PLAYLIST);
        assert!(ordinary.title.is_empty());
    }
}
