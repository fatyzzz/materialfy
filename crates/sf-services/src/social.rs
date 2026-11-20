//! Friends and Jam through the librespot session (no Web API), as the desktop client does it.
//!
//! * Friend activity — spclient `GET /presence-view/v1/buddylist` (login5 token of the session): what the
//!   people you follow played last. Polled while the Friends tab is shown (the desktop client polls too).
//! * Jam — spclient `social-connect`: `GET v3/sessions/current` (404 "No active session" — none),
//!   `GET v2/sessions/current_or_new?activate=true&local_device_id=…&type=REMOTE` starts one hosted by this
//!   device, `POST v2/sessions/join/{token}?playback_control=listen_and_control&join_type=deeplinking` joins,
//!   `POST v3/sessions/{id}/leave` leaves, `DELETE v3/sessions/{id}` ends (host). The invite link is
//!   `POST url-dispenser/v1/generate-url` (a spotify.link short link; open.spotify.com/socialsession/{token} when
//!   it fails). Changes arrive as dealer pushes `social-connect/v2/session_update` {reason, session}.
//!   Playback and the shared queue are the host device's connect-state: guests' additions reach Spirc of the host
//!   (this computer) or the cluster of the remote host, and show up in the queue as any other.
//!
//! Checked live (2026-10, Premium): buddylist answers `{"friends":[…]}`; a Jam was started, read, joined by its
//! token, shared (201 with shareable_url), left and ended, and pushes NEW_SESSION / SESSION_DELETED came.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures::StreamExt;
use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};
use sf_player::{PlayerHandle, Session};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use crate::auth::{AuthEvent, AuthService};
use crate::data::{persisted, route};
use crate::lock;

const TIMEOUT: Duration = Duration::from_secs(12);
/// Friend activity is not asked more often than this without `force` (the tab polls once a minute).
const FRIENDS_FRESH: Duration = Duration::from_secs(20);
/// How long to wait for the session while the librespot engine connects.
const SESSION_WAIT: Duration = Duration::from_secs(20);
const PUSH_URI: &str = "social-connect/v2/session_update";
/// Subscribing while Spirc launches the dealer fails; it is tried again this often, this many times.
const PUSH_RETRY: Duration = Duration::from_secs(2);
const PUSH_TRIES: u32 = 10;
const SHARE_BASE: &str = "https://open.spotify.com/socialsession/";

/// One friend's last play (a row of the desktop "Friend Activity").
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendActivity {
    pub user_uri: String,
    pub name: String,
    pub avatar: String,
    pub track_uri: String,
    pub track: String,
    pub cover: String,
    pub artist: String,
    pub artist_uri: String,
    pub album: String,
    pub album_uri: String,
    /// what it played from (playlist, album, artist…); "" — unknown
    pub context: String,
    pub context_uri: String,
    /// ms since the epoch
    pub timestamp: i64,
    pub is_episode: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamMember {
    pub id: String,
    pub username: String,
    pub name: String,
    pub avatar: String,
    pub is_host: bool,
    pub is_you: bool,
    pub listening: bool,
    pub controlling: bool,
}

/// The Jam the account is in.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Jam {
    pub session_id: String,
    pub join_token: String,
    /// invite link: a spotify.link short link once made, the open.spotify.com one before
    pub share_url: String,
    pub is_host: bool,
    pub host_name: String,
    pub members: Vec<JamMember>,
    /// connect device that plays the Jam ("" — none yet)
    pub host_device_id: String,
    pub max_members: u32,
    pub queue_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SocialSnapshot {
    pub friends: Arc<Vec<FriendActivity>>,
    /// friend activity was read at least once
    pub friends_loaded: bool,
    pub friends_loading: bool,
    pub friends_error: String,
    pub jam: Option<Jam>,
    /// the current Jam was checked at least once
    pub jam_checked: bool,
    /// a Jam request is running (check, start, join, leave)
    pub jam_busy: bool,
    pub jam_error: String,
}

#[derive(Default)]
struct State {
    friends_at: Option<Instant>,
    /// invite short links by join token
    links: HashMap<String, String>,
    /// session (username + device) whose pushes are listened to
    pushes_for: Option<String>,
}

pub struct SocialService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    snapshot: watch::Sender<SocialSnapshot>,
    state: Mutex<State>,
    push_tries: AtomicU32,
    me: Weak<SocialService>,
}

impl SocialService {
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth,
            player,
            snapshot: watch::channel(SocialSnapshot::default()).0,
            state: Mutex::new(State::default()),
            push_tries: AtomicU32::new(0),
            me: me.clone(),
        });
        rt.spawn(listen(Arc::downgrade(&service)));
        service
    }

    pub fn snapshot(&self) -> SocialSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<SocialSnapshot> {
        self.snapshot.subscribe()
    }

    /// Re-read friend activity (`force` — even if it is fresh).
    pub fn refresh_friends(&self, force: bool) {
        {
            let mut state = lock(&self.state);
            if !force && state.friends_at.is_some_and(|at| at.elapsed() < FRIENDS_FRESH) {
                return;
            }
            state.friends_at = Some(Instant::now());
        }
        self.snapshot.send_if_modified(|s| {
            let changed = !s.friends_loading || !s.friends_error.is_empty();
            s.friends_loading = true;
            s.friends_error.clear();
            changed
        });
        self.spawn(|me| async move {
            let result = match me.wait_session().await {
                Some(session) => me.friends(&session).await,
                None => Err(not_ready()),
            };
            me.snapshot.send_modify(|s| {
                s.friends_loading = false;
                match result {
                    Ok(friends) => {
                        s.friends_loaded = true;
                        if *s.friends != friends {
                            s.friends = Arc::new(friends);
                        }
                    }
                    Err(error) => s.friends_error = error,
                }
            });
        });
    }

    /// Re-read the current Jam.
    pub fn refresh_jam(&self) {
        self.subscribe_pushes();
        self.jam_request(|me, session| async move { me.current(&session).await });
    }

    /// Start a Jam hosted by this device (or get the one the account is already in).
    pub fn start_jam(&self) {
        self.jam_request(|me, session| async move {
            let path = format!(
                "/social-connect/v2/sessions/current_or_new?activate=true&local_device_id={}&type=REMOTE",
                encode(session.device_id())
            );
            let reply = me.request(&session, Method::GET, &path, None).await?;
            if !reply.ok() {
                tracing::warn!("jam not started: {}", reply.describe());
                return Err(mf_i18n::t("Couldn't start a Jam"));
            }
            Ok(parse_jam(&reply.json()).ok_or_else(|| mf_i18n::t("Couldn't start a Jam"))?.into())
        });
    }

    /// Join by an invite link (open.spotify.com/socialsession/…, spotify.link/…), a spotify:socialsession URI or
    /// the bare code.
    pub fn join_jam(&self, text: &str) {
        let text = text.trim().to_owned();
        self.jam_request(move |me, session| async move {
            let token = match join_token(&text) {
                Some(token) => token,
                None if is_short_link(&text) => me.resolve(&text).await.ok_or_else(|| mf_i18n::t("This Jam link doesn't work"))?,
                None => return Err(mf_i18n::t("This Jam link doesn't work")),
            };
            let path = format!(
                "/social-connect/v2/sessions/join/{}?playback_control=listen_and_control&join_type=deeplinking&participant_id=",
                encode(&token)
            );
            let reply = me.request(&session, Method::POST, &path, None).await?;
            if reply.status == 404 {
                return Err(mf_i18n::t("This Jam has ended or the link is wrong"));
            }
            if !reply.ok() {
                tracing::warn!("jam not joined: {}", reply.describe());
                return Err(mf_i18n::t("Couldn't join the Jam"));
            }
            let jam = parse_jam(&reply.json()).ok_or_else(|| mf_i18n::t("Couldn't join the Jam"))?;
            // playback now follows the host's device
            me.player.refresh_remote();
            Ok(Some(jam))
        });
    }

    /// Leave the Jam; the host ends it for everyone.
    pub fn leave_jam(&self) {
        let Some(jam) = self.snapshot.borrow().jam.clone() else { return };
        self.jam_request(move |me, session| async move {
            let reply = if jam.is_host {
                let path = format!("/social-connect/v3/sessions/{}", encode(&jam.session_id));
                me.request(&session, Method::DELETE, &path, None).await?
            } else {
                let path = format!("/social-connect/v3/sessions/{}/leave", encode(&jam.session_id));
                me.request(&session, Method::POST, &path, None).await?
            };
            // 404 — it is already over
            if !reply.ok() && reply.status != 404 {
                tracing::warn!("jam not left: {}", reply.describe());
                return Err(mf_i18n::t("Couldn't leave the Jam"));
            }
            if !jam.is_host {
                me.player.refresh_remote();
            }
            Ok(None)
        });
    }

    // --- inside -----------------------------------------------------------------------------------

    fn spawn<F, Fut>(&self, work: F)
    where
        F: FnOnce(Arc<Self>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(me) = self.me.upgrade() {
            self.rt.spawn(work(me));
        }
    }

    /// A Jam request: busy while it runs, its answer (Some/None) becomes the Jam, an error is shown.
    fn jam_request<F, Fut>(&self, work: F)
    where
        F: FnOnce(Arc<Self>, Session) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Option<Jam>, String>> + Send + 'static,
    {
        self.snapshot.send_modify(|s| {
            s.jam_busy = true;
            s.jam_error.clear();
        });
        self.spawn(|me| async move {
            let result = match me.wait_session().await {
                Some(session) => {
                    me.subscribe_pushes();
                    work(me.clone(), session).await
                }
                None => Err(not_ready()),
            };
            match result {
                Ok(jam) => me.set_jam(jam),
                Err(error) => me.snapshot.send_modify(|s| s.jam_error = error),
            }
            me.snapshot.send_modify(|s| {
                s.jam_busy = false;
                s.jam_checked = true;
            });
        });
    }

    /// New Jam state; an invite short link is made once per Jam.
    fn set_jam(&self, jam: Option<Jam>) {
        let jam = jam.map(|mut jam| {
            if let Some(link) = lock(&self.state).links.get(&jam.join_token) {
                jam.share_url = link.clone();
            }
            jam
        });
        let token = jam.as_ref().filter(|jam| !jam.share_url.starts_with("https://spotify.link")).map(|jam| jam.join_token.clone());
        self.snapshot.send_if_modified(|s| {
            if s.jam == jam {
                return false;
            }
            s.jam = jam;
            true
        });
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            self.spawn(|me| async move { me.make_link(token).await });
        }
    }

    async fn make_link(self: Arc<Self>, token: String) {
        if lock(&self.state).links.contains_key(&token) {
            return;
        }
        let Some(session) = self.session() else { return };
        let body = json!({
            "spotify_uri": format!("spotify:socialsession:{token}"),
            "custom_data": [{ "key": "ssp", "value": "1" }, { "key": "app_destination", "value": "socialsession" }],
            "utm_parameters": { "utm_medium": "share-link", "utm_source": "share-options-sheet" },
        });
        let link = match self.request(&session, Method::POST, "/url-dispenser/v1/generate-url", Some(body)).await {
            Ok(reply) if reply.ok() => short_link(&reply.json()),
            Ok(reply) => {
                tracing::debug!("jam link not made: {}", reply.describe());
                None
            }
            Err(error) => {
                tracing::debug!("jam link not made: {error}");
                None
            }
        };
        let Some(link) = link else { return };
        lock(&self.state).links.insert(token.clone(), link.clone());
        self.snapshot.send_if_modified(|s| match s.jam.as_mut() {
            Some(jam) if jam.join_token == token && jam.share_url != link => {
                jam.share_url = link;
                true
            }
            _ => false,
        });
    }

    async fn current(&self, session: &Session) -> Result<Option<Jam>, String> {
        let reply = self.request(session, Method::GET, "/social-connect/v3/sessions/current", None).await?;
        if reply.status == 404 {
            return Ok(None);
        }
        if !reply.ok() {
            tracing::warn!("jam not read: {}", reply.describe());
            return Err(mf_i18n::t("Couldn't check for a Jam"));
        }
        Ok(parse_jam(&reply.json()))
    }

    async fn friends(&self, session: &Session) -> Result<Vec<FriendActivity>, String> {
        let reply = self.request(session, Method::GET, "/presence-view/v1/buddylist", None).await?;
        if !reply.ok() {
            tracing::warn!("friend activity not read: {}", reply.describe());
            return Err(mf_i18n::t("Couldn't load friend activity"));
        }
        parse_buddylist(&reply.body).ok_or_else(|| mf_i18n::t("Couldn't load friend activity"))
    }

    /// The Jam code behind a spotify.link short link (its landing page names open.spotify.com/socialsession/…).
    async fn resolve(&self, link: &str) -> Option<String> {
        let url = if link.starts_with("http") { link.to_owned() } else { format!("https://{link}") };
        let client = self.auth.api().http().client().ok()?;
        let response = client
            .get(&url)
            .header(reqwest::header::USER_AGENT, persisted::USER_AGENT)
            .timeout(TIMEOUT)
            .send()
            .await
            .ok()?;
        if let Some(token) = join_token(response.url().as_str()) {
            return Some(token);
        }
        token_in_page(&response.text().await.ok()?)
    }

    /// A spclient request with the session's token; the status and body come back as they are.
    async fn request(&self, session: &Session, method: Method, path: &str, body: Option<Value>) -> Result<Reply, String> {
        let attempt = async {
            let token = session.login5().auth_token().await.map_err(|error| format!("login5: {error}"))?;
            let base = session.spclient().base_url().await.map_err(|error| format!("spclient: {error}"))?;
            let client = self.auth.api().http().client()?;
            let mut request = client
                .request(method.clone(), format!("{base}{path}"))
                .bearer_auth(&token.access_token)
                .header(reqwest::header::ACCEPT, "application/json")
                .timeout(TIMEOUT);
            if let Ok(client_token) = session.spclient().client_token().await {
                request = request.header("client-token", client_token);
            }
            if let Some(body) = body {
                request = request.json(&body);
            } else if method == Method::POST {
                request = request.header(reqwest::header::CONTENT_LENGTH, "0");
            }
            let response = request.send().await.map_err(|error| error.without_url().to_string())?;
            let status = response.status().as_u16();
            let body = response.bytes().await.map_err(|error| error.without_url().to_string())?.to_vec();
            Ok::<_, String>(Reply { status, body })
        };
        attempt.await.map_err(|error| {
            tracing::warn!("{method} {path}: {error}");
            mf_i18n::t("Couldn't reach Spotify")
        })
    }

    fn session(&self) -> Option<Session> {
        route::account_session(&self.auth.api(), &self.player)
    }

    async fn wait_session(&self) -> Option<Session> {
        let started = Instant::now();
        loop {
            if let Some(session) = self.session() {
                return Some(session);
            }
            let coming = matches!(self.player.local_playback(), sf_player::LocalPlayback::Connecting);
            if !coming || started.elapsed() >= SESSION_WAIT {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Listen to Jam pushes of the current session (once per session).
    fn subscribe_pushes(&self) {
        let Some(session) = self.session() else { return };
        let key = format!("{}/{}", session.username(), session.device_id());
        {
            let mut state = lock(&self.state);
            if state.pushes_for.as_deref() == Some(key.as_str()) {
                return;
            }
            state.pushes_for = Some(key.clone());
        }
        let mut pushes = match sf_player::pushes::listen_json(&session, PUSH_URI) {
            Ok(pushes) => pushes,
            Err(error) => {
                // Spirc is launching the dealer right now (neither builder nor dealer is there) — a bit later
                tracing::debug!("jam pushes unavailable: {error}");
                lock(&self.state).pushes_for = None;
                let tries = self.push_tries.fetch_add(1, Ordering::Relaxed);
                if tries < PUSH_TRIES {
                    self.spawn(|me| async move {
                        tokio::time::sleep(PUSH_RETRY).await;
                        me.subscribe_pushes();
                    });
                }
                return;
            }
        };
        self.push_tries.store(0, Ordering::Relaxed);
        let weak = self.me.clone();
        self.rt.spawn(async move {
            while let Some(text) = pushes.next().await {
                let Some(me) = weak.upgrade() else { return };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                let current = me.snapshot.borrow().jam.as_ref().map(|jam| jam.session_id.clone());
                match parse_update(&value, current.as_deref()) {
                    Update::Jam(jam) => me.set_jam(Some(jam)),
                    Update::Over => me.set_jam(None),
                    Update::Ignore => {}
                }
                me.snapshot.send_modify(|s| s.jam_checked = true);
            }
            if let Some(me) = weak.upgrade() {
                let mut state = lock(&me.state);
                if state.pushes_for.as_deref() == Some(key.as_str()) {
                    state.pushes_for = None;
                }
            }
        });
    }

    fn clear(&self) {
        *lock(&self.state) = State::default();
        self.snapshot.send_replace(SocialSnapshot::default());
    }
}

async fn listen(weak: Weak<SocialService>) {
    let Some(me) = weak.upgrade() else { return };
    let mut auth = me.auth.subscribe();
    drop(me);
    loop {
        let event = auth.recv().await;
        let Some(me) = weak.upgrade() else { break };
        match event {
            Ok(AuthEvent::SignedOut) => me.clear(),
            Ok(AuthEvent::PlaybackReady(_)) => me.refresh_jam(),
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn not_ready() -> String {
    mf_i18n::t("Friends and Jams need Spotify playback to be signed in on this computer")
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn describe(&self) -> String {
        let body = String::from_utf8_lossy(&self.body);
        format!("{} {}", self.status, body.chars().take(200).collect::<String>())
    }
}

/// Path segment / query value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' | b'~' => char::from(byte).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

// --- parsing ----------------------------------------------------------------------------------------

fn text(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned()
}

/// i.scdn.co answers over https too; the buddylist gives plain http covers.
fn secure(url: String) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url,
    }
}

fn number(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_default(),
        Some(Value::String(s)) => s.parse().unwrap_or_default(),
        _ => 0,
    }
}

/// `{"friends":[{timestamp, user{uri,name,imageUrl}, track{uri,name,imageUrl,album,artist,context}}]}`, newest
/// first. None — not a buddylist answer.
fn parse_buddylist(body: &[u8]) -> Option<Vec<FriendActivity>> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let friends = value.get("friends")?.as_array()?;
    let mut list: Vec<FriendActivity> = friends
        .iter()
        .filter_map(|friend| {
            let user_uri = text(friend, "/user/uri");
            let track_uri = text(friend, "/track/uri");
            if user_uri.is_empty() || track_uri.is_empty() {
                return None;
            }
            let name = Some(text(friend, "/user/name"))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| user_uri.rsplit(':').next().unwrap_or_default().to_owned());
            Some(FriendActivity {
                name,
                avatar: secure(text(friend, "/user/imageUrl")),
                track: text(friend, "/track/name"),
                cover: secure(text(friend, "/track/imageUrl")),
                artist: text(friend, "/track/artist/name"),
                artist_uri: text(friend, "/track/artist/uri"),
                album: text(friend, "/track/album/name"),
                album_uri: text(friend, "/track/album/uri"),
                context: text(friend, "/track/context/name"),
                context_uri: text(friend, "/track/context/uri"),
                timestamp: number(friend, "timestamp"),
                is_episode: track_uri.starts_with("spotify:episode:"),
                user_uri,
                track_uri,
            })
        })
        .collect();
    list.sort_by_key(|friend| std::cmp::Reverse(friend.timestamp));
    Some(list)
}

/// A social-connect session (the answer of current / current_or_new / join, or `session` of a push).
fn parse_jam(value: &Value) -> Option<Jam> {
    let session_id = text(value, "/session_id");
    if session_id.is_empty() || value.get("active").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let owner = text(value, "/session_owner_id");
    let members: Vec<JamMember> = value
        .get("session_members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|member| {
            let id = text(member, "/id");
            let username = text(member, "/username");
            let name = Some(text(member, "/display_name")).filter(|name| !name.is_empty()).unwrap_or_else(|| username.clone());
            let avatar = Some(text(member, "/image_url"))
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| text(member, "/large_image_url"));
            JamMember {
                is_host: !owner.is_empty() && id == owner,
                is_you: member.get("is_current_user").and_then(Value::as_bool).unwrap_or(false),
                listening: member.get("is_listening").and_then(Value::as_bool).unwrap_or(true),
                controlling: member.get("is_controlling").and_then(Value::as_bool).unwrap_or(false),
                id,
                username,
                name,
                avatar: secure(avatar),
            }
        })
        .collect();
    let join_token = text(value, "/join_session_token");
    let host_name = members.iter().find(|member| member.is_host).map(|member| member.name.clone()).unwrap_or_default();
    Some(Jam {
        share_url: if join_token.is_empty() { String::new() } else { format!("{SHARE_BASE}{join_token}") },
        is_host: value.get("is_session_owner").and_then(Value::as_bool).unwrap_or(false),
        host_name,
        members,
        host_device_id: text(value, "/hostActiveDeviceId"),
        max_members: u32::try_from(number(value, "maxMemberCount")).unwrap_or_default(),
        queue_only: value.get("queue_only_mode").and_then(Value::as_bool).unwrap_or(false),
        join_token,
        session_id,
    })
}

#[derive(Debug, PartialEq)]
enum Update {
    Jam(Jam),
    /// the Jam is over for this account
    Over,
    Ignore,
}

/// A `social-connect/v2/session_update` push: `{"reason": "...", "session": {...}}`. A push about another session
/// than the current one only matters when it starts or joins one.
fn parse_update(value: &Value, current: Option<&str>) -> Update {
    let reason = value.get("reason").and_then(Value::as_str).unwrap_or_default();
    let Some(session) = value.get("session") else { return Update::Ignore };
    let id = text(session, "/session_id");
    let ours = current.is_none_or(|current| current == id);
    match reason {
        "SESSION_DELETED" | "YOU_LEFT" | "YOU_WERE_KICKED" if ours => Update::Over,
        "SESSION_DELETED" | "YOU_LEFT" | "YOU_WERE_KICKED" => Update::Ignore,
        "NEW_SESSION" | "YOU_JOINED" => parse_jam(session).map_or(Update::Ignore, Update::Jam),
        _ if ours => parse_jam(session).map_or(Update::Ignore, Update::Jam),
        _ => Update::Ignore,
    }
}

/// shareable_url of a url-dispenser answer.
fn short_link(value: &Value) -> Option<String> {
    let link = text(value, "/shareable_url");
    link.starts_with("https://").then_some(link)
}

fn is_code(text: &str) -> bool {
    (16..=40).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The Jam code of an invite: open.spotify.com/socialsession/{code}?si=…, spotify:socialsession:{code},
/// hm://social-connect/v2/sessions/join/{code} or the code itself. Short links need resolving first.
fn join_token(text: &str) -> Option<String> {
    let text = text.trim();
    if is_code(text) {
        return Some(text.to_owned());
    }
    let after = ["socialsession/", "socialsession:", "sessions/join/"]
        .iter()
        .find_map(|marker| text.find(marker).map(|at| &text[at + marker.len()..]))?;
    let code: String = after.chars().take_while(char::is_ascii_alphanumeric).collect();
    is_code(&code).then_some(code)
}

/// spotify.link / spotify.app.link short link.
fn is_short_link(text: &str) -> bool {
    let rest = text.trim().trim_start_matches("https://").trim_start_matches("http://");
    rest.starts_with("spotify.link/") || rest.starts_with("spotify.app.link/")
}

fn token_in_page(page: &str) -> Option<String> {
    let at = page.find("socialsession/")?;
    join_token(&page[at..])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the buddylist format (the live account follows no people, so its answer was
    /// `{"friends":[]}`; this one carries every field the desktop client reads).
    const BUDDYLIST: &str = r#"{"friends":[
        {"timestamp":1791780000000,"user":{"uri":"spotify:user:alice","name":"Alice","imageUrl":"https://i.scdn.co/image/ab6775700000ee85a1"},
         "track":{"uri":"spotify:track:4uLU6hMCjMI75M1A2tKUQC","name":"Never Gonna Give You Up","imageUrl":"http://i.scdn.co/image/ab67616d00001e02b1",
           "album":{"uri":"spotify:album:6N9PS4QXF1D0OWPk0Sxtb4","name":"Whenever You Need Somebody"},
           "artist":{"uri":"spotify:artist:0gxyHStUsqpMadRV0Di1Qt","name":"Rick Astley"},
           "context":{"uri":"spotify:playlist:37i9dQZF1DXcBWIGoYBM5M","name":"Today's Top Hits","index":0}}},
        {"timestamp":1791791000000,"user":{"uri":"spotify:user:bob"},
         "track":{"uri":"spotify:episode:512ojhOuo1ktJprKbVcKyQ","name":"Episode","imageUrl":"",
           "album":{"uri":"spotify:show:2MAi0BvDc6GTFvKFPXnkCL","name":"Show"},
           "artist":{"uri":"spotify:show:2MAi0BvDc6GTFvKFPXnkCL","name":"Show"}}},
        {"timestamp":1,"user":{"uri":"spotify:user:carol","name":"Carol"}}
    ]}"#;

    /// Trimmed live answer of GET social-connect/v3/sessions/current (2026-10).
    const CURRENT: &str = r#"{"timestamp":"1791792418293","session_id":"f663581b3295b23868bcc730ee180097",
        "join_session_token":"7uVvNohpFt5ZsOdgfYewlh","join_session_url":"hm://social-connect/v2/sessions/join/7uVvNohpFt5ZsOdgfYewlh",
        "session_owner_id":"4bbcfbac6a854ae8c32cb667b57c4187","session_members":[{"joined_timestamp":"1791792417662",
        "id":"4bbcfbac6a854ae8c32cb667b57c4187","username":"31qn6rfojzf5efui4bxblwz6llhe","display_name":"fatyzzz",
        "image_url":"","large_image_url":"","is_listening":true,"is_controlling":true,"playbackControl":"LISTEN_AND_CONTROL",
        "is_current_user":true,"listen_mode":false,"is_anonymous":false},{"id":"guest1","username":"guest","display_name":"",
        "image_url":"http://i.scdn.co/image/guest","is_listening":true,"is_controlling":false}],
        "join_session_uri":"spotify:socialsession:7uVvNohpFt5ZsOdgfYewlh","is_session_owner":true,"is_listening":true,
        "is_controlling":true,"initialSessionType":"REMOTE","hostActiveDeviceId":"","maxMemberCount":32,"active":true,
        "queue_only_mode":false,"wifi_broadcast":false,"is_paused":false}"#;

    #[test]
    fn friend_activity_reads_newest_first() {
        let friends = parse_buddylist(BUDDYLIST.as_bytes()).unwrap();
        assert_eq!(friends.len(), 2, "an entry without a track is skipped");
        let bob = &friends[0];
        assert_eq!((bob.name.as_str(), bob.timestamp), ("bob", 1_791_791_000_000), "no name — the username");
        assert!(bob.is_episode);
        assert_eq!(bob.context, "");
        let alice = &friends[1];
        assert_eq!(alice.name, "Alice");
        assert_eq!(alice.avatar, "https://i.scdn.co/image/ab6775700000ee85a1");
        assert_eq!(alice.cover, "https://i.scdn.co/image/ab67616d00001e02b1", "covers go over https");
        assert_eq!((alice.track.as_str(), alice.artist.as_str()), ("Never Gonna Give You Up", "Rick Astley"));
        assert_eq!(alice.artist_uri, "spotify:artist:0gxyHStUsqpMadRV0Di1Qt");
        assert_eq!(alice.album_uri, "spotify:album:6N9PS4QXF1D0OWPk0Sxtb4");
        assert_eq!((alice.context.as_str(), alice.context_uri.as_str()), ("Today's Top Hits", "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"));
        assert!(!alice.is_episode);
        // the live answer of an account that follows nobody
        assert_eq!(parse_buddylist(br#"{"friends":[]}"#), Some(Vec::new()));
        assert_eq!(parse_buddylist(b"<html>"), None);
    }

    #[test]
    fn the_current_jam_reads_with_its_members() {
        let jam = parse_jam(&serde_json::from_str(CURRENT).unwrap()).unwrap();
        assert_eq!(jam.session_id, "f663581b3295b23868bcc730ee180097");
        assert_eq!(jam.join_token, "7uVvNohpFt5ZsOdgfYewlh");
        assert_eq!(jam.share_url, "https://open.spotify.com/socialsession/7uVvNohpFt5ZsOdgfYewlh");
        assert!(jam.is_host);
        assert_eq!(jam.host_name, "fatyzzz");
        assert_eq!(jam.max_members, 32);
        assert_eq!(jam.members.len(), 2);
        assert!(jam.members[0].is_host && jam.members[0].is_you && jam.members[0].controlling);
        let guest = &jam.members[1];
        assert_eq!((guest.name.as_str(), guest.avatar.as_str()), ("guest", "https://i.scdn.co/image/guest"));
        assert!(!guest.is_host && !guest.is_you && !guest.controlling);
        assert_eq!(parse_jam(&json!({ "session_id": "x", "active": false })), None, "an inactive session is no Jam");
        assert_eq!(parse_jam(&json!({ "code": 5, "message": "No active session" })), None);
    }

    #[test]
    fn jam_pushes_start_change_and_end_it() {
        let session: Value = serde_json::from_str(CURRENT).unwrap();
        // trimmed live pushes (2026-10)
        let started = json!({ "reason": "NEW_SESSION", "session": session.clone(), "update_session_members": [] });
        assert!(matches!(parse_update(&started, None), Update::Jam(jam) if jam.session_id == "f663581b3295b23868bcc730ee180097"));
        let deleted = json!({ "reason": "SESSION_DELETED", "session": session.clone() });
        assert_eq!(parse_update(&deleted, Some("f663581b3295b23868bcc730ee180097")), Update::Over);
        assert_eq!(parse_update(&deleted, Some("another")), Update::Ignore, "another Jam ending is not ours");
        let joined = json!({ "reason": "USER_JOINED", "session": session.clone() });
        assert!(matches!(parse_update(&joined, Some("f663581b3295b23868bcc730ee180097")), Update::Jam(_)));
        assert_eq!(parse_update(&joined, Some("another")), Update::Ignore);
        let kicked = json!({ "reason": "YOU_WERE_KICKED", "session": session });
        assert_eq!(parse_update(&kicked, None), Update::Over);
        assert_eq!(parse_update(&json!({ "reason": "NEW_SESSION" }), None), Update::Ignore);
    }

    #[test]
    fn invites_give_their_code() {
        let code = Some("3fPe4JtyWSuP4mEYTBwpF7".to_owned());
        assert_eq!(join_token("3fPe4JtyWSuP4mEYTBwpF7"), code);
        assert_eq!(join_token(" https://open.spotify.com/socialsession/3fPe4JtyWSuP4mEYTBwpF7?si=uAyp&ssp=1 "), code);
        assert_eq!(join_token("spotify:socialsession:3fPe4JtyWSuP4mEYTBwpF7"), code);
        assert_eq!(join_token("hm://social-connect/v2/sessions/join/3fPe4JtyWSuP4mEYTBwpF7"), code);
        assert_eq!(join_token("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"), None);
        assert_eq!(join_token("hello"), None);
        assert_eq!(join_token("https://spotify.link/jmnklg1Ga7b"), None, "a short link is resolved first");
        assert!(is_short_link("https://spotify.link/jmnklg1Ga7b"));
        assert!(is_short_link("spotify.app.link/jmnklg1Ga7b?_p=c5"));
        assert!(!is_short_link("https://open.spotify.com/socialsession/3fPe4JtyWSuP4mEYTBwpF7"));
        // trimmed landing page of the live short link
        let page = r#"<meta property="og:url" content="https://open.spotify.com/socialsession/3fPe4JtyWSuP4mEYTBwpF7?si=uAyp">"#;
        assert_eq!(token_in_page(page), code);
        assert_eq!(token_in_page("<html></html>"), None);
    }

    #[test]
    fn the_invite_short_link_comes_from_the_url_dispenser() {
        // trimmed live 201 answer
        let answer = json!({ "spotify_uri": "spotify:socialsession:3fPe4JtyWSuP4mEYTBwpF7",
            "shareable_url": "https://spotify.link/jmnklg1Ga7b", "share_id": "uAypvUByR8KoZUJhy3SoTQ" });
        assert_eq!(short_link(&answer).as_deref(), Some("https://spotify.link/jmnklg1Ga7b"));
        assert_eq!(short_link(&json!({})), None);
    }

    #[test]
    fn path_values_are_encoded() {
        assert_eq!(encode("4dc9318a-9049-4215"), "4dc9318a-9049-4215");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
