//! Discord IPC transport and activity protocol.
//!
//! Discord exposes a local socket (`discord-ipc-0` … `discord-ipc-9`) that
//! speaks a tiny binary framing protocol over JSON:
//!
//! ```text
//! ┌──────────────┬─────────────────┬──────────────────────────┐
//! │ op: u32 (LE) │ len: u32 (LE)   │ payload: `len` bytes      │
//! └──────────────┴─────────────────┴──────────────────────────┘
//! op 0 handshake · 1 frame · 2 close · 3 ping · 4 pong
//! ```
//!
//! The transport is deliberately invisible to the rest of the application: the
//! [`DiscordRpc`] handle queues the *latest* activity and a single task owns the
//! socket, the handshake, the reconnect loop and the exponential backoff. When
//! Discord is not running — which is the normal case for an audiophile player
//! — nobody blocks, nothing errors, and the status simply reads `unavailable`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// Handshake frame.
const OP_HANDSHAKE: u32 = 0;
/// Activity frame.
const OP_FRAME: u32 = 1;
/// Server-initiated close.
const OP_CLOSE: u32 = 2;
/// Keepalive request.
const OP_PING: u32 = 3;
/// Keepalive answer.
const OP_PONG: u32 = 4;

/// Largest payload accepted from the socket (defensive bound).
const MAX_PAYLOAD: usize = 1 << 20;

/// First reconnect delay.
const BACKOFF_START: Duration = Duration::from_secs(1);
/// Ceiling for the reconnect delay.
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Discord allows five activity updates every twenty seconds; staying under
/// half of that leaves headroom for the user opening and closing albums.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_millis(2_500);

/// Reconnect schedule.
///
/// Kept as a value rather than as three loose constants so the tests can drive a
/// full offline → online → offline cycle in milliseconds instead of minutes, and
/// so the growth curve is a property with an assertion instead of a comment.
///
/// The default policy waits 1 s after the first failure and multiplies by 1.5 on
/// every subsequent one, capped at 60 s. A user whose Discord is closed does not
/// notice; a user who opens Discord does not wait more than a minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// Delay after the first failure.
    start: Duration,
    /// Ceiling, however many failures pile up.
    max: Duration,
    /// Current delay, advanced by [`BackoffPolicy::next`].
    current: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            start: BACKOFF_START,
            max: BACKOFF_MAX,
            current: BACKOFF_START,
        }
    }
}

impl BackoffPolicy {
    /// A policy that starts at `start` and never exceeds `max`.
    #[must_use]
    pub fn new(start: Duration, max: Duration) -> Self {
        Self {
            start,
            max: max.max(start),
            current: start,
        }
    }

    /// Delay to wait *now*, then grow. Every call is jittered so that a hundred
    /// Prism instances do not reconnect in lockstep after a Discord restart.
    pub fn next(&mut self) -> Duration {
        let delay = jitter(self.current);
        self.current = self.grow(self.current);
        delay
    }

    /// The un-jittered delay that [`Self::next`] will use next.
    #[must_use]
    pub fn peek(&self) -> Duration {
        self.current
    }

    /// Back to the first delay, after a successful connection.
    pub fn reset(&mut self) {
        self.current = self.start;
    }

    fn grow(&self, delay: Duration) -> Duration {
        let grown = delay.saturating_mul(3) / 2;
        if grown > self.max { self.max } else { grown }
    }
}

/// Connection lifecycle, surfaced to the settings UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscordState {
    /// Rich presence is turned off.
    Disabled,
    /// Socket open, handshake in flight.
    Connecting,
    /// Ready to accept activity updates.
    Connected,
    /// The socket dropped; a retry is scheduled.
    Reconnecting,
    /// Discord is not running (or the client id is unknown to it).
    Unavailable,
    /// The service was shut down.
    Stopped,
}

/// Snapshot of the RPC connection.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordStatus {
    pub state: DiscordState,
    pub client_id: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub last_update_at: Option<String>,
}

impl DiscordStatus {
    #[must_use]
    pub fn disabled(client_id: String) -> Self {
        Self {
            state: DiscordState::Disabled,
            client_id,
            attempts: 0,
            last_error: None,
            last_update_at: None,
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self.state, DiscordState::Connected)
    }
}

/// The subset of the `SET_ACTIVITY` payload Prism uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// First line under the app name: normally "Title — Artist".
    pub details: String,
    /// Second line: normally the album (or "Paused").
    pub state: String,
    /// Unix epoch milliseconds of playback start.
    pub start_timestamp: Option<i64>,
    /// Unix epoch milliseconds of playback end.
    pub end_timestamp: Option<i64>,
    /// HTTPS URL of the cover, or `None` for the bundled fallback asset.
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    /// Small badge: "prism" while playing, "prism-paused" while paused.
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

impl Activity {
    /// Build the `activity` object of a `SET_ACTIVITY` frame.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut activity = serde_json::Map::new();
        activity.insert("type".into(), json!(2)); // 2 = "Listening to"
        activity.insert("details".into(), json!(clip(&self.details)));
        activity.insert("state".into(), json!(clip(&self.state)));

        if self.start_timestamp.is_some() || self.end_timestamp.is_some() {
            let mut timestamps = serde_json::Map::new();
            if let Some(start) = self.start_timestamp {
                timestamps.insert("start".into(), json!(start));
            }
            if let Some(end) = self.end_timestamp {
                timestamps.insert("end".into(), json!(end));
            }
            activity.insert("timestamps".into(), Value::Object(timestamps));
        }

        let mut assets = serde_json::Map::new();
        assets.insert(
            "large_image".into(),
            json!(self.large_image.as_deref().unwrap_or("prism")),
        );
        if let Some(text) = &self.large_text {
            assets.insert("large_text".into(), json!(clip(text)));
        }
        if let Some(image) = &self.small_image {
            assets.insert("small_image".into(), json!(image));
        }
        if let Some(text) = &self.small_text {
            assets.insert("small_text".into(), json!(clip(text)));
        }
        activity.insert("assets".into(), Value::Object(assets));

        Value::Object(activity)
    }
}

/// Discord rejects fields shorter than two characters; pad rather than drop
/// the update, which keeps very short track titles visible.
fn clip(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    let mut out: String = trimmed.chars().take(128).collect();
    if out.chars().count() < 2 {
        out.push(' ');
    }
    out
}

enum Command {
    Set(Box<Activity>),
    Clear,
    Stop,
}

/// Handle to the RPC worker.
#[derive(Debug)]
pub struct DiscordRpc {
    commands: mpsc::UnboundedSender<Command>,
    status: watch::Receiver<DiscordStatus>,
    task: tokio::task::JoinHandle<()>,
}

impl DiscordRpc {
    /// Spawn the worker for `client_id` and return immediately.
    ///
    /// The worker owns the socket: it probes Discord's IPC endpoints, reconnects
    /// with [`BackoffPolicy`] and publishes whatever activity is current the
    /// moment the connection comes up. Discord not running is a normal state,
    /// not an error — the UI simply shows "unavailable".
    #[must_use]
    pub fn spawn(client_id: String) -> Self {
        Self::spawn_with(client_id, candidate_sockets(), BackoffPolicy::default())
    }

    /// Spawn against an explicit socket list and schedule.
    ///
    /// Production always passes [`candidate_sockets`] and the default policy;
    /// the seam exists so the reconnect loop can be tested against a real Unix
    /// socket without touching the user's Discord.
    #[must_use]
    pub fn spawn_with(client_id: String, sockets: Vec<PathBuf>, policy: BackoffPolicy) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = watch::channel(DiscordStatus {
            state: DiscordState::Connecting,
            client_id: client_id.clone(),
            attempts: 0,
            last_error: None,
            last_update_at: None,
        });
        let task = tokio::spawn(run(client_id, sockets, policy, command_rx, status_tx));
        Self {
            commands: command_tx,
            status: status_rx,
            task,
        }
    }

    /// Queue an activity update. The newest one always wins.
    pub fn set_activity(&self, activity: Activity) {
        let _ = self.commands.send(Command::Set(Box::new(activity)));
    }

    /// Clear the activity (keeps the connection open).
    pub fn clear(&self) {
        let _ = self.commands.send(Command::Clear);
    }

    /// Stop the worker and close the socket.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    #[must_use]
    pub fn status(&self) -> DiscordStatus {
        self.status.borrow().clone()
    }
}

impl Drop for DiscordRpc {
    fn drop(&mut self) {
        // Ask the worker to close the socket cleanly, then make sure it cannot
        // outlive the handle if it was parked in a backoff sleep.
        let _ = self.commands.send(Command::Stop);
        self.task.abort();
    }
}

/// Owns the socket, reconnects with exponential backoff, applies the newest
/// activity as soon as the connection is ready.
async fn run(
    client_id: String,
    sockets: Vec<PathBuf>,
    policy: BackoffPolicy,
    mut commands: mpsc::UnboundedReceiver<Command>,
    status: watch::Sender<DiscordStatus>,
) {
    let mut desired: Option<Activity> = None;
    let mut attempts: u32 = 0;
    let mut backoff = policy;

    loop {
        match connect_and_handshake(&client_id, &sockets).await {
            Ok(mut transport) => {
                attempts = 0;
                backoff.reset();
                status.send_modify(|snapshot| {
                    snapshot.state = DiscordState::Connected;
                    snapshot.attempts = 0;
                    snapshot.last_error = None;
                });
                tracing::info!("discord rich presence connected");

                if let Some(activity) = &desired
                    && let Err(error) = send_activity(&mut transport, activity).await
                {
                    tracing::debug!(%error, "discord activity rejected");
                }

                let mut pending: Option<Activity> = None;
                let mut next_send: Option<tokio::time::Instant> = None;
                let mut last_sent = tokio::time::Instant::now();
                let mut alive = true;

                while alive {
                    let deadline = next_send.unwrap_or_else(|| {
                        tokio::time::Instant::now() + Duration::from_secs(3_600)
                    });
                    tokio::select! {
                        biased;

                        command = commands.recv() => {
                            match command {
                                Some(Command::Set(activity)) => {
                                    // Latest wins; the send timer coalesces a
                                    // burst of track changes into one update.
                                    pending = Some(*activity);
                                    if next_send.is_none() {
                                        let earliest = last_sent + MIN_UPDATE_INTERVAL;
                                        next_send = Some(earliest.max(tokio::time::Instant::now()));
                                    }
                                }
                                Some(Command::Clear) => {
                                    pending = None;
                                    next_send = None;
                                    desired = None;
                                    if let Err(error) = send_clear(&mut transport).await {
                                        tracing::debug!(%error, "discord clear failed");
                                    }
                                }
                                Some(Command::Stop) | None => {
                                    let _ = transport.shutdown().await;
                                    status.send_modify(|snapshot| snapshot.state = DiscordState::Stopped);
                                    tracing::info!("discord rich presence stopped");
                                    return;
                                }
                            }
                        }

                        frame = read_frame(&mut transport) => {
                            match frame {
                                Ok((OP_PING, payload)) => {
                                    let _ = write_frame(&mut transport, OP_PONG, &payload).await;
                                }
                                Ok((OP_CLOSE, _)) => {
                                    tracing::debug!("discord closed the connection");
                                    alive = false;
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    tracing::debug!(%error, "discord socket read failed");
                                    alive = false;
                                }
                            }
                        }

                        () = tokio::time::sleep_until(deadline) => {
                            next_send = None;
                            if let Some(activity) = pending.take() {
                                desired = Some(activity.clone());
                                if let Err(error) = send_activity(&mut transport, &activity).await {
                                    tracing::debug!(%error, "discord activity rejected");
                                }
                                last_sent = tokio::time::Instant::now();
                                status.send_modify(|snapshot| {
                                    snapshot.last_update_at = Some(
                                        time::OffsetDateTime::now_utc()
                                            .format(&time::format_description::well_known::Rfc3339)
                                            .unwrap_or_default(),
                                    );
                                });
                            }
                        }
                    }
                }

                status.send_modify(|snapshot| {
                    snapshot.state = DiscordState::Reconnecting;
                });
            }
            Err(error) => {
                attempts = attempts.saturating_add(1);
                status.send_modify(|snapshot| {
                    snapshot.state = DiscordState::Unavailable;
                    snapshot.attempts = attempts;
                    snapshot.last_error = Some(error);
                });
            }
        }

        // Back off before reconnecting, but stay responsive to Stop and to a
        // newer activity so the next connection carries fresh data.
        let delay = backoff.next();
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                () = &mut sleep => break,
                command = commands.recv() => {
                    match command {
                        Some(Command::Set(activity)) => desired = Some(*activity),
                        Some(Command::Clear) => desired = None,
                        Some(Command::Stop) | None => {
                            status.send_modify(|snapshot| snapshot.state = DiscordState::Stopped);
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Deterministic-but-jittered delay: a thundering herd of clients must not
/// reconnect in lockstep after a Discord restart.
fn jitter(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let spread = (u64::from(nanos) % 500) as u32;
    base + Duration::from_millis(u64::from(spread))
}

/// Open the first socket that answers and complete the handshake.
async fn connect_and_handshake(
    client_id: &str,
    sockets: &[PathBuf],
) -> Result<Box<dyn Transport>, String> {
    let mut last_error = "no discord-ipc socket found".to_string();
    for path in sockets {
        match open(&path).await {
            Ok(mut transport) => match handshake(&mut transport, client_id).await {
                Ok(()) => return Ok(transport),
                Err(error) => last_error = format!("{}: {error}", path.display()),
            },
            Err(error) => last_error = format!("{}: {error}", path.display()),
        }
    }
    Err(last_error)
}

async fn handshake(transport: &mut Box<dyn Transport>, client_id: &str) -> Result<(), String> {
    write_frame(
        &mut **transport,
        OP_HANDSHAKE,
        &json!({"v": 1, "client_id": client_id}),
    )
    .await
    .map_err(|error| format!("handshake write: {error}"))?;

    let read = tokio::time::timeout(Duration::from_secs(10), read_frame(transport)).await;
    match read {
        Ok(Ok((OP_FRAME, payload))) => {
            // Discord names the field `evt` on every DISPATCH frame. Older
            // clients and some forks emit `ev`; accept both, because reading the
            // wrong key silently turns a rejection into a reconnect loop.
            let event = payload
                .get("evt")
                .or_else(|| payload.get("ev"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            match event {
                "READY" => Ok(()),
                "ERROR" => Err(format!(
                    "discord rejected the client id: {}",
                    payload
                        .get("data")
                        .map_or_else(String::new, ToString::to_string)
                )),
                // Some clients deliver other events first; the pipe is alive.
                _ => Ok(()),
            }
        }
        Ok(Ok((OP_CLOSE, _))) => Err("discord closed during handshake".to_string()),
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(format!("handshake read: {error}")),
        Err(_) => Err("handshake timed out".to_string()),
    }
}

async fn send_activity(
    transport: &mut Box<dyn Transport>,
    activity: &Activity,
) -> std::io::Result<()> {
    let payload = json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity.to_json(),
        },
        "nonce": nonce(),
    });
    write_frame(transport, OP_FRAME, &payload).await
}

async fn send_clear(transport: &mut Box<dyn Transport>) -> std::io::Result<()> {
    let payload = json!({
        "cmd": "SET_ACTIVITY",
        "args": {"pid": std::process::id(), "activity": Value::Null},
        "nonce": nonce(),
    });
    write_frame(transport, OP_FRAME, &payload).await
}

fn nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

async fn write_frame<T: Transport + ?Sized>(
    transport: &mut T,
    op: u32,
    payload: &Value,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(payload)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut header = [0u8; 8];
    header[..4].copy_from_slice(&op.to_le_bytes());
    header[4..].copy_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_le_bytes());
    transport.write_all(&header).await?;
    transport.write_all(&body).await?;
    transport.flush().await
}

async fn read_frame<T: Transport + ?Sized>(transport: &mut T) -> std::io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    transport.read_exact(&mut header).await?;
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length > MAX_PAYLOAD {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "discord payload too large",
        ));
    }
    let mut body = vec![0u8; length];
    transport.read_exact(&mut body).await?;
    let payload = serde_json::from_slice(&body)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok((op, payload))
}

/// Anything that is a byte stream: `UnixStream` on Linux, `NamedPipeClient` on
/// Windows.
trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}

/// Candidate socket paths, in the order Discord itself probes them.
#[cfg(unix)]
fn candidate_sockets() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Ok(value) = std::env::var(variable)
            && !value.is_empty()
        {
            roots.push(PathBuf::from(value));
        }
    }
    roots.push(PathBuf::from("/tmp"));

    // Native clients, Flatpak and Snap each use their own runtime directory.
    let mut directories: Vec<PathBuf> = Vec::new();
    for root in &roots {
        directories.push(root.clone());
        directories.push(root.join("app/com.discordapp.Discord"));
        directories.push(root.join("app/com.discordapp.DiscordCanary"));
        directories.push(root.join("snap.discord"));
        directories.push(root.join(".flatpak/io.github.discord.Discord/xdg-run"));
    }

    let mut paths: Vec<PathBuf> = Vec::new();
    for directory in directories {
        for index in 0..10 {
            let candidate = directory.join(format!("discord-ipc-{index}"));
            if !paths.contains(&candidate) {
                paths.push(candidate);
            }
        }
    }
    paths
}

/// Windows exposes the same protocol through named pipes.
#[cfg(windows)]
fn candidate_sockets() -> Vec<PathBuf> {
    (0..10)
        .map(|index| PathBuf::from(format!(r"\\?\pipe\discord-ipc-{index}")))
        .collect()
}

#[cfg(unix)]
async fn open(path: &PathBuf) -> std::io::Result<Box<dyn Transport>> {
    let stream = tokio::net::UnixStream::connect(path).await?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
async fn open(path: &PathBuf) -> std::io::Result<Box<dyn Transport>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let client = ClientOptions::new().open(path)?;
    Ok(Box::new(client))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_payload_matches_the_discord_schema() {
        let activity = Activity {
            details: "Everything In Its Right Place".into(),
            state: "Kid A".into(),
            start_timestamp: Some(1_700_000_000_000),
            end_timestamp: Some(1_700_000_240_000),
            large_image: Some("https://example.test/cover.jpg".into()),
            large_text: Some("Kid A".into()),
            small_image: Some("prism".into()),
            small_text: Some("Bit-perfect".into()),
        };
        let json = activity.to_json();
        assert_eq!(json["type"], 2);
        assert_eq!(json["details"], "Everything In Its Right Place");
        assert_eq!(json["timestamps"]["start"], 1_700_000_000_000i64);
        assert_eq!(json["timestamps"]["end"], 1_700_000_240_000i64);
        assert_eq!(
            json["assets"]["large_image"],
            "https://example.test/cover.jpg"
        );
        assert_eq!(json["assets"]["small_text"], "Bit-perfect");
    }

    #[test]
    fn payload_falls_back_to_the_bundled_asset() {
        let activity = Activity {
            details: "A".into(),
            state: "B".into(),
            start_timestamp: None,
            end_timestamp: None,
            large_image: None,
            large_text: None,
            small_image: None,
            small_text: None,
        };
        let json = activity.to_json();
        assert_eq!(json["assets"]["large_image"], "prism");
        assert!(json.get("timestamps").is_none());
        // One-character fields are padded, never dropped.
        assert_eq!(json["details"], "A ");
    }

    #[test]
    fn control_characters_never_reach_the_payload() {
        let activity = Activity {
            details: "Title\nwith\u{0}breaks".into(),
            state: "Album".into(),
            start_timestamp: None,
            end_timestamp: None,
            large_image: None,
            large_text: None,
            small_image: None,
            small_text: None,
        };
        let details = activity.to_json()["details"].as_str().unwrap().to_string();
        assert!(!details.contains('\n'));
        assert!(!details.contains('\u{0}'));
    }

    #[tokio::test]
    async fn frames_round_trip_over_a_socket_pair() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut client: Box<dyn Transport> = Box::new(client);
        let writer = tokio::spawn(async move {
            write_frame(&mut client, OP_FRAME, &json!({"cmd": "SET_ACTIVITY"}))
                .await
                .expect("write");
        });
        let (op, payload) = read_frame(&mut server).await.expect("read");
        writer.await.expect("join");
        assert_eq!(op, OP_FRAME);
        assert_eq!(payload["cmd"], "SET_ACTIVITY");
    }

    #[test]
    fn backoff_is_bounded_and_jittered() {
        for _ in 0..32 {
            let delay = jitter(BACKOFF_START);
            assert!(delay >= BACKOFF_START);
            assert!(delay < BACKOFF_START + Duration::from_millis(500));
        }
    }

    #[test]
    fn socket_candidates_are_ordered_and_unique() {
        let paths = candidate_sockets();
        assert!(!paths.is_empty());
        let unique: std::collections::HashSet<_> = paths.iter().collect();
        assert_eq!(unique.len(), paths.len());
    }
}

#[cfg(all(test, unix))]
mod reconnect_tests {
    //! Offline behaviour, driven against a real Unix socket.
    //!
    //! The RPC worker is exercised end to end: it is pointed at a socket in a
    //! temporary directory that either does not exist (Discord closed), refuses
    //! the handshake (unknown client id) or answers `READY`. No mock and no
    //! sleeping for production delays — the tests inject a millisecond-scale
    //! [`BackoffPolicy`], which is exactly why the policy is a value.

    use std::path::Path;
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::time::{Instant, sleep, timeout};

    use super::*;

    const CLIENT: &str = "123456789012345678";

    /// Poll the handle until `predicate` holds, or fail after five seconds.
    async fn wait_for(
        rpc: &DiscordRpc,
        predicate: impl Fn(&DiscordStatus) -> bool,
    ) -> DiscordStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = rpc.status();
            if predicate(&status) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a status; last was {status:?}"
            );
            sleep(Duration::from_millis(2)).await;
        }
    }

    /// Read one frame, answering keepalives so the client never goes quiet.
    async fn read(stream: &mut UnixStream) -> (u32, Value) {
        loop {
            let (op, payload) = read_frame(stream).await.expect("frame");
            if op == OP_PING {
                write_frame(stream, OP_PONG, &payload).await.expect("pong");
                continue;
            }
            return (op, payload);
        }
    }

    /// Read frames until a `SET_ACTIVITY` arrives, and return it.
    async fn next_activity(stream: &mut UnixStream) -> Value {
        loop {
            let (op, payload) = read(stream).await;
            if op == OP_FRAME && payload["cmd"] == "SET_ACTIVITY" {
                return payload;
            }
        }
    }

    fn sample_activity() -> Activity {
        Activity {
            details: "Everything In Its Right Place".into(),
            state: "Kid A".into(),
            start_timestamp: Some(1_700_000_000_000),
            end_timestamp: None,
            large_image: None,
            large_text: Some("Kid A".into()),
            small_image: Some("prism".into()),
            small_text: Some("Bit-perfect".into()),
        }
    }

    #[tokio::test]
    async fn offline_discord_is_reported_and_retried_without_spinning() {
        let directory = tempfile::tempdir().expect("temp dir");
        let socket = directory.path().join("discord-ipc-0");
        let policy = BackoffPolicy::new(Duration::from_millis(5), Duration::from_millis(20));
        let started = Instant::now();
        let rpc = DiscordRpc::spawn_with(CLIENT.to_string(), vec![socket], policy);

        // With Discord closed, failures accumulate and each one names a reason.
        let status = wait_for(&rpc, |status| status.attempts >= 3).await;
        assert_eq!(status.state, DiscordState::Unavailable);
        let reason = status.last_error.expect("an error is reported");
        assert!(
            reason.contains("discord-ipc-0"),
            "the reason names the socket: {reason}"
        );

        // Three attempts cost at least the first two backoff sleeps (5 ms then
        // 7.5 ms): the loop waits between failures instead of spinning on the
        // socket, which is what keeps a closed Discord at 0 % CPU.
        assert!(
            started.elapsed() >= Duration::from_millis(12),
            "reconnects are rate limited, elapsed {:?}",
            started.elapsed()
        );

        rpc.stop();
        let stopped = wait_for(&rpc, |status| status.state == DiscordState::Stopped).await;
        assert_eq!(stopped.state, DiscordState::Stopped);
    }

    #[tokio::test]
    async fn a_rejected_client_id_falls_back_instead_of_failing_the_application() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bind");
        let policy = BackoffPolicy::new(Duration::from_millis(10), Duration::from_millis(40));
        let rpc = DiscordRpc::spawn_with(CLIENT.to_string(), vec![path], policy);

        let (mut first, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("connect")
            .expect("accept");
        let (op, handshake) = read(&mut first).await;
        assert_eq!(op, OP_HANDSHAKE);
        assert_eq!(handshake["v"], 1);
        assert_eq!(handshake["client_id"], CLIENT);
        write_frame(
            &mut first,
            OP_FRAME,
            &json!({"cmd": "DISPATCH", "evt": "ERROR", "data": {"code": 4000, "message": "Invalid Client ID"}}),
        )
        .await
        .expect("reject");
        drop(first);

        let status = wait_for(&rpc, |status| status.attempts >= 1).await;
        assert_eq!(status.state, DiscordState::Unavailable);
        let reason = status.last_error.unwrap_or_default();
        assert!(
            reason.contains("4000"),
            "the rejection is surfaced: {reason}"
        );

        // The rest of the application never sees an error: the handle is still
        // usable and can be stopped cleanly.
        rpc.set_activity(sample_activity());
        rpc.stop();
        wait_for(&rpc, |status| status.state == DiscordState::Stopped).await;
    }

    #[tokio::test]
    async fn reconnecting_flushes_the_pending_activity() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bind");
        let policy = BackoffPolicy::new(Duration::from_millis(10), Duration::from_millis(40));
        let rpc = DiscordRpc::spawn_with(CLIENT.to_string(), vec![path], policy);

        // Connection #1 is refused, as Discord does for an unregistered client.
        let (mut refused, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("connect")
            .expect("accept");
        let (op, _) = read(&mut refused).await;
        assert_eq!(op, OP_HANDSHAKE);
        write_frame(
            &mut refused,
            OP_FRAME,
            &json!({"cmd": "DISPATCH", "evt": "ERROR", "data": {"code": 4000}}),
        )
        .await
        .expect("reject");
        drop(refused);
        wait_for(&rpc, |status| status.attempts >= 1).await;

        // The user starts a track while Discord is unreachable.
        rpc.set_activity(sample_activity());

        // Connection #2 is healthy: the queued activity must go out immediately,
        // not on the next two-and-a-half-second tick.
        let (mut healthy, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("reconnect")
            .expect("accept");
        let (op, _) = read(&mut healthy).await;
        assert_eq!(op, OP_HANDSHAKE);
        write_frame(
            &mut healthy,
            OP_FRAME,
            &json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}}),
        )
        .await
        .expect("ready");

        let activity = timeout(Duration::from_secs(5), next_activity(&mut healthy))
            .await
            .expect("the pending activity is flushed");
        assert_eq!(
            activity["args"]["activity"]["details"],
            "Everything In Its Right Place"
        );
        assert_eq!(
            activity["args"]["activity"]["assets"]["small_text"],
            "Bit-perfect"
        );

        let status = wait_for(&rpc, DiscordStatus::is_active).await;
        assert_eq!(status.state, DiscordState::Connected);
        assert_eq!(status.attempts, 0, "a success resets the backoff");

        rpc.stop();
        wait_for(&rpc, |status| status.state == DiscordState::Stopped).await;
    }

    #[test]
    fn backoff_grows_geometrically_then_holds_at_the_ceiling() {
        let mut policy =
            BackoffPolicy::new(Duration::from_millis(100), Duration::from_millis(1_000));
        let mut delays = Vec::new();
        for _ in 0..10 {
            delays.push(policy.peek());
            let mut probe = policy.clone();
            let jittered = probe.next();
            assert!(jittered >= policy.peek(), "jitter only ever adds delay");
            assert!(
                jittered < policy.peek() + Duration::from_millis(500),
                "jitter stays inside its half-second window"
            );
            policy = probe;
        }

        assert_eq!(delays[0], Duration::from_millis(100));
        assert_eq!(delays[1], Duration::from_millis(150));
        assert_eq!(delays[2], Duration::from_millis(225));
        assert!(
            delays.windows(2).all(|pair| pair[1] >= pair[0]),
            "monotonic"
        );
        assert!(
            delays
                .iter()
                .all(|delay| *delay <= Duration::from_millis(1_000)),
            "never exceeds the ceiling"
        );
        assert_eq!(
            *delays.last().expect("delays"),
            Duration::from_millis(1_000),
            "reaches the ceiling"
        );

        policy.reset();
        assert_eq!(
            policy.peek(),
            Duration::from_millis(100),
            "reset restores the start"
        );
    }

    #[test]
    fn the_default_policy_waits_a_second_and_caps_at_a_minute() {
        let mut policy = BackoffPolicy::default();
        assert_eq!(policy.peek(), BACKOFF_START);
        for _ in 0..40 {
            let _ = policy.next();
        }
        assert_eq!(policy.peek(), BACKOFF_MAX);
    }

    #[tokio::test]
    async fn a_socket_that_does_not_exist_is_never_a_panic() {
        let missing = Path::new("/nonexistent/discord-ipc-0").to_path_buf();
        let error = connect_and_handshake(CLIENT, &[missing])
            .await
            .err()
            .expect("a missing socket is an error, never a panic");
        assert!(
            error.contains("discord-ipc-0"),
            "the path is named: {error}"
        );
    }
}
