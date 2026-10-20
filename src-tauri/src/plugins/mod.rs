//! Plugin runtime: discovery, sandboxing, configuration and dispatch.
//!
//! Prism ships no plugin *market*; it ships a contract. A plugin is a directory
//! under `<app-data>/plugins/` containing a `plugin.toml` and an entry file. Two
//! runtimes implement that contract, both **opt-in at compile time** so the
//! default binary carries neither interpreter:
//!
//! * `lua-plugins` — Lua 5.4 through `mlua` (vendored, no system Lua), standard
//!   library restricted to `table`/`string`/`math`/`utf8`/`coroutine`, 16 MiB
//!   memory ceiling and a per-call instruction budget.
//! * `wasm-plugins` — WebAssembly through `wasmtime`, fuel-metered, 64 MiB of
//!   linear memory and no WASI imports: a module can compute and call the host
//!   API, nothing else.
//!
//! Both runtimes see the same host surface ([`HostBridge`]), gated by the
//! capabilities the manifest declares. A plugin that asks for `player:control`
//! and does not declare it gets a permission error at call time — the same error
//! the UI shows, not a silent no-op.
//!
//! Failure policy: a plugin that errors during a hook is disabled for the rest
//! of the session and its error is surfaced in Settings. Playback never depends
//! on a plugin, so the failure stays contained.

pub mod manifest;
mod runtime;

#[cfg(feature = "lua-plugins")]
mod lua_rt;
#[cfg(feature = "wasm-plugins")]
mod wasm_rt;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::audio::engine::Engine;
use crate::error::PrismError;

use manifest::{Capability, Manifest, SettingSpec};

pub use runtime::Hook;

/// Event channel for plugin log lines and notifications.
pub const PLUGIN_MESSAGE_EVENT: &str = "plugin://message";

/// `settings` key holding the ids the user disabled.
const DISABLED_KEY: &str = "plugins.disabled";
/// Prefix for the per-plugin configuration object.
const CONFIG_PREFIX: &str = "plugin.config.";
/// Messages a plugin may emit per second before the host starts dropping them.
const MESSAGE_RATE_LIMIT: u32 = 40;
/// Lua memory ceiling per plugin instance.
pub const LUA_MEMORY_LIMIT: usize = 16 * 1024 * 1024;
/// Lua VM instructions a single hook call may execute.
pub const LUA_INSTRUCTION_BUDGET: u64 = 20_000_000;
/// WebAssembly fuel granted to a single hook call.
pub const WASM_FUEL_PER_CALL: u64 = 200_000_000;
/// WebAssembly linear-memory ceiling per plugin instance.
pub const WASM_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Everything a plugin can fail at, with a message the settings panel can show.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("invalid plugin manifest: {0}")]
    Manifest(String),
    #[error("plugin runtime error: {0}")]
    Runtime(String),
    #[error("plugin '{0}' is not allowed to do that")]
    Denied(String),
    #[error("unknown plugin '{0}'")]
    Unknown(String),
    #[error("plugin '{0}' is disabled")]
    Disabled(String),
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl PluginError {
    pub fn manifest(message: impl Into<String>) -> Self {
        Self::Manifest(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<PluginError> for PrismError {
    fn from(value: PluginError) -> Self {
        match value {
            PluginError::Io { path, source } => Self::io(path, source),
            PluginError::Runtime(message) => Self::Internal(format!("plugin runtime: {message}")),
            other => Self::invalid(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Host bridge
// ---------------------------------------------------------------------------

/// A message a plugin produced: a log line or a user-facing notification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMessage {
    pub plugin_id: String,
    /// `log` or `notification`.
    pub kind: &'static str,
    /// `debug`, `info`, `warn` or `error` (log lines only).
    pub level: &'static str,
    pub message: String,
    /// Notification topic, when the plugin supplied one.
    pub topic: Option<String>,
}

/// Playback operations a plugin may request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlOp {
    Pause,
    Resume,
    SetVolume,
}

impl ControlOp {
    /// Decode the integer ABI used by WebAssembly plugins.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Pause),
            1 => Some(Self::Resume),
            2 => Some(Self::SetVolume),
            _ => None,
        }
    }
}

/// The API every plugin sees, regardless of runtime.
///
/// Holds only what a host function needs: the declared capabilities, the live
/// configuration, the engine handle and the message sink. Runtimes call into
/// this; nothing here reaches the filesystem or the database.
pub struct HostBridge {
    pub plugin_id: String,
    capabilities: BTreeSet<Capability>,
    config: RwLock<BTreeMap<String, Value>>,
    engine: Arc<Engine>,
    messages: mpsc::UnboundedSender<PluginMessage>,
    rate: Mutex<RateWindow>,
}

#[derive(Debug)]
struct RateWindow {
    started: Instant,
    count: u32,
}

impl std::fmt::Debug for HostBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostBridge")
            .field("plugin_id", &self.plugin_id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl HostBridge {
    pub fn new(
        manifest: &Manifest,
        config: BTreeMap<String, Value>,
        engine: Arc<Engine>,
        messages: mpsc::UnboundedSender<PluginMessage>,
    ) -> Self {
        Self {
            plugin_id: manifest.id.clone(),
            capabilities: manifest.capabilities.iter().copied().collect(),
            config: RwLock::new(config),
            engine,
            messages,
            rate: Mutex::new(RateWindow {
                started: Instant::now(),
                count: 0,
            }),
        }
    }

    fn allows(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    fn require(&self, capability: Capability) -> Result<(), PluginError> {
        if self.allows(capability) {
            Ok(())
        } else {
            Err(PluginError::Denied(format!(
                "{} (declare '{}' in the manifest)",
                self.plugin_id,
                capability.as_str()
            )))
        }
    }

    fn permit_message(&self) -> bool {
        let mut window = self.rate.lock();
        if window.started.elapsed().as_secs() >= 1 {
            window.started = Instant::now();
            window.count = 0;
        }
        if window.count >= MESSAGE_RATE_LIMIT {
            return false;
        }
        window.count += 1;
        true
    }

    /// Append a log line to the plugin log stream.
    pub fn log(&self, level: &str, message: &str) -> Result<(), PluginError> {
        self.require(Capability::Log)?;
        if !self.permit_message() {
            return Ok(());
        }
        let level = normalise_level(level);
        let message = truncate(message, 2000);
        let _ = self.messages.send(PluginMessage {
            plugin_id: self.plugin_id.clone(),
            kind: "log",
            level,
            message,
            topic: None,
        });
        Ok(())
    }

    /// Publish a notification the UI can display.
    pub fn notify(&self, topic: Option<String>, message: &str) -> Result<(), PluginError> {
        self.require(Capability::Notify)?;
        if !self.permit_message() {
            return Ok(());
        }
        let _ = self.messages.send(PluginMessage {
            plugin_id: self.plugin_id.clone(),
            kind: "notification",
            level: "info",
            message: truncate(message, 500),
            topic: topic.map(|value| truncate(&value, 64)),
        });
        Ok(())
    }

    /// Serialised engine status. Read-only; the plugin gets exactly what the
    /// player bar shows.
    pub fn now_playing(&self) -> Result<String, PluginError> {
        self.require(Capability::PlayerRead)?;
        serde_json::to_string(&self.engine.status())
            .map_err(|error| PluginError::runtime(format!("cannot serialise status: {error}")))
    }

    /// One configured value, for runtimes whose host API exposes typed getters.
    #[must_use]
    pub fn config_value(&self, key: &str) -> Option<Value> {
        self.config.read().get(key).cloned()
    }

    /// Current configuration as a JSON object.
    #[must_use]
    pub fn config_json(&self) -> String {
        let config = self.config.read();
        serde_json::to_string(&*config).unwrap_or_else(|_| "{}".to_string())
    }

    /// Replace the live configuration after the user edits it.
    pub fn set_config(&self, config: BTreeMap<String, Value>) {
        *self.config.write() = config;
    }

    /// Apply a playback operation.
    pub fn control(&self, op: ControlOp, value: Option<f64>) -> Result<(), PluginError> {
        self.require(Capability::PlayerControl)?;
        match op {
            ControlOp::Pause => self
                .engine
                .pause()
                .map_err(|error| PluginError::runtime(error.to_string())),
            ControlOp::Resume => self
                .engine
                .resume()
                .map_err(|error| PluginError::runtime(error.to_string())),
            ControlOp::SetVolume => {
                let Some(db) = value else {
                    return Err(PluginError::runtime("set_volume requires a value"));
                };
                if !db.is_finite() || !(-60.0..=0.0).contains(&db) {
                    return Err(PluginError::runtime(
                        "volume must be within -60.0 ..= 0.0 dB",
                    ));
                }
                self.engine.set_volume_db(db as f32);
                Ok(())
            }
        }
    }
}

fn normalise_level(level: &str) -> &'static str {
    match level.to_ascii_lowercase().as_str() {
        "debug" => "debug",
        "warn" | "warning" => "warn",
        "error" => "error",
        _ => "info",
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let clipped: String = value.chars().take(max).collect();
    format!("{clipped}…")
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// An event forwarded to `on_event`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEvent {
    /// `track_changed`, `state_changed`, `device_changed`, `output_error`.
    pub kind: &'static str,
    pub payload: Value,
}

impl PluginEvent {
    #[must_use]
    pub fn new(kind: &'static str, payload: Value) -> Self {
        Self { kind, payload }
    }

    fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ---------------------------------------------------------------------------
// Descriptors for the configuration UI
// ---------------------------------------------------------------------------

/// One setting plus its current value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetting {
    #[serde(flatten)]
    pub spec: SettingSpec,
    pub value: Value,
}

/// Everything the Settings → Plugins panel needs about one plugin.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    /// `lua`, `wasm` or `unknown` when the manifest could not be read.
    pub runtime: String,
    pub capabilities: Vec<String>,
    pub settings: Vec<PluginSetting>,
    pub enabled: bool,
    pub loaded: bool,
    /// Present when the plugin failed to load; the panel shows it verbatim.
    pub error: Option<String>,
    pub path: String,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Entry {
    dir: PathBuf,
    manifest: Option<Manifest>,
    config: BTreeMap<String, Value>,
    runtime: Option<runtime::Runtime>,
    bridge: Option<Arc<HostBridge>>,
    error: Option<String>,
}

impl Entry {
    fn descriptor(&self, disabled: bool) -> PluginDescriptor {
        match &self.manifest {
            Some(manifest) => PluginDescriptor {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                api: manifest.api,
                description: manifest.description.clone(),
                author: manifest.author.clone(),
                homepage: manifest.homepage.clone(),
                runtime: manifest.runtime.as_str().to_string(),
                capabilities: manifest
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_string())
                    .collect(),
                settings: manifest
                    .settings
                    .iter()
                    .map(|spec| PluginSetting {
                        value: self
                            .config
                            .get(&spec.key)
                            .cloned()
                            .unwrap_or_else(|| spec.default.clone()),
                        spec: spec.clone(),
                    })
                    .collect(),
                enabled: !disabled,
                loaded: self.runtime.is_some(),
                error: self.error.clone(),
                path: self.dir.to_string_lossy().into_owned(),
            },
            None => PluginDescriptor {
                id: self.dir.file_name().map_or_else(
                    || "unknown".to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                name: self.dir.file_name().map_or_else(
                    || "Unknown plugin".to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                version: "—".to_string(),
                api: 0,
                description: String::new(),
                author: String::new(),
                homepage: None,
                runtime: "unknown".to_string(),
                capabilities: Vec::new(),
                settings: Vec::new(),
                enabled: false,
                loaded: false,
                error: self.error.clone(),
                path: self.dir.to_string_lossy().into_owned(),
            },
        }
    }
}

#[derive(Debug, Default)]
struct Registry {
    entries: BTreeMap<String, Entry>,
    disabled: BTreeSet<String>,
}

/// Owns every loaded plugin and the configuration that survives restarts.
#[derive(Debug)]
pub struct PluginManager {
    dir: PathBuf,
    pool: SqlitePool,
    engine: Arc<Engine>,
    messages: mpsc::UnboundedSender<PluginMessage>,
    registry: tokio::sync::Mutex<Registry>,
}

impl PluginManager {
    /// Create the plugin directory if needed and load what is already there.
    pub async fn bootstrap(
        dir: PathBuf,
        pool: SqlitePool,
        engine: Arc<Engine>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PluginMessage>), PrismError> {
        std::fs::create_dir_all(&dir).map_err(|error| PrismError::io(&dir, error))?;
        let (messages, receiver) = mpsc::unbounded_channel();
        let manager = Self {
            dir,
            pool,
            engine,
            messages,
            registry: tokio::sync::Mutex::new(Registry::default()),
        };
        manager.reload().await?;
        Ok((manager, receiver))
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// Rescan the plugin directory.
    ///
    /// Entries keep their configuration across a reload; a plugin that vanished
    /// from disk disappears from the list, and its stored config is left alone
    /// so reinstalling it restores the settings.
    pub async fn reload(&self) -> Result<(), PrismError> {
        let disabled = self.load_disabled().await;
        let discovered = discover(&self.dir);

        let mut registry = self.registry.lock().await;
        registry.disabled = disabled;
        let mut entries = BTreeMap::new();

        for folder in discovered {
            let key = folder.file_name().map_or_else(
                || folder.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            );

            match Manifest::load(&folder) {
                Ok(manifest) => {
                    let stored = self.load_config(&manifest.id).await;
                    let config = manifest.reconcile(&stored);
                    let mut entry = Entry {
                        dir: folder,
                        manifest: Some(manifest),
                        config,
                        runtime: None,
                        bridge: None,
                        error: None,
                    };
                    let is_disabled = registry.disabled.contains(
                        &entry
                            .manifest
                            .as_ref()
                            .map(|m| m.id.clone())
                            .unwrap_or_default(),
                    );
                    if !is_disabled {
                        self.instantiate(&mut entry);
                    }
                    entries.insert(key, entry);
                }
                Err(error) => {
                    let message = error.to_string();
                    tracing::warn!(plugin = %key, %message, "plugin rejected");
                    entries.insert(
                        key,
                        Entry {
                            dir: folder,
                            manifest: None,
                            config: BTreeMap::new(),
                            runtime: None,
                            bridge: None,
                            error: Some(message),
                        },
                    );
                }
            }
        }

        registry.entries = entries;
        Ok(())
    }

    /// Build the runtime for an enabled entry, recording any failure on it.
    fn instantiate(&self, entry: &mut Entry) {
        let Some(manifest) = &entry.manifest else {
            return;
        };
        let bridge = Arc::new(HostBridge::new(
            manifest,
            entry.config.clone(),
            Arc::clone(&self.engine),
            self.messages.clone(),
        ));
        match runtime::Runtime::spawn(manifest, &entry.dir, Arc::clone(&bridge)) {
            Ok(mut runtime) => {
                if let Err(error) = runtime.on_load(&bridge.config_json()) {
                    entry.error = Some(error.to_string());
                    tracing::warn!(plugin = %manifest.id, %error, "plugin failed on_load");
                    return;
                }
                entry.runtime = Some(runtime);
                entry.bridge = Some(bridge);
                entry.error = None;
                tracing::debug!(plugin = %manifest.id, runtime = manifest.runtime.as_str(), "plugin loaded");
            }
            Err(error) => {
                entry.error = Some(error.to_string());
                tracing::warn!(plugin = %manifest.id, %error, "plugin could not be instantiated");
            }
        }
    }

    pub async fn descriptors(&self) -> Vec<PluginDescriptor> {
        let registry = self.registry.lock().await;
        let mut list: Vec<PluginDescriptor> = registry
            .entries
            .values()
            .map(|entry| {
                let disabled = entry
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| registry.disabled.contains(&manifest.id));
                entry.descriptor(disabled)
            })
            .collect();
        list.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        list
    }

    /// Enable or disable a plugin, persisting the choice.
    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<PluginDescriptor, PrismError> {
        let mut registry = self.registry.lock().await;
        let key = registry
            .entries
            .iter()
            .find_map(|(key, entry)| {
                entry
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.id == id)
                    .then(|| key.clone())
            })
            .ok_or_else(|| PrismError::invalid(format!("unknown plugin '{id}'")))?;

        if enabled {
            registry.disabled.remove(id);
        } else {
            registry.disabled.insert(id.to_string());
        }
        let disabled: Vec<String> = registry.disabled.iter().cloned().collect();
        drop(registry);

        self.store_json(DISABLED_KEY, &serde_json::json!(disabled))
            .await?;

        let mut registry = self.registry.lock().await;
        if let Some(entry) = registry.entries.get_mut(&key) {
            if enabled {
                if entry.runtime.is_none() {
                    self.instantiate(entry);
                }
            } else {
                entry.runtime = None;
                entry.bridge = None;
            }
            let descriptor = entry.descriptor(!enabled);
            tracing::info!(plugin = %id, enabled, "plugin toggled");
            Ok(descriptor)
        } else {
            Err(PrismError::invalid(format!("unknown plugin '{id}'")))
        }
    }

    /// Replace one configuration value, validating it against the schema.
    ///
    /// The stored value is the coerced one: a `number` setting always comes back
    /// as a JSON number, never as the string the UI happened to send.
    pub async fn set_config(
        &self,
        id: &str,
        key_name: &str,
        value: Value,
    ) -> Result<PluginDescriptor, PrismError> {
        let (config_snapshot, callback) = {
            let mut registry = self.registry.lock().await;
            let entry = registry
                .entries
                .values_mut()
                .find(|entry| {
                    entry
                        .manifest
                        .as_ref()
                        .is_some_and(|manifest| manifest.id == id)
                })
                .ok_or_else(|| PrismError::invalid(format!("unknown plugin '{id}'")))?;

            let Some(manifest) = entry.manifest.clone() else {
                return Err(PrismError::invalid(format!(
                    "plugin '{id}' has no valid manifest"
                )));
            };
            let Some(spec) = manifest.settings.iter().find(|spec| spec.key == key_name) else {
                return Err(PrismError::invalid(format!(
                    "plugin '{id}' has no setting '{key_name}'"
                )));
            };
            let coerced = spec.coerce(&value)?;
            entry.config.insert(key_name.to_string(), coerced);

            if let Some(bridge) = &entry.bridge {
                bridge.set_config(entry.config.clone());
            }

            let snapshot = entry.config.clone();
            let payload = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
            // `on_config` runs while the registry lock is held: it may call the
            // host API (read-only or engine control), never the manager.
            let result = entry
                .runtime
                .as_mut()
                .map(|runtime| runtime.on_config(&payload));
            (snapshot, result)
        };

        self.store_json(&format!("{CONFIG_PREFIX}{id}"), &config_snapshot)
            .await?;

        if let Some(Err(error)) = callback {
            self.mark_failed(id, error.to_string()).await?;
        }

        let registry = self.registry.lock().await;
        registry
            .entries
            .values()
            .find(|entry| {
                entry
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.id == id)
            })
            .map(|entry| entry.descriptor(registry.disabled.contains(id)))
            .ok_or_else(|| PrismError::invalid(format!("unknown plugin '{id}'")))
    }

    /// Record a runtime failure and stop calling the plugin this session.
    async fn mark_failed(&self, id: &str, message: String) -> Result<(), PrismError> {
        let mut registry = self.registry.lock().await;
        if let Some(entry) = registry.entries.values_mut().find(|entry| {
            entry
                .manifest
                .as_ref()
                .is_some_and(|manifest| manifest.id == id)
        }) {
            entry.runtime = None;
            entry.bridge = None;
            entry.error = Some(message.clone());
        }
        tracing::warn!(plugin = %id, %message, "plugin disabled after a runtime error");
        drop(registry);
        Ok(())
    }

    /// Forward a playback event to every enabled plugin.
    ///
    /// Each call is budgeted by the sandbox, so this runs inline: the total
    /// worst case is bounded by the number of installed plugins times the fuel
    /// budget, and a plugin cannot stall the event loop for longer than that.
    pub async fn dispatch(&self, event: &PluginEvent) {
        let payload = event.encode();
        let mut registry = self.registry.lock().await;
        let mut failures: Vec<(String, String)> = Vec::new();

        for entry in registry.entries.values_mut() {
            let Some(runtime) = entry.runtime.as_mut() else {
                continue;
            };
            if let Err(error) = runtime.on_event(&payload) {
                if let Some(manifest) = &entry.manifest {
                    failures.push((manifest.id.clone(), error.to_string()));
                }
            }
        }
        drop(registry);

        for (id, message) in failures {
            if let Err(error) = self.mark_failed(&id, message).await {
                tracing::warn!(%error, "cannot record plugin failure");
            }
        }
    }

    /// Call `on_config` on a plugin after the host changed its settings.
    pub async fn notify_config(&self, id: &str) -> Result<(), PrismError> {
        let mut registry = self.registry.lock().await;
        let Some(entry) = registry.entries.values_mut().find(|entry| {
            entry
                .manifest
                .as_ref()
                .is_some_and(|manifest| manifest.id == id)
        }) else {
            return Err(PrismError::invalid(format!("unknown plugin '{id}'")));
        };
        let payload = serde_json::to_string(&entry.config).unwrap_or_else(|_| "{}".to_string());
        if let Some(runtime) = entry.runtime.as_mut() {
            runtime.on_config(&payload)?;
        }
        Ok(())
    }

    async fn load_disabled(&self) -> BTreeSet<String> {
        let stored: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
                .bind(DISABLED_KEY)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        stored
            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
            .map(|ids| ids.into_iter().collect())
            .unwrap_or_default()
    }

    async fn load_config(&self, id: &str) -> serde_json::Map<String, Value> {
        let key = format!("{CONFIG_PREFIX}{id}");
        let stored: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        stored
            .and_then(|json| serde_json::from_str::<serde_json::Map<String, Value>>(&json).ok())
            .unwrap_or_default()
    }

    async fn store_json(&self, key: &str, value: &impl Serialize) -> Result<(), PrismError> {
        let encoded = serde_json::to_string(value).map_err(|error| {
            PrismError::Internal(format!("cannot encode plugin state: {error}"))
        })?;
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(key)
        .bind(encoded)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Plugin directories, newest-compatible first.
fn discover(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(manifest::MANIFEST_FILENAME).is_file())
        .collect();
    folders.sort();
    folders
}

/// Feature availability reported to the UI so Settings can explain a missing
/// runtime instead of showing an empty list.
#[must_use]
pub fn runtime_support() -> Value {
    serde_json::json!({
        "lua": cfg!(feature = "lua-plugins"),
        "wasm": cfg!(feature = "wasm-plugins"),
        "api": manifest::SUPPORTED_API,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_messages_are_normalised() {
        assert_eq!(normalise_level("WARN"), "warn");
        assert_eq!(normalise_level("warning"), "warn");
        assert_eq!(normalise_level("verbose"), "info");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn control_codes_are_stable() {
        assert_eq!(ControlOp::from_code(0), Some(ControlOp::Pause));
        assert_eq!(ControlOp::from_code(1), Some(ControlOp::Resume));
        assert_eq!(ControlOp::from_code(2), Some(ControlOp::SetVolume));
        assert_eq!(ControlOp::from_code(9), None);
    }

    #[test]
    fn discovery_ignores_files_and_directories_without_a_manifest() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(root.path().join("not-a-plugin")).expect("dir");
        std::fs::write(root.path().join("loose-file.txt"), "x").expect("file");
        let valid = root.path().join("valid");
        std::fs::create_dir(&valid).expect("dir");
        std::fs::write(valid.join(manifest::MANIFEST_FILENAME), "x").expect("manifest");

        let found = discover(root.path());
        assert_eq!(found, vec![valid]);
    }

    #[test]
    fn runtime_support_reflects_the_feature_flags() {
        let support = runtime_support();
        assert_eq!(
            support["lua"],
            serde_json::json!(cfg!(feature = "lua-plugins"))
        );
        assert_eq!(
            support["wasm"],
            serde_json::json!(cfg!(feature = "wasm-plugins"))
        );
        assert_eq!(support["api"], serde_json::json!(1));
    }
}
