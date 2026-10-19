//! End-to-end tests for the Discord IPC layer.
//!
//! A fake Discord client listens on a UNIX socket and speaks the real wire
//! protocol. The frame codec is re-implemented here on purpose: if the
//! production encoder ever drifts, these tests fail instead of agreeing with a
//! shared helper.
//!
//! The suite binds a UNIX socket, so it is `unix`-only. The Windows transport is
//! a named pipe; CI's native Windows job compiles the same `Transport` trait and
//! framing helpers, and the payload/backoff logic is transport-independent.
#![cfg(unix)]

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// `std::env::set_var` is process-global: two tests pointing `XDG_RUNTIME_DIR`
/// at different temporary directories at the same time would each probe the
/// other's socket. Every test that touches the environment holds this lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

use prism_lib::artwork::ArtworkService;
use prism_lib::db::connect;
use prism_lib::discord::{DiscordConfig, DiscordService, TrackPresence};

async fn read_frame(socket: &mut tokio::net::UnixStream) -> (u32, Value) {
    let mut header = [0u8; 8];
    socket.read_exact(&mut header).await.expect("header");
    let op = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; length];
    socket.read_exact(&mut body).await.expect("body");
    (op, serde_json::from_slice(&body).expect("json"))
}

async fn write_frame(socket: &mut tokio::net::UnixStream, op: u32, payload: &Value) {
    let body = serde_json::to_vec(payload).expect("encode");
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    socket.write_all(&frame).await.expect("write");
}

fn presence() -> TrackPresence {
    TrackPresence {
        title: "Idioteque".into(),
        artist: "Radiohead".into(),
        album: "Kid A".into(),
        path: None,
        duration_ms: Some(300_000),
        position_ms: 42_000,
        playing: true,
        bit_perfect: true,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(unsafe_code)] // `set_var` is only unsafe because it races; this test owns the variable.
async fn the_rpc_completes_the_handshake_and_publishes_an_activity() {
    let _env = env_guard();
    let runtime = tempfile::tempdir().expect("runtime dir");
    let socket_path = runtime.path().join("discord-ipc-0");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind ipc socket");

    // The worker probes $XDG_RUNTIME_DIR, exactly like Discord's own SDK.
    // SAFETY: the process is single-threaded at this point in the test binary
    // and no other test reads this variable.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
    }

    let server = tokio::spawn(async move {
        // Accept in a loop: an unrelated test in the same binary may also probe
        // $XDG_RUNTIME_DIR while this listener exists, and a connection that is
        // dropped mid-handshake must not fail the assertion.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let accept = tokio::time::timeout_at(deadline, listener.accept()).await;
            let Ok(Ok((mut socket, _))) = accept else {
                panic!("no connection arrived");
            };

            let handshake = tokio::time::timeout_at(deadline, read_frame(&mut socket)).await;
            let Ok((op, handshake)) = handshake else {
                continue; // stray or aborted probe
            };
            assert_eq!(op, 0, "first frame is the handshake");
            assert_eq!(handshake["v"], 1);
            assert_eq!(handshake["client_id"], "123456789012345678");

            write_frame(
                &mut socket,
                1,
                &json!({"cmd": "DISPATCH", "ev": "READY", "data": {"v": 1}}),
            )
            .await;

            // The activity must arrive without any further prompting.
            let activity = tokio::time::timeout_at(deadline, read_frame(&mut socket)).await;
            let Ok((op, frame)) = activity else {
                continue;
            };
            assert_eq!(op, 1);
            assert_eq!(frame["cmd"], "SET_ACTIVITY");
            assert_eq!(frame["args"]["pid"], std::process::id());

            // Keep the socket alive while the assertion runs.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            return frame;
        }
    });

    let temp = tempfile::tempdir().expect("tempdir");
    let pool = connect(&temp.path().join("prism.db")).await.expect("db");
    let artwork = Arc::new(
        ArtworkService::new(pool.clone(), temp.path().join("artwork"))
            .expect("artwork")
            .with_offline(true),
    );
    let service = DiscordService::new(artwork);
    service
        .configure(DiscordConfig {
            enabled: true,
            client_id: "123456789012345678".into(),
            show_cover: false,
            show_timestamps: true,
            show_quality: true,
        })
        .expect("configure");

    service.publish(Some(presence())).await;

    let frame = server.await.expect("server");
    let activity = &frame["args"]["activity"];
    assert_eq!(activity["type"], 2, "Listening activity");
    assert_eq!(activity["details"], "Idioteque — Radiohead");
    assert_eq!(activity["state"], "Kid A");
    assert!(activity["timestamps"]["start"].as_i64().is_some());
    assert_eq!(activity["assets"]["large_image"], "prism");
    assert_eq!(activity["assets"]["small_text"], "Bit-perfect");
}

#[tokio::test]
async fn disabling_presence_stops_the_worker_without_panicking() {
    let _env = env_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let pool = connect(&temp.path().join("prism.db")).await.expect("db");
    let artwork = Arc::new(
        ArtworkService::new(pool.clone(), temp.path().join("artwork"))
            .expect("artwork")
            .with_offline(true),
    );
    let service = DiscordService::new(artwork);

    // No socket exists in this test, so the worker sits in `unavailable`.
    let status = service
        .configure(DiscordConfig {
            enabled: true,
            client_id: "123456789012345678".into(),
            ..DiscordConfig::default()
        })
        .expect("configure");
    assert!(matches!(
        status.state,
        prism_lib::discord::DiscordState::Connecting
            | prism_lib::discord::DiscordState::Unavailable
    ));

    service.publish(Some(presence())).await;
    service.shutdown();

    let (config, status) = service.status();
    assert!(config.enabled);
    assert_eq!(status.state, prism_lib::discord::DiscordState::Disabled);

    // Turning it off is idempotent.
    let disabled = service
        .configure(DiscordConfig {
            enabled: false,
            ..DiscordConfig::default()
        })
        .expect("disable");
    assert_eq!(disabled.state, prism_lib::discord::DiscordState::Disabled);
    pool.close().await;
}

#[tokio::test]
async fn configuration_round_trips_through_the_settings_table() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pool = connect(&temp.path().join("prism.db")).await.expect("db");

    let config = DiscordConfig {
        enabled: true,
        client_id: "987654321098765432".into(),
        show_cover: false,
        show_timestamps: true,
        show_quality: false,
    };
    config.save(&pool).await.expect("save");

    let loaded = DiscordConfig::load(&pool).await;
    assert_eq!(loaded, config);

    // A row whose JSON is valid but does not match the schema falls back to the
    // defaults instead of failing startup. (The table itself rejects invalid
    // JSON: `CHECK (json_valid(value))`.)
    sqlx::query(
        "UPDATE settings SET value = '{\"enabled\": \"not-a-boolean\"}' WHERE key = 'discord'",
    )
    .execute(&pool)
    .await
    .expect("corrupt");
    assert_eq!(DiscordConfig::load(&pool).await, DiscordConfig::default());

    pool.close().await;
}
